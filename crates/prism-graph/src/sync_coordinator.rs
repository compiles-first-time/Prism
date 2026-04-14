//! Hybrid store sync coordinator (REUSABLE_SyncCoordinator, SR_DM_22).
//!
//! Tracks eventual consistency between PostgreSQL (source of truth for
//! governance data) and Neo4j (graph queries, ADG, blast radius).
//!
//! For MVP this is a stub: PG-only writes mark state as `PgOnly` and
//! Neo4j sync is deferred. The coordinator records sync status so that
//! future Neo4j integration can detect and backfill missing graph nodes.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use prism_core::types::TenantId;

/// The consistency state of an entity across PG and Neo4j.
///
/// Implements: SR_DM_22 (sync state tracking)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// Both stores have been written and verified consistent.
    Consistent,
    /// Only PostgreSQL has been written. Neo4j sync is pending.
    PgOnly,
    /// Only Neo4j has been written. PG sync is pending.
    Neo4jOnly,
    /// Both stores were written but a verification mismatch was detected.
    Divergent,
    /// A compensating transaction is in progress after a partial failure.
    Compensating,
}

/// A sync event recording a cross-store write attempt.
///
/// Implements: SR_DM_22
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRecord {
    pub entity_type: String,
    pub entity_id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub state: SyncState,
    pub pg_written_at: Option<DateTime<Utc>>,
    pub neo4j_written_at: Option<DateTime<Utc>>,
    pub last_checked_at: DateTime<Utc>,
}

/// Trait for tracking cross-store sync state.
///
/// Implements: REUSABLE_SyncCoordinator
#[async_trait]
pub trait SyncCoordinator: Send + Sync {
    /// Record that an entity was written to PG only (Neo4j deferred).
    async fn record_pg_write(
        &self,
        tenant_id: TenantId,
        entity_type: &str,
        entity_id: uuid::Uuid,
    ) -> Result<SyncRecord, SyncError>;

    /// Mark an entity as consistent (both stores written).
    async fn mark_consistent(
        &self,
        tenant_id: TenantId,
        entity_id: uuid::Uuid,
    ) -> Result<(), SyncError>;

    /// Get the sync state for an entity.
    async fn get_state(&self, entity_id: uuid::Uuid) -> Result<Option<SyncRecord>, SyncError>;

    /// List all entities in a non-consistent state for a tenant.
    /// Used by the sync backfill worker.
    async fn list_pending(&self, tenant_id: TenantId) -> Result<Vec<SyncRecord>, SyncError>;
}

/// Sync coordinator errors.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("sync record not found: {0}")]
    NotFound(uuid::Uuid),

    #[error("store unavailable: {0}")]
    StoreUnavailable(String),

    #[error("internal sync error: {0}")]
    Internal(String),
}

/// In-memory sync coordinator for development and testing.
///
/// Stores sync records in a `Vec` behind a `Mutex`. In production this
/// would be backed by a `sync_state` table in PostgreSQL.
pub struct InMemorySyncCoordinator {
    records: std::sync::Mutex<Vec<SyncRecord>>,
}

impl InMemorySyncCoordinator {
    pub fn new() -> Self {
        Self {
            records: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Count of tracked entities (test helper).
    pub fn len(&self) -> usize {
        self.records.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InMemorySyncCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SyncCoordinator for InMemorySyncCoordinator {
    async fn record_pg_write(
        &self,
        tenant_id: TenantId,
        entity_type: &str,
        entity_id: uuid::Uuid,
    ) -> Result<SyncRecord, SyncError> {
        let now = Utc::now();
        let record = SyncRecord {
            entity_type: entity_type.to_string(),
            entity_id,
            tenant_id,
            state: SyncState::PgOnly,
            pg_written_at: Some(now),
            neo4j_written_at: None,
            last_checked_at: now,
        };
        self.records.lock().unwrap().push(record.clone());
        Ok(record)
    }

    async fn mark_consistent(
        &self,
        _tenant_id: TenantId,
        entity_id: uuid::Uuid,
    ) -> Result<(), SyncError> {
        let mut records = self.records.lock().unwrap();
        let record = records
            .iter_mut()
            .find(|r| r.entity_id == entity_id)
            .ok_or(SyncError::NotFound(entity_id))?;
        record.state = SyncState::Consistent;
        record.neo4j_written_at = Some(Utc::now());
        record.last_checked_at = Utc::now();
        Ok(())
    }

    async fn get_state(&self, entity_id: uuid::Uuid) -> Result<Option<SyncRecord>, SyncError> {
        let records = self.records.lock().unwrap();
        Ok(records.iter().find(|r| r.entity_id == entity_id).cloned())
    }

    async fn list_pending(&self, tenant_id: TenantId) -> Result<Vec<SyncRecord>, SyncError> {
        let records = self.records.lock().unwrap();
        Ok(records
            .iter()
            .filter(|r| r.tenant_id == tenant_id && r.state != SyncState::Consistent)
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn record_pg_write_creates_pg_only_record() {
        let coord = InMemorySyncCoordinator::new();
        let tid = TenantId::new();
        let eid = uuid::Uuid::new_v4();

        let record = coord.record_pg_write(tid, "Tenant", eid).await.unwrap();

        assert_eq!(record.state, SyncState::PgOnly);
        assert!(record.pg_written_at.is_some());
        assert!(record.neo4j_written_at.is_none());
        assert_eq!(coord.len(), 1);
    }

    #[tokio::test]
    async fn mark_consistent_updates_state() {
        let coord = InMemorySyncCoordinator::new();
        let tid = TenantId::new();
        let eid = uuid::Uuid::new_v4();

        coord.record_pg_write(tid, "Tenant", eid).await.unwrap();
        coord.mark_consistent(tid, eid).await.unwrap();

        let record = coord.get_state(eid).await.unwrap().unwrap();
        assert_eq!(record.state, SyncState::Consistent);
        assert!(record.neo4j_written_at.is_some());
    }

    #[tokio::test]
    async fn mark_consistent_nonexistent_returns_error() {
        let coord = InMemorySyncCoordinator::new();
        let result = coord
            .mark_consistent(TenantId::new(), uuid::Uuid::new_v4())
            .await;
        assert!(matches!(result, Err(SyncError::NotFound(_))));
    }

    #[tokio::test]
    async fn list_pending_filters_by_tenant_and_state() {
        let coord = InMemorySyncCoordinator::new();
        let t1 = TenantId::new();
        let t2 = TenantId::new();
        let e1 = uuid::Uuid::new_v4();
        let e2 = uuid::Uuid::new_v4();
        let e3 = uuid::Uuid::new_v4();

        coord.record_pg_write(t1, "Tenant", e1).await.unwrap();
        coord.record_pg_write(t1, "User", e2).await.unwrap();
        coord.record_pg_write(t2, "Tenant", e3).await.unwrap();

        // Mark one as consistent
        coord.mark_consistent(t1, e1).await.unwrap();

        let pending = coord.list_pending(t1).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].entity_id, e2);

        let pending_t2 = coord.list_pending(t2).await.unwrap();
        assert_eq!(pending_t2.len(), 1);
    }

    #[tokio::test]
    async fn get_state_returns_none_for_unknown() {
        let coord = InMemorySyncCoordinator::new();
        let result = coord.get_state(uuid::Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }
}
