//! Domain enumerations shared across all PRISM crates.

use serde::{Deserialize, Serialize};

/// Compliance profile classification for automations and data flows.
/// Determines which regulatory rules and compartment visibility apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceProfile {
    BsaAml,
    Sox,
    FairLending,
    InternalAudit,
    General,
}

/// Lifecycle state of a registered automation.
/// Transitions are governed by the state machine in prism-lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Draft,
    PendingApproval,
    ApprovedWithConditions,
    Active,
    UnderReview,
    Suspended,
    Sunset,
    Archived,
    Deleted,
    Rejected,
}

/// Severity classification for governance events, gaps, and alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// Evidence grading framework from architecture exploration.
/// Used to classify the confidence level of architectural decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceGrade {
    Unverified,
    Provisional,
    Emerging,
    HighProb,
    Proven,
}

/// Identity type for service principals.
/// Track A implements Automation only; CandidateAgent and QualifiedAgent
/// are Track B (Kernel V6) placeholders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityType {
    /// Standard automation identity (Track A).
    Automation,
    /// AI agent pending qualification (Track B placeholder).
    CandidateAgent,
    /// AI agent that has passed qualification (Track B placeholder).
    QualifiedAgent,
}

/// Governance profile classification.
/// Track A implements Tool only; Kernel is a Track B placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceProfile {
    /// Non-autonomous tool automation (Track A).
    Tool,
    /// Autonomous kernel-governed agent (Track B placeholder).
    Kernel,
}

/// Scope of an approval chain, determined by the LCA algorithm.
/// Wider scope requires higher-authority approvers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    SingleTeam,
    CrossTeam,
    CrossDepartment,
    CrossBu,
    CrossDivision,
    CrossEntity,
}

/// Status of an approval chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    InReview,
    Approved,
    ApprovedWithConditions,
    Rejected,
    Escalated,
    Withdrawn,
}

/// Platform role categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformRole {
    PlatformAdmin,
    TenantAdmin,
    GovernanceOfficer,
    ComplianceReviewer,
    AutomationOwner,
    AutomationDeveloper,
    Auditor,
    Examiner,
    ReadOnly,
}

/// Legal entity type for tenant classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegalEntityType {
    HoldingCompany,
    Bank,
    InsuranceCarrier,
    BrokerDealer,
    AssetManager,
    ServiceSubsidiary,
    JointVenture,
}

/// Blast radius tier for automations (from GAP-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlastRadiusTier {
    /// Affects a single process within one team.
    Contained,
    /// Affects multiple processes within one department.
    Department,
    /// Affects multiple departments or business units.
    CrossUnit,
    /// Affects multiple legal entities or external systems.
    Enterprise,
}

/// Environment classification (from GAP-35).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Environment {
    Dev,
    Uat,
    Prod,
}

/// Actor type for audit events (SR_GOV_47).
/// Identifies whether the actor is a human, a service principal, or the system itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    Human,
    ServicePrincipal,
    System,
}

/// Classification level for visibility compartments.
/// Determines the sensitivity of data within the compartment.
/// Implements: SR_GOV_31
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationLevel {
    /// Publicly available information.
    Public,
    /// Internal use only.
    Internal,
    /// Confidential -- limited distribution.
    Confidential,
    /// Restricted -- need-to-know basis.
    Restricted,
    /// Criminal penalty -- statutory penalties for unauthorized disclosure.
    CriminalPenalty,
}

/// Classification of a governance rule.
/// Implements: SR_GOV_16, SR_GOV_17
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleClass {
    /// Non-overridable security/compliance rules. Cannot be bypassed.
    Enforce,
    /// Advisory rules that can be overridden with justification.
    Advise,
}

/// Decision from an ENFORCE rule evaluation.
/// Implements: SR_GOV_16
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnforceDecision {
    Allow,
    Deny,
}

/// Decision from an ADVISE rule evaluation.
/// Implements: SR_GOV_17
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviseDecision {
    Allow,
    AllowWithWarning,
    RequireJustification,
}

/// Access decision for compartment-gated operations.
/// Implements: SR_GOV_33
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessDecision {
    Allow,
    Deny,
}

/// Export format for audit trail regulatory exports.
/// Implements: SR_GOV_50
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// JSON lines -- one event per line, signed envelope.
    JsonLines,
    /// CSV with header row.
    Csv,
    /// PDF report with chain proof appendix.
    Pdf,
}

/// Type of conflict between governance rules.
/// Implements: SR_GOV_20
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    /// Two rules produce opposite decisions for the same action/attributes.
    Contradiction,
    /// One rule is a strict subset of another (broader rule makes narrower redundant).
    Subsumption,
    /// Two rules overlap partially, creating ambiguous outcomes.
    Overlap,
}

/// Alert channel for severity-based routing.
/// Implements: SR_GOV_67
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertChannel {
    /// Page the on-call engineer (PagerDuty, OpsGenie, etc.).
    Page,
    /// SMS notification.
    Sms,
    /// In-app notification (real-time).
    InApp,
    /// Email notification.
    Email,
    /// Digest (batched, lower urgency).
    Digest,
}

/// Privacy level for query analytics (D-17).
/// Determines what identifying information is retained.
/// Implements: SR_GOV_37
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    /// Fully anonymized -- no identifying information retained.
    Anonymous,
    /// Role-level -- individual identity stripped, role/department retained.
    Role,
    /// Individual-level -- full identity retained (restricted access).
    Individual,
}

/// Scope of analytics access or export.
/// Implements: SR_GOV_39
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsScope {
    /// Anonymous aggregate data -- visible to anyone.
    Anonymous,
    /// Role-level data -- visible to department heads and C-suite.
    RoleBased,
    /// Individual-level data -- visible only to self and designated admin.
    Individual,
}

/// Complexity tier for query classification.
/// Implements: SR_GOV_37
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplexityTier {
    Simple,
    Moderate,
    Complex,
    Expert,
}

/// Query outcome for analytics.
/// Implements: SR_GOV_37
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryOutcome {
    Success,
    Partial,
    Failed,
    Blocked,
}

/// Action to take when a CSA rule matches.
/// Implements: SR_GOV_23
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CsaAction {
    /// Block the query entirely.
    Block,
    /// Anonymize sensitive attributes before returning results.
    Anonymize,
    /// Elevate to a human reviewer for approval.
    Elevate,
}

/// Decision from a CSA assessment.
/// Implements: SR_GOV_24
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CsaDecision {
    Allow,
    Block,
    Anonymize,
    Elevate,
}

/// Decision from a break-glass activation review.
/// Implements: SR_GOV_29
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakGlassReviewDecision {
    /// The break-glass activation was justified.
    Justified,
    /// The break-glass activation was not justified.
    Unjustified,
    /// The CSA rule that triggered the break-glass needs refinement.
    NeedsRuleRefinement,
}

/// UI element visibility decision.
/// Implements: SR_GOV_75
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiVisibility {
    /// Element is fully visible and interactive.
    Visible,
    /// Element is hidden from the user.
    Hidden,
    /// Element is visible but not editable.
    ReadOnly,
}

/// Decision for a connection pull preflight check.
/// Implements: SR_GOV_76
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullPreflightDecision {
    /// All checks passed -- pull is allowed.
    Allow,
    /// A hard requirement is not met -- pull is denied.
    Deny,
    /// A soft constraint (e.g. budget) prevents immediate pull -- retry later.
    Defer,
}

/// Decision recorded by an approver in an approval chain.
/// Implements: SR_GOV_43
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// The approver approves the request.
    Approve,
    /// The approver rejects the request.
    Reject,
    /// The approver defers the decision.
    Defer,
}

/// Origin of data ingested into a DataCollection node.
/// Implements: SR_DM_07
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataOrigin {
    /// Data pulled from an external system via a registered connection.
    ConnectionPull,
    /// Data uploaded manually by a user.
    UserUpload,
    /// Data streamed from application logs.
    LogStream,
    /// Data imported via a bulk migration or ETL job.
    BulkImport,
    /// Data generated by a predictive model.
    SystemPrediction,
    /// Data gathered by a research agent workflow.
    ResearchAgent,
}

/// Source layer that produced a governance event (SR_GOV_47).
/// Maps to the architectural layers in the PRISM spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLayer {
    Governance,
    Identity,
    Compliance,
    Credentials,
    Lifecycle,
    Audit,
    Graph,
    Llm,
    Connection,
    Runtime,
    Interface,
}
