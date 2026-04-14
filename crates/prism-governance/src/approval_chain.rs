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
use prism_core::repository::{ApprovalRequestRepository, OrgTreeRepository};
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
}
