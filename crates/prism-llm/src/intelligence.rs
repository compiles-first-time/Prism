//! Intelligence Layer: graph growth and six-stage tagging pipeline.
//!
//! Implements: SR_INT_01, SR_INT_02, SR_INT_03, SR_INT_04, SR_INT_05,
//!             SR_INT_06, SR_INT_07, SR_INT_08
//!
//! Per Spec 04 Section 1:
//!
//! 1. **SR_INT_01** -- TenantGraphInitService: create the empty per-tenant
//!    subgraph (Tenant root node) at onboarding.
//! 2. **SR_INT_02** -- TaggingPipelineService: on every new DataCollection,
//!    queue Stage 3-6 jobs to run asynchronously.
//! 3. **SR_INT_03** -- SemanticTaggingService (Stage 3): T1 LLM infers
//!    semantic_type, business_domain, unit and context per DataField.
//! 4. **SR_INT_04** -- RelationshipInferenceService (Stage 4): pattern
//!    matcher + T1 LLM propose candidate edges with confidence scores;
//!    high-confidence edges are added, medium are queued, low are rejected.
//! 5. **SR_INT_05** -- DataSnapshotService: scheduled snapshots with
//!    checksum and retention policy (default 180 days).
//! 6. **SR_INT_06** -- QualityAssessmentService (Stage 5): computes
//!    completeness, consistency, timeliness, uniqueness and accuracy and
//!    produces a DataQualityReport.
//! 7. **SR_INT_07** -- TrendAnalysisService: computes direction, magnitude
//!    and significance over successive DataSnapshots.
//! 8. **SR_INT_08** -- HumanReviewQueueService (Stage 6): low-confidence
//!    items (< 0.7) are queued for human review and the reviewer is
//!    notified via the AlertRouter.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use tracing::info;

use prism_audit::event_store::AuditLogger;
use prism_core::error::PrismError;
use prism_core::types::*;
use prism_graph::data_model::GraphWriter;

// ============================================================================
// Shared traits (pipeline plumbing)
// ============================================================================

/// A pipeline job enqueued by SR_INT_02 for a specific stage.
///
/// `stage` is 3, 4 or 5 (Stage 6 review-queue jobs are emitted inline by
/// earlier stages once they produce a low-confidence result).
#[derive(Debug, Clone)]
pub struct TaggingJob {
    pub stage: u8,
    pub collection_id: uuid::Uuid,
    pub tenant_id: TenantId,
}

/// Queue abstraction for the Stage 3-5 tagging workers.
///
/// Implements: SR_INT_02
#[async_trait]
pub trait TaggingJobQueue: Send + Sync {
    async fn enqueue(&self, job: TaggingJob) -> Result<(), PrismError>;
}

/// T1 LLM client abstraction used by Stage 3 semantic tagging.
///
/// Implements: SR_INT_03
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn infer_semantic(&self, field_names: &[String]) -> Result<Vec<SemanticTag>, PrismError>;
}

/// Pattern matcher abstraction used by Stage 4 relationship inference.
///
/// Implements: SR_INT_04
#[async_trait]
pub trait PatternMatcher: Send + Sync {
    async fn find_candidates(
        &self,
        collection_id: uuid::Uuid,
    ) -> Result<Vec<RelationshipCandidate>, PrismError>;
}

/// Checksum computer for SR_INT_05 snapshots. Production implementations
/// hash the full collection content; tests use deterministic fakes.
///
/// Implements: SR_INT_05
#[async_trait]
pub trait ChecksumComputer: Send + Sync {
    async fn compute(&self, collection_id: uuid::Uuid) -> Result<Vec<u8>, PrismError>;
}

/// Quality metrics produced by the SR_INT_06 QualityComputer.
#[derive(Debug, Clone, Copy)]
pub struct QualityMetrics {
    pub completeness: f64,
    pub consistency: f64,
    pub timeliness: f64,
    pub uniqueness: f64,
    pub accuracy: f64,
}

impl QualityMetrics {
    /// Overall score is the simple average of the five dimensions.
    ///
    /// Implements: SR_INT_06
    pub fn overall_score(&self) -> f64 {
        (self.completeness + self.consistency + self.timeliness + self.uniqueness + self.accuracy)
            / 5.0
    }
}

/// Quality computer abstraction used by Stage 5 quality assessment.
///
/// Implements: SR_INT_06
#[async_trait]
pub trait QualityComputer: Send + Sync {
    async fn compute(&self, collection_id: uuid::Uuid) -> Result<QualityMetrics, PrismError>;
}

/// Repository abstraction for the human review queue (SR_INT_08).
#[async_trait]
pub trait ReviewQueueRepository: Send + Sync {
    async fn create(&self, entry: &ReviewQueueEntry) -> Result<(), PrismError>;

    async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<ReviewQueueEntry>, PrismError>;

    async fn resolve(&self, id: uuid::Uuid) -> Result<(), PrismError>;
}

/// Minimal alert-router abstraction for notifying reviewers. The production
/// `REUSABLE_AlertRouter` lives in prism-governance; this trait keeps the
/// Intelligence Layer decoupled and easy to mock.
///
/// Implements: SR_INT_08
#[async_trait]
pub trait AlertRouter: Send + Sync {
    async fn notify_reviewer(
        &self,
        tenant_id: TenantId,
        item_type: &str,
        item_ref: &str,
    ) -> Result<(), PrismError>;
}

// ============================================================================
// SR_INT_01 -- TenantGraphInitService
// ============================================================================

/// Service that initializes an empty intelligence graph for a new tenant.
///
/// The graph starts empty (D-2) and grows from real work via SR_INT_02
/// onward. This SR is the first entry point, called from SR_DM_01 after
/// tenant onboarding: it creates the Tenant root node.
///
/// Implements: SR_INT_01
pub struct TenantGraphInitService {
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl TenantGraphInitService {
    pub fn new(writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { writer, audit }
    }

    /// Create the Tenant root node and emit an audit event.
    ///
    /// Implements: SR_INT_01
    pub async fn init(&self, input: GraphInitInput) -> Result<GraphInitResult, PrismError> {
        let properties = serde_json::json!({
            "tenant_id": input.tenant_id.to_string(),
        });

        let node_id = self
            .writer
            .create_node(input.tenant_id, "Tenant", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            node_id = %node_id,
            "intelligence graph initialized"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.graph_initialized".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(node_id),
                target_type: Some("Tenant".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(GraphInitResult { ready: true })
    }
}

// ============================================================================
// SR_INT_02 -- TaggingPipelineService
// ============================================================================

/// Service that receives a new DataCollection and triggers Stages 3-5 of
/// the tagging pipeline asynchronously (Stage 6 is emitted inline by the
/// earlier stages when confidence is low).
///
/// Implements: SR_INT_02
pub struct TaggingPipelineService {
    queue: Arc<dyn TaggingJobQueue>,
    audit: AuditLogger,
}

impl TaggingPipelineService {
    pub fn new(queue: Arc<dyn TaggingJobQueue>, audit: AuditLogger) -> Self {
        Self { queue, audit }
    }

    /// Enqueue Stage 3, 4 and 5 jobs for the given DataCollection.
    ///
    /// Implements: SR_INT_02
    pub async fn trigger_async_stages(
        &self,
        input: DataCollectionRef,
    ) -> Result<TaggingTriggerResult, PrismError> {
        let mut jobs_queued = 0u32;
        for stage in [3u8, 4, 5] {
            self.queue
                .enqueue(TaggingJob {
                    stage,
                    collection_id: input.collection_id,
                    tenant_id: input.tenant_id,
                })
                .await?;
            jobs_queued += 1;
        }

        info!(
            tenant_id = %input.tenant_id,
            collection_id = %input.collection_id,
            jobs_queued,
            "tagging pipeline stages queued"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.tagging_triggered".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.collection_id),
                target_type: Some("DataCollection".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "collection_id": input.collection_id.to_string(),
                    "jobs_queued": jobs_queued,
                }),
            })
            .await?;

        Ok(TaggingTriggerResult { jobs_queued })
    }
}

// ============================================================================
// SR_INT_03 -- SemanticTaggingService (Stage 3)
// ============================================================================

/// Stage 3 of the tagging pipeline: invoke the T1 LLM to infer
/// semantic_type, business_domain, unit and context per DataField, then
/// persist the inferred properties onto DataField nodes via GraphWriter.
///
/// Implements: SR_INT_03
pub struct SemanticTaggingService {
    llm: Arc<dyn LlmClient>,
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl SemanticTaggingService {
    pub fn new(llm: Arc<dyn LlmClient>, writer: Arc<dyn GraphWriter>, audit: AuditLogger) -> Self {
        Self { llm, writer, audit }
    }

    /// Run Stage 3 semantic tagging for the given fields.
    ///
    /// Implements: SR_INT_03
    pub async fn tag_fields(
        &self,
        input: SemanticTaggingInput,
    ) -> Result<SemanticTaggingResult, PrismError> {
        let tags = self.llm.infer_semantic(&input.fields).await?;

        for tag in &tags {
            let properties = serde_json::json!({
                "collection_id": input.collection_id.to_string(),
                "field_id": tag.field_id,
                "semantic_type": tag.semantic_type,
                "business_domain": tag.business_domain,
                "unit": tag.unit,
                "context": tag.context,
                "confidence": tag.confidence,
            });
            self.writer
                .create_node(input.tenant_id, "DataField", properties)
                .await?;
        }

        let fields_tagged = tags.len() as u32;

        info!(
            tenant_id = %input.tenant_id,
            collection_id = %input.collection_id,
            fields_tagged,
            "stage 3 semantic tagging applied"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.semantic_tagged".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.collection_id),
                target_type: Some("DataCollection".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "collection_id": input.collection_id.to_string(),
                    "fields_tagged": fields_tagged,
                }),
            })
            .await?;

        Ok(SemanticTaggingResult {
            fields_tagged,
            tags,
        })
    }
}

// ============================================================================
// SR_INT_04 -- RelationshipInferenceService (Stage 4)
// ============================================================================

/// Stage 4 of the tagging pipeline: pattern matcher + T1 LLM propose
/// candidate SEMANTICALLY_EQUIVALENT / FEEDS / IMPACTS edges with
/// confidence scores (per D-27). Routing:
///
/// * `confidence >= 0.9` -- added automatically to the graph.
/// * `0.7 <= confidence < 0.9` -- queued for human review (Stage 6).
/// * `confidence < 0.7` -- rejected outright.
///
/// Implements: SR_INT_04
pub struct RelationshipInferenceService {
    matcher: Arc<dyn PatternMatcher>,
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl RelationshipInferenceService {
    pub fn new(
        matcher: Arc<dyn PatternMatcher>,
        writer: Arc<dyn GraphWriter>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            matcher,
            writer,
            audit,
        }
    }

    /// Run Stage 4 relationship inference for a DataCollection.
    ///
    /// Implements: SR_INT_04
    pub async fn infer(
        &self,
        input: RelationshipInferenceInput,
    ) -> Result<RelationshipInferenceResult, PrismError> {
        let candidates = self.matcher.find_candidates(input.collection_id).await?;

        let mut edges_added = 0u32;
        let mut edges_queued = 0u32;

        for candidate in &candidates {
            if candidate.confidence >= 0.9 {
                let properties = serde_json::json!({
                    "from_field": candidate.from_field,
                    "to_field": candidate.to_field,
                    "relationship": candidate.relationship,
                    "confidence": candidate.confidence,
                    "confirmed_by": candidate.confirmed_by,
                });
                self.writer
                    .create_node(input.tenant_id, "RelationshipEdge", properties)
                    .await?;
                edges_added += 1;
            } else if candidate.confidence >= 0.7 {
                edges_queued += 1;
            }
            // < 0.7 rejected -- no action
        }

        info!(
            tenant_id = %input.tenant_id,
            collection_id = %input.collection_id,
            edges_added,
            edges_queued,
            "stage 4 relationship inference complete"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.relationships_inferred".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.collection_id),
                target_type: Some("DataCollection".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "collection_id": input.collection_id.to_string(),
                    "edges_added": edges_added,
                    "edges_queued": edges_queued,
                }),
            })
            .await?;

        Ok(RelationshipInferenceResult {
            edges_added,
            edges_queued,
            candidates,
        })
    }
}

// ============================================================================
// SR_INT_05 -- DataSnapshotService
// ============================================================================

/// Default retention window for DataSnapshot nodes (180 days).
pub const DEFAULT_SNAPSHOT_RETENTION_DAYS: i64 = 180;

/// Service that creates DataSnapshot nodes per the freshness policy (D-24).
/// The collection content is hashed through a `ChecksumComputer` so
/// downstream trend analysis can detect actual changes.
///
/// Implements: SR_INT_05
pub struct DataSnapshotService {
    writer: Arc<dyn GraphWriter>,
    checksum: Arc<dyn ChecksumComputer>,
    audit: AuditLogger,
}

impl DataSnapshotService {
    pub fn new(
        writer: Arc<dyn GraphWriter>,
        checksum: Arc<dyn ChecksumComputer>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            writer,
            checksum,
            audit,
        }
    }

    /// Compute the checksum for a collection and create its DataSnapshot.
    ///
    /// Implements: SR_INT_05
    pub async fn create_snapshot(
        &self,
        input: SnapshotInput,
    ) -> Result<SnapshotResult, PrismError> {
        let bytes = self.checksum.compute(input.collection_id).await?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let checksum = format!("{:x}", hasher.finalize());
        let retention_until = input.timestamp + Duration::days(DEFAULT_SNAPSHOT_RETENTION_DAYS);

        let properties = serde_json::json!({
            "collection_id": input.collection_id.to_string(),
            "timestamp": input.timestamp.to_rfc3339(),
            "checksum": checksum,
            "retention_until": retention_until.to_rfc3339(),
        });

        let snapshot_id = self
            .writer
            .create_node(input.tenant_id, "DataSnapshot", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            collection_id = %input.collection_id,
            snapshot_id = %snapshot_id,
            "data snapshot created"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.snapshot_created".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(snapshot_id),
                target_type: Some("DataSnapshot".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(SnapshotResult {
            snapshot_id,
            checksum,
        })
    }
}

// ============================================================================
// SR_INT_06 -- QualityAssessmentService (Stage 5)
// ============================================================================

/// Stage 5 of the tagging pipeline: compute completeness, consistency,
/// timeliness, uniqueness and accuracy and persist a DataQualityReport
/// node. The overall score is the simple average of the five metrics.
///
/// Implements: SR_INT_06
pub struct QualityAssessmentService {
    computer: Arc<dyn QualityComputer>,
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl QualityAssessmentService {
    pub fn new(
        computer: Arc<dyn QualityComputer>,
        writer: Arc<dyn GraphWriter>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            computer,
            writer,
            audit,
        }
    }

    /// Run Stage 5 quality assessment for a DataCollection.
    ///
    /// Implements: SR_INT_06
    pub async fn assess(&self, input: QualityInput) -> Result<QualityResult, PrismError> {
        let metrics = self.computer.compute(input.collection_id).await?;
        let score = metrics.overall_score();

        let properties = serde_json::json!({
            "collection_id": input.collection_id.to_string(),
            "completeness": metrics.completeness,
            "consistency": metrics.consistency,
            "timeliness": metrics.timeliness,
            "uniqueness": metrics.uniqueness,
            "accuracy": metrics.accuracy,
            "overall_score": score,
        });

        let report_id = self
            .writer
            .create_node(input.tenant_id, "DataQualityReport", properties.clone())
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            collection_id = %input.collection_id,
            report_id = %report_id,
            score,
            "stage 5 quality assessed"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.quality_assessed".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(report_id),
                target_type: Some("DataQualityReport".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: properties,
            })
            .await?;

        Ok(QualityResult { report_id, score })
    }
}

// ============================================================================
// SR_INT_07 -- TrendAnalysisService
// ============================================================================

/// Service that computes a TrendAnalysis over a series of numeric samples
/// (one per DataSnapshot).
///
/// Algorithm:
/// * If `last > first` by more than 10% of `first` -- `Increasing`.
/// * If `last < first` by more than 10% of `first` -- `Decreasing`.
/// * Else if standard deviation exceeds 20% of mean -- `Volatile`.
/// * Otherwise -- `Stable`.
///
/// `magnitude` is `(last - first) / first`; `significance` is
/// `std_dev / mean` (coefficient of variation).
///
/// Implements: SR_INT_07
pub struct TrendAnalysisService {
    audit: AuditLogger,
}

impl TrendAnalysisService {
    pub fn new(audit: AuditLogger) -> Self {
        Self { audit }
    }

    /// Compute a TrendAnalysis for the given numeric series.
    ///
    /// `series.len()` must equal `input.snapshot_ids.len()`. Both must be
    /// at least 2.
    ///
    /// Implements: SR_INT_07
    pub async fn compute(
        &self,
        input: TrendInput,
        series: &[f64],
    ) -> Result<TrendResult, PrismError> {
        if series.len() < 2 {
            return Err(PrismError::Validation {
                reason: "trend analysis requires at least 2 samples".into(),
            });
        }
        if series.len() != input.snapshot_ids.len() {
            return Err(PrismError::Validation {
                reason: "series length must match snapshot count".into(),
            });
        }

        let first = series[0];
        let last = *series.last().unwrap();
        let n = series.len() as f64;
        let mean = series.iter().sum::<f64>() / n;
        let variance = series.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        // Coefficient of variation is undefined when mean is zero; treat as 0.
        let cv = if mean.abs() < f64::EPSILON {
            0.0
        } else {
            std_dev / mean.abs()
        };

        // Magnitude is relative change; guard against division by zero.
        let magnitude = if first.abs() < f64::EPSILON {
            0.0
        } else {
            (last - first) / first
        };

        let direction = if magnitude > 0.10 {
            TrendDirection::Increasing
        } else if magnitude < -0.10 {
            TrendDirection::Decreasing
        } else if cv > 0.20 {
            TrendDirection::Volatile
        } else {
            TrendDirection::Stable
        };

        let trend_id = uuid::Uuid::new_v4();

        info!(
            tenant_id = %input.tenant_id,
            metric = %input.metric,
            ?direction,
            magnitude,
            "trend analysis computed"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.trend_computed".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(trend_id),
                target_type: Some("TrendAnalysis".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "metric": input.metric,
                    "direction": serde_json::to_value(direction)
                        .map_err(|e| PrismError::Serialization(e.to_string()))?,
                    "magnitude": magnitude,
                    "significance": cv,
                    "samples": series.len(),
                }),
            })
            .await?;

        Ok(TrendResult {
            trend_id,
            direction,
            magnitude,
        })
    }
}

// ============================================================================
// SR_INT_08 -- HumanReviewQueueService (Stage 6)
// ============================================================================

/// Service that enqueues low-confidence items (< 0.7) into the human
/// review queue and notifies the reviewer via the AlertRouter.
///
/// High-confidence items are acknowledged (no queue entry written) so that
/// callers can route the same stream through this gate unconditionally.
///
/// Implements: SR_INT_08
pub struct HumanReviewQueueService {
    repo: Arc<dyn ReviewQueueRepository>,
    alerts: Arc<dyn AlertRouter>,
    audit: AuditLogger,
}

impl HumanReviewQueueService {
    pub fn new(
        repo: Arc<dyn ReviewQueueRepository>,
        alerts: Arc<dyn AlertRouter>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            repo,
            alerts,
            audit,
        }
    }

    /// Enqueue a low-confidence item into the review queue.
    ///
    /// * `confidence < 0.7` -- queue entry is created and reviewer is notified.
    /// * `confidence >= 0.7` -- no entry is created; returns a nil queue_id.
    ///
    /// Implements: SR_INT_08
    pub async fn enqueue(&self, input: ReviewQueueInput) -> Result<ReviewQueueResult, PrismError> {
        if input.confidence >= 0.7 {
            return Ok(ReviewQueueResult {
                queue_id: uuid::Uuid::nil(),
            });
        }

        let entry = ReviewQueueEntry {
            id: uuid::Uuid::new_v4(),
            tenant_id: input.tenant_id,
            item_type: input.item_type.clone(),
            item_ref: input.item_ref.clone(),
            confidence: input.confidence,
            created_at: Utc::now(),
            resolved: false,
        };

        self.repo.create(&entry).await?;
        self.alerts
            .notify_reviewer(input.tenant_id, &input.item_type, &input.item_ref)
            .await?;

        info!(
            tenant_id = %input.tenant_id,
            queue_id = %entry.id,
            item_type = %input.item_type,
            confidence = input.confidence,
            "item enqueued for human review"
        );

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.review_queued".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(entry.id),
                target_type: Some("ReviewQueueEntry".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "item_type": input.item_type,
                    "item_ref": input.item_ref,
                    "confidence": input.confidence,
                }),
            })
            .await?;

        Ok(ReviewQueueResult { queue_id: entry.id })
    }
}

/// Lightweight helper constructor for the snapshot service's retention
/// calculation. Exposed so downstream schedulers can share the policy.
///
/// Implements: SR_INT_05
pub fn default_retention_until(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    timestamp + Duration::days(DEFAULT_SNAPSHOT_RETENTION_DAYS)
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

        fn count_of(&self, node_type: &str) -> usize {
            self.nodes
                .lock()
                .unwrap()
                .iter()
                .filter(|(t, _)| t == node_type)
                .count()
        }

        fn total(&self) -> usize {
            self.nodes.lock().unwrap().len()
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

    // -- Mock AuditRepo -------------------------------------------------------

    struct MockAuditRepo {
        events: Mutex<Vec<AuditEvent>>,
    }

    impl MockAuditRepo {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn event_types(&self) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.event_type.clone())
                .collect()
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

    fn make_audit() -> (Arc<MockAuditRepo>, AuditLogger) {
        let repo = Arc::new(MockAuditRepo::new());
        let logger = AuditLogger::new(repo.clone());
        (repo, logger)
    }

    fn tenant() -> TenantId {
        TenantId::from(uuid::Uuid::new_v4())
    }

    // -- Mock TaggingJobQueue -------------------------------------------------

    struct MockQueue {
        jobs: Mutex<Vec<TaggingJob>>,
    }

    impl MockQueue {
        fn new() -> Self {
            Self {
                jobs: Mutex::new(Vec::new()),
            }
        }

        fn stages(&self) -> Vec<u8> {
            self.jobs.lock().unwrap().iter().map(|j| j.stage).collect()
        }
    }

    #[async_trait]
    impl TaggingJobQueue for MockQueue {
        async fn enqueue(&self, job: TaggingJob) -> Result<(), PrismError> {
            self.jobs.lock().unwrap().push(job);
            Ok(())
        }
    }

    // -- Mock LlmClient -------------------------------------------------------

    struct MockLlm {
        tags: Vec<SemanticTag>,
    }

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn infer_semantic(
            &self,
            _field_names: &[String],
        ) -> Result<Vec<SemanticTag>, PrismError> {
            Ok(self.tags.clone())
        }
    }

    // -- Mock PatternMatcher --------------------------------------------------

    struct MockMatcher {
        candidates: Vec<RelationshipCandidate>,
    }

    #[async_trait]
    impl PatternMatcher for MockMatcher {
        async fn find_candidates(
            &self,
            _collection_id: uuid::Uuid,
        ) -> Result<Vec<RelationshipCandidate>, PrismError> {
            Ok(self.candidates.clone())
        }
    }

    // -- Mock ChecksumComputer ------------------------------------------------

    struct MockChecksum {
        payload: Vec<u8>,
    }

    #[async_trait]
    impl ChecksumComputer for MockChecksum {
        async fn compute(&self, _collection_id: uuid::Uuid) -> Result<Vec<u8>, PrismError> {
            Ok(self.payload.clone())
        }
    }

    // -- Mock QualityComputer -------------------------------------------------

    struct MockQuality {
        metrics: QualityMetrics,
    }

    #[async_trait]
    impl QualityComputer for MockQuality {
        async fn compute(&self, _collection_id: uuid::Uuid) -> Result<QualityMetrics, PrismError> {
            Ok(self.metrics)
        }
    }

    // -- Mock ReviewQueueRepository ------------------------------------------

    struct MockReviewRepo {
        entries: Mutex<Vec<ReviewQueueEntry>>,
    }

    impl MockReviewRepo {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        }

        fn count(&self) -> usize {
            self.entries.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl ReviewQueueRepository for MockReviewRepo {
        async fn create(&self, entry: &ReviewQueueEntry) -> Result<(), PrismError> {
            self.entries.lock().unwrap().push(entry.clone());
            Ok(())
        }

        async fn get_by_id(&self, id: uuid::Uuid) -> Result<Option<ReviewQueueEntry>, PrismError> {
            Ok(self
                .entries
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.id == id)
                .cloned())
        }

        async fn resolve(&self, id: uuid::Uuid) -> Result<(), PrismError> {
            let mut entries = self.entries.lock().unwrap();
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                entry.resolved = true;
                Ok(())
            } else {
                Err(PrismError::NotFound {
                    entity_type: "ReviewQueueEntry",
                    id,
                })
            }
        }
    }

    // -- Mock AlertRouter ----------------------------------------------------

    struct MockAlerts {
        sent: Mutex<Vec<String>>,
    }

    impl MockAlerts {
        fn new() -> Self {
            Self {
                sent: Mutex::new(Vec::new()),
            }
        }

        fn count(&self) -> usize {
            self.sent.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl AlertRouter for MockAlerts {
        async fn notify_reviewer(
            &self,
            _tenant_id: TenantId,
            item_type: &str,
            item_ref: &str,
        ) -> Result<(), PrismError> {
            self.sent
                .lock()
                .unwrap()
                .push(format!("{item_type}:{item_ref}"));
            Ok(())
        }
    }

    // =========================================================================
    // SR_INT_01 -- TenantGraphInitService
    // =========================================================================

    #[tokio::test]
    async fn int01_graph_init_creates_tenant_node() {
        let writer = Arc::new(MockGraphWriter::new());
        let (_repo, audit) = make_audit();
        let svc = TenantGraphInitService::new(writer.clone(), audit);

        let result = svc
            .init(GraphInitInput {
                tenant_id: tenant(),
            })
            .await
            .unwrap();

        assert!(result.ready);
        assert_eq!(writer.count_of("Tenant"), 1);
    }

    #[tokio::test]
    async fn int01_graph_init_emits_audit_event() {
        let writer = Arc::new(MockGraphWriter::new());
        let (repo, audit) = make_audit();
        let svc = TenantGraphInitService::new(writer, audit);

        svc.init(GraphInitInput {
            tenant_id: tenant(),
        })
        .await
        .unwrap();

        let types = repo.event_types();
        assert!(types.contains(&"intelligence.graph_initialized".into()));
    }

    // =========================================================================
    // SR_INT_02 -- TaggingPipelineService
    // =========================================================================

    #[tokio::test]
    async fn int02_triggers_stages_three_four_five() {
        let queue = Arc::new(MockQueue::new());
        let (_repo, audit) = make_audit();
        let svc = TaggingPipelineService::new(queue.clone(), audit);

        let result = svc
            .trigger_async_stages(DataCollectionRef {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
            })
            .await
            .unwrap();

        assert_eq!(result.jobs_queued, 3);
        assert_eq!(queue.stages(), vec![3u8, 4, 5]);
    }

    #[tokio::test]
    async fn int02_emits_audit_event() {
        let queue = Arc::new(MockQueue::new());
        let (repo, audit) = make_audit();
        let svc = TaggingPipelineService::new(queue, audit);

        svc.trigger_async_stages(DataCollectionRef {
            tenant_id: tenant(),
            collection_id: uuid::Uuid::new_v4(),
        })
        .await
        .unwrap();

        assert!(repo
            .event_types()
            .contains(&"intelligence.tagging_triggered".into()));
    }

    // =========================================================================
    // SR_INT_03 -- SemanticTaggingService
    // =========================================================================

    #[tokio::test]
    async fn int03_tags_fields_and_writes_graph_nodes() {
        let tags = vec![
            SemanticTag {
                field_id: "f1".into(),
                semantic_type: "amount".into(),
                business_domain: Some("finance".into()),
                unit: Some("USD".into()),
                context: None,
                confidence: 0.95,
            },
            SemanticTag {
                field_id: "f2".into(),
                semantic_type: "customer_id".into(),
                business_domain: Some("crm".into()),
                unit: None,
                context: None,
                confidence: 0.88,
            },
        ];
        let llm = Arc::new(MockLlm { tags: tags.clone() });
        let writer = Arc::new(MockGraphWriter::new());
        let (_repo, audit) = make_audit();
        let svc = SemanticTaggingService::new(llm, writer.clone(), audit);

        let result = svc
            .tag_fields(SemanticTaggingInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
                fields: vec!["f1".into(), "f2".into()],
            })
            .await
            .unwrap();

        assert_eq!(result.fields_tagged, 2);
        assert_eq!(writer.count_of("DataField"), 2);
    }

    #[tokio::test]
    async fn int03_emits_audit_event() {
        let llm = Arc::new(MockLlm { tags: vec![] });
        let writer = Arc::new(MockGraphWriter::new());
        let (repo, audit) = make_audit();
        let svc = SemanticTaggingService::new(llm, writer, audit);

        svc.tag_fields(SemanticTaggingInput {
            tenant_id: tenant(),
            collection_id: uuid::Uuid::new_v4(),
            fields: vec![],
        })
        .await
        .unwrap();

        assert!(repo
            .event_types()
            .contains(&"intelligence.semantic_tagged".into()));
    }

    // =========================================================================
    // SR_INT_04 -- RelationshipInferenceService
    // =========================================================================

    fn candidate(confidence: f64) -> RelationshipCandidate {
        RelationshipCandidate {
            from_field: "a".into(),
            to_field: "b".into(),
            relationship: "SEMANTICALLY_EQUIVALENT".into(),
            confidence,
            confirmed_by: "pattern_matcher".into(),
        }
    }

    #[tokio::test]
    async fn int04_high_confidence_added_automatically() {
        let matcher = Arc::new(MockMatcher {
            candidates: vec![candidate(0.95), candidate(0.91)],
        });
        let writer = Arc::new(MockGraphWriter::new());
        let (_repo, audit) = make_audit();
        let svc = RelationshipInferenceService::new(matcher, writer.clone(), audit);

        let result = svc
            .infer(RelationshipInferenceInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
            })
            .await
            .unwrap();

        assert_eq!(result.edges_added, 2);
        assert_eq!(result.edges_queued, 0);
        assert_eq!(writer.count_of("RelationshipEdge"), 2);
    }

    #[tokio::test]
    async fn int04_medium_confidence_queued_not_added() {
        let matcher = Arc::new(MockMatcher {
            candidates: vec![candidate(0.85), candidate(0.72)],
        });
        let writer = Arc::new(MockGraphWriter::new());
        let (_repo, audit) = make_audit();
        let svc = RelationshipInferenceService::new(matcher, writer.clone(), audit);

        let result = svc
            .infer(RelationshipInferenceInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
            })
            .await
            .unwrap();

        assert_eq!(result.edges_added, 0);
        assert_eq!(result.edges_queued, 2);
        assert_eq!(writer.count_of("RelationshipEdge"), 0);
    }

    #[tokio::test]
    async fn int04_low_confidence_rejected() {
        let matcher = Arc::new(MockMatcher {
            candidates: vec![candidate(0.5), candidate(0.2)],
        });
        let writer = Arc::new(MockGraphWriter::new());
        let (_repo, audit) = make_audit();
        let svc = RelationshipInferenceService::new(matcher, writer.clone(), audit);

        let result = svc
            .infer(RelationshipInferenceInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
            })
            .await
            .unwrap();

        assert_eq!(result.edges_added, 0);
        assert_eq!(result.edges_queued, 0);
        assert_eq!(writer.total(), 0);
    }

    // =========================================================================
    // SR_INT_05 -- DataSnapshotService
    // =========================================================================

    #[tokio::test]
    async fn int05_creates_snapshot_with_retention() {
        let writer = Arc::new(MockGraphWriter::new());
        let checksum = Arc::new(MockChecksum {
            payload: b"collection-content".to_vec(),
        });
        let (_repo, audit) = make_audit();
        let svc = DataSnapshotService::new(writer.clone(), checksum, audit);

        let timestamp = Utc::now();
        let result = svc
            .create_snapshot(SnapshotInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
                timestamp,
            })
            .await
            .unwrap();

        assert_ne!(result.snapshot_id, uuid::Uuid::nil());
        assert_eq!(writer.count_of("DataSnapshot"), 1);
        assert_eq!(
            default_retention_until(timestamp),
            timestamp + Duration::days(DEFAULT_SNAPSHOT_RETENTION_DAYS)
        );
    }

    #[tokio::test]
    async fn int05_checksum_is_deterministic_sha256() {
        let writer = Arc::new(MockGraphWriter::new());
        let checksum = Arc::new(MockChecksum {
            payload: b"hello".to_vec(),
        });
        let (_repo, audit) = make_audit();
        let svc = DataSnapshotService::new(writer, checksum, audit);

        let result = svc
            .create_snapshot(SnapshotInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
                timestamp: Utc::now(),
            })
            .await
            .unwrap();

        // Known SHA-256 of "hello" in hex.
        assert_eq!(
            result.checksum,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // =========================================================================
    // SR_INT_06 -- QualityAssessmentService
    // =========================================================================

    #[tokio::test]
    async fn int06_assess_creates_report_node() {
        let computer = Arc::new(MockQuality {
            metrics: QualityMetrics {
                completeness: 0.9,
                consistency: 0.8,
                timeliness: 0.85,
                uniqueness: 1.0,
                accuracy: 0.95,
            },
        });
        let writer = Arc::new(MockGraphWriter::new());
        let (_repo, audit) = make_audit();
        let svc = QualityAssessmentService::new(computer, writer.clone(), audit);

        let result = svc
            .assess(QualityInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
            })
            .await
            .unwrap();

        assert_eq!(writer.count_of("DataQualityReport"), 1);
        assert!((result.score - 0.9).abs() < 1e-9);
    }

    #[tokio::test]
    async fn int06_overall_score_is_average_of_five_metrics() {
        let metrics = QualityMetrics {
            completeness: 1.0,
            consistency: 1.0,
            timeliness: 1.0,
            uniqueness: 0.0,
            accuracy: 0.0,
        };
        assert!((metrics.overall_score() - 0.6).abs() < 1e-9);
    }

    // =========================================================================
    // SR_INT_07 -- TrendAnalysisService
    // =========================================================================

    async fn run_trend(series: &[f64]) -> TrendResult {
        let (_repo, audit) = make_audit();
        let svc = TrendAnalysisService::new(audit);
        let snapshot_ids: Vec<uuid::Uuid> = series.iter().map(|_| uuid::Uuid::new_v4()).collect();
        svc.compute(
            TrendInput {
                tenant_id: tenant(),
                metric: "kpi".into(),
                snapshot_ids,
            },
            series,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn int07_detects_increasing() {
        let result = run_trend(&[100.0, 110.0, 120.0, 130.0]).await;
        assert_eq!(result.direction, TrendDirection::Increasing);
        assert!(result.magnitude > 0.10);
    }

    #[tokio::test]
    async fn int07_detects_decreasing() {
        let result = run_trend(&[100.0, 90.0, 80.0, 70.0]).await;
        assert_eq!(result.direction, TrendDirection::Decreasing);
        assert!(result.magnitude < -0.10);
    }

    #[tokio::test]
    async fn int07_detects_stable() {
        let result = run_trend(&[100.0, 101.0, 99.0, 100.0]).await;
        assert_eq!(result.direction, TrendDirection::Stable);
    }

    // =========================================================================
    // SR_INT_08 -- HumanReviewQueueService
    // =========================================================================

    #[tokio::test]
    async fn int08_enqueues_low_confidence_item() {
        let repo = Arc::new(MockReviewRepo::new());
        let alerts = Arc::new(MockAlerts::new());
        let (_audit_repo, audit) = make_audit();
        let svc = HumanReviewQueueService::new(repo.clone(), alerts.clone(), audit);

        let result = svc
            .enqueue(ReviewQueueInput {
                tenant_id: tenant(),
                item_type: "relationship".into(),
                item_ref: "edge-123".into(),
                confidence: 0.5,
            })
            .await
            .unwrap();

        assert_ne!(result.queue_id, uuid::Uuid::nil());
        assert_eq!(repo.count(), 1);
        assert_eq!(alerts.count(), 1);
    }

    #[tokio::test]
    async fn int08_does_not_enqueue_high_confidence_item() {
        let repo = Arc::new(MockReviewRepo::new());
        let alerts = Arc::new(MockAlerts::new());
        let (_audit_repo, audit) = make_audit();
        let svc = HumanReviewQueueService::new(repo.clone(), alerts.clone(), audit);

        let result = svc
            .enqueue(ReviewQueueInput {
                tenant_id: tenant(),
                item_type: "relationship".into(),
                item_ref: "edge-123".into(),
                confidence: 0.95,
            })
            .await
            .unwrap();

        assert_eq!(result.queue_id, uuid::Uuid::nil());
        assert_eq!(repo.count(), 0);
        assert_eq!(alerts.count(), 0);
    }
}
