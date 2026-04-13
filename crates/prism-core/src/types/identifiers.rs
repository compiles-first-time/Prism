//! Strongly-typed identifiers for all core domain entities.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            pub fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }
    };
}

define_id!(
    /// Unique identifier for a tenant (legal entity).
    TenantId
);

define_id!(
    /// Unique identifier for a service principal (automation identity).
    ServicePrincipalId
);

define_id!(
    /// Unique identifier for a registered automation.
    AutomationId
);

define_id!(
    /// Unique identifier for a human user.
    UserId
);

define_id!(
    /// Unique identifier for a platform role.
    RoleId
);

define_id!(
    /// Unique identifier for an approval chain instance.
    ApprovalChainId
);

define_id!(
    /// Unique identifier for an audit event.
    AuditEventId
);

define_id!(
    /// Unique identifier for a compliance compartment.
    CompartmentId
);

define_id!(
    /// Unique identifier for a credential record.
    CredentialId
);
