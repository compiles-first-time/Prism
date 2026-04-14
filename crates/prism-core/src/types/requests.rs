//! Request and response types for PRISM service operations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::*;
use super::identifiers::*;

// -- Rule evaluation requests (SR_GOV_16, SR_GOV_17) -------------------------

/// A governance rule that can be evaluated against an action.
/// Rules are stored per-tenant and classified as ENFORCE or ADVISE.
/// Implements: SR_GOV_16, SR_GOV_17
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceRule {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub name: String,
    pub rule_class: RuleClass,
    /// The action pattern this rule matches (e.g., "automation.activate", "data.export").
    pub action_pattern: String,
    /// JSONLogic-style condition. When evaluated as true, the rule fires.
    pub condition: serde_json::Value,
    /// For ADVISE rules: the advisory message shown to the user.
    pub advisory_message: Option<String>,
    pub is_active: bool,
}

/// Request to evaluate governance rules against a candidate action.
/// Implements: SR_GOV_16, SR_GOV_17
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEvaluationRequest {
    pub tenant_id: TenantId,
    /// The action being performed (e.g., "automation.activate").
    pub action: String,
    /// The principal performing the action.
    pub subject_principal: uuid::Uuid,
    /// Attributes of the action context for rule condition evaluation.
    pub attributes: serde_json::Value,
    /// Which rule classes to evaluate.
    pub rule_classes: Vec<RuleClass>,
}

/// Result of evaluating ENFORCE rules.
/// Implements: SR_GOV_16
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforceEvaluationResult {
    pub decision: EnforceDecision,
    /// Rules that matched and contributed to the decision.
    pub matched_rules: Vec<String>,
    /// Reason for denial (if denied).
    pub reason: Option<String>,
}

/// Result of evaluating ADVISE rules.
/// Implements: SR_GOV_17
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdviseEvaluationResult {
    pub decision: AdviseDecision,
    /// Advisory messages from matched rules.
    pub advisory_messages: Vec<String>,
    /// Whether the caller needs to provide justification to proceed.
    pub requires_justification: bool,
    /// Rules that matched.
    pub matched_rules: Vec<String>,
}

// -- Tenant requests (SR_GOV_01) --------------------------------------------

/// Input for tenant onboarding.
/// Implements: SR_GOV_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantOnboardingRequest {
    pub name: String,
    pub legal_entity_type: LegalEntityType,
    pub parent_tenant_id: Option<TenantId>,
    pub compliance_profiles: Vec<ComplianceProfile>,
}

/// Result of tenant onboarding.
/// Implements: SR_GOV_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantOnboardingResult {
    pub tenant_id: TenantId,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// -- Service Principal requests (FOUND S 1.3.1) -----------------------------

/// Input for provisioning a service principal.
/// Implements: FOUND S 1.3.1, SR_DM_20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePrincipalProvisionRequest {
    pub tenant_id: TenantId,
    pub display_name: String,
    pub automation_id: Option<AutomationId>,
    pub identity_type: IdentityType,
    pub governance_profile: GovernanceProfile,
    pub owner_id: Option<UserId>,
}

// -- Audit requests (SR_GOV_47, SR_GOV_48, SR_GOV_49) ----------------------

/// Input for writing an audit event. The hash chain fields are computed
/// by the AuditLogger, not supplied by the caller.
/// Implements: SR_GOV_47
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEventInput {
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
}

/// Result of appending an audit event.
/// Implements: SR_GOV_47
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditCaptureResult {
    pub event_id: AuditEventId,
    pub chain_position: i64,
    pub event_hash: String,
}

/// Request to query audit events.
/// Implements: SR_GOV_49
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditQueryRequest {
    pub tenant_id: TenantId,
    pub event_type: Option<String>,
    pub actor_id: Option<uuid::Uuid>,
    pub target_id: Option<uuid::Uuid>,
    pub severity: Option<Severity>,
    pub from_time: Option<DateTime<Utc>>,
    pub to_time: Option<DateTime<Utc>>,
    pub page_size: i64,
    pub page_token: Option<i64>,
}

/// Result of an audit query.
/// Implements: SR_GOV_49
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditQueryResult {
    pub events: Vec<super::entities::AuditEvent>,
    pub next_page_token: Option<i64>,
    pub total_count: i64,
}

/// Result of chain verification.
/// Implements: SR_GOV_48
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerificationResult {
    pub is_valid: bool,
    pub verified_count: u32,
    pub mismatch_at: Option<i64>,
    pub anchor_hash: String,
}

// -- Audit export requests (SR_GOV_50) ----------------------------------------

/// Request to export an audit slice for regulatory or examiner review.
/// The export is signed and includes a chain proof for integrity verification.
/// Implements: SR_GOV_50
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditExportRequest {
    pub tenant_id: TenantId,
    pub time_range: TimeRange,
    pub format: ExportFormat,
}

/// Time range for audit queries and exports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

/// Cryptographic chain proof embedded in an audit export.
/// Allows a verifier to confirm the exported segment is contiguous
/// and anchored to the live chain.
/// Implements: SR_GOV_50
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainProof {
    /// Hash of the first event in the exported segment.
    pub anchor_hash: String,
    /// Hash of the last event in the exported segment.
    pub tip_hash: String,
    /// Number of events in the proven segment.
    pub segment_length: u64,
    /// Chain positions covered: [start, end] inclusive.
    pub position_range: (i64, i64),
}

/// Result of an audit export operation.
/// Contains the serialized export payload, a cryptographic signature,
/// and the chain proof linking this slice to the live chain.
/// Implements: SR_GOV_50
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditExportResult {
    /// Serialized export payload (format determined by request).
    pub export_payload: Vec<u8>,
    /// Hex-encoded HMAC-SHA256 signature of the export payload.
    pub signature: String,
    /// Chain proof linking this export to the tenant's audit chain.
    pub chain_proof: ChainProof,
    /// Number of events included in the export.
    pub event_count: u64,
}

// -- Tamper response requests (SR_GOV_51) -------------------------------------

/// Input triggered by SR_GOV_48 when chain verification detects tampering.
/// Implements: SR_GOV_51
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TamperResponseInput {
    pub tenant_id: TenantId,
    /// Chain position where the mismatch was detected.
    pub mismatch_at: i64,
    /// The anchor hash of the verified segment.
    pub anchor_hash: String,
}

/// Result of the tamper response workflow.
/// Implements: SR_GOV_51
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TamperResponseResult {
    /// Whether tenant writes have been frozen.
    pub freeze_active: bool,
    /// Incident ticket identifier for the security investigation.
    pub incident_id: String,
}

// -- Visibility compartment requests (SR_GOV_31, SR_GOV_32, SR_GOV_33) -------

/// Request to create a visibility compartment.
/// Implements: SR_GOV_31
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentCreateRequest {
    pub tenant_id: TenantId,
    pub name: String,
    pub classification_level: ClassificationLevel,
    /// Initial member persons (added at creation time).
    pub member_persons: Vec<UserId>,
    /// Initial member roles (added at creation time).
    pub member_roles: Vec<RoleId>,
    pub purpose: String,
    /// When true, overrides "visibility flows up" -- even executives
    /// cannot see data without explicit membership.
    pub criminal_penalty_isolation: bool,
}

/// Result of compartment creation.
/// Implements: SR_GOV_31
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentCreateResult {
    pub compartment_id: CompartmentId,
    pub member_count: usize,
    pub created_at: DateTime<Utc>,
}

/// Request to add a person or role to a compartment.
/// Exactly one of person_id or role_id must be provided.
/// Implements: SR_GOV_32
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentMembershipAddRequest {
    pub tenant_id: TenantId,
    pub compartment_id: CompartmentId,
    pub person_id: Option<UserId>,
    pub role_id: Option<RoleId>,
}

/// Result of a membership operation.
/// Implements: SR_GOV_32
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentMembershipResult {
    pub compartment_id: CompartmentId,
    pub added: bool,
}

/// Request to check whether a principal can access a compartment-bound resource.
/// The principal must be a member of ALL compartments the resource belongs to.
/// Implements: SR_GOV_33
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentAccessCheckRequest {
    pub tenant_id: TenantId,
    /// The principal (person) requesting access.
    pub principal_id: UserId,
    /// The roles currently held by the principal.
    pub principal_roles: Vec<RoleId>,
    /// The compartments the target resource belongs to.
    pub resource_compartments: Vec<CompartmentId>,
}

/// Result of a compartment access check.
/// Implements: SR_GOV_33
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentAccessCheckResult {
    pub decision: AccessDecision,
    /// Compartments that denied access (empty if allowed).
    pub denied_compartments: Vec<CompartmentId>,
    pub reason: Option<String>,
}

// -- Lifecycle requests (FOUND S 1.5.1) -------------------------------------

/// A validated state transition produced by the lifecycle state machine.
/// Implements: FOUND S 1.5.1, SR_DM_11
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: LifecycleState,
    pub to: LifecycleState,
    pub reason: String,
    pub transitioned_at: DateTime<Utc>,
}
