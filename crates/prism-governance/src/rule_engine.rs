//! Governance rule evaluation engine (SR_GOV_16, SR_GOV_17).
//!
//! Evaluates ENFORCE and ADVISE rules against candidate actions.
//!
//! - **ENFORCE** rules (SR_GOV_16): Non-overridable. DENY on any match.
//!   Default-DENY if the rule engine is unavailable (SR_GOV_16_SE-01).
//! - **ADVISE** rules (SR_GOV_17): Overridable. May return ALLOW,
//!   ALLOW_WITH_WARNING, or REQUIRE_JUSTIFICATION.
//!
//! Rule conditions use a simple attribute-matching model: a rule fires
//! when all key-value pairs in its `condition` JSON are present and equal
//! in the request's `attributes` JSON. This is intentionally simple for
//! the MVP; JSONLogic or a full policy engine can replace the matcher
//! without changing the service interface.

use std::sync::Arc;

use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::GovernanceRuleRepository;
use prism_core::types::*;

/// Governance rule evaluation engine.
///
/// Composes:
/// - `GovernanceRuleRepository` -- loads active rules per tenant/action
/// - `AuditLogger` -- records rule evaluation outcomes
///
/// Implements: SR_GOV_16, SR_GOV_17
pub struct RuleEngine {
    rules_repo: Arc<dyn GovernanceRuleRepository>,
    audit: AuditLogger,
}

impl RuleEngine {
    /// Create a new rule engine.
    pub fn new(rules_repo: Arc<dyn GovernanceRuleRepository>, audit: AuditLogger) -> Self {
        Self { rules_repo, audit }
    }

    /// Evaluate ENFORCE rules against a candidate action.
    ///
    /// Any matching ENFORCE rule results in DENY. If no rules match, ALLOW.
    /// On repository failure, default to DENY (SR_GOV_16_SE-01).
    ///
    /// Implements: SR_GOV_16
    pub async fn evaluate_enforce(
        &self,
        request: &RuleEvaluationRequest,
    ) -> Result<EnforceEvaluationResult, PrismError> {
        let rules = match self
            .rules_repo
            .list_active_rules(request.tenant_id, &request.action, RuleClass::Enforce)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // SR_GOV_16_SE-01: DENY by default when rule engine unavailable
                warn!(
                    tenant_id = %request.tenant_id,
                    action = %request.action,
                    error = %e,
                    "ENFORCE: rule engine unavailable -- defaulting to DENY"
                );

                self.audit_enforce_decision(
                    request,
                    EnforceDecision::Deny,
                    &[],
                    Some("rule engine unavailable -- failsafe deny"),
                )
                .await?;

                return Ok(EnforceEvaluationResult {
                    decision: EnforceDecision::Deny,
                    matched_rules: vec![],
                    reason: Some("rule engine unavailable -- failsafe deny".into()),
                });
            }
        };

        let matched: Vec<String> = rules
            .iter()
            .filter(|rule| Self::condition_matches(&rule.condition, &request.attributes))
            .map(|rule| rule.name.clone())
            .collect();

        let (decision, reason) = if matched.is_empty() {
            (EnforceDecision::Allow, None)
        } else {
            (
                EnforceDecision::Deny,
                Some(format!("denied by ENFORCE rule(s): {}", matched.join(", "))),
            )
        };

        self.audit_enforce_decision(request, decision, &matched, reason.as_deref())
            .await?;

        if decision == EnforceDecision::Deny {
            warn!(
                tenant_id = %request.tenant_id,
                action = %request.action,
                matched_rules = ?matched,
                "ENFORCE: action DENIED"
            );
        }

        Ok(EnforceEvaluationResult {
            decision,
            matched_rules: matched,
            reason,
        })
    }

    /// Evaluate ADVISE rules against a candidate action.
    ///
    /// Returns ALLOW (no rules match), ALLOW_WITH_WARNING (advisory match
    /// without justification requirement), or REQUIRE_JUSTIFICATION
    /// (at least one matched rule requires justification).
    ///
    /// Implements: SR_GOV_17
    pub async fn evaluate_advise(
        &self,
        request: &RuleEvaluationRequest,
    ) -> Result<AdviseEvaluationResult, PrismError> {
        let rules = self
            .rules_repo
            .list_active_rules(request.tenant_id, &request.action, RuleClass::Advise)
            .await?;

        let matched: Vec<&GovernanceRule> = rules
            .iter()
            .filter(|rule| Self::condition_matches(&rule.condition, &request.attributes))
            .collect();

        if matched.is_empty() {
            return Ok(AdviseEvaluationResult {
                decision: AdviseDecision::Allow,
                advisory_messages: vec![],
                requires_justification: false,
                matched_rules: vec![],
            });
        }

        let advisory_messages: Vec<String> = matched
            .iter()
            .filter_map(|r| r.advisory_message.clone())
            .collect();

        let matched_names: Vec<String> = matched.iter().map(|r| r.name.clone()).collect();

        // If any matched rule has a condition field "requires_justification": true,
        // escalate to REQUIRE_JUSTIFICATION
        let requires_justification = matched.iter().any(|r| {
            r.condition
                .get("requires_justification")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        });

        let decision = if requires_justification {
            AdviseDecision::RequireJustification
        } else {
            AdviseDecision::AllowWithWarning
        };

        info!(
            tenant_id = %request.tenant_id,
            action = %request.action,
            decision = ?decision,
            matched_rules = ?matched_names,
            "ADVISE: rules evaluated"
        );

        // Audit the advisory
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "governance.advise_evaluated".into(),
                actor_id: request.subject_principal,
                actor_type: ActorType::Human,
                target_id: None,
                target_type: None,
                severity: Severity::Low,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "action": request.action,
                    "decision": decision,
                    "matched_rules": matched_names,
                    "requires_justification": requires_justification,
                }),
            })
            .await?;

        Ok(AdviseEvaluationResult {
            decision,
            advisory_messages,
            requires_justification,
            matched_rules: matched_names,
        })
    }

    /// Simple attribute-matching condition evaluator.
    ///
    /// A condition matches when every key-value pair in `condition` has an
    /// equal value in `attributes`. This is the MVP matcher; it can be
    /// replaced with JSONLogic without changing the service interface.
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
            // Skip meta-fields (requires_justification is a rule directive, not a condition)
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

    /// Record an ENFORCE evaluation decision in the audit trail.
    async fn audit_enforce_decision(
        &self,
        request: &RuleEvaluationRequest,
        decision: EnforceDecision,
        matched_rules: &[String],
        reason: Option<&str>,
    ) -> Result<(), PrismError> {
        let severity = match decision {
            EnforceDecision::Allow => Severity::Low,
            EnforceDecision::Deny => Severity::High,
        };

        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: if decision == EnforceDecision::Deny {
                    "governance.enforce_denied".into()
                } else {
                    "governance.enforce_allowed".into()
                },
                actor_id: request.subject_principal,
                actor_type: ActorType::Human,
                target_id: None,
                target_type: None,
                severity,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "action": request.action,
                    "decision": decision,
                    "matched_rules": matched_rules,
                    "reason": reason,
                }),
            })
            .await?;

        Ok(())
    }

    /// Capture and validate an ADVISE override justification.
    ///
    /// Called after `evaluate_advise` returns `requires_justification = true`.
    /// Validates the justification text against the filler-word blocklist
    /// and minimum-quality rules, then persists it linked to the action
    /// and overridden rule.
    ///
    /// Implements: SR_GOV_18
    pub async fn capture_justification(
        &self,
        request: &OverrideJustificationRequest,
    ) -> Result<OverrideJustificationResult, PrismError> {
        // Validate the justification text
        if let Some(rejection) = JustificationValidator::validate(&request.justification_text) {
            info!(
                tenant_id = %request.tenant_id,
                person_id = %request.person_id,
                rule_id = %request.rule_id,
                "ADVISE override justification rejected: {rejection}"
            );

            // Audit the rejection
            self.audit
                .log(AuditEventInput {
                    tenant_id: request.tenant_id,
                    event_type: "governance.justification_rejected".into(),
                    actor_id: *request.person_id.as_uuid(),
                    actor_type: ActorType::Human,
                    target_id: Some(request.action_id),
                    target_type: Some("Action".into()),
                    severity: Severity::Medium,
                    source_layer: SourceLayer::Governance,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "rule_id": request.rule_id.to_string(),
                        "rejection_reason": rejection,
                        "category": request.category,
                    }),
                })
                .await?;

            return Ok(OverrideJustificationResult {
                accepted: false,
                rejection_reason: Some(rejection),
            });
        }

        // Justification accepted -- audit it
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "governance.advise_override_justified".into(),
                actor_id: *request.person_id.as_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(request.action_id),
                target_type: Some("Action".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "rule_id": request.rule_id.to_string(),
                    "justification_text": request.justification_text,
                    "category": request.category,
                }),
            })
            .await?;

        info!(
            tenant_id = %request.tenant_id,
            person_id = %request.person_id,
            rule_id = %request.rule_id,
            "ADVISE override justification accepted"
        );

        Ok(OverrideJustificationResult {
            accepted: true,
            rejection_reason: None,
        })
    }
}

// ---------------------------------------------------------------------------
// JustificationValidator
// ---------------------------------------------------------------------------

/// Validates justification text for ADVISE overrides.
///
/// Rejects:
/// - Empty or whitespace-only text
/// - Text shorter than 20 characters (per BP-134)
/// - Filler words that provide no meaningful justification
///
/// Returns `None` if valid, `Some(reason)` if rejected.
///
/// Implements: SR_GOV_18, SR_GOV_18_BE-01
pub struct JustificationValidator;

/// Words and phrases that indicate a low-effort justification.
const FILLER_BLOCKLIST: &[&str] = &[
    "because",
    "ok",
    "okay",
    "n/a",
    "na",
    "none",
    "no",
    "nope",
    "yes",
    "idk",
    "i don't know",
    "i dont know",
    "test",
    "testing",
    "asdf",
    "aaa",
    "xxx",
    "...",
    "---",
];

impl JustificationValidator {
    /// Minimum character length for a justification (per BP-134).
    const MIN_LENGTH: usize = 20;

    /// Validate a justification string.
    ///
    /// Returns `None` if the justification passes all checks, or
    /// `Some(reason)` describing the specific failure.
    ///
    /// Implements: SR_GOV_18_BE-01
    pub fn validate(text: &str) -> Option<String> {
        let trimmed = text.trim();

        // Check empty
        if trimmed.is_empty() {
            return Some("justification text cannot be empty".into());
        }

        // Check minimum length (BP-134: at least 20 characters)
        if trimmed.len() < Self::MIN_LENGTH {
            return Some(format!(
                "justification must be at least {} characters (got {}); \
                 please provide a meaningful explanation",
                Self::MIN_LENGTH,
                trimmed.len()
            ));
        }

        // Check filler-word blocklist
        let lower = trimmed.to_lowercase();
        for filler in FILLER_BLOCKLIST {
            if lower == *filler {
                return Some(format!(
                    "justification \"{trimmed}\" is not a meaningful explanation; \
                     please describe why this override is necessary"
                ));
            }
        }

        // Check if text is just repeated characters (e.g., "aaaaaaaaaaaaaaaaaaaaaa")
        let first_char = lower.chars().next().unwrap(); // safe: non-empty
        if lower.chars().all(|c| c == first_char) {
            return Some(
                "justification appears to be repeated characters; \
                 please provide a meaningful explanation"
                    .into(),
            );
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

    // -- Mock GovernanceRuleRepository ----------------------------------------

    struct MockRuleRepo {
        rules: Mutex<Vec<GovernanceRule>>,
        fail_on_query: Mutex<bool>,
    }

    impl MockRuleRepo {
        fn new() -> Self {
            Self {
                rules: Mutex::new(Vec::new()),
                fail_on_query: Mutex::new(false),
            }
        }

        fn add_rule(&self, rule: GovernanceRule) {
            self.rules.lock().unwrap().push(rule);
        }

        fn set_fail(&self, fail: bool) {
            *self.fail_on_query.lock().unwrap() = fail;
        }
    }

    #[async_trait]
    impl GovernanceRuleRepository for MockRuleRepo {
        async fn list_active_rules(
            &self,
            tenant_id: TenantId,
            action: &str,
            rule_class: RuleClass,
        ) -> Result<Vec<GovernanceRule>, PrismError> {
            if *self.fail_on_query.lock().unwrap() {
                return Err(PrismError::Database("simulated failure".into()));
            }

            let rules = self.rules.lock().unwrap();
            Ok(rules
                .iter()
                .filter(|r| {
                    r.tenant_id == tenant_id
                        && r.rule_class == rule_class
                        && r.is_active
                        && (r.action_pattern == action || r.action_pattern == "*")
                })
                .cloned()
                .collect())
        }
    }

    // -- Mock AuditEventRepository -------------------------------------------

    struct MockAuditRepo {
        events: Mutex<Vec<AuditEvent>>,
    }

    impl MockAuditRepo {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl AuditEventRepository for MockAuditRepo {
        async fn append(&self, event: &AuditEvent) -> Result<(), PrismError> {
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }

        async fn get_chain_head(
            &self,
            tenant_id: TenantId,
        ) -> Result<Option<AuditEvent>, PrismError> {
            let events = self.events.lock().unwrap();
            Ok(events
                .iter()
                .filter(|e| e.tenant_id == tenant_id)
                .max_by_key(|e| e.chain_position)
                .cloned())
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

    // -- Helpers --------------------------------------------------------------

    fn make_engine() -> (RuleEngine, Arc<MockRuleRepo>) {
        let rules_repo = Arc::new(MockRuleRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        let engine = RuleEngine::new(rules_repo.clone(), audit);
        (engine, rules_repo)
    }

    fn enforce_rule(
        tenant_id: TenantId,
        name: &str,
        action: &str,
        condition: serde_json::Value,
    ) -> GovernanceRule {
        GovernanceRule {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            name: name.into(),
            rule_class: RuleClass::Enforce,
            action_pattern: action.into(),
            condition,
            advisory_message: None,
            is_active: true,
        }
    }

    fn advise_rule(
        tenant_id: TenantId,
        name: &str,
        action: &str,
        condition: serde_json::Value,
        message: &str,
    ) -> GovernanceRule {
        GovernanceRule {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            name: name.into(),
            rule_class: RuleClass::Advise,
            action_pattern: action.into(),
            condition,
            advisory_message: Some(message.into()),
            is_active: true,
        }
    }

    fn make_request(
        tenant_id: TenantId,
        action: &str,
        attributes: serde_json::Value,
    ) -> RuleEvaluationRequest {
        RuleEvaluationRequest {
            tenant_id,
            action: action.into(),
            subject_principal: uuid::Uuid::nil(),
            attributes,
            rule_classes: vec![RuleClass::Enforce],
        }
    }

    // -- ENFORCE Tests --------------------------------------------------------

    #[tokio::test]
    async fn enforce_allows_when_no_rules_match() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let request = make_request(tenant_id, "automation.activate", serde_json::json!({}));
        let result = engine.evaluate_enforce(&request).await.unwrap();

        assert_eq!(result.decision, EnforceDecision::Allow);
        assert!(result.matched_rules.is_empty());
    }

    #[tokio::test]
    async fn enforce_denies_when_rule_matches() {
        let (engine, repo) = make_engine();
        let tenant_id = TenantId::new();

        repo.add_rule(enforce_rule(
            tenant_id,
            "block_prod_deploy",
            "automation.activate",
            serde_json::json!({"environment": "PROD"}),
        ));

        let request = make_request(
            tenant_id,
            "automation.activate",
            serde_json::json!({"environment": "PROD"}),
        );
        let result = engine.evaluate_enforce(&request).await.unwrap();

        assert_eq!(result.decision, EnforceDecision::Deny);
        assert_eq!(result.matched_rules, vec!["block_prod_deploy"]);
    }

    #[tokio::test]
    async fn enforce_allows_when_attributes_dont_match() {
        let (engine, repo) = make_engine();
        let tenant_id = TenantId::new();

        repo.add_rule(enforce_rule(
            tenant_id,
            "block_prod_deploy",
            "automation.activate",
            serde_json::json!({"environment": "PROD"}),
        ));

        let request = make_request(
            tenant_id,
            "automation.activate",
            serde_json::json!({"environment": "DEV"}),
        );
        let result = engine.evaluate_enforce(&request).await.unwrap();

        assert_eq!(result.decision, EnforceDecision::Allow);
    }

    #[tokio::test]
    async fn enforce_failsafe_deny_on_repo_failure() {
        let (engine, repo) = make_engine();
        let tenant_id = TenantId::new();
        repo.set_fail(true);

        let request = make_request(tenant_id, "automation.activate", serde_json::json!({}));
        let result = engine.evaluate_enforce(&request).await.unwrap();

        assert_eq!(result.decision, EnforceDecision::Deny);
        assert!(result.reason.unwrap().contains("failsafe"));
    }

    #[tokio::test]
    async fn enforce_multiple_rules_all_reported() {
        let (engine, repo) = make_engine();
        let tenant_id = TenantId::new();

        repo.add_rule(enforce_rule(
            tenant_id,
            "pii_restriction",
            "data.export",
            serde_json::json!({"contains_pii": true}),
        ));
        repo.add_rule(enforce_rule(
            tenant_id,
            "external_restriction",
            "data.export",
            serde_json::json!({"contains_pii": true}),
        ));

        let request = make_request(
            tenant_id,
            "data.export",
            serde_json::json!({"contains_pii": true}),
        );
        let result = engine.evaluate_enforce(&request).await.unwrap();

        assert_eq!(result.decision, EnforceDecision::Deny);
        assert_eq!(result.matched_rules.len(), 2);
    }

    #[tokio::test]
    async fn enforce_tenant_isolation() {
        let (engine, repo) = make_engine();
        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();

        repo.add_rule(enforce_rule(
            tenant_a,
            "tenant_a_rule",
            "automation.activate",
            serde_json::json!({}),
        ));

        // Tenant B should not see tenant A's rules
        let request = make_request(tenant_b, "automation.activate", serde_json::json!({}));
        let result = engine.evaluate_enforce(&request).await.unwrap();

        assert_eq!(result.decision, EnforceDecision::Allow);
    }

    // -- ADVISE Tests ---------------------------------------------------------

    #[tokio::test]
    async fn advise_allows_when_no_rules_match() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let request = make_request(tenant_id, "data.query", serde_json::json!({}));
        let result = engine.evaluate_advise(&request).await.unwrap();

        assert_eq!(result.decision, AdviseDecision::Allow);
        assert!(!result.requires_justification);
    }

    #[tokio::test]
    async fn advise_returns_warning_when_rule_matches() {
        let (engine, repo) = make_engine();
        let tenant_id = TenantId::new();

        repo.add_rule(advise_rule(
            tenant_id,
            "stale_data_warning",
            "data.query",
            serde_json::json!({"data_age_days": 90}),
            "Data is over 90 days old -- consider refreshing",
        ));

        let request = make_request(
            tenant_id,
            "data.query",
            serde_json::json!({"data_age_days": 90}),
        );
        let result = engine.evaluate_advise(&request).await.unwrap();

        assert_eq!(result.decision, AdviseDecision::AllowWithWarning);
        assert!(!result.requires_justification);
        assert_eq!(result.advisory_messages.len(), 1);
    }

    #[tokio::test]
    async fn advise_requires_justification_when_flagged() {
        let (engine, repo) = make_engine();
        let tenant_id = TenantId::new();

        repo.add_rule(advise_rule(
            tenant_id,
            "cross_entity_justification",
            "data.combine",
            serde_json::json!({"cross_entity": true, "requires_justification": true}),
            "Cross-entity data combination requires justification",
        ));

        let request = make_request(
            tenant_id,
            "data.combine",
            serde_json::json!({"cross_entity": true, "requires_justification": true}),
        );
        let result = engine.evaluate_advise(&request).await.unwrap();

        assert_eq!(result.decision, AdviseDecision::RequireJustification);
        assert!(result.requires_justification);
    }

    #[tokio::test]
    async fn condition_matcher_handles_partial_match() {
        // Condition requires two fields, attributes only has one
        let condition = serde_json::json!({"a": 1, "b": 2});
        let attributes = serde_json::json!({"a": 1});

        assert!(!RuleEngine::condition_matches(&condition, &attributes));
    }

    #[tokio::test]
    async fn condition_matcher_handles_superset_attributes() {
        // Attributes have more fields than condition -- should still match
        let condition = serde_json::json!({"a": 1});
        let attributes = serde_json::json!({"a": 1, "b": 2, "c": 3});

        assert!(RuleEngine::condition_matches(&condition, &attributes));
    }

    // -- SR_GOV_18 Justification Tests ----------------------------------------

    fn make_justification_request(tenant_id: TenantId, text: &str) -> OverrideJustificationRequest {
        OverrideJustificationRequest {
            tenant_id,
            person_id: UserId::new(),
            action_id: uuid::Uuid::new_v4(),
            rule_id: uuid::Uuid::new_v4(),
            justification_text: text.into(),
            category: None,
        }
    }

    #[tokio::test]
    async fn justification_accepts_valid_text() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let request = make_justification_request(
            tenant_id,
            "Overriding because the client requested expedited processing for the Q4 deadline",
        );
        let result = engine.capture_justification(&request).await.unwrap();

        assert!(result.accepted);
        assert!(result.rejection_reason.is_none());
    }

    #[tokio::test]
    async fn justification_rejects_empty_text() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let request = make_justification_request(tenant_id, "");
        let result = engine.capture_justification(&request).await.unwrap();

        assert!(!result.accepted);
        assert!(result.rejection_reason.unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn justification_rejects_whitespace_only() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let request = make_justification_request(tenant_id, "   \t\n  ");
        let result = engine.capture_justification(&request).await.unwrap();

        assert!(!result.accepted);
        assert!(result.rejection_reason.unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn justification_rejects_too_short() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let request = make_justification_request(tenant_id, "short reason");
        let result = engine.capture_justification(&request).await.unwrap();

        assert!(!result.accepted);
        assert!(result.rejection_reason.unwrap().contains("at least 20"));
    }

    #[tokio::test]
    async fn justification_rejects_filler_words() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        for filler in &["because", "ok", "n/a", "idk", "i don't know", "nope"] {
            let request = make_justification_request(tenant_id, filler);
            let result = engine.capture_justification(&request).await.unwrap();

            assert!(
                !result.accepted,
                "filler word '{filler}' should be rejected"
            );
        }
    }

    #[tokio::test]
    async fn justification_rejects_repeated_characters() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let request = make_justification_request(tenant_id, "aaaaaaaaaaaaaaaaaaaaaa");
        let result = engine.capture_justification(&request).await.unwrap();

        assert!(!result.accepted);
        assert!(result.rejection_reason.unwrap().contains("repeated"));
    }

    #[tokio::test]
    async fn justification_accepts_with_category() {
        let (engine, _) = make_engine();
        let tenant_id = TenantId::new();

        let mut request = make_justification_request(
            tenant_id,
            "Client deadline requires expedited processing per executive approval",
        );
        request.category = Some("business_urgency".into());

        let result = engine.capture_justification(&request).await.unwrap();
        assert!(result.accepted);
    }

    // -- JustificationValidator Unit Tests ------------------------------------

    #[test]
    fn validator_accepts_valid_justification() {
        assert!(JustificationValidator::validate(
            "This override is necessary because the compliance team approved the exception"
        )
        .is_none());
    }

    #[test]
    fn validator_rejects_empty() {
        assert!(JustificationValidator::validate("").is_some());
    }

    #[test]
    fn validator_rejects_under_min_length() {
        assert!(JustificationValidator::validate("too short").is_some());
    }

    #[test]
    fn validator_rejects_blocklist_match() {
        assert!(JustificationValidator::validate("because").is_some());
        assert!(JustificationValidator::validate("N/A").is_some());
        assert!(JustificationValidator::validate("IDK").is_some());
    }

    #[test]
    fn validator_allows_blocklist_word_in_longer_text() {
        // "because" alone is blocked, but "because X" as part of a real sentence is fine
        assert!(JustificationValidator::validate(
            "Overriding because the regulatory deadline is tomorrow and we have board approval"
        )
        .is_none());
    }

    #[test]
    fn validator_rejects_repeated_chars() {
        assert!(JustificationValidator::validate("xxxxxxxxxxxxxxxxxxxx").is_some());
    }
}
