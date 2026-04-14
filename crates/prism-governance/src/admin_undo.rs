//! Admin undo service (SR_GOV_69).
//!
//! Allows administrators to undo previously recorded actions within a
//! configurable time window. Security-critical actions cannot be undone.
//!
//! Implements: SR_GOV_69

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::AdminActionRepository;
use prism_core::types::*;

/// Default undo window in seconds (10 minutes).
const DEFAULT_UNDO_WINDOW_SECONDS: u64 = 600;

/// Service for recording and undoing admin actions.
///
/// Composes:
/// - `AdminActionRepository` -- persistence for admin actions
/// - `AuditLogger` -- audit trail for undo operations
///
/// Implements: SR_GOV_69
pub struct AdminUndoService {
    repo: Arc<dyn AdminActionRepository>,
    audit: AuditLogger,
}

impl AdminUndoService {
    /// Create a new admin undo service.
    pub fn new(repo: Arc<dyn AdminActionRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Record an admin action that may later be undone.
    ///
    /// Implements: SR_GOV_69
    pub async fn record_action(&self, action: &AdminAction) -> Result<(), PrismError> {
        self.repo.record(action).await
    }

    /// Attempt to undo a previously recorded admin action.
    ///
    /// Validates:
    /// - Action exists
    /// - Action is undoable (not security-critical)
    /// - Action is within the undo time window
    /// - Action has not already been undone
    ///
    /// Emits `admin.action_undone` audit event on success.
    ///
    /// Implements: SR_GOV_69
    pub async fn undo(&self, request: &UndoRequest) -> Result<UndoResult, PrismError> {
        // Look up the action
        let action = match self
            .repo
            .get_by_id(request.tenant_id, request.action_id)
            .await?
        {
            Some(a) => a,
            None => {
                return Ok(UndoResult {
                    undone: false,
                    reason_if_not: Some(format!("action {} not found", request.action_id)),
                });
            }
        };

        // Check: already undone?
        if action.is_undone {
            return Ok(UndoResult {
                undone: false,
                reason_if_not: Some("action has already been undone".into()),
            });
        }

        // Check: security-critical actions cannot be undone
        if action.is_security_critical {
            warn!(
                action_id = %action.id,
                tenant_id = %request.tenant_id,
                "undo rejected: security-critical action"
            );
            return Ok(UndoResult {
                undone: false,
                reason_if_not: Some("security-critical actions cannot be undone".into()),
            });
        }

        // Check: is the action undoable?
        if !action.is_undoable {
            return Ok(UndoResult {
                undone: false,
                reason_if_not: Some("action is not marked as undoable".into()),
            });
        }

        // Check: within time window
        let window = if action.undo_window_seconds > 0 {
            action.undo_window_seconds
        } else {
            DEFAULT_UNDO_WINDOW_SECONDS
        };

        let elapsed = Utc::now()
            .signed_duration_since(action.performed_at)
            .num_seconds();

        if elapsed < 0 || elapsed as u64 > window {
            return Ok(UndoResult {
                undone: false,
                reason_if_not: Some(format!(
                    "undo window expired ({}s elapsed, {}s allowed)",
                    elapsed, window
                )),
            });
        }

        // All checks passed -- mark as undone
        self.repo
            .mark_undone(request.tenant_id, request.action_id)
            .await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "admin.action_undone".into(),
                actor_id: *request.requesting_admin.as_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(request.action_id),
                target_type: Some("AdminAction".into()),
                severity: Severity::High,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "action_type": action.action_type,
                    "original_performer": action.performed_by.to_string(),
                    "elapsed_seconds": elapsed,
                }),
            })
            .await?;

        info!(
            action_id = %request.action_id,
            tenant_id = %request.tenant_id,
            requesting_admin = %request.requesting_admin,
            "admin action undone"
        );

        Ok(UndoResult {
            undone: true,
            reason_if_not: None,
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

    // -- Mock AdminActionRepository -------------------------------------------

    struct MockAdminActionRepo {
        actions: Mutex<Vec<AdminAction>>,
    }

    impl MockAdminActionRepo {
        fn new() -> Self {
            Self {
                actions: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl AdminActionRepository for MockAdminActionRepo {
        async fn record(&self, action: &AdminAction) -> Result<(), PrismError> {
            self.actions.lock().unwrap().push(action.clone());
            Ok(())
        }

        async fn get_by_id(
            &self,
            tenant_id: TenantId,
            action_id: uuid::Uuid,
        ) -> Result<Option<AdminAction>, PrismError> {
            let actions = self.actions.lock().unwrap();
            Ok(actions
                .iter()
                .find(|a| a.id == action_id && a.tenant_id == tenant_id)
                .cloned())
        }

        async fn mark_undone(
            &self,
            tenant_id: TenantId,
            action_id: uuid::Uuid,
        ) -> Result<(), PrismError> {
            let mut actions = self.actions.lock().unwrap();
            if let Some(action) = actions
                .iter_mut()
                .find(|a| a.id == action_id && a.tenant_id == tenant_id)
            {
                action.is_undone = true;
            }
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

    fn make_service() -> (AdminUndoService, Arc<MockAdminActionRepo>) {
        let repo = Arc::new(MockAdminActionRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        let svc = AdminUndoService::new(repo.clone(), audit);
        (svc, repo)
    }

    fn make_action(tenant_id: TenantId) -> AdminAction {
        AdminAction {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            action_type: "config.update".into(),
            payload: serde_json::json!({"key": "value"}),
            performed_by: UserId::new(),
            is_undoable: true,
            is_security_critical: false,
            performed_at: Utc::now(),
            undo_window_seconds: 600,
            is_undone: false,
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn undo_succeeds_within_window() {
        let (svc, repo) = make_service();
        let tenant_id = TenantId::new();
        let action = make_action(tenant_id);
        let action_id = action.id;
        repo.record(&action).await.unwrap();

        let request = UndoRequest {
            tenant_id,
            action_id,
            requesting_admin: UserId::new(),
        };

        let result = svc.undo(&request).await.unwrap();
        assert!(result.undone);
        assert!(result.reason_if_not.is_none());

        // Verify marked as undone in repo
        let stored = repo.get_by_id(tenant_id, action_id).await.unwrap().unwrap();
        assert!(stored.is_undone);
    }

    #[tokio::test]
    async fn undo_rejects_security_critical() {
        let (svc, repo) = make_service();
        let tenant_id = TenantId::new();
        let mut action = make_action(tenant_id);
        action.is_security_critical = true;
        let action_id = action.id;
        repo.record(&action).await.unwrap();

        let request = UndoRequest {
            tenant_id,
            action_id,
            requesting_admin: UserId::new(),
        };

        let result = svc.undo(&request).await.unwrap();
        assert!(!result.undone);
        assert!(result.reason_if_not.unwrap().contains("security-critical"));
    }

    #[tokio::test]
    async fn undo_rejects_expired_window() {
        let (svc, repo) = make_service();
        let tenant_id = TenantId::new();
        let mut action = make_action(tenant_id);
        // Set performed_at far in the past (beyond the 600s window)
        action.performed_at = Utc::now() - chrono::Duration::seconds(1200);
        let action_id = action.id;
        repo.record(&action).await.unwrap();

        let request = UndoRequest {
            tenant_id,
            action_id,
            requesting_admin: UserId::new(),
        };

        let result = svc.undo(&request).await.unwrap();
        assert!(!result.undone);
        assert!(result.reason_if_not.unwrap().contains("expired"));
    }

    #[tokio::test]
    async fn undo_rejects_nonexistent_action() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();

        let request = UndoRequest {
            tenant_id,
            action_id: uuid::Uuid::new_v4(),
            requesting_admin: UserId::new(),
        };

        let result = svc.undo(&request).await.unwrap();
        assert!(!result.undone);
        assert!(result.reason_if_not.unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn undo_rejects_already_undone() {
        let (svc, repo) = make_service();
        let tenant_id = TenantId::new();
        let mut action = make_action(tenant_id);
        action.is_undone = true;
        let action_id = action.id;
        repo.record(&action).await.unwrap();

        let request = UndoRequest {
            tenant_id,
            action_id,
            requesting_admin: UserId::new(),
        };

        let result = svc.undo(&request).await.unwrap();
        assert!(!result.undone);
        assert!(result
            .reason_if_not
            .unwrap()
            .contains("already been undone"));
    }
}
