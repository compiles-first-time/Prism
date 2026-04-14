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

// -- Governance Rule Repository (SR_GOV_16, SR_GOV_17) ----------------------

/// Persistence operations for governance rules.
/// Implements: SR_GOV_16, SR_GOV_17
#[async_trait]
pub trait GovernanceRuleRepository: Send + Sync {
    /// List active rules for a tenant matching the given action and rule class.
    async fn list_active_rules(
        &self,
        tenant_id: TenantId,
        action: &str,
        rule_class: RuleClass,
    ) -> Result<Vec<GovernanceRule>, PrismError>;
}

// -- Compartment Repository (SR_GOV_31) -------------------------------------

/// Persistence operations for visibility compartments.
/// Implements: SR_GOV_31, SR_GOV_32, SR_GOV_33
#[async_trait]
pub trait CompartmentRepository: Send + Sync {
    /// Create a new compartment.
    /// Implements: SR_GOV_31
    async fn create(&self, compartment: &Compartment) -> Result<(), PrismError>;

    /// Retrieve a compartment by ID.
    async fn get_by_id(
        &self,
        tenant_id: TenantId,
        id: CompartmentId,
    ) -> Result<Option<Compartment>, PrismError>;

    /// Add a membership record (person or role).
    /// Implements: SR_GOV_32
    async fn add_member(&self, membership: &CompartmentMembership) -> Result<bool, PrismError>;

    /// List members of a compartment.
    async fn list_members(
        &self,
        tenant_id: TenantId,
        compartment_id: CompartmentId,
    ) -> Result<Vec<CompartmentMembership>, PrismError>;

    /// Check if a person is a member of a compartment (directly or via role).
    /// Implements: SR_GOV_33
    async fn is_member(
        &self,
        tenant_id: TenantId,
        compartment_id: CompartmentId,
        person_id: UserId,
        role_ids: &[RoleId],
    ) -> Result<bool, PrismError>;

    /// Remove a membership record (person or role).
    /// Returns true if a membership was actually removed, false if it didn't exist.
    /// Implements: SR_GOV_34
    async fn remove_member(
        &self,
        tenant_id: TenantId,
        compartment_id: CompartmentId,
        person_id: Option<UserId>,
        role_id: Option<RoleId>,
    ) -> Result<bool, PrismError>;
}

// -- Feature Flag Repository (SR_GOV_68) ------------------------------------

/// Persistence operations for feature flags.
/// Implements: SR_GOV_68
#[async_trait]
pub trait FeatureFlagRepository: Send + Sync {
    /// Get a feature flag by tenant and flag_id.
    async fn get(
        &self,
        tenant_id: TenantId,
        flag_id: &str,
    ) -> Result<Option<FeatureFlag>, PrismError>;

    /// Set (create or update) a feature flag.
    async fn set(&self, flag: &FeatureFlag) -> Result<(), PrismError>;

    /// List all feature flags for a tenant.
    async fn list_for_tenant(&self, tenant_id: TenantId) -> Result<Vec<FeatureFlag>, PrismError>;
}

// -- Admin Action Repository (SR_GOV_69) ------------------------------------

/// Persistence operations for admin actions (undo support).
/// Implements: SR_GOV_69
#[async_trait]
pub trait AdminActionRepository: Send + Sync {
    /// Record a new admin action.
    async fn record(&self, action: &AdminAction) -> Result<(), PrismError>;

    /// Get an admin action by ID.
    async fn get_by_id(
        &self,
        tenant_id: TenantId,
        action_id: uuid::Uuid,
    ) -> Result<Option<AdminAction>, PrismError>;

    /// Mark an action as undone.
    async fn mark_undone(
        &self,
        tenant_id: TenantId,
        action_id: uuid::Uuid,
    ) -> Result<(), PrismError>;
}

// -- Ruleset Version Repository (SR_GOV_19) ---------------------------------

/// Persistence operations for versioned governance rulesets.
/// Implements: SR_GOV_19
#[async_trait]
pub trait RulesetVersionRepository: Send + Sync {
    /// Store a new ruleset version.
    async fn create(&self, version: &crate::types::RulesetVersion) -> Result<(), PrismError>;

    /// Get the currently active version for a tenant.
    async fn get_active(
        &self,
        tenant_id: TenantId,
    ) -> Result<Option<crate::types::RulesetVersion>, PrismError>;

    /// Get a specific version by ID.
    async fn get_by_id(
        &self,
        tenant_id: TenantId,
        version_id: uuid::Uuid,
    ) -> Result<Option<crate::types::RulesetVersion>, PrismError>;

    /// Promote a version to active (deactivating the current active version).
    async fn promote(&self, tenant_id: TenantId, version_id: uuid::Uuid) -> Result<(), PrismError>;
}

// -- Decision Sample Repository (SR_GOV_19) ---------------------------------

/// Provides recent decision samples for dry-run analysis.
/// Implements: SR_GOV_19
#[async_trait]
pub trait DecisionSampleRepository: Send + Sync {
    /// Get recent rule evaluation decisions for dry-run comparison.
    /// Returns (action, attributes, previous_decision) tuples.
    async fn get_recent_decisions(
        &self,
        tenant_id: TenantId,
        limit: usize,
    ) -> Result<Vec<(String, serde_json::Value, String)>, PrismError>;
}

// -- Connection Consent Repository (SR_GOV_70) --------------------------------

/// Persistence operations for connection consents.
/// Implements: SR_GOV_70
#[async_trait]
pub trait ConnectionConsentRepository: Send + Sync {
    /// Record a new connection consent.
    /// Implements: SR_GOV_70
    async fn record_consent(&self, consent: &ConnectionConsent) -> Result<(), PrismError>;
}

// -- CSA Rule Repository (SR_GOV_23) ------------------------------------------

/// Persistence operations for CSA rules.
/// Implements: SR_GOV_23
#[async_trait]
pub trait CsaRuleRepository: Send + Sync {
    /// Create a new CSA rule.
    /// Implements: SR_GOV_23
    async fn create(&self, rule: &CsaRule) -> Result<(), PrismError>;

    /// List all active CSA rules for a tenant.
    async fn list_active_rules(&self, tenant_id: TenantId) -> Result<Vec<CsaRule>, PrismError>;

    /// Get a CSA rule by ID.
    async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<CsaRule>, PrismError>;
}

// -- Break-Glass Repository (SR_GOV_29) ----------------------------------------

/// Persistence operations for break-glass activations.
/// Implements: SR_GOV_29
#[async_trait]
pub trait BreakGlassRepository: Send + Sync {
    /// Record a break-glass activation.
    /// Implements: SR_GOV_29
    async fn record_activation(
        &self,
        activation: &crate::types::BreakGlassActivation,
    ) -> Result<(), PrismError>;

    /// Get a break-glass activation by its review ID.
    async fn get_by_review_id(
        &self,
        review_id: uuid::Uuid,
    ) -> Result<Option<crate::types::BreakGlassActivation>, PrismError>;

    /// Mark a break-glass activation as reviewed.
    async fn mark_reviewed(&self, review_id: uuid::Uuid) -> Result<(), PrismError>;
}

// -- CSA Assessment Repository (SR_GOV_30) ------------------------------------

/// Persistence operations for CSA assessment records.
/// Implements: SR_GOV_30
#[async_trait]
pub trait CsaAssessmentRepository: Send + Sync {
    /// Persist a CSA assessment record.
    /// Implements: SR_GOV_30
    async fn persist(&self, record: &crate::types::CsaAssessmentRecord) -> Result<(), PrismError>;
}

// -- Connection Status Repository (SR_GOV_76) --------------------------------

/// Persistence operations for checking connection approval and credential status.
/// Implements: SR_GOV_76
#[async_trait]
pub trait ConnectionStatusRepository: Send + Sync {
    /// Check whether a connection is approved for a tenant.
    /// Implements: SR_GOV_76
    async fn is_approved(
        &self,
        tenant_id: TenantId,
        connection_id: &str,
    ) -> Result<bool, PrismError>;

    /// Check whether a connection has a valid credential.
    /// Implements: SR_GOV_76
    async fn has_credential(
        &self,
        tenant_id: TenantId,
        connection_id: &str,
    ) -> Result<bool, PrismError>;
}

// -- Quota Enforcer (SR_GOV_76) -----------------------------------------------

/// Budget and quota enforcement for connection pulls.
/// Implements: SR_GOV_76
#[async_trait]
pub trait QuotaEnforcer: Send + Sync {
    /// Check whether the expected volume is within the budget for this connection.
    /// Returns Ok(true) if within budget, Ok(false) if budget exceeded.
    /// Implements: SR_GOV_76
    async fn check_budget(
        &self,
        tenant_id: TenantId,
        connection_id: &str,
        expected_volume: u64,
    ) -> Result<bool, PrismError>;
}

// -- Component Registry (SR_GOV_78) -------------------------------------------

/// Registry for looking up component metadata during preflight checks.
/// Implements: SR_GOV_78
#[async_trait]
pub trait ComponentRegistry: Send + Sync {
    /// Get component metadata by tenant and component ID.
    /// Returns None if the component does not exist.
    /// Implements: SR_GOV_78
    async fn get_component(
        &self,
        tenant_id: TenantId,
        component_id: &str,
    ) -> Result<Option<ComponentInfo>, PrismError>;
}

// -- Org Tree Repository (SR_GOV_42) ------------------------------------------

/// Persistence operations for the organizational tree (reporting chains).
/// Used by the LCA algorithm to compute approval chains.
/// Implements: SR_GOV_42
#[async_trait]
pub trait OrgTreeRepository: Send + Sync {
    /// Get the reporting chain ancestors for a person up to the root.
    /// Returns an ordered list from direct manager up to the org root.
    /// Implements: SR_GOV_42
    async fn get_ancestors(
        &self,
        tenant_id: TenantId,
        person_id: UserId,
    ) -> Result<Vec<UserId>, PrismError>;
}

// -- Approval Request Repository (SR_GOV_41) ----------------------------------

/// Persistence operations for approval requests.
/// Implements: SR_GOV_41
#[async_trait]
pub trait ApprovalRequestRepository: Send + Sync {
    /// Create a new approval request record.
    /// Implements: SR_GOV_41
    async fn create(&self, request: &ApprovalRequestRecord) -> Result<(), PrismError>;

    /// Get an approval request by ID.
    /// Implements: SR_GOV_41
    async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<ApprovalRequestRecord>, PrismError>;

    /// Update the status and current_index of an approval request.
    /// Implements: SR_GOV_43
    async fn update_status(
        &self,
        id: uuid::Uuid,
        status: ApprovalStatus,
        current_index: usize,
    ) -> Result<(), PrismError>;

    /// List pending approval requests where the given approver is the current approver.
    /// Implements: SR_GOV_44
    async fn list_pending_for_approver(
        &self,
        tenant_id: TenantId,
        approver_id: UserId,
    ) -> Result<Vec<ApprovalRequestRecord>, PrismError>;

    /// Replace an approver in the approvers list for a given approval request.
    /// Implements: SR_GOV_44, SR_GOV_45
    async fn replace_approver(
        &self,
        id: uuid::Uuid,
        old_approver: UserId,
        new_approver: UserId,
    ) -> Result<(), PrismError>;
}

// -- Delegation Repository (SR_GOV_44) ----------------------------------------

/// Persistence operations for delegation records.
/// Implements: SR_GOV_44
#[async_trait]
pub trait DelegationRepository: Send + Sync {
    /// Create a new delegation record.
    /// Implements: SR_GOV_44
    async fn create(&self, delegation: &crate::types::Delegation) -> Result<(), PrismError>;

    /// Get the currently active delegation for a person within a tenant.
    /// Implements: SR_GOV_44
    async fn get_active_delegation(
        &self,
        tenant_id: TenantId,
        from_person: UserId,
    ) -> Result<Option<crate::types::Delegation>, PrismError>;

    /// List all active delegations for a tenant.
    /// Implements: SR_GOV_44
    async fn list_active(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<crate::types::Delegation>, PrismError>;
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
