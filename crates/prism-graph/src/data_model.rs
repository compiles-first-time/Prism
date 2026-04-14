//! Data model node services for dual-store (PG + Neo4j) persistence.
//!
//! Implements: SR_DM_03, SR_DM_04, SR_DM_06, SR_DM_07, SR_DM_08, SR_DM_09, SR_DM_10
//!
//! All services use trait-based abstractions (`GraphWriter`, `PgWriter`,
//! `PartitionManager`) so that mock implementations can be used in tests
//! while real Neo4j / PG backends are connected later.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use prism_audit::event_store::AuditLogger;
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
}
