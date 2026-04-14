//! Append-only audit event store with Merkle hash chain (REUSABLE_AuditLogger).
//!
//! The `AuditLogger` is the single write path for all governance events.
//! It composes the `AuditEventRepository` (persistence) with the
//! `MerkleChainHasher` (integrity) to enforce append-only semantics
//! and tamper evidence.
//!
//! Implements: SR_GOV_47 (write), SR_GOV_48 (verify), SR_GOV_49 (query)

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use prism_core::error::PrismError;
use prism_core::repository::AuditEventRepository;
use prism_core::types::*;

use crate::merkle_chain::MerkleChainHasher;
use crate::tamper_response::TamperResponseService;

/// The reusable audit logger service.
///
/// Thread-safe and cheaply cloneable (the repository is behind an `Arc`).
/// Every crate that needs to emit audit events takes an `AuditLogger` handle.
///
/// Implements: REUSABLE_AuditLogger (D-22, GAP-71)
#[derive(Clone)]
pub struct AuditLogger {
    repo: Arc<dyn AuditEventRepository>,
}

impl AuditLogger {
    /// Create a new `AuditLogger` wrapping the given repository.
    pub fn new(repo: Arc<dyn AuditEventRepository>) -> Self {
        Self { repo }
    }

    /// Append an audit event to the chain.
    ///
    /// 1. Fetch the current chain head for the tenant.
    /// 2. Compute the next `chain_position` and `event_hash`.
    /// 3. Persist the event via the repository.
    ///
    /// Implements: SR_GOV_47
    pub async fn log(&self, input: AuditEventInput) -> Result<AuditCaptureResult, PrismError> {
        // Step 1: get chain head
        let head = self.repo.get_chain_head(input.tenant_id).await?;

        let (prev_hash, next_position) = match &head {
            Some(h) => (Some(h.event_hash.as_str()), h.chain_position + 1),
            None => (None, 0),
        };

        let created_at = Utc::now();

        // Step 2: compute canonical bytes and hash
        let severity_str = format!("{:?}", input.severity).to_lowercase();
        let source_layer_str = format!("{:?}", input.source_layer).to_lowercase();
        let actor_type_str = format!("{:?}", input.actor_type).to_lowercase();

        let canonical = MerkleChainHasher::canonical_bytes(
            input.tenant_id.as_uuid(),
            &input.event_type,
            &input.actor_id,
            &actor_type_str,
            input.target_id.as_ref(),
            input.target_type.as_deref(),
            &severity_str,
            &source_layer_str,
            &input.payload,
            &created_at,
        );

        let event_hash = MerkleChainHasher::compute_hash(prev_hash, &canonical);
        let event_id = AuditEventId::new();

        // Step 3: build and persist the event
        let event = AuditEvent {
            id: event_id,
            tenant_id: input.tenant_id,
            event_type: input.event_type,
            actor_id: input.actor_id,
            actor_type: input.actor_type,
            target_id: input.target_id,
            target_type: input.target_type,
            severity: input.severity,
            source_layer: input.source_layer,
            governance_authority: input.governance_authority,
            payload: input.payload,
            prev_event_hash: prev_hash.map(String::from),
            event_hash: event_hash.clone(),
            chain_position: next_position,
            created_at,
        };

        self.repo.append(&event).await?;

        info!(
            event_id = %event_id,
            tenant_id = %input.tenant_id,
            chain_position = next_position,
            "audit event appended"
        );

        Ok(AuditCaptureResult {
            event_id,
            chain_position: next_position,
            event_hash,
        })
    }

    /// Verify the integrity of the last `depth` events in a tenant's chain.
    ///
    /// Fetches the chain segment from the repository and delegates to
    /// `MerkleChainHasher::verify_chain`. On failure, logs a CRITICAL warning.
    ///
    /// Implements: SR_GOV_48
    pub async fn verify_chain(
        &self,
        tenant_id: TenantId,
        depth: u32,
    ) -> Result<ChainVerificationResult, PrismError> {
        let mut segment = self.repo.get_chain_segment(tenant_id, depth).await?;

        // Repository returns descending order; verification needs ascending.
        segment.sort_by_key(|e| e.chain_position);

        let result = MerkleChainHasher::verify_chain(&segment);

        if !result.is_valid {
            warn!(
                tenant_id = %tenant_id,
                mismatch_at = ?result.mismatch_at,
                "CRITICAL: audit chain verification FAILED -- possible tampering"
            );
        }

        Ok(result)
    }

    /// Query audit events with filters and pagination.
    ///
    /// Implements: SR_GOV_49
    pub async fn query(&self, request: &AuditQueryRequest) -> Result<AuditQueryResult, PrismError> {
        self.repo.query(request).await
    }

    /// Verify the chain and, if tampering is detected, trigger the tamper
    /// response workflow (write-freeze + alert + incident).
    ///
    /// This is the composed path: SR_GOV_48 -> SR_GOV_51.
    ///
    /// Implements: SR_GOV_48, SR_GOV_51
    pub async fn verify_and_respond(
        &self,
        tenant_id: TenantId,
        depth: u32,
        responder: &TamperResponseService,
    ) -> Result<ChainVerificationResult, PrismError> {
        let result = self.verify_chain(tenant_id, depth).await?;

        if !result.is_valid {
            let input = TamperResponseInput {
                tenant_id,
                mismatch_at: result.mismatch_at.unwrap_or(-1),
                anchor_hash: result.anchor_hash.clone(),
            };

            warn!(
                tenant_id = %tenant_id,
                mismatch_at = ?result.mismatch_at,
                "chain verification failed -- initiating tamper response"
            );

            responder.respond(&input).await?;
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tamper_response::{AlertDispatcher, IncidentTracker, TenantWriteFreeze};
    use async_trait::async_trait;
    use std::sync::Mutex;

    /// In-memory mock repository for unit testing AuditLogger without a database.
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

        async fn query(&self, request: &AuditQueryRequest) -> Result<AuditQueryResult, PrismError> {
            let events = self.events.lock().unwrap();
            let filtered: Vec<_> = events
                .iter()
                .filter(|e| e.tenant_id == request.tenant_id)
                .cloned()
                .collect();
            let total = filtered.len() as i64;
            Ok(AuditQueryResult {
                events: filtered,
                next_page_token: None,
                total_count: total,
            })
        }

        async fn get_chain_segment(
            &self,
            tenant_id: TenantId,
            depth: u32,
        ) -> Result<Vec<AuditEvent>, PrismError> {
            let events = self.events.lock().unwrap();
            let mut segment: Vec<_> = events
                .iter()
                .filter(|e| e.tenant_id == tenant_id)
                .cloned()
                .collect();
            segment.sort_by_key(|e| std::cmp::Reverse(e.chain_position));
            segment.truncate(depth as usize);
            Ok(segment)
        }
    }

    fn test_input(tenant_id: TenantId) -> AuditEventInput {
        AuditEventInput {
            tenant_id,
            event_type: "tenant.created".into(),
            actor_id: uuid::Uuid::nil(),
            actor_type: ActorType::System,
            target_id: None,
            target_type: None,
            severity: Severity::Low,
            source_layer: SourceLayer::Governance,
            governance_authority: None,
            payload: serde_json::json!({"action": "test"}),
        }
    }

    #[tokio::test]
    async fn log_genesis_event() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_id = TenantId::new();

        let result = logger.log(test_input(tenant_id)).await.unwrap();

        assert_eq!(result.chain_position, 0);
        assert_eq!(result.event_hash.len(), 64);
    }

    #[tokio::test]
    async fn log_chained_events() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_id = TenantId::new();

        let r0 = logger.log(test_input(tenant_id)).await.unwrap();
        let r1 = logger.log(test_input(tenant_id)).await.unwrap();
        let r2 = logger.log(test_input(tenant_id)).await.unwrap();

        assert_eq!(r0.chain_position, 0);
        assert_eq!(r1.chain_position, 1);
        assert_eq!(r2.chain_position, 2);

        // Hashes should all differ (different timestamps at minimum)
        assert_ne!(r0.event_hash, r1.event_hash);
        assert_ne!(r1.event_hash, r2.event_hash);
    }

    #[tokio::test]
    async fn verify_chain_passes_for_valid_chain() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_id = TenantId::new();

        logger.log(test_input(tenant_id)).await.unwrap();
        logger.log(test_input(tenant_id)).await.unwrap();
        logger.log(test_input(tenant_id)).await.unwrap();

        let result = logger.verify_chain(tenant_id, 10).await.unwrap();
        assert!(result.is_valid);
        assert_eq!(result.verified_count, 3);
    }

    #[tokio::test]
    async fn verify_chain_detects_tampering() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_id = TenantId::new();

        logger.log(test_input(tenant_id)).await.unwrap();
        logger.log(test_input(tenant_id)).await.unwrap();

        // Tamper with the stored event
        {
            let mut events = repo.events.lock().unwrap();
            events[1].event_hash = "tampered".into();
        }

        let result = logger.verify_chain(tenant_id, 10).await.unwrap();
        assert!(!result.is_valid);
        assert_eq!(result.mismatch_at, Some(1));
    }

    #[tokio::test]
    async fn query_returns_tenant_events() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();

        logger.log(test_input(tenant_a)).await.unwrap();
        logger.log(test_input(tenant_a)).await.unwrap();
        logger.log(test_input(tenant_b)).await.unwrap();

        let result = logger
            .query(&AuditQueryRequest {
                tenant_id: tenant_a,
                event_type: None,
                actor_id: None,
                target_id: None,
                severity: None,
                from_time: None,
                to_time: None,
                page_size: 100,
                page_token: None,
            })
            .await
            .unwrap();

        assert_eq!(result.total_count, 2);
    }

    #[tokio::test]
    async fn tenant_isolation_separate_chains() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();

        let a0 = logger.log(test_input(tenant_a)).await.unwrap();
        let b0 = logger.log(test_input(tenant_b)).await.unwrap();

        // Both should start at position 0
        assert_eq!(a0.chain_position, 0);
        assert_eq!(b0.chain_position, 0);

        // Both chains should verify independently
        let va = logger.verify_chain(tenant_a, 10).await.unwrap();
        let vb = logger.verify_chain(tenant_b, 10).await.unwrap();
        assert!(va.is_valid);
        assert!(vb.is_valid);
    }

    // -- Mock traits for tamper response integration -------------------------

    struct MockFreeze {
        frozen: Mutex<Vec<TenantId>>,
    }

    impl MockFreeze {
        fn new() -> Self {
            Self {
                frozen: Mutex::new(Vec::new()),
            }
        }

        fn is_frozen(&self, tenant_id: TenantId) -> bool {
            self.frozen.lock().unwrap().contains(&tenant_id)
        }
    }

    #[async_trait]
    impl TenantWriteFreeze for MockFreeze {
        async fn freeze(&self, tenant_id: TenantId) -> Result<bool, PrismError> {
            let mut frozen = self.frozen.lock().unwrap();
            if frozen.contains(&tenant_id) {
                Ok(false)
            } else {
                frozen.push(tenant_id);
                Ok(true)
            }
        }

        async fn is_frozen(&self, tenant_id: TenantId) -> Result<bool, PrismError> {
            Ok(self.frozen.lock().unwrap().contains(&tenant_id))
        }
    }

    struct MockAlerter;

    #[async_trait]
    impl AlertDispatcher for MockAlerter {
        async fn dispatch_critical(
            &self,
            _title: &str,
            _detail: serde_json::Value,
        ) -> Result<(), PrismError> {
            Ok(())
        }
    }

    struct MockIncidents {
        counter: Mutex<u32>,
    }

    impl MockIncidents {
        fn new() -> Self {
            Self {
                counter: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl IncidentTracker for MockIncidents {
        async fn create_incident(
            &self,
            _title: &str,
            _severity: Severity,
            _detail: serde_json::Value,
        ) -> Result<String, PrismError> {
            let mut counter = self.counter.lock().unwrap();
            *counter += 1;
            Ok(format!("INC-{:04}", *counter))
        }
    }

    fn make_responder(freeze: Arc<MockFreeze>) -> TamperResponseService {
        TamperResponseService::new(
            freeze,
            Arc::new(MockAlerter),
            Arc::new(MockIncidents::new()),
        )
    }

    #[tokio::test]
    async fn verify_and_respond_triggers_on_tamper() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_id = TenantId::new();

        logger.log(test_input(tenant_id)).await.unwrap();
        logger.log(test_input(tenant_id)).await.unwrap();

        // Tamper with the stored event
        {
            let mut events = repo.events.lock().unwrap();
            events[1].event_hash = "tampered".into();
        }

        let freeze = Arc::new(MockFreeze::new());
        let responder = make_responder(freeze.clone());

        let result = logger
            .verify_and_respond(tenant_id, 10, &responder)
            .await
            .unwrap();

        assert!(!result.is_valid);
        assert!(freeze.is_frozen(tenant_id));
    }

    #[tokio::test]
    async fn verify_and_respond_skips_response_when_valid() {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        let tenant_id = TenantId::new();

        logger.log(test_input(tenant_id)).await.unwrap();
        logger.log(test_input(tenant_id)).await.unwrap();

        let freeze = Arc::new(MockFreeze::new());
        let responder = make_responder(freeze.clone());

        let result = logger
            .verify_and_respond(tenant_id, 10, &responder)
            .await
            .unwrap();

        assert!(result.is_valid);
        assert!(!freeze.is_frozen(tenant_id));
    }
}
