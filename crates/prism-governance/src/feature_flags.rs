//! Feature flag toggle service (SR_GOV_68).
//!
//! Provides governance-controlled feature flags that can be toggled by
//! authorized administrators. Each toggle is audited.
//!
//! Implements: SR_GOV_68

use std::sync::Arc;

use chrono::Utc;
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::FeatureFlagRepository;
use prism_core::types::*;

/// Service for managing governance-controlled feature flags.
///
/// Composes:
/// - `FeatureFlagRepository` -- persistence for flags
/// - `AuditLogger` -- audit trail for flag toggles
///
/// Implements: SR_GOV_68
pub struct FeatureFlagService {
    repo: Arc<dyn FeatureFlagRepository>,
    audit: AuditLogger,
}

impl FeatureFlagService {
    /// Create a new feature flag service.
    pub fn new(repo: Arc<dyn FeatureFlagRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Toggle a feature flag on or off.
    ///
    /// Validates:
    /// - Flag exists in the repository
    /// - `approved_by` is non-nil (basic admin check)
    ///
    /// Emits `feature_flag_toggled` audit event on success.
    ///
    /// Implements: SR_GOV_68
    pub async fn toggle(
        &self,
        request: &FeatureFlagToggleRequest,
    ) -> Result<FeatureFlagResult, PrismError> {
        // Validate approved_by is non-nil
        if *request.approved_by.as_uuid() == uuid::Uuid::nil() {
            return Err(PrismError::Validation {
                reason: "approved_by must identify a valid admin (non-nil)".into(),
            });
        }

        // Get existing flag
        let mut flag = self
            .repo
            .get(request.tenant_id, &request.flag_id)
            .await?
            .ok_or_else(|| PrismError::NotFound {
                entity_type: "FeatureFlag",
                id: *request.approved_by.as_uuid(), // best effort id for error
            })?;

        let previous_value = flag.value;
        flag.value = request.value;
        flag.approved_by = request.approved_by;
        flag.updated_at = Utc::now();

        self.repo.set(&flag).await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "feature_flag_toggled".into(),
                actor_id: *request.approved_by.as_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(flag.id),
                target_type: Some("FeatureFlag".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "flag_id": request.flag_id,
                    "previous_value": previous_value,
                    "new_value": request.value,
                }),
            })
            .await?;

        info!(
            tenant_id = %request.tenant_id,
            flag_id = %request.flag_id,
            previous = previous_value,
            new = request.value,
            "feature flag toggled"
        );

        Ok(FeatureFlagResult {
            active: request.value,
        })
    }

    /// Check whether a feature flag is enabled.
    ///
    /// Returns `false` if the flag does not exist (safe default).
    ///
    /// Implements: SR_GOV_68
    pub async fn is_enabled(&self, tenant_id: TenantId, flag_id: &str) -> Result<bool, PrismError> {
        match self.repo.get(tenant_id, flag_id).await? {
            Some(flag) => Ok(flag.value),
            None => Ok(false),
        }
    }

    /// List all feature flags for a tenant.
    ///
    /// Implements: SR_GOV_68
    pub async fn list_for_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<FeatureFlag>, PrismError> {
        self.repo.list_for_tenant(tenant_id).await
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

    // -- Mock FeatureFlagRepository -------------------------------------------

    struct MockFlagRepo {
        flags: Mutex<Vec<FeatureFlag>>,
    }

    impl MockFlagRepo {
        fn new() -> Self {
            Self {
                flags: Mutex::new(Vec::new()),
            }
        }

        fn seed(&self, flag: FeatureFlag) {
            self.flags.lock().unwrap().push(flag);
        }
    }

    #[async_trait]
    impl FeatureFlagRepository for MockFlagRepo {
        async fn get(
            &self,
            tenant_id: TenantId,
            flag_id: &str,
        ) -> Result<Option<FeatureFlag>, PrismError> {
            let flags = self.flags.lock().unwrap();
            Ok(flags
                .iter()
                .find(|f| f.tenant_id == tenant_id && f.flag_id == flag_id)
                .cloned())
        }

        async fn set(&self, flag: &FeatureFlag) -> Result<(), PrismError> {
            let mut flags = self.flags.lock().unwrap();
            if let Some(existing) = flags
                .iter_mut()
                .find(|f| f.tenant_id == flag.tenant_id && f.flag_id == flag.flag_id)
            {
                *existing = flag.clone();
            } else {
                flags.push(flag.clone());
            }
            Ok(())
        }

        async fn list_for_tenant(
            &self,
            tenant_id: TenantId,
        ) -> Result<Vec<FeatureFlag>, PrismError> {
            let flags = self.flags.lock().unwrap();
            Ok(flags
                .iter()
                .filter(|f| f.tenant_id == tenant_id)
                .cloned()
                .collect())
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

        fn event_count(&self) -> usize {
            self.events.lock().unwrap().len()
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

    fn make_flag(tenant_id: TenantId, flag_id: &str, value: bool) -> FeatureFlag {
        let now = Utc::now();
        FeatureFlag {
            id: uuid::Uuid::new_v4(),
            tenant_id,
            flag_id: flag_id.into(),
            value,
            approved_by: UserId::new(),
            plan_tier_required: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_service() -> (FeatureFlagService, Arc<MockFlagRepo>, Arc<MockAuditRepo>) {
        let flag_repo = Arc::new(MockFlagRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo.clone());
        let svc = FeatureFlagService::new(flag_repo.clone(), audit);
        (svc, flag_repo, audit_repo)
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn toggle_on() {
        let (svc, repo, _) = make_service();
        let tenant_id = TenantId::new();
        repo.seed(make_flag(tenant_id, "feature_x", false));

        let request = FeatureFlagToggleRequest {
            tenant_id,
            flag_id: "feature_x".into(),
            value: true,
            approved_by: UserId::new(),
        };

        let result = svc.toggle(&request).await.unwrap();
        assert!(result.active);

        // Verify persisted
        let flag = repo.get(tenant_id, "feature_x").await.unwrap().unwrap();
        assert!(flag.value);
    }

    #[tokio::test]
    async fn toggle_off() {
        let (svc, repo, _) = make_service();
        let tenant_id = TenantId::new();
        repo.seed(make_flag(tenant_id, "feature_y", true));

        let request = FeatureFlagToggleRequest {
            tenant_id,
            flag_id: "feature_y".into(),
            value: false,
            approved_by: UserId::new(),
        };

        let result = svc.toggle(&request).await.unwrap();
        assert!(!result.active);
    }

    #[tokio::test]
    async fn is_enabled_returns_false_for_unknown() {
        let (svc, _, _) = make_service();
        let tenant_id = TenantId::new();

        let enabled = svc.is_enabled(tenant_id, "nonexistent").await.unwrap();
        assert!(!enabled);
    }

    #[tokio::test]
    async fn toggle_emits_audit_event() {
        let (svc, repo, audit_repo) = make_service();
        let tenant_id = TenantId::new();
        repo.seed(make_flag(tenant_id, "audit_flag", false));

        let request = FeatureFlagToggleRequest {
            tenant_id,
            flag_id: "audit_flag".into(),
            value: true,
            approved_by: UserId::new(),
        };

        svc.toggle(&request).await.unwrap();
        assert_eq!(audit_repo.event_count(), 1);
    }

    #[tokio::test]
    async fn list_for_tenant_returns_tenant_flags() {
        let (svc, repo, _) = make_service();
        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();

        repo.seed(make_flag(tenant_a, "flag_1", true));
        repo.seed(make_flag(tenant_a, "flag_2", false));
        repo.seed(make_flag(tenant_b, "flag_3", true));

        let flags = svc.list_for_tenant(tenant_a).await.unwrap();
        assert_eq!(flags.len(), 2);
    }
}
