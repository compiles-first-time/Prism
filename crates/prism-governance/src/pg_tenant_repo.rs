//! PostgreSQL implementation of the tenant repository (SR_DM_01).
//!
//! Handles the PG side of tenant persistence. Neo4j dual-write is
//! deferred to Week 2 (requires SyncCoordinator).

use async_trait::async_trait;
use sqlx::PgPool;

use prism_core::error::PrismError;
use prism_core::repository::TenantRepository;
use prism_core::types::*;

/// PostgreSQL-backed tenant repository.
///
/// Implements: SR_DM_01 (PG path only; Neo4j deferred)
pub struct PgTenantRepository {
    pool: PgPool,
}

impl PgTenantRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenantRepository for PgTenantRepository {
    /// Create a new tenant. Implements: SR_DM_01
    ///
    /// Business exception BE-01: duplicate name is caught by the unique
    /// constraint and mapped to `PrismError::Conflict`.
    async fn create(&self, tenant: &Tenant) -> Result<(), PrismError> {
        let profiles: Vec<String> = tenant
            .compliance_profiles
            .iter()
            .map(|p| {
                serde_json::to_value(p)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| format!("{p:?}").to_lowercase())
            })
            .collect();

        let legal_entity_str = serde_json::to_value(tenant.legal_entity_type)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", tenant.legal_entity_type).to_lowercase());

        sqlx::query(
            r#"
            INSERT INTO tenants (
                id, name, legal_entity_type, parent_tenant_id,
                compliance_profiles, is_active, created_at, updated_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(tenant.id.into_uuid())
        .bind(&tenant.name)
        .bind(&legal_entity_str)
        .bind(tenant.parent_tenant_id.map(|id| id.into_uuid()))
        .bind(&profiles)
        .bind(tenant.is_active)
        .bind(tenant.created_at)
        .bind(tenant.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.constraint().is_some() {
                    return PrismError::Conflict {
                        reason: format!("tenant '{}' already exists", tenant.name),
                    };
                }
            }
            PrismError::Database(e.to_string())
        })?;

        Ok(())
    }

    /// Retrieve a tenant by ID.
    async fn get_by_id(&self, id: TenantId) -> Result<Option<Tenant>, PrismError> {
        let row = sqlx::query_as::<_, TenantRow>(
            r#"
            SELECT id, name, legal_entity_type, parent_tenant_id,
                   compliance_profiles, is_active, created_at, updated_at
            FROM tenants
            WHERE id = $1
            "#,
        )
        .bind(id.into_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(row.map(|r| r.into_tenant()))
    }

    /// Update an existing tenant.
    async fn update(&self, tenant: &Tenant) -> Result<(), PrismError> {
        let profiles: Vec<String> = tenant
            .compliance_profiles
            .iter()
            .map(|p| {
                serde_json::to_value(p)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| format!("{p:?}").to_lowercase())
            })
            .collect();

        let legal_entity_str = serde_json::to_value(tenant.legal_entity_type)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", tenant.legal_entity_type).to_lowercase());

        let result = sqlx::query(
            r#"
            UPDATE tenants SET
                name = $2, legal_entity_type = $3, parent_tenant_id = $4,
                compliance_profiles = $5, is_active = $6, updated_at = $7
            WHERE id = $1
            "#,
        )
        .bind(tenant.id.into_uuid())
        .bind(&tenant.name)
        .bind(&legal_entity_str)
        .bind(tenant.parent_tenant_id.map(|id| id.into_uuid()))
        .bind(&profiles)
        .bind(tenant.is_active)
        .bind(tenant.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(PrismError::NotFound {
                entity_type: "Tenant",
                id: tenant.id.into_uuid(),
            });
        }

        Ok(())
    }

    /// List child tenants of a parent.
    async fn list_by_parent(&self, parent_id: TenantId) -> Result<Vec<Tenant>, PrismError> {
        let rows = sqlx::query_as::<_, TenantRow>(
            r#"
            SELECT id, name, legal_entity_type, parent_tenant_id,
                   compliance_profiles, is_active, created_at, updated_at
            FROM tenants
            WHERE parent_tenant_id = $1
            ORDER BY name
            "#,
        )
        .bind(parent_id.into_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into_tenant()).collect())
    }
}

// -- Row mapping --------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TenantRow {
    id: uuid::Uuid,
    name: String,
    legal_entity_type: String,
    parent_tenant_id: Option<uuid::Uuid>,
    compliance_profiles: Vec<String>,
    is_active: bool,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

impl TenantRow {
    fn into_tenant(self) -> Tenant {
        Tenant {
            id: TenantId::from_uuid(self.id),
            name: self.name,
            legal_entity_type: parse_legal_entity_type(&self.legal_entity_type),
            parent_tenant_id: self.parent_tenant_id.map(TenantId::from_uuid),
            compliance_profiles: self
                .compliance_profiles
                .iter()
                .map(|s| parse_compliance_profile(s))
                .collect(),
            is_active: self.is_active,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

fn parse_legal_entity_type(s: &str) -> LegalEntityType {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .unwrap_or(LegalEntityType::HoldingCompany)
}

fn parse_compliance_profile(s: &str) -> ComplianceProfile {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .unwrap_or(ComplianceProfile::General)
}
