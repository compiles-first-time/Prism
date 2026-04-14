//! Automation lifecycle state machine (FOUND S 1.5.1, SR_DM_11).
//!
//! Deterministic transitions: DRAFT -> PENDING_APPROVAL -> ACTIVE -> ... -> DELETED.
//! Track A only -- Track B kernel removal states are not implemented.

use prism_core::error::PrismError;
use prism_core::types::LifecycleState;

/// Validate whether a lifecycle state transition is legal.
///
/// Returns `Ok(to)` if the transition is allowed, or
/// `Err(InvalidStateTransition)` with the list of valid next states.
///
/// Implements: FOUND S 1.5.1, SR_DM_11
pub fn validate_transition(
    from: LifecycleState,
    to: LifecycleState,
) -> Result<LifecycleState, PrismError> {
    let allowed = allowed_transitions(from);
    if allowed.contains(&to) {
        Ok(to)
    } else {
        Err(PrismError::InvalidStateTransition {
            from,
            to,
            reason: format!("allowed transitions from {from:?}: {allowed:?}"),
        })
    }
}

/// Return the set of legal next states for a given current state.
///
/// Track A lifecycle only (governance_profile = Tool).
///
/// Implements: FOUND S 1.5.1
pub fn allowed_transitions(from: LifecycleState) -> &'static [LifecycleState] {
    use LifecycleState::*;
    match from {
        Draft => &[PendingApproval],
        PendingApproval => &[ApprovedWithConditions, Active, Rejected],
        ApprovedWithConditions => &[Active, Rejected],
        Active => &[UnderReview, Suspended, Sunset],
        UnderReview => &[Active, Suspended, Sunset],
        Suspended => &[Active, Sunset],
        Sunset => &[Archived],
        Archived => &[Deleted],
        Deleted => &[],
        Rejected => &[Draft],
    }
}

/// Check whether a state is terminal (no further transitions possible).
pub fn is_terminal(state: LifecycleState) -> bool {
    allowed_transitions(state).is_empty()
}

/// Check whether an automation in this state should have active credentials.
///
/// Credentials are revoked on Sunset and remain revoked through Archived/Deleted.
pub fn has_active_credentials(state: LifecycleState) -> bool {
    use LifecycleState::*;
    matches!(
        state,
        Draft | PendingApproval | ApprovedWithConditions | Active | UnderReview | Suspended
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use LifecycleState::*;

    #[test]
    fn draft_can_only_move_to_pending() {
        assert_eq!(
            validate_transition(Draft, PendingApproval).unwrap(),
            PendingApproval
        );
        assert!(validate_transition(Draft, Active).is_err());
        assert!(validate_transition(Draft, Deleted).is_err());
    }

    #[test]
    fn pending_approval_three_outcomes() {
        assert!(validate_transition(PendingApproval, Active).is_ok());
        assert!(validate_transition(PendingApproval, ApprovedWithConditions).is_ok());
        assert!(validate_transition(PendingApproval, Rejected).is_ok());
        assert!(validate_transition(PendingApproval, Draft).is_err());
    }

    #[test]
    fn approved_with_conditions_to_active_or_rejected() {
        assert!(validate_transition(ApprovedWithConditions, Active).is_ok());
        assert!(validate_transition(ApprovedWithConditions, Rejected).is_ok());
        assert!(validate_transition(ApprovedWithConditions, Draft).is_err());
    }

    #[test]
    fn active_can_review_suspend_or_sunset() {
        assert!(validate_transition(Active, UnderReview).is_ok());
        assert!(validate_transition(Active, Suspended).is_ok());
        assert!(validate_transition(Active, Sunset).is_ok());
        assert!(validate_transition(Active, Draft).is_err());
        assert!(validate_transition(Active, Deleted).is_err());
    }

    #[test]
    fn under_review_can_reactivate_suspend_or_sunset() {
        assert!(validate_transition(UnderReview, Active).is_ok());
        assert!(validate_transition(UnderReview, Suspended).is_ok());
        assert!(validate_transition(UnderReview, Sunset).is_ok());
    }

    #[test]
    fn suspended_can_reactivate_or_sunset() {
        assert!(validate_transition(Suspended, Active).is_ok());
        assert!(validate_transition(Suspended, Sunset).is_ok());
        assert!(validate_transition(Suspended, Draft).is_err());
    }

    #[test]
    fn sunset_can_only_archive() {
        assert!(validate_transition(Sunset, Archived).is_ok());
        assert!(validate_transition(Sunset, Active).is_err());
    }

    #[test]
    fn archived_can_only_delete() {
        assert!(validate_transition(Archived, Deleted).is_ok());
        assert!(validate_transition(Archived, Active).is_err());
    }

    #[test]
    fn deleted_is_terminal() {
        assert!(is_terminal(Deleted));
        assert!(allowed_transitions(Deleted).is_empty());
    }

    #[test]
    fn rejected_can_return_to_draft() {
        assert!(validate_transition(Rejected, Draft).is_ok());
        assert!(validate_transition(Rejected, Active).is_err());
    }

    #[test]
    fn credentials_revoked_after_sunset() {
        assert!(has_active_credentials(Active));
        assert!(has_active_credentials(Suspended));
        assert!(!has_active_credentials(Sunset));
        assert!(!has_active_credentials(Archived));
        assert!(!has_active_credentials(Deleted));
    }

    #[test]
    fn all_states_have_consistent_transitions() {
        // Every state reachable via allowed_transitions should itself
        // have a valid allowed_transitions entry (no panic).
        let all_states = [
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
        ];
        for state in all_states {
            let next = allowed_transitions(state);
            for &target in next {
                // target should itself have a valid transition table
                let _ = allowed_transitions(target);
            }
        }
    }
}
