//! Approval chain lifecycle: create, compute LCA, and execute decisions.
//!
//! - SR_GOV_41: Create approval request with LCA-computed approver chain.
//! - SR_GOV_42: Compute Lowest Common Ancestor approval chain.
//! - SR_GOV_43: Execute approval chain (advance / reject / defer).

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{Duration, Utc};
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::{
    ApprovalRequestRepository, BreakGlassRepository, DelegationRepository, OrgTreeRepository,
};
use prism_core::types::*;

// ===========================================================================
// SR_GOV_42 -- Compute LCA Chain
// ===========================================================================

/// Pure-function LCA computer for determining the approval chain.
///
/// Algorithm (MVP):
/// 1. For each principal, fetch the ancestor chain from the org tree.
/// 2. Find the Lowest Common Ancestor -- the first person that appears
///    in ALL ancestor chains.
/// 3. Return the chain from the lowest principal up to the LCA.
/// 4. Fallback: if no common ancestor, return the union of all
///    first-level managers.
///
/// Implements: SR_GOV_42
pub struct LcaComputer;

impl LcaComputer {
    /// Compute the approval chain for a set of principals.
    ///
    /// Implements: SR_GOV_42
    pub async fn compute(
        principals: &[UserId],
        org_tree: &dyn OrgTreeRepository,
        tenant_id: TenantId,
    ) -> Result<Vec<UserId>, PrismError> {
        if principals.is_empty() {
            return Ok(Vec::new());
        }

        // Collect ancestor chains for each principal
        let mut ancestor_chains: Vec<Vec<UserId>> = Vec::new();
        for principal in principals {
            let ancestors = org_tree.get_ancestors(tenant_id, *principal).await?;
            ancestor_chains.push(ancestors);
        }

        // Single principal: return their direct manager chain
        if ancestor_chains.len() == 1 {
            return Ok(ancestor_chains.into_iter().next().unwrap_or_default());
        }

        // Find LCA: first person appearing in ALL ancestor chains
        if let Some(first_chain) = ancestor_chains.first() {
            for candidate in first_chain {
                let in_all = ancestor_chains
                    .iter()
                    .skip(1)
                    .all(|chain| chain.contains(candidate));
                if in_all {
                    // Build chain from first principal's ancestors up to and including the LCA
                    let mut result = Vec::new();
                    for ancestor in first_chain {
                        result.push(*ancestor);
                        if ancestor == candidate {
                            break;
                        }
                    }
                    return Ok(result);
                }
            }
        }

        // Fallback: union of first-level managers (first ancestor of each principal)
        let mut managers = HashSet::new();
        for chain in &ancestor_chains {
            if let Some(first_manager) = chain.first() {
                managers.insert(*first_manager);
            }
        }
        Ok(managers.into_iter().collect())
    }
}

// ===========================================================================
// SR_GOV_41 -- Create Approval Request
// ===========================================================================

/// SLA deadline computation based on tier.
///
/// Implements: SR_GOV_41
fn compute_sla_deadline(sla_tier: Option<&str>) -> chrono::DateTime<Utc> {
    let now = Utc::now();
    match sla_tier {
        Some("critical") => now + Duration::hours(4),
        Some("urgent") => now + Duration::hours(24),
        _ => now + Duration::days(5),
    }
}

/// Service managing the full approval chain lifecycle.
///
/// Implements: SR_GOV_41, SR_GOV_43
pub struct ApprovalChainService {
    repo: Arc<dyn ApprovalRequestRepository>,
    org_tree: Arc<dyn OrgTreeRepository>,
    audit: AuditLogger,
}

impl ApprovalChainService {
    /// Create a new ApprovalChainService.
    pub fn new(
        repo: Arc<dyn ApprovalRequestRepository>,
        org_tree: Arc<dyn OrgTreeRepository>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            repo,
            org_tree,
            audit,
        }
    }

    /// Create a new approval request.
    ///
    /// Uses LCA compute (SR_GOV_42) to determine the approver chain,
    /// then persists the request with the computed SLA deadline.
    ///
    /// Implements: SR_GOV_41
    pub async fn create_request(
        &self,
        input: &ApprovalCreateRequest,
    ) -> Result<ApprovalRequestResult, PrismError> {
        // Compute approver chain using LCA
        let approvers = LcaComputer::compute(
            &[input.requested_by],
            self.org_tree.as_ref(),
            input.tenant_id,
        )
        .await?;

        let sla_deadline = compute_sla_deadline(input.sla_tier.as_deref());
        let approval_id = uuid::Uuid::now_v7();

        let record = ApprovalRequestRecord {
            id: approval_id,
            tenant_id: input.tenant_id,
            action: input.action.clone(),
            requested_by: input.requested_by,
            payload: input.payload.clone(),
            approvers: approvers.clone(),
            current_index: 0,
            status: ApprovalStatus::Pending,
            sla_deadline,
            created_at: Utc::now(),
        };

        self.repo.create(&record).await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "approval.request_created".into(),
                actor_id: input.requested_by.into_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(approval_id),
                target_type: Some("ApprovalRequest".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "action": input.action,
                    "approver_count": approvers.len(),
                    "sla_tier": input.sla_tier,
                }),
            })
            .await?;

        info!(
            approval_id = %approval_id,
            tenant_id = %input.tenant_id,
            approver_count = approvers.len(),
            "approval request created"
        );

        Ok(ApprovalRequestResult {
            approval_id,
            approvers,
            sla_deadline,
        })
    }

    /// Record an approver's decision and advance the chain.
    ///
    /// - Approve: advance to next approver; mark APPROVED if last.
    /// - Reject: mark entire chain as REJECTED.
    /// - Defer: mark as DEFERRED (mapped to Escalated status).
    ///
    /// Validates that the approver is the current approver and the
    /// request is in PENDING or IN_REVIEW status.
    ///
    /// Implements: SR_GOV_43
    pub async fn record_decision(
        &self,
        input: &ApprovalChainExecution,
    ) -> Result<ApprovalChainResult, PrismError> {
        let record = self
            .repo
            .get_by_id(input.approval_id)
            .await?
            .ok_or(PrismError::NotFound {
                entity_type: "ApprovalRequest",
                id: input.approval_id,
            })?;

        // Validate status
        if record.status != ApprovalStatus::Pending && record.status != ApprovalStatus::InReview {
            return Err(PrismError::Validation {
                reason: format!(
                    "approval request is in {:?} status, expected Pending or InReview",
                    record.status
                ),
            });
        }

        // Validate approver
        let current_approver =
            record
                .approvers
                .get(record.current_index)
                .ok_or(PrismError::Validation {
                    reason: "approval chain has no approver at current index".into(),
                })?;

        if *current_approver != input.approver_id {
            return Err(PrismError::Validation {
                reason: "approver is not the current approver in the chain".into(),
            });
        }

        let (new_status, new_index) = match input.decision {
            ApprovalDecision::Approve => {
                if record.current_index + 1 >= record.approvers.len() {
                    // Last approver -- chain is complete
                    (ApprovalStatus::Approved, record.current_index)
                } else {
                    // Advance to next
                    (ApprovalStatus::InReview, record.current_index + 1)
                }
            }
            ApprovalDecision::Reject => (ApprovalStatus::Rejected, record.current_index),
            ApprovalDecision::Defer => (ApprovalStatus::Escalated, record.current_index),
        };

        self.repo
            .update_status(input.approval_id, new_status, new_index)
            .await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: record.tenant_id,
                event_type: "approval.decision_recorded".into(),
                actor_id: input.approver_id.into_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(input.approval_id),
                target_type: Some("ApprovalRequest".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "decision": format!("{:?}", input.decision),
                    "new_status": format!("{:?}", new_status),
                }),
            })
            .await?;

        info!(
            approval_id = %input.approval_id,
            decision = ?input.decision,
            new_status = ?new_status,
            "approval decision recorded"
        );

        Ok(ApprovalChainResult {
            final_state: new_status,
            decisions: vec![(input.approver_id, input.decision)],
        })
    }
}

// ===========================================================================
// SR_GOV_44 -- Delegation
// ===========================================================================

/// Service for creating approver delegations that re-route in-flight
/// approval requests from one person to another.
///
/// Implements: SR_GOV_44
pub struct DelegationService {
    delegation_repo: Arc<dyn DelegationRepository>,
    approval_repo: Arc<dyn ApprovalRequestRepository>,
    audit: AuditLogger,
}

impl DelegationService {
    /// Create a new DelegationService.
    pub fn new(
        delegation_repo: Arc<dyn DelegationRepository>,
        approval_repo: Arc<dyn ApprovalRequestRepository>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            delegation_repo,
            approval_repo,
            audit,
        }
    }

    /// Create a delegation and re-route in-flight approvals.
    ///
    /// Validates that from_person != to_person, scope is non-empty,
    /// and expires_at is in the future. Then scans pending approvals
    /// where from_person is the current approver and re-routes them
    /// to to_person.
    ///
    /// Implements: SR_GOV_44
    pub async fn delegate(
        &self,
        input: &DelegationRequest,
    ) -> Result<DelegationResult, PrismError> {
        // Validation: self-delegation
        if input.from_person == input.to_person {
            return Err(PrismError::Validation {
                reason: "cannot delegate to yourself".into(),
            });
        }

        // Validation: scope non-empty
        if input.scope.trim().is_empty() {
            return Err(PrismError::Validation {
                reason: "delegation scope must not be empty".into(),
            });
        }

        // Validation: expires_at in the future
        if input.expires_at <= Utc::now() {
            return Err(PrismError::Validation {
                reason: "delegation expiration must be in the future".into(),
            });
        }

        let delegation_id = uuid::Uuid::now_v7();
        let delegation = Delegation {
            id: delegation_id,
            tenant_id: input.tenant_id,
            from_person: input.from_person,
            to_person: input.to_person,
            scope: input.scope.clone(),
            created_at: Utc::now(),
            expires_at: input.expires_at,
            is_active: true,
        };

        self.delegation_repo.create(&delegation).await?;

        // Scan in-flight approvals and re-route
        let pending = self
            .approval_repo
            .list_pending_for_approver(input.tenant_id, input.from_person)
            .await?;

        let mut affected_approvals = Vec::new();
        for record in &pending {
            self.approval_repo
                .replace_approver(record.id, input.from_person, input.to_person)
                .await?;
            affected_approvals.push(record.id);
        }

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "delegation.created".into(),
                actor_id: input.from_person.into_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(delegation_id),
                target_type: Some("Delegation".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "to_person": input.to_person.into_uuid(),
                    "scope": input.scope,
                    "affected_approvals": affected_approvals.len(),
                }),
            })
            .await?;

        info!(
            delegation_id = %delegation_id,
            from = %input.from_person,
            to = %input.to_person,
            affected = affected_approvals.len(),
            "delegation created"
        );

        Ok(DelegationResult {
            delegation_id,
            affected_approvals,
        })
    }
}

// ===========================================================================
// SR_GOV_45 -- SLA Escalation
// ===========================================================================

/// Service for escalating approval requests past their SLA deadline
/// by reassigning the current approver to a new approver.
///
/// Implements: SR_GOV_45
pub struct EscalationService {
    repo: Arc<dyn ApprovalRequestRepository>,
    audit: AuditLogger,
}

impl EscalationService {
    /// Create a new EscalationService.
    pub fn new(repo: Arc<dyn ApprovalRequestRepository>, audit: AuditLogger) -> Self {
        Self { repo, audit }
    }

    /// Escalate an approval request by reassigning the current approver.
    ///
    /// Loads the approval request, verifies the current_approver matches,
    /// replaces the current approver with new_approver, updates status
    /// to Escalated then back to Pending, and sets a new 24-hour SLA deadline.
    ///
    /// Implements: SR_GOV_45
    pub async fn escalate(
        &self,
        input: &EscalationRequest,
    ) -> Result<EscalationResult, PrismError> {
        let record = self
            .repo
            .get_by_id(input.approval_id)
            .await?
            .ok_or(PrismError::NotFound {
                entity_type: "ApprovalRequest",
                id: input.approval_id,
            })?;

        // Validate that current_approver matches
        let actual_approver =
            record
                .approvers
                .get(record.current_index)
                .ok_or(PrismError::Validation {
                    reason: "approval chain has no approver at current index".into(),
                })?;

        if *actual_approver != input.current_approver {
            return Err(PrismError::Validation {
                reason: "current_approver does not match the actual current approver".into(),
            });
        }

        // Replace the current approver with the new approver
        self.repo
            .replace_approver(
                input.approval_id,
                input.current_approver,
                input.new_approver,
            )
            .await?;

        // Briefly escalate then back to Pending with new SLA
        self.repo
            .update_status(
                input.approval_id,
                ApprovalStatus::Escalated,
                record.current_index,
            )
            .await?;
        self.repo
            .update_status(
                input.approval_id,
                ApprovalStatus::Pending,
                record.current_index,
            )
            .await?;

        let new_deadline = Utc::now() + Duration::hours(24);

        // Audit event at HIGH severity
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "approval.escalated".into(),
                actor_id: input.current_approver.into_uuid(),
                actor_type: ActorType::System,
                target_id: Some(input.approval_id),
                target_type: Some("ApprovalRequest".into()),
                severity: Severity::High,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "old_approver": input.current_approver.into_uuid(),
                    "new_approver": input.new_approver.into_uuid(),
                    "new_deadline": new_deadline,
                }),
            })
            .await?;

        info!(
            approval_id = %input.approval_id,
            new_approver = %input.new_approver,
            "approval escalated"
        );

        Ok(EscalationResult {
            reassigned_to: input.new_approver,
            new_deadline,
        })
    }
}

// ===========================================================================
// SR_GOV_46 -- Approval Break-Glass
// ===========================================================================

/// Service for activating and reviewing approval break-glass overrides.
///
/// Requires two-person approval (requested_by != second_approver) and
/// mandatory post-incident review. Default duration is 240 minutes (4 hours)
/// per BP-133.
///
/// Implements: SR_GOV_46
pub struct ApprovalBreakGlassService {
    break_glass_repo: Arc<dyn BreakGlassRepository>,
    audit: AuditLogger,
}

impl ApprovalBreakGlassService {
    /// Create a new ApprovalBreakGlassService.
    pub fn new(break_glass_repo: Arc<dyn BreakGlassRepository>, audit: AuditLogger) -> Self {
        Self {
            break_glass_repo,
            audit,
        }
    }

    /// Activate an approval break-glass override.
    ///
    /// Validates two-person rule (requested_by != second_approver) and
    /// justification length (>= 20 chars). Creates a BreakGlassActivation
    /// record and queues a mandatory review.
    ///
    /// Implements: SR_GOV_46
    pub async fn activate(
        &self,
        input: &ApprovalBreakGlassRequest,
    ) -> Result<ApprovalBreakGlassResult, PrismError> {
        // Validation: two-person rule
        if input.requested_by == input.second_approver {
            return Err(PrismError::Validation {
                reason: "break-glass requires two different persons (two-person rule)".into(),
            });
        }

        // Validation: justification non-empty and >= 20 chars
        if input.justification.trim().len() < 20 {
            return Err(PrismError::Validation {
                reason: "justification must be at least 20 characters".into(),
            });
        }

        // Default duration: 240 minutes (4 hours per BP-133)
        let duration_minutes = input.duration_minutes.unwrap_or(240);
        let now = Utc::now();
        let expires_at = now + Duration::minutes(duration_minutes as i64);
        let review_id = uuid::Uuid::now_v7();
        let activation_id = uuid::Uuid::now_v7();

        let activation = BreakGlassActivation {
            id: activation_id,
            assessment_id: uuid::Uuid::now_v7(), // synthetic assessment for approval context
            tenant_id: input.tenant_id,
            justification: input.justification.clone(),
            approver_1: input.requested_by,
            approver_2: input.second_approver,
            duration_minutes,
            activated_at: now,
            expires_at,
            review_id,
            is_reviewed: false,
        };

        self.break_glass_repo.record_activation(&activation).await?;

        // Audit event at CRITICAL severity
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "approval.break_glass_activated".into(),
                actor_id: input.requested_by.into_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(activation_id),
                target_type: Some("BreakGlassActivation".into()),
                severity: Severity::Critical,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "action": input.action,
                    "second_approver": input.second_approver.into_uuid(),
                    "duration_minutes": duration_minutes,
                    "review_id": review_id,
                }),
            })
            .await?;

        info!(
            activation_id = %activation_id,
            review_id = %review_id,
            duration_minutes = duration_minutes,
            "approval break-glass activated"
        );

        Ok(ApprovalBreakGlassResult {
            authorized: true,
            expires_at,
            review_id,
        })
    }

    /// Review a break-glass activation, deciding whether it was justified.
    ///
    /// Returns follow-up actions based on the decision:
    /// - Unjustified -> security_investigation
    /// - NeedsRuleRefinement -> approval_policy_review
    /// - Justified -> (no follow-ups)
    ///
    /// Implements: SR_GOV_46
    pub async fn review(
        &self,
        input: &ApprovalBreakGlassReviewInput,
    ) -> Result<ApprovalBreakGlassReviewResult, PrismError> {
        // Verify the review exists
        let _activation = self
            .break_glass_repo
            .get_by_review_id(input.review_id)
            .await?
            .ok_or(PrismError::NotFound {
                entity_type: "BreakGlassActivation",
                id: input.review_id,
            })?;

        // Mark as reviewed
        self.break_glass_repo.mark_reviewed(input.review_id).await?;

        // Determine follow-ups based on decision
        let follow_ups = match input.review_decision {
            BreakGlassReviewDecision::Unjustified => {
                vec!["security_investigation".to_string()]
            }
            BreakGlassReviewDecision::NeedsRuleRefinement => {
                vec!["approval_policy_review".to_string()]
            }
            BreakGlassReviewDecision::Justified => Vec::new(),
        };

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "approval.break_glass_reviewed".into(),
                actor_id: input.review_id, // reviewer context
                actor_type: ActorType::Human,
                target_id: Some(input.review_id),
                target_type: Some("BreakGlassActivation".into()),
                severity: Severity::High,
                source_layer: SourceLayer::Governance,
                governance_authority: None,
                payload: serde_json::json!({
                    "decision": format!("{:?}", input.review_decision),
                    "notes": input.notes,
                    "follow_ups": follow_ups,
                }),
            })
            .await?;

        info!(
            review_id = %input.review_id,
            decision = ?input.review_decision,
            "approval break-glass reviewed"
        );

        Ok(ApprovalBreakGlassReviewResult {
            review_decision: input.review_decision,
            follow_ups,
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use prism_core::repository::AuditEventRepository;
    use std::sync::Mutex;

    // -- Mock AuditEventRepository -------------------------------------------

    struct MockAuditRepo {
        events: Mutex<Vec<AuditEvent>>,
    }

    impl MockAuditRepo {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn event_count(&self) -> usize {
            self.events.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl AuditEventRepository for MockAuditRepo {
        async fn append(&self, event: &AuditEvent) -> Result<(), PrismError> {
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }

        async fn get_chain_head(
            &self,
            tenant_id: TenantId,
        ) -> Result<Option<AuditEvent>, PrismError> {
            let events = self.events.lock().unwrap();
            Ok(events
                .iter()
                .filter(|e| e.tenant_id == tenant_id)
                .max_by_key(|e| e.chain_position)
                .cloned())
        }

        async fn query(
            &self,
            _request: &AuditQueryRequest,
        ) -> Result<AuditQueryResult, PrismError> {
            Ok(AuditQueryResult {
                events: Vec::new(),
                next_page_token: None,
                total_count: 0,
            })
        }

        async fn get_chain_segment(
            &self,
            _tenant_id: TenantId,
            _depth: u32,
        ) -> Result<Vec<AuditEvent>, PrismError> {
            Ok(Vec::new())
        }
    }

    // -- Mock OrgTreeRepository (SR_GOV_42) ------------------------------------

    struct MockOrgTree {
        /// Map from person_id to their ancestor chain
        chains: Mutex<Vec<(UserId, Vec<UserId>)>>,
    }

    impl MockOrgTree {
        fn new() -> Self {
            Self {
                chains: Mutex::new(Vec::new()),
            }
        }

        fn add_chain(&self, person: UserId, ancestors: Vec<UserId>) {
            self.chains.lock().unwrap().push((person, ancestors));
        }
    }

    #[async_trait]
    impl OrgTreeRepository for MockOrgTree {
        async fn get_ancestors(
            &self,
            _tenant_id: TenantId,
            person_id: UserId,
        ) -> Result<Vec<UserId>, PrismError> {
            let chains = self.chains.lock().unwrap();
            Ok(chains
                .iter()
                .find(|(p, _)| *p == person_id)
                .map(|(_, c)| c.clone())
                .unwrap_or_default())
        }
    }

    // -- Mock ApprovalRequestRepository (SR_GOV_41) ----------------------------

    struct MockApprovalRepo {
        requests: Mutex<Vec<ApprovalRequestRecord>>,
    }

    impl MockApprovalRepo {
        fn new() -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ApprovalRequestRepository for MockApprovalRepo {
        async fn create(&self, request: &ApprovalRequestRecord) -> Result<(), PrismError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(())
        }

        async fn get_by_id(
            &self,
            id: uuid::Uuid,
        ) -> Result<Option<ApprovalRequestRecord>, PrismError> {
            let requests = self.requests.lock().unwrap();
            Ok(requests.iter().find(|r| r.id == id).cloned())
        }

        async fn update_status(
            &self,
            id: uuid::Uuid,
            status: ApprovalStatus,
            current_index: usize,
        ) -> Result<(), PrismError> {
            let mut requests = self.requests.lock().unwrap();
            if let Some(r) = requests.iter_mut().find(|r| r.id == id) {
                r.status = status;
                r.current_index = current_index;
            }
            Ok(())
        }

        async fn list_pending_for_approver(
            &self,
            tenant_id: TenantId,
            approver_id: UserId,
        ) -> Result<Vec<ApprovalRequestRecord>, PrismError> {
            let requests = self.requests.lock().unwrap();
            Ok(requests
                .iter()
                .filter(|r| {
                    r.tenant_id == tenant_id
                        && r.status == ApprovalStatus::Pending
                        && r.approvers.get(r.current_index) == Some(&approver_id)
                })
                .cloned()
                .collect())
        }

        async fn replace_approver(
            &self,
            id: uuid::Uuid,
            old_approver: UserId,
            new_approver: UserId,
        ) -> Result<(), PrismError> {
            let mut requests = self.requests.lock().unwrap();
            if let Some(r) = requests.iter_mut().find(|r| r.id == id) {
                for a in &mut r.approvers {
                    if *a == old_approver {
                        *a = new_approver;
                    }
                }
            }
            Ok(())
        }
    }

    // -- Helpers --------------------------------------------------------------

    fn make_audit() -> (Arc<MockAuditRepo>, AuditLogger) {
        let audit_repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(audit_repo.clone());
        (audit_repo, logger)
    }

    // -- SR_GOV_42 Tests: LCA Compute ------------------------------------------

    #[tokio::test]
    async fn lca_single_principal_returns_direct_manager() {
        let org_tree = Arc::new(MockOrgTree::new());
        let person = UserId::new();
        let manager = UserId::new();
        org_tree.add_chain(person, vec![manager]);

        let result = LcaComputer::compute(&[person], org_tree.as_ref(), TenantId::new())
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], manager);
    }

    #[tokio::test]
    async fn lca_two_principals_returns_common_ancestor() {
        let org_tree = Arc::new(MockOrgTree::new());
        let person_a = UserId::new();
        let person_b = UserId::new();
        let mgr_a = UserId::new();
        let mgr_b = UserId::new();
        let common_ancestor = UserId::new();

        org_tree.add_chain(person_a, vec![mgr_a, common_ancestor]);
        org_tree.add_chain(person_b, vec![mgr_b, common_ancestor]);

        let result =
            LcaComputer::compute(&[person_a, person_b], org_tree.as_ref(), TenantId::new())
                .await
                .unwrap();

        // Should contain the chain from person_a's ancestors up to and including the LCA
        assert!(result.contains(&common_ancestor));
    }

    #[tokio::test]
    async fn lca_no_common_ancestor_returns_managers() {
        let org_tree = Arc::new(MockOrgTree::new());
        let person_a = UserId::new();
        let person_b = UserId::new();
        let mgr_a = UserId::new();
        let mgr_b = UserId::new();

        org_tree.add_chain(person_a, vec![mgr_a]);
        org_tree.add_chain(person_b, vec![mgr_b]);

        let result =
            LcaComputer::compute(&[person_a, person_b], org_tree.as_ref(), TenantId::new())
                .await
                .unwrap();

        // Fallback: union of first-level managers
        assert_eq!(result.len(), 2);
        assert!(result.contains(&mgr_a));
        assert!(result.contains(&mgr_b));
    }

    #[tokio::test]
    async fn lca_empty_principals_returns_empty() {
        let org_tree = Arc::new(MockOrgTree::new());
        let result = LcaComputer::compute(&[], org_tree.as_ref(), TenantId::new())
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    // -- SR_GOV_41 Tests: Create Approval Request --------------------------------

    #[tokio::test]
    async fn create_request_with_approvers() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let org_tree = Arc::new(MockOrgTree::new());
        let requester = UserId::new();
        let manager = UserId::new();
        org_tree.add_chain(requester, vec![manager]);

        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalChainService::new(approval_repo.clone(), org_tree, audit);

        let input = ApprovalCreateRequest {
            tenant_id: TenantId::new(),
            action: "automation.activate".into(),
            requested_by: requester,
            payload: serde_json::json!({"automation_id": "abc"}),
            sla_tier: None,
        };

        let result = svc.create_request(&input).await.unwrap();
        assert!(!result.approvers.is_empty());
        assert_eq!(result.approvers[0], manager);
    }

    #[tokio::test]
    async fn create_request_urgent_sla_deadline() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let org_tree = Arc::new(MockOrgTree::new());
        let requester = UserId::new();
        org_tree.add_chain(requester, vec![UserId::new()]);

        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalChainService::new(approval_repo, org_tree, audit);

        let input = ApprovalCreateRequest {
            tenant_id: TenantId::new(),
            action: "automation.activate".into(),
            requested_by: requester,
            payload: serde_json::json!({}),
            sla_tier: Some("urgent".into()),
        };

        let before = Utc::now();
        let result = svc.create_request(&input).await.unwrap();
        let after = Utc::now();

        // Urgent tier = 24 hours
        let expected_min = before + Duration::hours(24);
        let expected_max = after + Duration::hours(24);
        assert!(result.sla_deadline >= expected_min);
        assert!(result.sla_deadline <= expected_max);
    }

    #[tokio::test]
    async fn create_request_emits_audit_event() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let org_tree = Arc::new(MockOrgTree::new());
        let requester = UserId::new();
        org_tree.add_chain(requester, vec![UserId::new()]);

        let (audit_repo, audit) = make_audit();
        let svc = ApprovalChainService::new(approval_repo, org_tree, audit);

        let input = ApprovalCreateRequest {
            tenant_id: TenantId::new(),
            action: "automation.activate".into(),
            requested_by: requester,
            payload: serde_json::json!({}),
            sla_tier: None,
        };

        svc.create_request(&input).await.unwrap();
        assert_eq!(audit_repo.event_count(), 1);
    }

    // -- SR_GOV_43 Tests: Execute Approval Chain ---------------------------------

    #[tokio::test]
    async fn approve_advances_to_next_approver() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let org_tree = Arc::new(MockOrgTree::new());
        let requester = UserId::new();
        let approver_1 = UserId::new();
        let approver_2 = UserId::new();
        org_tree.add_chain(requester, vec![approver_1, approver_2]);

        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalChainService::new(approval_repo.clone(), org_tree, audit);

        let input = ApprovalCreateRequest {
            tenant_id: TenantId::new(),
            action: "automation.activate".into(),
            requested_by: requester,
            payload: serde_json::json!({}),
            sla_tier: None,
        };

        let created = svc.create_request(&input).await.unwrap();

        let exec = ApprovalChainExecution {
            approval_id: created.approval_id,
            approver_id: approver_1,
            decision: ApprovalDecision::Approve,
        };

        let result = svc.record_decision(&exec).await.unwrap();
        assert_eq!(result.final_state, ApprovalStatus::InReview);
    }

    #[tokio::test]
    async fn approve_on_last_completes_chain() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let org_tree = Arc::new(MockOrgTree::new());
        let requester = UserId::new();
        let approver = UserId::new();
        org_tree.add_chain(requester, vec![approver]);

        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalChainService::new(approval_repo.clone(), org_tree, audit);

        let input = ApprovalCreateRequest {
            tenant_id: TenantId::new(),
            action: "automation.activate".into(),
            requested_by: requester,
            payload: serde_json::json!({}),
            sla_tier: None,
        };

        let created = svc.create_request(&input).await.unwrap();

        let exec = ApprovalChainExecution {
            approval_id: created.approval_id,
            approver_id: approver,
            decision: ApprovalDecision::Approve,
        };

        let result = svc.record_decision(&exec).await.unwrap();
        assert_eq!(result.final_state, ApprovalStatus::Approved);
    }

    #[tokio::test]
    async fn reject_terminates_chain() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let org_tree = Arc::new(MockOrgTree::new());
        let requester = UserId::new();
        let approver_1 = UserId::new();
        let approver_2 = UserId::new();
        org_tree.add_chain(requester, vec![approver_1, approver_2]);

        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalChainService::new(approval_repo.clone(), org_tree, audit);

        let input = ApprovalCreateRequest {
            tenant_id: TenantId::new(),
            action: "automation.activate".into(),
            requested_by: requester,
            payload: serde_json::json!({}),
            sla_tier: None,
        };

        let created = svc.create_request(&input).await.unwrap();

        let exec = ApprovalChainExecution {
            approval_id: created.approval_id,
            approver_id: approver_1,
            decision: ApprovalDecision::Reject,
        };

        let result = svc.record_decision(&exec).await.unwrap();
        assert_eq!(result.final_state, ApprovalStatus::Rejected);
    }

    #[tokio::test]
    async fn wrong_approver_rejected() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let org_tree = Arc::new(MockOrgTree::new());
        let requester = UserId::new();
        let approver = UserId::new();
        let wrong_person = UserId::new();
        org_tree.add_chain(requester, vec![approver]);

        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalChainService::new(approval_repo.clone(), org_tree, audit);

        let input = ApprovalCreateRequest {
            tenant_id: TenantId::new(),
            action: "automation.activate".into(),
            requested_by: requester,
            payload: serde_json::json!({}),
            sla_tier: None,
        };

        let created = svc.create_request(&input).await.unwrap();

        let exec = ApprovalChainExecution {
            approval_id: created.approval_id,
            approver_id: wrong_person,
            decision: ApprovalDecision::Approve,
        };

        let result = svc.record_decision(&exec).await;
        assert!(result.is_err());
    }

    // -- Mock DelegationRepository (SR_GOV_44) ----------------------------------

    struct MockDelegationRepo {
        delegations: Mutex<Vec<Delegation>>,
    }

    impl MockDelegationRepo {
        fn new() -> Self {
            Self {
                delegations: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl prism_core::repository::DelegationRepository for MockDelegationRepo {
        async fn create(&self, delegation: &Delegation) -> Result<(), PrismError> {
            self.delegations.lock().unwrap().push(delegation.clone());
            Ok(())
        }

        async fn get_active_delegation(
            &self,
            tenant_id: TenantId,
            from_person: UserId,
        ) -> Result<Option<Delegation>, PrismError> {
            let delegations = self.delegations.lock().unwrap();
            Ok(delegations
                .iter()
                .find(|d| d.tenant_id == tenant_id && d.from_person == from_person && d.is_active)
                .cloned())
        }

        async fn list_active(&self, tenant_id: TenantId) -> Result<Vec<Delegation>, PrismError> {
            let delegations = self.delegations.lock().unwrap();
            Ok(delegations
                .iter()
                .filter(|d| d.tenant_id == tenant_id && d.is_active)
                .cloned()
                .collect())
        }
    }

    // -- Mock BreakGlassRepository (SR_GOV_46) ----------------------------------

    struct MockBreakGlassRepo {
        activations: Mutex<Vec<BreakGlassActivation>>,
    }

    impl MockBreakGlassRepo {
        fn new() -> Self {
            Self {
                activations: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl prism_core::repository::BreakGlassRepository for MockBreakGlassRepo {
        async fn record_activation(
            &self,
            activation: &BreakGlassActivation,
        ) -> Result<(), PrismError> {
            self.activations.lock().unwrap().push(activation.clone());
            Ok(())
        }

        async fn get_by_review_id(
            &self,
            review_id: uuid::Uuid,
        ) -> Result<Option<BreakGlassActivation>, PrismError> {
            let activations = self.activations.lock().unwrap();
            Ok(activations
                .iter()
                .find(|a| a.review_id == review_id)
                .cloned())
        }

        async fn mark_reviewed(&self, review_id: uuid::Uuid) -> Result<(), PrismError> {
            let mut activations = self.activations.lock().unwrap();
            if let Some(a) = activations.iter_mut().find(|a| a.review_id == review_id) {
                a.is_reviewed = true;
            }
            Ok(())
        }
    }

    // -- SR_GOV_44 Tests: Delegation -------------------------------------------

    #[tokio::test]
    async fn delegation_succeeds() {
        let delegation_repo = Arc::new(MockDelegationRepo::new());
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let (_audit_repo, audit) = make_audit();

        let svc = DelegationService::new(delegation_repo.clone(), approval_repo, audit);

        let tenant = TenantId::new();
        let from = UserId::new();
        let to = UserId::new();

        let input = DelegationRequest {
            tenant_id: tenant,
            from_person: from,
            to_person: to,
            scope: "approval.all".into(),
            expires_at: Utc::now() + Duration::hours(48),
        };

        let result = svc.delegate(&input).await.unwrap();
        assert_ne!(result.delegation_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn delegation_rejects_self_delegation() {
        let delegation_repo = Arc::new(MockDelegationRepo::new());
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let (_audit_repo, audit) = make_audit();

        let svc = DelegationService::new(delegation_repo, approval_repo, audit);

        let person = UserId::new();
        let input = DelegationRequest {
            tenant_id: TenantId::new(),
            from_person: person,
            to_person: person,
            scope: "approval.all".into(),
            expires_at: Utc::now() + Duration::hours(48),
        };

        let result = svc.delegate(&input).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("yourself"));
    }

    #[tokio::test]
    async fn delegation_rejects_empty_scope() {
        let delegation_repo = Arc::new(MockDelegationRepo::new());
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let (_audit_repo, audit) = make_audit();

        let svc = DelegationService::new(delegation_repo, approval_repo, audit);

        let input = DelegationRequest {
            tenant_id: TenantId::new(),
            from_person: UserId::new(),
            to_person: UserId::new(),
            scope: "   ".into(),
            expires_at: Utc::now() + Duration::hours(48),
        };

        let result = svc.delegate(&input).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("scope"));
    }

    #[tokio::test]
    async fn delegation_rejects_past_expiration() {
        let delegation_repo = Arc::new(MockDelegationRepo::new());
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let (_audit_repo, audit) = make_audit();

        let svc = DelegationService::new(delegation_repo, approval_repo, audit);

        let input = DelegationRequest {
            tenant_id: TenantId::new(),
            from_person: UserId::new(),
            to_person: UserId::new(),
            scope: "approval.all".into(),
            expires_at: Utc::now() - Duration::hours(1),
        };

        let result = svc.delegate(&input).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("future"));
    }

    // -- SR_GOV_45 Tests: Escalation -------------------------------------------

    #[tokio::test]
    async fn escalation_succeeds() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let tenant = TenantId::new();
        let current_approver = UserId::new();
        let new_approver = UserId::new();
        let approval_id = uuid::Uuid::now_v7();

        // Seed a pending approval request
        let record = ApprovalRequestRecord {
            id: approval_id,
            tenant_id: tenant,
            action: "automation.activate".into(),
            requested_by: UserId::new(),
            payload: serde_json::json!({}),
            approvers: vec![current_approver],
            current_index: 0,
            status: ApprovalStatus::Pending,
            sla_deadline: Utc::now() - Duration::hours(1), // past deadline
            created_at: Utc::now(),
        };
        approval_repo.create(&record).await.unwrap();

        let (_audit_repo, audit) = make_audit();
        let svc = EscalationService::new(approval_repo, audit);

        let input = EscalationRequest {
            tenant_id: tenant,
            approval_id,
            current_approver,
            new_approver,
        };

        let result = svc.escalate(&input).await.unwrap();
        assert_eq!(result.reassigned_to, new_approver);
        assert!(result.new_deadline > Utc::now());
    }

    #[tokio::test]
    async fn escalation_rejects_nonexistent_approval() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = EscalationService::new(approval_repo, audit);

        let input = EscalationRequest {
            tenant_id: TenantId::new(),
            approval_id: uuid::Uuid::now_v7(),
            current_approver: UserId::new(),
            new_approver: UserId::new(),
        };

        let result = svc.escalate(&input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn escalation_rejects_wrong_current_approver() {
        let approval_repo = Arc::new(MockApprovalRepo::new());
        let tenant = TenantId::new();
        let real_approver = UserId::new();
        let wrong_approver = UserId::new();
        let approval_id = uuid::Uuid::now_v7();

        let record = ApprovalRequestRecord {
            id: approval_id,
            tenant_id: tenant,
            action: "automation.activate".into(),
            requested_by: UserId::new(),
            payload: serde_json::json!({}),
            approvers: vec![real_approver],
            current_index: 0,
            status: ApprovalStatus::Pending,
            sla_deadline: Utc::now(),
            created_at: Utc::now(),
        };
        approval_repo.create(&record).await.unwrap();

        let (_audit_repo, audit) = make_audit();
        let svc = EscalationService::new(approval_repo, audit);

        let input = EscalationRequest {
            tenant_id: tenant,
            approval_id,
            current_approver: wrong_approver,
            new_approver: UserId::new(),
        };

        let result = svc.escalate(&input).await;
        assert!(result.is_err());
    }

    // -- SR_GOV_46 Tests: Approval Break-Glass ---------------------------------

    #[tokio::test]
    async fn break_glass_activation_succeeds() {
        let bg_repo = Arc::new(MockBreakGlassRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalBreakGlassService::new(bg_repo, audit);

        let input = ApprovalBreakGlassRequest {
            tenant_id: TenantId::new(),
            action: "override.approval".into(),
            requested_by: UserId::new(),
            justification: "Emergency production incident requires immediate action for safety"
                .into(),
            second_approver: UserId::new(),
            duration_minutes: None,
        };

        let result = svc.activate(&input).await.unwrap();
        assert!(result.authorized);
        // Default duration is 240 minutes (4 hours)
        let expected_min = Utc::now() + Duration::minutes(239);
        assert!(result.expires_at > expected_min);
    }

    #[tokio::test]
    async fn break_glass_custom_duration() {
        let bg_repo = Arc::new(MockBreakGlassRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalBreakGlassService::new(bg_repo, audit);

        let input = ApprovalBreakGlassRequest {
            tenant_id: TenantId::new(),
            action: "override.approval".into(),
            requested_by: UserId::new(),
            justification: "Emergency production incident requires immediate action for safety"
                .into(),
            second_approver: UserId::new(),
            duration_minutes: Some(60),
        };

        let before = Utc::now();
        let result = svc.activate(&input).await.unwrap();
        let after = Utc::now();

        // Custom 60-minute duration
        let expected_min = before + Duration::minutes(60);
        let expected_max = after + Duration::minutes(60);
        assert!(result.expires_at >= expected_min);
        assert!(result.expires_at <= expected_max);
    }

    #[tokio::test]
    async fn break_glass_rejects_same_person() {
        let bg_repo = Arc::new(MockBreakGlassRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalBreakGlassService::new(bg_repo, audit);

        let person = UserId::new();
        let input = ApprovalBreakGlassRequest {
            tenant_id: TenantId::new(),
            action: "override.approval".into(),
            requested_by: person,
            justification: "Emergency production incident requires immediate action for safety"
                .into(),
            second_approver: person,
            duration_minutes: None,
        };

        let result = svc.activate(&input).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("two"));
    }

    #[tokio::test]
    async fn break_glass_rejects_short_justification() {
        let bg_repo = Arc::new(MockBreakGlassRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalBreakGlassService::new(bg_repo, audit);

        let input = ApprovalBreakGlassRequest {
            tenant_id: TenantId::new(),
            action: "override.approval".into(),
            requested_by: UserId::new(),
            justification: "too short".into(),
            second_approver: UserId::new(),
            duration_minutes: None,
        };

        let result = svc.activate(&input).await;
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("20 characters"));
    }

    // -- SR_GOV_46 Tests: Approval Break-Glass Review --------------------------

    #[tokio::test]
    async fn break_glass_review_justified() {
        let bg_repo = Arc::new(MockBreakGlassRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalBreakGlassService::new(bg_repo.clone(), audit);

        // First activate to create a review record
        let activate_input = ApprovalBreakGlassRequest {
            tenant_id: TenantId::new(),
            action: "override.approval".into(),
            requested_by: UserId::new(),
            justification: "Emergency production incident requires immediate action for safety"
                .into(),
            second_approver: UserId::new(),
            duration_minutes: None,
        };
        let activation = svc.activate(&activate_input).await.unwrap();

        let review_input = ApprovalBreakGlassReviewInput {
            tenant_id: activate_input.tenant_id,
            review_id: activation.review_id,
            review_decision: BreakGlassReviewDecision::Justified,
            notes: "Confirmed emergency".into(),
        };

        let result = svc.review(&review_input).await.unwrap();
        assert_eq!(result.review_decision, BreakGlassReviewDecision::Justified);
        assert!(result.follow_ups.is_empty());
    }

    #[tokio::test]
    async fn break_glass_review_unjustified_triggers_investigation() {
        let bg_repo = Arc::new(MockBreakGlassRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalBreakGlassService::new(bg_repo.clone(), audit);

        let activate_input = ApprovalBreakGlassRequest {
            tenant_id: TenantId::new(),
            action: "override.approval".into(),
            requested_by: UserId::new(),
            justification: "Emergency production incident requires immediate action for safety"
                .into(),
            second_approver: UserId::new(),
            duration_minutes: None,
        };
        let activation = svc.activate(&activate_input).await.unwrap();

        let review_input = ApprovalBreakGlassReviewInput {
            tenant_id: activate_input.tenant_id,
            review_id: activation.review_id,
            review_decision: BreakGlassReviewDecision::Unjustified,
            notes: "Not a real emergency".into(),
        };

        let result = svc.review(&review_input).await.unwrap();
        assert_eq!(
            result.review_decision,
            BreakGlassReviewDecision::Unjustified
        );
        assert_eq!(result.follow_ups, vec!["security_investigation"]);
    }

    #[tokio::test]
    async fn break_glass_review_needs_refinement() {
        let bg_repo = Arc::new(MockBreakGlassRepo::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ApprovalBreakGlassService::new(bg_repo.clone(), audit);

        let activate_input = ApprovalBreakGlassRequest {
            tenant_id: TenantId::new(),
            action: "override.approval".into(),
            requested_by: UserId::new(),
            justification: "Emergency production incident requires immediate action for safety"
                .into(),
            second_approver: UserId::new(),
            duration_minutes: None,
        };
        let activation = svc.activate(&activate_input).await.unwrap();

        let review_input = ApprovalBreakGlassReviewInput {
            tenant_id: activate_input.tenant_id,
            review_id: activation.review_id,
            review_decision: BreakGlassReviewDecision::NeedsRuleRefinement,
            notes: "Rule was too restrictive for this scenario".into(),
        };

        let result = svc.review(&review_input).await.unwrap();
        assert_eq!(
            result.review_decision,
            BreakGlassReviewDecision::NeedsRuleRefinement
        );
        assert_eq!(result.follow_ups, vec!["approval_policy_review"]);
    }
}
