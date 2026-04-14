//! Cross-System Aggregation (CSA) engine (SR_GOV_23, SR_GOV_24, SR_GOV_25, SR_GOV_26).
//!
//! Implements the CSA rule lifecycle:
//! - SR_GOV_23: Rule registration with expression parsing and validation
//! - SR_GOV_24: Assessment trigger composing rule loading + evaluation + audit
//! - SR_GOV_25: Pure-function evaluator (no I/O)
//! - SR_GOV_26: BLOCK action handler
//!
//! CSA rules fire when multiple data collections are combined and the
//! combined attribute set matches a rule expression. The expression grammar
//! is `ATTR1 + ATTR2 = SEVERITY`.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::CsaRuleRepository;
use prism_core::types::*;

/// Known attribute names that may appear in CSA rule expressions.
const KNOWN_ATTRIBUTES: &[&str] = &[
    "pii",
    "phi",
    "cui",
    "financial",
    "location",
    "temporal",
    "group_size",
    "external",
];

// ===========================================================================
// SR_GOV_23 -- CSA Rule Registration
// ===========================================================================

/// Service for registering CSA rules.
///
/// Composes:
/// - `CsaRuleRepository` -- persistence for rules
/// - `AuditLogger` -- audit trail for rule events
///
/// Implements: SR_GOV_23
pub struct CsaRuleService {
    repo: Arc<dyn CsaRuleRepository>,
    audit: AuditLogger,
}

impl CsaRuleService {
    /// Create a new CSA rule service.
    pub fn new(repo: Arc<dyn CsaRuleRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Register a new CSA rule.
    ///
    /// Parses the rule expression, validates all attributes are from the known set,
    /// persists the rule, and emits an audit event.
    ///
    /// Expression grammar: `ATTR1 + ATTR2 = SEVERITY`
    ///
    /// Implements: SR_GOV_23, SR_GOV_23_BE-01
    pub async fn register_rule(
        &self,
        request: &CsaRuleRegistration,
    ) -> Result<CsaRuleResult, PrismError> {
        // Parse and validate the expression
        let _attributes = parse_rule_expression(&request.rule_expression)?;

        let rule_id = uuid::Uuid::now_v7();
        let rule = CsaRule {
            id: rule_id,
            tenant_id: request.tenant_id,
            rule_expression: request.rule_expression.clone(),
            action: request.action,
            severity: request.severity,
            version: 1,
            is_active: true,
            created_at: Utc::now(),
        };

        self.repo.create(&rule).await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "csa.rule_registered".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(rule_id),
                target_type: Some("CsaRule".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "rule_expression": request.rule_expression,
                    "action": format!("{:?}", request.action),
                    "severity": format!("{:?}", request.severity),
                }),
            })
            .await?;

        info!(
            tenant_id = %request.tenant_id,
            rule_id = %rule_id,
            expression = %request.rule_expression,
            "CSA rule registered"
        );

        Ok(CsaRuleResult {
            rule_id,
            version: 1,
            active: true,
        })
    }
}

/// Parse a rule expression into a list of attribute names.
///
/// Grammar: `ATTR1 + ATTR2 [+ ...] = SEVERITY`
/// The part before `=` is split on `+` and trimmed.
/// Each attribute must be in the known set.
///
/// Implements: SR_GOV_23_BE-01
fn parse_rule_expression(expression: &str) -> Result<Vec<String>, PrismError> {
    let expr = expression.trim();

    if expr.is_empty() {
        return Err(PrismError::Validation {
            reason: "rule expression must not be empty".into(),
        });
    }

    // Split on '=' to separate attributes from severity label
    let parts: Vec<&str> = expr.splitn(2, '=').collect();
    if parts.len() < 2 {
        return Err(PrismError::Validation {
            reason: format!(
                "rule expression must contain '=' separator: '{}'",
                expression
            ),
        });
    }

    let attr_part = parts[0].trim();
    if attr_part.is_empty() {
        return Err(PrismError::Validation {
            reason: "rule expression has no attributes before '='".into(),
        });
    }

    let attributes: Vec<String> = attr_part
        .split('+')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    if attributes.is_empty() {
        return Err(PrismError::Validation {
            reason: "rule expression must contain at least one attribute".into(),
        });
    }

    // Validate all attributes are known
    for attr in &attributes {
        if !KNOWN_ATTRIBUTES.contains(&attr.as_str()) {
            return Err(PrismError::Validation {
                reason: format!(
                    "unknown attribute '{}'; known attributes: {}",
                    attr,
                    KNOWN_ATTRIBUTES.join(", ")
                ),
            });
        }
    }

    Ok(attributes)
}

// ===========================================================================
// SR_GOV_25 -- CSA Evaluator (pure function, no I/O)
// ===========================================================================

/// Pure-function evaluator for CSA rules.
///
/// Given a set of rules and an attribute set, determines which rules match
/// and the highest-severity action to apply.
///
/// Implements: SR_GOV_25
pub struct CsaEvaluator;

impl CsaEvaluator {
    /// Evaluate CSA rules against an attribute set.
    ///
    /// A rule matches when ALL attributes in its expression are present
    /// in the provided `attribute_set`.
    ///
    /// Returns matched rule IDs sorted by severity (descending), and the
    /// highest-severity action.
    ///
    /// Implements: SR_GOV_25
    pub fn evaluate(
        rules: &[CsaRule],
        attribute_set: &HashSet<String>,
        _group_size: Option<u64>,
        _query_purpose: Option<String>,
    ) -> CsaEvaluationOutput {
        let mut matched: Vec<(&CsaRule, Vec<String>)> = Vec::new();

        for rule in rules {
            if let Ok(attrs) = parse_rule_expression(&rule.rule_expression) {
                let all_present = attrs.iter().all(|a| attribute_set.contains(a));
                if all_present {
                    matched.push((rule, attrs));
                }
            }
        }

        if matched.is_empty() {
            return CsaEvaluationOutput {
                matched_rules: Vec::new(),
                highest_action: None,
            };
        }

        // Sort by severity descending (Ord is derived Low < Medium < High < Critical)
        matched.sort_by(|a, b| b.0.severity.cmp(&a.0.severity));

        let highest_action = Some(matched[0].0.action);
        let matched_rules = matched
            .iter()
            .map(|(rule, _)| rule.id.to_string())
            .collect();

        CsaEvaluationOutput {
            matched_rules,
            highest_action,
        }
    }
}

// ===========================================================================
// SR_GOV_24 -- CSA Assessment Trigger
// ===========================================================================

/// Service for triggering CSA assessments.
///
/// Composes:
/// - `CsaRuleRepository` -- loads active rules
/// - `CsaEvaluator` -- pure-function evaluator
/// - `AuditLogger` -- audit trail for assessments
///
/// Implements: SR_GOV_24
pub struct CsaAssessmentService {
    repo: Arc<dyn CsaRuleRepository>,
    audit: AuditLogger,
}

impl CsaAssessmentService {
    /// Create a new CSA assessment service.
    pub fn new(repo: Arc<dyn CsaRuleRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Trigger a CSA assessment for a multi-collection query.
    ///
    /// CSA only triggers when N >= 2 data collection references are present.
    /// Loads active rules, evaluates them, and returns the decision.
    ///
    /// Implements: SR_GOV_24
    pub async fn assess(
        &self,
        request: &CsaAssessmentRequest,
    ) -> Result<CsaAssessmentResult, PrismError> {
        // CSA only triggers for N >= 2 data collections
        if request.data_collection_refs.len() < 2 {
            return Ok(CsaAssessmentResult {
                decision: CsaDecision::Allow,
                applied_rules: Vec::new(),
                required_action: None,
            });
        }

        // Load active rules for tenant
        let rules = self.repo.list_active_rules(request.tenant_id).await?;

        // Run the evaluator
        let eval_output = CsaEvaluator::evaluate(
            &rules,
            &request.combined_attribute_set,
            None,
            request.query_purpose.clone(),
        );

        // Map highest_action to decision
        let (decision, required_action) = match eval_output.highest_action {
            None => (CsaDecision::Allow, None),
            Some(CsaAction::Block) => (CsaDecision::Block, Some(CsaAction::Block)),
            Some(CsaAction::Anonymize) => (CsaDecision::Anonymize, Some(CsaAction::Anonymize)),
            Some(CsaAction::Elevate) => (CsaDecision::Elevate, Some(CsaAction::Elevate)),
        };

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "csa.assessed".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(request.query_id),
                target_type: Some("Query".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "decision": format!("{:?}", decision),
                    "matched_rules_count": eval_output.matched_rules.len(),
                    "data_collection_count": request.data_collection_refs.len(),
                }),
            })
            .await?;

        info!(
            tenant_id = %request.tenant_id,
            query_id = %request.query_id,
            decision = ?decision,
            matched_count = eval_output.matched_rules.len(),
            "CSA assessment completed"
        );

        Ok(CsaAssessmentResult {
            decision,
            applied_rules: eval_output.matched_rules,
            required_action,
        })
    }
}

// ===========================================================================
// SR_GOV_26 -- CSA Block Handler
// ===========================================================================

/// Handler for CSA BLOCK actions.
///
/// Composes:
/// - `AuditLogger` -- audit trail for block events
///
/// Implements: SR_GOV_26
pub struct CsaBlockHandler {
    audit: AuditLogger,
}

impl CsaBlockHandler {
    /// Create a new CSA block handler.
    pub fn new(audit: AuditLogger) -> Self {
        Self { audit }
    }

    /// Handle a CSA block action by recording the rejection and suggesting
    /// alternative approaches.
    ///
    /// Emits `csa.blocked` audit event.
    ///
    /// Implements: SR_GOV_26
    pub async fn handle_block(
        &self,
        action: &CsaBlockAction,
    ) -> Result<CsaBlockResult, PrismError> {
        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: TenantId::from_uuid(uuid::Uuid::nil()),
                event_type: "csa.blocked".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(action.assessment_id),
                target_type: Some("CsaAssessment".into()),
                severity: Severity::High,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "reason": action.reason,
                    "alternatives_count": action.suggested_alternatives.len(),
                }),
            })
            .await?;

        warn!(
            assessment_id = %action.assessment_id,
            reason = %action.reason,
            "CSA BLOCK action executed"
        );

        Ok(CsaBlockResult {
            rejected: true,
            alternatives: action.suggested_alternatives.clone(),
        })
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

    // -- Mock CsaRuleRepository -----------------------------------------------

    struct MockCsaRuleRepo {
        rules: Mutex<Vec<CsaRule>>,
    }

    impl MockCsaRuleRepo {
        fn new() -> Self {
            Self {
                rules: Mutex::new(Vec::new()),
            }
        }

        fn seed(&self, rule: CsaRule) {
            self.rules.lock().unwrap().push(rule);
        }
    }

    #[async_trait]
    impl CsaRuleRepository for MockCsaRuleRepo {
        async fn create(&self, rule: &CsaRule) -> Result<(), PrismError> {
            self.rules.lock().unwrap().push(rule.clone());
            Ok(())
        }

        async fn list_active_rules(&self, tenant_id: TenantId) -> Result<Vec<CsaRule>, PrismError> {
            let rules = self.rules.lock().unwrap();
            Ok(rules
                .iter()
                .filter(|r| r.tenant_id == tenant_id && r.is_active)
                .cloned()
                .collect())
        }

        async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<CsaRule>, PrismError> {
            let rules = self.rules.lock().unwrap();
            Ok(rules.iter().find(|r| r.id == id).cloned())
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

    // -- Helpers ---------------------------------------------------------------

    fn make_audit() -> AuditLogger {
        let audit_repo = Arc::new(MockAuditRepo::new());
        AuditLogger::new(audit_repo)
    }

    fn make_rule_service() -> (CsaRuleService, Arc<MockCsaRuleRepo>) {
        let repo = Arc::new(MockCsaRuleRepo::new());
        let audit = make_audit();
        let svc = CsaRuleService::new(repo.clone(), audit);
        (svc, repo)
    }

    fn make_assessment_service(repo: Arc<MockCsaRuleRepo>) -> CsaAssessmentService {
        let audit = make_audit();
        CsaAssessmentService::new(repo, audit)
    }

    fn make_csa_rule(
        tenant_id: TenantId,
        expression: &str,
        action: CsaAction,
        severity: Severity,
    ) -> CsaRule {
        CsaRule {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            rule_expression: expression.into(),
            action,
            severity,
            version: 1,
            is_active: true,
            created_at: Utc::now(),
        }
    }

    // -- SR_GOV_23 Tests: Rule Registration -----------------------------------

    #[tokio::test]
    async fn register_valid_rule() {
        let (svc, repo) = make_rule_service();
        let request = CsaRuleRegistration {
            tenant_id: TenantId::new(),
            rule_expression: "pii + financial = high".into(),
            action: CsaAction::Block,
            severity: Severity::High,
            dry_run_sample_size: None,
        };

        let result = svc.register_rule(&request).await.unwrap();
        assert!(result.active);
        assert_eq!(result.version, 1);

        let rules = repo.rules.lock().unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[tokio::test]
    async fn reject_invalid_expression() {
        let (svc, _repo) = make_rule_service();
        let request = CsaRuleRegistration {
            tenant_id: TenantId::new(),
            rule_expression: "pii + financial".into(), // missing '='
            action: CsaAction::Block,
            severity: Severity::High,
            dry_run_sample_size: None,
        };

        let err = svc.register_rule(&request).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'='"));
    }

    #[tokio::test]
    async fn reject_unknown_attribute() {
        let (svc, _repo) = make_rule_service();
        let request = CsaRuleRegistration {
            tenant_id: TenantId::new(),
            rule_expression: "pii + secret_sauce = high".into(),
            action: CsaAction::Block,
            severity: Severity::High,
            dry_run_sample_size: None,
        };

        let err = svc.register_rule(&request).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown attribute"));
        assert!(msg.contains("secret_sauce"));
    }

    #[tokio::test]
    async fn reject_empty_expression() {
        let (svc, _repo) = make_rule_service();
        let request = CsaRuleRegistration {
            tenant_id: TenantId::new(),
            rule_expression: "".into(),
            action: CsaAction::Block,
            severity: Severity::High,
            dry_run_sample_size: None,
        };

        let err = svc.register_rule(&request).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("empty"));
    }

    // -- SR_GOV_25 Tests: CSA Evaluator ---------------------------------------

    #[test]
    fn no_rules_match_returns_none() {
        let rules = vec![make_csa_rule(
            TenantId::new(),
            "pii + financial = high",
            CsaAction::Block,
            Severity::High,
        )];
        let attrs: HashSet<String> = ["location"].iter().map(|s| s.to_string()).collect();

        let output = CsaEvaluator::evaluate(&rules, &attrs, None, None);
        assert!(output.matched_rules.is_empty());
        assert!(output.highest_action.is_none());
    }

    #[test]
    fn single_rule_matches() {
        let rules = vec![make_csa_rule(
            TenantId::new(),
            "pii + financial = high",
            CsaAction::Block,
            Severity::High,
        )];
        let attrs: HashSet<String> = ["pii", "financial"].iter().map(|s| s.to_string()).collect();

        let output = CsaEvaluator::evaluate(&rules, &attrs, None, None);
        assert_eq!(output.matched_rules.len(), 1);
        assert_eq!(output.highest_action, Some(CsaAction::Block));
    }

    #[test]
    fn multiple_rules_highest_severity_wins() {
        let tenant_id = TenantId::new();
        let rules = vec![
            make_csa_rule(
                tenant_id,
                "pii + financial = medium",
                CsaAction::Anonymize,
                Severity::Medium,
            ),
            make_csa_rule(
                tenant_id,
                "pii + phi = critical",
                CsaAction::Block,
                Severity::Critical,
            ),
        ];
        let attrs: HashSet<String> = ["pii", "financial", "phi"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let output = CsaEvaluator::evaluate(&rules, &attrs, None, None);
        assert_eq!(output.matched_rules.len(), 2);
        // The Critical-severity rule's action should be highest
        assert_eq!(output.highest_action, Some(CsaAction::Block));
    }

    #[test]
    fn partial_attribute_match_does_not_fire() {
        let rules = vec![make_csa_rule(
            TenantId::new(),
            "pii + financial + phi = critical",
            CsaAction::Block,
            Severity::Critical,
        )];
        // Only 2 of 3 attributes present
        let attrs: HashSet<String> = ["pii", "financial"].iter().map(|s| s.to_string()).collect();

        let output = CsaEvaluator::evaluate(&rules, &attrs, None, None);
        assert!(output.matched_rules.is_empty());
        assert!(output.highest_action.is_none());
    }

    // -- SR_GOV_24 Tests: CSA Assessment Trigger ------------------------------

    #[tokio::test]
    async fn allow_when_no_rules_match() {
        let repo = Arc::new(MockCsaRuleRepo::new());
        let tenant_id = TenantId::new();
        // Seed a rule that won't match the attribute set
        repo.seed(make_csa_rule(
            tenant_id,
            "phi + cui = critical",
            CsaAction::Block,
            Severity::Critical,
        ));

        let svc = make_assessment_service(repo);
        let request = CsaAssessmentRequest {
            tenant_id,
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into(), "coll_b".into()],
            combined_attribute_set: ["pii", "financial"].iter().map(|s| s.to_string()).collect(),
            query_purpose: None,
        };

        let result = svc.assess(&request).await.unwrap();
        assert_eq!(result.decision, CsaDecision::Allow);
        assert!(result.applied_rules.is_empty());
        assert!(result.required_action.is_none());
    }

    #[tokio::test]
    async fn block_when_enforce_rule_matches() {
        let repo = Arc::new(MockCsaRuleRepo::new());
        let tenant_id = TenantId::new();
        repo.seed(make_csa_rule(
            tenant_id,
            "pii + financial = high",
            CsaAction::Block,
            Severity::High,
        ));

        let svc = make_assessment_service(repo);
        let request = CsaAssessmentRequest {
            tenant_id,
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into(), "coll_b".into()],
            combined_attribute_set: ["pii", "financial"].iter().map(|s| s.to_string()).collect(),
            query_purpose: None,
        };

        let result = svc.assess(&request).await.unwrap();
        assert_eq!(result.decision, CsaDecision::Block);
        assert_eq!(result.required_action, Some(CsaAction::Block));
        assert_eq!(result.applied_rules.len(), 1);
    }

    #[tokio::test]
    async fn skips_assessment_for_single_data_source() {
        let repo = Arc::new(MockCsaRuleRepo::new());
        let tenant_id = TenantId::new();
        repo.seed(make_csa_rule(
            tenant_id,
            "pii + financial = high",
            CsaAction::Block,
            Severity::High,
        ));

        let svc = make_assessment_service(repo);
        let request = CsaAssessmentRequest {
            tenant_id,
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into()], // only 1
            combined_attribute_set: ["pii", "financial"].iter().map(|s| s.to_string()).collect(),
            query_purpose: None,
        };

        let result = svc.assess(&request).await.unwrap();
        assert_eq!(result.decision, CsaDecision::Allow);
        assert!(result.applied_rules.is_empty());
    }

    #[tokio::test]
    async fn returns_highest_severity_action() {
        let repo = Arc::new(MockCsaRuleRepo::new());
        let tenant_id = TenantId::new();
        repo.seed(make_csa_rule(
            tenant_id,
            "pii + location = low",
            CsaAction::Elevate,
            Severity::Low,
        ));
        repo.seed(make_csa_rule(
            tenant_id,
            "pii + financial = high",
            CsaAction::Block,
            Severity::High,
        ));

        let svc = make_assessment_service(repo);
        let request = CsaAssessmentRequest {
            tenant_id,
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into(), "coll_b".into()],
            combined_attribute_set: ["pii", "financial", "location"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            query_purpose: None,
        };

        let result = svc.assess(&request).await.unwrap();
        // High severity rule should win over Low
        assert_eq!(result.required_action, Some(CsaAction::Block));
        assert_eq!(result.applied_rules.len(), 2);
    }

    // -- SR_GOV_26 Tests: CSA Block Handler -----------------------------------

    #[tokio::test]
    async fn block_returns_rejection_with_alternatives() {
        let audit = make_audit();
        let handler = CsaBlockHandler::new(audit);

        let action = CsaBlockAction {
            assessment_id: uuid::Uuid::new_v4(),
            reason: "PII + Financial combined risk too high".into(),
            suggested_alternatives: vec![
                "Query each collection separately".into(),
                "Request elevated access".into(),
            ],
        };

        let result = handler.handle_block(&action).await.unwrap();
        assert!(result.rejected);
        assert_eq!(result.alternatives.len(), 2);
    }

    #[tokio::test]
    async fn block_with_empty_alternatives() {
        let audit = make_audit();
        let handler = CsaBlockHandler::new(audit);

        let action = CsaBlockAction {
            assessment_id: uuid::Uuid::new_v4(),
            reason: "Absolute block -- no alternatives available".into(),
            suggested_alternatives: Vec::new(),
        };

        let result = handler.handle_block(&action).await.unwrap();
        assert!(result.rejected);
        assert!(result.alternatives.is_empty());
    }
}
