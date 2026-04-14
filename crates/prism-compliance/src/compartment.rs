//! Visibility compartment engine (GAP-77).
//!
//! Compartments isolate data by classification level and enforce
//! explicit membership for access. Criminal-penalty compartments
//! override the default "visibility flows up" model -- even executives
//! cannot see data without explicit membership.
//!
//! Implements: SR_GOV_31 (create), SR_GOV_32 (add member), SR_GOV_33 (access check),
//!             SR_GOV_34 (revoke member)

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tracing::{info, warn};

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::repository::CompartmentRepository;
use prism_core::types::*;

// ---------------------------------------------------------------------------
// Trait: CompartmentReportSigner
// ---------------------------------------------------------------------------

/// Signs compartment audit report payloads.
///
/// Similar to `ExportSigner` in prism-audit but scoped to compartment reports.
///
/// Implements: SR_GOV_36
#[async_trait]
pub trait CompartmentReportSigner: Send + Sync {
    /// Sign the given payload bytes and return a hex-encoded signature.
    async fn sign(&self, payload: &[u8]) -> Result<String, PrismError>;
}

// ---------------------------------------------------------------------------
// Trait: SessionTerminator
// ---------------------------------------------------------------------------

/// Terminates active sessions that are exposing data from a compartment.
///
/// When membership is revoked, any sessions the revoked principal has open
/// that touch compartment-bound data must be terminated immediately to
/// prevent lingering access.
///
/// Implements: SR_GOV_34 (session termination on revocation)
#[async_trait]
pub trait SessionTerminator: Send + Sync {
    /// Terminate all sessions for the given principal (person or role members)
    /// that are currently accessing data from the specified compartment.
    /// Returns the number of sessions terminated.
    async fn terminate_compartment_sessions(
        &self,
        tenant_id: TenantId,
        compartment_id: CompartmentId,
        person_id: Option<UserId>,
        role_id: Option<RoleId>,
    ) -> Result<u64, PrismError>;
}

/// Service for managing visibility compartments.
///
/// Composes:
/// - `CompartmentRepository` -- persistence for compartments and membership
/// - `AuditLogger` -- audit trail for all compartment operations
/// - `SessionTerminator` -- terminates sessions on revocation (SR_GOV_34)
/// - `CompartmentReportSigner` -- signs compartment audit reports (SR_GOV_36)
///
/// Implements: SR_GOV_31, SR_GOV_32, SR_GOV_33, SR_GOV_34, SR_GOV_35, SR_GOV_36
pub struct CompartmentService {
    repo: Arc<dyn CompartmentRepository>,
    audit: AuditLogger,
    session_terminator: Option<Arc<dyn SessionTerminator>>,
    report_signer: Option<Arc<dyn CompartmentReportSigner>>,
}

impl CompartmentService {
    /// Create a new compartment service.
    pub fn new(repo: Arc<dyn CompartmentRepository>, audit: AuditLogger) -> Self {
        Self {
            repo,
            audit,
            session_terminator: None,
            report_signer: None,
        }
    }

    /// Create a new compartment service with session termination support.
    pub fn with_session_terminator(
        repo: Arc<dyn CompartmentRepository>,
        audit: AuditLogger,
        session_terminator: Arc<dyn SessionTerminator>,
    ) -> Self {
        Self {
            repo,
            audit,
            session_terminator: Some(session_terminator),
            report_signer: None,
        }
    }

    /// Create a new compartment service with report signing support.
    pub fn with_report_signer(
        repo: Arc<dyn CompartmentRepository>,
        audit: AuditLogger,
        report_signer: Arc<dyn CompartmentReportSigner>,
    ) -> Self {
        Self {
            repo,
            audit,
            session_terminator: None,
            report_signer: Some(report_signer),
        }
    }

    /// Create a visibility compartment with initial members.
    ///
    /// Validates:
    /// - Name is non-empty
    /// - Purpose is non-empty
    /// - Criminal-penalty isolation requires Restricted or CriminalPenalty classification
    /// - At least one initial member (person or role) is provided
    ///
    /// Implements: SR_GOV_31
    pub async fn create(
        &self,
        request: &CompartmentCreateRequest,
    ) -> Result<CompartmentCreateResult, PrismError> {
        // Validation
        let name = request.name.trim();
        if name.is_empty() {
            return Err(PrismError::Validation {
                reason: "compartment name cannot be empty".into(),
            });
        }

        if request.purpose.trim().is_empty() {
            return Err(PrismError::Validation {
                reason: "compartment purpose cannot be empty".into(),
            });
        }

        // SR_GOV_31_BE-01: criminal_penalty_isolation requires appropriate classification
        if request.criminal_penalty_isolation
            && request.classification_level != ClassificationLevel::Restricted
            && request.classification_level != ClassificationLevel::CriminalPenalty
        {
            return Err(PrismError::Validation {
                reason: "criminal penalty isolation requires Restricted or CriminalPenalty classification level".into(),
            });
        }

        if request.member_persons.is_empty() && request.member_roles.is_empty() {
            return Err(PrismError::Validation {
                reason: "compartment must have at least one initial member (person or role)".into(),
            });
        }

        let now = Utc::now();
        let compartment_id = CompartmentId::new();

        let compartment = Compartment {
            id: compartment_id,
            tenant_id: request.tenant_id,
            name: name.to_string(),
            classification_level: request.classification_level,
            purpose: request.purpose.clone(),
            criminal_penalty_isolation: request.criminal_penalty_isolation,
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        self.repo.create(&compartment).await?;

        // Add initial person members
        let mut member_count = 0;
        for person_id in &request.member_persons {
            let membership = CompartmentMembership {
                compartment_id,
                tenant_id: request.tenant_id,
                person_id: Some(*person_id),
                role_id: None,
                added_at: now,
            };
            self.repo.add_member(&membership).await?;
            member_count += 1;
        }

        // Add initial role members
        for role_id in &request.member_roles {
            let membership = CompartmentMembership {
                compartment_id,
                tenant_id: request.tenant_id,
                person_id: None,
                role_id: Some(*role_id),
                added_at: now,
            };
            self.repo.add_member(&membership).await?;
            member_count += 1;
        }

        // Audit trail
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "compartment.created".into(),
                actor_id: uuid::Uuid::nil(), // caller provides via context in real usage
                actor_type: ActorType::Human,
                target_id: Some(*compartment_id.as_uuid()),
                target_type: Some("Compartment".into()),
                severity: if request.criminal_penalty_isolation {
                    Severity::High
                } else {
                    Severity::Medium
                },
                source_layer: SourceLayer::Compliance,
                governance_authority: None,
                payload: serde_json::json!({
                    "name": name,
                    "classification_level": request.classification_level,
                    "criminal_penalty_isolation": request.criminal_penalty_isolation,
                    "initial_member_count": member_count,
                }),
            })
            .await?;

        info!(
            compartment_id = %compartment_id,
            tenant_id = %request.tenant_id,
            criminal_penalty = request.criminal_penalty_isolation,
            member_count,
            "compartment created"
        );

        Ok(CompartmentCreateResult {
            compartment_id,
            member_count,
            created_at: now,
        })
    }

    /// Add a person or role to a compartment.
    ///
    /// Validates:
    /// - Exactly one of person_id or role_id is provided
    /// - Compartment exists and belongs to the same tenant
    ///
    /// Implements: SR_GOV_32
    pub async fn add_member(
        &self,
        request: &CompartmentMembershipAddRequest,
    ) -> Result<CompartmentMembershipResult, PrismError> {
        // Exactly one of person_id or role_id
        match (&request.person_id, &request.role_id) {
            (None, None) => {
                return Err(PrismError::Validation {
                    reason: "exactly one of person_id or role_id must be provided".into(),
                });
            }
            (Some(_), Some(_)) => {
                return Err(PrismError::Validation {
                    reason: "provide either person_id or role_id, not both".into(),
                });
            }
            _ => {}
        }

        // Verify compartment exists and belongs to tenant
        let compartment = self
            .repo
            .get_by_id(request.tenant_id, request.compartment_id)
            .await?
            .ok_or_else(|| PrismError::NotFound {
                entity_type: "Compartment",
                id: *request.compartment_id.as_uuid(),
            })?;

        if !compartment.is_active {
            return Err(PrismError::Validation {
                reason: "cannot add members to an inactive compartment".into(),
            });
        }

        let membership = CompartmentMembership {
            compartment_id: request.compartment_id,
            tenant_id: request.tenant_id,
            person_id: request.person_id,
            role_id: request.role_id,
            added_at: Utc::now(),
        };

        let added = self.repo.add_member(&membership).await?;

        // Audit trail
        let target_desc = if let Some(pid) = request.person_id {
            format!("person:{pid}")
        } else {
            format!("role:{}", request.role_id.unwrap())
        };

        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "compartment.member_added".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::Human,
                target_id: Some(*request.compartment_id.as_uuid()),
                target_type: Some("Compartment".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Compliance,
                governance_authority: None,
                payload: serde_json::json!({
                    "member": target_desc,
                    "already_member": !added,
                }),
            })
            .await?;

        Ok(CompartmentMembershipResult {
            compartment_id: request.compartment_id,
            added,
        })
    }

    /// Check whether a principal can access a compartment-bound resource.
    ///
    /// The principal must be a member of ALL compartments the resource belongs to.
    /// Membership is checked both by person_id and by any of the principal's roles.
    ///
    /// Implements: SR_GOV_33
    pub async fn check_access(
        &self,
        request: &CompartmentAccessCheckRequest,
    ) -> Result<CompartmentAccessCheckResult, PrismError> {
        if request.resource_compartments.is_empty() {
            // Resource is not compartment-bound -- allow by default
            return Ok(CompartmentAccessCheckResult {
                decision: AccessDecision::Allow,
                denied_compartments: Vec::new(),
                reason: None,
            });
        }

        let mut denied = Vec::new();

        for &compartment_id in &request.resource_compartments {
            let is_member = self
                .repo
                .is_member(
                    request.tenant_id,
                    compartment_id,
                    request.principal_id,
                    &request.principal_roles,
                )
                .await?;

            if !is_member {
                denied.push(compartment_id);
            }
        }

        let decision = if denied.is_empty() {
            AccessDecision::Allow
        } else {
            AccessDecision::Deny
        };

        let reason = if denied.is_empty() {
            None
        } else {
            Some(format!(
                "principal is not a member of {} required compartment(s)",
                denied.len()
            ))
        };

        Ok(CompartmentAccessCheckResult {
            decision,
            denied_compartments: denied,
            reason,
        })
    }

    /// Revoke compartment membership for a person or role.
    ///
    /// Validates:
    /// - Exactly one of person_id or role_id is provided
    /// - Compartment exists and belongs to the same tenant
    ///
    /// On successful revocation, terminates any active sessions that are
    /// exposing data from the compartment for the revoked principal.
    ///
    /// Implements: SR_GOV_34
    pub async fn revoke_member(
        &self,
        request: &CompartmentMembershipRemoveRequest,
    ) -> Result<CompartmentMembershipRemoveResult, PrismError> {
        // Exactly one of person_id or role_id
        match (&request.person_id, &request.role_id) {
            (None, None) => {
                return Err(PrismError::Validation {
                    reason: "exactly one of person_id or role_id must be provided".into(),
                });
            }
            (Some(_), Some(_)) => {
                return Err(PrismError::Validation {
                    reason: "provide either person_id or role_id, not both".into(),
                });
            }
            _ => {}
        }

        // Verify compartment exists and belongs to tenant
        let _compartment = self
            .repo
            .get_by_id(request.tenant_id, request.compartment_id)
            .await?
            .ok_or_else(|| PrismError::NotFound {
                entity_type: "Compartment",
                id: *request.compartment_id.as_uuid(),
            })?;

        // Remove the membership
        let removed = self
            .repo
            .remove_member(
                request.tenant_id,
                request.compartment_id,
                request.person_id,
                request.role_id,
            )
            .await?;

        // Terminate active sessions exposing compartment-bound data
        let sessions_terminated = if removed {
            if let Some(ref terminator) = self.session_terminator {
                terminator
                    .terminate_compartment_sessions(
                        request.tenant_id,
                        request.compartment_id,
                        request.person_id,
                        request.role_id,
                    )
                    .await?
            } else {
                0
            }
        } else {
            0
        };

        // Audit trail
        let target_desc = if let Some(pid) = request.person_id {
            format!("person:{pid}")
        } else {
            format!("role:{}", request.role_id.unwrap())
        };

        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "compartment.member_removed".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::Human,
                target_id: Some(*request.compartment_id.as_uuid()),
                target_type: Some("Compartment".into()),
                severity: Severity::High,
                source_layer: SourceLayer::Compliance,
                governance_authority: None,
                payload: serde_json::json!({
                    "member": target_desc,
                    "removed": removed,
                    "sessions_terminated": sessions_terminated,
                }),
            })
            .await?;

        if removed {
            warn!(
                compartment_id = %request.compartment_id,
                tenant_id = %request.tenant_id,
                member = %target_desc,
                sessions_terminated,
                "compartment membership revoked"
            );
        } else {
            info!(
                compartment_id = %request.compartment_id,
                tenant_id = %request.tenant_id,
                member = %target_desc,
                "compartment membership revocation: member not found"
            );
        }

        Ok(CompartmentMembershipRemoveResult {
            compartment_id: request.compartment_id,
            removed,
            sessions_terminated,
        })
    }

    /// Check criminal-penalty visibility override.
    ///
    /// For criminal-penalty compartments: DENY any principal not explicitly
    /// listed as a member, regardless of their position in the org tree.
    /// Even if the principal_chain includes executives or org-tree ancestors,
    /// they are ignored -- only direct membership counts.
    ///
    /// For non-criminal-penalty compartments: ALLOW (fallback to normal
    /// access check via `check_access`).
    ///
    /// Implements: SR_GOV_35
    pub async fn check_criminal_penalty_override(
        &self,
        request: &CriminalPenaltyOverrideCheck,
    ) -> Result<CriminalPenaltyOverrideResult, PrismError> {
        // Load the compartment
        let compartment = self
            .repo
            .get_by_id(request.tenant_id, request.compartment_id)
            .await?
            .ok_or_else(|| PrismError::NotFound {
                entity_type: "Compartment",
                id: *request.compartment_id.as_uuid(),
            })?;

        // Non-criminal-penalty compartments: ALLOW (normal access rules apply)
        if !compartment.criminal_penalty_isolation {
            return Ok(CriminalPenaltyOverrideResult {
                decision: AccessDecision::Allow,
                reason: Some(
                    "compartment does not have criminal-penalty isolation; \
                     normal access rules apply"
                        .into(),
                ),
            });
        }

        // Criminal-penalty compartment: check ONLY explicit membership
        // principal_chain (org-tree ancestors) is intentionally ignored
        let is_member = self
            .repo
            .is_member(
                request.tenant_id,
                request.compartment_id,
                request.principal_id,
                &request.principal_roles,
            )
            .await?;

        if is_member {
            Ok(CriminalPenaltyOverrideResult {
                decision: AccessDecision::Allow,
                reason: Some(
                    "principal is an explicit member of the criminal-penalty compartment".into(),
                ),
            })
        } else {
            warn!(
                tenant_id = %request.tenant_id,
                compartment_id = %request.compartment_id,
                principal_id = %request.principal_id,
                ancestor_count = request.principal_chain.len(),
                "SR_GOV_35: criminal-penalty override DENIED -- principal not explicit member \
                 (org-tree ancestors ignored)"
            );

            Ok(CriminalPenaltyOverrideResult {
                decision: AccessDecision::Deny,
                reason: Some(
                    "criminal-penalty compartment denies access to non-members; \
                     org-tree position is not sufficient"
                        .into(),
                ),
            })
        }
    }

    /// Generate a compartment audit report.
    ///
    /// Queries the compartment membership list, serializes a report payload,
    /// signs it via the `CompartmentReportSigner`, and emits an audit event.
    ///
    /// Implements: SR_GOV_36
    pub async fn generate_audit_report(
        &self,
        request: &CompartmentAuditRequest,
    ) -> Result<CompartmentAuditResult, PrismError> {
        // Verify compartment exists
        let compartment = self
            .repo
            .get_by_id(request.tenant_id, request.compartment_id)
            .await?
            .ok_or_else(|| PrismError::NotFound {
                entity_type: "Compartment",
                id: *request.compartment_id.as_uuid(),
            })?;

        // Get report signer (required for this operation)
        let signer = self
            .report_signer
            .as_ref()
            .ok_or_else(|| PrismError::Internal("CompartmentReportSigner not configured".into()))?;

        // Query membership
        let members = self
            .repo
            .list_members(request.tenant_id, request.compartment_id)
            .await?;

        let member_count = members.len();

        // Build report payload
        let report = serde_json::json!({
            "compartment_id": request.compartment_id.to_string(),
            "compartment_name": compartment.name,
            "classification_level": compartment.classification_level,
            "criminal_penalty_isolation": compartment.criminal_penalty_isolation,
            "period": request.period,
            "member_count": member_count,
            "members": members.iter().map(|m| {
                serde_json::json!({
                    "person_id": m.person_id.map(|p| p.to_string()),
                    "role_id": m.role_id.map(|r| r.to_string()),
                    "added_at": m.added_at.to_rfc3339(),
                })
            }).collect::<Vec<_>>(),
            "generated_at": Utc::now().to_rfc3339(),
        });

        let report_payload = serde_json::to_vec(&report)
            .map_err(|e| PrismError::Serialization(format!("failed to serialize report: {e}")))?;

        // Sign the report
        let signature = signer.sign(&report_payload).await?;

        // Audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "compartment.audit_report_generated".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(*request.compartment_id.as_uuid()),
                target_type: Some("Compartment".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Compliance,
                governance_authority: None,
                payload: serde_json::json!({
                    "period": request.period,
                    "member_count": member_count,
                }),
            })
            .await?;

        info!(
            compartment_id = %request.compartment_id,
            tenant_id = %request.tenant_id,
            member_count,
            "compartment audit report generated"
        );

        Ok(CompartmentAuditResult {
            report_payload,
            signature,
            member_count,
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

    // -- Mock CompartmentRepository ------------------------------------------

    struct MockCompartmentRepo {
        compartments: Mutex<Vec<Compartment>>,
        members: Mutex<Vec<CompartmentMembership>>,
    }

    impl MockCompartmentRepo {
        fn new() -> Self {
            Self {
                compartments: Mutex::new(Vec::new()),
                members: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl CompartmentRepository for MockCompartmentRepo {
        async fn create(&self, compartment: &Compartment) -> Result<(), PrismError> {
            self.compartments.lock().unwrap().push(compartment.clone());
            Ok(())
        }

        async fn get_by_id(
            &self,
            tenant_id: TenantId,
            id: CompartmentId,
        ) -> Result<Option<Compartment>, PrismError> {
            let comps = self.compartments.lock().unwrap();
            Ok(comps
                .iter()
                .find(|c| c.id == id && c.tenant_id == tenant_id)
                .cloned())
        }

        async fn add_member(&self, membership: &CompartmentMembership) -> Result<bool, PrismError> {
            let mut members = self.members.lock().unwrap();
            // Check for duplicate
            let exists = members.iter().any(|m| {
                m.compartment_id == membership.compartment_id
                    && m.tenant_id == membership.tenant_id
                    && m.person_id == membership.person_id
                    && m.role_id == membership.role_id
            });
            if exists {
                return Ok(false);
            }
            members.push(membership.clone());
            Ok(true)
        }

        async fn list_members(
            &self,
            tenant_id: TenantId,
            compartment_id: CompartmentId,
        ) -> Result<Vec<CompartmentMembership>, PrismError> {
            let members = self.members.lock().unwrap();
            Ok(members
                .iter()
                .filter(|m| m.compartment_id == compartment_id && m.tenant_id == tenant_id)
                .cloned()
                .collect())
        }

        async fn is_member(
            &self,
            tenant_id: TenantId,
            compartment_id: CompartmentId,
            person_id: UserId,
            role_ids: &[RoleId],
        ) -> Result<bool, PrismError> {
            let members = self.members.lock().unwrap();
            Ok(members.iter().any(|m| {
                m.compartment_id == compartment_id
                    && m.tenant_id == tenant_id
                    && (m.person_id == Some(person_id)
                        || m.role_id.map_or(false, |rid| role_ids.contains(&rid)))
            }))
        }

        async fn remove_member(
            &self,
            tenant_id: TenantId,
            compartment_id: CompartmentId,
            person_id: Option<UserId>,
            role_id: Option<RoleId>,
        ) -> Result<bool, PrismError> {
            let mut members = self.members.lock().unwrap();
            let before = members.len();
            members.retain(|m| {
                !(m.compartment_id == compartment_id
                    && m.tenant_id == tenant_id
                    && m.person_id == person_id
                    && m.role_id == role_id)
            });
            Ok(members.len() < before)
        }
    }

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

    // -- Mock SessionTerminator -------------------------------------------------

    struct MockSessionTerminator {
        terminated: Mutex<u64>,
    }

    impl MockSessionTerminator {
        fn new() -> Self {
            Self {
                terminated: Mutex::new(0),
            }
        }

        fn terminated_count(&self) -> u64 {
            *self.terminated.lock().unwrap()
        }
    }

    #[async_trait]
    impl SessionTerminator for MockSessionTerminator {
        async fn terminate_compartment_sessions(
            &self,
            _tenant_id: TenantId,
            _compartment_id: CompartmentId,
            _person_id: Option<UserId>,
            _role_id: Option<RoleId>,
        ) -> Result<u64, PrismError> {
            let mut count = self.terminated.lock().unwrap();
            *count += 2; // simulate 2 sessions terminated per call
            Ok(2)
        }
    }

    // -- Helpers --------------------------------------------------------------

    fn make_service() -> (CompartmentService, Arc<MockCompartmentRepo>) {
        let repo = Arc::new(MockCompartmentRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        let svc = CompartmentService::new(repo.clone(), audit);
        (svc, repo)
    }

    fn make_service_with_terminator() -> (
        CompartmentService,
        Arc<MockCompartmentRepo>,
        Arc<MockSessionTerminator>,
    ) {
        let repo = Arc::new(MockCompartmentRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        let terminator = Arc::new(MockSessionTerminator::new());
        let svc =
            CompartmentService::with_session_terminator(repo.clone(), audit, terminator.clone());
        (svc, repo, terminator)
    }

    fn make_create_request(tenant_id: TenantId) -> CompartmentCreateRequest {
        CompartmentCreateRequest {
            tenant_id,
            name: "BSA/AML Investigations".into(),
            classification_level: ClassificationLevel::CriminalPenalty,
            member_persons: vec![UserId::new()],
            member_roles: vec![],
            purpose: "Isolate BSA/AML investigation data per 31 USC § 5318(g)(2)".into(),
            criminal_penalty_isolation: true,
        }
    }

    // -- Tests ----------------------------------------------------------------

    #[tokio::test]
    async fn create_compartment_succeeds() {
        let (svc, _repo) = make_service();
        let tenant_id = TenantId::new();
        let result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        assert_eq!(result.member_count, 1);
    }

    #[tokio::test]
    async fn create_rejects_empty_name() {
        let (svc, _) = make_service();
        let mut req = make_create_request(TenantId::new());
        req.name = "  ".into();

        let err = svc.create(&req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn create_rejects_empty_purpose() {
        let (svc, _) = make_service();
        let mut req = make_create_request(TenantId::new());
        req.purpose = "".into();

        let err = svc.create(&req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn create_rejects_criminal_penalty_with_low_classification() {
        let (svc, _) = make_service();
        let mut req = make_create_request(TenantId::new());
        req.classification_level = ClassificationLevel::Internal;
        // criminal_penalty_isolation is true but classification is Internal

        let err = svc.create(&req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn create_rejects_no_initial_members() {
        let (svc, _) = make_service();
        let mut req = make_create_request(TenantId::new());
        req.member_persons = vec![];
        req.member_roles = vec![];

        let err = svc.create(&req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn create_with_mixed_members() {
        let (svc, _) = make_service();
        let mut req = make_create_request(TenantId::new());
        req.member_persons = vec![UserId::new(), UserId::new()];
        req.member_roles = vec![RoleId::new()];

        let result = svc.create(&req).await.unwrap();
        assert_eq!(result.member_count, 3);
    }

    #[tokio::test]
    async fn add_member_person() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let create_result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        let add_req = CompartmentMembershipAddRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: Some(UserId::new()),
            role_id: None,
        };

        let result = svc.add_member(&add_req).await.unwrap();
        assert!(result.added);
    }

    #[tokio::test]
    async fn add_member_rejects_both_person_and_role() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let create_result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        let add_req = CompartmentMembershipAddRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: Some(UserId::new()),
            role_id: Some(RoleId::new()),
        };

        let err = svc.add_member(&add_req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn add_member_rejects_neither_person_nor_role() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let create_result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        let add_req = CompartmentMembershipAddRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: None,
            role_id: None,
        };

        let err = svc.add_member(&add_req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn add_member_to_nonexistent_compartment_fails() {
        let (svc, _) = make_service();
        let add_req = CompartmentMembershipAddRequest {
            tenant_id: TenantId::new(),
            compartment_id: CompartmentId::new(),
            person_id: Some(UserId::new()),
            role_id: None,
        };

        let err = svc.add_member(&add_req).await.unwrap_err();
        assert!(matches!(err, PrismError::NotFound { .. }));
    }

    #[tokio::test]
    async fn check_access_allows_member() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let person_id = UserId::new();

        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![person_id];
        let create_result = svc.create(&req).await.unwrap();

        let check = CompartmentAccessCheckRequest {
            tenant_id,
            principal_id: person_id,
            principal_roles: vec![],
            resource_compartments: vec![create_result.compartment_id],
        };

        let result = svc.check_access(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Allow);
        assert!(result.denied_compartments.is_empty());
    }

    #[tokio::test]
    async fn check_access_denies_non_member() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let create_result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        let outsider = UserId::new();
        let check = CompartmentAccessCheckRequest {
            tenant_id,
            principal_id: outsider,
            principal_roles: vec![],
            resource_compartments: vec![create_result.compartment_id],
        };

        let result = svc.check_access(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Deny);
        assert_eq!(result.denied_compartments.len(), 1);
    }

    #[tokio::test]
    async fn check_access_allows_via_role_membership() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let role_id = RoleId::new();

        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![];
        req.member_roles = vec![role_id];
        let create_result = svc.create(&req).await.unwrap();

        let person = UserId::new();
        let check = CompartmentAccessCheckRequest {
            tenant_id,
            principal_id: person,
            principal_roles: vec![role_id],
            resource_compartments: vec![create_result.compartment_id],
        };

        let result = svc.check_access(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Allow);
    }

    #[tokio::test]
    async fn check_access_requires_all_compartments() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let person_id = UserId::new();

        // Create two compartments, person is member of only the first
        let mut req1 = make_create_request(tenant_id);
        req1.member_persons = vec![person_id];
        let c1 = svc.create(&req1).await.unwrap();

        let mut req2 = make_create_request(tenant_id);
        req2.name = "SOX Financial".into();
        req2.member_persons = vec![UserId::new()]; // different person
        req2.criminal_penalty_isolation = false;
        req2.classification_level = ClassificationLevel::Confidential;
        let c2 = svc.create(&req2).await.unwrap();

        let check = CompartmentAccessCheckRequest {
            tenant_id,
            principal_id: person_id,
            principal_roles: vec![],
            resource_compartments: vec![c1.compartment_id, c2.compartment_id],
        };

        let result = svc.check_access(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Deny);
        // Should deny on the second compartment
        assert_eq!(result.denied_compartments, vec![c2.compartment_id]);
    }

    #[tokio::test]
    async fn check_access_allows_unbound_resource() {
        let (svc, _) = make_service();
        let check = CompartmentAccessCheckRequest {
            tenant_id: TenantId::new(),
            principal_id: UserId::new(),
            principal_roles: vec![],
            resource_compartments: vec![], // not compartment-bound
        };

        let result = svc.check_access(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Allow);
    }

    // -- SR_GOV_34 Revoke Member Tests ----------------------------------------

    #[tokio::test]
    async fn revoke_member_removes_person() {
        let (svc, _repo, terminator) = make_service_with_terminator();
        let tenant_id = TenantId::new();
        let person_id = UserId::new();

        // Create compartment with the person as a member
        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![person_id];
        let create_result = svc.create(&req).await.unwrap();

        // Revoke the person
        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: Some(person_id),
            role_id: None,
        };

        let result = svc.revoke_member(&revoke_req).await.unwrap();
        assert!(result.removed);
        assert_eq!(result.sessions_terminated, 2);
        assert_eq!(terminator.terminated_count(), 2);
    }

    #[tokio::test]
    async fn revoke_member_removes_role() {
        let (svc, _repo, _term) = make_service_with_terminator();
        let tenant_id = TenantId::new();
        let role_id = RoleId::new();

        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![];
        req.member_roles = vec![role_id];
        let create_result = svc.create(&req).await.unwrap();

        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: None,
            role_id: Some(role_id),
        };

        let result = svc.revoke_member(&revoke_req).await.unwrap();
        assert!(result.removed);
    }

    #[tokio::test]
    async fn revoke_member_returns_false_if_not_found() {
        let (svc, _repo, _term) = make_service_with_terminator();
        let tenant_id = TenantId::new();
        let create_result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        // Try to remove a person who was never a member
        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: Some(UserId::new()),
            role_id: None,
        };

        let result = svc.revoke_member(&revoke_req).await.unwrap();
        assert!(!result.removed);
        assert_eq!(result.sessions_terminated, 0);
    }

    #[tokio::test]
    async fn revoke_member_rejects_both_person_and_role() {
        let (svc, _, _) = make_service_with_terminator();
        let tenant_id = TenantId::new();
        let create_result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: Some(UserId::new()),
            role_id: Some(RoleId::new()),
        };

        let err = svc.revoke_member(&revoke_req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn revoke_member_rejects_neither_person_nor_role() {
        let (svc, _, _) = make_service_with_terminator();
        let tenant_id = TenantId::new();
        let create_result = svc.create(&make_create_request(tenant_id)).await.unwrap();

        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: None,
            role_id: None,
        };

        let err = svc.revoke_member(&revoke_req).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn revoke_member_from_nonexistent_compartment_fails() {
        let (svc, _, _) = make_service_with_terminator();

        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id: TenantId::new(),
            compartment_id: CompartmentId::new(),
            person_id: Some(UserId::new()),
            role_id: None,
        };

        let err = svc.revoke_member(&revoke_req).await.unwrap_err();
        assert!(matches!(err, PrismError::NotFound { .. }));
    }

    #[tokio::test]
    async fn revoke_member_denies_access_after_revocation() {
        let (svc, _repo, _term) = make_service_with_terminator();
        let tenant_id = TenantId::new();
        let person_id = UserId::new();

        // Create compartment with person as member
        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![person_id];
        let create_result = svc.create(&req).await.unwrap();

        // Verify access is allowed before revocation
        let check = CompartmentAccessCheckRequest {
            tenant_id,
            principal_id: person_id,
            principal_roles: vec![],
            resource_compartments: vec![create_result.compartment_id],
        };
        let before = svc.check_access(&check).await.unwrap();
        assert_eq!(before.decision, AccessDecision::Allow);

        // Revoke membership
        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: Some(person_id),
            role_id: None,
        };
        svc.revoke_member(&revoke_req).await.unwrap();

        // Verify access is denied after revocation
        let after = svc.check_access(&check).await.unwrap();
        assert_eq!(after.decision, AccessDecision::Deny);
    }

    #[tokio::test]
    async fn revoke_member_without_terminator_skips_session_kill() {
        // Use the basic service without a session terminator
        let (svc, _repo) = make_service();
        let tenant_id = TenantId::new();
        let person_id = UserId::new();

        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![person_id];
        let create_result = svc.create(&req).await.unwrap();

        let revoke_req = CompartmentMembershipRemoveRequest {
            tenant_id,
            compartment_id: create_result.compartment_id,
            person_id: Some(person_id),
            role_id: None,
        };

        let result = svc.revoke_member(&revoke_req).await.unwrap();
        assert!(result.removed);
        assert_eq!(result.sessions_terminated, 0); // no terminator = 0 sessions
    }

    // -- SR_GOV_35 Criminal-Penalty Override Tests ----------------------------

    #[tokio::test]
    async fn criminal_penalty_denies_non_member_with_ancestors() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let member = UserId::new();
        let outsider = UserId::new();
        let ancestor_vp = UserId::new();
        let ancestor_ceo = UserId::new();

        // Create criminal-penalty compartment with only `member`
        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![member];
        let created = svc.create(&req).await.unwrap();

        // Outsider has ancestors (VP, CEO) but is NOT a member
        let check = CriminalPenaltyOverrideCheck {
            tenant_id,
            compartment_id: created.compartment_id,
            principal_id: outsider,
            principal_roles: vec![],
            principal_chain: vec![ancestor_vp, ancestor_ceo],
        };

        let result = svc.check_criminal_penalty_override(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Deny);
        assert!(result.reason.unwrap().contains("non-members"));
    }

    #[tokio::test]
    async fn criminal_penalty_allows_explicit_member() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let member = UserId::new();

        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![member];
        let created = svc.create(&req).await.unwrap();

        let check = CriminalPenaltyOverrideCheck {
            tenant_id,
            compartment_id: created.compartment_id,
            principal_id: member,
            principal_roles: vec![],
            principal_chain: vec![],
        };

        let result = svc.check_criminal_penalty_override(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Allow);
    }

    #[tokio::test]
    async fn non_criminal_penalty_allows_fallback() {
        let (svc, _) = make_service();
        let tenant_id = TenantId::new();
        let outsider = UserId::new();

        // Create non-criminal-penalty compartment
        let mut req = make_create_request(tenant_id);
        req.criminal_penalty_isolation = false;
        req.classification_level = ClassificationLevel::Confidential;
        let created = svc.create(&req).await.unwrap();

        let check = CriminalPenaltyOverrideCheck {
            tenant_id,
            compartment_id: created.compartment_id,
            principal_id: outsider,
            principal_roles: vec![],
            principal_chain: vec![],
        };

        let result = svc.check_criminal_penalty_override(&check).await.unwrap();
        assert_eq!(result.decision, AccessDecision::Allow);
    }

    #[tokio::test]
    async fn criminal_penalty_override_nonexistent_compartment_fails() {
        let (svc, _) = make_service();

        let check = CriminalPenaltyOverrideCheck {
            tenant_id: TenantId::new(),
            compartment_id: CompartmentId::new(),
            principal_id: UserId::new(),
            principal_roles: vec![],
            principal_chain: vec![],
        };

        let err = svc
            .check_criminal_penalty_override(&check)
            .await
            .unwrap_err();
        assert!(matches!(err, PrismError::NotFound { .. }));
    }

    // -- SR_GOV_36 Compartment Audit Report Tests ----------------------------

    // Mock CompartmentReportSigner
    struct MockReportSigner;

    #[async_trait]
    impl CompartmentReportSigner for MockReportSigner {
        async fn sign(&self, payload: &[u8]) -> Result<String, PrismError> {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            payload.hash(&mut hasher);
            Ok(format!("{:016x}", hasher.finish()))
        }
    }

    fn make_service_with_signer() -> (CompartmentService, Arc<MockCompartmentRepo>) {
        let repo = Arc::new(MockCompartmentRepo::new());
        let audit_repo = Arc::new(MockAuditRepo::new());
        let audit = AuditLogger::new(audit_repo);
        let signer = Arc::new(MockReportSigner);
        let svc = CompartmentService::with_report_signer(repo.clone(), audit, signer);
        (svc, repo)
    }

    #[tokio::test]
    async fn audit_report_generated_successfully() {
        let (svc, _) = make_service_with_signer();
        let tenant_id = TenantId::new();
        let created = svc.create(&make_create_request(tenant_id)).await.unwrap();

        let report_req = CompartmentAuditRequest {
            tenant_id,
            compartment_id: created.compartment_id,
            period: "2026-Q1".into(),
        };

        let result = svc.generate_audit_report(&report_req).await.unwrap();
        assert!(!result.report_payload.is_empty());
        assert!(!result.signature.is_empty());
        assert_eq!(result.member_count, 1);
    }

    #[tokio::test]
    async fn audit_report_nonexistent_compartment_fails() {
        let (svc, _) = make_service_with_signer();

        let report_req = CompartmentAuditRequest {
            tenant_id: TenantId::new(),
            compartment_id: CompartmentId::new(),
            period: "2026-Q1".into(),
        };

        let err = svc.generate_audit_report(&report_req).await.unwrap_err();
        assert!(matches!(err, PrismError::NotFound { .. }));
    }

    #[tokio::test]
    async fn audit_report_includes_membership_data() {
        let (svc, _) = make_service_with_signer();
        let tenant_id = TenantId::new();
        let person1 = UserId::new();
        let person2 = UserId::new();

        let mut req = make_create_request(tenant_id);
        req.member_persons = vec![person1, person2];
        let created = svc.create(&req).await.unwrap();

        let report_req = CompartmentAuditRequest {
            tenant_id,
            compartment_id: created.compartment_id,
            period: "2026-Q1".into(),
        };

        let result = svc.generate_audit_report(&report_req).await.unwrap();
        assert_eq!(result.member_count, 2);

        // Verify report payload contains member data
        let report: serde_json::Value = serde_json::from_slice(&result.report_payload).unwrap();
        let members = report.get("members").unwrap().as_array().unwrap();
        assert_eq!(members.len(), 2);
    }
}
