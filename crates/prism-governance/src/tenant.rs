//! Tenant onboarding and management (SR_GOV_01, FOUND S 1.2).
//!
//! The `TenantService` handles tenant creation with validation, audit trail
//! integration, and duplicate detection. This is the single entry point for
//! all tenant lifecycle operations.

use std::sync::Arc;

use chrono::Utc;
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::TenantRepository;
use prism_core::types::*;

/// Tenant governance service.
///
/// Composes `TenantRepository` + `AuditLogger` following the
/// Repository + Service + AuditLogger pattern.
///
/// Implements: SR_GOV_01 (tenant onboarding)
#[derive(Clone)]
pub struct TenantService {
    repo: Arc<dyn TenantRepository>,
    audit: AuditLogger,
}

impl TenantService {
    pub fn new(repo: Arc<dyn TenantRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Onboard a new tenant.
    ///
    /// 1. Validate the request (name not empty, valid entity type).
    /// 2. If `parent_tenant_id` is set, verify the parent exists.
    /// 3. Create the tenant via the repository.
    /// 4. Write an audit event.
    /// 5. Return the onboarding result.
    ///
    /// Implements: SR_GOV_01
    ///
    /// Business exceptions:
    /// - BE-01: duplicate tenant name -> `PrismError::Conflict`
    /// - BE-02: invalid compliance profile -> `PrismError::Validation`
    /// - BE-03: caller lacks platform_admin role -> handled at API layer
    pub async fn onboard(
        &self,
        request: TenantOnboardingRequest,
        actor_id: uuid::Uuid,
    ) -> Result<TenantOnboardingResult, PrismError> {
        // Validate: name must not be empty (SR_GOV_01 input validation)
        let name = request.name.trim().to_string();
        if name.is_empty() {
            return Err(PrismError::Validation {
                reason: "tenant name must not be empty".into(),
            });
        }

        // Validate: at least one compliance profile required
        if request.compliance_profiles.is_empty() {
            return Err(PrismError::Validation {
                reason: "at least one compliance profile is required".into(),
            });
        }

        // Validate: parent exists if specified
        if let Some(parent_id) = request.parent_tenant_id {
            let parent = self.repo.get_by_id(parent_id).await?;
            if parent.is_none() {
                return Err(PrismError::NotFound {
                    entity_type: "Tenant",
                    id: parent_id.into_uuid(),
                });
            }
        }

        let now = Utc::now();
        let tenant = Tenant {
            id: TenantId::new(),
            name,
            legal_entity_type: request.legal_entity_type,
            parent_tenant_id: request.parent_tenant_id,
            compliance_profiles: request.compliance_profiles,
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        // Create tenant (SR_DM_01 PG path -- Neo4j deferred to Week 2)
        self.repo.create(&tenant).await?;

        info!(tenant_id = %tenant.id, name = %tenant.name, "tenant onboarded");

        // Write audit event (SR_GOV_47)
        self.audit
            .log(AuditEventInput {
                tenant_id: tenant.id,
                event_type: "tenant.created".into(),
                actor_id,
                actor_type: ActorType::Human,
                target_id: Some(tenant.id.into_uuid()),
                target_type: Some("Tenant".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "legal_entity_type": tenant.legal_entity_type,
                    "compliance_profiles": tenant.compliance_profiles,
                    "parent_tenant_id": tenant.parent_tenant_id,
                }),
            })
            .await?;

        Ok(TenantOnboardingResult {
            tenant_id: tenant.id,
            is_active: true,
            created_at: tenant.created_at,
        })
    }

    /// Retrieve a tenant by ID.
    pub async fn get(&self, id: TenantId) -> Result<Tenant, PrismError> {
        self.repo
            .get_by_id(id)
            .await?
            .ok_or(PrismError::NotFound {
                entity_type: "Tenant",
                id: id.into_uuid(),
            })
    }

    /// List child tenants of a parent.
    pub async fn list_children(&self, parent_id: TenantId) -> Result<Vec<Tenant>, PrismError> {
        self.repo.list_by_parent(parent_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use prism_core::repository::AuditEventRepository;
    use std::sync::Mutex;

    // -- Mock TenantRepository ------------------------------------------------

    struct MockTenantRepo {
        tenants: Mutex<Vec<Tenant>>,
    }

    impl MockTenantRepo {
        fn new() -> Self {
            Self {
                tenants: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl TenantRepository for MockTenantRepo {
        async fn create(&self, tenant: &Tenant) -> Result<(), PrismError> {
            let mut tenants = self.tenants.lock().unwrap();
            if tenants.iter().any(|t| t.name == tenant.name) {
                return Err(PrismError::Conflict {
                    reason: format!("tenant name '{}' already exists", tenant.name),
                });
            }
            tenants.push(tenant.clone());
            Ok(())
        }

        async fn get_by_id(&self, id: TenantId) -> Result<Option<Tenant>, PrismError> {
            let tenants = self.tenants.lock().unwrap();
            Ok(tenants.iter().find(|t| t.id == id).cloned())
        }

        async fn update(&self, tenant: &Tenant) -> Result<(), PrismError> {
            let mut tenants = self.tenants.lock().unwrap();
            if let Some(existing) = tenants.iter_mut().find(|t| t.id == tenant.id) {
                *existing = tenant.clone();
                Ok(())
            } else {
                Err(PrismError::NotFound {
                    entity_type: "Tenant",
                    id: tenant.id.into_uuid(),
                })
            }
        }

        async fn list_by_parent(&self, parent_id: TenantId) -> Result<Vec<Tenant>, PrismError> {
            let tenants = self.tenants.lock().unwrap();
            Ok(tenants
                .iter()
                .filter(|t| t.parent_tenant_id == Some(parent_id))
                .cloned()
                .collect())
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

    // -- Helpers --------------------------------------------------------------

    fn make_service() -> (TenantService, Arc<MockTenantRepo>, Arc<MockAuditRepo>) {
        let tenant_repo = Arc::new(MockTenantRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit_logger = AuditLogger::new(audit_repo.clone());
        let service = TenantService::new(tenant_repo.clone(), audit_logger);
        (service, tenant_repo, audit_repo)
    }

    fn onboard_request(name: &str) -> TenantOnboardingRequest {
        TenantOnboardingRequest {
            name: name.into(),
            legal_entity_type: LegalEntityType::Bank,
            parent_tenant_id: None,
            compliance_profiles: vec![ComplianceProfile::General],
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn onboard_creates_tenant_and_audit_event() {
        let (service, tenant_repo, audit_repo) = make_service();
        let actor = uuid::Uuid::nil();

        let result = service
            .onboard(onboard_request("Meridian Bank"), actor)
            .await
            .unwrap();

        assert!(result.is_active);

        // Tenant persisted
        let tenants = tenant_repo.tenants.lock().unwrap();
        assert_eq!(tenants.len(), 1);
        assert_eq!(tenants[0].name, "Meridian Bank");

        // Audit event written
        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "tenant.created");
    }

    #[tokio::test]
    async fn onboard_rejects_empty_name() {
        let (service, _, _) = make_service();
        let result = service.onboard(onboard_request(""), uuid::Uuid::nil()).await;
        assert!(matches!(result, Err(PrismError::Validation { .. })));
    }

    #[tokio::test]
    async fn onboard_rejects_whitespace_name() {
        let (service, _, _) = make_service();
        let result = service
            .onboard(onboard_request("   "), uuid::Uuid::nil())
            .await;
        assert!(matches!(result, Err(PrismError::Validation { .. })));
    }

    #[tokio::test]
    async fn onboard_rejects_empty_compliance_profiles() {
        let (service, _, _) = make_service();
        let mut req = onboard_request("Test");
        req.compliance_profiles = vec![];
        let result = service.onboard(req, uuid::Uuid::nil()).await;
        assert!(matches!(result, Err(PrismError::Validation { .. })));
    }

    #[tokio::test]
    async fn onboard_rejects_nonexistent_parent() {
        let (service, _, _) = make_service();
        let mut req = onboard_request("Child Tenant");
        req.parent_tenant_id = Some(TenantId::new());
        let result = service.onboard(req, uuid::Uuid::nil()).await;
        assert!(matches!(result, Err(PrismError::NotFound { .. })));
    }

    #[tokio::test]
    async fn onboard_with_valid_parent() {
        let (service, _, _) = make_service();
        let actor = uuid::Uuid::nil();

        // Create parent
        let parent_result = service
            .onboard(onboard_request("Meridian Holdings"), actor)
            .await
            .unwrap();

        // Create child
        let mut child_req = onboard_request("Meridian Bank");
        child_req.parent_tenant_id = Some(parent_result.tenant_id);
        let child_result = service.onboard(child_req, actor).await.unwrap();
        assert!(child_result.is_active);
    }

    #[tokio::test]
    async fn onboard_duplicate_name_is_conflict() {
        let (service, _, _) = make_service();
        let actor = uuid::Uuid::nil();

        service
            .onboard(onboard_request("Meridian Bank"), actor)
            .await
            .unwrap();

        let result = service
            .onboard(onboard_request("Meridian Bank"), actor)
            .await;
        assert!(matches!(result, Err(PrismError::Conflict { .. })));
    }

    #[tokio::test]
    async fn get_returns_onboarded_tenant() {
        let (service, _, _) = make_service();
        let result = service
            .onboard(onboard_request("Test Tenant"), uuid::Uuid::nil())
            .await
            .unwrap();

        let tenant = service.get(result.tenant_id).await.unwrap();
        assert_eq!(tenant.name, "Test Tenant");
    }

    #[tokio::test]
    async fn get_nonexistent_returns_not_found() {
        let (service, _, _) = make_service();
        let result = service.get(TenantId::new()).await;
        assert!(matches!(result, Err(PrismError::NotFound { .. })));
    }

    #[tokio::test]
    async fn list_children_returns_child_tenants() {
        let (service, _, _) = make_service();
        let actor = uuid::Uuid::nil();

        let parent = service
            .onboard(onboard_request("Holdings"), actor)
            .await
            .unwrap();

        let mut child_req = onboard_request("Bank");
        child_req.parent_tenant_id = Some(parent.tenant_id);
        service.onboard(child_req, actor).await.unwrap();

        let mut child_req2 = onboard_request("Insurance");
        child_req2.parent_tenant_id = Some(parent.tenant_id);
        service.onboard(child_req2, actor).await.unwrap();

        let children = service.list_children(parent.tenant_id).await.unwrap();
        assert_eq!(children.len(), 2);
    }
}
