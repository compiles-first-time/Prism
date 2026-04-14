//! Tenant isolation context (REUSABLE_TenantFilter, SR_DM_27).
//!
//! Every data-path operation must be scoped to a tenant. The `TenantContext`
//! carries the authenticated tenant_id and is threaded through service calls
//! to ensure queries are always filtered.
//!
//! For MVP this is a simple wrapper. Full query-rewrite enforcement
//! (Cypher + PG row-level security) is deferred to Week 2+.

use crate::error::PrismError;
use crate::types::TenantId;

/// Authenticated tenant context carried through every request.
///
/// Created at the API boundary from the authenticated JWT claims.
/// Passed to all service and repository methods that touch tenant-scoped data.
///
/// Implements: SR_DM_27 (single-tenant stub)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantContext {
    tenant_id: TenantId,
}

impl TenantContext {
    /// Create a new tenant context. In production this is constructed
    /// by the auth middleware after validating the JWT.
    pub fn new(tenant_id: TenantId) -> Self {
        Self { tenant_id }
    }

    /// The tenant this request is scoped to.
    pub fn tenant_id(&self) -> TenantId {
        self.tenant_id
    }

    /// Validate that an entity belongs to this tenant.
    ///
    /// Returns `Err(Forbidden)` if the entity's tenant does not match.
    /// This is the programmatic enforcement of tenant isolation at the
    /// service layer.
    pub fn enforce(&self, entity_tenant_id: TenantId) -> Result<(), PrismError> {
        if self.tenant_id == entity_tenant_id {
            Ok(())
        } else {
            Err(PrismError::Forbidden {
                reason: format!(
                    "tenant isolation violation: request scoped to {} but entity belongs to {}",
                    self.tenant_id, entity_tenant_id
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_same_tenant_passes() {
        let tid = TenantId::new();
        let ctx = TenantContext::new(tid);
        assert!(ctx.enforce(tid).is_ok());
    }

    #[test]
    fn enforce_different_tenant_fails() {
        let ctx = TenantContext::new(TenantId::new());
        let other = TenantId::new();
        assert!(matches!(ctx.enforce(other), Err(PrismError::Forbidden { .. })));
    }

    #[test]
    fn tenant_id_accessor() {
        let tid = TenantId::new();
        let ctx = TenantContext::new(tid);
        assert_eq!(ctx.tenant_id(), tid);
    }
}
