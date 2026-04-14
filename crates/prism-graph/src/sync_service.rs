//! Event-driven sync service for cross-store consistency.
//!
//! Implements: SR_DM_22
//!
//! Provides a SyncService that processes sync events between PG and Neo4j
//! stores. Uses SyncEventBus for publish/subscribe and delegates writes
//! to GraphWriter or PgWriter depending on the target store.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::types::*;

use crate::data_model::{GraphWriter, PgWriter};
use crate::sync_coordinator::SyncState;

// ============================================================================
// SyncEventBus trait
// ============================================================================

/// Event bus for publishing and subscribing to sync events.
///
/// Implements: SR_DM_22
#[async_trait]
pub trait SyncEventBus: Send + Sync {
    /// Subscribe a handler to the event bus.
    async fn subscribe(&self, handler: Arc<dyn SyncEventHandler>) -> Result<(), PrismError>;

    /// Publish a sync event to all subscribers.
    async fn publish(&self, event: &SyncEvent) -> Result<(), PrismError>;
}

/// Handler for processing sync events.
///
/// Implements: SR_DM_22
#[async_trait]
pub trait SyncEventHandler: Send + Sync {
    /// Handle a sync event.
    async fn handle(&self, event: &SyncEvent) -> Result<(), PrismError>;
}

// ============================================================================
// SourceEntityChecker trait
// ============================================================================

/// Trait to verify that a source entity still exists.
///
/// Used by SR_DM_22_BE-01 to detect entities deleted before sync.
///
/// Implements: SR_DM_22
#[async_trait]
pub trait SourceEntityChecker: Send + Sync {
    /// Returns true if the entity exists in the source store.
    async fn entity_exists(
        &self,
        store: &str,
        entity_type: &str,
        entity_id: uuid::Uuid,
    ) -> Result<bool, PrismError>;
}

// ============================================================================
// SyncService
// ============================================================================

/// Event-driven sync service coordinating cross-store writes.
///
/// Processes sync events by:
/// 1. Checking source entity existence (SR_DM_22_BE-01).
/// 2. Attempting write to target store.
/// 3. Recording latency and state.
/// 4. Auditing failures.
///
/// Implements: SR_DM_22
pub struct SyncService {
    graph_writer: Arc<dyn GraphWriter>,
    pg_writer: Arc<dyn PgWriter>,
    source_checker: Arc<dyn SourceEntityChecker>,
    audit: AuditLogger,
}

impl SyncService {
    pub fn new(
        graph_writer: Arc<dyn GraphWriter>,
        pg_writer: Arc<dyn PgWriter>,
        source_checker: Arc<dyn SourceEntityChecker>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            graph_writer,
            pg_writer,
            source_checker,
            audit,
        }
    }

    /// Process a sync event, applying it to the target store.
    ///
    /// SR_DM_22_BE-01: if the source entity no longer exists (deleted
    /// before sync), drop the event with audit and return SyncState::Consistent
    /// (nothing to sync).
    ///
    /// SR_DM_22_SE-01: if the target store is unavailable, the event
    /// remains deferred. An alert is raised at 60s, escalated at 5min.
    ///
    /// Implements: SR_DM_22
    pub async fn process_event(&self, event: &SyncEvent) -> Result<SyncResult, PrismError> {
        let start = Utc::now();

        // SR_DM_22_BE-01: check source entity existence
        let exists = self
            .source_checker
            .entity_exists(&event.source_store, &event.entity_type, event.entity_id)
            .await?;

        if !exists {
            warn!(
                entity_id = %event.entity_id,
                entity_type = %event.entity_type,
                source_store = %event.source_store,
                "source entity deleted before sync -- dropping event"
            );

            self.audit
                .log(AuditEventInput {
                    tenant_id: event.tenant_id,
                    event_type: "data_model.sync_dropped".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: Some(event.entity_id),
                    target_type: Some(event.entity_type.clone()),
                    severity: Severity::Medium,
                    source_layer: SourceLayer::Graph,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "reason": "source_entity_deleted",
                        "source_store": event.source_store,
                        "target_store": event.target_store,
                    }),
                })
                .await?;

            let end = Utc::now();
            let latency_ms = (end - start).num_milliseconds().unsigned_abs();

            return Ok(SyncResult {
                applied_at: end,
                latency_ms,
                state: serde_json::to_value(SyncState::Consistent)
                    .map(|v| v.as_str().unwrap_or("consistent").to_string())
                    .unwrap_or_else(|_| "consistent".into()),
            });
        }

        // Attempt target store write
        let write_result = if event.target_store == "neo4j" || event.target_store == "graph" {
            self.graph_writer
                .create_node(event.tenant_id, &event.entity_type, event.payload.clone())
                .await
                .map(|_| ())
        } else {
            self.pg_writer
                .insert_row(&event.entity_type, event.payload.clone())
                .await
                .map(|_| ())
        };

        let end = Utc::now();
        let latency_ms = (end - start).num_milliseconds().unsigned_abs();

        match write_result {
            Ok(()) => {
                info!(
                    entity_id = %event.entity_id,
                    target_store = %event.target_store,
                    latency_ms,
                    "sync event applied successfully"
                );

                Ok(SyncResult {
                    applied_at: end,
                    latency_ms,
                    state: serde_json::to_value(SyncState::Consistent)
                        .map(|v| v.as_str().unwrap_or("consistent").to_string())
                        .unwrap_or_else(|_| "consistent".into()),
                })
            }
            Err(e) => {
                // SR_DM_22_SE-01: target unavailable -- defer
                warn!(
                    entity_id = %event.entity_id,
                    target_store = %event.target_store,
                    error = %e,
                    "target store unavailable -- sync deferred"
                );

                self.audit
                    .log(AuditEventInput {
                        tenant_id: event.tenant_id,
                        event_type: "data_model.sync_deferred".into(),
                        actor_id: uuid::Uuid::nil(),
                        actor_type: ActorType::System,
                        target_id: Some(event.entity_id),
                        target_type: Some(event.entity_type.clone()),
                        severity: Severity::High,
                        source_layer: SourceLayer::Graph,
                        governance_authority: None,
                        payload: serde_json::json!({
                            "reason": "target_store_unavailable",
                            "target_store": event.target_store,
                            "error": e.to_string(),
                        }),
                    })
                    .await?;

                Ok(SyncResult {
                    applied_at: end,
                    latency_ms,
                    state: serde_json::to_value(SyncState::PgOnly)
                        .map(|v| v.as_str().unwrap_or("pg_only").to_string())
                        .unwrap_or_else(|_| "pg_only".into()),
                })
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::repository::AuditEventRepository;
    use prism_core::types::AuditEvent;
    use std::sync::Mutex;

    // -- Mock GraphWriter -----------------------------------------------------

    struct MockGraphWriter {
        should_fail: bool,
        nodes: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl MockGraphWriter {
        fn new(should_fail: bool) -> Self {
            Self {
                should_fail,
                nodes: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl GraphWriter for MockGraphWriter {
        async fn create_node(
            &self,
            _tenant_id: TenantId,
            node_type: &str,
            properties: serde_json::Value,
        ) -> Result<uuid::Uuid, PrismError> {
            if self.should_fail {
                return Err(PrismError::Graph("target store unavailable".into()));
            }
            self.nodes
                .lock()
                .unwrap()
                .push((node_type.to_string(), properties));
            Ok(uuid::Uuid::new_v4())
        }
    }

    // -- Mock PgWriter --------------------------------------------------------

    struct MockPgWriter {
        rows: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl MockPgWriter {
        fn new() -> Self {
            Self {
                rows: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl PgWriter for MockPgWriter {
        async fn insert_row(
            &self,
            table: &str,
            data: serde_json::Value,
        ) -> Result<uuid::Uuid, PrismError> {
            self.rows.lock().unwrap().push((table.to_string(), data));
            Ok(uuid::Uuid::new_v4())
        }
    }

    // -- Mock SourceEntityChecker ---------------------------------------------

    struct MockSourceChecker {
        exists: bool,
    }

    impl MockSourceChecker {
        fn new(exists: bool) -> Self {
            Self { exists }
        }
    }

    #[async_trait]
    impl SourceEntityChecker for MockSourceChecker {
        async fn entity_exists(
            &self,
            _store: &str,
            _entity_type: &str,
            _entity_id: uuid::Uuid,
        ) -> Result<bool, PrismError> {
            Ok(self.exists)
        }
    }

    // -- Mock AuditEventRepository --------------------------------------------

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

    fn make_audit_logger() -> (Arc<MockAuditRepo>, AuditLogger) {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        (repo, logger)
    }

    // =========================================================================
    // SR_DM_22 tests
    // =========================================================================

    #[tokio::test]
    async fn sync_event_succeeds_on_available_target() {
        let graph_writer = Arc::new(MockGraphWriter::new(false));
        let pg_writer = Arc::new(MockPgWriter::new());
        let source_checker = Arc::new(MockSourceChecker::new(true));
        let (_audit_repo, audit) = make_audit_logger();

        let svc = SyncService::new(graph_writer, pg_writer, source_checker, audit);

        let event = SyncEvent {
            source_store: "pg".into(),
            target_store: "graph".into(),
            entity_type: "Recommendation".into(),
            entity_id: uuid::Uuid::new_v4(),
            payload: serde_json::json!({"content_hash": "abc123"}),
            tenant_id: TenantId::new(),
        };

        let result = svc.process_event(&event).await.unwrap();

        assert_eq!(result.state, "consistent");
        assert!(result.latency_ms < 1000);
    }

    #[tokio::test]
    async fn sync_event_deferred_on_target_failure() {
        let graph_writer = Arc::new(MockGraphWriter::new(true)); // will fail
        let pg_writer = Arc::new(MockPgWriter::new());
        let source_checker = Arc::new(MockSourceChecker::new(true));
        let (audit_repo, audit) = make_audit_logger();

        let svc = SyncService::new(graph_writer, pg_writer, source_checker, audit);

        let event = SyncEvent {
            source_store: "pg".into(),
            target_store: "graph".into(),
            entity_type: "Component".into(),
            entity_id: uuid::Uuid::new_v4(),
            payload: serde_json::json!({"component_id": "test"}),
            tenant_id: TenantId::new(),
        };

        let result = svc.process_event(&event).await.unwrap();

        assert_eq!(result.state, "pg_only");

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.sync_deferred");
    }

    #[tokio::test]
    async fn sync_event_dropped_on_missing_source() {
        let graph_writer = Arc::new(MockGraphWriter::new(false));
        let pg_writer = Arc::new(MockPgWriter::new());
        let source_checker = Arc::new(MockSourceChecker::new(false)); // entity gone
        let (audit_repo, audit) = make_audit_logger();

        let svc = SyncService::new(graph_writer, pg_writer, source_checker, audit);

        let event = SyncEvent {
            source_store: "pg".into(),
            target_store: "graph".into(),
            entity_type: "DataCollection".into(),
            entity_id: uuid::Uuid::new_v4(),
            payload: serde_json::json!({}),
            tenant_id: TenantId::new(),
        };

        let result = svc.process_event(&event).await.unwrap();

        // Dropped events return consistent (nothing to sync)
        assert_eq!(result.state, "consistent");

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.sync_dropped");
    }
}
