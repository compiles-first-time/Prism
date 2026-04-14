//! Rejection justification validation service (SR_GOV_72).
//!
//! Validates recommendation rejection justifications before storing them.
//! Reuses `JustificationValidator` from `crate::rule_engine` for filler-word
//! and length checks, and adds category validation.
//!
//! Implements: SR_GOV_72

use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::types::*;

use crate::rule_engine::JustificationValidator;

/// Known rejection categories.
const VALID_CATEGORIES: &[&str] = &[
    "inaccurate",
    "irrelevant",
    "incomplete",
    "outdated",
    "other",
];

/// Service for validating recommendation rejection justifications.
///
/// Composes:
/// - `JustificationValidator` -- reused filler/length checks from rule_engine
/// - `AuditLogger` -- audit trail for rejection events
///
/// Implements: SR_GOV_72
pub struct RejectionValidationService {
    audit: AuditLogger,
}

impl RejectionValidationService {
    /// Create a new rejection validation service.
    pub fn new(audit: AuditLogger) -> Self {
        Self { audit }
    }

    /// Validate and record a recommendation rejection.
    ///
    /// Checks:
    /// 1. Category is one of the known set
    /// 2. Justification text passes filler/length checks (via JustificationValidator)
    ///
    /// On success: emits `recommendation.rejection_recorded` audit event.
    /// On failure: emits `recommendation.rejection_invalid` audit event.
    ///
    /// Implements: SR_GOV_72
    pub async fn validate_rejection(
        &self,
        input: &RejectionInput,
    ) -> Result<RejectionResult, PrismError> {
        // Check category
        if !VALID_CATEGORIES.contains(&input.category.as_str()) {
            let finding = format!(
                "invalid category '{}'; must be one of: {}",
                input.category,
                VALID_CATEGORIES.join(", ")
            );

            warn!(
                tenant_id = %input.tenant_id,
                recommendation_id = %input.recommendation_id,
                category = %input.category,
                "SR_GOV_72: rejection invalid -- bad category"
            );

            self.audit_rejection_invalid(input, &finding).await?;

            return Ok(RejectionResult {
                stored: false,
                validation_findings: Some(finding),
            });
        }

        // Check justification text using the shared validator
        if let Some(finding) = JustificationValidator::validate(&input.justification_text) {
            warn!(
                tenant_id = %input.tenant_id,
                recommendation_id = %input.recommendation_id,
                "SR_GOV_72: rejection invalid -- justification failed validation"
            );

            self.audit_rejection_invalid(input, &finding).await?;

            return Ok(RejectionResult {
                stored: false,
                validation_findings: Some(finding),
            });
        }

        // All checks passed -- record the rejection
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "recommendation.rejection_recorded".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::Human,
                target_id: Some(input.recommendation_id),
                target_type: Some("Recommendation".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "category": input.category,
                    "justification_length": input.justification_text.len(),
                }),
            })
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            recommendation_id = %input.recommendation_id,
            category = %input.category,
            "recommendation rejection recorded"
        );

        Ok(RejectionResult {
            stored: true,
            validation_findings: None,
        })
    }

    /// Emit an audit event for an invalid rejection attempt.
    async fn audit_rejection_invalid(
        &self,
        input: &RejectionInput,
        finding: &str,
    ) -> Result<(), PrismError> {
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "recommendation.rejection_invalid".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::Human,
                target_id: Some(input.recommendation_id),
                target_type: Some("Recommendation".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "category": input.category,
                    "finding": finding,
                }),
            })
            .await?;
        Ok(())
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

    fn make_service() -> RejectionValidationService {
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        RejectionValidationService::new(audit)
    }

    fn make_input(category: &str, text: &str) -> RejectionInput {
        RejectionInput {
            tenant_id: TenantId::new(),
            recommendation_id: uuid::Uuid::new_v4(),
            category: category.into(),
            justification_text: text.into(),
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn valid_rejection_accepted() {
        let svc = make_service();
        let input = make_input(
            "inaccurate",
            "The recommendation contains outdated market data from Q2 2024 that no longer applies",
        );

        let result = svc.validate_rejection(&input).await.unwrap();
        assert!(result.stored);
        assert!(result.validation_findings.is_none());
    }

    #[tokio::test]
    async fn empty_text_rejected() {
        let svc = make_service();
        let input = make_input("inaccurate", "");

        let result = svc.validate_rejection(&input).await.unwrap();
        assert!(!result.stored);
        assert!(result.validation_findings.unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn invalid_category_rejected() {
        let svc = make_service();
        let input = make_input(
            "not_a_real_category",
            "This is a perfectly valid justification text that is long enough",
        );

        let result = svc.validate_rejection(&input).await.unwrap();
        assert!(!result.stored);
        assert!(result
            .validation_findings
            .unwrap()
            .contains("invalid category"));
    }

    #[tokio::test]
    async fn filler_text_rejected() {
        let svc = make_service();
        let input = make_input("irrelevant", "because");

        let result = svc.validate_rejection(&input).await.unwrap();
        assert!(!result.stored);
        assert!(result.validation_findings.is_some());
    }

    #[tokio::test]
    async fn short_text_rejected() {
        let svc = make_service();
        let input = make_input("incomplete", "too short");

        let result = svc.validate_rejection(&input).await.unwrap();
        assert!(!result.stored);
        assert!(result.validation_findings.unwrap().contains("at least 20"));
    }
}
