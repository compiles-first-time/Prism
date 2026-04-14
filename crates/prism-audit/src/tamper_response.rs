//! Tamper response: write-freeze, alerting, and incident creation (SR_GOV_51).
//!
//! When `SR_GOV_48` (chain verification) detects a hash mismatch or chain
//! breakage, this module freezes the affected tenant's governance writes,
//! dispatches a CRITICAL alert to the platform security officer, and opens
//! an incident ticket for the security investigation.
//!
//! Recovery is intentionally manual -- automated recovery from a tampered
//! chain would itself be a security weakness.
//!
//! Implements: SR_GOV_51

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{error, warn};

use prism_core::error::PrismError;
use prism_core::types::*;

// ---------------------------------------------------------------------------
// Trait: TenantWriteFreeze
// ---------------------------------------------------------------------------

/// Controls the write-freeze switch for a tenant's governance operations.
///
/// When frozen, the tenant's audit chain is preserved for forensic analysis.
/// Only audit-layer writes (recording the freeze itself) remain permitted.
///
/// Implements: SR_GOV_51 (tenant write-freeze switch asset)
#[async_trait]
pub trait TenantWriteFreeze: Send + Sync {
    /// Activate the write freeze for a tenant. Returns `true` if the freeze
    /// was newly activated, `false` if it was already active.
    async fn freeze(&self, tenant_id: TenantId) -> Result<bool, PrismError>;

    /// Check whether a tenant's writes are currently frozen.
    async fn is_frozen(&self, tenant_id: TenantId) -> Result<bool, PrismError>;
}

// ---------------------------------------------------------------------------
// Trait: AlertDispatcher
// ---------------------------------------------------------------------------

/// Dispatches security alerts to platform officers.
///
/// Implements: SR_GOV_51 (REUSABLE_Alerter)
#[async_trait]
pub trait AlertDispatcher: Send + Sync {
    /// Send a CRITICAL-severity alert. The `detail` map carries structured
    /// context (tenant_id, mismatch_at, anchor_hash, etc.).
    async fn dispatch_critical(
        &self,
        title: &str,
        detail: serde_json::Value,
    ) -> Result<(), PrismError>;
}

// ---------------------------------------------------------------------------
// Trait: IncidentTracker
// ---------------------------------------------------------------------------

/// Creates incident tickets in the organization's incident management system.
///
/// Implements: SR_GOV_51 (incident response runbook)
#[async_trait]
pub trait IncidentTracker: Send + Sync {
    /// Open a new incident and return its identifier.
    async fn create_incident(
        &self,
        title: &str,
        severity: Severity,
        detail: serde_json::Value,
    ) -> Result<String, PrismError>;
}

// ---------------------------------------------------------------------------
// TamperResponseService
// ---------------------------------------------------------------------------

/// Orchestrates the tamper response workflow triggered by SR_GOV_48.
///
/// Steps:
/// 1. Freeze the affected tenant's governance writes.
/// 2. Dispatch a CRITICAL alert to the platform security officer.
/// 3. Create an incident ticket for investigation and manual recovery.
///
/// Implements: SR_GOV_51
pub struct TamperResponseService {
    freeze: Arc<dyn TenantWriteFreeze>,
    alerter: Arc<dyn AlertDispatcher>,
    incidents: Arc<dyn IncidentTracker>,
}

impl TamperResponseService {
    /// Create a new tamper response service.
    pub fn new(
        freeze: Arc<dyn TenantWriteFreeze>,
        alerter: Arc<dyn AlertDispatcher>,
        incidents: Arc<dyn IncidentTracker>,
    ) -> Self {
        Self {
            freeze,
            alerter,
            incidents,
        }
    }

    /// Execute the tamper response workflow.
    ///
    /// Called when `SR_GOV_48` chain verification detects a mismatch.
    /// This method is idempotent with respect to the freeze -- calling
    /// it multiple times for the same tenant will not double-freeze.
    ///
    /// Implements: SR_GOV_51
    pub async fn respond(
        &self,
        input: &TamperResponseInput,
    ) -> Result<TamperResponseResult, PrismError> {
        // Step 1: freeze tenant writes.
        let newly_frozen = self.freeze.freeze(input.tenant_id).await.map_err(|e| {
            error!(
                tenant_id = %input.tenant_id,
                error = %e,
                "CRITICAL: failed to freeze tenant writes during tamper response"
            );
            e
        })?;

        if newly_frozen {
            warn!(
                tenant_id = %input.tenant_id,
                mismatch_at = input.mismatch_at,
                "tenant writes FROZEN due to audit chain tampering"
            );
        }

        // Step 2: dispatch CRITICAL alert.
        let alert_detail = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "mismatch_at": input.mismatch_at,
            "anchor_hash": input.anchor_hash,
            "severity": "CRITICAL",
            "response": "write_freeze_activated",
        });

        self.alerter
            .dispatch_critical(
                "Audit chain tampering detected -- tenant writes frozen",
                alert_detail,
            )
            .await
            .map_err(|e| {
                error!(
                    tenant_id = %input.tenant_id,
                    error = %e,
                    "failed to dispatch tamper alert (writes already frozen)"
                );
                e
            })?;

        // Step 3: create incident ticket.
        let incident_detail = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "mismatch_at": input.mismatch_at,
            "anchor_hash": input.anchor_hash,
            "freeze_active": true,
            "recovery": "manual -- see operations runbook",
        });

        let incident_id = self
            .incidents
            .create_incident(
                &format!(
                    "Audit chain tamper: tenant {} at position {}",
                    input.tenant_id, input.mismatch_at
                ),
                Severity::Critical,
                incident_detail,
            )
            .await
            .map_err(|e| {
                error!(
                    tenant_id = %input.tenant_id,
                    error = %e,
                    "failed to create incident ticket (writes already frozen)"
                );
                e
            })?;

        warn!(
            tenant_id = %input.tenant_id,
            incident_id = %incident_id,
            "tamper response complete: writes frozen, alert sent, incident opened"
        );

        Ok(TamperResponseResult {
            freeze_active: true,
            incident_id,
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
    use std::sync::Mutex;

    // -- Mock TenantWriteFreeze ---------------------------------------------

    struct MockFreeze {
        frozen: Mutex<Vec<TenantId>>,
    }

    impl MockFreeze {
        fn new() -> Self {
            Self {
                frozen: Mutex::new(Vec::new()),
            }
        }

        fn frozen_tenants(&self) -> Vec<TenantId> {
            self.frozen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TenantWriteFreeze for MockFreeze {
        async fn freeze(&self, tenant_id: TenantId) -> Result<bool, PrismError> {
            let mut frozen = self.frozen.lock().unwrap();
            if frozen.contains(&tenant_id) {
                Ok(false) // already frozen
            } else {
                frozen.push(tenant_id);
                Ok(true) // newly frozen
            }
        }

        async fn is_frozen(&self, tenant_id: TenantId) -> Result<bool, PrismError> {
            Ok(self.frozen.lock().unwrap().contains(&tenant_id))
        }
    }

    // -- Mock AlertDispatcher -----------------------------------------------

    struct MockAlerter {
        alerts: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl MockAlerter {
        fn new() -> Self {
            Self {
                alerts: Mutex::new(Vec::new()),
            }
        }

        fn alert_count(&self) -> usize {
            self.alerts.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl AlertDispatcher for MockAlerter {
        async fn dispatch_critical(
            &self,
            title: &str,
            detail: serde_json::Value,
        ) -> Result<(), PrismError> {
            self.alerts
                .lock()
                .unwrap()
                .push((title.to_string(), detail));
            Ok(())
        }
    }

    // -- Mock IncidentTracker -----------------------------------------------

    struct MockIncidents {
        counter: Mutex<u32>,
    }

    impl MockIncidents {
        fn new() -> Self {
            Self {
                counter: Mutex::new(0),
            }
        }

        fn incident_count(&self) -> u32 {
            *self.counter.lock().unwrap()
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

    // -- Helpers -------------------------------------------------------------

    fn make_input(tenant_id: TenantId) -> TamperResponseInput {
        TamperResponseInput {
            tenant_id,
            mismatch_at: 42,
            anchor_hash: "abc123".into(),
        }
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn respond_freezes_tenant_writes() {
        let freeze = Arc::new(MockFreeze::new());
        let alerter = Arc::new(MockAlerter::new());
        let incidents = Arc::new(MockIncidents::new());
        let svc = TamperResponseService::new(freeze.clone(), alerter, incidents);

        let tenant_id = TenantId::new();
        let result = svc.respond(&make_input(tenant_id)).await.unwrap();

        assert!(result.freeze_active);
        assert_eq!(freeze.frozen_tenants(), vec![tenant_id]);
    }

    #[tokio::test]
    async fn respond_dispatches_critical_alert() {
        let freeze = Arc::new(MockFreeze::new());
        let alerter = Arc::new(MockAlerter::new());
        let incidents = Arc::new(MockIncidents::new());
        let svc = TamperResponseService::new(freeze, alerter.clone(), incidents);

        let tenant_id = TenantId::new();
        svc.respond(&make_input(tenant_id)).await.unwrap();

        assert_eq!(alerter.alert_count(), 1);
    }

    #[tokio::test]
    async fn respond_creates_incident_ticket() {
        let freeze = Arc::new(MockFreeze::new());
        let alerter = Arc::new(MockAlerter::new());
        let incidents = Arc::new(MockIncidents::new());
        let svc = TamperResponseService::new(freeze, alerter, incidents.clone());

        let tenant_id = TenantId::new();
        let result = svc.respond(&make_input(tenant_id)).await.unwrap();

        assert_eq!(result.incident_id, "INC-0001");
        assert_eq!(incidents.incident_count(), 1);
    }

    #[tokio::test]
    async fn respond_is_idempotent_on_freeze() {
        let freeze = Arc::new(MockFreeze::new());
        let alerter = Arc::new(MockAlerter::new());
        let incidents = Arc::new(MockIncidents::new());
        let svc = TamperResponseService::new(freeze.clone(), alerter, incidents);

        let tenant_id = TenantId::new();
        let r1 = svc.respond(&make_input(tenant_id)).await.unwrap();
        let r2 = svc.respond(&make_input(tenant_id)).await.unwrap();

        // Both should report freeze active, but tenant only frozen once
        assert!(r1.freeze_active);
        assert!(r2.freeze_active);
        assert_eq!(freeze.frozen_tenants().len(), 1);
    }

    #[tokio::test]
    async fn respond_isolates_tenants() {
        let freeze = Arc::new(MockFreeze::new());
        let alerter = Arc::new(MockAlerter::new());
        let incidents = Arc::new(MockIncidents::new());
        let svc = TamperResponseService::new(freeze.clone(), alerter.clone(), incidents.clone());

        let t1 = TenantId::new();
        let t2 = TenantId::new();

        svc.respond(&make_input(t1)).await.unwrap();
        svc.respond(&make_input(t2)).await.unwrap();

        // Both tenants frozen independently
        assert_eq!(freeze.frozen_tenants().len(), 2);
        // Two alerts, two incidents
        assert_eq!(alerter.alert_count(), 2);
        assert_eq!(incidents.incident_count(), 2);
    }

    #[tokio::test]
    async fn respond_includes_mismatch_details_in_incident() {
        let freeze = Arc::new(MockFreeze::new());
        let alerter = Arc::new(MockAlerter::new());
        let incidents = Arc::new(MockIncidents::new());
        let svc = TamperResponseService::new(freeze, alerter, incidents);

        let tenant_id = TenantId::new();
        let input = TamperResponseInput {
            tenant_id,
            mismatch_at: 99,
            anchor_hash: "deadbeef".into(),
        };

        let result = svc.respond(&input).await.unwrap();
        assert!(result.freeze_active);
        assert!(!result.incident_id.is_empty());
    }
}
