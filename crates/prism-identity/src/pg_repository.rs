//! PostgreSQL implementations for User and ServicePrincipal repositories
//! (SR_DM_02, SR_DM_20).

use async_trait::async_trait;
use sqlx::PgPool;

use prism_core::error::PrismError;
use prism_core::repository::{ServicePrincipalRepository, UserRepository};
use prism_core::types::*;

// =============================================================================
// PgUserRepository (SR_DM_02)
// =============================================================================

/// PostgreSQL-backed user repository.
///
/// Implements: SR_DM_02
pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn create(&self, user: &User) -> Result<(), PrismError> {
        let role_ids: Vec<uuid::Uuid> = user.role_ids.iter().map(|r| r.into_uuid()).collect();

        sqlx::query(
            r#"
            INSERT INTO users (
                id, tenant_id, idp_id, email, display_name,
                role_ids, primary_reporting_line, secondary_reporting_line,
                department, business_unit, is_active, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
        )
        .bind(user.id.into_uuid())
        .bind(user.tenant_id.into_uuid())
        .bind(&user.idp_id)
        .bind(&user.email)
        .bind(&user.display_name)
        .bind(&role_ids)
        .bind(user.primary_reporting_line.map(|id| id.into_uuid()))
        .bind(user.secondary_reporting_line.map(|id| id.into_uuid()))
        .bind(&user.department)
        .bind(&user.business_unit)
        .bind(user.is_active)
        .bind(user.created_at)
        .bind(user.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint().is_some() {
                    return PrismError::Conflict {
                        reason: format!("user '{}' already exists in tenant", user.email),
                    };
                }
            }
            PrismError::Database(e.to_string())
        })?;

        Ok(())
    }

    async fn get_by_id(
        &self,
        tenant_id: TenantId,
        id: UserId,
    ) -> Result<Option<User>, PrismError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, tenant_id, idp_id, email, display_name,
                   role_ids, primary_reporting_line, secondary_reporting_line,
                   department, business_unit, is_active, created_at, updated_at
            FROM users
            WHERE id = $1 AND tenant_id = $2
            "#,
        )
        .bind(id.into_uuid())
        .bind(tenant_id.into_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(row.map(|r| r.into_user()))
    }

    async fn get_by_email(
        &self,
        tenant_id: TenantId,
        email: &str,
    ) -> Result<Option<User>, PrismError> {
        let row = sqlx::query_as::<_, UserRow>(
            r#"
            SELECT id, tenant_id, idp_id, email, display_name,
                   role_ids, primary_reporting_line, secondary_reporting_line,
                   department, business_unit, is_active, created_at, updated_at
            FROM users
            WHERE tenant_id = $1 AND email = $2
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(row.map(|r| r.into_user()))
    }
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: uuid::Uuid,
    tenant_id: uuid::Uuid,
    idp_id: Option<String>,
    email: String,
    display_name: String,
    role_ids: Vec<uuid::Uuid>,
    primary_reporting_line: Option<uuid::Uuid>,
    secondary_reporting_line: Option<uuid::Uuid>,
    department: Option<String>,
    business_unit: Option<String>,
    is_active: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl UserRow {
    fn into_user(self) -> User {
        User {
            id: UserId::from_uuid(self.id),
            tenant_id: TenantId::from_uuid(self.tenant_id),
            idp_id: self.idp_id,
            email: self.email,
            display_name: self.display_name,
            role_ids: self.role_ids.into_iter().map(RoleId::from_uuid).collect(),
            primary_reporting_line: self.primary_reporting_line.map(UserId::from_uuid),
            secondary_reporting_line: self.secondary_reporting_line.map(UserId::from_uuid),
            department: self.department,
            business_unit: self.business_unit,
            is_active: self.is_active,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

// =============================================================================
// PgServicePrincipalRepository (SR_DM_20)
// =============================================================================

/// PostgreSQL-backed service principal repository.
///
/// Implements: SR_DM_20
pub struct PgServicePrincipalRepository {
    pool: PgPool,
}

impl PgServicePrincipalRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ServicePrincipalRepository for PgServicePrincipalRepository {
    async fn create(&self, sp: &ServicePrincipal) -> Result<(), PrismError> {
        let identity_type_str = enum_to_str(sp.identity_type);
        let governance_profile_str = enum_to_str(sp.governance_profile);

        sqlx::query(
            r#"
            INSERT INTO service_principals (
                id, tenant_id, automation_id, display_name,
                identity_type, governance_profile, permissions,
                credential_id, owner_id, is_active, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(sp.id.into_uuid())
        .bind(sp.tenant_id.into_uuid())
        .bind(sp.automation_id.map(|id| id.into_uuid()))
        .bind(&sp.display_name)
        .bind(&identity_type_str)
        .bind(&governance_profile_str)
        .bind(&sp.permissions)
        .bind(sp.credential_id.map(|id| id.into_uuid()))
        .bind(sp.owner_id.map(|id| id.into_uuid()))
        .bind(sp.is_active)
        .bind(sp.created_at)
        .bind(sp.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_by_id(
        &self,
        id: ServicePrincipalId,
    ) -> Result<Option<ServicePrincipal>, PrismError> {
        let row = sqlx::query_as::<_, SpRow>(
            r#"
            SELECT id, tenant_id, automation_id, display_name,
                   identity_type, governance_profile, permissions,
                   credential_id, owner_id, is_active, created_at, updated_at
            FROM service_principals
            WHERE id = $1
            "#,
        )
        .bind(id.into_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(row.map(|r| r.into_sp()))
    }

    async fn list_by_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<ServicePrincipal>, PrismError> {
        let rows = sqlx::query_as::<_, SpRow>(
            r#"
            SELECT id, tenant_id, automation_id, display_name,
                   identity_type, governance_profile, permissions,
                   credential_id, owner_id, is_active, created_at, updated_at
            FROM service_principals
            WHERE tenant_id = $1
            ORDER BY display_name
            "#,
        )
        .bind(tenant_id.into_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into_sp()).collect())
    }

    async fn deactivate(&self, id: ServicePrincipalId) -> Result<(), PrismError> {
        let result = sqlx::query(
            r#"
            UPDATE service_principals
            SET is_active = FALSE, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id.into_uuid())
        .execute(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(PrismError::NotFound {
                entity_type: "ServicePrincipal",
                id: id.into_uuid(),
            });
        }

        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct SpRow {
    id: uuid::Uuid,
    tenant_id: uuid::Uuid,
    automation_id: Option<uuid::Uuid>,
    display_name: String,
    identity_type: String,
    governance_profile: String,
    permissions: serde_json::Value,
    credential_id: Option<uuid::Uuid>,
    owner_id: Option<uuid::Uuid>,
    is_active: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl SpRow {
    fn into_sp(self) -> ServicePrincipal {
        ServicePrincipal {
            id: ServicePrincipalId::from_uuid(self.id),
            tenant_id: TenantId::from_uuid(self.tenant_id),
            automation_id: self.automation_id.map(AutomationId::from_uuid),
            display_name: self.display_name,
            identity_type: serde_json::from_value(serde_json::Value::String(
                self.identity_type,
            ))
            .unwrap_or(IdentityType::Automation),
            governance_profile: serde_json::from_value(serde_json::Value::String(
                self.governance_profile,
            ))
            .unwrap_or(GovernanceProfile::Tool),
            permissions: self.permissions,
            credential_id: self.credential_id.map(CredentialId::from_uuid),
            owner_id: self.owner_id.map(UserId::from_uuid),
            is_active: self.is_active,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Helper: serialize a serde-enabled enum to its snake_case string.
fn enum_to_str<T: serde::Serialize>(val: T) -> String {
    serde_json::to_value(val)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}
