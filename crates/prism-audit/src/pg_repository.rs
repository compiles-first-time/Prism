//! PostgreSQL implementation of the audit event repository (SR_DM_05).
//!
//! This is the only persistence backend for audit events. The table is
//! append-only: only INSERT and SELECT are issued. UPDATE and DELETE are
//! never called from application code.

use async_trait::async_trait;
use sqlx::PgPool;

use prism_core::error::PrismError;
use prism_core::repository::AuditEventRepository;
use prism_core::types::*;

/// PostgreSQL-backed audit event repository.
///
/// Implements: SR_DM_05
pub struct PgAuditEventRepository {
    pool: PgPool,
}

impl PgAuditEventRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AuditEventRepository for PgAuditEventRepository {
    /// Append an audit event. Implements: SR_DM_05, SR_GOV_47
    async fn append(&self, event: &AuditEvent) -> Result<(), PrismError> {
        let actor_type_str = serde_json::to_value(event.actor_type)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", event.actor_type).to_lowercase());
        let severity_str = serde_json::to_value(event.severity)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", event.severity).to_lowercase());
        let source_layer_str = serde_json::to_value(event.source_layer)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| format!("{:?}", event.source_layer).to_lowercase());

        sqlx::query(
            r#"
            INSERT INTO audit_events (
                id, tenant_id, event_type, actor_id, actor_type,
                target_id, target_type, severity, source_layer,
                governance_authority, payload, prev_event_hash,
                event_hash, chain_position, created_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9,
                $10, $11, $12,
                $13, $14, $15
            )
            "#,
        )
        .bind(event.id.into_uuid())
        .bind(event.tenant_id.into_uuid())
        .bind(&event.event_type)
        .bind(event.actor_id)
        .bind(&actor_type_str)
        .bind(event.target_id)
        .bind(&event.target_type)
        .bind(&severity_str)
        .bind(&source_layer_str)
        .bind(&event.governance_authority)
        .bind(&event.payload)
        .bind(&event.prev_event_hash)
        .bind(&event.event_hash)
        .bind(event.chain_position)
        .bind(event.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(())
    }

    /// Get the most recent event for a tenant (the chain head).
    async fn get_chain_head(&self, tenant_id: TenantId) -> Result<Option<AuditEvent>, PrismError> {
        let row = sqlx::query_as::<_, AuditEventRow>(
            r#"
            SELECT id, tenant_id, event_type, actor_id, actor_type,
                   target_id, target_type, severity, source_layer,
                   governance_authority, payload, prev_event_hash,
                   event_hash, chain_position, created_at
            FROM audit_events
            WHERE tenant_id = $1
            ORDER BY chain_position DESC
            LIMIT 1
            "#,
        )
        .bind(tenant_id.into_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(row.map(|r| r.into_audit_event()))
    }

    /// Query audit events with filters and pagination. Implements: SR_GOV_49
    async fn query(&self, request: &AuditQueryRequest) -> Result<AuditQueryResult, PrismError> {
        // Build the count query
        let count_row = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM audit_events
            WHERE tenant_id = $1
              AND ($2::text IS NULL OR event_type = $2)
              AND ($3::uuid IS NULL OR actor_id = $3)
              AND ($4::uuid IS NULL OR target_id = $4)
              AND ($5::text IS NULL OR severity = $5)
              AND ($6::timestamptz IS NULL OR created_at >= $6)
              AND ($7::timestamptz IS NULL OR created_at <= $7)
            "#,
        )
        .bind(request.tenant_id.into_uuid())
        .bind(&request.event_type)
        .bind(request.actor_id)
        .bind(request.target_id)
        .bind(request.severity.map(|s| {
            serde_json::to_value(s)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| format!("{s:?}").to_lowercase())
        }))
        .bind(request.from_time)
        .bind(request.to_time)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        let offset = request.page_token.unwrap_or(0);

        let rows = sqlx::query_as::<_, AuditEventRow>(
            r#"
            SELECT id, tenant_id, event_type, actor_id, actor_type,
                   target_id, target_type, severity, source_layer,
                   governance_authority, payload, prev_event_hash,
                   event_hash, chain_position, created_at
            FROM audit_events
            WHERE tenant_id = $1
              AND ($2::text IS NULL OR event_type = $2)
              AND ($3::uuid IS NULL OR actor_id = $3)
              AND ($4::uuid IS NULL OR target_id = $4)
              AND ($5::text IS NULL OR severity = $5)
              AND ($6::timestamptz IS NULL OR created_at >= $6)
              AND ($7::timestamptz IS NULL OR created_at <= $7)
            ORDER BY chain_position ASC
            LIMIT $8 OFFSET $9
            "#,
        )
        .bind(request.tenant_id.into_uuid())
        .bind(&request.event_type)
        .bind(request.actor_id)
        .bind(request.target_id)
        .bind(request.severity.map(|s| {
            serde_json::to_value(s)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| format!("{s:?}").to_lowercase())
        }))
        .bind(request.from_time)
        .bind(request.to_time)
        .bind(request.page_size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        let events: Vec<AuditEvent> = rows.into_iter().map(|r| r.into_audit_event()).collect();
        let next_token = if (offset + request.page_size) < count_row {
            Some(offset + request.page_size)
        } else {
            None
        };

        Ok(AuditQueryResult {
            events,
            next_page_token: next_token,
            total_count: count_row,
        })
    }

    /// Get a contiguous chain segment for verification (descending order).
    /// Implements: SR_GOV_48
    async fn get_chain_segment(
        &self,
        tenant_id: TenantId,
        depth: u32,
    ) -> Result<Vec<AuditEvent>, PrismError> {
        let rows = sqlx::query_as::<_, AuditEventRow>(
            r#"
            SELECT id, tenant_id, event_type, actor_id, actor_type,
                   target_id, target_type, severity, source_layer,
                   governance_authority, payload, prev_event_hash,
                   event_hash, chain_position, created_at
            FROM audit_events
            WHERE tenant_id = $1
            ORDER BY chain_position DESC
            LIMIT $2
            "#,
        )
        .bind(tenant_id.into_uuid())
        .bind(depth as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| PrismError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.into_audit_event()).collect())
    }
}

// -- Row mapping --------------------------------------------------------------

/// Intermediate row type for sqlx deserialization.
/// Maps the TEXT columns back to their enum variants.
#[derive(sqlx::FromRow)]
struct AuditEventRow {
    id: uuid::Uuid,
    tenant_id: uuid::Uuid,
    event_type: String,
    actor_id: uuid::Uuid,
    actor_type: String,
    target_id: Option<uuid::Uuid>,
    target_type: Option<String>,
    severity: String,
    source_layer: String,
    governance_authority: Option<String>,
    payload: serde_json::Value,
    prev_event_hash: Option<String>,
    event_hash: String,
    chain_position: i64,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl AuditEventRow {
    fn into_audit_event(self) -> AuditEvent {
        AuditEvent {
            id: AuditEventId::from_uuid(self.id),
            tenant_id: TenantId::from_uuid(self.tenant_id),
            event_type: self.event_type,
            actor_id: self.actor_id,
            actor_type: parse_actor_type(&self.actor_type),
            target_id: self.target_id,
            target_type: self.target_type,
            severity: parse_severity(&self.severity),
            source_layer: parse_source_layer(&self.source_layer),
            governance_authority: self.governance_authority,
            payload: self.payload,
            prev_event_hash: self.prev_event_hash,
            event_hash: self.event_hash,
            chain_position: self.chain_position,
            created_at: self.created_at,
        }
    }
}

fn parse_actor_type(s: &str) -> ActorType {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or(ActorType::System)
}

fn parse_severity(s: &str) -> Severity {
    serde_json::from_value(serde_json::Value::String(s.to_string())).unwrap_or(Severity::Low)
}

fn parse_source_layer(s: &str) -> SourceLayer {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .unwrap_or(SourceLayer::Governance)
}
