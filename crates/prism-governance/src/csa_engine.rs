//! Cross-System Aggregation (CSA) engine (SR_GOV_23 -- SR_GOV_30).
//!
//! Implements the CSA rule lifecycle:
//! - SR_GOV_23: Rule registration with expression parsing and validation
//! - SR_GOV_24: Assessment trigger composing rule loading + evaluation + audit
//! - SR_GOV_25: Pure-function evaluator (no I/O)
//! - SR_GOV_26: BLOCK action handler
//! - SR_GOV_27: ANONYMIZE action handler
//! - SR_GOV_28: ELEVATE action handler
//! - SR_GOV_29: Break-glass activation and review
//! - SR_GOV_30: Assessment persistence
//!
//! CSA rules fire when multiple data collections are combined and the
//! combined attribute set matches a rule expression. The expression grammar
//! is `ATTR1 + ATTR2 = SEVERITY`.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{Duration, Utc};
use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::{BreakGlassRepository, CsaAssessmentRepository, CsaRuleRepository};
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
// SR_GOV_27 -- CSA ANONYMIZE Handler
// ===========================================================================

/// Trait for pluggable anonymization strategies.
///
/// Implementations provide the actual anonymization algorithm (k-anonymity,
/// differential privacy, etc.). The CSA engine orchestrates via this trait.
///
/// Implements: SR_GOV_27
#[async_trait::async_trait]
pub trait AnonymizationFunction: Send + Sync {
    /// Anonymize the given data references using the specified parameters.
    ///
    /// Implements: SR_GOV_27
    async fn anonymize(
        &self,
        data: &[String],
        k_anonymity: u32,
        strategy: &str,
    ) -> Result<AnonymizedPayload, PrismError>;
}

/// Output of an anonymization operation.
///
/// Implements: SR_GOV_27
#[derive(Debug, Clone)]
pub struct AnonymizedPayload {
    pub payload: serde_json::Value,
    pub parameters: String,
    pub residual_risk: f64,
}

/// Handler for CSA ANONYMIZE actions.
///
/// Composes:
/// - `AnonymizationFunction` -- pluggable anonymization strategy
/// - `AuditLogger` -- audit trail for anonymize events
///
/// Implements: SR_GOV_27
pub struct CsaAnonymizeHandler {
    anonymizer: Arc<dyn AnonymizationFunction>,
    audit: AuditLogger,
}

impl CsaAnonymizeHandler {
    /// Create a new CSA anonymize handler.
    pub fn new(anonymizer: Arc<dyn AnonymizationFunction>, audit: AuditLogger) -> Self {
        Self { anonymizer, audit }
    }

    /// Handle a CSA anonymize action.
    ///
    /// Delegates to the `AnonymizationFunction` trait for the actual
    /// anonymization, records the result and residual risk in the audit trail.
    ///
    /// Implements: SR_GOV_27
    pub async fn handle_anonymize(
        &self,
        action: &CsaAnonymizeAction,
    ) -> Result<CsaAnonymizeResult, PrismError> {
        let result = self
            .anonymizer
            .anonymize(
                &action.data_collection_refs,
                action.target_k_anonymity,
                &action.aggregation_strategy,
            )
            .await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: TenantId::from_uuid(uuid::Uuid::nil()),
                event_type: "csa.anonymized".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(action.assessment_id),
                target_type: Some("CsaAssessment".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "k_anonymity": action.target_k_anonymity,
                    "strategy": action.aggregation_strategy,
                    "residual_risk": result.residual_risk,
                    "parameters_applied": result.parameters,
                }),
            })
            .await?;

        info!(
            assessment_id = %action.assessment_id,
            k_anonymity = action.target_k_anonymity,
            residual_risk = result.residual_risk,
            "CSA ANONYMIZE action executed"
        );

        Ok(CsaAnonymizeResult {
            anonymized_payload: result.payload,
            parameters_applied: result.parameters,
            residual_risk_score: result.residual_risk,
        })
    }
}

// ===========================================================================
// SR_GOV_28 -- CSA ELEVATE Handler
// ===========================================================================

/// Handler for CSA ELEVATE actions.
///
/// Returns the required permission and the path to request elevation.
///
/// Implements: SR_GOV_28
pub struct CsaElevateHandler {
    audit: AuditLogger,
}

impl CsaElevateHandler {
    /// Create a new CSA elevate handler.
    pub fn new(audit: AuditLogger) -> Self {
        Self { audit }
    }

    /// Handle a CSA elevate action.
    ///
    /// Returns the required permission and the request path. Does not
    /// grant the permission -- the caller must follow the request_path.
    ///
    /// Implements: SR_GOV_28
    pub async fn handle_elevate(
        &self,
        action: &CsaElevateAction,
    ) -> Result<CsaElevateResult, PrismError> {
        let request_path = if action.justification_required {
            format!("/governance/elevate/{}/justify", action.required_permission)
        } else {
            format!("/governance/elevate/{}/request", action.required_permission)
        };

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: TenantId::from_uuid(uuid::Uuid::nil()),
                event_type: "csa.elevation_required".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(action.assessment_id),
                target_type: Some("CsaAssessment".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "required_permission": action.required_permission,
                    "justification_required": action.justification_required,
                    "request_path": request_path,
                }),
            })
            .await?;

        info!(
            assessment_id = %action.assessment_id,
            required_permission = %action.required_permission,
            justification_required = action.justification_required,
            "CSA ELEVATE action executed"
        );

        Ok(CsaElevateResult {
            required_permission: action.required_permission.clone(),
            request_path,
        })
    }
}

// ===========================================================================
// SR_GOV_29 -- CSA Break-Glass
// ===========================================================================

/// Default break-glass duration in minutes per BP-133.
const DEFAULT_BREAK_GLASS_DURATION_MINUTES: u64 = 240;

/// Minimum justification length for break-glass activations.
const BREAK_GLASS_MIN_JUSTIFICATION_LEN: usize = 20;

/// Service for emergency break-glass activations and reviews.
///
/// Composes:
/// - `BreakGlassRepository` -- persistence for activations
/// - `AuditLogger` -- CRITICAL-severity audit trail
///
/// Implements: SR_GOV_29
pub struct CsaBreakGlassService {
    repo: Arc<dyn BreakGlassRepository>,
    audit: AuditLogger,
}

impl CsaBreakGlassService {
    /// Create a new break-glass service.
    pub fn new(repo: Arc<dyn BreakGlassRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Activate a break-glass override.
    ///
    /// Validates:
    /// - Two-person rule (approver_1 != approver_2)
    /// - Justification is non-empty and >= 20 characters
    ///
    /// Default duration is 240 minutes (4 hours) per BP-133.
    ///
    /// Implements: SR_GOV_29
    pub async fn activate(
        &self,
        request: &CsaBreakGlassRequest,
    ) -> Result<CsaBreakGlassResult, PrismError> {
        // Validate two-person rule
        if request.approver_1 == request.approver_2 {
            return Err(PrismError::Validation {
                reason: "break-glass requires two distinct approvers (two-person rule)".into(),
            });
        }

        // Validate justification
        let justification = request.justification.trim();
        if justification.is_empty() {
            return Err(PrismError::Validation {
                reason: "break-glass justification must not be empty".into(),
            });
        }
        if justification.len() < BREAK_GLASS_MIN_JUSTIFICATION_LEN {
            return Err(PrismError::Validation {
                reason: format!(
                    "break-glass justification must be at least {} characters (got {})",
                    BREAK_GLASS_MIN_JUSTIFICATION_LEN,
                    justification.len()
                ),
            });
        }

        let duration_minutes = request
            .duration_minutes
            .unwrap_or(DEFAULT_BREAK_GLASS_DURATION_MINUTES);
        let now = Utc::now();
        let expires_at = now + Duration::minutes(duration_minutes as i64);
        let review_id = uuid::Uuid::now_v7();

        let activation = BreakGlassActivation {
            id: uuid::Uuid::now_v7(),
            assessment_id: request.assessment_id,
            tenant_id: request.tenant_id,
            justification: justification.to_string(),
            approver_1: request.approver_1,
            approver_2: request.approver_2,
            duration_minutes,
            activated_at: now,
            expires_at,
            review_id,
            is_reviewed: false,
        };

        self.repo.record_activation(&activation).await?;

        // Audit event at CRITICAL severity
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "csa.break_glass_activated".into(),
                actor_id: *request.approver_1.as_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(request.assessment_id),
                target_type: Some("CsaAssessment".into()),
                severity: Severity::Critical,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "assessment_id": request.assessment_id.to_string(),
                    "approver_1": request.approver_1.to_string(),
                    "approver_2": request.approver_2.to_string(),
                    "duration_minutes": duration_minutes,
                    "review_id": review_id.to_string(),
                }),
            })
            .await?;

        warn!(
            tenant_id = %request.tenant_id,
            assessment_id = %request.assessment_id,
            duration_minutes = duration_minutes,
            "CSA BREAK-GLASS activated -- mandatory review required"
        );

        Ok(CsaBreakGlassResult {
            authorized: true,
            expires_at,
            review_id,
        })
    }

    /// Review a break-glass activation.
    ///
    /// Follow-ups are generated based on the review decision:
    /// - Justified: no follow-ups
    /// - Unjustified: security review with user
    /// - NeedsRuleRefinement: CSA rule review
    ///
    /// Implements: SR_GOV_29
    pub async fn review(
        &self,
        input: &BreakGlassReviewInput,
    ) -> Result<BreakGlassReviewResult, PrismError> {
        // Verify the activation exists
        let activation = self.repo.get_by_review_id(input.review_id).await?;
        let activation = activation.ok_or(PrismError::NotFound {
            entity_type: "BreakGlassActivation",
            id: input.review_id,
        })?;

        if activation.is_reviewed {
            return Err(PrismError::Conflict {
                reason: "break-glass activation has already been reviewed".into(),
            });
        }

        // Mark as reviewed
        self.repo.mark_reviewed(input.review_id).await?;

        // Determine follow-ups
        let follow_ups = match input.review_decision {
            BreakGlassReviewDecision::Justified => Vec::new(),
            BreakGlassReviewDecision::Unjustified => {
                vec!["security_review_with_user".to_string()]
            }
            BreakGlassReviewDecision::NeedsRuleRefinement => {
                vec!["csa_rule_review".to_string()]
            }
        };

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "csa.break_glass_reviewed".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::Human,
                target_id: Some(input.review_id),
                target_type: Some("BreakGlassActivation".into()),
                severity: Severity::High,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "review_decision": format!("{:?}", input.review_decision),
                    "notes": input.notes,
                    "follow_ups": follow_ups,
                }),
            })
            .await?;

        info!(
            review_id = %input.review_id,
            decision = ?input.review_decision,
            follow_ups = ?follow_ups,
            "Break-glass activation reviewed"
        );

        Ok(BreakGlassReviewResult {
            review_decision: input.review_decision,
            follow_ups,
        })
    }
}

// ===========================================================================
// SR_GOV_30 -- CSA Assessment Persistence
// ===========================================================================

/// Service for persisting CSA assessment records to the governance graph.
///
/// Composes:
/// - `CsaAssessmentRepository` -- persistence for assessment records
/// - `AuditLogger` -- audit trail for persistence events
///
/// Implements: SR_GOV_30
pub struct CsaAssessmentPersistService {
    repo: Arc<dyn CsaAssessmentRepository>,
    audit: AuditLogger,
}

impl CsaAssessmentPersistService {
    /// Create a new assessment persistence service.
    pub fn new(repo: Arc<dyn CsaAssessmentRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Persist a CSA assessment record.
    ///
    /// Creates a record with all assessment details and emits an audit event.
    ///
    /// Implements: SR_GOV_30
    pub async fn persist(
        &self,
        input: &CsaAssessmentPersistInput,
    ) -> Result<CsaAssessmentPersistResult, PrismError> {
        let record = CsaAssessmentRecord {
            id: input.assessment_id,
            tenant_id: input.tenant_id,
            query_id: input.query_id,
            data_collection_refs: input.data_collection_refs.clone(),
            decision: input.decision,
            applied_rules: input.applied_rules.clone(),
            created_at: Utc::now(),
        };

        self.repo.persist(&record).await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "csa.assessment_persisted".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.assessment_id),
                target_type: Some("CsaAssessmentRecord".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "query_id": input.query_id.to_string(),
                    "decision": format!("{:?}", input.decision),
                    "applied_rules_count": input.applied_rules.len(),
                    "data_collection_count": input.data_collection_refs.len(),
                }),
            })
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            assessment_id = %input.assessment_id,
            decision = ?input.decision,
            "CSA assessment persisted"
        );

        Ok(CsaAssessmentPersistResult {
            node_id: input.assessment_id,
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

    // -- Mock AnonymizationFunction -------------------------------------------

    struct MockAnonymizer;

    #[async_trait]
    impl AnonymizationFunction for MockAnonymizer {
        async fn anonymize(
            &self,
            data: &[String],
            k_anonymity: u32,
            strategy: &str,
        ) -> Result<AnonymizedPayload, PrismError> {
            Ok(AnonymizedPayload {
                payload: serde_json::json!({
                    "anonymized_refs": data,
                    "k": k_anonymity,
                }),
                parameters: format!("k={},strategy={}", k_anonymity, strategy),
                residual_risk: 0.15,
            })
        }
    }

    // -- Mock BreakGlassRepository --------------------------------------------

    struct MockBreakGlassRepo {
        activations: Mutex<Vec<BreakGlassActivation>>,
    }

    impl MockBreakGlassRepo {
        fn new() -> Self {
            Self {
                activations: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl BreakGlassRepository for MockBreakGlassRepo {
        async fn record_activation(
            &self,
            activation: &BreakGlassActivation,
        ) -> Result<(), PrismError> {
            self.activations.lock().unwrap().push(activation.clone());
            Ok(())
        }

        async fn get_by_review_id(
            &self,
            review_id: uuid::Uuid,
        ) -> Result<Option<BreakGlassActivation>, PrismError> {
            let activations = self.activations.lock().unwrap();
            Ok(activations
                .iter()
                .find(|a| a.review_id == review_id)
                .cloned())
        }

        async fn mark_reviewed(&self, review_id: uuid::Uuid) -> Result<(), PrismError> {
            let mut activations = self.activations.lock().unwrap();
            if let Some(a) = activations.iter_mut().find(|a| a.review_id == review_id) {
                a.is_reviewed = true;
            }
            Ok(())
        }
    }

    // -- Mock CsaAssessmentRepository -----------------------------------------

    struct MockCsaAssessmentRepo {
        records: Mutex<Vec<CsaAssessmentRecord>>,
    }

    impl MockCsaAssessmentRepo {
        fn new() -> Self {
            Self {
                records: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl CsaAssessmentRepository for MockCsaAssessmentRepo {
        async fn persist(&self, record: &CsaAssessmentRecord) -> Result<(), PrismError> {
            self.records.lock().unwrap().push(record.clone());
            Ok(())
        }
    }

    // -- SR_GOV_27 Tests: CSA ANONYMIZE Handler --------------------------------

    #[tokio::test]
    async fn anonymize_succeeds_with_parameters() {
        let anonymizer = Arc::new(MockAnonymizer);
        let audit = make_audit();
        let handler = CsaAnonymizeHandler::new(anonymizer, audit);

        let action = CsaAnonymizeAction {
            assessment_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into(), "coll_b".into()],
            target_k_anonymity: 5,
            aggregation_strategy: "suppression".into(),
        };

        let result = handler.handle_anonymize(&action).await.unwrap();
        assert!(result.parameters_applied.contains("k=5"));
        assert!(result.parameters_applied.contains("suppression"));
        assert!(!result.anonymized_payload.is_null());
    }

    #[tokio::test]
    async fn anonymize_records_residual_risk() {
        let anonymizer = Arc::new(MockAnonymizer);
        let audit = make_audit();
        let handler = CsaAnonymizeHandler::new(anonymizer, audit);

        let action = CsaAnonymizeAction {
            assessment_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into()],
            target_k_anonymity: 3,
            aggregation_strategy: "generalization".into(),
        };

        let result = handler.handle_anonymize(&action).await.unwrap();
        assert!(result.residual_risk_score > 0.0);
        assert!(result.residual_risk_score < 1.0);
    }

    // -- SR_GOV_28 Tests: CSA ELEVATE Handler ----------------------------------

    #[tokio::test]
    async fn elevate_returns_permission_info() {
        let audit = make_audit();
        let handler = CsaElevateHandler::new(audit);

        let action = CsaElevateAction {
            assessment_id: uuid::Uuid::new_v4(),
            required_permission: "data.cross_system_read".into(),
            justification_required: false,
        };

        let result = handler.handle_elevate(&action).await.unwrap();
        assert_eq!(result.required_permission, "data.cross_system_read");
        assert!(result.request_path.contains("data.cross_system_read"));
        assert!(result.request_path.contains("/request"));
    }

    #[tokio::test]
    async fn elevate_with_justification_required() {
        let audit = make_audit();
        let handler = CsaElevateHandler::new(audit);

        let action = CsaElevateAction {
            assessment_id: uuid::Uuid::new_v4(),
            required_permission: "data.pii_access".into(),
            justification_required: true,
        };

        let result = handler.handle_elevate(&action).await.unwrap();
        assert_eq!(result.required_permission, "data.pii_access");
        assert!(result.request_path.contains("/justify"));
    }

    // -- SR_GOV_29 Tests: CSA Break-Glass --------------------------------------

    fn make_break_glass_service() -> (CsaBreakGlassService, Arc<MockBreakGlassRepo>) {
        let repo = Arc::new(MockBreakGlassRepo::new());
        let audit = make_audit();
        let svc = CsaBreakGlassService::new(repo.clone(), audit);
        (svc, repo)
    }

    #[tokio::test]
    async fn break_glass_activation_succeeds_with_defaults() {
        let (svc, repo) = make_break_glass_service();
        let tenant_id = TenantId::new();

        let request = CsaBreakGlassRequest {
            tenant_id,
            assessment_id: uuid::Uuid::new_v4(),
            justification: "Emergency access needed for production incident remediation workflow"
                .into(),
            approver_1: UserId::new(),
            approver_2: UserId::new(),
            duration_minutes: None,
        };

        let result = svc.activate(&request).await.unwrap();
        assert!(result.authorized);

        // Verify default duration was applied (expires_at ~ now + 240min)
        let activations = repo.activations.lock().unwrap();
        assert_eq!(activations.len(), 1);
        assert_eq!(activations[0].duration_minutes, 240);
    }

    #[tokio::test]
    async fn break_glass_activation_custom_duration() {
        let (svc, repo) = make_break_glass_service();
        let tenant_id = TenantId::new();

        let request = CsaBreakGlassRequest {
            tenant_id,
            assessment_id: uuid::Uuid::new_v4(),
            justification: "Regulatory deadline requires extended access for data remediation"
                .into(),
            approver_1: UserId::new(),
            approver_2: UserId::new(),
            duration_minutes: Some(60),
        };

        let result = svc.activate(&request).await.unwrap();
        assert!(result.authorized);

        let activations = repo.activations.lock().unwrap();
        assert_eq!(activations[0].duration_minutes, 60);
    }

    #[tokio::test]
    async fn break_glass_rejects_same_approver() {
        let (svc, _) = make_break_glass_service();
        let tenant_id = TenantId::new();
        let same_user = UserId::new();

        let request = CsaBreakGlassRequest {
            tenant_id,
            assessment_id: uuid::Uuid::new_v4(),
            justification: "Emergency access needed for production incident remediation workflow"
                .into(),
            approver_1: same_user,
            approver_2: same_user,
            duration_minutes: None,
        };

        let err = svc.activate(&request).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("two-person rule") || msg.contains("distinct approvers"));
    }

    #[tokio::test]
    async fn break_glass_rejects_empty_justification() {
        let (svc, _) = make_break_glass_service();
        let tenant_id = TenantId::new();

        let request = CsaBreakGlassRequest {
            tenant_id,
            assessment_id: uuid::Uuid::new_v4(),
            justification: "".into(),
            approver_1: UserId::new(),
            approver_2: UserId::new(),
            duration_minutes: None,
        };

        let err = svc.activate(&request).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("empty"));
    }

    // -- SR_GOV_29_REVIEW Tests: Break-Glass Review ----------------------------

    async fn make_activation_for_review(svc: &CsaBreakGlassService) -> (TenantId, uuid::Uuid) {
        let tenant_id = TenantId::new();
        let request = CsaBreakGlassRequest {
            tenant_id,
            assessment_id: uuid::Uuid::new_v4(),
            justification: "Emergency access needed for production incident remediation workflow"
                .into(),
            approver_1: UserId::new(),
            approver_2: UserId::new(),
            duration_minutes: None,
        };
        let result = svc.activate(&request).await.unwrap();
        (tenant_id, result.review_id)
    }

    #[tokio::test]
    async fn break_glass_review_justified() {
        let (svc, _) = make_break_glass_service();
        let (tenant_id, review_id) = make_activation_for_review(&svc).await;

        let input = BreakGlassReviewInput {
            review_id,
            tenant_id,
            review_decision: BreakGlassReviewDecision::Justified,
            notes: "Verified incident required emergency access".into(),
        };

        let result = svc.review(&input).await.unwrap();
        assert_eq!(result.review_decision, BreakGlassReviewDecision::Justified);
        assert!(result.follow_ups.is_empty());
    }

    #[tokio::test]
    async fn break_glass_review_unjustified_triggers_security_review() {
        let (svc, _) = make_break_glass_service();
        let (tenant_id, review_id) = make_activation_for_review(&svc).await;

        let input = BreakGlassReviewInput {
            review_id,
            tenant_id,
            review_decision: BreakGlassReviewDecision::Unjustified,
            notes: "No evidence of actual incident found".into(),
        };

        let result = svc.review(&input).await.unwrap();
        assert_eq!(
            result.review_decision,
            BreakGlassReviewDecision::Unjustified
        );
        assert!(result
            .follow_ups
            .contains(&"security_review_with_user".to_string()));
    }

    #[tokio::test]
    async fn break_glass_review_needs_rule_refinement_triggers_rule_review() {
        let (svc, _) = make_break_glass_service();
        let (tenant_id, review_id) = make_activation_for_review(&svc).await;

        let input = BreakGlassReviewInput {
            review_id,
            tenant_id,
            review_decision: BreakGlassReviewDecision::NeedsRuleRefinement,
            notes: "CSA rule is too broad for this use case".into(),
        };

        let result = svc.review(&input).await.unwrap();
        assert_eq!(
            result.review_decision,
            BreakGlassReviewDecision::NeedsRuleRefinement
        );
        assert!(result.follow_ups.contains(&"csa_rule_review".to_string()));
    }

    // -- SR_GOV_30 Tests: CSA Assessment Persistence ---------------------------

    fn make_persist_service() -> (CsaAssessmentPersistService, Arc<MockCsaAssessmentRepo>) {
        let repo = Arc::new(MockCsaAssessmentRepo::new());
        let audit = make_audit();
        let svc = CsaAssessmentPersistService::new(repo.clone(), audit);
        (svc, repo)
    }

    #[tokio::test]
    async fn persist_assessment_succeeds() {
        let (svc, repo) = make_persist_service();
        let tenant_id = TenantId::new();
        let assessment_id = uuid::Uuid::new_v4();

        let input = CsaAssessmentPersistInput {
            tenant_id,
            assessment_id,
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into(), "coll_b".into()],
            decision: CsaDecision::Block,
            applied_rules: vec!["rule_1".into()],
        };

        let result = svc.persist(&input).await.unwrap();
        assert_eq!(result.node_id, assessment_id);

        let records = repo.records.lock().unwrap();
        assert_eq!(records.len(), 1);
    }

    #[tokio::test]
    async fn persist_assessment_records_all_fields() {
        let (svc, repo) = make_persist_service();
        let tenant_id = TenantId::new();
        let assessment_id = uuid::Uuid::new_v4();
        let query_id = uuid::Uuid::new_v4();

        let input = CsaAssessmentPersistInput {
            tenant_id,
            assessment_id,
            query_id,
            data_collection_refs: vec!["coll_a".into(), "coll_b".into(), "coll_c".into()],
            decision: CsaDecision::Anonymize,
            applied_rules: vec!["rule_1".into(), "rule_2".into()],
        };

        svc.persist(&input).await.unwrap();

        let records = repo.records.lock().unwrap();
        let record = &records[0];
        assert_eq!(record.id, assessment_id);
        assert_eq!(record.tenant_id, tenant_id);
        assert_eq!(record.query_id, query_id);
        assert_eq!(record.data_collection_refs.len(), 3);
        assert_eq!(record.decision, CsaDecision::Anonymize);
        assert_eq!(record.applied_rules.len(), 2);
    }
}
