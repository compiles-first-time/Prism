//! Governance rule versioning: publication, conflict detection, rollback, export.
//!
//! - **SR_GOV_19**: Publish a new ruleset version with grammar validation,
//!   dry-run against recent decisions, and atomic promotion.
//! - **SR_GOV_19_BE-01**: Block promotion when >5% of decisions change.
//! - **SR_GOV_20**: Detect conflicts (contradiction, subsumption, overlap)
//!   between rules within a ruleset.
//!
//! Implements: SR_GOV_19, SR_GOV_19_BE-01, SR_GOV_20

use std::sync::Arc;

use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::{DecisionSampleRepository, RulesetVersionRepository};
use prism_core::types::*;

/// Threshold percentage of decision changes above which promotion is blocked.
/// Per SR_GOV_19_BE-01: >5% triggers blocking.
const DELTA_THRESHOLD_PERCENT: f64 = 5.0;

// ---------------------------------------------------------------------------
// RulePublicationService (SR_GOV_19)
// ---------------------------------------------------------------------------

/// Service for publishing new governance rule versions.
///
/// Composes:
/// - `RulesetVersionRepository` -- versioned ruleset persistence
/// - `DecisionSampleRepository` -- recent decisions for dry-run
/// - `AuditLogger` -- audit trail
///
/// Implements: SR_GOV_19, SR_GOV_19_BE-01
pub struct RulePublicationService {
    versions: Arc<dyn RulesetVersionRepository>,
    decisions: Arc<dyn DecisionSampleRepository>,
    audit: AuditLogger,
}

impl RulePublicationService {
    /// Create a new rule publication service.
    pub fn new(
        versions: Arc<dyn RulesetVersionRepository>,
        decisions: Arc<dyn DecisionSampleRepository>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            versions,
            decisions,
            audit,
        }
    }

    /// Publish a new ruleset version.
    ///
    /// Workflow:
    /// 1. Validate rule grammar (all rules must have non-empty names, valid
    ///    action patterns, and parseable condition JSON).
    /// 2. Run conflict detection (SR_GOV_20); HIGH conflicts block promotion.
    /// 3. Dry-run against recent decisions to compute the delta.
    /// 4. If delta >5%, block promotion (SR_GOV_19_BE-01).
    /// 5. If all checks pass, atomically promote the new version.
    ///
    /// Implements: SR_GOV_19
    pub async fn publish(
        &self,
        request: &RulePublishRequest,
    ) -> Result<RulePublishResult, PrismError> {
        // Step 1: validate rule grammar
        Self::validate_rules(&request.rules)?;

        // Step 2: run conflict detection (SR_GOV_20)
        let conflict_report = ConflictDetector::scan(&request.rules);
        if conflict_report.blocks_promotion {
            warn!(
                tenant_id = %request.tenant_id,
                conflicts = conflict_report.conflicts.len(),
                "rule publication blocked by HIGH-severity conflicts"
            );

            let version_id = uuid::Uuid::new_v4();
            return Ok(RulePublishResult {
                version_id,
                dry_run_report: DryRunReport {
                    sample_size: 0,
                    decisions_changed: 0,
                    delta_percentage: 0.0,
                    exceeds_threshold: false,
                    rule_deltas: vec![],
                },
                promoted: false,
            });
        }

        // Step 3: dry-run against recent decisions
        let sample_size = request.dry_run_sample_size.unwrap_or(100);
        let recent_decisions = self
            .decisions
            .get_recent_decisions(request.tenant_id, sample_size)
            .await?;

        let dry_run_report = self.compute_dry_run(&request.rules, &recent_decisions);

        // Step 4: check delta threshold (SR_GOV_19_BE-01)
        let can_promote = !dry_run_report.exceeds_threshold;

        // Step 5: create version and conditionally promote
        let current = self.versions.get_active(request.tenant_id).await?;
        let next_version_number = current.map_or(1, |v| v.version_number + 1);
        let version_id = uuid::Uuid::new_v4();

        let version = RulesetVersion {
            id: version_id,
            tenant_id: request.tenant_id,
            rules: request.rules.clone(),
            change_description: request.change_description.clone(),
            is_active: can_promote,
            version_number: next_version_number,
            created_at: chrono::Utc::now(),
        };

        self.versions.create(&version).await?;

        if can_promote {
            self.versions.promote(request.tenant_id, version_id).await?;
        }

        // Audit trail
        let event_type = if can_promote {
            "governance.rule_published"
        } else {
            "governance.rule_publication_blocked"
        };

        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: event_type.into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::Human,
                target_id: Some(version_id),
                target_type: Some("RulesetVersion".into()),
                severity: if can_promote {
                    Severity::Medium
                } else {
                    Severity::High
                },
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "version_number": next_version_number,
                    "change_description": request.change_description,
                    "promoted": can_promote,
                    "delta_percentage": dry_run_report.delta_percentage,
                    "sample_size": dry_run_report.sample_size,
                    "conflict_count": conflict_report.conflicts.len(),
                }),
            })
            .await?;

        info!(
            tenant_id = %request.tenant_id,
            version_id = %version_id,
            version_number = next_version_number,
            promoted = can_promote,
            delta = dry_run_report.delta_percentage,
            "rule publication {}",
            if can_promote { "promoted" } else { "blocked by delta threshold" }
        );

        Ok(RulePublishResult {
            version_id,
            dry_run_report,
            promoted: can_promote,
        })
    }

    /// Validate all rules in a proposed ruleset.
    fn validate_rules(rules: &[GovernanceRule]) -> Result<(), PrismError> {
        if rules.is_empty() {
            return Err(PrismError::Validation {
                reason: "ruleset must contain at least one rule".into(),
            });
        }

        for rule in rules {
            if rule.name.trim().is_empty() {
                return Err(PrismError::Validation {
                    reason: "rule name cannot be empty".into(),
                });
            }

            if rule.action_pattern.trim().is_empty() {
                return Err(PrismError::Validation {
                    reason: format!("rule '{}' has an empty action pattern", rule.name),
                });
            }

            if !rule.condition.is_object() {
                return Err(PrismError::Validation {
                    reason: format!("rule '{}' condition must be a JSON object", rule.name),
                });
            }
        }

        Ok(())
    }

    /// Compute dry-run impact by re-evaluating recent decisions against proposed rules.
    ///
    /// Implements: SR_GOV_19 (dry-run analysis)
    fn compute_dry_run(
        &self,
        rules: &[GovernanceRule],
        recent_decisions: &[(String, serde_json::Value, String)],
    ) -> DryRunReport {
        if recent_decisions.is_empty() {
            return DryRunReport {
                sample_size: 0,
                decisions_changed: 0,
                delta_percentage: 0.0,
                exceeds_threshold: false,
                rule_deltas: vec![],
            };
        }

        let mut decisions_changed = 0;
        let mut rule_delta_map: std::collections::HashMap<String, (usize, usize)> =
            std::collections::HashMap::new();

        for (action, attributes, prev_decision) in recent_decisions {
            // Find matching rules for this action/attributes
            let matching: Vec<&GovernanceRule> = rules
                .iter()
                .filter(|r| {
                    (r.action_pattern == *action || r.action_pattern == "*")
                        && r.is_active
                        && Self::condition_matches(&r.condition, attributes)
                })
                .collect();

            // Compute new decision
            let has_enforce_deny = matching.iter().any(|r| r.rule_class == RuleClass::Enforce);
            let new_decision = if has_enforce_deny { "deny" } else { "allow" };

            if new_decision != prev_decision {
                decisions_changed += 1;

                for rule in &matching {
                    let entry = rule_delta_map.entry(rule.name.clone()).or_insert((0, 0));
                    if new_decision == "deny" {
                        entry.0 += 1; // new denial
                    } else {
                        entry.1 += 1; // new allowance
                    }
                }
            }
        }

        let sample_size = recent_decisions.len();
        let delta_percentage = if sample_size > 0 {
            (decisions_changed as f64 / sample_size as f64) * 100.0
        } else {
            0.0
        };

        let rule_deltas: Vec<RuleDelta> = rule_delta_map
            .into_iter()
            .map(|(name, (denials, allowances))| RuleDelta {
                rule_name: name,
                new_denials: denials,
                new_allowances: allowances,
            })
            .collect();

        DryRunReport {
            sample_size,
            decisions_changed,
            delta_percentage,
            exceeds_threshold: delta_percentage > DELTA_THRESHOLD_PERCENT,
            rule_deltas,
        }
    }

    /// Simple attribute-matching condition evaluator (same as RuleEngine).
    fn condition_matches(condition: &serde_json::Value, attributes: &serde_json::Value) -> bool {
        let cond_obj = match condition.as_object() {
            Some(obj) => obj,
            None => return false,
        };
        let attr_obj = match attributes.as_object() {
            Some(obj) => obj,
            None => return false,
        };
        for (key, expected) in cond_obj {
            if key == "requires_justification" {
                continue;
            }
            match attr_obj.get(key) {
                Some(actual) if actual == expected => continue,
                _ => return false,
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// ConflictDetector (SR_GOV_20)
// ---------------------------------------------------------------------------

/// Detects conflicts between governance rules within a single ruleset.
///
/// Conflict types:
/// - **Contradiction**: two rules match the same action/attributes but produce
///   opposite decisions (e.g., one ENFORCE DENY, one no-match ALLOW).
/// - **Subsumption**: one rule's conditions are a strict subset of another's,
///   making the broader rule redundant.
/// - **Overlap**: two rules partially overlap in their condition space.
///
/// Implements: SR_GOV_20
pub struct ConflictDetector;

impl ConflictDetector {
    /// Scan a ruleset for internal conflicts.
    ///
    /// Compares each pair of rules sharing the same action pattern (or wildcard)
    /// for contradictions, subsumption, and overlaps.
    ///
    /// Implements: SR_GOV_20
    pub fn scan(rules: &[GovernanceRule]) -> RuleConflictReport {
        let mut conflicts = Vec::new();

        for i in 0..rules.len() {
            for j in (i + 1)..rules.len() {
                let rule_a = &rules[i];
                let rule_b = &rules[j];

                // Only compare rules that could match the same actions
                if !Self::actions_overlap(&rule_a.action_pattern, &rule_b.action_pattern) {
                    continue;
                }

                // Check for contradiction: different rule classes with overlapping conditions
                if rule_a.rule_class != rule_b.rule_class
                    && Self::conditions_overlap(&rule_a.condition, &rule_b.condition)
                {
                    conflicts.push(RuleConflict {
                        rule_a: rule_a.name.clone(),
                        rule_b: rule_b.name.clone(),
                        conflict_type: ConflictType::Contradiction,
                        description: format!(
                            "rules '{}' ({:?}) and '{}' ({:?}) match the same action '{}' with overlapping conditions but different rule classes",
                            rule_a.name, rule_a.rule_class,
                            rule_b.name, rule_b.rule_class,
                            rule_a.action_pattern
                        ),
                    });
                }

                // Check for subsumption: same class, one condition is subset of other
                if rule_a.rule_class == rule_b.rule_class {
                    if let Some(conflict_type) =
                        Self::check_subsumption(&rule_a.condition, &rule_b.condition)
                    {
                        let description = match conflict_type {
                            ConflictType::Subsumption => format!(
                                "rule '{}' conditions are a subset of '{}' -- the broader rule is redundant",
                                rule_a.name, rule_b.name
                            ),
                            ConflictType::Overlap => format!(
                                "rules '{}' and '{}' have partially overlapping conditions on action '{}'",
                                rule_a.name, rule_b.name, rule_a.action_pattern
                            ),
                            _ => String::new(),
                        };

                        if !description.is_empty() {
                            conflicts.push(RuleConflict {
                                rule_a: rule_a.name.clone(),
                                rule_b: rule_b.name.clone(),
                                conflict_type,
                                description,
                            });
                        }
                    }
                }
            }
        }

        let has_contradiction = conflicts
            .iter()
            .any(|c| c.conflict_type == ConflictType::Contradiction);

        let severity = if has_contradiction {
            Severity::High
        } else if conflicts.is_empty() {
            Severity::Low
        } else {
            Severity::Medium
        };

        RuleConflictReport {
            blocks_promotion: has_contradiction,
            severity,
            conflicts,
        }
    }

    /// Check if two action patterns could match the same action.
    fn actions_overlap(a: &str, b: &str) -> bool {
        a == b || a == "*" || b == "*"
    }

    /// Check if two condition objects have overlapping key-value pairs.
    fn conditions_overlap(a: &serde_json::Value, b: &serde_json::Value) -> bool {
        let a_obj = match a.as_object() {
            Some(obj) => obj,
            None => return false,
        };
        let b_obj = match b.as_object() {
            Some(obj) => obj,
            None => return false,
        };

        // Conditions overlap if they share at least one key with the same value
        for (key, val_a) in a_obj {
            if key == "requires_justification" {
                continue;
            }
            if let Some(val_b) = b_obj.get(key) {
                if val_a == val_b {
                    return true;
                }
            }
        }
        false
    }

    /// Check if one condition set subsumes or overlaps with another.
    /// Returns Some(Subsumption) if a ⊂ b, Some(Overlap) if they share keys, None otherwise.
    fn check_subsumption(a: &serde_json::Value, b: &serde_json::Value) -> Option<ConflictType> {
        let a_obj = a.as_object()?;
        let b_obj = b.as_object()?;

        // Filter out meta-fields
        let a_conds: Vec<(&String, &serde_json::Value)> = a_obj
            .iter()
            .filter(|(k, _)| k.as_str() != "requires_justification")
            .collect();
        let b_conds: Vec<(&String, &serde_json::Value)> = b_obj
            .iter()
            .filter(|(k, _)| k.as_str() != "requires_justification")
            .collect();

        if a_conds.is_empty() || b_conds.is_empty() {
            return None;
        }

        // Check if a is a subset of b (every condition in a appears in b with the same value)
        let a_subset_of_b = a_conds
            .iter()
            .all(|(k, v)| b_conds.iter().any(|(bk, bv)| bk == k && bv == v));

        if a_subset_of_b && a_conds.len() < b_conds.len() {
            return Some(ConflictType::Subsumption);
        }

        // Check for partial overlap
        let shared_keys = a_conds
            .iter()
            .filter(|(k, v)| b_conds.iter().any(|(bk, bv)| bk == k && bv == v))
            .count();

        if shared_keys > 0 && shared_keys < a_conds.len().min(b_conds.len()) {
            return Some(ConflictType::Overlap);
        }

        None
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use prism_core::repository::AuditEventRepository;
    use std::sync::Mutex;

    // -- Mock RulesetVersionRepository ----------------------------------------

    struct MockVersionRepo {
        versions: Mutex<Vec<RulesetVersion>>,
    }

    impl MockVersionRepo {
        fn new() -> Self {
            Self {
                versions: Mutex::new(Vec::new()),
            }
        }

        fn version_count(&self) -> usize {
            self.versions.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl RulesetVersionRepository for MockVersionRepo {
        async fn create(&self, version: &RulesetVersion) -> Result<(), PrismError> {
            self.versions.lock().unwrap().push(version.clone());
            Ok(())
        }

        async fn get_active(
            &self,
            tenant_id: TenantId,
        ) -> Result<Option<RulesetVersion>, PrismError> {
            let versions = self.versions.lock().unwrap();
            Ok(versions
                .iter()
                .filter(|v| v.tenant_id == tenant_id && v.is_active)
                .max_by_key(|v| v.version_number)
                .cloned())
        }

        async fn get_by_id(
            &self,
            tenant_id: TenantId,
            version_id: uuid::Uuid,
        ) -> Result<Option<RulesetVersion>, PrismError> {
            let versions = self.versions.lock().unwrap();
            Ok(versions
                .iter()
                .find(|v| v.tenant_id == tenant_id && v.id == version_id)
                .cloned())
        }

        async fn promote(
            &self,
            tenant_id: TenantId,
            version_id: uuid::Uuid,
        ) -> Result<(), PrismError> {
            let mut versions = self.versions.lock().unwrap();
            for v in versions.iter_mut() {
                if v.tenant_id == tenant_id {
                    v.is_active = v.id == version_id;
                }
            }
            Ok(())
        }
    }

    // -- Mock DecisionSampleRepository ----------------------------------------

    struct MockDecisionRepo {
        decisions: Mutex<Vec<(String, serde_json::Value, String)>>,
    }

    impl MockDecisionRepo {
        fn new() -> Self {
            Self {
                decisions: Mutex::new(Vec::new()),
            }
        }

        fn with_decisions(decisions: Vec<(String, serde_json::Value, String)>) -> Self {
            Self {
                decisions: Mutex::new(decisions),
            }
        }
    }

    #[async_trait]
    impl DecisionSampleRepository for MockDecisionRepo {
        async fn get_recent_decisions(
            &self,
            _tenant_id: TenantId,
            limit: usize,
        ) -> Result<Vec<(String, serde_json::Value, String)>, PrismError> {
            let decisions = self.decisions.lock().unwrap();
            Ok(decisions.iter().take(limit).cloned().collect())
        }
    }

    // -- Mock AuditEventRepository -------------------------------------------

    struct MockAuditRepo;

    #[async_trait]
    impl AuditEventRepository for MockAuditRepo {
        async fn append(&self, _event: &AuditEvent) -> Result<(), PrismError> {
            Ok(())
        }

        async fn get_chain_head(
            &self,
            _tenant_id: TenantId,
        ) -> Result<Option<AuditEvent>, PrismError> {
            Ok(None)
        }

        async fn query(
            &self,
            _request: &AuditQueryRequest,
        ) -> Result<AuditQueryResult, PrismError> {
            Ok(AuditQueryResult {
                events: Vec::new(),
                next_page_token: None,
                total_count: 0,
            })
        }

        async fn get_chain_segment(
            &self,
            _tenant_id: TenantId,
            _depth: u32,
        ) -> Result<Vec<AuditEvent>, PrismError> {
            Ok(Vec::new())
        }
    }

    // -- Helpers ---------------------------------------------------------------

    fn make_service(
        decisions: Vec<(String, serde_json::Value, String)>,
    ) -> (RulePublicationService, Arc<MockVersionRepo>) {
        let versions = Arc::new(MockVersionRepo::new());
        let decision_repo = Arc::new(MockDecisionRepo::with_decisions(decisions));
        let audit_repo = Arc::new(MockAuditRepo);
        let audit = AuditLogger::new(audit_repo);
        let svc = RulePublicationService::new(versions.clone(), decision_repo, audit);
        (svc, versions)
    }

    fn make_rule(
        tenant_id: TenantId,
        name: &str,
        action: &str,
        class: RuleClass,
        condition: serde_json::Value,
    ) -> GovernanceRule {
        GovernanceRule {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            name: name.into(),
            rule_class: class,
            action_pattern: action.into(),
            condition,
            advisory_message: None,
            is_active: true,
        }
    }

    // -- SR_GOV_19 Publication Tests ------------------------------------------

    #[tokio::test]
    async fn publish_succeeds_with_low_delta() {
        let tenant_id = TenantId::new();
        // 100 decisions, all previously allowed, none will be changed by new rule
        let decisions: Vec<_> = (0..100)
            .map(|_| {
                (
                    "data.query".to_string(),
                    serde_json::json!({"env": "dev"}),
                    "allow".to_string(),
                )
            })
            .collect();

        let (svc, versions) = make_service(decisions);

        let request = RulePublishRequest {
            tenant_id,
            rules: vec![make_rule(
                tenant_id,
                "block_prod",
                "data.query",
                RuleClass::Enforce,
                serde_json::json!({"env": "prod"}), // won't match "dev" attributes
            )],
            change_description: "Add prod block rule".into(),
            dry_run_sample_size: None,
        };

        let result = svc.publish(&request).await.unwrap();
        assert!(result.promoted);
        assert_eq!(result.dry_run_report.delta_percentage, 0.0);
        assert!(!result.dry_run_report.exceeds_threshold);
        assert_eq!(versions.version_count(), 1);
    }

    #[tokio::test]
    async fn publish_blocked_by_high_delta() {
        let tenant_id = TenantId::new();
        // 100 decisions, 10 will be changed (10% > 5% threshold)
        let mut decisions: Vec<_> = (0..90)
            .map(|_| {
                (
                    "data.query".to_string(),
                    serde_json::json!({"env": "dev"}),
                    "allow".to_string(),
                )
            })
            .collect();

        // 10 decisions that were previously "allow" but will now be "deny"
        decisions.extend((0..10).map(|_| {
            (
                "data.query".to_string(),
                serde_json::json!({"env": "prod"}),
                "allow".to_string(),
            )
        }));

        let (svc, _) = make_service(decisions);

        let request = RulePublishRequest {
            tenant_id,
            rules: vec![make_rule(
                tenant_id,
                "block_prod",
                "data.query",
                RuleClass::Enforce,
                serde_json::json!({"env": "prod"}),
            )],
            change_description: "Block all prod queries".into(),
            dry_run_sample_size: None,
        };

        let result = svc.publish(&request).await.unwrap();
        assert!(!result.promoted); // blocked by delta
        assert!(result.dry_run_report.exceeds_threshold);
        assert_eq!(result.dry_run_report.delta_percentage, 10.0);
    }

    #[tokio::test]
    async fn publish_rejects_empty_ruleset() {
        let (svc, _) = make_service(vec![]);
        let tenant_id = TenantId::new();

        let request = RulePublishRequest {
            tenant_id,
            rules: vec![],
            change_description: "empty".into(),
            dry_run_sample_size: None,
        };

        let err = svc.publish(&request).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn publish_rejects_rule_with_empty_name() {
        let (svc, _) = make_service(vec![]);
        let tenant_id = TenantId::new();

        let request = RulePublishRequest {
            tenant_id,
            rules: vec![make_rule(
                tenant_id,
                "",
                "data.query",
                RuleClass::Enforce,
                serde_json::json!({}),
            )],
            change_description: "bad rule".into(),
            dry_run_sample_size: None,
        };

        let err = svc.publish(&request).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn publish_rejects_non_object_condition() {
        let (svc, _) = make_service(vec![]);
        let tenant_id = TenantId::new();

        let request = RulePublishRequest {
            tenant_id,
            rules: vec![make_rule(
                tenant_id,
                "bad_condition",
                "data.query",
                RuleClass::Enforce,
                serde_json::json!("not an object"),
            )],
            change_description: "bad condition".into(),
            dry_run_sample_size: None,
        };

        let err = svc.publish(&request).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn publish_with_no_decision_history_promotes() {
        let (svc, _) = make_service(vec![]); // no recent decisions
        let tenant_id = TenantId::new();

        let request = RulePublishRequest {
            tenant_id,
            rules: vec![make_rule(
                tenant_id,
                "first_rule",
                "data.query",
                RuleClass::Enforce,
                serde_json::json!({"env": "prod"}),
            )],
            change_description: "First ruleset".into(),
            dry_run_sample_size: None,
        };

        let result = svc.publish(&request).await.unwrap();
        assert!(result.promoted);
        assert_eq!(result.dry_run_report.sample_size, 0);
    }

    #[tokio::test]
    async fn publish_increments_version_number() {
        let tenant_id = TenantId::new();
        let (svc, versions) = make_service(vec![]);

        let rules = vec![make_rule(
            tenant_id,
            "rule_v1",
            "data.query",
            RuleClass::Enforce,
            serde_json::json!({"env": "prod"}),
        )];

        let r1 = svc
            .publish(&RulePublishRequest {
                tenant_id,
                rules: rules.clone(),
                change_description: "v1".into(),
                dry_run_sample_size: None,
            })
            .await
            .unwrap();

        let r2 = svc
            .publish(&RulePublishRequest {
                tenant_id,
                rules,
                change_description: "v2".into(),
                dry_run_sample_size: None,
            })
            .await
            .unwrap();

        assert!(r1.promoted);
        assert!(r2.promoted);
        assert_ne!(r1.version_id, r2.version_id);
        assert_eq!(versions.version_count(), 2);
    }

    // -- SR_GOV_20 Conflict Detection Tests -----------------------------------

    #[test]
    fn detects_contradiction_between_enforce_and_advise() {
        let tenant_id = TenantId::new();
        let rules = vec![
            make_rule(
                tenant_id,
                "enforce_block_pii",
                "data.export",
                RuleClass::Enforce,
                serde_json::json!({"contains_pii": true}),
            ),
            make_rule(
                tenant_id,
                "advise_pii_ok",
                "data.export",
                RuleClass::Advise,
                serde_json::json!({"contains_pii": true}),
            ),
        ];

        let report = ConflictDetector::scan(&rules);
        assert!(!report.conflicts.is_empty());
        assert_eq!(
            report.conflicts[0].conflict_type,
            ConflictType::Contradiction
        );
        assert!(report.blocks_promotion);
        assert_eq!(report.severity, Severity::High);
    }

    #[test]
    fn detects_subsumption_between_same_class_rules() {
        let tenant_id = TenantId::new();
        let rules = vec![
            make_rule(
                tenant_id,
                "broad_rule",
                "data.export",
                RuleClass::Enforce,
                serde_json::json!({"env": "prod"}),
            ),
            make_rule(
                tenant_id,
                "narrow_rule",
                "data.export",
                RuleClass::Enforce,
                serde_json::json!({"env": "prod", "region": "us-east"}),
            ),
        ];

        let report = ConflictDetector::scan(&rules);
        assert!(!report.conflicts.is_empty());
        assert_eq!(report.conflicts[0].conflict_type, ConflictType::Subsumption);
        assert!(!report.blocks_promotion); // subsumption doesn't block
    }

    #[test]
    fn no_conflicts_for_non_overlapping_rules() {
        let tenant_id = TenantId::new();
        let rules = vec![
            make_rule(
                tenant_id,
                "block_pii_export",
                "data.export",
                RuleClass::Enforce,
                serde_json::json!({"contains_pii": true}),
            ),
            make_rule(
                tenant_id,
                "warn_large_query",
                "data.query",
                RuleClass::Advise,
                serde_json::json!({"row_count_gt": 10000}),
            ),
        ];

        let report = ConflictDetector::scan(&rules);
        assert!(report.conflicts.is_empty());
        assert!(!report.blocks_promotion);
    }

    #[test]
    fn wildcard_action_overlaps_with_specific() {
        let tenant_id = TenantId::new();
        let rules = vec![
            make_rule(
                tenant_id,
                "enforce_all",
                "*",
                RuleClass::Enforce,
                serde_json::json!({"contains_pii": true}),
            ),
            make_rule(
                tenant_id,
                "advise_export",
                "data.export",
                RuleClass::Advise,
                serde_json::json!({"contains_pii": true}),
            ),
        ];

        let report = ConflictDetector::scan(&rules);
        assert!(!report.conflicts.is_empty());
        assert_eq!(
            report.conflicts[0].conflict_type,
            ConflictType::Contradiction
        );
    }

    #[test]
    fn contradiction_blocks_publication() {
        let tenant_id = TenantId::new();
        let rules = vec![
            make_rule(
                tenant_id,
                "enforce_deny",
                "automation.activate",
                RuleClass::Enforce,
                serde_json::json!({"risk_tier": "high"}),
            ),
            make_rule(
                tenant_id,
                "advise_allow",
                "automation.activate",
                RuleClass::Advise,
                serde_json::json!({"risk_tier": "high"}),
            ),
        ];

        let report = ConflictDetector::scan(&rules);
        assert!(report.blocks_promotion);
    }

    #[tokio::test]
    async fn publish_blocked_by_contradiction() {
        let tenant_id = TenantId::new();
        let (svc, _) = make_service(vec![]);

        let request = RulePublishRequest {
            tenant_id,
            rules: vec![
                make_rule(
                    tenant_id,
                    "enforce_deny",
                    "data.export",
                    RuleClass::Enforce,
                    serde_json::json!({"pii": true}),
                ),
                make_rule(
                    tenant_id,
                    "advise_allow",
                    "data.export",
                    RuleClass::Advise,
                    serde_json::json!({"pii": true}),
                ),
            ],
            change_description: "conflicting rules".into(),
            dry_run_sample_size: None,
        };

        let result = svc.publish(&request).await.unwrap();
        assert!(!result.promoted); // blocked by contradiction
    }

    #[test]
    fn empty_ruleset_reports_no_conflicts() {
        let report = ConflictDetector::scan(&[]);
        assert!(report.conflicts.is_empty());
        assert!(!report.blocks_promotion);
    }
}
