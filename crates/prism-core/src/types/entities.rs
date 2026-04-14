//! Domain entity structs shared across all PRISM crates.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::*;
use super::identifiers::*;

// -- Tenant (FOUND S 1.2) --------------------------------------------------

/// A legal entity with isolated governance, data, and identity boundaries.
/// Implements: FOUND S 1.2, SR_GOV_01, SR_DM_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub legal_entity_type: LegalEntityType,
    pub parent_tenant_id: Option<TenantId>,
    pub compliance_profiles: Vec<ComplianceProfile>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- User -------------------------------------------------------------------

/// A human user synced from IdP or platform-managed identity.
/// Implements: SR_GOV_10, SR_DM_02
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub tenant_id: TenantId,
    pub idp_id: Option<String>,
    pub email: String,
    pub display_name: String,
    pub role_ids: Vec<RoleId>,
    pub primary_reporting_line: Option<UserId>,
    pub secondary_reporting_line: Option<UserId>,
    pub department: Option<String>,
    pub business_unit: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- Service Principal (FOUND S 1.3.1) --------------------------------------

/// First-class automation identity, distinct from its human creator.
/// Implements: FOUND S 1.3.1, SR_DM_20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePrincipal {
    pub id: ServicePrincipalId,
    pub tenant_id: TenantId,
    pub automation_id: Option<AutomationId>,
    pub display_name: String,
    pub identity_type: IdentityType,
    pub governance_profile: GovernanceProfile,
    pub permissions: serde_json::Value,
    pub credential_id: Option<CredentialId>,
    pub owner_id: Option<UserId>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- Automation -------------------------------------------------------------

/// A registered automation governed by PRISM.
/// PRISM does not execute automations -- it governs them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Automation {
    pub id: AutomationId,
    pub tenant_id: TenantId,
    pub service_principal_id: Option<ServicePrincipalId>,
    pub name: String,
    pub description: Option<String>,
    pub lifecycle_state: LifecycleState,
    pub compliance_profiles: Vec<ComplianceProfile>,
    pub owner_id: UserId,
    pub platform_type: Option<String>,
    pub external_ref: Option<String>,
    pub blast_radius_tier: BlastRadiusTier,
    pub environment: Environment,
    pub sunset_date: Option<DateTime<Utc>>,
    pub next_review_date: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- Audit Event (D-22) -----------------------------------------------------

/// An append-only, cryptographically chained audit event.
/// Implements: SR_DM_05, SR_GOV_47, D-22
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: AuditEventId,
    pub tenant_id: TenantId,
    pub event_type: String,
    pub actor_id: uuid::Uuid,
    pub actor_type: ActorType,
    pub target_id: Option<uuid::Uuid>,
    pub target_type: Option<String>,
    pub severity: Severity,
    pub source_layer: SourceLayer,
    pub governance_authority: Option<String>,
    pub payload: serde_json::Value,
    pub prev_event_hash: Option<String>,
    pub event_hash: String,
    pub chain_position: i64,
    pub created_at: DateTime<Utc>,
}

// -- Visibility Compartment (SR_GOV_31) ------------------------------------

/// A visibility compartment that isolates data by classification level.
/// Criminal-penalty compartments override the default "visibility flows up"
/// model -- even executives cannot see data without explicit membership.
/// Implements: SR_GOV_31, GAP-77
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compartment {
    pub id: CompartmentId,
    pub tenant_id: TenantId,
    pub name: String,
    pub classification_level: ClassificationLevel,
    pub purpose: String,
    pub criminal_penalty_isolation: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A membership record linking a person or role to a compartment.
/// Implements: SR_GOV_32
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentMembership {
    pub compartment_id: CompartmentId,
    pub tenant_id: TenantId,
    pub person_id: Option<UserId>,
    pub role_id: Option<RoleId>,
    pub added_at: DateTime<Utc>,
}

// -- Approval Chain ---------------------------------------------------------

/// An approval chain instance computed by the LCA algorithm.
/// Implements: SR_GOV_41, FOUND S 1.4.1
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalChain {
    pub id: ApprovalChainId,
    pub automation_id: AutomationId,
    pub scope: ApprovalScope,
    pub status: ApprovalStatus,
    pub requested_by: UserId,
    pub approvers: serde_json::Value,
    pub conditions: Option<serde_json::Value>,
    pub decided_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
