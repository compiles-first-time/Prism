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
use prism_core::repository::{
    ComponentRegistry, ConnectionStatusRepository, GovernanceRuleRepository, QuotaEnforcer,
};
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
// SR_GOV_76 -- Connection Pull Preflight
// ===========================================================================

/// Service that performs preflight checks before a data pull from an
/// external connection.
///
/// Checks: connection approval, credential presence, and budget quota.
/// DENY if not approved or no credential; DEFER if budget exceeded;
/// ALLOW otherwise.
///
/// Implements: SR_GOV_76
pub struct ConnectionPullPreflightService {
    conn_status: Arc<dyn ConnectionStatusRepository>,
    quota: Arc<dyn QuotaEnforcer>,
    audit: AuditLogger,
}

impl ConnectionPullPreflightService {
    /// Create a new connection pull preflight service.
    pub fn new(
        conn_status: Arc<dyn ConnectionStatusRepository>,
        quota: Arc<dyn QuotaEnforcer>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            conn_status,
            quota,
            audit,
        }
    }

    /// Check whether a connection pull is allowed.
    ///
    /// - DENY if the connection is not approved.
    /// - DENY if the connection has no credential.
    /// - DEFER if the budget would be exceeded.
    /// - ALLOW otherwise.
    ///
    /// Implements: SR_GOV_76
    pub async fn check(
        &self,
        input: &ConnectionPullPreflight,
    ) -> Result<ConnectionPullPreflightResult, PrismError> {
        // Check approval
        let approved = self
            .conn_status
            .is_approved(input.tenant_id, &input.connection_id)
            .await?;
        if !approved {
            self.audit_deny_or_defer(
                input,
                PullPreflightDecision::Deny,
                "connection not approved",
            )
            .await?;
            return Ok(ConnectionPullPreflightResult {
                decision: PullPreflightDecision::Deny,
                defer_reason: None,
            });
        }

        // Check credential
        let has_cred = self
            .conn_status
            .has_credential(input.tenant_id, &input.connection_id)
            .await?;
        if !has_cred {
            self.audit_deny_or_defer(
                input,
                PullPreflightDecision::Deny,
                "no credential for connection",
            )
            .await?;
            return Ok(ConnectionPullPreflightResult {
                decision: PullPreflightDecision::Deny,
                defer_reason: None,
            });
        }

        // Check budget
        let within_budget = self
            .quota
            .check_budget(input.tenant_id, &input.connection_id, input.expected_volume)
            .await?;
        if !within_budget {
            let reason = "budget exceeded for expected volume".to_string();
            self.audit_deny_or_defer(input, PullPreflightDecision::Defer, &reason)
                .await?;
            return Ok(ConnectionPullPreflightResult {
                decision: PullPreflightDecision::Defer,
                defer_reason: Some(reason),
            });
        }

        Ok(ConnectionPullPreflightResult {
            decision: PullPreflightDecision::Allow,
            defer_reason: None,
        })
    }

    /// Emit an audit event for DENY or DEFER decisions.
    ///
    /// Implements: SR_GOV_76
    async fn audit_deny_or_defer(
        &self,
        input: &ConnectionPullPreflight,
        decision: PullPreflightDecision,
        reason: &str,
    ) -> Result<(), PrismError> {
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "connection.pull_preflight".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("Connection".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Connection,
                governance_authority: None,
                payload: serde_json::json!({
                    "connection_id": input.connection_id,
                    "decision": format!("{:?}", decision),
                    "reason": reason,
                }),
            })
            .await?;
        Ok(())
    }
}

// ===========================================================================
// SR_GOV_77 -- Intelligence Query Rewrite
// ===========================================================================

/// Forbidden SQL/Cypher constructs that must not appear in intelligence queries.
const FORBIDDEN_CONSTRUCTS: &[&str] = &[
    "DROP ",
    "DELETE ",
    "DETACH DELETE",
    "MERGE",
    "CALL dbms.",
    "CALL db.",
];

/// Service that rewrites intelligence queries to enforce tenant isolation
/// and blocks forbidden constructs.
///
/// Implements: SR_GOV_77
pub struct QueryRewriteService;

impl QueryRewriteService {
    /// Rewrite a raw intelligence query.
    ///
    /// - Rejects queries containing forbidden constructs (DROP, DELETE, etc.).
    /// - Injects a `WHERE tenant_id = '<tenant_id>'` constraint.
    ///
    /// Implements: SR_GOV_77
    pub fn rewrite(input: &QueryRewriteInput) -> Result<QueryRewriteResult, PrismError> {
        let upper = input.raw_query.to_uppercase();

        for construct in FORBIDDEN_CONSTRUCTS {
            if upper.contains(&construct.to_uppercase()) {
                return Err(PrismError::Validation {
                    reason: format!("query contains forbidden construct: {}", construct.trim()),
                });
            }
        }

        let tenant_filter = format!("WHERE tenant_id = '{}'", input.tenant_id);
        let rewritten = format!("{} {}", input.raw_query, tenant_filter);
        let applied_filters = vec![tenant_filter];

        Ok(QueryRewriteResult {
            rewritten_query: rewritten,
            applied_filters,
        })
    }
}

// ===========================================================================
// SR_GOV_78 -- Component Execution Preflight
// ===========================================================================

/// Service that performs preflight checks before executing a component.
///
/// Checks: component exists, is active, not deprecated, principal has
/// required role (if any), and credential is available (if required).
///
/// Implements: SR_GOV_78
pub struct ComponentPreflightService {
    registry: Arc<dyn ComponentRegistry>,
}

impl ComponentPreflightService {
    /// Create a new component preflight service.
    pub fn new(registry: Arc<dyn ComponentRegistry>) -> Self {
        Self { registry }
    }

    /// Check whether a component execution is allowed.
    ///
    /// - DENY if component not found, not active, deprecated, missing role, or missing credential.
    /// - ALLOW otherwise.
    ///
    /// Implements: SR_GOV_78
    pub async fn check(
        &self,
        input: &ComponentExecutionPreflight,
    ) -> Result<ComponentExecutionPreflightResult, PrismError> {
        let component = self
            .registry
            .get_component(input.tenant_id, &input.component_id)
            .await?;

        let Some(comp) = component else {
            return Ok(ComponentExecutionPreflightResult {
                decision: AccessDecision::Deny,
            });
        };

        if !comp.is_active {
            return Ok(ComponentExecutionPreflightResult {
                decision: AccessDecision::Deny,
            });
        }

        if comp.is_deprecated {
            return Ok(ComponentExecutionPreflightResult {
                decision: AccessDecision::Deny,
            });
        }

        if let Some(ref required_role) = comp.required_role {
            if !input.principal_roles.iter().any(|r| r == required_role) {
                return Ok(ComponentExecutionPreflightResult {
                    decision: AccessDecision::Deny,
                });
            }
        }

        if comp.credential_required && !comp.has_credential {
            return Ok(ComponentExecutionPreflightResult {
                decision: AccessDecision::Deny,
            });
        }

        Ok(ComponentExecutionPreflightResult {
            decision: AccessDecision::Allow,
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

    // -- Mock ConnectionStatusRepository (SR_GOV_76) ----------------------------

    struct MockConnectionStatus {
        approved: Mutex<Vec<String>>,
        has_cred: Mutex<Vec<String>>,
    }

    impl MockConnectionStatus {
        fn new(approved: Vec<&str>, has_cred: Vec<&str>) -> Self {
            Self {
                approved: Mutex::new(approved.into_iter().map(String::from).collect()),
                has_cred: Mutex::new(has_cred.into_iter().map(String::from).collect()),
            }
        }
    }

    #[async_trait]
    impl ConnectionStatusRepository for MockConnectionStatus {
        async fn is_approved(
            &self,
            _tenant_id: TenantId,
            connection_id: &str,
        ) -> Result<bool, PrismError> {
            Ok(self
                .approved
                .lock()
                .unwrap()
                .iter()
                .any(|c| c == connection_id))
        }

        async fn has_credential(
            &self,
            _tenant_id: TenantId,
            connection_id: &str,
        ) -> Result<bool, PrismError> {
            Ok(self
                .has_cred
                .lock()
                .unwrap()
                .iter()
                .any(|c| c == connection_id))
        }
    }

    // -- Mock QuotaEnforcer (SR_GOV_76) -----------------------------------------

    struct MockQuota {
        within_budget: bool,
    }

    #[async_trait]
    impl QuotaEnforcer for MockQuota {
        async fn check_budget(
            &self,
            _tenant_id: TenantId,
            _connection_id: &str,
            _expected_volume: u64,
        ) -> Result<bool, PrismError> {
            Ok(self.within_budget)
        }
    }

    // -- SR_GOV_76 Tests: Connection Pull Preflight ------------------------------

    #[tokio::test]
    async fn pull_preflight_allows_when_all_checks_pass() {
        let conn_status = Arc::new(MockConnectionStatus::new(vec!["conn_1"], vec!["conn_1"]));
        let quota = Arc::new(MockQuota {
            within_budget: true,
        });
        let audit = make_audit();
        let svc = ConnectionPullPreflightService::new(conn_status, quota, audit);

        let input = ConnectionPullPreflight {
            tenant_id: TenantId::new(),
            connection_id: "conn_1".into(),
            scope: "full".into(),
            expected_volume: 100,
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, PullPreflightDecision::Allow);
        assert!(result.defer_reason.is_none());
    }

    #[tokio::test]
    async fn pull_preflight_denies_when_not_approved() {
        let conn_status = Arc::new(MockConnectionStatus::new(vec![], vec!["conn_1"]));
        let quota = Arc::new(MockQuota {
            within_budget: true,
        });
        let audit = make_audit();
        let svc = ConnectionPullPreflightService::new(conn_status, quota, audit);

        let input = ConnectionPullPreflight {
            tenant_id: TenantId::new(),
            connection_id: "conn_1".into(),
            scope: "full".into(),
            expected_volume: 100,
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, PullPreflightDecision::Deny);
    }

    #[tokio::test]
    async fn pull_preflight_denies_when_no_credential() {
        let conn_status = Arc::new(MockConnectionStatus::new(vec!["conn_1"], vec![]));
        let quota = Arc::new(MockQuota {
            within_budget: true,
        });
        let audit = make_audit();
        let svc = ConnectionPullPreflightService::new(conn_status, quota, audit);

        let input = ConnectionPullPreflight {
            tenant_id: TenantId::new(),
            connection_id: "conn_1".into(),
            scope: "full".into(),
            expected_volume: 100,
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, PullPreflightDecision::Deny);
    }

    #[tokio::test]
    async fn pull_preflight_defers_when_budget_exceeded() {
        let conn_status = Arc::new(MockConnectionStatus::new(vec!["conn_1"], vec!["conn_1"]));
        let quota = Arc::new(MockQuota {
            within_budget: false,
        });
        let audit = make_audit();
        let svc = ConnectionPullPreflightService::new(conn_status, quota, audit);

        let input = ConnectionPullPreflight {
            tenant_id: TenantId::new(),
            connection_id: "conn_1".into(),
            scope: "full".into(),
            expected_volume: 999_999,
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, PullPreflightDecision::Defer);
        assert!(result.defer_reason.is_some());
    }

    // -- SR_GOV_77 Tests: Query Rewrite ------------------------------------------

    #[test]
    fn query_rewrite_adds_tenant_filter() {
        let input = QueryRewriteInput {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["analyst".into()],
            raw_query: "MATCH (n:Account) RETURN n".into(),
        };

        let result = QueryRewriteService::rewrite(&input).unwrap();
        assert!(result.rewritten_query.contains("tenant_id"));
        assert!(!result.applied_filters.is_empty());
    }

    #[test]
    fn query_rewrite_rejects_drop() {
        let input = QueryRewriteInput {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["analyst".into()],
            raw_query: "DROP INDEX my_index".into(),
        };

        let result = QueryRewriteService::rewrite(&input);
        assert!(result.is_err());
    }

    #[test]
    fn query_rewrite_rejects_delete() {
        let input = QueryRewriteInput {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["analyst".into()],
            raw_query: "MATCH (n) DELETE n".into(),
        };

        let result = QueryRewriteService::rewrite(&input);
        assert!(result.is_err());
    }

    #[test]
    fn query_rewrite_passes_clean_query() {
        let input = QueryRewriteInput {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["analyst".into()],
            raw_query: "MATCH (n:Transaction) RETURN n LIMIT 10".into(),
        };

        let result = QueryRewriteService::rewrite(&input).unwrap();
        assert!(result.rewritten_query.contains("tenant_id"));
        assert_eq!(result.applied_filters.len(), 1);
    }

    // -- Mock ComponentRegistry (SR_GOV_78) --------------------------------------

    struct MockComponentRegistry {
        components: Mutex<Vec<ComponentInfo>>,
    }

    impl MockComponentRegistry {
        fn new() -> Self {
            Self {
                components: Mutex::new(Vec::new()),
            }
        }

        fn add(&self, info: ComponentInfo) {
            self.components.lock().unwrap().push(info);
        }
    }

    #[async_trait]
    impl ComponentRegistry for MockComponentRegistry {
        async fn get_component(
            &self,
            _tenant_id: TenantId,
            component_id: &str,
        ) -> Result<Option<ComponentInfo>, PrismError> {
            let components = self.components.lock().unwrap();
            Ok(components
                .iter()
                .find(|c| c.component_id == component_id)
                .cloned())
        }
    }

    // -- SR_GOV_78 Tests: Component Execution Preflight ---------------------------

    #[tokio::test]
    async fn component_preflight_allows_when_all_checks_pass() {
        let registry = Arc::new(MockComponentRegistry::new());
        registry.add(ComponentInfo {
            component_id: "comp_1".into(),
            is_active: true,
            is_deprecated: false,
            required_role: Some("operator".into()),
            credential_required: false,
            has_credential: false,
        });
        let svc = ComponentPreflightService::new(registry);

        let input = ComponentExecutionPreflight {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["operator".into()],
            component_id: "comp_1".into(),
            args: serde_json::json!({}),
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Allow);
    }

    #[tokio::test]
    async fn component_preflight_denies_when_inactive() {
        let registry = Arc::new(MockComponentRegistry::new());
        registry.add(ComponentInfo {
            component_id: "comp_1".into(),
            is_active: false,
            is_deprecated: false,
            required_role: None,
            credential_required: false,
            has_credential: false,
        });
        let svc = ComponentPreflightService::new(registry);

        let input = ComponentExecutionPreflight {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["operator".into()],
            component_id: "comp_1".into(),
            args: serde_json::json!({}),
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Deny);
    }

    #[tokio::test]
    async fn component_preflight_denies_when_deprecated() {
        let registry = Arc::new(MockComponentRegistry::new());
        registry.add(ComponentInfo {
            component_id: "comp_1".into(),
            is_active: true,
            is_deprecated: true,
            required_role: None,
            credential_required: false,
            has_credential: false,
        });
        let svc = ComponentPreflightService::new(registry);

        let input = ComponentExecutionPreflight {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["operator".into()],
            component_id: "comp_1".into(),
            args: serde_json::json!({}),
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Deny);
    }

    #[tokio::test]
    async fn component_preflight_denies_when_missing_role() {
        let registry = Arc::new(MockComponentRegistry::new());
        registry.add(ComponentInfo {
            component_id: "comp_1".into(),
            is_active: true,
            is_deprecated: false,
            required_role: Some("admin".into()),
            credential_required: false,
            has_credential: false,
        });
        let svc = ComponentPreflightService::new(registry);

        let input = ComponentExecutionPreflight {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec!["operator".into()],
            component_id: "comp_1".into(),
            args: serde_json::json!({}),
        };

        let result = svc.check(&input).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Deny);
    }
}
