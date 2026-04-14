//! Async repository trait definitions for persistence abstraction.
//!
//! Each domain crate provides a concrete implementation (typically PostgreSQL).
//! These traits enable testing without a database.

use async_trait::async_trait;

use crate::error::PrismError;
use crate::types::*;

// -- Tenant Repository (SR_DM_01) -------------------------------------------

/// Persistence operations for tenants.
/// Implements: SR_DM_01
#[async_trait]
pub trait TenantRepository: Send + Sync {
    /// Create a new tenant.
    /// Implements: SR_DM_01
    async fn create(&self, tenant: &Tenant) -> Result<(), PrismError>;

    /// Retrieve a tenant by ID.
    async fn get_by_id(&self, id: TenantId) -> Result<Option<Tenant>, PrismError>;

    /// Update an existing tenant.
    async fn update(&self, tenant: &Tenant) -> Result<(), PrismError>;

    /// List child tenants of a parent.
    async fn list_by_parent(&self, parent_id: TenantId) -> Result<Vec<Tenant>, PrismError>;
}

// -- User Repository (SR_DM_02) ---------------------------------------------

/// Persistence operations for users.
/// Implements: SR_DM_02
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Create a new user.
    /// Implements: SR_DM_02, SR_GOV_10
    async fn create(&self, user: &User) -> Result<(), PrismError>;

    /// Retrieve a user by ID.
    async fn get_by_id(&self, tenant_id: TenantId, id: UserId) -> Result<Option<User>, PrismError>;

    /// Find a user by email within a tenant.
    async fn get_by_email(
        &self,
        tenant_id: TenantId,
        email: &str,
    ) -> Result<Option<User>, PrismError>;
}

// -- Service Principal Repository (FOUND S 1.3.1) --------------------------

/// Persistence operations for service principals.
/// Implements: FOUND S 1.3.1, SR_DM_20
#[async_trait]
pub trait ServicePrincipalRepository: Send + Sync {
    /// Create a new service principal.
    /// Implements: FOUND S 1.3.1
    async fn create(&self, sp: &ServicePrincipal) -> Result<(), PrismError>;

    /// Retrieve a service principal by ID.
    async fn get_by_id(
        &self,
        id: ServicePrincipalId,
    ) -> Result<Option<ServicePrincipal>, PrismError>;

    /// List service principals for a tenant.
    async fn list_by_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<ServicePrincipal>, PrismError>;

    /// Deactivate a service principal (set is_active = false).
    /// Implements: SR_GOV_13
    async fn deactivate(&self, id: ServicePrincipalId) -> Result<(), PrismError>;
}

// -- Audit Event Repository (SR_DM_05) --------------------------------------

/// Persistence operations for the append-only audit event store.
/// Implements: SR_DM_05
#[async_trait]
pub trait AuditEventRepository: Send + Sync {
    /// Append an audit event. This is the only write operation -- no updates or deletes.
    /// Implements: SR_DM_05, SR_GOV_47
    async fn append(&self, event: &AuditEvent) -> Result<(), PrismError>;

    /// Get the most recent event for a tenant (the chain head).
    /// Used by the MerkleChainHasher to compute the next hash.
    async fn get_chain_head(&self, tenant_id: TenantId) -> Result<Option<AuditEvent>, PrismError>;

    /// Query audit events with filters and pagination.
    /// Implements: SR_GOV_49
    async fn query(&self, request: &AuditQueryRequest) -> Result<AuditQueryResult, PrismError>;

    /// Get a contiguous chain segment for verification.
    /// Returns events ordered by chain_position descending, limited to `depth`.
    /// Implements: SR_GOV_48
    async fn get_chain_segment(
        &self,
        tenant_id: TenantId,
        depth: u32,
    ) -> Result<Vec<AuditEvent>, PrismError>;
}
