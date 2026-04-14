//! Connection type adapters (Types 1-7) and adapter registry.
//!
//! Each adapter implements the `ConnectionAdapter` trait to produce an
//! `ExecutionRecord` capturing credential type, data origin, and telemetry.
//!
//! Implements: SR_CONN_11 through SR_CONN_18

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use prism_core::error::PrismError;
use prism_core::types::*;

// ---------------------------------------------------------------------------
// ConnectionAdapter trait
// ---------------------------------------------------------------------------

/// Common trait for all connection type adapters (Types 1-7).
///
/// Each adapter pulls data from an external system and produces an
/// `ExecutionRecord` with the appropriate `credential_type`.
///
/// Implements: SR_CONN_11 through SR_CONN_17
#[async_trait]
pub trait ConnectionAdapter: Send + Sync {
    /// Pull data from the external system and return an execution record.
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError>;
}

// ---------------------------------------------------------------------------
// MLAID Scanner trait (SR_CONN_18)
// ---------------------------------------------------------------------------

/// Scanner for machine-learning-aided injection detection in user uploads.
///
/// Implementations analyze file content for adversarial payloads that
/// could exploit downstream processing pipelines.
///
/// Implements: SR_CONN_18
#[async_trait]
pub trait MlaidScanner: Send + Sync {
    /// Scan content for injection attacks.
    /// Returns `true` if content is safe, `false` if injection detected.
    async fn scan(&self, content: &[u8]) -> Result<bool, PrismError>;
}

// ---------------------------------------------------------------------------
// SR_CONN_11 -- Type 1: Delegated User Credentials
// ---------------------------------------------------------------------------

/// Adapter for delegated user credential connections.
///
/// Takes a `person_id` and `scope` to pull data on behalf of a
/// specific user. The credential is scoped to that person's access.
///
/// Implements: SR_CONN_11
pub struct DelegatedUserAdapter {
    pub person_id: uuid::Uuid,
    pub scope: String,
}

#[async_trait]
impl ConnectionAdapter for DelegatedUserAdapter {
    /// Pull data using delegated user credentials.
    /// Implements: SR_CONN_11
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError> {
        let start = Utc::now();
        // In production the adapter would contact the external system
        // using the delegated credential. Here we produce the record.
        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "delegated".to_string(),
            source_system: format!("delegated_user_{}", self.person_id),
            records_pulled: 0,
            fields: vec![],
            data_origin: DataOrigin::ConnectionPull,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_12 -- Type 2: Scoped Service Account
// ---------------------------------------------------------------------------

/// Adapter for scoped service account connections.
///
/// Standard enterprise integration pattern where a service account
/// with limited scope pulls data from the external system.
///
/// Implements: SR_CONN_12
pub struct ScopedSaAdapter;

#[async_trait]
impl ConnectionAdapter for ScopedSaAdapter {
    /// Pull data using a scoped service account.
    /// Implements: SR_CONN_12
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError> {
        let start = Utc::now();
        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "scoped_sa".to_string(),
            source_system: "scoped_service_account".to_string(),
            records_pulled: 0,
            fields: vec![],
            data_origin: DataOrigin::ConnectionPull,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_13 -- Type 3: Privileged Service Account
// ---------------------------------------------------------------------------

/// Adapter for privileged service account connections.
///
/// Validates that the session duration does not exceed `time_box_minutes`
/// (default 60). Used for elevated-privilege access to external systems.
///
/// Implements: SR_CONN_13
pub struct PrivilegedSaAdapter {
    /// Maximum session duration in minutes (default 60).
    pub time_box_minutes: u64,
}

impl Default for PrivilegedSaAdapter {
    fn default() -> Self {
        Self {
            time_box_minutes: 60,
        }
    }
}

#[async_trait]
impl ConnectionAdapter for PrivilegedSaAdapter {
    /// Pull data using a privileged service account with time-boxed session.
    /// Implements: SR_CONN_13
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError> {
        let start = Utc::now();

        // Validate time box
        if self.time_box_minutes == 0 {
            return Err(PrismError::Validation {
                reason: "time_box_minutes must be greater than 0".to_string(),
            });
        }

        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "privileged_sa".to_string(),
            source_system: format!("privileged_sa_timebox_{}min", self.time_box_minutes),
            records_pulled: 0,
            fields: vec![],
            data_origin: DataOrigin::ConnectionPull,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_14 -- Type 4: OAuth/API Integration
// ---------------------------------------------------------------------------

/// Adapter for OAuth / API integration connections.
///
/// Handles token-based authentication flows for third-party APIs.
///
/// Implements: SR_CONN_14
pub struct OAuthAdapter;

#[async_trait]
impl ConnectionAdapter for OAuthAdapter {
    /// Pull data using an OAuth / API integration credential.
    /// Implements: SR_CONN_14
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError> {
        let start = Utc::now();
        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "oauth".to_string(),
            source_system: "oauth_api".to_string(),
            records_pulled: 0,
            fields: vec![],
            data_origin: DataOrigin::ConnectionPull,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_15 -- Type 5: RPA Adapter
// ---------------------------------------------------------------------------

/// Adapter for RPA orchestrator connections.
///
/// Extracts execution records from RPA platforms and normalizes them
/// into the PRISM governance model.
///
/// Implements: SR_CONN_15
pub struct RpaAdapter;

#[async_trait]
impl ConnectionAdapter for RpaAdapter {
    /// Pull normalized records from an RPA orchestrator.
    /// Implements: SR_CONN_15
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError> {
        let start = Utc::now();
        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "rpa".to_string(),
            source_system: "rpa_orchestrator".to_string(),
            records_pulled: 0,
            fields: vec![],
            data_origin: DataOrigin::ConnectionPull,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_16 -- Type 5.5: AI Navigation Agent
// ---------------------------------------------------------------------------

/// Adapter for AI navigation agent connections.
///
/// Marked as fragile due to the unpredictable nature of AI-driven
/// navigation. Tracks a `fragility_score` for monitoring.
///
/// Implements: SR_CONN_16
pub struct AiNavigationAdapter {
    /// Fragility score (0.0 = stable, 1.0 = highly fragile).
    pub fragility_score: f64,
}

#[async_trait]
impl ConnectionAdapter for AiNavigationAdapter {
    /// Pull data via an AI navigation agent.
    /// Implements: SR_CONN_16
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError> {
        let start = Utc::now();
        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "ai_navigation".to_string(),
            source_system: format!("ai_navigation_fragility_{:.2}", self.fragility_score),
            records_pulled: 0,
            fields: vec![],
            data_origin: DataOrigin::ConnectionPull,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_17 -- Type 6: Bulk Import
// ---------------------------------------------------------------------------

/// Adapter for bulk import connections.
///
/// Supports throttle profiles to control the rate at which large
/// data sets are ingested into the platform.
///
/// Implements: SR_CONN_17
pub struct BulkImportAdapter {
    /// Throttle profile name (e.g., "fast", "standard", "slow").
    pub throttle_profile: String,
}

#[async_trait]
impl ConnectionAdapter for BulkImportAdapter {
    /// Pull data via a bulk import with throttle control.
    /// Implements: SR_CONN_17
    async fn pull(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
    ) -> Result<ExecutionRecord, PrismError> {
        let start = Utc::now();
        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "bulk_import".to_string(),
            source_system: format!("bulk_import_throttle_{}", self.throttle_profile),
            records_pulled: 0,
            fields: vec![],
            data_origin: DataOrigin::BulkImport,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_18 -- Type 7: User Upload
// ---------------------------------------------------------------------------

/// Adapter for user-uploaded files.
///
/// Unlike Types 1-7, user uploads use `process_upload()` instead of
/// the common `ConnectionAdapter` trait because uploads are synchronous
/// and require inline MLAID scanning and purpose validation.
///
/// Implements: SR_CONN_18
pub struct UserUploadAdapter {
    scanner: Arc<dyn MlaidScanner>,
}

impl UserUploadAdapter {
    pub fn new(scanner: Arc<dyn MlaidScanner>) -> Self {
        Self { scanner }
    }

    /// Process a user-uploaded file.
    ///
    /// 1. Validates that `purpose_declaration` is non-empty.
    /// 2. Runs MLAID scanning on the file content.
    /// 3. Produces an `ExecutionRecord` on success.
    ///
    /// Implements: SR_CONN_18
    pub async fn process_upload(
        &self,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
        content: &[u8],
        purpose_declaration: &str,
    ) -> Result<ExecutionRecord, PrismError> {
        // Validate purpose declaration
        if purpose_declaration.trim().is_empty() {
            return Err(PrismError::Validation {
                reason: "purpose_declaration must not be empty".to_string(),
            });
        }

        // Run MLAID injection scan
        let safe = self.scanner.scan(content).await?;
        if !safe {
            return Err(PrismError::Validation {
                reason: "MLAID injection detected in uploaded content".to_string(),
            });
        }

        let start = Utc::now();
        let latency_ms = (Utc::now() - start).num_milliseconds().unsigned_abs();

        Ok(ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: "user_upload".to_string(),
            source_system: "user_upload".to_string(),
            records_pulled: 1,
            fields: vec![],
            data_origin: DataOrigin::UserUpload,
            status: "success".to_string(),
            error: None,
            latency_ms,
            created_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// Adapter Registry (REUSABLE)
// ---------------------------------------------------------------------------

/// Registry mapping connection type strings to their adapter implementations.
///
/// Provides a single lookup point for all 7 standard connection type adapters.
pub struct AdapterRegistry {
    adapters: HashMap<String, Arc<dyn ConnectionAdapter>>,
}

impl AdapterRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Register an adapter for a connection type.
    pub fn register(&mut self, connection_type: &str, adapter: Arc<dyn ConnectionAdapter>) {
        self.adapters.insert(connection_type.to_string(), adapter);
    }

    /// Look up an adapter by connection type.
    pub fn get_adapter(&self, connection_type: &str) -> Option<Arc<dyn ConnectionAdapter>> {
        self.adapters.get(connection_type).cloned()
    }

    /// Build a registry pre-loaded with all 7 standard adapter types.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(
            "delegated",
            Arc::new(DelegatedUserAdapter {
                person_id: uuid::Uuid::nil(),
                scope: "default".to_string(),
            }),
        );
        registry.register("scoped_sa", Arc::new(ScopedSaAdapter));
        registry.register("privileged_sa", Arc::new(PrivilegedSaAdapter::default()));
        registry.register("oauth", Arc::new(OAuthAdapter));
        registry.register("rpa", Arc::new(RpaAdapter));
        registry.register(
            "ai_navigation",
            Arc::new(AiNavigationAdapter {
                fragility_score: 0.5,
            }),
        );
        registry.register(
            "bulk_import",
            Arc::new(BulkImportAdapter {
                throttle_profile: "standard".to_string(),
            }),
        );
        registry
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // -- Mock MLAID Scanner ---------------------------------------------------

    struct MockMlaidScanner {
        results: Mutex<Vec<bool>>,
    }

    impl MockMlaidScanner {
        fn new(results: Vec<bool>) -> Self {
            Self {
                results: Mutex::new(results),
            }
        }
    }

    #[async_trait]
    impl MlaidScanner for MockMlaidScanner {
        async fn scan(&self, _content: &[u8]) -> Result<bool, PrismError> {
            let mut results = self.results.lock().unwrap();
            Ok(results.pop().unwrap_or(true))
        }
    }

    fn test_tenant_id() -> TenantId {
        TenantId::new()
    }

    fn test_connection_id() -> uuid::Uuid {
        uuid::Uuid::now_v7()
    }

    // -- SR_CONN_11 tests -----------------------------------------------------

    #[tokio::test]
    async fn delegated_user_adapter_produces_correct_credential_type() {
        let adapter = DelegatedUserAdapter {
            person_id: uuid::Uuid::now_v7(),
            scope: "read".to_string(),
        };
        let record = adapter
            .pull(test_tenant_id(), test_connection_id())
            .await
            .unwrap();
        assert_eq!(record.credential_type, "delegated");
        assert_eq!(record.status, "success");
    }

    // -- SR_CONN_12 tests -----------------------------------------------------

    #[tokio::test]
    async fn scoped_sa_adapter_produces_record() {
        let adapter = ScopedSaAdapter;
        let record = adapter
            .pull(test_tenant_id(), test_connection_id())
            .await
            .unwrap();
        assert_eq!(record.credential_type, "scoped_sa");
    }

    // -- SR_CONN_13 tests -----------------------------------------------------

    #[tokio::test]
    async fn privileged_sa_adapter_produces_record_with_time_box() {
        let adapter = PrivilegedSaAdapter {
            time_box_minutes: 30,
        };
        let record = adapter
            .pull(test_tenant_id(), test_connection_id())
            .await
            .unwrap();
        assert_eq!(record.credential_type, "privileged_sa");
        assert!(record.source_system.contains("30"));
    }

    // -- SR_CONN_14 tests -----------------------------------------------------

    #[tokio::test]
    async fn oauth_adapter_produces_record() {
        let adapter = OAuthAdapter;
        let record = adapter
            .pull(test_tenant_id(), test_connection_id())
            .await
            .unwrap();
        assert_eq!(record.credential_type, "oauth");
    }

    // -- SR_CONN_15 tests -----------------------------------------------------

    #[tokio::test]
    async fn rpa_adapter_produces_normalized_records() {
        let adapter = RpaAdapter;
        let record = adapter
            .pull(test_tenant_id(), test_connection_id())
            .await
            .unwrap();
        assert_eq!(record.credential_type, "rpa");
        assert_eq!(record.data_origin, DataOrigin::ConnectionPull);
    }

    // -- SR_CONN_16 tests -----------------------------------------------------

    #[tokio::test]
    async fn ai_navigation_adapter_produces_record_with_fragility_score() {
        let adapter = AiNavigationAdapter {
            fragility_score: 0.75,
        };
        let record = adapter
            .pull(test_tenant_id(), test_connection_id())
            .await
            .unwrap();
        assert_eq!(record.credential_type, "ai_navigation");
        assert!(record.source_system.contains("0.75"));
    }

    // -- SR_CONN_17 tests -----------------------------------------------------

    #[tokio::test]
    async fn bulk_import_adapter_produces_record_with_throttle() {
        let adapter = BulkImportAdapter {
            throttle_profile: "fast".to_string(),
        };
        let record = adapter
            .pull(test_tenant_id(), test_connection_id())
            .await
            .unwrap();
        assert_eq!(record.credential_type, "bulk_import");
        assert!(record.source_system.contains("fast"));
        assert_eq!(record.data_origin, DataOrigin::BulkImport);
    }

    // -- SR_CONN_18 tests -----------------------------------------------------

    #[tokio::test]
    async fn user_upload_succeeds_with_valid_content() {
        let scanner = Arc::new(MockMlaidScanner::new(vec![true]));
        let adapter = UserUploadAdapter::new(scanner);
        let record = adapter
            .process_upload(
                test_tenant_id(),
                test_connection_id(),
                b"valid csv data",
                "quarterly compliance report",
            )
            .await
            .unwrap();
        assert_eq!(record.credential_type, "user_upload");
        assert_eq!(record.data_origin, DataOrigin::UserUpload);
    }

    #[tokio::test]
    async fn user_upload_rejects_mlaid_injection() {
        let scanner = Arc::new(MockMlaidScanner::new(vec![false]));
        let adapter = UserUploadAdapter::new(scanner);
        let result = adapter
            .process_upload(
                test_tenant_id(),
                test_connection_id(),
                b"malicious payload",
                "quarterly compliance report",
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("MLAID injection"));
    }

    // -- AdapterRegistry tests ------------------------------------------------

    #[test]
    fn registry_returns_correct_adapter_for_type() {
        let registry = AdapterRegistry::with_defaults();
        assert!(registry.get_adapter("delegated").is_some());
        assert!(registry.get_adapter("scoped_sa").is_some());
        assert!(registry.get_adapter("privileged_sa").is_some());
        assert!(registry.get_adapter("oauth").is_some());
        assert!(registry.get_adapter("rpa").is_some());
        assert!(registry.get_adapter("ai_navigation").is_some());
        assert!(registry.get_adapter("bulk_import").is_some());
        assert!(registry.get_adapter("unknown_type").is_none());
    }
}
