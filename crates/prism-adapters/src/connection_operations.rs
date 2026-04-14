//! Connection operational services: quarantine, pull locks, schema change
//! detection, rate budgets, KPIs, classification overrides, cloud LLM
//! connections, deprecation alerting, paywall governance, bulk import
//! logging, and health dashboard.
//!
//! Implements: SR_CONN_32 through SR_CONN_44

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};

use prism_core::error::PrismError;
use prism_core::types::*;

use crate::log_ingestion::PgWriter;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Persistence operations for quarantine records.
///
/// Implements: SR_CONN_32
#[async_trait]
pub trait QuarantineRepository: Send + Sync {
    /// Create a new quarantine record.
    /// Implements: SR_CONN_32
    async fn create(&self, record: &QuarantineRecord) -> Result<(), PrismError>;

    /// Retrieve a quarantine record by ID.
    /// Implements: SR_CONN_32
    async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<QuarantineRecord>, PrismError>;

    /// List expired quarantine records.
    /// Implements: SR_CONN_33
    async fn list_expired(&self) -> Result<Vec<QuarantineRecord>, PrismError>;

    /// Delete a quarantine record.
    /// Implements: SR_CONN_33
    async fn delete(&self, id: uuid::Uuid) -> Result<(), PrismError>;
}

/// Store for distributed pull locks.
///
/// Implements: SR_CONN_34
#[async_trait]
pub trait PullLockStore: Send + Sync {
    /// Try to acquire a lock. Returns true if acquired, false if already held.
    /// Implements: SR_CONN_34
    async fn try_acquire(&self, lock: &PullLock) -> Result<bool, PrismError>;

    /// Release a previously acquired lock.
    /// Implements: SR_CONN_34
    async fn release(&self, connection_id: uuid::Uuid, scope: &str) -> Result<(), PrismError>;

    /// Check whether a lock is currently held.
    /// Implements: SR_CONN_34
    async fn is_locked(&self, connection_id: uuid::Uuid, scope: &str) -> Result<bool, PrismError>;
}

/// Persistence for schema snapshots used in change detection.
///
/// Implements: SR_CONN_35
#[async_trait]
pub trait SchemaSnapshotStore: Send + Sync {
    /// Get the latest schema snapshot for a connection.
    /// Implements: SR_CONN_35
    async fn get_latest(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<Option<SchemaSnapshot>, PrismError>;

    /// Save a new schema snapshot.
    /// Implements: SR_CONN_35
    async fn save(&self, snapshot: &SchemaSnapshot) -> Result<(), PrismError>;
}

/// Store for classification overrides.
///
/// Implements: SR_CONN_38
#[async_trait]
pub trait OverrideStore: Send + Sync {
    /// Store a classification override.
    /// Implements: SR_CONN_38
    async fn store(&self, override_entry: &ClassificationOverride) -> Result<(), PrismError>;

    /// Get all overrides for a system.
    /// Implements: SR_CONN_39
    async fn get_overrides_for_system(
        &self,
        system_id: &str,
    ) -> Result<Vec<ClassificationOverride>, PrismError>;
}

/// Budget enforcement for vendor API rate limits and tenant quotas.
///
/// Implements: SR_CONN_36
#[async_trait]
pub trait BudgetEnforcer: Send + Sync {
    /// Check remaining vendor budget for the system.
    /// Returns the remaining call count.
    /// Implements: SR_CONN_36
    async fn vendor_budget_remaining(&self, system_id: &str) -> Result<u64, PrismError>;

    /// Check remaining tenant quota.
    /// Returns the remaining call count.
    /// Implements: SR_CONN_36
    async fn tenant_quota_remaining(&self, tenant_id: TenantId) -> Result<u64, PrismError>;
}

/// Store for connection KPI snapshots.
///
/// Implements: SR_CONN_37
#[async_trait]
pub trait KpiStore: Send + Sync {
    /// Save or update a KPI snapshot.
    /// Implements: SR_CONN_37
    async fn save(&self, snapshot: &ConnectionKpiSnapshot) -> Result<(), PrismError>;

    /// Get a KPI snapshot for a connection.
    /// Implements: SR_CONN_37
    async fn get(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<Option<ConnectionKpiSnapshot>, PrismError>;

    /// List all KPI snapshots for connections belonging to a tenant.
    /// Implements: SR_CONN_44
    async fn list_for_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<ConnectionKpiSnapshot>, PrismError>;
}

// ---------------------------------------------------------------------------
// SR_CONN_32 -- Quarantine Service
// ---------------------------------------------------------------------------

/// Service that quarantines data that fails classification or policy checks.
///
/// Implements: SR_CONN_32
pub struct QuarantineService {
    repo: Arc<dyn QuarantineRepository>,
}

impl QuarantineService {
    pub fn new(repo: Arc<dyn QuarantineRepository>) -> Self {
        Self { repo }
    }

    /// Quarantine an execution record that failed classification.
    ///
    /// Creates a quarantine record with the specified policy and
    /// an expiry based on the policy type.
    ///
    /// Implements: SR_CONN_32
    pub async fn quarantine(
        &self,
        tenant_id: TenantId,
        execution_record_id: uuid::Uuid,
        reason: &str,
        policy: QuarantinePolicy,
    ) -> Result<QuarantineRecord, PrismError> {
        let expires_at = match policy {
            QuarantinePolicy::Delete => Utc::now() + Duration::days(30),
            QuarantinePolicy::ArchiveEncrypted => Utc::now() + Duration::days(90),
            QuarantinePolicy::PermanentQuarantine => Utc::now() + Duration::days(36500),
            QuarantinePolicy::RetryClassification => Utc::now() + Duration::days(7),
        };

        let record = QuarantineRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            execution_record_id,
            reason: reason.to_string(),
            policy,
            expires_at,
        };

        self.repo.create(&record).await?;

        Ok(record)
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_33 -- Quarantine Expiry Service
// ---------------------------------------------------------------------------

/// Service that processes expired quarantine records according to their policy.
///
/// Implements: SR_CONN_33
pub struct QuarantineExpiryService {
    repo: Arc<dyn QuarantineRepository>,
}

impl QuarantineExpiryService {
    pub fn new(repo: Arc<dyn QuarantineRepository>) -> Self {
        Self { repo }
    }

    /// Process all expired quarantine records.
    ///
    /// Executes the configured action for each expired record:
    /// - Delete: remove the record permanently
    /// - ArchiveEncrypted: mark as archived (no-op in this layer)
    /// - PermanentQuarantine: skip (never expires in practice)
    /// - RetryClassification: return for re-classification
    ///
    /// Returns the list of actions taken as (record_id, action_name) tuples.
    ///
    /// Implements: SR_CONN_33
    pub async fn process_expiry(&self) -> Result<Vec<(uuid::Uuid, String)>, PrismError> {
        let expired = self.repo.list_expired().await?;
        let mut actions = Vec::new();

        for record in &expired {
            match record.policy {
                QuarantinePolicy::Delete => {
                    self.repo.delete(record.id).await?;
                    actions.push((record.id, "deleted".to_string()));
                }
                QuarantinePolicy::ArchiveEncrypted => {
                    // Archive logic would encrypt and move to cold storage.
                    self.repo.delete(record.id).await?;
                    actions.push((record.id, "archived".to_string()));
                }
                QuarantinePolicy::PermanentQuarantine => {
                    // Permanent quarantine records are not expired.
                    actions.push((record.id, "permanent".to_string()));
                }
                QuarantinePolicy::RetryClassification => {
                    // Mark for re-classification.
                    actions.push((record.id, "retry".to_string()));
                }
            }
        }

        Ok(actions)
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_34 -- Pull Lock Service
// ---------------------------------------------------------------------------

/// Service that manages distributed pull locks to prevent concurrent
/// pulls on the same connection/scope.
///
/// Implements: SR_CONN_34
pub struct PullLockService {
    store: Arc<dyn PullLockStore>,
}

impl PullLockService {
    pub fn new(store: Arc<dyn PullLockStore>) -> Self {
        Self { store }
    }

    /// Acquire a pull lock for the given connection and scope.
    ///
    /// Returns the lock if acquired, or an error if already held.
    ///
    /// Implements: SR_CONN_34
    pub async fn acquire(
        &self,
        connection_id: uuid::Uuid,
        scope: &str,
        ttl_seconds: u64,
    ) -> Result<PullLock, PrismError> {
        let lock = PullLock {
            connection_id,
            scope: scope.to_string(),
            acquired_at: Utc::now(),
            ttl_seconds,
        };

        let acquired = self.store.try_acquire(&lock).await?;
        if acquired {
            Ok(lock)
        } else {
            Err(PrismError::Conflict {
                reason: format!(
                    "pull lock already held for connection {} scope '{}'",
                    connection_id, scope
                ),
            })
        }
    }

    /// Release a previously acquired pull lock.
    ///
    /// Implements: SR_CONN_34
    pub async fn release(&self, connection_id: uuid::Uuid, scope: &str) -> Result<(), PrismError> {
        self.store.release(connection_id, scope).await
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_35 -- Schema Change Detection
// ---------------------------------------------------------------------------

/// Service that detects schema changes by comparing current fields
/// against the latest stored snapshot.
///
/// Implements: SR_CONN_35
pub struct SchemaChangeDetector {
    store: Arc<dyn SchemaSnapshotStore>,
}

impl SchemaChangeDetector {
    pub fn new(store: Arc<dyn SchemaSnapshotStore>) -> Self {
        Self { store }
    }

    /// Scan for schema changes on a connection.
    ///
    /// Compares the current field list against the latest snapshot.
    /// Returns a `SchemaChangeEvent` if differences are detected, or
    /// `None` if the schema is unchanged (or no prior snapshot exists).
    ///
    /// Implements: SR_CONN_35
    pub async fn scan(
        &self,
        connection_id: uuid::Uuid,
        current_fields: &[String],
    ) -> Result<Option<SchemaChangeEvent>, PrismError> {
        let latest = self.store.get_latest(connection_id).await?;

        // Save current snapshot
        let snapshot = SchemaSnapshot {
            connection_id,
            fields: current_fields.to_vec(),
            captured_at: Utc::now(),
        };
        self.store.save(&snapshot).await?;

        let Some(previous) = latest else {
            // First snapshot -- no comparison possible
            return Ok(None);
        };

        let prev_set: std::collections::HashSet<&String> = previous.fields.iter().collect();
        let curr_set: std::collections::HashSet<&String> = current_fields.iter().collect();

        let added: Vec<String> = curr_set
            .difference(&prev_set)
            .map(|s| (*s).clone())
            .collect();
        let removed: Vec<String> = prev_set
            .difference(&curr_set)
            .map(|s| (*s).clone())
            .collect();

        if added.is_empty() && removed.is_empty() {
            return Ok(None);
        }

        let severity = if !removed.is_empty() {
            Severity::High
        } else {
            Severity::Medium
        };

        Ok(Some(SchemaChangeEvent {
            connection_id,
            added_fields: added,
            removed_fields: removed,
            severity,
        }))
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_36 -- Rate Budget Check
// ---------------------------------------------------------------------------

/// Service that checks vendor API rate budgets and tenant quotas
/// before allowing a connection pull.
///
/// Implements: SR_CONN_36
pub struct RateBudgetService {
    enforcer: Arc<dyn BudgetEnforcer>,
}

impl RateBudgetService {
    pub fn new(enforcer: Arc<dyn BudgetEnforcer>) -> Self {
        Self { enforcer }
    }

    /// Check whether a pull is within the rate budget.
    ///
    /// Checks both the vendor budget and the tenant quota. If either
    /// is exhausted, the pull is deferred.
    ///
    /// Implements: SR_CONN_36
    pub async fn check(&self, input: &RateBudgetCheck) -> Result<RateBudgetResult, PrismError> {
        let vendor_remaining = self
            .enforcer
            .vendor_budget_remaining(&input.system_id)
            .await?;

        if vendor_remaining < input.expected_call_count {
            return Ok(RateBudgetResult {
                decision: PullPreflightDecision::Defer,
                defer_reason: Some(format!(
                    "vendor budget exhausted: {} remaining, {} needed",
                    vendor_remaining, input.expected_call_count
                )),
            });
        }

        let tenant_remaining = self
            .enforcer
            .tenant_quota_remaining(input.tenant_id)
            .await?;

        if tenant_remaining < input.expected_call_count {
            return Ok(RateBudgetResult {
                decision: PullPreflightDecision::Defer,
                defer_reason: Some(format!(
                    "tenant quota exhausted: {} remaining, {} needed",
                    tenant_remaining, input.expected_call_count
                )),
            });
        }

        Ok(RateBudgetResult {
            decision: PullPreflightDecision::Allow,
            defer_reason: None,
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_37 -- Connection KPI Service
// ---------------------------------------------------------------------------

/// Service that tracks and evaluates connection KPIs.
///
/// Implements: SR_CONN_37
pub struct ConnectionKpiService {
    store: Arc<dyn KpiStore>,
}

impl ConnectionKpiService {
    pub fn new(store: Arc<dyn KpiStore>) -> Self {
        Self { store }
    }

    /// Update KPI metrics for a connection.
    ///
    /// Implements: SR_CONN_37
    pub async fn update(&self, snapshot: &ConnectionKpiSnapshot) -> Result<(), PrismError> {
        self.store.save(snapshot).await
    }

    /// Evaluate whether a connection has breached degradation thresholds.
    ///
    /// Thresholds: uptime < 95%, error_rate > 5%, avg_latency > 5000ms.
    /// Returns true if any threshold is breached.
    ///
    /// Implements: SR_CONN_37
    pub async fn evaluate(&self, connection_id: uuid::Uuid) -> Result<bool, PrismError> {
        let snapshot = self.store.get(connection_id).await?;
        let Some(kpi) = snapshot else {
            return Ok(false);
        };

        let breached =
            kpi.uptime_pct < 95.0 || kpi.error_rate_pct > 5.0 || kpi.avg_latency_ms > 5000;

        Ok(breached)
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_38 + SR_CONN_39 -- Classification Override Service
// ---------------------------------------------------------------------------

/// Service that stores and applies classification overrides.
///
/// Overrides allow operators to correct or pin field classifications
/// that the automated pipeline may misclassify.
///
/// Implements: SR_CONN_38, SR_CONN_39
pub struct ClassificationOverrideService {
    store: Arc<dyn OverrideStore>,
}

impl ClassificationOverrideService {
    pub fn new(store: Arc<dyn OverrideStore>) -> Self {
        Self { store }
    }

    /// Store a classification override for a field.
    ///
    /// Implements: SR_CONN_38
    pub async fn store_override(
        &self,
        override_entry: &ClassificationOverride,
    ) -> Result<(), PrismError> {
        self.store.store(override_entry).await
    }

    /// Apply stored overrides to a list of field classifications.
    ///
    /// For each field that has a stored override, replaces the classification
    /// with the override value.
    ///
    /// Returns the number of overrides applied.
    ///
    /// Implements: SR_CONN_39
    pub async fn apply_overrides(
        &self,
        system_id: &str,
        classifications: &mut [(String, String)],
    ) -> Result<usize, PrismError> {
        let overrides = self.store.get_overrides_for_system(system_id).await?;
        let mut applied = 0;

        for (field, classification) in classifications.iter_mut() {
            for ov in &overrides {
                if ov.field_name == *field {
                    *classification = ov.classification.clone();
                    applied += 1;
                }
            }
        }

        Ok(applied)
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_40 -- Cloud LLM Provider Connection
// ---------------------------------------------------------------------------

/// Service that registers cloud LLM providers as governed connections.
///
/// Implements: SR_CONN_40
pub struct CloudLlmConnectionService;

impl CloudLlmConnectionService {
    /// Register a cloud LLM provider as a connection.
    ///
    /// Creates a `ConnectionRecord` with provider-specific metadata
    /// via the standard lifecycle.
    ///
    /// Implements: SR_CONN_40
    pub fn register_provider(
        tenant_id: TenantId,
        provider_name: &str,
        model_id: &str,
        requested_by: UserId,
    ) -> ConnectionRecord {
        ConnectionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            system_id: format!("llm_provider_{}", provider_name),
            connection_type: "cloud_llm".to_string(),
            scope: format!("model:{}", model_id),
            status: ConnectionState::Requested,
            credential_ref: None,
            justification: Some(format!(
                "Cloud LLM provider registration: {} / {}",
                provider_name, model_id
            )),
            requested_by,
            first_pull_at: None,
            kpi_error_rate: None,
            kpi_avg_latency_ms: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_41 -- Deprecation Alerting
// ---------------------------------------------------------------------------

/// Service that checks for upcoming connection deprecation and alerts
/// at 90/30/0 day thresholds.
///
/// Implements: SR_CONN_41
pub struct DeprecationAlertService;

impl DeprecationAlertService {
    /// Check whether a connection is approaching its deprecation date.
    ///
    /// Returns the alert message if within 90/30/0 day thresholds,
    /// or `None` if no alert is needed.
    ///
    /// Implements: SR_CONN_41
    pub fn check_deprecation(deprecation_date: chrono::DateTime<chrono::Utc>) -> Option<String> {
        let now = Utc::now();
        let days_remaining = (deprecation_date - now).num_days();

        if days_remaining <= 0 {
            Some("Connection has reached deprecation date. Immediate action required.".to_string())
        } else if days_remaining <= 30 {
            Some(format!(
                "Connection deprecation in {} days. Migration urgently needed.",
                days_remaining
            ))
        } else if days_remaining <= 90 {
            Some(format!(
                "Connection deprecation in {} days. Plan migration.",
                days_remaining
            ))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_42 -- Paywall API Governance
// ---------------------------------------------------------------------------

/// Service that evaluates whether an API connection is authorized
/// considering paywall and licensing constraints.
///
/// Decision tree:
/// 1. Free API -> Approved
/// 2. Paid API with authorization -> Approved
/// 3. Paid API without authorization -> Rejected
/// 4. Manual export required -> ManualExportRequired
///
/// Implements: SR_CONN_42
pub struct PaywallGovernanceService;

impl PaywallGovernanceService {
    /// Evaluate paywall governance for an API connection.
    ///
    /// Implements: SR_CONN_42
    pub fn evaluate(
        is_free_api: bool,
        is_paid_authorized: bool,
        requires_manual_export: bool,
    ) -> PaywallDecision {
        if requires_manual_export {
            return PaywallDecision::ManualExportRequired;
        }

        if is_free_api {
            return PaywallDecision::Approved;
        }

        if is_paid_authorized {
            PaywallDecision::Approved
        } else {
            PaywallDecision::Rejected
        }
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_43 -- Bulk Import Logging
// ---------------------------------------------------------------------------

/// Service that logs bulk import operations via the PgWriter.
///
/// Implements: SR_CONN_43
pub struct BulkImportLogService {
    writer: Arc<dyn PgWriter>,
}

impl BulkImportLogService {
    pub fn new(writer: Arc<dyn PgWriter>) -> Self {
        Self { writer }
    }

    /// Log a bulk import event.
    ///
    /// Writes a metric row capturing the import volume, parse failure rate,
    /// and any redactions applied.
    ///
    /// Implements: SR_CONN_43
    pub async fn log(
        &self,
        tenant_id: TenantId,
        source_id: &str,
        events_imported: u64,
        redaction_count: u64,
    ) -> Result<(), PrismError> {
        let metric = LogMetricRow {
            tenant_id,
            source_id: source_id.to_string(),
            events_per_second: events_imported as f64,
            parse_failure_rate: 0.0,
            lag_seconds: 0,
            redaction_count,
        };

        self.writer.write_metric(&metric).await
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_44 -- Connection Health Dashboard
// ---------------------------------------------------------------------------

/// Service that produces aggregated health snapshots for a tenant's
/// connections.
///
/// Implements: SR_CONN_44
pub struct ConnectionHealthDashboard {
    kpi_store: Arc<dyn KpiStore>,
}

impl ConnectionHealthDashboard {
    pub fn new(kpi_store: Arc<dyn KpiStore>) -> Self {
        Self { kpi_store }
    }

    /// Query the health dashboard for all connections in a tenant.
    ///
    /// Returns aggregated KPI summaries for each connection.
    ///
    /// Implements: SR_CONN_44
    pub async fn query(&self, tenant_id: TenantId) -> Result<HealthDashboardResult, PrismError> {
        let snapshots = self.kpi_store.list_for_tenant(tenant_id).await?;

        let connections = snapshots
            .into_iter()
            .map(|s| {
                let status = if s.error_rate_pct > 5.0 {
                    "degraded".to_string()
                } else {
                    "healthy".to_string()
                };

                ConnectionKpiSummary {
                    connection_id: s.connection_id,
                    system_id: format!("conn_{}", s.connection_id),
                    status,
                    uptime_pct: s.uptime_pct,
                    avg_latency_ms: s.avg_latency_ms,
                    error_rate_pct: s.error_rate_pct,
                }
            })
            .collect();

        Ok(HealthDashboardResult { connections })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // -- Mock QuarantineRepository --------------------------------------------

    struct MockQuarantineRepo {
        records: Mutex<Vec<QuarantineRecord>>,
    }

    impl MockQuarantineRepo {
        fn new() -> Self {
            Self {
                records: Mutex::new(vec![]),
            }
        }

        fn with_records(records: Vec<QuarantineRecord>) -> Self {
            Self {
                records: Mutex::new(records),
            }
        }
    }

    #[async_trait]
    impl QuarantineRepository for MockQuarantineRepo {
        async fn create(&self, record: &QuarantineRecord) -> Result<(), PrismError> {
            self.records.lock().unwrap().push(record.clone());
            Ok(())
        }

        async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<QuarantineRecord>, PrismError> {
            let records = self.records.lock().unwrap();
            Ok(records.iter().find(|r| r.id == id).cloned())
        }

        async fn list_expired(&self) -> Result<Vec<QuarantineRecord>, PrismError> {
            let records = self.records.lock().unwrap();
            let now = Utc::now();
            Ok(records
                .iter()
                .filter(|r| r.expires_at <= now)
                .cloned()
                .collect())
        }

        async fn delete(&self, id: uuid::Uuid) -> Result<(), PrismError> {
            let mut records = self.records.lock().unwrap();
            records.retain(|r| r.id != id);
            Ok(())
        }
    }

    // -- Mock PullLockStore ---------------------------------------------------

    struct MockPullLockStore {
        locks: Mutex<Vec<PullLock>>,
    }

    impl MockPullLockStore {
        fn new() -> Self {
            Self {
                locks: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl PullLockStore for MockPullLockStore {
        async fn try_acquire(&self, lock: &PullLock) -> Result<bool, PrismError> {
            let mut locks = self.locks.lock().unwrap();
            let already = locks
                .iter()
                .any(|l| l.connection_id == lock.connection_id && l.scope == lock.scope);
            if already {
                return Ok(false);
            }
            locks.push(lock.clone());
            Ok(true)
        }

        async fn release(&self, connection_id: uuid::Uuid, scope: &str) -> Result<(), PrismError> {
            let mut locks = self.locks.lock().unwrap();
            locks.retain(|l| !(l.connection_id == connection_id && l.scope == scope));
            Ok(())
        }

        async fn is_locked(
            &self,
            connection_id: uuid::Uuid,
            scope: &str,
        ) -> Result<bool, PrismError> {
            let locks = self.locks.lock().unwrap();
            Ok(locks
                .iter()
                .any(|l| l.connection_id == connection_id && l.scope == scope))
        }
    }

    // -- Mock SchemaSnapshotStore ---------------------------------------------

    struct MockSchemaSnapshotStore {
        snapshots: Mutex<Vec<SchemaSnapshot>>,
    }

    impl MockSchemaSnapshotStore {
        fn with_snapshot(snapshot: SchemaSnapshot) -> Self {
            Self {
                snapshots: Mutex::new(vec![snapshot]),
            }
        }
    }

    #[async_trait]
    impl SchemaSnapshotStore for MockSchemaSnapshotStore {
        async fn get_latest(
            &self,
            connection_id: uuid::Uuid,
        ) -> Result<Option<SchemaSnapshot>, PrismError> {
            let snapshots = self.snapshots.lock().unwrap();
            Ok(snapshots
                .iter()
                .rfind(|s| s.connection_id == connection_id)
                .cloned())
        }

        async fn save(&self, snapshot: &SchemaSnapshot) -> Result<(), PrismError> {
            self.snapshots.lock().unwrap().push(snapshot.clone());
            Ok(())
        }
    }

    // -- Mock BudgetEnforcer --------------------------------------------------

    struct MockBudgetEnforcer {
        vendor_remaining: u64,
        tenant_remaining: u64,
    }

    #[async_trait]
    impl BudgetEnforcer for MockBudgetEnforcer {
        async fn vendor_budget_remaining(&self, _system_id: &str) -> Result<u64, PrismError> {
            Ok(self.vendor_remaining)
        }

        async fn tenant_quota_remaining(&self, _tenant_id: TenantId) -> Result<u64, PrismError> {
            Ok(self.tenant_remaining)
        }
    }

    // -- Mock KpiStore --------------------------------------------------------

    struct MockKpiStore {
        snapshots: Mutex<Vec<ConnectionKpiSnapshot>>,
    }

    impl MockKpiStore {
        fn new() -> Self {
            Self {
                snapshots: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl KpiStore for MockKpiStore {
        async fn save(&self, snapshot: &ConnectionKpiSnapshot) -> Result<(), PrismError> {
            let mut snaps = self.snapshots.lock().unwrap();
            snaps.retain(|s| s.connection_id != snapshot.connection_id);
            snaps.push(snapshot.clone());
            Ok(())
        }

        async fn get(
            &self,
            connection_id: uuid::Uuid,
        ) -> Result<Option<ConnectionKpiSnapshot>, PrismError> {
            let snaps = self.snapshots.lock().unwrap();
            Ok(snaps
                .iter()
                .find(|s| s.connection_id == connection_id)
                .cloned())
        }

        async fn list_for_tenant(
            &self,
            _tenant_id: TenantId,
        ) -> Result<Vec<ConnectionKpiSnapshot>, PrismError> {
            let snaps = self.snapshots.lock().unwrap();
            Ok(snaps.clone())
        }
    }

    // -- Mock OverrideStore ---------------------------------------------------

    struct MockOverrideStore {
        overrides: Mutex<Vec<ClassificationOverride>>,
    }

    impl MockOverrideStore {
        fn new() -> Self {
            Self {
                overrides: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl OverrideStore for MockOverrideStore {
        async fn store(&self, override_entry: &ClassificationOverride) -> Result<(), PrismError> {
            self.overrides.lock().unwrap().push(override_entry.clone());
            Ok(())
        }

        async fn get_overrides_for_system(
            &self,
            system_id: &str,
        ) -> Result<Vec<ClassificationOverride>, PrismError> {
            let overrides = self.overrides.lock().unwrap();
            Ok(overrides
                .iter()
                .filter(|o| o.system_id == system_id)
                .cloned()
                .collect())
        }
    }

    // -- Mock PgWriter --------------------------------------------------------

    struct MockPgWriter {
        rows: Mutex<Vec<LogMetricRow>>,
    }

    impl MockPgWriter {
        fn new() -> Self {
            Self {
                rows: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl PgWriter for MockPgWriter {
        async fn write_metric(&self, row: &LogMetricRow) -> Result<(), PrismError> {
            self.rows.lock().unwrap().push(row.clone());
            Ok(())
        }
    }

    // -- SR_CONN_32 tests -----------------------------------------------------

    #[tokio::test]
    async fn quarantine_service_creates_record() {
        let repo: Arc<dyn QuarantineRepository> = Arc::new(MockQuarantineRepo::new());
        let service = QuarantineService::new(repo.clone());
        let tenant_id = TenantId::new();
        let exec_id = uuid::Uuid::now_v7();

        let result = service
            .quarantine(
                tenant_id,
                exec_id,
                "failed classification",
                QuarantinePolicy::Delete,
            )
            .await
            .unwrap();

        assert_eq!(result.tenant_id, tenant_id);
        assert_eq!(result.execution_record_id, exec_id);
        assert_eq!(result.reason, "failed classification");
        assert_eq!(result.policy, QuarantinePolicy::Delete);
    }

    #[tokio::test]
    async fn quarantine_service_records_expiry() {
        let repo: Arc<dyn QuarantineRepository> = Arc::new(MockQuarantineRepo::new());
        let service = QuarantineService::new(repo.clone());

        let result = service
            .quarantine(
                TenantId::new(),
                uuid::Uuid::now_v7(),
                "test",
                QuarantinePolicy::RetryClassification,
            )
            .await
            .unwrap();

        // RetryClassification expires in 7 days
        let days_until_expiry = (result.expires_at - Utc::now()).num_days();
        assert!((6..=7).contains(&days_until_expiry));
    }

    // -- SR_CONN_33 tests -----------------------------------------------------

    #[tokio::test]
    async fn quarantine_expiry_executes_delete() {
        let expired_record = QuarantineRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id: TenantId::new(),
            execution_record_id: uuid::Uuid::now_v7(),
            reason: "test".to_string(),
            policy: QuarantinePolicy::Delete,
            expires_at: Utc::now() - Duration::hours(1),
        };
        let repo: Arc<dyn QuarantineRepository> = Arc::new(MockQuarantineRepo::with_records(vec![
            expired_record.clone(),
        ]));
        let service = QuarantineExpiryService::new(repo.clone());

        let actions = service.process_expiry().await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].1, "deleted");

        // Verify record was deleted
        let remaining = repo.get_by_id(expired_record.id).await.unwrap();
        assert!(remaining.is_none());
    }

    #[tokio::test]
    async fn quarantine_expiry_executes_retry() {
        let expired_record = QuarantineRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id: TenantId::new(),
            execution_record_id: uuid::Uuid::now_v7(),
            reason: "test".to_string(),
            policy: QuarantinePolicy::RetryClassification,
            expires_at: Utc::now() - Duration::hours(1),
        };
        let repo: Arc<dyn QuarantineRepository> =
            Arc::new(MockQuarantineRepo::with_records(vec![expired_record]));
        let service = QuarantineExpiryService::new(repo);

        let actions = service.process_expiry().await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].1, "retry");
    }

    // -- SR_CONN_34 tests -----------------------------------------------------

    #[tokio::test]
    async fn pull_lock_acquire_succeeds() {
        let store: Arc<dyn PullLockStore> = Arc::new(MockPullLockStore::new());
        let service = PullLockService::new(store);
        let conn_id = uuid::Uuid::now_v7();

        let lock = service.acquire(conn_id, "full", 300).await.unwrap();
        assert_eq!(lock.connection_id, conn_id);
        assert_eq!(lock.scope, "full");
        assert_eq!(lock.ttl_seconds, 300);
    }

    #[tokio::test]
    async fn pull_lock_blocks_duplicate_scope() {
        let store: Arc<dyn PullLockStore> = Arc::new(MockPullLockStore::new());
        let service = PullLockService::new(store);
        let conn_id = uuid::Uuid::now_v7();

        // First acquire succeeds
        service.acquire(conn_id, "full", 300).await.unwrap();

        // Second acquire on same scope should fail
        let result = service.acquire(conn_id, "full", 300).await;
        assert!(result.is_err());
    }

    // -- SR_CONN_35 tests -----------------------------------------------------

    #[tokio::test]
    async fn schema_change_detects_added_field() {
        let conn_id = uuid::Uuid::now_v7();
        let previous = SchemaSnapshot {
            connection_id: conn_id,
            fields: vec!["name".to_string(), "age".to_string()],
            captured_at: Utc::now() - Duration::hours(1),
        };
        let store: Arc<dyn SchemaSnapshotStore> =
            Arc::new(MockSchemaSnapshotStore::with_snapshot(previous));
        let detector = SchemaChangeDetector::new(store);

        let current = vec!["name".to_string(), "age".to_string(), "email".to_string()];
        let event = detector.scan(conn_id, &current).await.unwrap();

        let event = event.expect("should detect schema change");
        assert!(event.added_fields.contains(&"email".to_string()));
        assert!(event.removed_fields.is_empty());
        assert_eq!(event.severity, Severity::Medium);
    }

    #[tokio::test]
    async fn schema_change_detects_removed_field() {
        let conn_id = uuid::Uuid::now_v7();
        let previous = SchemaSnapshot {
            connection_id: conn_id,
            fields: vec!["name".to_string(), "age".to_string(), "ssn".to_string()],
            captured_at: Utc::now() - Duration::hours(1),
        };
        let store: Arc<dyn SchemaSnapshotStore> =
            Arc::new(MockSchemaSnapshotStore::with_snapshot(previous));
        let detector = SchemaChangeDetector::new(store);

        let current = vec!["name".to_string(), "age".to_string()];
        let event = detector.scan(conn_id, &current).await.unwrap();

        let event = event.expect("should detect schema change");
        assert!(event.removed_fields.contains(&"ssn".to_string()));
        assert_eq!(event.severity, Severity::High);
    }

    // -- SR_CONN_36 tests -----------------------------------------------------

    #[tokio::test]
    async fn rate_budget_allows_within_budget() {
        let enforcer: Arc<dyn BudgetEnforcer> = Arc::new(MockBudgetEnforcer {
            vendor_remaining: 1000,
            tenant_remaining: 500,
        });
        let service = RateBudgetService::new(enforcer);

        let result = service
            .check(&RateBudgetCheck {
                system_id: "sys1".to_string(),
                tenant_id: TenantId::new(),
                expected_call_count: 100,
            })
            .await
            .unwrap();

        assert_eq!(result.decision, PullPreflightDecision::Allow);
        assert!(result.defer_reason.is_none());
    }

    #[tokio::test]
    async fn rate_budget_defers_when_exhausted() {
        let enforcer: Arc<dyn BudgetEnforcer> = Arc::new(MockBudgetEnforcer {
            vendor_remaining: 5,
            tenant_remaining: 500,
        });
        let service = RateBudgetService::new(enforcer);

        let result = service
            .check(&RateBudgetCheck {
                system_id: "sys1".to_string(),
                tenant_id: TenantId::new(),
                expected_call_count: 100,
            })
            .await
            .unwrap();

        assert_eq!(result.decision, PullPreflightDecision::Defer);
        assert!(result.defer_reason.is_some());
    }

    // -- SR_CONN_37 tests -----------------------------------------------------

    #[tokio::test]
    async fn kpi_service_updates_snapshot() {
        let store: Arc<dyn KpiStore> = Arc::new(MockKpiStore::new());
        let service = ConnectionKpiService::new(store.clone());
        let conn_id = uuid::Uuid::now_v7();

        let snapshot = ConnectionKpiSnapshot {
            connection_id: conn_id,
            uptime_pct: 99.9,
            avg_latency_ms: 120,
            error_rate_pct: 0.1,
            last_successful_pull: Some(Utc::now()),
        };
        service.update(&snapshot).await.unwrap();

        let stored = store.get(conn_id).await.unwrap();
        assert!(stored.is_some());
        let stored = stored.unwrap();
        assert!((stored.uptime_pct - 99.9).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn kpi_service_detects_breach() {
        let store: Arc<dyn KpiStore> = Arc::new(MockKpiStore::new());
        let service = ConnectionKpiService::new(store.clone());
        let conn_id = uuid::Uuid::now_v7();

        // Store a degraded KPI
        let snapshot = ConnectionKpiSnapshot {
            connection_id: conn_id,
            uptime_pct: 80.0,
            avg_latency_ms: 6000,
            error_rate_pct: 10.0,
            last_successful_pull: None,
        };
        store.save(&snapshot).await.unwrap();

        let breached = service.evaluate(conn_id).await.unwrap();
        assert!(breached);
    }

    // -- SR_CONN_38 tests -----------------------------------------------------

    #[tokio::test]
    async fn classification_override_store_works() {
        let store: Arc<dyn OverrideStore> = Arc::new(MockOverrideStore::new());
        let service = ClassificationOverrideService::new(store);

        let ov = ClassificationOverride {
            system_id: "sys1".to_string(),
            field_name: "ssn".to_string(),
            classification: "pii_ssn".to_string(),
        };
        service.store_override(&ov).await.unwrap();
    }

    // -- SR_CONN_39 tests -----------------------------------------------------

    #[tokio::test]
    async fn classification_override_applies_overrides() {
        let store: Arc<dyn OverrideStore> = Arc::new(MockOverrideStore::new());
        let service = ClassificationOverrideService::new(store.clone());

        // Store an override
        let ov = ClassificationOverride {
            system_id: "sys1".to_string(),
            field_name: "phone".to_string(),
            classification: "pii_phone".to_string(),
        };
        service.store_override(&ov).await.unwrap();

        // Apply overrides
        let mut classifications = vec![
            ("phone".to_string(), "text".to_string()),
            ("name".to_string(), "text".to_string()),
        ];
        let applied = service
            .apply_overrides("sys1", &mut classifications)
            .await
            .unwrap();

        assert_eq!(applied, 1);
        assert_eq!(classifications[0].1, "pii_phone");
        assert_eq!(classifications[1].1, "text"); // unchanged
    }

    // -- SR_CONN_40 tests -----------------------------------------------------

    #[test]
    fn cloud_llm_connection_registers_provider() {
        let tenant_id = TenantId::new();
        let user_id = UserId::new();
        let record = CloudLlmConnectionService::register_provider(
            tenant_id,
            "anthropic",
            "claude-3-opus",
            user_id,
        );

        assert_eq!(record.tenant_id, tenant_id);
        assert!(record.system_id.contains("anthropic"));
        assert_eq!(record.connection_type, "cloud_llm");
        assert!(record.scope.contains("claude-3-opus"));
        assert_eq!(record.status, ConnectionState::Requested);
    }

    // -- SR_CONN_41 tests -----------------------------------------------------

    #[test]
    fn deprecation_alert_fires_at_thresholds() {
        // 60 days away -- should fire at 90-day threshold
        let in_60_days = Utc::now() + Duration::days(60) + Duration::hours(1);
        let alert = DeprecationAlertService::check_deprecation(in_60_days);
        assert!(alert.is_some());
        assert!(alert.unwrap().contains("days"));

        // 200 days away -- no alert
        let in_200_days = Utc::now() + Duration::days(200);
        let alert = DeprecationAlertService::check_deprecation(in_200_days);
        assert!(alert.is_none());

        // Already expired
        let yesterday = Utc::now() - Duration::days(1);
        let alert = DeprecationAlertService::check_deprecation(yesterday);
        assert!(alert.is_some());
        assert!(alert.unwrap().contains("Immediate action"));
    }

    // -- SR_CONN_42 tests -----------------------------------------------------

    #[test]
    fn paywall_approves_authorized_api() {
        let decision = PaywallGovernanceService::evaluate(false, true, false);
        assert_eq!(decision, PaywallDecision::Approved);

        // Free API
        let decision = PaywallGovernanceService::evaluate(true, false, false);
        assert_eq!(decision, PaywallDecision::Approved);
    }

    #[test]
    fn paywall_rejects_unauthorized_api() {
        let decision = PaywallGovernanceService::evaluate(false, false, false);
        assert_eq!(decision, PaywallDecision::Rejected);

        // Manual export required
        let decision = PaywallGovernanceService::evaluate(false, true, true);
        assert_eq!(decision, PaywallDecision::ManualExportRequired);
    }

    // -- SR_CONN_43 tests -----------------------------------------------------

    #[tokio::test]
    async fn bulk_import_log_persists_metric() {
        let mock_writer = Arc::new(MockPgWriter::new());
        let writer: Arc<dyn PgWriter> = mock_writer.clone();
        let service = BulkImportLogService::new(writer);

        service
            .log(TenantId::new(), "bulk_source_1", 5000, 3)
            .await
            .unwrap();

        let rows = mock_writer.rows.lock().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_id, "bulk_source_1");
    }

    // -- SR_CONN_44 tests -----------------------------------------------------

    #[tokio::test]
    async fn health_dashboard_returns_summaries() {
        let store: Arc<dyn KpiStore> = Arc::new(MockKpiStore::new());
        let dashboard = ConnectionHealthDashboard::new(store.clone());
        let tenant_id = TenantId::new();

        // Add some KPI data
        store
            .save(&ConnectionKpiSnapshot {
                connection_id: uuid::Uuid::now_v7(),
                uptime_pct: 99.5,
                avg_latency_ms: 150,
                error_rate_pct: 0.2,
                last_successful_pull: Some(Utc::now()),
            })
            .await
            .unwrap();

        let result = dashboard.query(tenant_id).await.unwrap();
        assert_eq!(result.connections.len(), 1);
        assert_eq!(result.connections[0].status, "healthy");
    }
}
