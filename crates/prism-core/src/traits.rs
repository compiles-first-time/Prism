//! Core trait definitions implemented by domain entities across crates.

use crate::types::TenantId;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Entities that produce audit trail entries.
pub trait Auditable {
    /// The type of audit event this entity produces.
    fn event_type(&self) -> &'static str;

    /// The ID of the actor performing the auditable action.
    fn actor_id(&self) -> Uuid;

    /// The tenant context for this audit event.
    fn tenant_id(&self) -> TenantId;
}

/// Entities subject to governance approval workflows.
pub trait Governable {
    /// The governance authority level required for approval.
    fn required_approval_scope(&self) -> crate::types::ApprovalScope;

    /// The compliance profiles applicable to this entity.
    fn compliance_profiles(&self) -> &[crate::types::ComplianceProfile];

    /// Whether this entity requires committee review.
    fn requires_committee_review(&self) -> bool;
}

/// Entities with a stable unique identifier.
pub trait Identifiable {
    /// Returns the entity's unique identifier.
    fn id(&self) -> Uuid;

    /// Returns the entity type name for logging and error messages.
    fn entity_type(&self) -> &'static str;
}

/// Entities scoped to a specific tenant.
pub trait TenantScoped {
    /// Returns the tenant this entity belongs to.
    fn tenant_id(&self) -> TenantId;
}

/// Entities with creation and modification timestamps.
pub trait Timestamped {
    fn created_at(&self) -> DateTime<Utc>;
    fn updated_at(&self) -> DateTime<Utc>;
}
