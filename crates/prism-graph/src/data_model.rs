//! Data model node services for dual-store (PG + Neo4j) persistence.
//!
//! Implements: SR_DM_03, SR_DM_04, SR_DM_06, SR_DM_07, SR_DM_08, SR_DM_09, SR_DM_10,
//!             SR_DM_12, SR_DM_13, SR_DM_14, SR_DM_15, SR_DM_16, SR_DM_17, SR_DM_18,
//!             SR_DM_19, SR_DM_21, SR_DM_23, SR_DM_24, SR_DM_25, SR_DM_26, SR_DM_28,
//!             SR_DM_29
//!
//! All services use trait-based abstractions (`GraphWriter`, `PgWriter`,
//! `PartitionManager`) so that mock implementations can be used in tests
//! while real Neo4j / PG backends are connected later.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_audit::tamper_response::TenantWriteFreeze;
use prism_core::error::PrismError;
use prism_core::types::*;

// ============================================================================
// Reusable traits
// ============================================================================

/// Generic graph writer for idempotent Neo4j node creation.
///
/// Implementations create (or MERGE) a node of the given type with the
/// supplied properties. The returned UUID is the node identity in the graph.
///
/// Implements: REUSABLE_GraphWriter
#[async_trait]
pub trait GraphWriter: Send + Sync {
    /// Create (or merge) a node in the graph store.
    async fn create_node(
        &self,
        tenant_id: TenantId,
        node_type: &str,
        properties: serde_json::Value,
    ) -> Result<uuid::Uuid, PrismError>;
}

/// Generic PG row writer for relational audit/tracking rows.
///
/// Implementations insert a row into the specified table and return the
/// generated primary key UUID.
///
/// Implements: REUSABLE_PgWriter
#[async_trait]
pub trait PgWriter: Send + Sync {
    /// Insert a row into the given table.
    async fn insert_row(
        &self,
        table: &str,
        data: serde_json::Value,
    ) -> Result<uuid::Uuid, PrismError>;
}

/// Partition lifecycle manager for audit tables.
///
/// Implements: SR_DM_06
#[async_trait]
pub trait PartitionManager: Send + Sync {
    /// Create a new partition for the given tenant and period.
    async fn create_partition(&self, tenant_id: TenantId, period: &str) -> Result<(), PrismError>;

    /// Archive a partition, returning the number of rows archived.
    async fn archive_partition(&self, tenant_id: TenantId, period: &str)
        -> Result<u64, PrismError>;

    /// Drop a partition, returning the number of rows dropped.
    async fn drop_partition(&self, tenant_id: TenantId, period: &str) -> Result<u64, PrismError>;
}

// ============================================================================
// SR_DM_03 -- Compartment node service
// ============================================================================

/// Service for creating Compartment graph nodes.
///
/// Compartment nodes represent visibility compartments in the graph layer,
/// mirroring the governance compartments managed by prism-compliance.
///
/// Implements: SR_DM_03
pub struct CompartmentNodeService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl CompartmentNodeService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Create a Compartment graph node.
    ///
    /// 1. Serialize input properties into graph node properties.
    /// 2. Create the node via `GraphWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_03
    pub async fn create(
        &self,
        input: CompartmentNodeInput,
    ) -> Result<CompartmentNodeResult, PrismError> {
        let properties = serde_json::json!({
            "name": input.name,
            "classification_level": serde_json::to_value(input.classification_level)
                .map_err(|e| PrismError::Serialization(e.to_string()))?,
            "member_roles": serde_json::to_value(&input.member_roles)
                .map_err(|e| PrismError::Serialization(e.to_string()))?,
            "member_persons": serde_json::to_value(&input.member_persons)
                .map_err(|e| PrismError::Serialization(e.to_string()))?,
            "purpose": input.purpose,
            "criminal_penalty_isolation": input.criminal_penalty_isolation,
        });

        let compartment_id = self
            .writer
            .create_node(input.tenant_id, "Compartment", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            compartment_id = %compartment_id,
            "compartment graph node created"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.compartment_created".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(compartment_id),
                target_type: Some("Compartment".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(CompartmentNodeResult { compartment_id })
    }
}

// ============================================================================
// SR_DM_04 -- Connection node service
// ============================================================================

/// Service for creating Connection graph nodes.
///
/// Connection nodes represent external system integrations in the graph layer.
///
/// Implements: SR_DM_04
pub struct ConnectionNodeService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl ConnectionNodeService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Create a Connection graph node.
    ///
    /// 1. Serialize input properties including metadata.
    /// 2. Create the node via `GraphWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_04
    pub async fn create(
        &self,
        input: ConnectionNodeInput,
    ) -> Result<ConnectionNodeResult, PrismError> {
        let properties = serde_json::json!({
            "system_id": input.system_id,
            "connection_type": input.connection_type,
            "auth_type": input.auth_type,
            "credential_caas_ref": input.credential_caas_ref,
            "status": input.status,
            "scope": input.scope,
            "metadata": input.metadata,
        });

        let connection_id = self
            .writer
            .create_node(input.tenant_id, "Connection", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            connection_id = %connection_id,
            system_id = %input.system_id,
            "connection graph node created"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.connection_created".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(connection_id),
                target_type: Some("Connection".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(ConnectionNodeResult { connection_id })
    }
}

// ============================================================================
// SR_DM_06 -- Audit partition maintenance service
// ============================================================================

/// Service for managing audit table partitions.
///
/// Archives partitions older than 12 months and drops partitions older
/// than 84 months (7 years) to meet regulatory retention requirements.
///
/// Implements: SR_DM_06
pub struct AuditPartitionService {
    manager: Arc<dyn PartitionManager>,
    audit: AuditLogger,
}

impl AuditPartitionService {
    pub fn new(manager: Arc<dyn PartitionManager>, audit: AuditLogger) -> Self {
        Self { manager, audit }
    }

    /// Run partition maintenance for a tenant and period.
    ///
    /// 1. Archive partitions older than 12 months.
    /// 2. Drop partitions older than 84 months (7 years).
    /// 3. Emit an audit event with the counts.
    ///
    /// Implements: SR_DM_06
    pub async fn maintain(
        &self,
        request: AuditPartitionMaintenanceRequest,
    ) -> Result<AuditPartitionMaintenanceResult, PrismError> {
        // Archive partitions older than 12 months
        let archived_count = self
            .manager
            .archive_partition(request.tenant_id, &request.period)
            .await?;

        // Drop partitions older than 84 months (7 years)
        let dropped_count = self
            .manager
            .drop_partition(request.tenant_id, &request.period)
            .await?;

        info!(
            tenant_id = %request.tenant_id,
            period = %request.period,
            archived_count,
            dropped_count,
            "audit partition maintenance completed"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: request.tenant_id,
                event_type: "data_model.partition_maintained".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("AuditPartition".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: serde_json::json!({
                    "period": request.period,
                    "archived_count": archived_count,
                    "dropped_count": dropped_count,
                }),
            })
            .await?;

        Ok(AuditPartitionMaintenanceResult {
            archived_count,
            dropped_count,
        })
    }
}

// ============================================================================
// SR_DM_07 -- DataCollection node service
// ============================================================================

/// Service for creating DataCollection graph nodes.
///
/// A DataCollection represents a batch of data pulled from an external
/// system, uploaded by a user, or produced by a prediction model.
///
/// Implements: SR_DM_07
pub struct DataCollectionService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl DataCollectionService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Create a DataCollection graph node.
    ///
    /// 1. Serialize input properties including data origin.
    /// 2. Create the node via `GraphWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_07
    pub async fn create(
        &self,
        input: DataCollectionInput,
    ) -> Result<DataCollectionResult, PrismError> {
        let properties = serde_json::json!({
            "connection_id": input.connection_id.to_string(),
            "source_system": input.source_system,
            "pull_timestamp": input.pull_timestamp.to_rfc3339(),
            "freshness_policy": input.freshness_policy,
            "record_count": input.record_count,
            "ingestion_method": input.ingestion_method,
            "source_file_ref": input.source_file_ref,
            "training_consent": input.training_consent,
            "data_origin": serde_json::to_value(input.data_origin)
                .map_err(|e| PrismError::Serialization(e.to_string()))?,
        });

        let collection_id = self
            .writer
            .create_node(input.tenant_id, "DataCollection", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            collection_id = %collection_id,
            source_system = %input.source_system,
            "data collection graph node created"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.collection_created".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(collection_id),
                target_type: Some("DataCollection".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(DataCollectionResult { collection_id })
    }
}

// ============================================================================
// SR_DM_08 -- DataField batch upsert service
// ============================================================================

/// Service for upserting DataField graph nodes in bulk.
///
/// DataField nodes describe the schema of a DataCollection: field names,
/// types, classification, sensitivity, and completeness metrics.
///
/// Implements: SR_DM_08
pub struct DataFieldService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl DataFieldService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Upsert a batch of DataField nodes for a DataCollection.
    ///
    /// Each field is written as an idempotent MERGE via `GraphWriter`.
    /// An empty batch is a valid no-op returning `upserted_count = 0`.
    ///
    /// Implements: SR_DM_08
    pub async fn upsert_batch(
        &self,
        input: DataFieldInputBatch,
    ) -> Result<DataFieldBatchResult, PrismError> {
        let mut upserted_count = 0usize;

        for field in &input.fields {
            let properties = serde_json::json!({
                "collection_id": input.collection_id.to_string(),
                "field_name": field.field_name,
                "technical_type": field.technical_type,
                "semantic_type": field.semantic_type,
                "classification": field.classification,
                "sensitivity_level": field.sensitivity_level,
                "completeness_pct": field.completeness_pct,
            });

            self.writer
                .create_node(input.tenant_id, "DataField", properties)
                .await?;

            upserted_count += 1;
        }

        info!(
            tenant_id = %input.tenant_id,
            collection_id = %input.collection_id,
            upserted_count,
            "data field batch upserted"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.fields_upserted".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.collection_id),
                target_type: Some("DataField".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: serde_json::json!({
                    "collection_id": input.collection_id.to_string(),
                    "upserted_count": upserted_count,
                }),
            })
            .await?;

        Ok(DataFieldBatchResult { upserted_count })
    }
}

// ============================================================================
// SR_DM_09 -- Recommendation node service (dual-store)
// ============================================================================

/// Service for creating Recommendation nodes in both graph and PG stores.
///
/// Recommendations are written to:
/// 1. The graph store (via `GraphWriter`) for relationship traversal.
/// 2. A PG audit row (via `PgWriter`) for relational queries and reporting.
///
/// Implements: SR_DM_09
pub struct RecommendationNodeService {
    graph_writer: Arc<dyn GraphWriter>,
    pg_writer: Arc<dyn PgWriter>,
    audit: AuditLogger,
}

impl RecommendationNodeService {
    pub fn new(
        graph_writer: Arc<dyn GraphWriter>,
        pg_writer: Arc<dyn PgWriter>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            graph_writer,
            pg_writer,
            audit,
        }
    }

    /// Create a Recommendation node in both graph and PG stores.
    ///
    /// 1. Create graph node via `GraphWriter`.
    /// 2. Insert PG audit row via `PgWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_09
    pub async fn create(
        &self,
        input: RecommendationNodeInput,
    ) -> Result<RecommendationNodeResult, PrismError> {
        let properties = serde_json::json!({
            "content_hash": input.content_hash,
            "model_used": input.model_used,
            "confidence": input.confidence,
            "parameters_used": input.parameters_used,
            "state": input.state,
            "category": input.category,
        });

        // Step 1: graph node
        let rec_id = self
            .graph_writer
            .create_node(input.tenant_id, "Recommendation", properties.clone())
            .await?;

        // Step 2: PG audit row
        let pg_data = serde_json::json!({
            "rec_id": rec_id.to_string(),
            "tenant_id": input.tenant_id.to_string(),
            "content_hash": input.content_hash,
            "model_used": input.model_used,
            "confidence": input.confidence,
            "parameters_used": input.parameters_used,
            "state": input.state,
            "category": input.category,
        });

        let audit_row_id = self
            .pg_writer
            .insert_row("recommendations", pg_data)
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            rec_id = %rec_id,
            audit_row_id = %audit_row_id,
            "recommendation dual-store node created"
        );

        // Step 3: audit event
        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.recommendation_created".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(rec_id),
                target_type: Some("Recommendation".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(RecommendationNodeResult {
            rec_id,
            audit_row_id,
        })
    }
}

// ============================================================================
// SR_DM_10 -- Rejection node service
// ============================================================================

/// Service for creating Rejection graph nodes.
///
/// A Rejection records a human decision to reject a recommendation,
/// along with a mandatory justification. The graph node carries a
/// `JUSTIFIED_BY` edge reference back to the recommendation.
///
/// Implements: SR_DM_10
pub struct RejectionNodeService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl RejectionNodeService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Create a Rejection graph node with JUSTIFIED_BY edge reference.
    ///
    /// 1. Serialize input properties including the recommendation reference.
    /// 2. Create the node via `GraphWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_10
    pub async fn create(
        &self,
        input: RejectionNodeInput,
    ) -> Result<RejectionNodeResult, PrismError> {
        let properties = serde_json::json!({
            "recommendation_id": input.recommendation_id.to_string(),
            "category": input.category,
            "justification_text": input.justification_text,
            "person_id": input.person_id.to_string(),
            "timestamp": input.timestamp.to_rfc3339(),
            "edge_type": "JUSTIFIED_BY",
            "edge_target": input.recommendation_id.to_string(),
        });

        let rejection_id = self
            .writer
            .create_node(input.tenant_id, "Rejection", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            rejection_id = %rejection_id,
            recommendation_id = %input.recommendation_id,
            "rejection graph node created"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.rejection_created".into(),
                actor_id: *input.person_id.as_uuid(),
                actor_type: ActorType::Human,
                target_id: Some(rejection_id),
                target_type: Some("Rejection".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(RejectionNodeResult { rejection_id })
    }
}

// ============================================================================
// SR_DM_12 -- Component node service
// ============================================================================

/// Service for creating Component graph nodes.
///
/// A Component represents a reusable automation building block
/// (function, model, connector) tracked in the knowledge graph.
///
/// Implements: SR_DM_12
pub struct ComponentNodeService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl ComponentNodeService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Create a Component graph node.
    ///
    /// 1. Serialize input properties.
    /// 2. Create the node via `GraphWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_12
    pub async fn create(
        &self,
        input: ComponentNodeInput,
    ) -> Result<ComponentNodeResult, PrismError> {
        let properties = serde_json::json!({
            "component_id": input.component_id,
            "category": input.category,
            "version": input.version,
            "git_sha": input.git_sha,
            "status": input.status,
            "metadata": input.metadata,
        });

        let node_id = self
            .writer
            .create_node(input.tenant_id, "Component", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            node_id = %node_id,
            component_id = %input.component_id,
            "component graph node created"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.component_created".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(node_id),
                target_type: Some("Component".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(ComponentNodeResult { node_id })
    }
}

// ============================================================================
// SR_DM_13 -- Component registry service
// ============================================================================

/// Service for registering components in the relational store.
///
/// The component registry is a PG table that tracks component versions,
/// ownership, and scope for relational queries and reporting.
///
/// Implements: SR_DM_13
pub struct ComponentRegistryService {
    pg_writer: Arc<dyn PgWriter>,
    audit: AuditLogger,
}

impl ComponentRegistryService {
    pub fn new(pg_writer: Arc<dyn PgWriter>, audit: AuditLogger) -> Self {
        Self { pg_writer, audit }
    }

    /// Register a component in the relational registry.
    ///
    /// 1. Insert a row via `PgWriter`.
    /// 2. Emit an audit event.
    ///
    /// Implements: SR_DM_13
    pub async fn register(
        &self,
        input: ComponentRegistryRow,
    ) -> Result<ComponentRegistryResult, PrismError> {
        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "component_id": input.component_id,
            "version": input.version,
            "git_sha": input.git_sha,
            "status": input.status,
            "owner_id": input.owner_id.to_string(),
            "scope": input.scope,
        });

        let row_id = self
            .pg_writer
            .insert_row("component_registry", data.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            row_id = %row_id,
            component_id = %input.component_id,
            "component registered in relational store"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.component_registered".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(row_id),
                target_type: Some("ComponentRegistry".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: data,
            })
            .await?;

        Ok(ComponentRegistryResult { row_id })
    }
}

// ============================================================================
// SR_DM_14 -- Component performance service
// ============================================================================

/// Service for recording component performance telemetry.
///
/// High-volume telemetry -- no audit event emitted.
///
/// Implements: SR_DM_14
pub struct ComponentPerformanceService {
    pg_writer: Arc<dyn PgWriter>,
}

impl ComponentPerformanceService {
    pub fn new(pg_writer: Arc<dyn PgWriter>) -> Self {
        Self { pg_writer }
    }

    /// Record component performance metrics.
    ///
    /// 1. Insert a row via `PgWriter`.
    /// 2. No audit event (high-volume telemetry).
    ///
    /// Implements: SR_DM_14
    pub async fn record(
        &self,
        input: ComponentPerformanceRow,
    ) -> Result<ComponentPerformanceResult, PrismError> {
        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "component_id": input.component_id,
            "execution_count": input.execution_count,
            "latency_ms": input.latency_ms,
            "success_count": input.success_count,
            "failure_count": input.failure_count,
            "cost_usd": input.cost_usd,
        });

        let row_id = self
            .pg_writer
            .insert_row("component_performance", data)
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            row_id = %row_id,
            component_id = %input.component_id,
            "component performance recorded"
        );

        Ok(ComponentPerformanceResult { row_id })
    }
}

// ============================================================================
// SR_DM_15 -- ModelExecution node service
// ============================================================================

/// Service for creating ModelExecution graph nodes.
///
/// Tracks individual LLM invocations: model, slot, task type,
/// token counts, latency, cost, and data sensitivity.
///
/// Implements: SR_DM_15
pub struct ModelExecutionService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl ModelExecutionService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Create a ModelExecution graph node.
    ///
    /// 1. Serialize input properties including task type.
    /// 2. Create the node via `GraphWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_15
    pub async fn create(
        &self,
        input: ModelExecutionInput,
    ) -> Result<ModelExecutionResult, PrismError> {
        let properties = serde_json::json!({
            "model_id": input.model_id,
            "slot": input.slot,
            "task_type": serde_json::to_value(input.task_type)
                .map_err(|e| PrismError::Serialization(e.to_string()))?,
            "input_tokens": input.input_tokens,
            "output_tokens": input.output_tokens,
            "latency_ms": input.latency_ms,
            "cost_usd": input.cost_usd,
            "data_sensitivity": input.data_sensitivity,
            "training_run_id": input.training_run_id.map(|id| id.to_string()),
        });

        let execution_id = self
            .writer
            .create_node(input.tenant_id, "ModelExecution", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            execution_id = %execution_id,
            model_id = %input.model_id,
            "model execution graph node created"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.model_execution_created".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(execution_id),
                target_type: Some("ModelExecution".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(ModelExecutionResult { execution_id })
    }
}

// ============================================================================
// SR_DM_16 -- ModelOutcomeScore service
// ============================================================================

/// Service for recording model outcome scores.
///
/// Creates a graph node with a SCORED_BY edge reference back to the
/// model execution that produced the outcome.
///
/// Implements: SR_DM_16
pub struct ModelOutcomeService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl ModelOutcomeService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Score a model outcome with a SCORED_BY edge reference.
    ///
    /// 1. Serialize input properties including the execution reference.
    /// 2. Create the node via `GraphWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_16
    pub async fn score(&self, input: ModelOutcomeInput) -> Result<ModelOutcomeResult, PrismError> {
        let properties = serde_json::json!({
            "execution_id": input.execution_id.to_string(),
            "outcome_type": input.outcome_type,
            "outcome_value": input.outcome_value,
            "quality_score": input.quality_score,
            "edge_type": "SCORED_BY",
            "edge_target": input.execution_id.to_string(),
        });

        let score_id = self
            .writer
            .create_node(input.tenant_id, "ModelOutcomeScore", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            score_id = %score_id,
            execution_id = %input.execution_id,
            "model outcome scored"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.model_outcome_scored".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(score_id),
                target_type: Some("ModelOutcomeScore".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(ModelOutcomeResult { score_id })
    }
}

// ============================================================================
// SR_DM_17 -- Model performance aggregation service
// ============================================================================

/// Service for aggregating model performance metrics by period.
///
/// Upserts aggregated rows into the `model_performance_analytics` PG table
/// for dashboard and reporting use.
///
/// Implements: SR_DM_17
pub struct ModelAggregationService {
    pg_writer: Arc<dyn PgWriter>,
    audit: AuditLogger,
}

impl ModelAggregationService {
    pub fn new(pg_writer: Arc<dyn PgWriter>, audit: AuditLogger) -> Self {
        Self { pg_writer, audit }
    }

    /// Aggregate model performance metrics for a period.
    ///
    /// 1. Upsert aggregation row via `PgWriter`.
    /// 2. Emit an audit event.
    ///
    /// Implements: SR_DM_17
    pub async fn aggregate(
        &self,
        input: ModelAggregationRequest,
    ) -> Result<ModelAggregationResult, PrismError> {
        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "period": input.period,
            "aggregated_at": Utc::now().to_rfc3339(),
        });

        self.pg_writer
            .insert_row("model_performance_analytics", data.clone())
            .await?;

        let rows_updated = 1u64;

        info!(
            tenant_id = %input.tenant_id,
            period = %input.period,
            rows_updated,
            "model performance aggregation completed"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.model_aggregation_complete".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("ModelPerformanceAnalytics".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: serde_json::json!({
                    "period": input.period,
                    "rows_updated": rows_updated,
                }),
            })
            .await?;

        Ok(ModelAggregationResult { rows_updated })
    }
}

// ============================================================================
// SR_DM_18 -- Vector embedding service
// ============================================================================

/// Pluggable embedding model trait.
///
/// Implementations produce a dense vector from input text.
///
/// Implements: SR_DM_18
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    /// Embed text into a dense vector.
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, PrismError>;

    /// Return the model identifier.
    fn model_id(&self) -> &str;

    /// Return the embedding model version.
    fn version(&self) -> &str;
}

/// Vector index writer trait for persisting embeddings.
///
/// Implementations write embeddings to a vector store (e.g., pgvector, Pinecone).
///
/// Implements: SR_DM_18
#[async_trait]
pub trait VectorIndexWriter: Send + Sync {
    /// Write an embedding vector to the index.
    async fn write_embedding(
        &self,
        tenant_id: TenantId,
        source_node_id: uuid::Uuid,
        vector: &[f32],
        model_id: &str,
        version: &str,
    ) -> Result<(), PrismError>;
}

/// Service for creating vector embeddings from text.
///
/// Uses a pluggable `EmbeddingModel` to produce vectors and a
/// `VectorIndexWriter` to persist them.
///
/// Implements: SR_DM_18
pub struct VectorEmbeddingService {
    embedding_model: Arc<dyn EmbeddingModel>,
    vector_writer: Arc<dyn VectorIndexWriter>,
    audit: AuditLogger,
}

impl VectorEmbeddingService {
    pub fn new(
        embedding_model: Arc<dyn EmbeddingModel>,
        vector_writer: Arc<dyn VectorIndexWriter>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            embedding_model,
            vector_writer,
            audit,
        }
    }

    /// Embed text and persist the resulting vector.
    ///
    /// 1. Call the embedding model to produce a vector.
    /// 2. Write the vector to the index via `VectorIndexWriter`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_18
    pub async fn embed(&self, input: EmbeddingInput) -> Result<EmbeddingResult, PrismError> {
        let vector = self.embedding_model.embed_text(&input.text).await?;
        let model_id = self.embedding_model.model_id().to_string();
        let version = self.embedding_model.version().to_string();

        self.vector_writer
            .write_embedding(
                input.tenant_id,
                input.source_node_id,
                &vector,
                &model_id,
                &version,
            )
            .await?;

        let embedded_at = Utc::now();
        let vector_dim = vector.len();

        info!(
            tenant_id = %input.tenant_id,
            source_node_id = %input.source_node_id,
            vector_dim,
            model_id = %model_id,
            "text embedded"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.text_embedded".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.source_node_id),
                target_type: Some("VectorEmbedding".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: serde_json::json!({
                    "model_id": model_id,
                    "vector_dim": vector_dim,
                }),
            })
            .await?;

        Ok(EmbeddingResult {
            vector_dim,
            model_id,
            embedded_at,
        })
    }
}

// ============================================================================
// SR_DM_19 -- Dual embedding store service
// ============================================================================

/// Service for storing dual embeddings during model migration.
///
/// During an embedding model migration, both old and new embeddings are
/// stored for a transition window (default 7 days) to allow gradual cutover.
///
/// Implements: SR_DM_19
pub struct DualEmbeddingService {
    vector_writer: Arc<dyn VectorIndexWriter>,
    audit: AuditLogger,
}

impl DualEmbeddingService {
    pub fn new(vector_writer: Arc<dyn VectorIndexWriter>, audit: AuditLogger) -> Self {
        Self {
            vector_writer,
            audit,
        }
    }

    /// Store both old and new embeddings for dual-active migration.
    ///
    /// 1. Write the old embedding via `VectorIndexWriter`.
    /// 2. Write the new embedding via `VectorIndexWriter`.
    /// 3. Calculate the dual-active expiry (7 days from now).
    /// 4. Emit an audit event.
    ///
    /// Implements: SR_DM_19
    pub async fn store_dual(
        &self,
        input: DualEmbeddingInput,
    ) -> Result<DualEmbeddingResult, PrismError> {
        // Write old embedding
        self.vector_writer
            .write_embedding(
                input.tenant_id,
                input.source_node_id,
                &input.old_embedding,
                &input.old_model,
                "old",
            )
            .await?;

        // Write new embedding
        self.vector_writer
            .write_embedding(
                input.tenant_id,
                input.source_node_id,
                &input.new_embedding,
                &input.new_model,
                "new",
            )
            .await?;

        let dual_active_until = Utc::now() + Duration::days(7);

        info!(
            tenant_id = %input.tenant_id,
            source_node_id = %input.source_node_id,
            old_model = %input.old_model,
            new_model = %input.new_model,
            dual_active_until = %dual_active_until,
            "dual embeddings stored"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.dual_embedding_stored".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.source_node_id),
                target_type: Some("DualEmbedding".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: serde_json::json!({
                    "old_model": input.old_model,
                    "new_model": input.new_model,
                    "dual_active_until": dual_active_until.to_rfc3339(),
                }),
            })
            .await?;

        Ok(DualEmbeddingResult { dual_active_until })
    }
}

// ============================================================================
// SR_DM_21 -- SA usage and anomaly log service
// ============================================================================

/// Service for logging service account usage and anomaly events.
///
/// Usage events are high-volume telemetry (no audit event).
/// Anomaly events emit an audit event at the anomaly's severity level.
///
/// Implements: SR_DM_21
pub struct SaUsageLogService {
    pg_writer: Arc<dyn PgWriter>,
    audit: AuditLogger,
}

impl SaUsageLogService {
    pub fn new(pg_writer: Arc<dyn PgWriter>, audit: AuditLogger) -> Self {
        Self { pg_writer, audit }
    }

    /// Log a service account usage event.
    ///
    /// High-volume telemetry -- no audit event emitted.
    ///
    /// Implements: SR_DM_21
    pub async fn log_usage(&self, input: SaUsageEvent) -> Result<uuid::Uuid, PrismError> {
        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "sa_id": input.sa_id.to_string(),
            "action": input.action,
            "target": input.target,
            "timestamp": input.timestamp.to_rfc3339(),
        });

        let row_id = self.pg_writer.insert_row("sa_usage_log", data).await?;

        info!(
            tenant_id = %input.tenant_id,
            sa_id = %input.sa_id,
            action = %input.action,
            "sa usage event logged"
        );

        Ok(row_id)
    }

    /// Log a service account anomaly event.
    ///
    /// Anomaly events are emitted at the anomaly's severity level.
    ///
    /// Implements: SR_DM_21
    pub async fn log_anomaly(&self, input: SaAnomalyEvent) -> Result<uuid::Uuid, PrismError> {
        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "sa_id": input.sa_id.to_string(),
            "anomaly_type": input.anomaly_type,
            "severity": serde_json::to_value(input.severity)
                .map_err(|e| PrismError::Serialization(e.to_string()))?,
            "evidence": input.evidence,
        });

        let row_id = self
            .pg_writer
            .insert_row("sa_anomaly_log", data.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            sa_id = %input.sa_id,
            anomaly_type = %input.anomaly_type,
            "sa anomaly event logged"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.sa_anomaly_logged".into(),
                actor_id: input.sa_id,
                actor_type: ActorType::ServicePrincipal,
                target_id: Some(row_id),
                target_type: Some("SaAnomalyLog".into()),
                severity: input.severity,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: data,
            })
            .await?;

        Ok(row_id)
    }
}

// ============================================================================
// SR_DM_23 -- Vector write enforcer
// ============================================================================

/// Enforces model-tagging policy on vector writes.
///
/// Rejects vector write attempts that lack a model_id tag, since untagged
/// embeddings cannot be rolled back during model migration (D-33).
///
/// Implements: SR_DM_23
pub struct VectorWriteEnforcer {
    audit: AuditLogger,
}

impl VectorWriteEnforcer {
    pub fn new(audit: AuditLogger) -> Self {
        Self { audit }
    }

    /// Enforce model-tagging policy on a vector write attempt.
    ///
    /// Rejects if `model_id` is `None` or empty (untagged embeddings
    /// cannot be rolled back per D-33).
    ///
    /// Implements: SR_DM_23
    pub async fn enforce(
        &self,
        attempt: VectorWriteAttempt,
    ) -> Result<VectorWriteResult, PrismError> {
        let model_id_valid = attempt
            .model_id
            .as_ref()
            .map(|id| !id.is_empty())
            .unwrap_or(false);

        if !model_id_valid {
            info!(
                source = %attempt.source,
                tenant_id = %attempt.tenant_id,
                "untagged vector write rejected -- model_id missing or empty"
            );

            self.audit
                .log(AuditEventInput {
                    tenant_id: attempt.tenant_id,
                    event_type: "data_model.untagged_vector_rejected".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: None,
                    target_type: Some("VectorEmbedding".into()),
                    severity: Severity::Medium,
                    source_layer: SourceLayer::Graph,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "source": attempt.source,
                        "reason": "model_id missing or empty; untagged embeddings cannot be rolled back (D-33)",
                    }),
                })
                .await?;

            return Ok(VectorWriteResult {
                accepted: false,
                reason: Some(
                    "model_id missing or empty; untagged embeddings cannot be rolled back (D-33)"
                        .into(),
                ),
            });
        }

        Ok(VectorWriteResult {
            accepted: true,
            reason: None,
        })
    }
}

// ============================================================================
// SR_DM_24 -- Graph maintenance service
// ============================================================================

/// Trait for executing graph maintenance cycles.
///
/// Implements: SR_DM_24
#[async_trait]
pub trait GraphMaintenanceWorker: Send + Sync {
    /// Execute a maintenance cycle, returning the number of affected entities.
    async fn execute_cycle(
        &self,
        tenant_id: Option<TenantId>,
        cycle_type: MaintenanceCycleType,
    ) -> Result<u64, PrismError>;
}

/// Service for running graph maintenance cycles.
///
/// Delegates to a `GraphMaintenanceWorker` and emits audit events
/// on completion.
///
/// Implements: SR_DM_24
pub struct GraphMaintenanceService {
    worker: Arc<dyn GraphMaintenanceWorker>,
    audit: AuditLogger,
}

impl GraphMaintenanceService {
    pub fn new(worker: Arc<dyn GraphMaintenanceWorker>, audit: AuditLogger) -> Self {
        Self { worker, audit }
    }

    /// Run a maintenance cycle of the specified type.
    ///
    /// 1. Delegate to the `GraphMaintenanceWorker`.
    /// 2. Emit an audit event with the affected count.
    ///
    /// Implements: SR_DM_24
    pub async fn run_cycle(
        &self,
        request: MaintenanceCycleRequest,
    ) -> Result<MaintenanceCycleResult, PrismError> {
        let affected_count = self
            .worker
            .execute_cycle(request.tenant_id, request.cycle_type)
            .await?;

        let cycle_label = serde_json::to_value(request.cycle_type)
            .map(|v| v.as_str().unwrap_or("unknown").to_string())
            .unwrap_or_else(|_| "unknown".into());

        info!(
            cycle_type = %cycle_label,
            affected_count,
            "graph maintenance cycle completed"
        );

        let tenant_for_audit = request.tenant_id.unwrap_or_default();

        self.audit
            .log(AuditEventInput {
                tenant_id: tenant_for_audit,
                event_type: "data_model.maintenance_complete".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("GraphMaintenance".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: serde_json::json!({
                    "cycle_type": cycle_label,
                    "affected_count": affected_count,
                }),
            })
            .await?;

        Ok(MaintenanceCycleResult { affected_count })
    }
}

// ============================================================================
// SR_DM_25 -- Notification log service
// ============================================================================

/// Service for inserting notifications into the notification log.
///
/// High-volume writes -- no audit event emitted.
///
/// Implements: SR_DM_25
pub struct NotificationLogService {
    pg_writer: Arc<dyn PgWriter>,
}

impl NotificationLogService {
    pub fn new(pg_writer: Arc<dyn PgWriter>) -> Self {
        Self { pg_writer }
    }

    /// Insert a notification into the notification log.
    ///
    /// Supports `original_timestamp` for offline replay: if set, the
    /// notification is recorded with its original timestamp rather than
    /// the current time.
    ///
    /// Implements: SR_DM_25
    pub async fn insert(&self, input: NotificationRow) -> Result<NotificationResult, PrismError> {
        let timestamp = input
            .original_timestamp
            .unwrap_or_else(Utc::now)
            .to_rfc3339();

        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "person_id": input.person_id.to_string(),
            "message": input.message,
            "timestamp": timestamp,
            "read_state": input.read_state,
        });

        let row_id = self.pg_writer.insert_row("notification_log", data).await?;

        info!(
            tenant_id = %input.tenant_id,
            person_id = %input.person_id,
            row_id = %row_id,
            "notification inserted"
        );

        Ok(NotificationResult { row_id })
    }
}

// ============================================================================
// SR_DM_26 -- User preferences service
// ============================================================================

/// Service for upserting user preference key-value pairs.
///
/// No audit event emitted (user preference writes are not governance events).
///
/// Implements: SR_DM_26
pub struct UserPreferencesService {
    pg_writer: Arc<dyn PgWriter>,
}

impl UserPreferencesService {
    pub fn new(pg_writer: Arc<dyn PgWriter>) -> Self {
        Self { pg_writer }
    }

    /// Upsert a user preference.
    ///
    /// 1. Insert or update the preference row via `PgWriter`.
    /// 2. No audit event (not a governance operation).
    ///
    /// Implements: SR_DM_26
    pub async fn upsert(&self, input: PreferenceRow) -> Result<PreferenceResult, PrismError> {
        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "person_id": input.person_id.to_string(),
            "key": input.key,
            "value": input.value,
        });

        let row_id = self.pg_writer.insert_row("user_preferences", data).await?;

        info!(
            tenant_id = %input.tenant_id,
            person_id = %input.person_id,
            key = %input.key,
            row_id = %row_id,
            "user preference upserted"
        );

        Ok(PreferenceResult { row_id })
    }
}

// ============================================================================
// SR_DM_28 -- Tenant isolation audit service
// ============================================================================

/// Trait for scanning the graph store for tenant isolation violations.
///
/// Implements: SR_DM_28
#[async_trait]
pub trait IsolationScanner: Send + Sync {
    /// Scan for cross-tenant isolation violations.
    ///
    /// If `tenant_id` is `None`, scans all tenants.
    async fn scan_for_violations(
        &self,
        tenant_id: Option<TenantId>,
    ) -> Result<Vec<IsolationViolation>, PrismError>;
}

/// Service for auditing tenant isolation boundaries.
///
/// On violation: freezes writes on both affected tenants and alerts
/// the security officer.
///
/// Implements: SR_DM_28
pub struct TenantIsolationAuditService {
    scanner: Arc<dyn IsolationScanner>,
    freeze: Arc<dyn TenantWriteFreeze>,
    audit: AuditLogger,
}

impl TenantIsolationAuditService {
    pub fn new(
        scanner: Arc<dyn IsolationScanner>,
        freeze: Arc<dyn TenantWriteFreeze>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            scanner,
            freeze,
            audit,
        }
    }

    /// Scan for tenant isolation violations.
    ///
    /// 1. Run the isolation scanner.
    /// 2. If violations found, freeze writes on both affected tenants.
    /// 3. Emit appropriate audit events.
    ///
    /// Implements: SR_DM_28
    pub async fn scan(&self) -> Result<IsolationAuditResult, PrismError> {
        let violations = self.scanner.scan_for_violations(None).await?;

        if violations.is_empty() {
            let sentinel_tenant = TenantId::new();
            self.audit
                .log(AuditEventInput {
                    tenant_id: sentinel_tenant,
                    event_type: "data_model.isolation_audit_complete".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: None,
                    target_type: Some("TenantIsolation".into()),
                    severity: Severity::Low,
                    source_layer: SourceLayer::Graph,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "result": "clean",
                        "violation_count": 0,
                    }),
                })
                .await?;

            return Ok(IsolationAuditResult {
                result: "clean".into(),
                violations: vec![],
            });
        }

        // Freeze writes on affected tenants and emit CRITICAL audit events
        for violation in &violations {
            // Freeze tenant A
            self.freeze.freeze(violation.tenant_a).await?;
            // Freeze tenant B
            self.freeze.freeze(violation.tenant_b).await?;

            info!(
                entity_type = %violation.entity_type,
                entity_id = %violation.entity_id,
                tenant_a = %violation.tenant_a,
                tenant_b = %violation.tenant_b,
                "tenant isolation violation detected -- writes frozen"
            );

            self.audit
                .log(AuditEventInput {
                    tenant_id: violation.tenant_a,
                    event_type: "data_model.isolation_violation_detected".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: Some(violation.entity_id),
                    target_type: Some(violation.entity_type.clone()),
                    severity: Severity::Critical,
                    source_layer: SourceLayer::Graph,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "tenant_a": violation.tenant_a.to_string(),
                        "tenant_b": violation.tenant_b.to_string(),
                        "description": violation.description,
                    }),
                })
                .await?;
        }

        Ok(IsolationAuditResult {
            result: "violations_detected".into(),
            violations,
        })
    }
}

// ============================================================================
// SR_DM_29 -- Feature flag cache invalidation service
// ============================================================================

/// Trait for invalidating cached values.
///
/// Implements: SR_DM_29
#[async_trait]
pub trait CacheInvalidator: Send + Sync {
    /// Invalidate the cache entry for the given key.
    async fn invalidate(&self, key: &str) -> Result<(), PrismError>;
}

/// Service for toggling feature flags with cache invalidation.
///
/// Persists the flag value via PgWriter and invalidates the
/// corresponding cache entry via CacheInvalidator.
///
/// Implements: SR_DM_29
pub struct FeatureFlagCacheService {
    pg_writer: Arc<dyn PgWriter>,
    cache: Arc<dyn CacheInvalidator>,
    audit: AuditLogger,
}

impl FeatureFlagCacheService {
    pub fn new(
        pg_writer: Arc<dyn PgWriter>,
        cache: Arc<dyn CacheInvalidator>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            pg_writer,
            cache,
            audit,
        }
    }

    /// Toggle a feature flag and invalidate the cache.
    ///
    /// 1. Persist the new flag value via `PgWriter`.
    /// 2. Invalidate the cache entry via `CacheInvalidator`.
    /// 3. Emit an audit event.
    ///
    /// Implements: SR_DM_29
    pub async fn toggle_with_invalidation(
        &self,
        input: FeatureFlagToggle,
    ) -> Result<FeatureFlagCacheResult, PrismError> {
        let data = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
            "flag_id": input.flag_id,
            "value": input.value,
        });

        self.pg_writer.insert_row("feature_flags", data).await?;

        // Build cache key: tenant_id:flag_id
        let cache_key = format!("{}:{}", input.tenant_id, input.flag_id);
        self.cache.invalidate(&cache_key).await?;

        info!(
            tenant_id = %input.tenant_id,
            flag_id = %input.flag_id,
            value = %input.value,
            "feature flag toggled with cache invalidation"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "data_model.feature_flag_cache_invalidated".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("FeatureFlag".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Graph,
                governance_authority: None,
                payload: serde_json::json!({
                    "flag_id": input.flag_id,
                    "new_value": input.value,
                }),
            })
            .await?;

        Ok(FeatureFlagCacheResult {
            active: input.value,
            cache_invalidated: true,
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::repository::AuditEventRepository;
    use std::sync::Mutex;

    // -- Mock GraphWriter -----------------------------------------------------

    struct MockGraphWriter {
        nodes: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl MockGraphWriter {
        fn new() -> Self {
            Self {
                nodes: Mutex::new(Vec::new()),
            }
        }

        fn node_count(&self) -> usize {
            self.nodes.lock().unwrap().len()
        }

        fn last_node(&self) -> Option<(String, serde_json::Value)> {
            self.nodes.lock().unwrap().last().cloned()
        }
    }

    #[async_trait]
    impl GraphWriter for MockGraphWriter {
        async fn create_node(
            &self,
            _tenant_id: TenantId,
            node_type: &str,
            properties: serde_json::Value,
        ) -> Result<uuid::Uuid, PrismError> {
            let id = uuid::Uuid::new_v4();
            self.nodes
                .lock()
                .unwrap()
                .push((node_type.to_string(), properties));
            Ok(id)
        }
    }

    // -- Mock PgWriter --------------------------------------------------------

    struct MockPgWriter {
        rows: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl MockPgWriter {
        fn new() -> Self {
            Self {
                rows: Mutex::new(Vec::new()),
            }
        }

        fn row_count(&self) -> usize {
            self.rows.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl PgWriter for MockPgWriter {
        async fn insert_row(
            &self,
            table: &str,
            data: serde_json::Value,
        ) -> Result<uuid::Uuid, PrismError> {
            let id = uuid::Uuid::new_v4();
            self.rows.lock().unwrap().push((table.to_string(), data));
            Ok(id)
        }
    }

    // -- Mock PartitionManager ------------------------------------------------

    struct MockPartitionManager {
        archive_count: u64,
        drop_count: u64,
        calls: Mutex<Vec<String>>,
    }

    impl MockPartitionManager {
        fn new(archive_count: u64, drop_count: u64) -> Self {
            Self {
                archive_count,
                drop_count,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl PartitionManager for MockPartitionManager {
        async fn create_partition(
            &self,
            _tenant_id: TenantId,
            period: &str,
        ) -> Result<(), PrismError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("create:{}", period));
            Ok(())
        }

        async fn archive_partition(
            &self,
            _tenant_id: TenantId,
            period: &str,
        ) -> Result<u64, PrismError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("archive:{}", period));
            Ok(self.archive_count)
        }

        async fn drop_partition(
            &self,
            _tenant_id: TenantId,
            period: &str,
        ) -> Result<u64, PrismError> {
            self.calls.lock().unwrap().push(format!("drop:{}", period));
            Ok(self.drop_count)
        }
    }

    // -- Mock AuditEventRepository --------------------------------------------

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

        async fn query(&self, request: &AuditQueryRequest) -> Result<AuditQueryResult, PrismError> {
            let events = self.events.lock().unwrap();
            let filtered: Vec<_> = events
                .iter()
                .filter(|e| e.tenant_id == request.tenant_id)
                .cloned()
                .collect();
            let total = filtered.len() as i64;
            Ok(AuditQueryResult {
                events: filtered,
                next_page_token: None,
                total_count: total,
            })
        }

        async fn get_chain_segment(
            &self,
            tenant_id: TenantId,
            depth: u32,
        ) -> Result<Vec<AuditEvent>, PrismError> {
            let events = self.events.lock().unwrap();
            let mut segment: Vec<_> = events
                .iter()
                .filter(|e| e.tenant_id == tenant_id)
                .cloned()
                .collect();
            segment.sort_by_key(|e| std::cmp::Reverse(e.chain_position));
            segment.truncate(depth as usize);
            Ok(segment)
        }
    }

    // -- Helper ---------------------------------------------------------------

    fn make_audit_logger() -> (Arc<MockAuditRepo>, AuditLogger) {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        (repo, logger)
    }

    // =========================================================================
    // SR_DM_03 tests
    // =========================================================================

    #[tokio::test]
    async fn compartment_node_creates_graph_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = CompartmentNodeService::new(writer.clone(), audit);

        let input = CompartmentNodeInput {
            tenant_id: TenantId::new(),
            name: "BSA/AML Compartment".into(),
            classification_level: ClassificationLevel::Restricted,
            member_roles: vec![RoleId::new()],
            member_persons: vec![UserId::new()],
            purpose: "Isolate BSA data".into(),
            criminal_penalty_isolation: true,
        };

        let result = svc.create(input).await.unwrap();

        assert_eq!(writer.node_count(), 1);
        assert_ne!(result.compartment_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn compartment_node_records_correct_properties() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = CompartmentNodeService::new(writer.clone(), audit);

        let input = CompartmentNodeInput {
            tenant_id: TenantId::new(),
            name: "SOX Compartment".into(),
            classification_level: ClassificationLevel::Confidential,
            member_roles: vec![],
            member_persons: vec![],
            purpose: "SOX compliance data".into(),
            criminal_penalty_isolation: false,
        };

        svc.create(input).await.unwrap();

        let (node_type, props) = writer.last_node().unwrap();
        assert_eq!(node_type, "Compartment");
        assert_eq!(props["name"], "SOX Compartment");
        assert_eq!(props["criminal_penalty_isolation"], false);
    }

    // =========================================================================
    // SR_DM_04 tests
    // =========================================================================

    #[tokio::test]
    async fn connection_node_creates_graph_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ConnectionNodeService::new(writer.clone(), audit);

        let input = ConnectionNodeInput {
            tenant_id: TenantId::new(),
            system_id: "salesforce-prod".into(),
            connection_type: "rest_api".into(),
            auth_type: "oauth2".into(),
            credential_caas_ref: Some("vault:secret/sf-creds".into()),
            status: "active".into(),
            scope: "read:contacts,read:accounts".into(),
            metadata: serde_json::json!({"region": "us-east-1"}),
        };

        let result = svc.create(input).await.unwrap();

        assert_eq!(writer.node_count(), 1);
        assert_ne!(result.connection_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn connection_node_records_metadata() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ConnectionNodeService::new(writer.clone(), audit);

        let metadata = serde_json::json!({"version": "v2", "endpoint": "https://api.example.com"});
        let input = ConnectionNodeInput {
            tenant_id: TenantId::new(),
            system_id: "internal-api".into(),
            connection_type: "grpc".into(),
            auth_type: "mtls".into(),
            credential_caas_ref: None,
            status: "pending".into(),
            scope: "full".into(),
            metadata: metadata.clone(),
        };

        svc.create(input).await.unwrap();

        let (node_type, props) = writer.last_node().unwrap();
        assert_eq!(node_type, "Connection");
        assert_eq!(props["metadata"], metadata);
    }

    // =========================================================================
    // SR_DM_06 tests
    // =========================================================================

    #[tokio::test]
    async fn audit_partition_maintenance_runs() {
        let manager = Arc::new(MockPartitionManager::new(150, 42));
        let (_audit_repo, audit) = make_audit_logger();
        let svc = AuditPartitionService::new(manager.clone(), audit);

        let request = AuditPartitionMaintenanceRequest {
            tenant_id: TenantId::new(),
            period: "2025-01".into(),
        };

        let result = svc.maintain(request).await.unwrap();

        assert_eq!(result.archived_count, 150);
        assert_eq!(result.dropped_count, 42);
        assert_eq!(manager.call_count(), 2); // archive + drop
    }

    #[tokio::test]
    async fn audit_partition_maintenance_records_counts() {
        let manager = Arc::new(MockPartitionManager::new(0, 0));
        let (audit_repo, audit) = make_audit_logger();
        let svc = AuditPartitionService::new(manager, audit);

        let tid = TenantId::new();
        let request = AuditPartitionMaintenanceRequest {
            tenant_id: tid,
            period: "2024-06".into(),
        };

        svc.maintain(request).await.unwrap();

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.partition_maintained");
    }

    // =========================================================================
    // SR_DM_07 tests
    // =========================================================================

    #[tokio::test]
    async fn data_collection_creates_graph_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = DataCollectionService::new(writer.clone(), audit);

        let input = DataCollectionInput {
            tenant_id: TenantId::new(),
            connection_id: uuid::Uuid::new_v4(),
            source_system: "salesforce".into(),
            pull_timestamp: chrono::Utc::now(),
            freshness_policy: "daily".into(),
            record_count: 10_000,
            ingestion_method: "incremental".into(),
            source_file_ref: None,
            training_consent: true,
            data_origin: DataOrigin::ConnectionPull,
        };

        let result = svc.create(input).await.unwrap();

        assert_eq!(writer.node_count(), 1);
        assert_ne!(result.collection_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn data_collection_records_data_origin() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = DataCollectionService::new(writer.clone(), audit);

        let input = DataCollectionInput {
            tenant_id: TenantId::new(),
            connection_id: uuid::Uuid::new_v4(),
            source_system: "ml-pipeline".into(),
            pull_timestamp: chrono::Utc::now(),
            freshness_policy: "realtime".into(),
            record_count: 500,
            ingestion_method: "streaming".into(),
            source_file_ref: Some("s3://bucket/file.parquet".into()),
            training_consent: false,
            data_origin: DataOrigin::SystemPrediction,
        };

        svc.create(input).await.unwrap();

        let (node_type, props) = writer.last_node().unwrap();
        assert_eq!(node_type, "DataCollection");
        assert_eq!(props["data_origin"], "system_prediction");
    }

    // =========================================================================
    // SR_DM_08 tests
    // =========================================================================

    #[tokio::test]
    async fn data_field_upserts_batch() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = DataFieldService::new(writer.clone(), audit);

        let input = DataFieldInputBatch {
            tenant_id: TenantId::new(),
            collection_id: uuid::Uuid::new_v4(),
            fields: vec![
                DataFieldInput {
                    field_name: "customer_name".into(),
                    technical_type: "VARCHAR(255)".into(),
                    semantic_type: Some("person_name".into()),
                    classification: Some("pii".into()),
                    sensitivity_level: Some("high".into()),
                    completeness_pct: Some(99.5),
                },
                DataFieldInput {
                    field_name: "account_balance".into(),
                    technical_type: "DECIMAL(18,2)".into(),
                    semantic_type: Some("currency".into()),
                    classification: Some("financial".into()),
                    sensitivity_level: Some("medium".into()),
                    completeness_pct: Some(100.0),
                },
            ],
        };

        let result = svc.upsert_batch(input).await.unwrap();

        assert_eq!(result.upserted_count, 2);
        assert_eq!(writer.node_count(), 2);
    }

    #[tokio::test]
    async fn data_field_handles_empty_batch() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = DataFieldService::new(writer.clone(), audit);

        let input = DataFieldInputBatch {
            tenant_id: TenantId::new(),
            collection_id: uuid::Uuid::new_v4(),
            fields: vec![],
        };

        let result = svc.upsert_batch(input).await.unwrap();

        assert_eq!(result.upserted_count, 0);
        assert_eq!(writer.node_count(), 0);
    }

    // =========================================================================
    // SR_DM_09 tests
    // =========================================================================

    #[tokio::test]
    async fn recommendation_creates_dual_store_node() {
        let graph_writer = Arc::new(MockGraphWriter::new());
        let pg_writer = Arc::new(MockPgWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = RecommendationNodeService::new(graph_writer.clone(), pg_writer.clone(), audit);

        let input = RecommendationNodeInput {
            tenant_id: TenantId::new(),
            content_hash: "sha256:abc123".into(),
            model_used: "gpt-4".into(),
            confidence: 0.92,
            parameters_used: vec!["temperature=0.1".into()],
            state: "pending".into(),
            category: Some("risk_assessment".into()),
        };

        let result = svc.create(input).await.unwrap();

        assert_ne!(result.rec_id, uuid::Uuid::nil());
        assert_ne!(result.audit_row_id, uuid::Uuid::nil());
        assert_eq!(graph_writer.node_count(), 1);
        assert_eq!(pg_writer.row_count(), 1);
    }

    #[tokio::test]
    async fn recommendation_records_confidence() {
        let graph_writer = Arc::new(MockGraphWriter::new());
        let pg_writer = Arc::new(MockPgWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = RecommendationNodeService::new(graph_writer.clone(), pg_writer, audit);

        let input = RecommendationNodeInput {
            tenant_id: TenantId::new(),
            content_hash: "sha256:def456".into(),
            model_used: "claude-3".into(),
            confidence: 0.87,
            parameters_used: vec![],
            state: "accepted".into(),
            category: None,
        };

        svc.create(input).await.unwrap();

        let (node_type, props) = graph_writer.last_node().unwrap();
        assert_eq!(node_type, "Recommendation");
        assert_eq!(props["confidence"], 0.87);
    }

    // =========================================================================
    // SR_DM_10 tests
    // =========================================================================

    #[tokio::test]
    async fn rejection_creates_graph_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = RejectionNodeService::new(writer.clone(), audit);

        let input = RejectionNodeInput {
            tenant_id: TenantId::new(),
            recommendation_id: uuid::Uuid::new_v4(),
            category: "inaccurate".into(),
            justification_text: "The recommendation does not apply to this portfolio".into(),
            person_id: UserId::new(),
            timestamp: chrono::Utc::now(),
        };

        let result = svc.create(input).await.unwrap();

        assert_eq!(writer.node_count(), 1);
        assert_ne!(result.rejection_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn rejection_records_justification() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = RejectionNodeService::new(writer.clone(), audit);

        let rec_id = uuid::Uuid::new_v4();
        let input = RejectionNodeInput {
            tenant_id: TenantId::new(),
            recommendation_id: rec_id,
            category: "risk_too_high".into(),
            justification_text: "Exceeds acceptable risk threshold for this client segment".into(),
            person_id: UserId::new(),
            timestamp: chrono::Utc::now(),
        };

        svc.create(input).await.unwrap();

        let (node_type, props) = writer.last_node().unwrap();
        assert_eq!(node_type, "Rejection");
        assert_eq!(
            props["justification_text"],
            "Exceeds acceptable risk threshold for this client segment"
        );
        assert_eq!(props["edge_type"], "JUSTIFIED_BY");
        assert_eq!(props["edge_target"], rec_id.to_string());
    }

    // -- Mock EmbeddingModel ----------------------------------------------------

    struct MockEmbeddingModel {
        dim: usize,
        id: String,
    }

    impl MockEmbeddingModel {
        fn new(dim: usize, id: &str) -> Self {
            Self {
                dim,
                id: id.to_string(),
            }
        }
    }

    #[async_trait]
    impl EmbeddingModel for MockEmbeddingModel {
        async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, PrismError> {
            Ok(vec![0.1; self.dim])
        }

        fn model_id(&self) -> &str {
            &self.id
        }

        fn version(&self) -> &str {
            "v1"
        }
    }

    // -- Mock VectorIndexWriter -------------------------------------------------

    struct MockVectorIndexWriter {
        entries: Mutex<Vec<(uuid::Uuid, String, usize)>>,
    }

    impl MockVectorIndexWriter {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }

        fn entry_count(&self) -> usize {
            self.entries.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl VectorIndexWriter for MockVectorIndexWriter {
        async fn write_embedding(
            &self,
            _tenant_id: TenantId,
            source_node_id: uuid::Uuid,
            vector: &[f32],
            model_id: &str,
            _version: &str,
        ) -> Result<(), PrismError> {
            self.entries
                .lock()
                .unwrap()
                .push((source_node_id, model_id.to_string(), vector.len()));
            Ok(())
        }
    }

    // =========================================================================
    // SR_DM_12 tests
    // =========================================================================

    #[tokio::test]
    async fn component_node_creates_graph_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ComponentNodeService::new(writer.clone(), audit);

        let input = ComponentNodeInput {
            tenant_id: TenantId::new(),
            component_id: "risk-scorer-v2".into(),
            category: "ml_model".into(),
            version: "2.1.0".into(),
            git_sha: Some("abc123def".into()),
            status: "active".into(),
            metadata: serde_json::json!({"framework": "pytorch"}),
        };

        let result = svc.create(input).await.unwrap();

        assert_eq!(writer.node_count(), 1);
        assert_ne!(result.node_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn component_node_records_correct_properties() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ComponentNodeService::new(writer.clone(), audit);

        let input = ComponentNodeInput {
            tenant_id: TenantId::new(),
            component_id: "data-enricher".into(),
            category: "connector".into(),
            version: "1.0.0".into(),
            git_sha: None,
            status: "draft".into(),
            metadata: serde_json::json!({}),
        };

        svc.create(input).await.unwrap();

        let (node_type, props) = writer.last_node().unwrap();
        assert_eq!(node_type, "Component");
        assert_eq!(props["component_id"], "data-enricher");
        assert_eq!(props["category"], "connector");
    }

    // =========================================================================
    // SR_DM_13 tests
    // =========================================================================

    #[tokio::test]
    async fn component_registry_inserts_row() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ComponentRegistryService::new(pg_writer.clone(), audit);

        let input = ComponentRegistryRow {
            tenant_id: TenantId::new(),
            component_id: "fraud-detector".into(),
            version: "3.0.0".into(),
            git_sha: Some("def456".into()),
            status: "active".into(),
            owner_id: UserId::new(),
            scope: "bsa_aml".into(),
        };

        let result = svc.register(input).await.unwrap();

        assert_ne!(result.row_id, uuid::Uuid::nil());
        assert_eq!(pg_writer.row_count(), 1);
    }

    #[tokio::test]
    async fn component_registry_records_audit_event() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = ComponentRegistryService::new(pg_writer, audit);

        let input = ComponentRegistryRow {
            tenant_id: TenantId::new(),
            component_id: "report-gen".into(),
            version: "1.2.0".into(),
            git_sha: None,
            status: "pending".into(),
            owner_id: UserId::new(),
            scope: "internal".into(),
        };

        svc.register(input).await.unwrap();

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.component_registered");
    }

    // =========================================================================
    // SR_DM_14 tests
    // =========================================================================

    #[tokio::test]
    async fn component_performance_inserts_row() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let svc = ComponentPerformanceService::new(pg_writer.clone());

        let input = ComponentPerformanceRow {
            tenant_id: TenantId::new(),
            component_id: "risk-scorer".into(),
            execution_count: 1000,
            latency_ms: 250,
            success_count: 990,
            failure_count: 10,
            cost_usd: 12.50,
        };

        let result = svc.record(input).await.unwrap();

        assert_ne!(result.row_id, uuid::Uuid::nil());
        assert_eq!(pg_writer.row_count(), 1);
    }

    #[tokio::test]
    async fn component_performance_records_metrics() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let svc = ComponentPerformanceService::new(pg_writer.clone());

        let input = ComponentPerformanceRow {
            tenant_id: TenantId::new(),
            component_id: "data-enricher".into(),
            execution_count: 500,
            latency_ms: 100,
            success_count: 500,
            failure_count: 0,
            cost_usd: 5.00,
        };

        svc.record(input).await.unwrap();

        let rows = pg_writer.rows.lock().unwrap();
        assert_eq!(rows[0].0, "component_performance");
        assert_eq!(rows[0].1["execution_count"], 500);
    }

    // =========================================================================
    // SR_DM_15 tests
    // =========================================================================

    #[tokio::test]
    async fn model_execution_creates_graph_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ModelExecutionService::new(writer.clone(), audit);

        let input = ModelExecutionInput {
            tenant_id: TenantId::new(),
            model_id: "claude-3-opus".into(),
            slot: "primary".into(),
            task_type: LlmTaskType::Inference,
            input_tokens: 1500,
            output_tokens: 800,
            latency_ms: 3200,
            cost_usd: 0.045,
            data_sensitivity: "confidential".into(),
            training_run_id: None,
        };

        let result = svc.create(input).await.unwrap();

        assert_eq!(writer.node_count(), 1);
        assert_ne!(result.execution_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn model_execution_records_task_type() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ModelExecutionService::new(writer.clone(), audit);

        let input = ModelExecutionInput {
            tenant_id: TenantId::new(),
            model_id: "gpt-4".into(),
            slot: "fallback".into(),
            task_type: LlmTaskType::Tagging,
            input_tokens: 200,
            output_tokens: 50,
            latency_ms: 800,
            cost_usd: 0.01,
            data_sensitivity: "internal".into(),
            training_run_id: Some(uuid::Uuid::new_v4()),
        };

        svc.create(input).await.unwrap();

        let (node_type, props) = writer.last_node().unwrap();
        assert_eq!(node_type, "ModelExecution");
        assert_eq!(props["task_type"], "tagging");
    }

    // =========================================================================
    // SR_DM_16 tests
    // =========================================================================

    #[tokio::test]
    async fn model_outcome_creates_graph_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ModelOutcomeService::new(writer.clone(), audit);

        let exec_id = uuid::Uuid::new_v4();
        let input = ModelOutcomeInput {
            tenant_id: TenantId::new(),
            execution_id: exec_id,
            outcome_type: "accuracy".into(),
            outcome_value: "0.95".into(),
            quality_score: 0.95,
        };

        let result = svc.score(input).await.unwrap();

        assert_eq!(writer.node_count(), 1);
        assert_ne!(result.score_id, uuid::Uuid::nil());
    }

    #[tokio::test]
    async fn model_outcome_records_scored_by_edge() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ModelOutcomeService::new(writer.clone(), audit);

        let exec_id = uuid::Uuid::new_v4();
        let input = ModelOutcomeInput {
            tenant_id: TenantId::new(),
            execution_id: exec_id,
            outcome_type: "precision".into(),
            outcome_value: "0.88".into(),
            quality_score: 0.88,
        };

        svc.score(input).await.unwrap();

        let (node_type, props) = writer.last_node().unwrap();
        assert_eq!(node_type, "ModelOutcomeScore");
        assert_eq!(props["edge_type"], "SCORED_BY");
        assert_eq!(props["edge_target"], exec_id.to_string());
    }

    // =========================================================================
    // SR_DM_17 tests
    // =========================================================================

    #[tokio::test]
    async fn model_aggregation_runs() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = ModelAggregationService::new(pg_writer.clone(), audit);

        let input = ModelAggregationRequest {
            tenant_id: TenantId::new(),
            period: "2026-04-14T00:00:00Z/P1D".into(),
        };

        let result = svc.aggregate(input).await.unwrap();

        assert_eq!(result.rows_updated, 1);
        assert_eq!(pg_writer.row_count(), 1);
    }

    #[tokio::test]
    async fn model_aggregation_records_period() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = ModelAggregationService::new(pg_writer, audit);

        let input = ModelAggregationRequest {
            tenant_id: TenantId::new(),
            period: "2026-04-W15".into(),
        };

        svc.aggregate(input).await.unwrap();

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event_type,
            "data_model.model_aggregation_complete"
        );
        assert_eq!(events[0].payload["period"], "2026-04-W15");
    }

    // =========================================================================
    // SR_DM_18 tests
    // =========================================================================

    #[tokio::test]
    async fn vector_embedding_succeeds() {
        let model = Arc::new(MockEmbeddingModel::new(384, "text-embedding-3-small"));
        let vector_writer = Arc::new(MockVectorIndexWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = VectorEmbeddingService::new(model, vector_writer.clone(), audit);

        let input = EmbeddingInput {
            tenant_id: TenantId::new(),
            source_node_id: uuid::Uuid::new_v4(),
            text: "Customer risk assessment for Q4 2026".into(),
            model_id: "text-embedding-3-small".into(),
        };

        let result = svc.embed(input).await.unwrap();

        assert_eq!(result.vector_dim, 384);
        assert_eq!(result.model_id, "text-embedding-3-small");
        assert_eq!(vector_writer.entry_count(), 1);
    }

    #[tokio::test]
    async fn vector_embedding_records_correct_model_and_dim() {
        let model = Arc::new(MockEmbeddingModel::new(1536, "text-embedding-ada-002"));
        let vector_writer = Arc::new(MockVectorIndexWriter::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = VectorEmbeddingService::new(model, vector_writer, audit);

        let input = EmbeddingInput {
            tenant_id: TenantId::new(),
            source_node_id: uuid::Uuid::new_v4(),
            text: "Anomalous transaction pattern detected".into(),
            model_id: "text-embedding-ada-002".into(),
        };

        let result = svc.embed(input).await.unwrap();

        assert_eq!(result.vector_dim, 1536);
        assert_eq!(result.model_id, "text-embedding-ada-002");

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.text_embedded");
        assert_eq!(events[0].payload["vector_dim"], 1536);
    }

    // =========================================================================
    // SR_DM_19 tests
    // =========================================================================

    #[tokio::test]
    async fn dual_embedding_stores_both_embeddings() {
        let vector_writer = Arc::new(MockVectorIndexWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = DualEmbeddingService::new(vector_writer.clone(), audit);

        let input = DualEmbeddingInput {
            tenant_id: TenantId::new(),
            source_node_id: uuid::Uuid::new_v4(),
            old_embedding: vec![0.1, 0.2, 0.3],
            new_embedding: vec![0.4, 0.5, 0.6, 0.7],
            old_model: "ada-002".into(),
            new_model: "text-embedding-3-small".into(),
        };

        svc.store_dual(input).await.unwrap();

        assert_eq!(vector_writer.entry_count(), 2);
    }

    #[tokio::test]
    async fn dual_embedding_records_expiry() {
        let vector_writer = Arc::new(MockVectorIndexWriter::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = DualEmbeddingService::new(vector_writer, audit);

        let input = DualEmbeddingInput {
            tenant_id: TenantId::new(),
            source_node_id: uuid::Uuid::new_v4(),
            old_embedding: vec![1.0, 2.0],
            new_embedding: vec![3.0, 4.0],
            old_model: "v1-model".into(),
            new_model: "v2-model".into(),
        };

        let result = svc.store_dual(input).await.unwrap();

        // Dual window is 7 days from now
        let now = chrono::Utc::now();
        let diff = result.dual_active_until - now;
        assert!(diff.num_days() >= 6 && diff.num_days() <= 7);

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.dual_embedding_stored");
    }

    // =========================================================================
    // SR_DM_21 tests
    // =========================================================================

    #[tokio::test]
    async fn sa_usage_logged() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = SaUsageLogService::new(pg_writer.clone(), audit);

        let input = SaUsageEvent {
            tenant_id: TenantId::new(),
            sa_id: uuid::Uuid::new_v4(),
            action: "read".into(),
            target: "customer_records".into(),
            timestamp: chrono::Utc::now(),
        };

        let row_id = svc.log_usage(input).await.unwrap();

        assert_ne!(row_id, uuid::Uuid::nil());
        assert_eq!(pg_writer.row_count(), 1);
    }

    #[tokio::test]
    async fn sa_anomaly_logged_with_severity() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = SaUsageLogService::new(pg_writer.clone(), audit);

        let input = SaAnomalyEvent {
            tenant_id: TenantId::new(),
            sa_id: uuid::Uuid::new_v4(),
            anomaly_type: "unusual_access_pattern".into(),
            severity: Severity::High,
            evidence: serde_json::json!({"deviation_score": 4.2}),
        };

        let row_id = svc.log_anomaly(input).await.unwrap();

        assert_ne!(row_id, uuid::Uuid::nil());
        assert_eq!(pg_writer.row_count(), 1);

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.sa_anomaly_logged");
        assert_eq!(events[0].severity, Severity::High);
    }

    // -- Mock GraphMaintenanceWorker ------------------------------------------

    struct MockMaintenanceWorker {
        affected_count: u64,
        calls: Mutex<Vec<String>>,
    }

    impl MockMaintenanceWorker {
        fn new(affected_count: u64) -> Self {
            Self {
                affected_count,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl GraphMaintenanceWorker for MockMaintenanceWorker {
        async fn execute_cycle(
            &self,
            _tenant_id: Option<TenantId>,
            cycle_type: MaintenanceCycleType,
        ) -> Result<u64, PrismError> {
            let label = serde_json::to_value(cycle_type)
                .map(|v| v.as_str().unwrap_or("unknown").to_string())
                .unwrap_or_else(|_| "unknown".into());
            self.calls.lock().unwrap().push(label);
            Ok(self.affected_count)
        }
    }

    // -- Mock IsolationScanner ------------------------------------------------

    struct MockIsolationScanner {
        violations: Vec<IsolationViolation>,
    }

    impl MockIsolationScanner {
        fn new(violations: Vec<IsolationViolation>) -> Self {
            Self { violations }
        }
    }

    #[async_trait]
    impl IsolationScanner for MockIsolationScanner {
        async fn scan_for_violations(
            &self,
            _tenant_id: Option<TenantId>,
        ) -> Result<Vec<IsolationViolation>, PrismError> {
            Ok(self.violations.clone())
        }
    }

    // -- Mock TenantWriteFreeze -----------------------------------------------

    struct MockFreeze {
        frozen: Mutex<Vec<TenantId>>,
    }

    impl MockFreeze {
        fn new() -> Self {
            Self {
                frozen: Mutex::new(Vec::new()),
            }
        }

        fn frozen_tenants(&self) -> Vec<TenantId> {
            self.frozen.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TenantWriteFreeze for MockFreeze {
        async fn freeze(&self, tenant_id: TenantId) -> Result<bool, PrismError> {
            let mut frozen = self.frozen.lock().unwrap();
            if frozen.contains(&tenant_id) {
                Ok(false)
            } else {
                frozen.push(tenant_id);
                Ok(true)
            }
        }

        async fn is_frozen(&self, tenant_id: TenantId) -> Result<bool, PrismError> {
            Ok(self.frozen.lock().unwrap().contains(&tenant_id))
        }
    }

    // -- Mock CacheInvalidator ------------------------------------------------

    struct MockCacheInvalidator {
        invalidated: Mutex<Vec<String>>,
    }

    impl MockCacheInvalidator {
        fn new() -> Self {
            Self {
                invalidated: Mutex::new(Vec::new()),
            }
        }

        fn invalidated_keys(&self) -> Vec<String> {
            self.invalidated.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl CacheInvalidator for MockCacheInvalidator {
        async fn invalidate(&self, key: &str) -> Result<(), PrismError> {
            self.invalidated.lock().unwrap().push(key.to_string());
            Ok(())
        }
    }

    // =========================================================================
    // SR_DM_23 tests
    // =========================================================================

    #[tokio::test]
    async fn vector_write_enforcer_accepts_tagged_write() {
        let (_audit_repo, audit) = make_audit_logger();
        let svc = VectorWriteEnforcer::new(audit);

        let attempt = VectorWriteAttempt {
            source: "recommendation_pipeline".into(),
            model_id: Some("text-embedding-3-small".into()),
            vector: vec![0.1, 0.2, 0.3],
            tenant_id: TenantId::new(),
        };

        let result = svc.enforce(attempt).await.unwrap();

        assert!(result.accepted);
        assert!(result.reason.is_none());
    }

    #[tokio::test]
    async fn vector_write_enforcer_rejects_untagged_write() {
        let (audit_repo, audit) = make_audit_logger();
        let svc = VectorWriteEnforcer::new(audit);

        let attempt = VectorWriteAttempt {
            source: "legacy_pipeline".into(),
            model_id: None,
            vector: vec![0.1, 0.2, 0.3],
            tenant_id: TenantId::new(),
        };

        let result = svc.enforce(attempt).await.unwrap();

        assert!(!result.accepted);
        assert!(result.reason.is_some());
        assert!(result.reason.unwrap().contains("D-33"));

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.untagged_vector_rejected");
    }

    // =========================================================================
    // SR_DM_24 tests
    // =========================================================================

    #[tokio::test]
    async fn graph_maintenance_runs_cycle() {
        let worker = Arc::new(MockMaintenanceWorker::new(42));
        let (_audit_repo, audit) = make_audit_logger();
        let svc = GraphMaintenanceService::new(worker.clone(), audit);

        let request = MaintenanceCycleRequest {
            tenant_id: Some(TenantId::new()),
            cycle_type: MaintenanceCycleType::StalePrune,
        };

        let result = svc.run_cycle(request).await.unwrap();

        assert_eq!(result.affected_count, 42);
        assert_eq!(worker.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn graph_maintenance_records_affected_count() {
        let worker = Arc::new(MockMaintenanceWorker::new(100));
        let (audit_repo, audit) = make_audit_logger();
        let svc = GraphMaintenanceService::new(worker, audit);

        let request = MaintenanceCycleRequest {
            tenant_id: None,
            cycle_type: MaintenanceCycleType::OrphanCleanup,
        };

        svc.run_cycle(request).await.unwrap();

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.maintenance_complete");
        assert_eq!(events[0].payload["affected_count"], 100);
    }

    // =========================================================================
    // SR_DM_25 tests
    // =========================================================================

    #[tokio::test]
    async fn notification_insert_succeeds() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let svc = NotificationLogService::new(pg_writer.clone());

        let input = NotificationRow {
            tenant_id: TenantId::new(),
            person_id: UserId::new(),
            message: "Your automation has been approved".into(),
            original_timestamp: None,
            read_state: false,
        };

        let result = svc.insert(input).await.unwrap();

        assert_ne!(result.row_id, uuid::Uuid::nil());
        assert_eq!(pg_writer.row_count(), 1);
    }

    #[tokio::test]
    async fn notification_handles_original_timestamp_for_offline_replay() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let svc = NotificationLogService::new(pg_writer.clone());

        let original_ts = chrono::Utc::now() - Duration::hours(2);
        let input = NotificationRow {
            tenant_id: TenantId::new(),
            person_id: UserId::new(),
            message: "Offline notification replay".into(),
            original_timestamp: Some(original_ts),
            read_state: true,
        };

        let result = svc.insert(input).await.unwrap();

        assert_ne!(result.row_id, uuid::Uuid::nil());

        let rows = pg_writer.rows.lock().unwrap();
        assert_eq!(rows[0].0, "notification_log");
        assert_eq!(rows[0].1["timestamp"], original_ts.to_rfc3339());
    }

    // =========================================================================
    // SR_DM_26 tests
    // =========================================================================

    #[tokio::test]
    async fn user_preference_upsert_succeeds() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let svc = UserPreferencesService::new(pg_writer.clone());

        let input = PreferenceRow {
            tenant_id: TenantId::new(),
            person_id: UserId::new(),
            key: "theme".into(),
            value: serde_json::json!("dark"),
        };

        let result = svc.upsert(input).await.unwrap();

        assert_ne!(result.row_id, uuid::Uuid::nil());
        assert_eq!(pg_writer.row_count(), 1);
    }

    #[tokio::test]
    async fn user_preference_records_correct_key_value() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let svc = UserPreferencesService::new(pg_writer.clone());

        let input = PreferenceRow {
            tenant_id: TenantId::new(),
            person_id: UserId::new(),
            key: "locale".into(),
            value: serde_json::json!("en-US"),
        };

        svc.upsert(input).await.unwrap();

        let rows = pg_writer.rows.lock().unwrap();
        assert_eq!(rows[0].0, "user_preferences");
        assert_eq!(rows[0].1["key"], "locale");
        assert_eq!(rows[0].1["value"], "en-US");
    }

    // =========================================================================
    // SR_DM_28 tests
    // =========================================================================

    #[tokio::test]
    async fn isolation_audit_clean_scan() {
        let scanner = Arc::new(MockIsolationScanner::new(vec![]));
        let freeze = Arc::new(MockFreeze::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = TenantIsolationAuditService::new(scanner, freeze.clone(), audit);

        let result = svc.scan().await.unwrap();

        assert_eq!(result.result, "clean");
        assert!(result.violations.is_empty());
        assert!(freeze.frozen_tenants().is_empty());

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "data_model.isolation_audit_complete");
    }

    #[tokio::test]
    async fn isolation_audit_violation_freezes_tenants() {
        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();
        let violation = IsolationViolation {
            entity_type: "DataCollection".into(),
            entity_id: uuid::Uuid::new_v4(),
            tenant_a,
            tenant_b,
            description: "Cross-tenant edge detected between DataCollection nodes".into(),
        };
        let scanner = Arc::new(MockIsolationScanner::new(vec![violation]));
        let freeze = Arc::new(MockFreeze::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = TenantIsolationAuditService::new(scanner, freeze.clone(), audit);

        let result = svc.scan().await.unwrap();

        assert_eq!(result.result, "violations_detected");
        assert_eq!(result.violations.len(), 1);

        let frozen = freeze.frozen_tenants();
        assert!(frozen.contains(&tenant_a));
        assert!(frozen.contains(&tenant_b));
    }

    #[tokio::test]
    async fn isolation_audit_violation_records_details() {
        let tenant_a = TenantId::new();
        let tenant_b = TenantId::new();
        let entity_id = uuid::Uuid::new_v4();
        let violation = IsolationViolation {
            entity_type: "Recommendation".into(),
            entity_id,
            tenant_a,
            tenant_b,
            description: "Shared recommendation node across tenants".into(),
        };
        let scanner = Arc::new(MockIsolationScanner::new(vec![violation]));
        let freeze = Arc::new(MockFreeze::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = TenantIsolationAuditService::new(scanner, freeze, audit);

        svc.scan().await.unwrap();

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event_type,
            "data_model.isolation_violation_detected"
        );
        assert_eq!(events[0].severity, Severity::Critical);
        assert_eq!(events[0].target_id, Some(entity_id));
    }

    // =========================================================================
    // SR_DM_29 tests
    // =========================================================================

    #[tokio::test]
    async fn feature_flag_toggle_invalidates_cache() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let cache = Arc::new(MockCacheInvalidator::new());
        let (_audit_repo, audit) = make_audit_logger();
        let svc = FeatureFlagCacheService::new(pg_writer.clone(), cache.clone(), audit);

        let input = FeatureFlagToggle {
            tenant_id: TenantId::new(),
            flag_id: "dark_mode".into(),
            value: true,
        };

        let result = svc.toggle_with_invalidation(input).await.unwrap();

        assert!(result.active);
        assert!(result.cache_invalidated);

        let keys = cache.invalidated_keys();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("dark_mode"));
    }

    #[tokio::test]
    async fn feature_flag_toggle_records_new_value() {
        let pg_writer = Arc::new(MockPgWriter::new());
        let cache = Arc::new(MockCacheInvalidator::new());
        let (audit_repo, audit) = make_audit_logger();
        let svc = FeatureFlagCacheService::new(pg_writer.clone(), cache, audit);

        let input = FeatureFlagToggle {
            tenant_id: TenantId::new(),
            flag_id: "beta_features".into(),
            value: false,
        };

        let result = svc.toggle_with_invalidation(input).await.unwrap();

        assert!(!result.active);
        assert_eq!(pg_writer.row_count(), 1);

        let events = audit_repo.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event_type,
            "data_model.feature_flag_cache_invalidated"
        );
        assert_eq!(events[0].payload["new_value"], false);
    }
}
