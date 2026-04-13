//! Unified error types for the PRISM platform.

use thiserror::Error;
use uuid::Uuid;

/// Top-level error type for cross-crate error propagation.
#[derive(Debug, Error)]
pub enum PrismError {
    #[error("not found: {entity_type} with id {id}")]
    NotFound { entity_type: &'static str, id: Uuid },

    #[error("authorization denied: {reason}")]
    Unauthorized { reason: String },

    #[error("forbidden: {reason}")]
    Forbidden { reason: String },

    #[error("validation failed: {reason}")]
    Validation { reason: String },

    #[error("conflict: {reason}")]
    Conflict { reason: String },

    #[error("governance violation: {rule} - {detail}")]
    GovernanceViolation { rule: String, detail: String },

    #[error("compliance violation: {profile:?} - {detail}")]
    ComplianceViolation {
        profile: crate::types::ComplianceProfile,
        detail: String,
    },

    #[error("segregation of duties violation: {detail}")]
    SodViolation { detail: String },

    #[error("lifecycle transition denied: {from:?} -> {to:?} - {reason}")]
    InvalidStateTransition {
        from: crate::types::LifecycleState,
        to: crate::types::LifecycleState,
        reason: String,
    },

    #[error("credential error: {reason}")]
    Credential { reason: String },

    #[error("vault error: {reason}")]
    Vault { reason: String },

    #[error("database error: {0}")]
    Database(String),

    #[error("graph database error: {0}")]
    Graph(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Error code for structured API error responses.
/// Each variant maps to a stable string code for client consumption.
impl PrismError {
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "NOT_FOUND",
            Self::Unauthorized { .. } => "UNAUTHORIZED",
            Self::Forbidden { .. } => "FORBIDDEN",
            Self::Validation { .. } => "VALIDATION_ERROR",
            Self::Conflict { .. } => "CONFLICT",
            Self::GovernanceViolation { .. } => "GOVERNANCE_VIOLATION",
            Self::ComplianceViolation { .. } => "COMPLIANCE_VIOLATION",
            Self::SodViolation { .. } => "SOD_VIOLATION",
            Self::InvalidStateTransition { .. } => "INVALID_STATE_TRANSITION",
            Self::Credential { .. } => "CREDENTIAL_ERROR",
            Self::Vault { .. } => "VAULT_ERROR",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Graph(_) => "GRAPH_ERROR",
            Self::Cache(_) => "CACHE_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }
}
