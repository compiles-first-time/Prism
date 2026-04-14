//! Connection lifecycle state machine for external system connections.
//!
//! Implements SR_CONN_01 through SR_CONN_10: the full lifecycle from
//! request through decommission, including credential provisioning,
//! connectivity testing, KPI-based degradation, and recovery.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::ConnectionRecordRepository;
use prism_core::types::*;

// ---------------------------------------------------------------------------
// Traits for external dependencies
// ---------------------------------------------------------------------------

/// Client for the Credential-as-a-Service (CaaS) vault.
///
/// Abstracts credential storage so that the lifecycle service never
/// handles raw secrets beyond the initial handoff.
#[async_trait]
pub trait CaaSClient: Send + Sync {
    /// Store a credential in the vault and return an opaque reference.
    /// Implements: SR_CONN_04
    async fn store_credential(
        &self,
        connection_id: uuid::Uuid,
        raw_credential: &str,
    ) -> Result<String, PrismError>;

    /// Revoke a previously stored credential.
    /// Implements: SR_CONN_09
    async fn revoke_credential(&self, credential_ref: &str) -> Result<(), PrismError>;
}

/// Tester for external system connectivity.
///
/// Implementations probe the target system to verify the credential
/// works and the endpoint is reachable.
#[async_trait]
pub trait ExternalSystemTester: Send + Sync {
    /// Test connectivity to the external system.
    /// Implements: SR_CONN_05
    async fn test_connection(
        &self,
        connection_id: uuid::Uuid,
        credential_ref: &str,
    ) -> Result<ConnectionTestResult, PrismError>;
}

// ---------------------------------------------------------------------------
// State transition validation
// ---------------------------------------------------------------------------

/// Validate that a connection state transition is legal.
///
/// Only the transitions defined in the connection lifecycle state machine
/// are permitted. Returns an error if the transition is invalid.
///
/// Implements: SR_CONN_01 through SR_CONN_10
pub fn validate_transition(from: ConnectionState, to: ConnectionState) -> Result<(), PrismError> {
    let valid = matches!(
        (from, to),
        (ConnectionState::Requested, ConnectionState::Configuring)
            | (ConnectionState::Configuring, ConnectionState::Testing)
            | (ConnectionState::Testing, ConnectionState::Active)
            | (ConnectionState::Testing, ConnectionState::Failed)
            | (ConnectionState::Active, ConnectionState::Degraded)
            | (ConnectionState::Active, ConnectionState::Suspended)
            | (ConnectionState::Active, ConnectionState::Decommissioned)
            | (ConnectionState::Degraded, ConnectionState::Active)
            | (ConnectionState::Degraded, ConnectionState::Failed)
            | (ConnectionState::Degraded, ConnectionState::Suspended)
            | (ConnectionState::Suspended, ConnectionState::Active)
            | (ConnectionState::Suspended, ConnectionState::Decommissioned)
            | (ConnectionState::Failed, ConnectionState::Active)
            | (ConnectionState::Failed, ConnectionState::Decommissioned)
    );

    if valid {
        Ok(())
    } else {
        Err(PrismError::Validation {
            reason: format!(
                "illegal connection state transition: {:?} -> {:?}",
                from, to
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Service orchestrating the full connection lifecycle state machine.
///
/// Composes a `ConnectionRecordRepository`, `CaaSClient`,
/// `ExternalSystemTester`, and `AuditLogger` following the established
/// repository + service + audit pattern.
///
/// Implements: SR_CONN_01 through SR_CONN_10
pub struct ConnectionLifecycleService {
    repo: Arc<dyn ConnectionRecordRepository>,
    caas: Arc<dyn CaaSClient>,
    tester: Arc<dyn ExternalSystemTester>,
    audit: AuditLogger,
}

impl ConnectionLifecycleService {
    pub fn new(
        repo: Arc<dyn ConnectionRecordRepository>,
        caas: Arc<dyn CaaSClient>,
        tester: Arc<dyn ExternalSystemTester>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            repo,
            caas,
            tester,
            audit,
        }
    }

    // -- helpers ------------------------------------------------------------

    async fn get_connection(&self, id: uuid::Uuid) -> Result<ConnectionRecord, PrismError> {
        self.repo.get_by_id(id).await?.ok_or(PrismError::NotFound {
            entity_type: "ConnectionRecord",
            id,
        })
    }

    async fn transition(
        &self,
        conn: &ConnectionRecord,
        to: ConnectionState,
    ) -> Result<(), PrismError> {
        validate_transition(conn.status, to)?;
        self.repo.update_status(conn.id, to).await
    }

    fn audit_input(
        &self,
        tenant_id: TenantId,
        event_type: &str,
        target_id: uuid::Uuid,
        payload: serde_json::Value,
    ) -> AuditEventInput {
        AuditEventInput {
            tenant_id,
            event_type: event_type.to_string(),
            actor_id: uuid::Uuid::nil(),
            actor_type: ActorType::System,
            target_id: Some(target_id),
            target_type: Some("ConnectionRecord".to_string()),
            severity: Severity::Medium,
            source_layer: SourceLayer::Connection,
            governance_authority: None,
            payload,
        }
    }

    // -- SR_CONN_01: request_connection -------------------------------------

    /// Create a new external system connection in REQUESTED state.
    ///
    /// Validates that `system_id` and `connection_type` are non-empty.
    ///
    /// Implements: SR_CONN_01
    pub async fn request_connection(
        &self,
        input: ConnectionRequestInput,
    ) -> Result<ConnectionRequestResult, PrismError> {
        // Validation
        if input.system_id.trim().is_empty() {
            return Err(PrismError::Validation {
                reason: "system_id must not be empty".to_string(),
            });
        }
        if input.connection_type.trim().is_empty() {
            return Err(PrismError::Validation {
                reason: "connection_type must not be empty".to_string(),
            });
        }

        let now = Utc::now();
        let id = uuid::Uuid::now_v7();

        let record = ConnectionRecord {
            id,
            tenant_id: input.tenant_id,
            system_id: input.system_id.clone(),
            connection_type: input.connection_type.clone(),
            scope: input.scope.clone(),
            status: ConnectionState::Requested,
            credential_ref: None,
            justification: input.justification.clone(),
            requested_by: input.requested_by,
            first_pull_at: None,
            kpi_error_rate: None,
            kpi_avg_latency_ms: None,
            created_at: now,
            updated_at: now,
        };

        self.repo.create(&record).await?;

        self.audit
            .log(self.audit_input(
                input.tenant_id,
                "connection.requested",
                id,
                serde_json::json!({
                    "system_id": input.system_id,
                    "connection_type": input.connection_type,
                    "scope": input.scope,
                }),
            ))
            .await?;

        info!(connection_id = %id, "connection requested");

        Ok(ConnectionRequestResult {
            connection_id: id,
            state: ConnectionState::Requested,
        })
    }

    // -- SR_CONN_02: approve_connection -------------------------------------

    /// Approve a connection request, moving it from REQUESTED to CONFIGURING.
    ///
    /// The approval gate itself is delegated to the governance rule engine
    /// (SR_GOV_70); this method performs the state transition.
    ///
    /// Implements: SR_CONN_02
    pub async fn approve_connection(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<ConnectionRequestResult, PrismError> {
        let conn = self.get_connection(connection_id).await?;
        self.transition(&conn, ConnectionState::Configuring).await?;

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.approved",
                connection_id,
                serde_json::json!({"from": "requested", "to": "configuring"}),
            ))
            .await?;

        info!(connection_id = %connection_id, "connection approved");

        Ok(ConnectionRequestResult {
            connection_id,
            state: ConnectionState::Configuring,
        })
    }

    // -- SR_CONN_03: capture_consent ----------------------------------------

    /// Record that tenant consent has been captured for this connection.
    ///
    /// The connection stays in CONFIGURING state until credential provisioning
    /// moves it to TESTING. This method records the consent event for audit.
    ///
    /// Implements: SR_CONN_03
    pub async fn capture_consent(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<ConnectionRequestResult, PrismError> {
        let conn = self.get_connection(connection_id).await?;

        if conn.status != ConnectionState::Configuring {
            return Err(PrismError::Validation {
                reason: format!(
                    "consent can only be captured in Configuring state, current: {:?}",
                    conn.status
                ),
            });
        }

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.consent_captured",
                connection_id,
                serde_json::json!({"system_id": conn.system_id}),
            ))
            .await?;

        info!(connection_id = %connection_id, "consent captured");

        Ok(ConnectionRequestResult {
            connection_id,
            state: ConnectionState::Configuring,
        })
    }

    // -- SR_CONN_04: provision_credential -----------------------------------

    /// Provision a credential for the connection via the CaaS vault.
    ///
    /// Stores the raw credential in CaaS, saves the opaque reference,
    /// and transitions the connection from CONFIGURING to TESTING.
    ///
    /// Implements: SR_CONN_04
    pub async fn provision_credential(
        &self,
        input: CredentialProvisionInput,
    ) -> Result<CredentialProvisionResult, PrismError> {
        let conn = self.get_connection(input.connection_id).await?;

        // Store credential in CaaS
        let credential_ref = self
            .caas
            .store_credential(input.connection_id, &input.raw_credential)
            .await?;

        // Persist the credential reference
        self.repo
            .update_credential(input.connection_id, Some(credential_ref.clone()))
            .await?;

        // Transition to TESTING
        self.transition(&conn, ConnectionState::Testing).await?;

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.credential_provisioned",
                input.connection_id,
                serde_json::json!({"credential_ref": credential_ref}),
            ))
            .await?;

        info!(connection_id = %input.connection_id, "credential provisioned");

        Ok(CredentialProvisionResult { credential_ref })
    }

    // -- SR_CONN_05: test_connection ----------------------------------------

    /// Test connectivity to the external system.
    ///
    /// On success, moves the connection to ACTIVE. On failure, moves to
    /// FAILED. Records the latency in KPIs.
    ///
    /// Implements: SR_CONN_05
    pub async fn test_connection(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<ConnectionTestResult, PrismError> {
        let conn = self.get_connection(connection_id).await?;

        let credential_ref = conn
            .credential_ref
            .as_deref()
            .ok_or(PrismError::Validation {
                reason: "no credential provisioned for this connection".to_string(),
            })?;

        let result = self
            .tester
            .test_connection(connection_id, credential_ref)
            .await?;

        // Record latency KPI
        self.repo
            .update_kpis(connection_id, None, Some(result.latency_ms))
            .await?;

        if result.passed {
            self.transition(&conn, ConnectionState::Active).await?;
        } else {
            self.transition(&conn, ConnectionState::Failed).await?;
        }

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.tested",
                connection_id,
                serde_json::json!({
                    "passed": result.passed,
                    "latency_ms": result.latency_ms,
                    "error": result.error,
                }),
            ))
            .await?;

        info!(
            connection_id = %connection_id,
            passed = result.passed,
            "connection tested"
        );

        Ok(result)
    }

    // -- SR_CONN_06: activate -----------------------------------------------

    /// Activate a connection after a successful test, scheduling the first pull.
    ///
    /// Implements: SR_CONN_06
    pub async fn activate(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<ConnectionActivationResult, PrismError> {
        let conn = self.get_connection(connection_id).await?;
        self.transition(&conn, ConnectionState::Active).await?;

        let first_pull_at = Some(Utc::now());

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.activated",
                connection_id,
                serde_json::json!({"first_pull_at": first_pull_at}),
            ))
            .await?;

        info!(connection_id = %connection_id, "connection activated");

        Ok(ConnectionActivationResult {
            active: true,
            first_pull_at,
        })
    }

    // -- SR_CONN_07: mark_degraded ------------------------------------------

    /// Mark a connection as degraded when KPIs breach thresholds.
    ///
    /// Thresholds: error_rate > 10% OR latency > 5x baseline.
    /// The caller provides evidence of the breach.
    ///
    /// Implements: SR_CONN_07
    pub async fn mark_degraded(
        &self,
        input: DegradationInput,
    ) -> Result<ConnectionRequestResult, PrismError> {
        let conn = self.get_connection(input.connection_id).await?;
        self.transition(&conn, ConnectionState::Degraded).await?;

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.degraded",
                input.connection_id,
                serde_json::json!({
                    "reason": input.reason,
                    "evidence": input.evidence,
                }),
            ))
            .await?;

        info!(connection_id = %input.connection_id, "connection degraded");

        Ok(ConnectionRequestResult {
            connection_id: input.connection_id,
            state: ConnectionState::Degraded,
        })
    }

    // -- SR_CONN_08: suspend ------------------------------------------------

    /// Suspend a connection by explicit admin action.
    ///
    /// Implements: SR_CONN_08
    pub async fn suspend(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<ConnectionRequestResult, PrismError> {
        let conn = self.get_connection(connection_id).await?;

        if conn.status == ConnectionState::Suspended {
            return Err(PrismError::Validation {
                reason: "connection is already suspended".to_string(),
            });
        }

        self.transition(&conn, ConnectionState::Suspended).await?;

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.suspended",
                connection_id,
                serde_json::json!({"from": format!("{:?}", conn.status)}),
            ))
            .await?;

        info!(connection_id = %connection_id, "connection suspended");

        Ok(ConnectionRequestResult {
            connection_id,
            state: ConnectionState::Suspended,
        })
    }

    // -- SR_CONN_09: decommission -------------------------------------------

    /// Decommission a connection, revoking its credential in CaaS.
    ///
    /// Implements: SR_CONN_09
    pub async fn decommission(
        &self,
        input: DecommissionInput,
    ) -> Result<ConnectionRequestResult, PrismError> {
        let conn = self.get_connection(input.connection_id).await?;

        // Revoke credential if present
        if let Some(ref cred_ref) = conn.credential_ref {
            self.caas.revoke_credential(cred_ref).await?;
            self.repo
                .update_credential(input.connection_id, None)
                .await?;
        }

        self.transition(&conn, ConnectionState::Decommissioned)
            .await?;

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.decommissioned",
                input.connection_id,
                serde_json::json!({
                    "reason": input.reason,
                    "retain_data": input.retain_data,
                }),
            ))
            .await?;

        info!(connection_id = %input.connection_id, "connection decommissioned");

        Ok(ConnectionRequestResult {
            connection_id: input.connection_id,
            state: ConnectionState::Decommissioned,
        })
    }

    // -- SR_CONN_10: recover ------------------------------------------------

    /// Attempt to recover a FAILED connection by re-testing connectivity.
    ///
    /// On success, moves to ACTIVE. On failure, stays in FAILED.
    ///
    /// Implements: SR_CONN_10
    pub async fn recover(
        &self,
        connection_id: uuid::Uuid,
    ) -> Result<ConnectionTestResult, PrismError> {
        let conn = self.get_connection(connection_id).await?;

        if conn.status != ConnectionState::Failed {
            return Err(PrismError::Validation {
                reason: format!(
                    "recovery only applies to Failed connections, current: {:?}",
                    conn.status
                ),
            });
        }

        let credential_ref = conn
            .credential_ref
            .as_deref()
            .ok_or(PrismError::Validation {
                reason: "no credential provisioned for this connection".to_string(),
            })?;

        let result = self
            .tester
            .test_connection(connection_id, credential_ref)
            .await?;

        if result.passed {
            // Failed -> Active is a legal transition
            self.repo
                .update_status(connection_id, ConnectionState::Active)
                .await?;
        }
        // If not passed, stays Failed (no state change needed)

        self.audit
            .log(self.audit_input(
                conn.tenant_id,
                "connection.recovery_attempted",
                connection_id,
                serde_json::json!({
                    "passed": result.passed,
                    "latency_ms": result.latency_ms,
                    "error": result.error,
                }),
            ))
            .await?;

        info!(
            connection_id = %connection_id,
            passed = result.passed,
            "connection recovery attempted"
        );

        Ok(result)
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

    // -- Mock ConnectionRecordRepository ------------------------------------

    struct MockConnectionRepo {
        records: Mutex<Vec<ConnectionRecord>>,
    }

    impl MockConnectionRepo {
        fn new() -> Self {
            Self {
                records: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ConnectionRecordRepository for MockConnectionRepo {
        async fn create(&self, record: &ConnectionRecord) -> Result<(), PrismError> {
            self.records.lock().unwrap().push(record.clone());
            Ok(())
        }

        async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<ConnectionRecord>, PrismError> {
            Ok(self
                .records
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.id == id)
                .cloned())
        }

        async fn update_status(
            &self,
            id: uuid::Uuid,
            status: ConnectionState,
        ) -> Result<(), PrismError> {
            let mut records = self.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                r.status = status;
                r.updated_at = Utc::now();
                Ok(())
            } else {
                Err(PrismError::NotFound {
                    entity_type: "ConnectionRecord",
                    id,
                })
            }
        }

        async fn update_credential(
            &self,
            id: uuid::Uuid,
            credential_ref: Option<String>,
        ) -> Result<(), PrismError> {
            let mut records = self.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                r.credential_ref = credential_ref;
                r.updated_at = Utc::now();
                Ok(())
            } else {
                Err(PrismError::NotFound {
                    entity_type: "ConnectionRecord",
                    id,
                })
            }
        }

        async fn update_kpis(
            &self,
            id: uuid::Uuid,
            error_rate: Option<f64>,
            avg_latency_ms: Option<u64>,
        ) -> Result<(), PrismError> {
            let mut records = self.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                if let Some(er) = error_rate {
                    r.kpi_error_rate = Some(er);
                }
                if let Some(lat) = avg_latency_ms {
                    r.kpi_avg_latency_ms = Some(lat);
                }
                r.updated_at = Utc::now();
                Ok(())
            } else {
                Err(PrismError::NotFound {
                    entity_type: "ConnectionRecord",
                    id,
                })
            }
        }
    }

    // -- Mock AuditEventRepository ------------------------------------------

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
                events: vec![],
                next_page_token: None,
                total_count: 0,
            })
        }

        async fn get_chain_segment(
            &self,
            _tenant_id: TenantId,
            _depth: u32,
        ) -> Result<Vec<AuditEvent>, PrismError> {
            Ok(vec![])
        }
    }

    // -- Mock CaaSClient ----------------------------------------------------

    struct MockCaaS {
        stored: Mutex<Vec<(uuid::Uuid, String)>>,
        revoked: Mutex<Vec<String>>,
        should_fail: bool,
    }

    impl MockCaaS {
        fn new() -> Self {
            Self {
                stored: Mutex::new(Vec::new()),
                revoked: Mutex::new(Vec::new()),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                stored: Mutex::new(Vec::new()),
                revoked: Mutex::new(Vec::new()),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl CaaSClient for MockCaaS {
        async fn store_credential(
            &self,
            connection_id: uuid::Uuid,
            raw_credential: &str,
        ) -> Result<String, PrismError> {
            if self.should_fail {
                return Err(PrismError::Vault {
                    reason: "CaaS unavailable".to_string(),
                });
            }
            let cred_ref = format!("vault://conn/{}", connection_id);
            self.stored
                .lock()
                .unwrap()
                .push((connection_id, raw_credential.to_string()));
            Ok(cred_ref)
        }

        async fn revoke_credential(&self, credential_ref: &str) -> Result<(), PrismError> {
            self.revoked
                .lock()
                .unwrap()
                .push(credential_ref.to_string());
            Ok(())
        }
    }

    // -- Mock ExternalSystemTester ------------------------------------------

    struct MockTester {
        pass: bool,
        latency_ms: u64,
    }

    impl MockTester {
        fn passing() -> Self {
            Self {
                pass: true,
                latency_ms: 42,
            }
        }

        fn failing() -> Self {
            Self {
                pass: false,
                latency_ms: 5000,
            }
        }
    }

    #[async_trait]
    impl ExternalSystemTester for MockTester {
        async fn test_connection(
            &self,
            _connection_id: uuid::Uuid,
            _credential_ref: &str,
        ) -> Result<ConnectionTestResult, PrismError> {
            Ok(ConnectionTestResult {
                passed: self.pass,
                latency_ms: self.latency_ms,
                error: if self.pass {
                    None
                } else {
                    Some("connection refused".to_string())
                },
            })
        }
    }

    // -- Test helpers -------------------------------------------------------

    fn make_service(
        conn_repo: Arc<MockConnectionRepo>,
        caas: Arc<dyn CaaSClient>,
        tester: Arc<dyn ExternalSystemTester>,
    ) -> ConnectionLifecycleService {
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        ConnectionLifecycleService::new(conn_repo, caas, tester, audit)
    }

    fn default_input() -> ConnectionRequestInput {
        ConnectionRequestInput {
            tenant_id: TenantId::new(),
            system_id: "salesforce".to_string(),
            connection_type: "oauth2".to_string(),
            scope: "read:contacts".to_string(),
            justification: Some("quarterly sync".to_string()),
            requested_by: UserId::new(),
        }
    }

    /// Create a connection and advance it to the given state.
    async fn setup_connection(
        svc: &ConnectionLifecycleService,
        repo: &MockConnectionRepo,
        state: ConnectionState,
    ) -> uuid::Uuid {
        let result = svc.request_connection(default_input()).await.unwrap();
        let id = result.connection_id;

        if state == ConnectionState::Requested {
            return id;
        }

        svc.approve_connection(id).await.unwrap();
        if state == ConnectionState::Configuring {
            return id;
        }

        svc.provision_credential(CredentialProvisionInput {
            connection_id: id,
            raw_credential: "secret-key-123".to_string(),
        })
        .await
        .unwrap();
        if state == ConnectionState::Testing {
            return id;
        }

        // For Active and beyond, we need the test to pass, but test_connection
        // drives the tester mock, which may or may not pass. For states past
        // TESTING we directly set the status.
        {
            let mut records = repo.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                r.status = state;
            }
        }

        id
    }

    // -----------------------------------------------------------------------
    // SR_CONN_01 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_01_request_connection_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let result = svc.request_connection(default_input()).await.unwrap();

        assert_eq!(result.state, ConnectionState::Requested);
        let records = repo.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].system_id, "salesforce");
    }

    #[tokio::test]
    async fn sr_conn_01_request_rejects_empty_system_id() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo,
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let mut input = default_input();
        input.system_id = "  ".to_string();

        let err = svc.request_connection(input).await.unwrap_err();
        assert!(err.to_string().contains("system_id must not be empty"));
    }

    // -----------------------------------------------------------------------
    // SR_CONN_02 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_02_approve_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let req = svc.request_connection(default_input()).await.unwrap();
        let result = svc.approve_connection(req.connection_id).await.unwrap();

        assert_eq!(result.state, ConnectionState::Configuring);
    }

    #[tokio::test]
    async fn sr_conn_02_approve_rejects_invalid_state() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let req = svc.request_connection(default_input()).await.unwrap();
        // Approve once to move to Configuring
        svc.approve_connection(req.connection_id).await.unwrap();
        // Approve again should fail (Configuring -> Configuring is not valid)
        let err = svc.approve_connection(req.connection_id).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("illegal connection state transition"));
    }

    // -----------------------------------------------------------------------
    // SR_CONN_03 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_03_capture_consent_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let req = svc.request_connection(default_input()).await.unwrap();
        svc.approve_connection(req.connection_id).await.unwrap();
        let result = svc.capture_consent(req.connection_id).await.unwrap();

        assert_eq!(result.state, ConnectionState::Configuring);
    }

    #[tokio::test]
    async fn sr_conn_03_capture_consent_rejects_wrong_state() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let req = svc.request_connection(default_input()).await.unwrap();
        // Still in Requested state -- consent should be rejected
        let err = svc.capture_consent(req.connection_id).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("consent can only be captured in Configuring state"));
    }

    // -----------------------------------------------------------------------
    // SR_CONN_04 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_04_provision_credential_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let caas = Arc::new(MockCaaS::new());
        let svc = make_service(repo.clone(), caas.clone(), Arc::new(MockTester::passing()));

        let req = svc.request_connection(default_input()).await.unwrap();
        svc.approve_connection(req.connection_id).await.unwrap();

        let result = svc
            .provision_credential(CredentialProvisionInput {
                connection_id: req.connection_id,
                raw_credential: "my-secret".to_string(),
            })
            .await
            .unwrap();

        assert!(result.credential_ref.starts_with("vault://"));
        let stored = caas.stored.lock().unwrap();
        assert_eq!(stored.len(), 1);
    }

    #[tokio::test]
    async fn sr_conn_04_provision_credential_caas_failure() {
        let repo = Arc::new(MockConnectionRepo::new());
        let caas = Arc::new(MockCaaS::failing());
        let svc = make_service(repo.clone(), caas, Arc::new(MockTester::passing()));

        let req = svc.request_connection(default_input()).await.unwrap();
        svc.approve_connection(req.connection_id).await.unwrap();

        let err = svc
            .provision_credential(CredentialProvisionInput {
                connection_id: req.connection_id,
                raw_credential: "my-secret".to_string(),
            })
            .await
            .unwrap_err();

        assert!(err.to_string().contains("CaaS unavailable"));
    }

    // -----------------------------------------------------------------------
    // SR_CONN_05 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_05_test_connection_passes() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let req = svc.request_connection(default_input()).await.unwrap();
        svc.approve_connection(req.connection_id).await.unwrap();
        svc.provision_credential(CredentialProvisionInput {
            connection_id: req.connection_id,
            raw_credential: "secret".to_string(),
        })
        .await
        .unwrap();

        let result = svc.test_connection(req.connection_id).await.unwrap();

        assert!(result.passed);
        assert_eq!(result.latency_ms, 42);
        // Connection should now be Active
        let conn = repo.records.lock().unwrap();
        let c = conn.iter().find(|r| r.id == req.connection_id).unwrap();
        assert_eq!(c.status, ConnectionState::Active);
    }

    #[tokio::test]
    async fn sr_conn_05_test_connection_fails() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::failing()),
        );

        let req = svc.request_connection(default_input()).await.unwrap();
        svc.approve_connection(req.connection_id).await.unwrap();
        svc.provision_credential(CredentialProvisionInput {
            connection_id: req.connection_id,
            raw_credential: "secret".to_string(),
        })
        .await
        .unwrap();

        let result = svc.test_connection(req.connection_id).await.unwrap();

        assert!(!result.passed);
        assert!(result.error.is_some());
        // Connection should now be Failed
        let conn = repo.records.lock().unwrap();
        let c = conn.iter().find(|r| r.id == req.connection_id).unwrap();
        assert_eq!(c.status, ConnectionState::Failed);
    }

    // -----------------------------------------------------------------------
    // SR_CONN_06 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_06_activate_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Testing).await;

        let result = svc.activate(id).await.unwrap();

        assert!(result.active);
        assert!(result.first_pull_at.is_some());
    }

    #[tokio::test]
    async fn sr_conn_06_activate_rejects_wrong_state() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Requested).await;

        let err = svc.activate(id).await.unwrap_err();
        assert!(err
            .to_string()
            .contains("illegal connection state transition"));
    }

    // -----------------------------------------------------------------------
    // SR_CONN_07 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_07_mark_degraded_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Active).await;

        let result = svc
            .mark_degraded(DegradationInput {
                connection_id: id,
                reason: "error rate 15%".to_string(),
                evidence: serde_json::json!({"error_rate": 0.15}),
            })
            .await
            .unwrap();

        assert_eq!(result.state, ConnectionState::Degraded);
    }

    #[tokio::test]
    async fn sr_conn_07_mark_degraded_rejects_wrong_state() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Requested).await;

        let err = svc
            .mark_degraded(DegradationInput {
                connection_id: id,
                reason: "test".to_string(),
                evidence: serde_json::json!({}),
            })
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("illegal connection state transition"));
    }

    // -----------------------------------------------------------------------
    // SR_CONN_08 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_08_suspend_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Active).await;

        let result = svc.suspend(id).await.unwrap();
        assert_eq!(result.state, ConnectionState::Suspended);
    }

    #[tokio::test]
    async fn sr_conn_08_suspend_rejects_already_suspended() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Suspended).await;

        let err = svc.suspend(id).await.unwrap_err();
        assert!(err.to_string().contains("already suspended"));
    }

    // -----------------------------------------------------------------------
    // SR_CONN_09 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_09_decommission_revokes_credential() {
        let repo = Arc::new(MockConnectionRepo::new());
        let caas = Arc::new(MockCaaS::new());
        let svc = make_service(repo.clone(), caas.clone(), Arc::new(MockTester::passing()));

        let id = setup_connection(&svc, &repo, ConnectionState::Active).await;
        // Give the record a credential_ref
        {
            let mut records = repo.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                r.credential_ref = Some("vault://conn/test".to_string());
            }
        }

        let result = svc
            .decommission(DecommissionInput {
                connection_id: id,
                reason: "end of contract".to_string(),
                retain_data: false,
            })
            .await
            .unwrap();

        assert_eq!(result.state, ConnectionState::Decommissioned);
        let revoked = caas.revoked.lock().unwrap();
        assert_eq!(revoked.len(), 1);
        assert_eq!(revoked[0], "vault://conn/test");
    }

    #[tokio::test]
    async fn sr_conn_09_decommission_without_credential() {
        let repo = Arc::new(MockConnectionRepo::new());
        let caas = Arc::new(MockCaaS::new());
        let svc = make_service(repo.clone(), caas.clone(), Arc::new(MockTester::passing()));

        let id = setup_connection(&svc, &repo, ConnectionState::Failed).await;
        // Remove the credential_ref set during setup to simulate a connection
        // that failed before credential provisioning completed.
        {
            let mut records = repo.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                r.credential_ref = None;
            }
        }

        let result = svc
            .decommission(DecommissionInput {
                connection_id: id,
                reason: "cleanup".to_string(),
                retain_data: true,
            })
            .await
            .unwrap();

        assert_eq!(result.state, ConnectionState::Decommissioned);
        // No credential to revoke -- CaaS store_credential was called during
        // setup but we cleared the ref to simulate the no-credential path.
        let revoked = caas.revoked.lock().unwrap();
        assert_eq!(revoked.len(), 0);
    }

    // -----------------------------------------------------------------------
    // SR_CONN_10 tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sr_conn_10_recover_succeeds() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::passing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Failed).await;
        // Give it a credential
        {
            let mut records = repo.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                r.credential_ref = Some("vault://conn/test".to_string());
            }
        }

        let result = svc.recover(id).await.unwrap();

        assert!(result.passed);
        let conn = repo.records.lock().unwrap();
        let c = conn.iter().find(|r| r.id == id).unwrap();
        assert_eq!(c.status, ConnectionState::Active);
    }

    #[tokio::test]
    async fn sr_conn_10_recover_stays_failed() {
        let repo = Arc::new(MockConnectionRepo::new());
        let svc = make_service(
            repo.clone(),
            Arc::new(MockCaaS::new()),
            Arc::new(MockTester::failing()),
        );

        let id = setup_connection(&svc, &repo, ConnectionState::Failed).await;
        {
            let mut records = repo.records.lock().unwrap();
            if let Some(r) = records.iter_mut().find(|r| r.id == id) {
                r.credential_ref = Some("vault://conn/test".to_string());
            }
        }

        let result = svc.recover(id).await.unwrap();

        assert!(!result.passed);
        let conn = repo.records.lock().unwrap();
        let c = conn.iter().find(|r| r.id == id).unwrap();
        assert_eq!(c.status, ConnectionState::Failed);
    }
}
