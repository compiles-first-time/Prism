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

// -- Ruleset Version (SR_GOV_19) -------------------------------------------

/// A versioned snapshot of a tenant's governance ruleset.
/// Each publication creates a new version; only one is active at a time.
/// Implements: SR_GOV_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesetVersion {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    /// The rules in this version.
    pub rules: Vec<super::requests::GovernanceRule>,
    /// Human-readable description of what changed.
    pub change_description: String,
    /// Whether this version is currently the active one.
    pub is_active: bool,
    /// Version number (monotonically increasing per tenant).
    pub version_number: u64,
    pub created_at: DateTime<Utc>,
}

// -- Alert History (SR_GOV_67) ---------------------------------------------

/// A record of a dispatched alert for acknowledgement tracking.
/// Implements: SR_GOV_67
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertHistoryEntry {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub severity: Severity,
    pub source: String,
    pub message: String,
    pub channels: Vec<AlertChannel>,
    pub recipients: Vec<String>,
    pub acknowledged: bool,
    pub created_at: DateTime<Utc>,
}

// -- Feature Flag (SR_GOV_68) -----------------------------------------------

/// A tenant-scoped feature flag controlled by governance.
/// Implements: SR_GOV_68
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    /// Unique human-readable flag identifier (e.g., "enable_ai_suggestions").
    pub flag_id: String,
    pub value: bool,
    pub approved_by: UserId,
    /// Optional plan tier that must be active for this flag to be eligible.
    pub plan_tier_required: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- Admin Action (SR_GOV_69) -----------------------------------------------

/// A recorded admin action that may be undone within a time window.
/// Implements: SR_GOV_69
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminAction {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub action_type: String,
    pub payload: serde_json::Value,
    pub performed_by: UserId,
    pub is_undoable: bool,
    /// Security-critical actions cannot be undone.
    pub is_security_critical: bool,
    pub performed_at: DateTime<Utc>,
    /// How many seconds after performed_at the action can be undone.
    pub undo_window_seconds: u64,
    pub is_undone: bool,
}

// -- Connection Consent (SR_GOV_70) -----------------------------------------

/// A recorded consent for an external system connection.
/// Implements: SR_GOV_70
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConsent {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub system_id: String,
    pub connection_type: String,
    pub scope: String,
    pub vendor_terms_acknowledged: bool,
    pub paywall_recorded: bool,
    pub authorized_by: UserId,
    pub created_at: DateTime<Utc>,
}

// -- CSA Rule (SR_GOV_23) ---------------------------------------------------

/// A Cross-System Aggregation rule that triggers when multiple data collections
/// are combined and matching attributes are present.
/// Implements: SR_GOV_23
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaRule {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub rule_expression: String,
    pub action: CsaAction,
    pub severity: Severity,
    pub version: u64,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// -- Break-Glass Activation (SR_GOV_29) -------------------------------------

/// A record of a break-glass emergency activation.
/// Requires two-person approval and mandatory post-incident review.
/// Implements: SR_GOV_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakGlassActivation {
    pub id: uuid::Uuid,
    pub assessment_id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub justification: String,
    pub approver_1: UserId,
    pub approver_2: UserId,
    pub duration_minutes: u64,
    pub activated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub review_id: uuid::Uuid,
    pub is_reviewed: bool,
}

// -- CSA Assessment Record (SR_GOV_30) --------------------------------------

/// A persisted CSA assessment record for graph and historical queries.
/// Implements: SR_GOV_30
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaAssessmentRecord {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub query_id: uuid::Uuid,
    pub data_collection_refs: Vec<String>,
    pub decision: super::enums::CsaDecision,
    pub applied_rules: Vec<String>,
    pub created_at: DateTime<Utc>,
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

// -- Approval Request Record (SR_GOV_41) ------------------------------------

/// A persisted approval request with ordered approver chain and SLA tracking.
/// Implements: SR_GOV_41
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestRecord {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub action: String,
    pub requested_by: UserId,
    pub payload: serde_json::Value,
    pub approvers: Vec<UserId>,
    pub current_index: usize,
    pub status: ApprovalStatus,
    pub sla_deadline: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

// -- Delegation (SR_GOV_44) ------------------------------------------------

/// An active delegation that re-routes approval authority from one person
/// to another within a defined scope and time window.
/// Implements: SR_GOV_44
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    pub id: uuid::Uuid,
    pub tenant_id: super::identifiers::TenantId,
    pub from_person: super::identifiers::UserId,
    pub to_person: super::identifiers::UserId,
    pub scope: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub is_active: bool,
}

// -- Connection Record (SR_CONN_01) -----------------------------------------

/// A registered external system connection governed by PRISM.
/// Tracks the full lifecycle from request through decommission.
/// Implements: SR_CONN_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRecord {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub system_id: String,
    pub connection_type: String,
    pub scope: String,
    pub status: super::enums::ConnectionState,
    pub credential_ref: Option<String>,
    pub justification: Option<String>,
    pub requested_by: UserId,
    pub first_pull_at: Option<DateTime<Utc>>,
    pub kpi_error_rate: Option<f64>,
    pub kpi_avg_latency_ms: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// -- Component Info (SR_GOV_78) ---------------------------------------------

/// Metadata about a registered component for preflight checks.
/// Implements: SR_GOV_78
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentInfo {
    pub component_id: String,
    pub is_active: bool,
    pub is_deprecated: bool,
    pub required_role: Option<String>,
    pub credential_required: bool,
    pub has_credential: bool,
}
