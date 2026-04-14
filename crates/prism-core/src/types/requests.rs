//! Request and response types for PRISM service operations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::enums::*;
use super::identifiers::*;

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
