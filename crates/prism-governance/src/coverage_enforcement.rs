//! Coverage disclosure enforcement service (SR_GOV_71).
//!
//! Enforces that Decision Support responses include a coverage disclosure
//! with required subfields. This is a stateless validation service -- no
//! repository is needed.
//!
//! Implements: SR_GOV_71

use tracing::warn;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::types::*;

/// Service for enforcing coverage disclosure in Decision Support responses.
///
/// Composes:
/// - `AuditLogger` -- audit trail for enforcement failures
///
/// Implements: SR_GOV_71
pub struct CoverageEnforcementService {
    audit: AuditLogger,
}

impl CoverageEnforcementService {
    /// Create a new coverage enforcement service.
    pub fn new(audit: AuditLogger) -> Self {
        Self { audit }
    }

    /// Enforce that the response payload contains a valid coverage disclosure.
    ///
    /// Checks that `response_payload` has a `coverage_disclosure` object with:
    /// - `coverage_percentage` (number)
    /// - `data_sources_count` (number)
    ///
    /// Emits `coverage.disclosure_missing` audit event on failure.
    ///
    /// Implements: SR_GOV_71
    pub async fn enforce(
        &self,
        input: &CoverageEnforcementInput,
    ) -> Result<CoverageEnforcementResult, PrismError> {
        let mut missing_fields = Vec::new();

        let disclosure = input.response_payload.get("coverage_disclosure");

        match disclosure {
            None => {
                missing_fields.push("coverage_disclosure".to_string());
            }
            Some(disc) => {
                if !disc
                    .get("coverage_percentage")
                    .is_some_and(|v| v.is_number())
                {
                    missing_fields.push("coverage_disclosure.coverage_percentage".to_string());
                }
                if !disc
                    .get("data_sources_count")
                    .is_some_and(|v| v.is_number())
                {
                    missing_fields.push("coverage_disclosure.data_sources_count".to_string());
                }
            }
        }

        if !missing_fields.is_empty() {
            warn!(
                tenant_id = %input.tenant_id,
                missing = ?missing_fields,
                "SR_GOV_71: coverage disclosure missing required fields"
            );

            self.audit
                .log(AuditEventInput {
                    tenant_id: input.tenant_id,
                    event_type: "coverage.disclosure_missing".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: None,
                    target_type: Some("DecisionSupportResponse".into()),
                    severity: Severity::High,
                    source_layer: SourceLayer::Governance,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "missing_fields": missing_fields,
                    }),
                })
                .await?;

            return Ok(CoverageEnforcementResult {
                passed: false,
                missing_fields: Some(missing_fields),
            });
        }

        Ok(CoverageEnforcementResult {
            passed: true,
            missing_fields: None,
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
    use std::sync::Arc;
    use std::sync::Mutex;

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

    fn make_service() -> CoverageEnforcementService {
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        CoverageEnforcementService::new(audit)
    }

    fn make_input(payload: serde_json::Value) -> CoverageEnforcementInput {
        CoverageEnforcementInput {
            tenant_id: TenantId::new(),
            response_payload: payload,
            query_context: serde_json::json!({}),
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn valid_response_passes() {
        let svc = make_service();
        let input = make_input(serde_json::json!({
            "coverage_disclosure": {
                "coverage_percentage": 85.5,
                "data_sources_count": 3
            }
        }));

        let result = svc.enforce(&input).await.unwrap();
        assert!(result.passed);
        assert!(result.missing_fields.is_none());
    }

    #[tokio::test]
    async fn missing_disclosure_fails() {
        let svc = make_service();
        let input = make_input(serde_json::json!({
            "result": "some data"
        }));

        let result = svc.enforce(&input).await.unwrap();
        assert!(!result.passed);
        let missing = result.missing_fields.unwrap();
        assert!(missing.contains(&"coverage_disclosure".to_string()));
    }

    #[tokio::test]
    async fn missing_coverage_percentage_fails() {
        let svc = make_service();
        let input = make_input(serde_json::json!({
            "coverage_disclosure": {
                "data_sources_count": 3
            }
        }));

        let result = svc.enforce(&input).await.unwrap();
        assert!(!result.passed);
        let missing = result.missing_fields.unwrap();
        assert!(missing.contains(&"coverage_disclosure.coverage_percentage".to_string()));
    }

    #[tokio::test]
    async fn missing_data_sources_count_fails() {
        let svc = make_service();
        let input = make_input(serde_json::json!({
            "coverage_disclosure": {
                "coverage_percentage": 90.0
            }
        }));

        let result = svc.enforce(&input).await.unwrap();
        assert!(!result.passed);
        let missing = result.missing_fields.unwrap();
        assert!(missing.contains(&"coverage_disclosure.data_sources_count".to_string()));
    }
}
