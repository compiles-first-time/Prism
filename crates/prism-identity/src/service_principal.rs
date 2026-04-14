//! Service principal and user identity management (FOUND S 1.3.1, SR_GOV_10).
//!
//! The `IdentityService` is the single entry point for provisioning users
//! and service principals within a tenant. It enforces validation, writes
//! audit events, and composes the repository traits from prism-core.

use std::sync::Arc;

use chrono::Utc;
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::{ServicePrincipalRepository, UserRepository};
use prism_core::types::*;

/// Identity governance service.
///
/// Implements: SR_GOV_10 (user provisioning), FOUND S 1.3.1 (service principal)
#[derive(Clone)]
pub struct IdentityService {
    user_repo: Arc<dyn UserRepository>,
    sp_repo: Arc<dyn ServicePrincipalRepository>,
    audit: AuditLogger,
}

impl IdentityService {
    pub fn new(
        user_repo: Arc<dyn UserRepository>,
        sp_repo: Arc<dyn ServicePrincipalRepository>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            user_repo,
            sp_repo,
            audit,
        }
    }

    /// Provision a new user within a tenant.
    ///
    /// 1. Validate input (email, display name).
    /// 2. Check for duplicate email within the tenant.
    /// 3. Create the user via repository.
    /// 4. Write audit event.
    ///
    /// Implements: SR_GOV_10, SR_DM_02
    pub async fn provision_user(
        &self,
        tenant_id: TenantId,
        email: &str,
        display_name: &str,
        department: Option<&str>,
        idp_id: Option<&str>,
        actor_id: uuid::Uuid,
    ) -> Result<User, PrismError> {
        // Validate email
        let email = email.trim().to_lowercase();
        if email.is_empty() || !email.contains('@') {
            return Err(PrismError::Validation {
                reason: "valid email address is required".into(),
            });
        }

        // Validate display name
        let display_name = display_name.trim().to_string();
        if display_name.is_empty() {
            return Err(PrismError::Validation {
                reason: "display name must not be empty".into(),
            });
        }

        // Check duplicate (BE-01 equivalent for users)
        if let Some(_existing) = self.user_repo.get_by_email(tenant_id, &email).await? {
            return Err(PrismError::Conflict {
                reason: format!("user with email '{}' already exists in tenant", email),
            });
        }

        let now = Utc::now();
        let user = User {
            id: UserId::new(),
            tenant_id,
            idp_id: idp_id.map(String::from),
            email: email.clone(),
            display_name,
            role_ids: vec![],
            primary_reporting_line: None,
            secondary_reporting_line: None,
            department: department.map(String::from),
            business_unit: None,
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        self.user_repo.create(&user).await?;

        info!(user_id = %user.id, tenant_id = %tenant_id, email = %email, "user provisioned");

        self.audit
            .log(AuditEventInput {
                tenant_id,
                event_type: "person.provisioned".into(),
                actor_id,
                actor_type: ActorType::System,
                target_id: Some(user.id.into_uuid()),
                target_type: Some("User".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Identity,
                governance_authority: None,
                payload: serde_json::json!({
                    "email": email,
                    "department": user.department,
                    "idp_id": user.idp_id,
                }),
            })
            .await?;

        Ok(user)
    }

    /// Provision a new service principal for an automation.
    ///
    /// Track A only: `identity_type` defaults to `Automation`,
    /// `governance_profile` defaults to `Tool`.
    ///
    /// Implements: FOUND S 1.3.1, SR_DM_20
    pub async fn provision_service_principal(
        &self,
        request: ServicePrincipalProvisionRequest,
        actor_id: uuid::Uuid,
    ) -> Result<ServicePrincipal, PrismError> {
        // Validate display name
        let display_name = request.display_name.trim().to_string();
        if display_name.is_empty() {
            return Err(PrismError::Validation {
                reason: "service principal display name must not be empty".into(),
            });
        }

        let now = Utc::now();
        let sp = ServicePrincipal {
            id: ServicePrincipalId::new(),
            tenant_id: request.tenant_id,
            automation_id: request.automation_id,
            display_name,
            identity_type: request.identity_type,
            governance_profile: request.governance_profile,
            permissions: serde_json::json!({}),
            credential_id: None,
            owner_id: request.owner_id,
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        self.sp_repo.create(&sp).await?;

        info!(
            sp_id = %sp.id,
            tenant_id = %request.tenant_id,
            identity_type = ?sp.identity_type,
            "service principal provisioned"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "service_principal.provisioned".into(),
                actor_id,
                actor_type: ActorType::Human,
                target_id: Some(sp.id.into_uuid()),
                target_type: Some("ServicePrincipal".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Identity,
                governance_authority: None,
                payload: serde_json::json!({
                    "identity_type": sp.identity_type,
                    "governance_profile": sp.governance_profile,
                    "automation_id": sp.automation_id,
                    "owner_id": sp.owner_id,
                }),
            })
            .await?;

        Ok(sp)
    }

    /// Deactivate a service principal (kill switch).
    ///
    /// Track A: instant and irreversible.
    ///
    /// Implements: FOUND S 1.3.1 (security properties)
    pub async fn deactivate_service_principal(
        &self,
        tenant_id: TenantId,
        sp_id: ServicePrincipalId,
        actor_id: uuid::Uuid,
    ) -> Result<(), PrismError> {
        // Verify SP exists and belongs to this tenant
        let sp = self
            .sp_repo
            .get_by_id(sp_id)
            .await?
            .ok_or(PrismError::NotFound {
                entity_type: "ServicePrincipal",
                id: sp_id.into_uuid(),
            })?;

        if sp.tenant_id != tenant_id {
            return Err(PrismError::Forbidden {
                reason: "service principal does not belong to this tenant".into(),
            });
        }

        self.sp_repo.deactivate(sp_id).await?;

        info!(sp_id = %sp_id, tenant_id = %tenant_id, "service principal deactivated");

        self.audit
            .log(AuditEventInput {
                tenant_id,
                event_type: "service_principal.deactivated".into(),
                actor_id,
                actor_type: ActorType::Human,
                target_id: Some(sp_id.into_uuid()),
                target_type: Some("ServicePrincipal".into()),
                severity: Severity::High,
                source_layer: SourceLayer::Identity,
                governance_authority: None,
                payload: serde_json::json!({
                    "action": "kill_switch",
                    "identity_type": sp.identity_type,
                }),
            })
            .await?;

        Ok(())
    }

    /// Retrieve a user by ID within a tenant.
    pub async fn get_user(&self, tenant_id: TenantId, user_id: UserId) -> Result<User, PrismError> {
        self.user_repo
            .get_by_id(tenant_id, user_id)
            .await?
            .ok_or(PrismError::NotFound {
                entity_type: "User",
                id: user_id.into_uuid(),
            })
    }

    /// List service principals for a tenant.
    pub async fn list_service_principals(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<ServicePrincipal>, PrismError> {
        self.sp_repo.list_by_tenant(tenant_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use prism_core::repository::AuditEventRepository;
    use std::sync::Mutex;

    // -- Mock UserRepository --------------------------------------------------

    struct MockUserRepo {
        users: Mutex<Vec<User>>,
    }

    impl MockUserRepo {
        fn new() -> Self {
            Self {
                users: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl UserRepository for MockUserRepo {
        async fn create(&self, user: &User) -> Result<(), PrismError> {
            self.users.lock().unwrap().push(user.clone());
            Ok(())
        }

        async fn get_by_id(
            &self,
            tenant_id: TenantId,
            id: UserId,
        ) -> Result<Option<User>, PrismError> {
            let users = self.users.lock().unwrap();
            Ok(users
                .iter()
                .find(|u| u.id == id && u.tenant_id == tenant_id)
                .cloned())
        }

        async fn get_by_email(
            &self,
            tenant_id: TenantId,
            email: &str,
        ) -> Result<Option<User>, PrismError> {
            let users = self.users.lock().unwrap();
            Ok(users
                .iter()
                .find(|u| u.email == email && u.tenant_id == tenant_id)
                .cloned())
        }
    }

    // -- Mock ServicePrincipalRepository --------------------------------------

    struct MockSpRepo {
        sps: Mutex<Vec<ServicePrincipal>>,
    }

    impl MockSpRepo {
        fn new() -> Self {
            Self {
                sps: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ServicePrincipalRepository for MockSpRepo {
        async fn create(&self, sp: &ServicePrincipal) -> Result<(), PrismError> {
            self.sps.lock().unwrap().push(sp.clone());
            Ok(())
        }

        async fn get_by_id(
            &self,
            id: ServicePrincipalId,
        ) -> Result<Option<ServicePrincipal>, PrismError> {
            let sps = self.sps.lock().unwrap();
            Ok(sps.iter().find(|s| s.id == id).cloned())
        }

        async fn list_by_tenant(
            &self,
            tenant_id: TenantId,
        ) -> Result<Vec<ServicePrincipal>, PrismError> {
            let sps = self.sps.lock().unwrap();
            Ok(sps
                .iter()
                .filter(|s| s.tenant_id == tenant_id)
                .cloned()
                .collect())
        }

        async fn deactivate(&self, id: ServicePrincipalId) -> Result<(), PrismError> {
            let mut sps = self.sps.lock().unwrap();
            if let Some(sp) = sps.iter_mut().find(|s| s.id == id) {
                sp.is_active = false;
                Ok(())
            } else {
                Err(PrismError::NotFound {
                    entity_type: "ServicePrincipal",
                    id: id.into_uuid(),
                })
            }
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

        async fn query(&self, _req: &AuditQueryRequest) -> Result<AuditQueryResult, PrismError> {
            Ok(AuditQueryResult {
                events: vec![],
                next_page_token: None,
                total_count: 0,
            })
        }

        async fn get_chain_segment(
            &self,
            _tid: TenantId,
            _depth: u32,
        ) -> Result<Vec<AuditEvent>, PrismError> {
            Ok(vec![])
        }
    }

    // -- Helpers --------------------------------------------------------------

    fn make_service() -> (
        IdentityService,
        Arc<MockUserRepo>,
        Arc<MockSpRepo>,
        Arc<MockAuditRepo>,
    ) {
        let user_repo = Arc::new(MockUserRepo::new());
        let sp_repo = Arc::new(MockSpRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo.clone());
        let svc = IdentityService::new(user_repo.clone(), sp_repo.clone(), audit);
        (svc, user_repo, sp_repo, audit_repo)
    }

    // -- User tests -----------------------------------------------------------

    #[tokio::test]
    async fn provision_user_creates_and_audits() {
        let (svc, user_repo, _, audit_repo) = make_service();
        let tid = TenantId::new();
        let actor = uuid::Uuid::nil();

        let user = svc
            .provision_user(
                tid,
                "jane@meridian.com",
                "Jane Doe",
                Some("Engineering"),
                None,
                actor,
            )
            .await
            .unwrap();

        assert_eq!(user.email, "jane@meridian.com");
        assert!(user.is_active);
        assert_eq!(user_repo.users.lock().unwrap().len(), 1);
        assert_eq!(
            audit_repo.events.lock().unwrap()[0].event_type,
            "person.provisioned"
        );
    }

    #[tokio::test]
    async fn provision_user_rejects_invalid_email() {
        let (svc, _, _, _) = make_service();
        let result = svc
            .provision_user(
                TenantId::new(),
                "not-an-email",
                "Test",
                None,
                None,
                uuid::Uuid::nil(),
            )
            .await;
        assert!(matches!(result, Err(PrismError::Validation { .. })));
    }

    #[tokio::test]
    async fn provision_user_rejects_empty_name() {
        let (svc, _, _, _) = make_service();
        let result = svc
            .provision_user(
                TenantId::new(),
                "x@y.com",
                "  ",
                None,
                None,
                uuid::Uuid::nil(),
            )
            .await;
        assert!(matches!(result, Err(PrismError::Validation { .. })));
    }

    #[tokio::test]
    async fn provision_user_rejects_duplicate_email() {
        let (svc, _, _, _) = make_service();
        let tid = TenantId::new();
        let actor = uuid::Uuid::nil();

        svc.provision_user(tid, "a@b.com", "First", None, None, actor)
            .await
            .unwrap();

        let result = svc
            .provision_user(tid, "a@b.com", "Second", None, None, actor)
            .await;
        assert!(matches!(result, Err(PrismError::Conflict { .. })));
    }

    #[tokio::test]
    async fn provision_user_normalizes_email() {
        let (svc, user_repo, _, _) = make_service();
        svc.provision_user(
            TenantId::new(),
            "  Jane@MERIDIAN.com  ",
            "Jane",
            None,
            None,
            uuid::Uuid::nil(),
        )
        .await
        .unwrap();

        let users = user_repo.users.lock().unwrap();
        assert_eq!(users[0].email, "jane@meridian.com");
    }

    // -- Service Principal tests ----------------------------------------------

    #[tokio::test]
    async fn provision_sp_creates_and_audits() {
        let (svc, _, sp_repo, audit_repo) = make_service();
        let tid = TenantId::new();

        let sp = svc
            .provision_service_principal(
                ServicePrincipalProvisionRequest {
                    tenant_id: tid,
                    display_name: "Invoice Bot".into(),
                    automation_id: None,
                    identity_type: IdentityType::Automation,
                    governance_profile: GovernanceProfile::Tool,
                    owner_id: None,
                },
                uuid::Uuid::nil(),
            )
            .await
            .unwrap();

        assert_eq!(sp.display_name, "Invoice Bot");
        assert!(sp.is_active);
        assert_eq!(sp.identity_type, IdentityType::Automation);
        assert_eq!(sp_repo.sps.lock().unwrap().len(), 1);

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events[0].event_type, "service_principal.provisioned");
    }

    #[tokio::test]
    async fn provision_sp_rejects_empty_name() {
        let (svc, _, _, _) = make_service();
        let result = svc
            .provision_service_principal(
                ServicePrincipalProvisionRequest {
                    tenant_id: TenantId::new(),
                    display_name: "".into(),
                    automation_id: None,
                    identity_type: IdentityType::Automation,
                    governance_profile: GovernanceProfile::Tool,
                    owner_id: None,
                },
                uuid::Uuid::nil(),
            )
            .await;
        assert!(matches!(result, Err(PrismError::Validation { .. })));
    }

    // -- Deactivation tests ---------------------------------------------------

    #[tokio::test]
    async fn deactivate_sp_kill_switch() {
        let (svc, _, sp_repo, audit_repo) = make_service();
        let tid = TenantId::new();
        let actor = uuid::Uuid::nil();

        let sp = svc
            .provision_service_principal(
                ServicePrincipalProvisionRequest {
                    tenant_id: tid,
                    display_name: "Bot".into(),
                    automation_id: None,
                    identity_type: IdentityType::Automation,
                    governance_profile: GovernanceProfile::Tool,
                    owner_id: None,
                },
                actor,
            )
            .await
            .unwrap();

        svc.deactivate_service_principal(tid, sp.id, actor)
            .await
            .unwrap();

        let sps = sp_repo.sps.lock().unwrap();
        assert!(!sps[0].is_active);

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(
            events.last().unwrap().event_type,
            "service_principal.deactivated"
        );
    }

    #[tokio::test]
    async fn deactivate_sp_wrong_tenant_forbidden() {
        let (svc, _, _, _) = make_service();
        let tid = TenantId::new();
        let other_tid = TenantId::new();
        let actor = uuid::Uuid::nil();

        let sp = svc
            .provision_service_principal(
                ServicePrincipalProvisionRequest {
                    tenant_id: tid,
                    display_name: "Bot".into(),
                    automation_id: None,
                    identity_type: IdentityType::Automation,
                    governance_profile: GovernanceProfile::Tool,
                    owner_id: None,
                },
                actor,
            )
            .await
            .unwrap();

        let result = svc
            .deactivate_service_principal(other_tid, sp.id, actor)
            .await;
        assert!(matches!(result, Err(PrismError::Forbidden { .. })));
    }

    #[tokio::test]
    async fn deactivate_nonexistent_sp_not_found() {
        let (svc, _, _, _) = make_service();
        let result = svc
            .deactivate_service_principal(
                TenantId::new(),
                ServicePrincipalId::new(),
                uuid::Uuid::nil(),
            )
            .await;
        assert!(matches!(result, Err(PrismError::NotFound { .. })));
    }
}
