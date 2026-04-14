//! Governance integration points (SR_GOV_73, SR_GOV_74, SR_GOV_75).
//!
//! These services compose governance checks at key integration boundaries:
//! - SR_GOV_73: LLM Router Stage 1 -- model filtering via ENFORCE rules
//! - SR_GOV_74: Decision Support preflight -- multi-source CSA checks
//! - SR_GOV_75: UI visibility -- element-level access control

use std::sync::Arc;

use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::GovernanceRuleRepository;
use prism_core::types::*;

// ===========================================================================
// SR_GOV_73 -- LLM Router Stage 1
// ===========================================================================

/// Default model list available for routing before governance filtering.
const DEFAULT_MODELS: &[&str] = &["claude-sonnet", "claude-haiku", "gpt-4o", "gpt-4o-mini"];

/// Service that evaluates governance rules to filter the set of allowed
/// LLM models for a given request context.
///
/// Uses the SR_GOV_16 pattern: loads ENFORCE rules for the "llm.route"
/// action and removes any model that is explicitly denied.
///
/// Implements: SR_GOV_73
pub struct RouterStage1Service {
    rules_repo: Arc<dyn GovernanceRuleRepository>,
    audit: AuditLogger,
}

impl RouterStage1Service {
    /// Create a new Router Stage 1 service.
    pub fn new(rules_repo: Arc<dyn GovernanceRuleRepository>, audit: AuditLogger) -> Self {
        Self { rules_repo, audit }
    }

    /// Evaluate governance rules to determine which LLM models are allowed.
    ///
    /// Loads ENFORCE rules for the "llm.route" action, then filters the
    /// default model list. A rule blocks a model when its condition contains
    /// `"blocked_model": "<model_name>"` and the condition otherwise matches.
    ///
    /// Implements: SR_GOV_73
    pub async fn evaluate(
        &self,
        input: &RouterStage1Input,
    ) -> Result<RouterStage1Result, PrismError> {
        let rules = self
            .rules_repo
            .list_active_rules(input.tenant_id, "llm.route", RuleClass::Enforce)
            .await?;

        let mut allowed: Vec<String> = DEFAULT_MODELS.iter().map(|s| s.to_string()).collect();
        let mut reasoning: Vec<String> = Vec::new();

        for rule in &rules {
            if let Some(blocked_model) =
                rule.condition.get("blocked_model").and_then(|v| v.as_str())
            {
                let before_len = allowed.len();
                allowed.retain(|m| m != blocked_model);
                if allowed.len() < before_len {
                    reasoning.push(format!(
                        "model '{}' blocked by rule '{}'",
                        blocked_model, rule.name
                    ));
                }
            }
        }

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "router.stage1_evaluated".into(),
                actor_id: input.principal_id,
                actor_type: ActorType::System,
                target_id: None,
                target_type: None,
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "allowed_models_count": allowed.len(),
                    "rules_evaluated": rules.len(),
                    "reasoning": reasoning,
                }),
            })
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            allowed_count = allowed.len(),
            rules_count = rules.len(),
            "Router Stage 1 evaluated"
        );

        Ok(RouterStage1Result {
            allowed_models: allowed,
            reasoning,
        })
    }
}

// ===========================================================================
// SR_GOV_74 -- Decision Support Preflight
// ===========================================================================

/// Service that performs preflight checks before decision-support queries.
///
/// For MVP: blocks if data_collection_refs >= 2 (multi-source) and
/// no CSA assessment clearance flag is present in parameter_overrides.
///
/// Implements: SR_GOV_74
pub struct DecisionSupportPreflightService {
    audit: AuditLogger,
}

impl DecisionSupportPreflightService {
    /// Create a new decision-support preflight service.
    pub fn new(audit: AuditLogger) -> Self {
        Self { audit }
    }

    /// Check whether a decision-support query is allowed to proceed.
    ///
    /// Multi-source queries (>= 2 data_collection_refs) require a CSA
    /// assessment clearance flag ("csa_cleared") in parameter_overrides.
    ///
    /// Implements: SR_GOV_74
    pub async fn check(
        &self,
        input: &DecisionSupportPreflightInput,
    ) -> Result<DecisionSupportPreflightResult, PrismError> {
        let mut blocked_reasons: Vec<String> = Vec::new();

        // Multi-source check
        if input.data_collection_refs.len() >= 2 {
            let has_csa_clearance = input.parameter_overrides.iter().any(|p| p == "csa_cleared");

            if !has_csa_clearance {
                blocked_reasons
                    .push("multi-source query requires CSA assessment clearance".to_string());
            }
        }

        let allowed = blocked_reasons.is_empty();

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "ds.preflight_evaluated".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.query_id),
                target_type: Some("Query".into()),
                severity: if allowed {
                    Severity::Low
                } else {
                    Severity::Medium
                },
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "allowed": allowed,
                    "data_collection_count": input.data_collection_refs.len(),
                    "blocked_reasons": blocked_reasons,
                }),
            })
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            query_id = %input.query_id,
            allowed = allowed,
            "Decision support preflight evaluated"
        );

        Ok(DecisionSupportPreflightResult {
            allowed,
            blocked_reasons,
        })
    }
}

// ===========================================================================
// SR_GOV_75 -- UI Visibility Check
// ===========================================================================

/// Service that determines UI element visibility based on principal roles.
///
/// Logic:
/// - Elements with "admin_" prefix require an "admin" role; hidden otherwise.
/// - Elements with "readonly_" prefix are ReadOnly for non-admins.
/// - All other elements are Visible.
///
/// Implements: SR_GOV_75
pub struct UiVisibilityService;

impl UiVisibilityService {
    /// Check the visibility of a UI element for a given principal.
    ///
    /// Implements: SR_GOV_75
    pub fn check(input: &UiVisibilityCheck) -> UiVisibilityResult {
        let is_admin = input.principal_roles.iter().any(|r| r == "admin");

        let decision = if input.ui_element_id.starts_with("admin_") {
            if is_admin {
                UiVisibility::Visible
            } else {
                UiVisibility::Hidden
            }
        } else if input.ui_element_id.starts_with("readonly_") {
            if is_admin {
                UiVisibility::Visible
            } else {
                UiVisibility::ReadOnly
            }
        } else {
            UiVisibility::Visible
        };

        UiVisibilityResult { decision }
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
    }

    impl MockRuleRepo {
        fn new() -> Self {
            Self {
                rules: Mutex::new(Vec::new()),
            }
        }

        fn add_rule(&self, rule: GovernanceRule) {
            self.rules.lock().unwrap().push(rule);
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

    fn make_audit() -> AuditLogger {
        let audit_repo = Arc::new(MockAuditRepo::new());
        AuditLogger::new(audit_repo)
    }

    // -- SR_GOV_73 Tests: LLM Router Stage 1 ----------------------------------

    #[tokio::test]
    async fn router_all_models_allowed_when_no_rules() {
        let rules_repo = Arc::new(MockRuleRepo::new());
        let audit = make_audit();
        let svc = RouterStage1Service::new(rules_repo, audit);

        let input = RouterStage1Input {
            tenant_id: TenantId::new(),
            principal_id: uuid::Uuid::new_v4(),
            data_attributes: serde_json::json!({}),
            request_context: serde_json::json!({}),
        };

        let result = svc.evaluate(&input).await.unwrap();
        assert_eq!(result.allowed_models.len(), 4);
        assert!(result.allowed_models.contains(&"claude-sonnet".to_string()));
        assert!(result.allowed_models.contains(&"gpt-4o".to_string()));
        assert!(result.reasoning.is_empty());
    }

    #[tokio::test]
    async fn router_specific_model_blocked_by_rule() {
        let rules_repo = Arc::new(MockRuleRepo::new());
        let tenant_id = TenantId::new();

        rules_repo.add_rule(GovernanceRule {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            name: "block_gpt4o".into(),
            rule_class: RuleClass::Enforce,
            action_pattern: "llm.route".into(),
            condition: serde_json::json!({"blocked_model": "gpt-4o"}),
            advisory_message: None,
            is_active: true,
        });

        let audit = make_audit();
        let svc = RouterStage1Service::new(rules_repo, audit);

        let input = RouterStage1Input {
            tenant_id,
            principal_id: uuid::Uuid::new_v4(),
            data_attributes: serde_json::json!({}),
            request_context: serde_json::json!({}),
        };

        let result = svc.evaluate(&input).await.unwrap();
        assert_eq!(result.allowed_models.len(), 3);
        assert!(!result.allowed_models.contains(&"gpt-4o".to_string()));
        assert_eq!(result.reasoning.len(), 1);
        assert!(result.reasoning[0].contains("gpt-4o"));
    }

    #[tokio::test]
    async fn router_empty_allowed_list_when_all_blocked() {
        let rules_repo = Arc::new(MockRuleRepo::new());
        let tenant_id = TenantId::new();

        for model in DEFAULT_MODELS {
            rules_repo.add_rule(GovernanceRule {
                id: uuid::Uuid::new_v4(),
                tenant_id,
                name: format!("block_{}", model),
                rule_class: RuleClass::Enforce,
                action_pattern: "llm.route".into(),
                condition: serde_json::json!({"blocked_model": model}),
                advisory_message: None,
                is_active: true,
            });
        }

        let audit = make_audit();
        let svc = RouterStage1Service::new(rules_repo, audit);

        let input = RouterStage1Input {
            tenant_id,
            principal_id: uuid::Uuid::new_v4(),
            data_attributes: serde_json::json!({}),
            request_context: serde_json::json!({}),
        };

        let result = svc.evaluate(&input).await.unwrap();
        assert!(result.allowed_models.is_empty());
        assert_eq!(result.reasoning.len(), 4);
    }

    // -- SR_GOV_74 Tests: Decision Support Preflight ---------------------------

    #[tokio::test]
    async fn preflight_allows_when_no_restrictions() {
        let audit = make_audit();
        let svc = DecisionSupportPreflightService::new(audit);

        let input = DecisionSupportPreflightInput {
            tenant_id: TenantId::new(),
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into()],
            parameter_overrides: Vec::new(),
        };

        let result = svc.check(&input).await.unwrap();
        assert!(result.allowed);
        assert!(result.blocked_reasons.is_empty());
    }

    #[tokio::test]
    async fn preflight_blocks_multi_source_without_csa_clearance() {
        let audit = make_audit();
        let svc = DecisionSupportPreflightService::new(audit);

        let input = DecisionSupportPreflightInput {
            tenant_id: TenantId::new(),
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into(), "coll_b".into()],
            parameter_overrides: Vec::new(),
        };

        let result = svc.check(&input).await.unwrap();
        assert!(!result.allowed);
        assert!(result.blocked_reasons[0].contains("CSA"));
    }

    #[tokio::test]
    async fn preflight_allows_single_source() {
        let audit = make_audit();
        let svc = DecisionSupportPreflightService::new(audit);

        let input = DecisionSupportPreflightInput {
            tenant_id: TenantId::new(),
            query_id: uuid::Uuid::new_v4(),
            data_collection_refs: vec!["coll_a".into()],
            parameter_overrides: vec!["some_override".into()],
        };

        let result = svc.check(&input).await.unwrap();
        assert!(result.allowed);
        assert!(result.blocked_reasons.is_empty());
    }

    // -- SR_GOV_75 Tests: UI Visibility Check ---------------------------------

    #[test]
    fn ui_admin_element_hidden_for_non_admin() {
        let input = UiVisibilityCheck {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["viewer".into()],
            ui_element_id: "admin_settings_panel".into(),
            context: serde_json::json!({}),
        };

        let result = UiVisibilityService::check(&input);
        assert_eq!(result.decision, UiVisibility::Hidden);
    }

    #[test]
    fn ui_admin_element_visible_for_admin() {
        let input = UiVisibilityCheck {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["admin".into()],
            ui_element_id: "admin_settings_panel".into(),
            context: serde_json::json!({}),
        };

        let result = UiVisibilityService::check(&input);
        assert_eq!(result.decision, UiVisibility::Visible);
    }

    #[test]
    fn ui_readonly_element_returns_read_only_for_non_admin() {
        let input = UiVisibilityCheck {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["viewer".into()],
            ui_element_id: "readonly_audit_log".into(),
            context: serde_json::json!({}),
        };

        let result = UiVisibilityService::check(&input);
        assert_eq!(result.decision, UiVisibility::ReadOnly);
    }
}
