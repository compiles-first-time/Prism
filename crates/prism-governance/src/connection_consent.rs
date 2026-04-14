//! Connection consent capture service (SR_GOV_70).
//!
//! Captures explicit tenant consent for external system connections,
//! including optional paywall acknowledgements for vendor terms of service.
//!
//! Implements: SR_GOV_70

use std::sync::Arc;

use chrono::Utc;
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::ConnectionConsentRepository;
use prism_core::types::*;

/// Service for capturing connection consents.
///
/// Composes:
/// - `ConnectionConsentRepository` -- persistence for consents
/// - `AuditLogger` -- audit trail for consent events
///
/// Implements: SR_GOV_70
pub struct ConnectionConsentService {
    repo: Arc<dyn ConnectionConsentRepository>,
    audit: AuditLogger,
}

impl ConnectionConsentService {
    /// Create a new connection consent service.
    pub fn new(repo: Arc<dyn ConnectionConsentRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Capture explicit consent for an external system connection.
    ///
    /// Validates:
    /// - `vendor_terms_acknowledged` must be true
    /// - `system_id` must be non-empty
    /// - `authorized_by` must be non-nil
    ///
    /// Emits `connection.consent_captured` audit event on success.
    ///
    /// Implements: SR_GOV_70
    pub async fn capture_consent(
        &self,
        request: &ConnectionConsentRequest,
    ) -> Result<ConnectionConsentResult, PrismError> {
        // Validate vendor_terms_acknowledged
        if !request.vendor_terms_acknowledged {
            return Err(PrismError::Validation {
                reason: "vendor_terms_acknowledged must be true to capture consent".into(),
            });
        }

        // Validate system_id is non-empty
        if request.system_id.trim().is_empty() {
            return Err(PrismError::Validation {
                reason: "system_id must not be empty".into(),
            });
        }

        // Validate authorized_by is non-nil
        if *request.authorized_by.as_uuid() == uuid::Uuid::nil() {
            return Err(PrismError::Validation {
                reason: "authorized_by must identify a valid user (non-nil)".into(),
            });
        }

        let paywall_recorded = request.paywall_acknowledgement.is_some();
        let consent_id = uuid::Uuid::now_v7();

        let consent = ConnectionConsent {
            id: consent_id,
            tenant_id: request.tenant_id,
            system_id: request.system_id.clone(),
            connection_type: request.connection_type.clone(),
            scope: request.scope.clone(),
            vendor_terms_acknowledged: request.vendor_terms_acknowledged,
            paywall_recorded,
            authorized_by: request.authorized_by,
            created_at: Utc::now(),
        };

        self.repo.record_consent(&consent).await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "connection.consent_captured".into(),
                actor_id: *request.authorized_by.as_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(consent_id),
                target_type: Some("ConnectionConsent".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Connection,
                governance_authority: None,
                payload: serde_json::json!({
                    "system_id": request.system_id,
                    "connection_type": request.connection_type,
                    "scope": request.scope,
                    "paywall_recorded": paywall_recorded,
                }),
            })
            .await?;

        info!(
            tenant_id = %request.tenant_id,
            system_id = %request.system_id,
            consent_id = %consent_id,
            paywall_recorded = paywall_recorded,
            "connection consent captured"
        );

        Ok(ConnectionConsentResult {
            consent_id,
            paywall_recorded,
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

    // -- Mock ConnectionConsentRepository ------------------------------------

    struct MockConsentRepo {
        consents: Mutex<Vec<ConnectionConsent>>,
    }

    impl MockConsentRepo {
        fn new() -> Self {
            Self {
                consents: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ConnectionConsentRepository for MockConsentRepo {
        async fn record_consent(&self, consent: &ConnectionConsent) -> Result<(), PrismError> {
            self.consents.lock().unwrap().push(consent.clone());
            Ok(())
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

    fn make_service() -> (ConnectionConsentService, Arc<MockConsentRepo>) {
        let consent_repo = Arc::new(MockConsentRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        let svc = ConnectionConsentService::new(consent_repo.clone(), audit);
        (svc, consent_repo)
    }

    fn make_request() -> ConnectionConsentRequest {
        ConnectionConsentRequest {
            tenant_id: TenantId::new(),
            system_id: "salesforce-crm".into(),
            connection_type: "oauth2".into(),
            scope: "read:contacts".into(),
            vendor_terms_acknowledged: true,
            paywall_acknowledgement: None,
            authorized_by: UserId::new(),
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn consent_succeeds() {
        let (svc, repo) = make_service();
        let request = make_request();

        let result = svc.capture_consent(&request).await.unwrap();
        assert!(!result.paywall_recorded);

        let consents = repo.consents.lock().unwrap();
        assert_eq!(consents.len(), 1);
        assert_eq!(consents[0].system_id, "salesforce-crm");
    }

    #[tokio::test]
    async fn consent_with_paywall() {
        let (svc, _repo) = make_service();
        let mut request = make_request();
        request.paywall_acknowledgement = Some(PaywallAcknowledgement {
            vendor_tos_url: "https://vendor.example.com/tos".into(),
            accepted_at: Utc::now(),
            accepted_by: UserId::new(),
        });

        let result = svc.capture_consent(&request).await.unwrap();
        assert!(result.paywall_recorded);
    }

    #[tokio::test]
    async fn rejects_unacknowledged_terms() {
        let (svc, _repo) = make_service();
        let mut request = make_request();
        request.vendor_terms_acknowledged = false;

        let err = svc.capture_consent(&request).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("vendor_terms_acknowledged"));
    }

    #[tokio::test]
    async fn rejects_empty_system_id() {
        let (svc, _repo) = make_service();
        let mut request = make_request();
        request.system_id = "".into();

        let err = svc.capture_consent(&request).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("system_id"));
    }
}
