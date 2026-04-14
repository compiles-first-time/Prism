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

// -- Override justification (SR_GOV_18) ---------------------------------------

/// Request to capture an ADVISE override justification.
/// Submitted after SR_GOV_17 returns `requires_justification = true`.
/// Implements: SR_GOV_18
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideJustificationRequest {
    pub tenant_id: TenantId,
    /// The person providing the justification.
    pub person_id: UserId,
    /// Opaque action identifier linking this justification to the action being overridden.
    pub action_id: uuid::Uuid,
    /// The rule that was overridden.
    pub rule_id: uuid::Uuid,
    /// Free-text justification. Must pass relevance validation.
    pub justification_text: String,
    /// Optional category for structured classification.
    pub category: Option<String>,
}

/// Result of a justification capture.
/// Implements: SR_GOV_18
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideJustificationResult {
    /// Whether the justification was accepted.
    pub accepted: bool,
    /// If rejected, the specific reason and guidance.
    pub rejection_reason: Option<String>,
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

// -- Compartment revocation requests (SR_GOV_34) ----------------------------

/// Request to revoke compartment membership for a person or role.
/// Exactly one of person_id or role_id must be provided.
/// Implements: SR_GOV_34
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentMembershipRemoveRequest {
    pub tenant_id: TenantId,
    pub compartment_id: CompartmentId,
    pub person_id: Option<UserId>,
    pub role_id: Option<RoleId>,
}

/// Result of a membership revocation.
/// Implements: SR_GOV_34
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentMembershipRemoveResult {
    pub compartment_id: CompartmentId,
    /// Whether a membership was actually removed.
    pub removed: bool,
    /// Number of sessions terminated due to the revocation.
    pub sessions_terminated: u64,
}

// -- Alert routing requests (SR_GOV_67) --------------------------------------

/// An alert event to be routed via the severity matrix.
/// Implements: SR_GOV_67
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub tenant_id: TenantId,
    pub severity: Severity,
    /// Source system or SR that raised the alert.
    pub source: String,
    pub message: String,
    /// Attribution: who or what caused the condition.
    pub attribution: Option<String>,
}

/// Result of alert dispatch via the severity matrix.
/// Implements: SR_GOV_67
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertDispatchResult {
    /// Recipients who received the alert.
    pub recipients: Vec<String>,
    /// Dispatch identifiers for acknowledgement tracking.
    pub dispatch_ids: Vec<String>,
    /// Channels used for dispatch.
    pub channels_used: Vec<String>,
}

// -- Rule publication requests (SR_GOV_19) -----------------------------------

/// Request to publish a new version of a governance rule set.
/// Implements: SR_GOV_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulePublishRequest {
    pub tenant_id: TenantId,
    /// The rules to include in this version.
    pub rules: Vec<GovernanceRule>,
    /// Description of what changed in this version.
    pub change_description: String,
    /// Number of recent decisions to use for dry-run (default 100).
    pub dry_run_sample_size: Option<usize>,
}

/// Result of a rule publication attempt.
/// Implements: SR_GOV_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulePublishResult {
    /// The version ID assigned to this publication.
    pub version_id: uuid::Uuid,
    /// Dry-run report showing what would change.
    pub dry_run_report: DryRunReport,
    /// Whether the version was promoted to active.
    pub promoted: bool,
}

/// Dry-run report showing the impact of proposed rule changes.
/// Implements: SR_GOV_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunReport {
    /// Total decisions sampled.
    pub sample_size: usize,
    /// Decisions that would have changed under the new rules.
    pub decisions_changed: usize,
    /// Percentage of decisions affected.
    pub delta_percentage: f64,
    /// Whether the delta exceeds the promotion threshold (5%).
    pub exceeds_threshold: bool,
    /// Per-rule breakdown of changes.
    pub rule_deltas: Vec<RuleDelta>,
}

/// Per-rule impact in a dry-run report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleDelta {
    pub rule_name: String,
    pub new_denials: usize,
    pub new_allowances: usize,
}

// -- Rule conflict detection requests (SR_GOV_20) ----------------------------

/// Request to scan a ruleset for internal conflicts.
/// Implements: SR_GOV_20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictScanRequest {
    pub tenant_id: TenantId,
    /// The rules to scan for conflicts.
    pub rules: Vec<GovernanceRule>,
}

/// A conflict between two governance rules.
/// Implements: SR_GOV_20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConflict {
    pub rule_a: String,
    pub rule_b: String,
    pub conflict_type: ConflictType,
    pub description: String,
}

/// Report of all conflicts found in a ruleset.
/// Implements: SR_GOV_20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConflictReport {
    pub conflicts: Vec<RuleConflict>,
    /// Overall severity: HIGH if any contradiction found, LOW for subsumption only.
    pub severity: Severity,
    /// Whether this conflict report should block rule promotion.
    pub blocks_promotion: bool,
}

// -- Rule rollback requests (SR_GOV_21) --------------------------------------

/// Request to roll back to a prior ruleset version.
/// Implements: SR_GOV_21
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleRollbackRequest {
    pub tenant_id: TenantId,
    /// The version to roll back to.
    pub target_version_id: uuid::Uuid,
    /// Reason for the rollback.
    pub reason: String,
}

/// Result of a rule rollback.
/// Implements: SR_GOV_21
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleRollbackResult {
    /// The now-active version after rollback.
    pub active_version: uuid::Uuid,
    pub rollback_reason: String,
}

// -- Rule export requests (SR_GOV_22) ----------------------------------------

/// Request to export rules in effect at a given date.
/// Implements: SR_GOV_22
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExportRequest {
    pub tenant_id: TenantId,
    /// Export rules as they were on this date.
    pub as_of_date: DateTime<Utc>,
    pub format: ExportFormat,
}

/// Result of a rule export.
/// Implements: SR_GOV_22
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExportResult {
    /// Serialized export payload.
    pub export_payload: Vec<u8>,
    /// Hex-encoded signature of the export.
    pub signature: String,
    /// Number of rules included.
    pub rule_count: usize,
}

// -- Query analytics requests (SR_GOV_37-40) --------------------------------

/// A query analytics event to be captured.
/// Privacy-level determines which fields are retained.
/// Implements: SR_GOV_37
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAnalyticsEvent {
    pub tenant_id: TenantId,
    pub query_id: uuid::Uuid,
    /// Hashed representation of query type (preserves privacy).
    pub query_type_hash: String,
    pub complexity_tier: ComplexityTier,
    /// The model used to process the query.
    pub model_used: String,
    pub response_time_ms: u64,
    pub outcome: QueryOutcome,
    pub privacy_level: PrivacyLevel,
    /// The user who submitted the query (stripped at Anonymous level).
    pub user_id: Option<UserId>,
    /// The user's role (stripped at Anonymous level, retained at Role level).
    pub role: Option<String>,
    /// The user's department (stripped at Anonymous level, retained at Role level).
    pub department: Option<String>,
}

/// Result of capturing a query analytics event.
/// Implements: SR_GOV_37
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsCaptureResult {
    pub recorded: bool,
    /// Privacy level applied to the stored record.
    pub privacy_level_applied: PrivacyLevel,
}

/// Request to aggregate query analytics.
/// Implements: SR_GOV_38
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationRequest {
    pub tenant_id: TenantId,
    /// Aggregation period label (e.g., "2026-04-14T10:00:00Z/PT1H").
    pub period: String,
}

/// Result of an aggregation run.
/// Implements: SR_GOV_38
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    pub rows_processed: u64,
    pub aggregates_written: u64,
}

/// Request to check analytics access.
/// Implements: SR_GOV_39
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsAccessRequest {
    pub tenant_id: TenantId,
    /// The principal requesting access.
    pub principal_id: UserId,
    /// The principal's roles.
    pub principal_roles: Vec<String>,
    /// Scope of data being requested.
    pub requested_scope: AnalyticsScope,
    /// If individual scope, the subject user whose data is requested.
    pub requested_subject: Option<UserId>,
}

/// Result of an analytics access check.
/// Implements: SR_GOV_39
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsAccessResult {
    pub decision: AccessDecision,
    pub reason: Option<String>,
}

/// Request to export query analytics.
/// Implements: SR_GOV_40
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsExportRequest {
    pub tenant_id: TenantId,
    pub period: String,
    pub scope: AnalyticsScope,
    pub format: ExportFormat,
    /// The principal requesting the export (for access control).
    pub principal_id: UserId,
    pub principal_roles: Vec<String>,
}

/// Result of an analytics export.
/// Implements: SR_GOV_40
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsExportResult {
    pub export_payload: Vec<u8>,
    pub signature: String,
    pub event_count: u64,
}

// -- Criminal-penalty visibility override requests (SR_GOV_35) ---------------

/// Request to check criminal-penalty compartment visibility override.
/// For criminal-penalty compartments, denies ANY principal not explicitly listed
/// as a member, regardless of position in the org tree (including executives).
/// Implements: SR_GOV_35
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriminalPenaltyOverrideCheck {
    pub tenant_id: TenantId,
    pub compartment_id: CompartmentId,
    pub principal_id: UserId,
    pub principal_roles: Vec<RoleId>,
    /// Org-tree ancestors of the principal (e.g., manager, director, VP, ...).
    /// For criminal-penalty compartments, these are ignored -- only explicit membership counts.
    pub principal_chain: Vec<UserId>,
}

/// Result of a criminal-penalty visibility override check.
/// Implements: SR_GOV_35
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriminalPenaltyOverrideResult {
    pub decision: AccessDecision,
    pub reason: Option<String>,
}

// -- Compartment audit report requests (SR_GOV_36) ---------------------------

/// Request to generate a compartment audit report.
/// Implements: SR_GOV_36
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentAuditRequest {
    pub tenant_id: TenantId,
    pub compartment_id: CompartmentId,
    pub period: String,
}

/// Result of a compartment audit report generation.
/// Implements: SR_GOV_36
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentAuditResult {
    pub report_payload: Vec<u8>,
    pub signature: String,
    pub member_count: usize,
}

// -- Feature flag requests (SR_GOV_68) ---------------------------------------

/// Request to toggle a feature flag.
/// Implements: SR_GOV_68
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagToggleRequest {
    pub tenant_id: TenantId,
    pub flag_id: String,
    pub value: bool,
    pub approved_by: UserId,
}

/// Result of a feature flag operation.
/// Implements: SR_GOV_68
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagResult {
    pub active: bool,
}

// -- Admin undo requests (SR_GOV_69) -----------------------------------------

/// Request to undo a previously recorded admin action.
/// Implements: SR_GOV_69
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoRequest {
    pub tenant_id: TenantId,
    pub action_id: uuid::Uuid,
    pub requesting_admin: UserId,
}

/// Result of an undo attempt.
/// Implements: SR_GOV_69
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoResult {
    pub undone: bool,
    pub reason_if_not: Option<String>,
}

// -- Rejection justification validation requests (SR_GOV_72) -----------------

/// Input for validating a recommendation rejection.
/// Implements: SR_GOV_72
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionInput {
    pub tenant_id: TenantId,
    pub recommendation_id: uuid::Uuid,
    pub category: String,
    pub justification_text: String,
}

/// Result of a rejection validation.
/// Implements: SR_GOV_72
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionResult {
    pub stored: bool,
    pub validation_findings: Option<String>,
}

// -- Connection consent requests (SR_GOV_70) ---------------------------------

/// Request to capture explicit tenant consent for an external system connection.
/// Implements: SR_GOV_70
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConsentRequest {
    pub tenant_id: TenantId,
    pub system_id: String,
    pub connection_type: String,
    pub scope: String,
    pub vendor_terms_acknowledged: bool,
    pub paywall_acknowledgement: Option<PaywallAcknowledgement>,
    pub authorized_by: UserId,
}

/// Paywall acknowledgement for vendor terms of service.
/// Implements: SR_GOV_70
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaywallAcknowledgement {
    pub vendor_tos_url: String,
    pub accepted_at: DateTime<Utc>,
    pub accepted_by: UserId,
}

/// Result of a connection consent capture.
/// Implements: SR_GOV_70
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConsentResult {
    pub consent_id: uuid::Uuid,
    pub paywall_recorded: bool,
}

// -- Coverage enforcement requests (SR_GOV_71) --------------------------------

/// Input for coverage disclosure enforcement.
/// Implements: SR_GOV_71
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageEnforcementInput {
    pub tenant_id: TenantId,
    pub response_payload: serde_json::Value,
    pub query_context: serde_json::Value,
}

/// Result of coverage disclosure enforcement.
/// Implements: SR_GOV_71
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageEnforcementResult {
    pub passed: bool,
    pub missing_fields: Option<Vec<String>>,
}

// -- CSA rule registration requests (SR_GOV_23) -------------------------------

/// Request to register a new CSA rule.
/// Implements: SR_GOV_23
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaRuleRegistration {
    pub tenant_id: TenantId,
    pub rule_expression: String,
    pub action: CsaAction,
    pub severity: Severity,
    pub dry_run_sample_size: Option<usize>,
}

/// Result of CSA rule registration.
/// Implements: SR_GOV_23
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaRuleResult {
    pub rule_id: uuid::Uuid,
    pub version: u64,
    pub active: bool,
}

// -- CSA evaluator types (SR_GOV_25) ------------------------------------------

/// Output from the pure-function CSA evaluator.
/// Implements: SR_GOV_25
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaEvaluationOutput {
    pub matched_rules: Vec<String>,
    pub highest_action: Option<CsaAction>,
}

// -- CSA assessment requests (SR_GOV_24) --------------------------------------

/// Request to trigger a CSA assessment.
/// Implements: SR_GOV_24
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaAssessmentRequest {
    pub tenant_id: TenantId,
    pub query_id: uuid::Uuid,
    pub data_collection_refs: Vec<String>,
    pub combined_attribute_set: std::collections::HashSet<String>,
    pub query_purpose: Option<String>,
}

/// Result of a CSA assessment.
/// Implements: SR_GOV_24
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaAssessmentResult {
    pub decision: CsaDecision,
    pub applied_rules: Vec<String>,
    pub required_action: Option<CsaAction>,
}

// -- CSA block action requests (SR_GOV_26) ------------------------------------

/// Input for handling a CSA block action.
/// Implements: SR_GOV_26
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaBlockAction {
    pub assessment_id: uuid::Uuid,
    pub reason: String,
    pub suggested_alternatives: Vec<String>,
}

/// Result of a CSA block action.
/// Implements: SR_GOV_26
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaBlockResult {
    pub rejected: bool,
    pub alternatives: Vec<String>,
}

// -- CSA ANONYMIZE action requests (SR_GOV_27) --------------------------------

/// Input for a CSA anonymize action.
/// Implements: SR_GOV_27
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaAnonymizeAction {
    pub assessment_id: uuid::Uuid,
    pub data_collection_refs: Vec<String>,
    pub target_k_anonymity: u32,
    pub aggregation_strategy: String,
}

/// Result of a CSA anonymize action.
/// Implements: SR_GOV_27
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaAnonymizeResult {
    pub anonymized_payload: serde_json::Value,
    pub parameters_applied: String,
    pub residual_risk_score: f64,
}

// -- CSA ELEVATE action requests (SR_GOV_28) ----------------------------------

/// Input for a CSA elevate action.
/// Implements: SR_GOV_28
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaElevateAction {
    pub assessment_id: uuid::Uuid,
    pub required_permission: String,
    pub justification_required: bool,
}

/// Result of a CSA elevate action.
/// Implements: SR_GOV_28
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaElevateResult {
    pub required_permission: String,
    pub request_path: String,
}

// -- CSA break-glass requests (SR_GOV_29) -------------------------------------

/// Request to activate a break-glass override.
/// Implements: SR_GOV_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaBreakGlassRequest {
    pub tenant_id: TenantId,
    pub assessment_id: uuid::Uuid,
    pub justification: String,
    pub approver_1: UserId,
    pub approver_2: UserId,
    pub duration_minutes: Option<u64>,
}

/// Result of a break-glass activation.
/// Implements: SR_GOV_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaBreakGlassResult {
    pub authorized: bool,
    pub expires_at: DateTime<Utc>,
    pub review_id: uuid::Uuid,
}

/// Input for reviewing a break-glass activation.
/// Implements: SR_GOV_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakGlassReviewInput {
    pub review_id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub review_decision: super::enums::BreakGlassReviewDecision,
    pub notes: String,
}

/// Result of a break-glass review.
/// Implements: SR_GOV_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakGlassReviewResult {
    pub review_decision: super::enums::BreakGlassReviewDecision,
    pub follow_ups: Vec<String>,
}

// -- CSA assessment persistence requests (SR_GOV_30) --------------------------

/// Input for persisting a CSA assessment record.
/// Implements: SR_GOV_30
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaAssessmentPersistInput {
    pub tenant_id: TenantId,
    pub assessment_id: uuid::Uuid,
    pub query_id: uuid::Uuid,
    pub data_collection_refs: Vec<String>,
    pub decision: super::enums::CsaDecision,
    pub applied_rules: Vec<String>,
}

/// Result of persisting a CSA assessment.
/// Implements: SR_GOV_30
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsaAssessmentPersistResult {
    pub node_id: uuid::Uuid,
}

// -- LLM Router Stage 1 requests (SR_GOV_73) ----------------------------------

/// Input for the LLM Router Stage 1 governance check.
/// Implements: SR_GOV_73
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStage1Input {
    pub tenant_id: TenantId,
    pub principal_id: uuid::Uuid,
    pub data_attributes: serde_json::Value,
    pub request_context: serde_json::Value,
}

/// Result of the LLM Router Stage 1 evaluation.
/// Implements: SR_GOV_73
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStage1Result {
    pub allowed_models: Vec<String>,
    pub reasoning: Vec<String>,
}

// -- Decision Support preflight requests (SR_GOV_74) --------------------------

/// Input for a decision-support preflight check.
/// Implements: SR_GOV_74
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionSupportPreflightInput {
    pub tenant_id: TenantId,
    pub query_id: uuid::Uuid,
    pub data_collection_refs: Vec<String>,
    pub parameter_overrides: Vec<String>,
}

/// Result of a decision-support preflight check.
/// Implements: SR_GOV_74
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionSupportPreflightResult {
    pub allowed: bool,
    pub blocked_reasons: Vec<String>,
}

// -- UI visibility check requests (SR_GOV_75) ---------------------------------

/// Input for checking UI element visibility.
/// Implements: SR_GOV_75
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiVisibilityCheck {
    pub tenant_id: TenantId,
    pub principal_id: UserId,
    pub principal_roles: Vec<String>,
    pub ui_element_id: String,
    pub context: serde_json::Value,
}

/// Result of a UI element visibility check.
/// Implements: SR_GOV_75
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiVisibilityResult {
    pub decision: super::enums::UiVisibility,
}

// -- Connection Pull Preflight requests (SR_GOV_76) --------------------------

/// Input for a connection pull preflight check.
/// Implements: SR_GOV_76
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPullPreflight {
    pub tenant_id: TenantId,
    pub connection_id: String,
    pub scope: String,
    pub expected_volume: u64,
}

/// Result of a connection pull preflight check.
/// Implements: SR_GOV_76
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPullPreflightResult {
    pub decision: super::enums::PullPreflightDecision,
    pub defer_reason: Option<String>,
}

// -- Query Rewrite requests (SR_GOV_77) --------------------------------------

/// Input for an intelligence query rewrite.
/// Implements: SR_GOV_77
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRewriteInput {
    pub tenant_id: TenantId,
    pub principal_id: UserId,
    pub principal_roles: Vec<String>,
    pub raw_query: String,
}

/// Result of a query rewrite.
/// Implements: SR_GOV_77
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRewriteResult {
    pub rewritten_query: String,
    pub applied_filters: Vec<String>,
}

// -- Component Execution Preflight requests (SR_GOV_78) ----------------------

/// Input for a component execution preflight check.
/// Implements: SR_GOV_78
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentExecutionPreflight {
    pub tenant_id: TenantId,
    pub principal_id: UserId,
    pub principal_roles: Vec<String>,
    pub component_id: String,
    pub args: serde_json::Value,
}

/// Result of a component execution preflight check.
/// Implements: SR_GOV_78
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentExecutionPreflightResult {
    pub decision: super::enums::AccessDecision,
}

// -- Approval chain requests (SR_GOV_41, SR_GOV_43) --------------------------

/// Request to create an approval request.
/// Implements: SR_GOV_41
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalCreateRequest {
    pub tenant_id: TenantId,
    pub action: String,
    pub requested_by: UserId,
    pub payload: serde_json::Value,
    pub sla_tier: Option<String>,
}

/// Result of creating an approval request.
/// Implements: SR_GOV_41
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequestResult {
    pub approval_id: uuid::Uuid,
    pub approvers: Vec<UserId>,
    pub sla_deadline: DateTime<Utc>,
}

/// Input for recording an approver's decision in an approval chain.
/// Implements: SR_GOV_43
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalChainExecution {
    pub approval_id: uuid::Uuid,
    pub approver_id: UserId,
    pub decision: super::enums::ApprovalDecision,
}

/// Result of executing an approval chain step.
/// Implements: SR_GOV_43
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalChainResult {
    pub final_state: ApprovalStatus,
    pub decisions: Vec<(UserId, super::enums::ApprovalDecision)>,
}

// -- Delegation requests (SR_GOV_44) ----------------------------------------

/// Request to create a delegation from one approver to another.
/// Implements: SR_GOV_44
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationRequest {
    pub tenant_id: TenantId,
    pub from_person: UserId,
    pub to_person: UserId,
    pub scope: String,
    pub expires_at: DateTime<Utc>,
}

/// Result of creating a delegation.
/// Implements: SR_GOV_44
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationResult {
    pub delegation_id: uuid::Uuid,
    pub affected_approvals: Vec<uuid::Uuid>,
}

// -- Escalation requests (SR_GOV_45) ----------------------------------------

/// Request to escalate an approval past its SLA deadline.
/// Implements: SR_GOV_45
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationRequest {
    pub tenant_id: TenantId,
    pub approval_id: uuid::Uuid,
    pub current_approver: UserId,
    pub new_approver: UserId,
}

/// Result of an SLA escalation.
/// Implements: SR_GOV_45
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationResult {
    pub reassigned_to: UserId,
    pub new_deadline: DateTime<Utc>,
}

// -- Approval Break-Glass requests (SR_GOV_46) -------------------------------

/// Request to activate an approval break-glass override.
/// Requires two-person approval and mandatory post-incident review.
/// Implements: SR_GOV_46
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalBreakGlassRequest {
    pub tenant_id: TenantId,
    pub action: String,
    pub requested_by: UserId,
    pub justification: String,
    pub second_approver: UserId,
    pub duration_minutes: Option<u64>,
}

/// Result of an approval break-glass activation.
/// Implements: SR_GOV_46
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalBreakGlassResult {
    pub authorized: bool,
    pub expires_at: DateTime<Utc>,
    pub review_id: uuid::Uuid,
}

/// Input for reviewing an approval break-glass activation.
/// Implements: SR_GOV_46
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalBreakGlassReviewInput {
    pub tenant_id: TenantId,
    pub review_id: uuid::Uuid,
    pub review_decision: super::enums::BreakGlassReviewDecision,
    pub notes: String,
}

/// Result of an approval break-glass review.
/// Implements: SR_GOV_46
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalBreakGlassReviewResult {
    pub review_decision: super::enums::BreakGlassReviewDecision,
    pub follow_ups: Vec<String>,
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

// -- Data model node requests (SR_DM_03 .. SR_DM_10) --------------------------

/// Input for creating a Compartment graph node.
/// Implements: SR_DM_03
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentNodeInput {
    pub tenant_id: TenantId,
    pub name: String,
    pub classification_level: ClassificationLevel,
    pub member_roles: Vec<RoleId>,
    pub member_persons: Vec<UserId>,
    pub purpose: String,
    pub criminal_penalty_isolation: bool,
}

/// Result of compartment graph node creation.
/// Implements: SR_DM_03
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentNodeResult {
    pub compartment_id: uuid::Uuid,
}

/// Input for creating a Connection graph node.
/// Implements: SR_DM_04
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionNodeInput {
    pub tenant_id: TenantId,
    pub system_id: String,
    pub connection_type: String,
    pub auth_type: String,
    pub credential_caas_ref: Option<String>,
    pub status: String,
    pub scope: String,
    pub metadata: serde_json::Value,
}

/// Result of connection graph node creation.
/// Implements: SR_DM_04
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionNodeResult {
    pub connection_id: uuid::Uuid,
}

/// Input for audit partition maintenance.
/// Implements: SR_DM_06
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPartitionMaintenanceRequest {
    pub tenant_id: TenantId,
    pub period: String,
}

/// Result of audit partition maintenance.
/// Implements: SR_DM_06
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPartitionMaintenanceResult {
    pub archived_count: u64,
    pub dropped_count: u64,
}

/// Input for creating a DataCollection graph node.
/// Implements: SR_DM_07
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollectionInput {
    pub tenant_id: TenantId,
    pub connection_id: uuid::Uuid,
    pub source_system: String,
    pub pull_timestamp: DateTime<Utc>,
    pub freshness_policy: String,
    pub record_count: u64,
    pub ingestion_method: String,
    pub source_file_ref: Option<String>,
    pub training_consent: bool,
    pub data_origin: DataOrigin,
}

/// Result of DataCollection graph node creation.
/// Implements: SR_DM_07
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollectionResult {
    pub collection_id: uuid::Uuid,
}

/// Input for upserting a batch of DataField graph nodes.
/// Implements: SR_DM_08
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFieldInputBatch {
    pub tenant_id: TenantId,
    pub collection_id: uuid::Uuid,
    pub fields: Vec<DataFieldInput>,
}

/// A single data field definition within a DataCollection.
/// Implements: SR_DM_08
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFieldInput {
    pub field_name: String,
    pub technical_type: String,
    pub semantic_type: Option<String>,
    pub classification: Option<String>,
    pub sensitivity_level: Option<String>,
    pub completeness_pct: Option<f64>,
}

/// Result of a DataField batch upsert.
/// Implements: SR_DM_08
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFieldBatchResult {
    pub upserted_count: usize,
}

/// Input for creating a Recommendation graph node (dual-store).
/// Implements: SR_DM_09
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationNodeInput {
    pub tenant_id: TenantId,
    pub content_hash: String,
    pub model_used: String,
    pub confidence: f64,
    pub parameters_used: Vec<String>,
    pub state: String,
    pub category: Option<String>,
}

/// Result of Recommendation graph node creation.
/// Implements: SR_DM_09
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationNodeResult {
    pub rec_id: uuid::Uuid,
    pub audit_row_id: uuid::Uuid,
}

/// Input for creating a Rejection graph node.
/// Implements: SR_DM_10
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionNodeInput {
    pub tenant_id: TenantId,
    pub recommendation_id: uuid::Uuid,
    pub category: String,
    pub justification_text: String,
    pub person_id: UserId,
    pub timestamp: DateTime<Utc>,
}

/// Result of Rejection graph node creation.
/// Implements: SR_DM_10
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectionNodeResult {
    pub rejection_id: uuid::Uuid,
}

// -- Component node requests (SR_DM_12) ----------------------------------------

/// Input for creating a Component graph node.
/// Implements: SR_DM_12
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentNodeInput {
    pub tenant_id: TenantId,
    pub component_id: String,
    pub category: String,
    pub version: String,
    pub git_sha: Option<String>,
    pub status: String,
    pub metadata: serde_json::Value,
}

/// Result of Component graph node creation.
/// Implements: SR_DM_12
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentNodeResult {
    pub node_id: uuid::Uuid,
}

// -- Component registry row requests (SR_DM_13) --------------------------------

/// Input for registering a component in the relational registry.
/// Implements: SR_DM_13
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRegistryRow {
    pub tenant_id: TenantId,
    pub component_id: String,
    pub version: String,
    pub git_sha: Option<String>,
    pub status: String,
    pub owner_id: UserId,
    pub scope: String,
}

/// Result of component registry insertion.
/// Implements: SR_DM_13
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentRegistryResult {
    pub row_id: uuid::Uuid,
}

// -- Component performance requests (SR_DM_14) ---------------------------------

/// Input for recording component performance telemetry.
/// Implements: SR_DM_14
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPerformanceRow {
    pub tenant_id: TenantId,
    pub component_id: String,
    pub execution_count: u64,
    pub latency_ms: u64,
    pub success_count: u64,
    pub failure_count: u64,
    pub cost_usd: f64,
}

/// Result of component performance recording.
/// Implements: SR_DM_14
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentPerformanceResult {
    pub row_id: uuid::Uuid,
}

// -- ModelExecution node requests (SR_DM_15) ------------------------------------

/// Input for creating a ModelExecution graph node.
/// Implements: SR_DM_15
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelExecutionInput {
    pub tenant_id: TenantId,
    pub model_id: String,
    pub slot: String,
    pub task_type: LlmTaskType,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
    pub cost_usd: f64,
    pub data_sensitivity: String,
    pub training_run_id: Option<uuid::Uuid>,
}

/// Result of ModelExecution graph node creation.
/// Implements: SR_DM_15
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelExecutionResult {
    pub execution_id: uuid::Uuid,
}

// -- ModelOutcomeScore requests (SR_DM_16) --------------------------------------

/// Input for recording a model outcome score.
/// Implements: SR_DM_16
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOutcomeInput {
    pub tenant_id: TenantId,
    pub execution_id: uuid::Uuid,
    pub outcome_type: String,
    pub outcome_value: String,
    pub quality_score: f64,
}

/// Result of model outcome scoring.
/// Implements: SR_DM_16
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOutcomeResult {
    pub score_id: uuid::Uuid,
}

// -- Model performance aggregation requests (SR_DM_17) --------------------------

/// Request to aggregate model performance metrics for a period.
/// Implements: SR_DM_17
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAggregationRequest {
    pub tenant_id: TenantId,
    pub period: String,
}

/// Result of model performance aggregation.
/// Implements: SR_DM_17
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAggregationResult {
    pub rows_updated: u64,
}

// -- Vector embedding requests (SR_DM_18) ---------------------------------------

/// Input for creating a vector embedding.
/// Implements: SR_DM_18
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingInput {
    pub tenant_id: TenantId,
    pub source_node_id: uuid::Uuid,
    pub text: String,
    pub model_id: String,
}

/// Result of vector embedding creation.
/// Implements: SR_DM_18
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResult {
    pub vector_dim: usize,
    pub model_id: String,
    pub embedded_at: DateTime<Utc>,
}

// -- Dual embedding store requests (SR_DM_19) -----------------------------------

/// Input for storing dual embeddings during model migration.
/// Implements: SR_DM_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualEmbeddingInput {
    pub tenant_id: TenantId,
    pub source_node_id: uuid::Uuid,
    pub old_embedding: Vec<f32>,
    pub new_embedding: Vec<f32>,
    pub old_model: String,
    pub new_model: String,
}

/// Result of dual embedding storage.
/// Implements: SR_DM_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualEmbeddingResult {
    pub dual_active_until: DateTime<Utc>,
}

// -- SA usage and anomaly log requests (SR_DM_21) -------------------------------

/// Input for logging a service account usage event.
/// Implements: SR_DM_21
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaUsageEvent {
    pub tenant_id: TenantId,
    pub sa_id: uuid::Uuid,
    pub action: String,
    pub target: String,
    pub timestamp: DateTime<Utc>,
}

/// Input for logging a service account anomaly event.
/// Implements: SR_DM_21
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaAnomalyEvent {
    pub tenant_id: TenantId,
    pub sa_id: uuid::Uuid,
    pub anomaly_type: String,
    pub severity: Severity,
    pub evidence: serde_json::Value,
}

// -- Event-driven sync requests (SR_DM_22) ------------------------------------

/// An event describing a cross-store sync operation.
/// Implements: SR_DM_22
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEvent {
    pub source_store: String,
    pub target_store: String,
    pub entity_type: String,
    pub entity_id: uuid::Uuid,
    pub payload: serde_json::Value,
    pub tenant_id: TenantId,
}

/// Result of processing a sync event.
/// Implements: SR_DM_22
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub applied_at: DateTime<Utc>,
    pub latency_ms: u64,
    pub state: String,
}

// -- Vector write enforcer requests (SR_DM_23) --------------------------------

/// A vector write attempt to be validated before persistence.
/// Implements: SR_DM_23
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorWriteAttempt {
    pub source: String,
    pub model_id: Option<String>,
    pub vector: Vec<f32>,
    pub tenant_id: TenantId,
}

/// Result of vector write enforcement.
/// Implements: SR_DM_23
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorWriteResult {
    pub accepted: bool,
    pub reason: Option<String>,
}

// -- Graph maintenance requests (SR_DM_24) ------------------------------------

/// Request to run a graph maintenance cycle.
/// Implements: SR_DM_24
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceCycleRequest {
    pub tenant_id: Option<TenantId>,
    pub cycle_type: MaintenanceCycleType,
}

/// Result of a graph maintenance cycle.
/// Implements: SR_DM_24
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceCycleResult {
    pub affected_count: u64,
}

// -- Notification log requests (SR_DM_25) -------------------------------------

/// A notification row to be inserted into the notification log.
/// Implements: SR_DM_25
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRow {
    pub tenant_id: TenantId,
    pub person_id: UserId,
    pub message: String,
    pub original_timestamp: Option<DateTime<Utc>>,
    pub read_state: bool,
}

/// Result of notification log insertion.
/// Implements: SR_DM_25
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationResult {
    pub row_id: uuid::Uuid,
}

// -- User preferences requests (SR_DM_26) -------------------------------------

/// A user preference row to be upserted.
/// Implements: SR_DM_26
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceRow {
    pub tenant_id: TenantId,
    pub person_id: UserId,
    pub key: String,
    pub value: serde_json::Value,
}

/// Result of preference upsert.
/// Implements: SR_DM_26
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceResult {
    pub row_id: uuid::Uuid,
}

// -- Tenant isolation audit requests (SR_DM_28) -------------------------------

/// A detected tenant isolation violation.
/// Implements: SR_DM_28
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationViolation {
    pub entity_type: String,
    pub entity_id: uuid::Uuid,
    pub tenant_a: TenantId,
    pub tenant_b: TenantId,
    pub description: String,
}

/// Result of a tenant isolation audit scan.
/// Implements: SR_DM_28
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationAuditResult {
    pub result: String,
    pub violations: Vec<IsolationViolation>,
}

// -- Connection lifecycle requests (SR_CONN_01 .. SR_CONN_10) -----------------

/// Input for requesting a new external system connection.
/// Implements: SR_CONN_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRequestInput {
    pub tenant_id: TenantId,
    pub system_id: String,
    pub connection_type: String,
    pub scope: String,
    pub justification: Option<String>,
    pub requested_by: UserId,
}

/// Result of creating a connection request.
/// Implements: SR_CONN_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRequestResult {
    pub connection_id: uuid::Uuid,
    pub state: super::enums::ConnectionState,
}

/// Input for provisioning a credential on a connection.
/// Implements: SR_CONN_04
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialProvisionInput {
    pub connection_id: uuid::Uuid,
    pub raw_credential: String,
}

/// Result of credential provisioning.
/// Implements: SR_CONN_04
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialProvisionResult {
    pub credential_ref: String,
}

/// Result of testing connectivity to an external system.
/// Implements: SR_CONN_05
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub passed: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// Result of activating a connection.
/// Implements: SR_CONN_06
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionActivationResult {
    pub active: bool,
    pub first_pull_at: Option<DateTime<Utc>>,
}

/// Input for marking a connection as degraded.
/// Implements: SR_CONN_07
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationInput {
    pub connection_id: uuid::Uuid,
    pub reason: String,
    pub evidence: serde_json::Value,
}

/// Input for decommissioning a connection.
/// Implements: SR_CONN_09
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecommissionInput {
    pub connection_id: uuid::Uuid,
    pub reason: String,
    pub retain_data: bool,
}

// -- Feature flag cache invalidation requests (SR_DM_29) ----------------------

/// Request to toggle a feature flag with cache invalidation.
/// Implements: SR_DM_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagToggle {
    pub tenant_id: TenantId,
    pub flag_id: String,
    pub value: bool,
}

/// Result of feature flag toggle with cache invalidation.
/// Implements: SR_DM_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlagCacheResult {
    pub active: bool,
    pub cache_invalidated: bool,
}

// -- Log stream ingestion requests (SR_CONN_19 .. SR_CONN_24) ----------------

/// A raw log event read from a log source before parsing or redaction.
/// Implements: SR_CONN_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawLogEvent {
    pub content: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub source_id: String,
}

/// Input for a log ingestion run.
/// Implements: SR_CONN_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogIngestInput {
    pub tenant_id: TenantId,
    pub source_id: String,
    pub mode: String,
    pub since_offset: u64,
}

/// Result of a log ingestion run.
/// Implements: SR_CONN_19
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogIngestResult {
    pub events_ingested: u64,
    pub last_offset: u64,
}

/// Input for parser selection over sample lines.
/// Implements: SR_CONN_20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserSelectionInput {
    pub sample_lines: Vec<String>,
}

/// Result of parser selection.
/// Implements: SR_CONN_20
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserSelectionResult {
    pub parser_id: Option<String>,
    pub candidate: Option<String>,
}

/// A PII match found in log text.
/// Implements: SR_CONN_21
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiiMatch {
    pub field: String,
    pub pii_type: String,
    pub start: usize,
    pub end: usize,
}

/// A correlation trace linking events across log sources.
/// Implements: SR_CONN_22
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationTrace {
    pub trace_id: String,
    pub source_events: Vec<uuid::Uuid>,
    pub created_at: DateTime<Utc>,
}

/// Request to configure the ingestion mode for a log source.
/// Implements: SR_CONN_23
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogIngestionModeRequest {
    pub source_id: String,
    pub mode: IngestionMode,
}

/// Result of configuring the ingestion mode.
/// Implements: SR_CONN_23
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogIngestionModeResult {
    pub mode: IngestionMode,
    pub next_run_at: Option<DateTime<Utc>>,
}

/// A log metric row for ingestion monitoring.
/// Implements: SR_CONN_24
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogMetricRow {
    pub tenant_id: TenantId,
    pub source_id: String,
    pub events_per_second: f64,
    pub parse_failure_rate: f64,
    pub lag_seconds: u64,
    pub redaction_count: u64,
}

// -- Classification gate requests (SR_CONN_25 .. SR_CONN_31) -----------------

/// Result of Stage 1 technical classification.
/// Implements: SR_CONN_26
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalClassificationResult {
    pub types: Vec<String>,
    pub formats: Vec<String>,
    pub schema_version: String,
}

/// A named-entity recognition match from an NER ensemble.
/// Implements: SR_CONN_28
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NerMatch {
    pub entity: String,
    pub classification: String,
    pub confidence: f64,
}

/// Result of Stage 2 security classification.
/// Implements: SR_CONN_28
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityClassificationResult {
    pub classifications: Vec<String>,
    pub confidence_per_field: Vec<(String, f64)>,
}

/// Result of the full classification gate evaluation.
/// Implements: SR_CONN_27
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationGateResult {
    pub gate: ClassificationGateDecision,
    pub technical_result: TechnicalClassificationResult,
    pub security_result: SecurityClassificationResult,
}

/// Result of Stage 3 semantic classification.
/// Implements: SR_CONN_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticClassificationResult {
    pub fields: Vec<SemanticFieldTag>,
}

/// A semantic tag applied to a single data field.
/// Implements: SR_CONN_29
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFieldTag {
    pub field_name: String,
    pub semantic_type: String,
    pub business_domain: String,
}

/// A candidate edge proposed by relationship inference.
/// Implements: SR_CONN_30
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateEdge {
    pub from_field: String,
    pub to_field: String,
    pub relationship: String,
    pub confidence: f64,
    pub confirmed_by: String,
}

/// Data quality report from Stage 5 quality assessment.
/// Implements: SR_CONN_31
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataQualityReport {
    pub collection_id: uuid::Uuid,
    pub overall_score: f64,
    pub completeness: f64,
    pub consistency: f64,
    pub freshness: f64,
}

// -- Connection operations requests (SR_CONN_32 .. SR_CONN_44) ----------------

/// A quarantine record for data that failed classification.
/// Implements: SR_CONN_32
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineRecord {
    pub id: uuid::Uuid,
    pub tenant_id: TenantId,
    pub execution_record_id: uuid::Uuid,
    pub reason: String,
    pub policy: QuarantinePolicy,
    pub expires_at: DateTime<Utc>,
}

/// A pull lock preventing concurrent pulls on the same scope.
/// Implements: SR_CONN_34
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullLock {
    pub connection_id: uuid::Uuid,
    pub scope: String,
    pub acquired_at: DateTime<Utc>,
    pub ttl_seconds: u64,
}

/// A snapshot of connection schema fields at a point in time.
/// Implements: SR_CONN_35
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    pub connection_id: uuid::Uuid,
    pub fields: Vec<String>,
    pub captured_at: DateTime<Utc>,
}

/// An event emitted when schema changes are detected.
/// Implements: SR_CONN_35
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaChangeEvent {
    pub connection_id: uuid::Uuid,
    pub added_fields: Vec<String>,
    pub removed_fields: Vec<String>,
    pub severity: Severity,
}

/// Input for a rate budget check.
/// Implements: SR_CONN_36
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateBudgetCheck {
    pub system_id: String,
    pub tenant_id: TenantId,
    pub expected_call_count: u64,
}

/// Result of a rate budget check.
/// Implements: SR_CONN_36
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateBudgetResult {
    pub decision: PullPreflightDecision,
    pub defer_reason: Option<String>,
}

/// KPI snapshot for a single connection.
/// Implements: SR_CONN_37
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionKpiSnapshot {
    pub connection_id: uuid::Uuid,
    pub uptime_pct: f64,
    pub avg_latency_ms: u64,
    pub error_rate_pct: f64,
    pub last_successful_pull: Option<DateTime<Utc>>,
}

/// A classification override for a specific field.
/// Implements: SR_CONN_38
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationOverride {
    pub system_id: String,
    pub field_name: String,
    pub classification: String,
}

/// Summary of a connection's KPIs for the health dashboard.
/// Implements: SR_CONN_44
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionKpiSummary {
    pub connection_id: uuid::Uuid,
    pub system_id: String,
    pub status: String,
    pub uptime_pct: f64,
    pub avg_latency_ms: u64,
    pub error_rate_pct: f64,
}

/// Result of a health dashboard query.
/// Implements: SR_CONN_44
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthDashboardResult {
    pub connections: Vec<ConnectionKpiSummary>,
}

// ============================================================================
// Intelligence Layer request / result types (SR_INT_01 .. SR_INT_08)
// ============================================================================

use super::entities::RelationshipCandidate;
use super::entities::SemanticTag;

/// Input for initializing an empty intelligence graph for a new tenant.
/// Implements: SR_INT_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInitInput {
    pub tenant_id: TenantId,
}

/// Result of graph initialization: the per-tenant subgraph is ready to
/// receive nodes from subsequent SRs.
/// Implements: SR_INT_01
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphInitResult {
    pub ready: bool,
}

/// Reference to a DataCollection that has arrived and needs Stage 3-6 jobs
/// queued for async tagging.
/// Implements: SR_INT_02
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollectionRef {
    pub tenant_id: TenantId,
    pub collection_id: uuid::Uuid,
}

/// Result of triggering the tagging pipeline: the number of Stage 3-6 jobs
/// enqueued.
/// Implements: SR_INT_02
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggingTriggerResult {
    pub jobs_queued: u32,
}

/// Input for Stage 3 semantic tagging: the fields to tag inside a
/// DataCollection.
/// Implements: SR_INT_03
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticTaggingInput {
    pub tenant_id: TenantId,
    pub collection_id: uuid::Uuid,
    pub fields: Vec<String>,
}

/// Result of Stage 3 semantic tagging: count tagged and the inferred tags.
/// Implements: SR_INT_03
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticTaggingResult {
    pub fields_tagged: u32,
    pub tags: Vec<SemanticTag>,
}

/// Input for Stage 4 relationship inference across a DataCollection.
/// Implements: SR_INT_04
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipInferenceInput {
    pub tenant_id: TenantId,
    pub collection_id: uuid::Uuid,
}

/// Result of Stage 4 relationship inference with separate counts for
/// auto-added and queued-for-review candidates.
/// Implements: SR_INT_04
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipInferenceResult {
    pub edges_added: u32,
    pub edges_queued: u32,
    pub candidates: Vec<RelationshipCandidate>,
}

/// Input for creating a DataSnapshot at a specific timestamp.
/// Implements: SR_INT_05
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotInput {
    pub tenant_id: TenantId,
    pub collection_id: uuid::Uuid,
    pub timestamp: DateTime<Utc>,
}

/// Result of snapshot creation: the new snapshot id and its content checksum.
/// Implements: SR_INT_05
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResult {
    pub snapshot_id: uuid::Uuid,
    pub checksum: String,
}

/// Input for Stage 5 quality assessment of a DataCollection.
/// Implements: SR_INT_06
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityInput {
    pub tenant_id: TenantId,
    pub collection_id: uuid::Uuid,
}

/// Result of Stage 5 quality assessment: the DataQualityReport id and an
/// overall [0,1] score.
/// Implements: SR_INT_06
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityResult {
    pub report_id: uuid::Uuid,
    pub score: f64,
}

/// Input for computing a TrendAnalysis over a series of snapshots.
/// Implements: SR_INT_07
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendInput {
    pub tenant_id: TenantId,
    pub metric: String,
    pub snapshot_ids: Vec<uuid::Uuid>,
}

/// Result of a TrendAnalysis computation.
/// Implements: SR_INT_07
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendResult {
    pub trend_id: uuid::Uuid,
    pub direction: TrendDirection,
    pub magnitude: f64,
}

/// Input for enqueuing a low-confidence item into the human review queue.
/// Implements: SR_INT_08
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQueueInput {
    pub tenant_id: TenantId,
    pub item_type: String,
    pub item_ref: String,
    pub confidence: f64,
}

/// Result of enqueuing an item into the review queue.
/// Implements: SR_INT_08
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewQueueResult {
    pub queue_id: uuid::Uuid,
}
