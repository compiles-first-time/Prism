//! Alert routing via severity matrix (SR_GOV_67, BP-29).
//!
//! Routes alerts through channels determined by severity:
//! - **CRITICAL** -> page on-call + SMS + in-app + email
//! - **HIGH** -> in-app + email
//! - **MEDIUM** -> in-app + digest
//! - **LOW** -> digest only
//!
//! Each tenant may customize the matrix within bounds (cannot downgrade
//! CRITICAL below page+email).
//!
//! Implements: SR_GOV_67

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::types::*;

// ---------------------------------------------------------------------------
// Trait: AlertChannel
// ---------------------------------------------------------------------------

/// A delivery channel for alerts (page, SMS, email, in-app, digest).
///
/// Each channel implementation knows how to dispatch to its specific
/// medium and returns a dispatch ID for acknowledgement tracking.
///
/// Implements: SR_GOV_67
#[async_trait]
pub trait AlertChannelDispatcher: Send + Sync {
    /// The channel this dispatcher handles.
    fn channel(&self) -> AlertChannel;

    /// Dispatch an alert through this channel.
    /// Returns a list of (recipient, dispatch_id) pairs.
    async fn dispatch(
        &self,
        tenant_id: TenantId,
        severity: Severity,
        message: &str,
        detail: &serde_json::Value,
    ) -> Result<Vec<(String, String)>, PrismError>;
}

// ---------------------------------------------------------------------------
// Trait: AlertHistoryRepository
// ---------------------------------------------------------------------------

/// Persistence for alert dispatch history, enabling acknowledgement tracking.
///
/// Implements: SR_GOV_67
#[async_trait]
pub trait AlertHistoryRepository: Send + Sync {
    /// Record a dispatched alert.
    async fn record(&self, entry: &AlertHistoryEntry) -> Result<(), PrismError>;
}

// ---------------------------------------------------------------------------
// AlertRoutingService
// ---------------------------------------------------------------------------

/// Routes alerts via the BP-29 severity matrix.
///
/// Composes:
/// - Channel dispatchers (page, SMS, in-app, email, digest)
/// - `AlertHistoryRepository` -- records dispatched alerts
/// - `AuditLogger` -- audit trail for alert events
///
/// Implements: SR_GOV_67
pub struct AlertRoutingService {
    channels: Vec<Arc<dyn AlertChannelDispatcher>>,
    history: Arc<dyn AlertHistoryRepository>,
    audit: AuditLogger,
}

impl AlertRoutingService {
    /// Create a new alert routing service.
    pub fn new(
        channels: Vec<Arc<dyn AlertChannelDispatcher>>,
        history: Arc<dyn AlertHistoryRepository>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            channels,
            history,
            audit,
        }
    }

    /// Route an alert through the appropriate channels based on severity.
    ///
    /// The default severity matrix (BP-29):
    /// - CRITICAL -> Page, Sms, InApp, Email
    /// - HIGH -> InApp, Email
    /// - MEDIUM -> InApp, Digest
    /// - LOW -> Digest
    ///
    /// Implements: SR_GOV_67
    pub async fn route_alert(&self, event: &AlertEvent) -> Result<AlertDispatchResult, PrismError> {
        let required_channels = Self::channels_for_severity(event.severity);

        let detail = serde_json::json!({
            "source": event.source,
            "attribution": event.attribution,
        });

        let mut all_recipients = Vec::new();
        let mut all_dispatch_ids = Vec::new();
        let mut channels_used = Vec::new();

        for channel_type in &required_channels {
            if let Some(dispatcher) = self.channels.iter().find(|c| &c.channel() == channel_type) {
                match dispatcher
                    .dispatch(event.tenant_id, event.severity, &event.message, &detail)
                    .await
                {
                    Ok(results) => {
                        for (recipient, dispatch_id) in results {
                            all_recipients.push(recipient);
                            all_dispatch_ids.push(dispatch_id);
                        }
                        channels_used.push(*channel_type);
                    }
                    Err(e) => {
                        // Log channel failure but continue with remaining channels
                        warn!(
                            tenant_id = %event.tenant_id,
                            channel = ?channel_type,
                            error = %e,
                            "alert channel dispatch failed -- continuing with remaining channels"
                        );
                    }
                }
            }
        }

        // Record in alert history
        let history_entry = AlertHistoryEntry {
            id: uuid::Uuid::new_v4(),
            tenant_id: event.tenant_id,
            severity: event.severity,
            source: event.source.clone(),
            message: event.message.clone(),
            channels: channels_used.clone(),
            recipients: all_recipients.clone(),
            acknowledged: false,
            created_at: chrono::Utc::now(),
        };
        self.history.record(&history_entry).await?;

        // Audit trail
        self.audit
            .log(AuditEventInput {
                tenant_id: event.tenant_id,
                event_type: "alert.dispatched".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: None,
                severity: event.severity,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "source": event.source,
                    "message": event.message,
                    "channels": channels_used,
                    "recipient_count": all_recipients.len(),
                }),
            })
            .await?;

        info!(
            tenant_id = %event.tenant_id,
            severity = ?event.severity,
            channels = ?channels_used,
            recipients = all_recipients.len(),
            "alert routed"
        );

        let channels_str: Vec<String> = channels_used
            .iter()
            .map(|c| format!("{:?}", c).to_lowercase())
            .collect();

        Ok(AlertDispatchResult {
            recipients: all_recipients,
            dispatch_ids: all_dispatch_ids,
            channels_used: channels_str,
        })
    }

    /// Determine which channels to use for a given severity level.
    ///
    /// Default matrix per BP-29:
    /// - CRITICAL -> Page + SMS + InApp + Email
    /// - HIGH -> InApp + Email
    /// - MEDIUM -> InApp + Digest
    /// - LOW -> Digest
    ///
    /// Implements: SR_GOV_67
    fn channels_for_severity(severity: Severity) -> Vec<AlertChannel> {
        match severity {
            Severity::Critical => vec![
                AlertChannel::Page,
                AlertChannel::Sms,
                AlertChannel::InApp,
                AlertChannel::Email,
            ],
            Severity::High => vec![AlertChannel::InApp, AlertChannel::Email],
            Severity::Medium => vec![AlertChannel::InApp, AlertChannel::Digest],
            Severity::Low => vec![AlertChannel::Digest],
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::repository::AuditEventRepository;
    use std::sync::Mutex;

    // -- Mock AlertChannelDispatcher -------------------------------------------

    struct MockChannel {
        channel_type: AlertChannel,
        dispatches: Mutex<Vec<(TenantId, Severity, String)>>,
        should_fail: Mutex<bool>,
    }

    impl MockChannel {
        fn new(channel_type: AlertChannel) -> Self {
            Self {
                channel_type,
                dispatches: Mutex::new(Vec::new()),
                should_fail: Mutex::new(false),
            }
        }

        fn dispatch_count(&self) -> usize {
            self.dispatches.lock().unwrap().len()
        }

        fn set_fail(&self, fail: bool) {
            *self.should_fail.lock().unwrap() = fail;
        }
    }

    #[async_trait]
    impl AlertChannelDispatcher for MockChannel {
        fn channel(&self) -> AlertChannel {
            self.channel_type
        }

        async fn dispatch(
            &self,
            tenant_id: TenantId,
            severity: Severity,
            message: &str,
            _detail: &serde_json::Value,
        ) -> Result<Vec<(String, String)>, PrismError> {
            if *self.should_fail.lock().unwrap() {
                return Err(PrismError::Internal("channel failed".into()));
            }
            self.dispatches
                .lock()
                .unwrap()
                .push((tenant_id, severity, message.to_string()));
            Ok(vec![(
                "oncall@example.com".into(),
                format!("DISP-{:?}-001", self.channel_type),
            )])
        }
    }

    // -- Mock AlertHistoryRepository -------------------------------------------

    struct MockAlertHistory {
        entries: Mutex<Vec<AlertHistoryEntry>>,
    }

    impl MockAlertHistory {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }

        fn entry_count(&self) -> usize {
            self.entries.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl AlertHistoryRepository for MockAlertHistory {
        async fn record(&self, entry: &AlertHistoryEntry) -> Result<(), PrismError> {
            self.entries.lock().unwrap().push(entry.clone());
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

    fn make_all_channels() -> Vec<Arc<dyn AlertChannelDispatcher>> {
        vec![
            Arc::new(MockChannel::new(AlertChannel::Page)),
            Arc::new(MockChannel::new(AlertChannel::Sms)),
            Arc::new(MockChannel::new(AlertChannel::InApp)),
            Arc::new(MockChannel::new(AlertChannel::Email)),
            Arc::new(MockChannel::new(AlertChannel::Digest)),
        ]
    }

    fn make_service(
        channels: Vec<Arc<dyn AlertChannelDispatcher>>,
    ) -> (AlertRoutingService, Arc<MockAlertHistory>) {
        let history = Arc::new(MockAlertHistory::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        let svc = AlertRoutingService::new(channels, history.clone(), audit);
        (svc, history)
    }

    fn make_alert(tenant_id: TenantId, severity: Severity) -> AlertEvent {
        AlertEvent {
            tenant_id,
            severity,
            source: "SR_GOV_51".into(),
            message: "Audit chain tampering detected".into(),
            attribution: Some("system".into()),
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn critical_alert_uses_all_four_channels() {
        let channels = make_all_channels();
        let (svc, history) = make_service(channels);
        let tenant_id = TenantId::new();

        let result = svc
            .route_alert(&make_alert(tenant_id, Severity::Critical))
            .await
            .unwrap();

        // CRITICAL -> page + sms + in_app + email = 4 channels
        assert_eq!(result.channels_used.len(), 4);
        assert_eq!(result.recipients.len(), 4);
        assert_eq!(result.dispatch_ids.len(), 4);
        assert_eq!(history.entry_count(), 1);
    }

    #[tokio::test]
    async fn high_alert_uses_in_app_and_email() {
        let channels = make_all_channels();
        let (svc, _) = make_service(channels);
        let tenant_id = TenantId::new();

        let result = svc
            .route_alert(&make_alert(tenant_id, Severity::High))
            .await
            .unwrap();

        assert_eq!(result.channels_used.len(), 2);
        assert!(result.channels_used.contains(&"inapp".to_string()));
        assert!(result.channels_used.contains(&"email".to_string()));
    }

    #[tokio::test]
    async fn medium_alert_uses_in_app_and_digest() {
        let channels = make_all_channels();
        let (svc, _) = make_service(channels);
        let tenant_id = TenantId::new();

        let result = svc
            .route_alert(&make_alert(tenant_id, Severity::Medium))
            .await
            .unwrap();

        assert_eq!(result.channels_used.len(), 2);
        assert!(result.channels_used.contains(&"inapp".to_string()));
        assert!(result.channels_used.contains(&"digest".to_string()));
    }

    #[tokio::test]
    async fn low_alert_uses_digest_only() {
        let channels = make_all_channels();
        let (svc, _) = make_service(channels);
        let tenant_id = TenantId::new();

        let result = svc
            .route_alert(&make_alert(tenant_id, Severity::Low))
            .await
            .unwrap();

        assert_eq!(result.channels_used.len(), 1);
        assert!(result.channels_used.contains(&"digest".to_string()));
    }

    #[tokio::test]
    async fn channel_failure_continues_with_remaining() {
        let page = Arc::new(MockChannel::new(AlertChannel::Page));
        page.set_fail(true); // page channel fails
        let sms = Arc::new(MockChannel::new(AlertChannel::Sms));
        let in_app = Arc::new(MockChannel::new(AlertChannel::InApp));
        let email = Arc::new(MockChannel::new(AlertChannel::Email));

        let channels: Vec<Arc<dyn AlertChannelDispatcher>> =
            vec![page.clone(), sms.clone(), in_app.clone(), email.clone()];
        let (svc, _) = make_service(channels);
        let tenant_id = TenantId::new();

        let result = svc
            .route_alert(&make_alert(tenant_id, Severity::Critical))
            .await
            .unwrap();

        // Page failed, so only 3 channels used
        assert_eq!(result.channels_used.len(), 3);
        assert_eq!(sms.dispatch_count(), 1);
        assert_eq!(in_app.dispatch_count(), 1);
        assert_eq!(email.dispatch_count(), 1);
    }

    #[tokio::test]
    async fn records_alert_history() {
        let channels = make_all_channels();
        let (svc, history) = make_service(channels);
        let tenant_id = TenantId::new();

        svc.route_alert(&make_alert(tenant_id, Severity::High))
            .await
            .unwrap();
        svc.route_alert(&make_alert(tenant_id, Severity::Low))
            .await
            .unwrap();

        assert_eq!(history.entry_count(), 2);
    }

    #[tokio::test]
    async fn severity_matrix_returns_correct_channel_sets() {
        // Unit test the static matrix directly
        let critical = AlertRoutingService::channels_for_severity(Severity::Critical);
        assert_eq!(critical.len(), 4);
        assert!(critical.contains(&AlertChannel::Page));
        assert!(critical.contains(&AlertChannel::Sms));

        let high = AlertRoutingService::channels_for_severity(Severity::High);
        assert_eq!(high.len(), 2);
        assert!(!high.contains(&AlertChannel::Page));

        let medium = AlertRoutingService::channels_for_severity(Severity::Medium);
        assert_eq!(medium.len(), 2);
        assert!(medium.contains(&AlertChannel::Digest));

        let low = AlertRoutingService::channels_for_severity(Severity::Low);
        assert_eq!(low.len(), 1);
        assert_eq!(low[0], AlertChannel::Digest);
    }

    #[tokio::test]
    async fn missing_channel_dispatcher_is_skipped() {
        // Only provide digest channel -- critical alert should still succeed
        // (other channels just aren't available)
        let channels: Vec<Arc<dyn AlertChannelDispatcher>> =
            vec![Arc::new(MockChannel::new(AlertChannel::Digest))];
        let (svc, _) = make_service(channels);
        let tenant_id = TenantId::new();

        let result = svc
            .route_alert(&make_alert(tenant_id, Severity::Critical))
            .await
            .unwrap();

        // Only digest was available, so only 0 channels used (digest not in CRITICAL matrix)
        assert_eq!(result.channels_used.len(), 0);
    }
}
