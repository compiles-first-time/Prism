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
