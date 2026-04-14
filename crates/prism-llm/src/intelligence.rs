//! Intelligence Layer: graph growth, six-stage tagging pipeline, coverage,
//! process mapping, vector search, maintenance, CIA, SDA, research agent,
//! cross-tenant learning, query rewrite, proactive triggers, cost estimation,
//! bulk import, read-through cache, DR drills, and tenant offboarding.
//!
//! Implements: SR_INT_01, SR_INT_02, SR_INT_03, SR_INT_04, SR_INT_05,
//!             SR_INT_06, SR_INT_07, SR_INT_08, SR_INT_09, SR_INT_10,
//!             SR_INT_11, SR_INT_12, SR_INT_13, SR_INT_14, SR_INT_15,
//!             SR_INT_16, SR_INT_17, SR_INT_18, SR_INT_19, SR_INT_20,
//!             SR_INT_21, SR_INT_22, SR_INT_23, SR_INT_24, SR_INT_25,
//!             SR_INT_26, SR_INT_27, SR_INT_28, SR_INT_29, SR_INT_30
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
// SR_INT_09 -- CoverageCalculator
// ============================================================================

/// Per-dimension raw counts used by the coverage calculator.
///
/// Each pair `(known, total)` represents how much of the dimension is
/// currently covered by the tenant graph. Values of `total = 0` mean the
/// dimension is not applicable and the coverage is reported as `1.0`.
///
/// Implements: SR_INT_09
#[derive(Debug, Clone, Copy, Default)]
pub struct CoverageCounts {
    pub systems_known: u64,
    pub systems_total: u64,
    pub processes_known: u64,
    pub processes_total: u64,
    pub data_collections_known: u64,
    pub data_collections_total: u64,
    pub departments_known: u64,
    pub departments_total: u64,
    pub relationships_known: u64,
    pub relationships_total: u64,
}

/// Source of per-dimension coverage counts, implemented over the graph store.
///
/// Implements: SR_INT_09
#[async_trait]
pub trait CoverageSource: Send + Sync {
    async fn get_counts(&self, tenant_id: TenantId) -> Result<CoverageCounts, PrismError>;
}

/// Compute coverage percentages across the five dimensions and emit an
/// audit event so downstream services (`SR_GOV_71`, `SR_DS_*`) can weight
/// their confidence accordingly.
///
/// Implements: SR_INT_09
pub struct CoverageCalculator {
    source: Arc<dyn CoverageSource>,
    audit: AuditLogger,
}

impl CoverageCalculator {
    pub fn new(source: Arc<dyn CoverageSource>, audit: AuditLogger) -> Self {
        Self { source, audit }
    }

    /// Compute per-dimension coverage, collect limitations for dimensions
    /// below 50%, and log the `intelligence.coverage_computed` audit event.
    ///
    /// Implements: SR_INT_09
    pub async fn compute(&self, input: CoverageRequest) -> Result<CoverageResult, PrismError> {
        let counts = self.source.get_counts(input.tenant_id).await?;

        let pct = |known: u64, total: u64| -> f64 {
            if total == 0 {
                1.0
            } else {
                (known as f64) / (total as f64)
            }
        };

        let dimensions = vec![
            (
                CoverageDimension::System,
                pct(counts.systems_known, counts.systems_total),
            ),
            (
                CoverageDimension::Process,
                pct(counts.processes_known, counts.processes_total),
            ),
            (
                CoverageDimension::Data,
                pct(counts.data_collections_known, counts.data_collections_total),
            ),
            (
                CoverageDimension::Department,
                pct(counts.departments_known, counts.departments_total),
            ),
            (
                CoverageDimension::Relationship,
                pct(counts.relationships_known, counts.relationships_total),
            ),
        ];

        let limitations: Vec<String> = dimensions
            .iter()
            .filter(|(_, p)| *p < 0.5)
            .map(|(d, p)| format!("{:?} coverage at {:.0}% (<50%)", d, p * 100.0))
            .collect();

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.coverage_computed".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("Coverage".into()),
                severity: if limitations.is_empty() {
                    Severity::Low
                } else {
                    Severity::Medium
                },
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "limitations_count": limitations.len(),
                }),
            })
            .await?;

        Ok(CoverageResult {
            dimensions,
            limitations,
        })
    }
}

// ============================================================================
// SR_INT_10 -- ProcessEmergenceDetector
// ============================================================================

/// Pattern matcher that finds emergent `Process` candidates by correlating
/// field overlap, FEEDS edges, and timing patterns between components and
/// DataCollections.
///
/// Implements: SR_INT_10
#[async_trait]
pub trait ProcessPatternMatcher: Send + Sync {
    async fn find_process_candidates(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<ProcessCandidate>, PrismError>;
}

/// Discovery window after which unconfirmed candidates are considered expired.
///
/// Per Spec 04 Section 2 Q&A #2: candidates that are never confirmed expire
/// after 30 days and are demoted to `discarded`.
///
/// Implements: SR_INT_10
pub const PROCESS_CANDIDATE_EXPIRY_DAYS: i64 = 30;

/// Detect emergent `Process` candidates, materialize them as graph nodes in
/// status `pending_confirmation`, and emit one audit event per candidate.
///
/// Implements: SR_INT_10
pub struct ProcessEmergenceDetector {
    matcher: Arc<dyn ProcessPatternMatcher>,
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl ProcessEmergenceDetector {
    pub fn new(
        matcher: Arc<dyn ProcessPatternMatcher>,
        writer: Arc<dyn GraphWriter>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            matcher,
            writer,
            audit,
        }
    }

    /// Find candidates, write them to the graph with an expiry timestamp, and
    /// audit each discovery.
    ///
    /// Implements: SR_INT_10
    pub async fn discover(
        &self,
        input: ProcessDiscoveryInput,
    ) -> Result<ProcessDiscoveryResult, PrismError> {
        let mut candidates = self
            .matcher
            .find_process_candidates(input.tenant_id)
            .await?;

        for candidate in candidates.iter_mut() {
            candidate.status = "pending_confirmation".to_string();
            let expires_at = candidate.created_at + Duration::days(PROCESS_CANDIDATE_EXPIRY_DAYS);

            let properties = serde_json::json!({
                "candidate_id": candidate.id.to_string(),
                "suggested_name": candidate.suggested_name,
                "component_ids": candidate.component_ids
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>(),
                "confidence": candidate.confidence,
                "status": candidate.status,
                "expires_at": expires_at,
            });

            let node_id = self
                .writer
                .create_node(input.tenant_id, "ProcessCandidate", properties.clone())
                .await?;

            self.audit
                .log(AuditEventInput {
                    tenant_id: input.tenant_id,
                    event_type: "intelligence.process_discovered".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: Some(node_id),
                    target_type: Some("ProcessCandidate".into()),
                    severity: Severity::Low,
                    source_layer: SourceLayer::Llm,
                    governance_authority: None,
                    payload: properties,
                })
                .await?;
        }

        Ok(ProcessDiscoveryResult { candidates })
    }
}

// ============================================================================
// SR_INT_11 -- DataGroupMembershipService
// ============================================================================

/// Repository for DataGroup membership (per D-48).
///
/// Implements: SR_INT_11
#[async_trait]
pub trait DataGroupRepository: Send + Sync {
    /// Add a membership row. Returns `Ok(false)` if the membership already
    /// exists (idempotent).
    async fn add_member(&self, membership: &DataGroupMembership) -> Result<bool, PrismError>;

    async fn get_members(
        &self,
        tenant_id: TenantId,
        group_id: uuid::Uuid,
    ) -> Result<Vec<DataGroupMembership>, PrismError>;

    async fn remove_member(
        &self,
        tenant_id: TenantId,
        group_id: uuid::Uuid,
        collection_id: uuid::Uuid,
    ) -> Result<(), PrismError>;
}

/// Service that materializes `MEMBER_OF` edges grouping DataCollections into
/// DataGroup nodes (D-48).
///
/// Implements: SR_INT_11
pub struct DataGroupMembershipService {
    repo: Arc<dyn DataGroupRepository>,
    writer: Arc<dyn GraphWriter>,
}

impl DataGroupMembershipService {
    pub fn new(repo: Arc<dyn DataGroupRepository>, writer: Arc<dyn GraphWriter>) -> Self {
        Self { repo, writer }
    }

    /// Add each collection as a member of the group, creating a `MEMBER_OF`
    /// edge node for each newly added membership (duplicates are skipped).
    ///
    /// Implements: SR_INT_11
    pub async fn add_to_group(
        &self,
        input: DataGroupingInput,
    ) -> Result<DataGroupingResult, PrismError> {
        let mut added: u32 = 0;

        for collection_id in input.collection_ids.iter().copied() {
            let membership = DataGroupMembership {
                group_id: input.group_id,
                collection_id,
                tenant_id: input.tenant_id,
                added_at: Utc::now(),
            };

            let inserted = self.repo.add_member(&membership).await?;
            if !inserted {
                continue;
            }

            let properties = serde_json::json!({
                "group_id": input.group_id.to_string(),
                "collection_id": collection_id.to_string(),
                "edge_type": "MEMBER_OF",
            });

            self.writer
                .create_node(input.tenant_id, "MemberOfEdge", properties)
                .await?;

            added += 1;
        }

        Ok(DataGroupingResult {
            members_added: added,
        })
    }
}

// ============================================================================
// SR_INT_12 -- TagWeightService
// ============================================================================

/// Default tag weights per D-49.
///
/// Implements: SR_INT_12
pub const DEFAULT_TAG_WEIGHT_SECURITY: f64 = 1.0;
/// Implements: SR_INT_12
pub const DEFAULT_TAG_WEIGHT_BUSINESS: f64 = 0.7;
/// Implements: SR_INT_12
pub const DEFAULT_TAG_WEIGHT_TECHNICAL: f64 = 0.5;

/// Per-tenant tag-weight override config repository. Returns the overrides
/// for a tenant (categories not present fall back to defaults).
///
/// Implements: SR_INT_12
#[async_trait]
pub trait TagWeightConfigRepository: Send + Sync {
    async fn get_overrides(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<(TagCategory, f64)>, PrismError>;
}

/// Evaluate tag weights for a tenant operation, layering tenant overrides on
/// top of the D-49 defaults.
///
/// Implements: SR_INT_12
pub struct TagWeightService {
    repo: Arc<dyn TagWeightConfigRepository>,
}

impl TagWeightService {
    pub fn new(repo: Arc<dyn TagWeightConfigRepository>) -> Self {
        Self { repo }
    }

    fn default_weight(category: TagCategory) -> f64 {
        match category {
            TagCategory::Security => DEFAULT_TAG_WEIGHT_SECURITY,
            TagCategory::Business => DEFAULT_TAG_WEIGHT_BUSINESS,
            TagCategory::Technical => DEFAULT_TAG_WEIGHT_TECHNICAL,
        }
    }

    /// Resolve the effective weight for each requested tag category.
    ///
    /// Implements: SR_INT_12
    pub async fn compute_weights(
        &self,
        input: TagWeightInput,
    ) -> Result<TagWeightResult, PrismError> {
        let overrides = self.repo.get_overrides(input.tenant_id).await?;

        let weights = input
            .tag_categories
            .into_iter()
            .map(|category| {
                let w = overrides
                    .iter()
                    .find(|(c, _)| *c == category)
                    .map(|(_, w)| *w)
                    .unwrap_or_else(|| Self::default_weight(category));
                (category, w)
            })
            .collect();

        Ok(TagWeightResult { weights })
    }
}

// ============================================================================
// SR_INT_13 -- CompletenessTagService
// ============================================================================

/// Apply completeness metadata (D-50) onto a DataCollection node so that
/// Decision Support can weight partial data lower than full data.
///
/// Implements: SR_INT_13
pub struct CompletenessTagService {
    writer: Arc<dyn GraphWriter>,
}

impl CompletenessTagService {
    pub fn new(writer: Arc<dyn GraphWriter>) -> Self {
        Self { writer }
    }

    /// Write the `completeness_status` and `missing_fields` properties onto
    /// the DataCollection node.
    ///
    /// Implements: SR_INT_13
    pub async fn apply(
        &self,
        input: CompletenessTagInput,
    ) -> Result<CompletenessTagResult, PrismError> {
        let properties = serde_json::json!({
            "collection_id": input.collection_id.to_string(),
            "completeness_status": serde_json::to_value(input.status)
                .map_err(|e| PrismError::Serialization(e.to_string()))?,
            "missing_fields": input.missing_fields,
            "tagged_at": Utc::now(),
        });

        self.writer
            .create_node(input.tenant_id, "DataCollectionCompleteness", properties)
            .await?;

        Ok(CompletenessTagResult { tagged: true })
    }
}

// ============================================================================
// SR_INT_14 -- RecommendationAccuracyService
// ============================================================================

/// Repository for per-collection recommendation accuracy counters (D-56).
///
/// Implements: SR_INT_14
#[async_trait]
pub trait RecommendationAccuracyRepository: Send + Sync {
    async fn get(
        &self,
        tenant_id: TenantId,
        collection_id: uuid::Uuid,
    ) -> Result<Option<RecommendationAccuracy>, PrismError>;

    async fn upsert(
        &self,
        tenant_id: TenantId,
        accuracy: &RecommendationAccuracy,
    ) -> Result<(), PrismError>;
}

/// Maintain the `recommendation_track_record` counters on each DataCollection
/// so Decision Support can prefer historically accurate sources.
///
/// Implements: SR_INT_14
pub struct RecommendationAccuracyService {
    repo: Arc<dyn RecommendationAccuracyRepository>,
}

impl RecommendationAccuracyService {
    pub fn new(repo: Arc<dyn RecommendationAccuracyRepository>) -> Self {
        Self { repo }
    }

    /// Increment `used_in_count` (and `accurate_count` when accurate), then
    /// recompute `accuracy_rate`. Creates a fresh record on first update.
    ///
    /// Implements: SR_INT_14
    pub async fn update_accuracy(
        &self,
        input: AccuracyUpdateInput,
    ) -> Result<AccuracyUpdateResult, PrismError> {
        let current = self.repo.get(input.tenant_id, input.collection_id).await?;

        let mut record = current.unwrap_or(RecommendationAccuracy {
            collection_id: input.collection_id,
            used_in_count: 0,
            accurate_count: 0,
            accuracy_rate: 0.0,
        });

        record.used_in_count = record.used_in_count.saturating_add(1);
        if input.was_accurate {
            record.accurate_count = record.accurate_count.saturating_add(1);
        }
        record.accuracy_rate = if record.used_in_count == 0 {
            0.0
        } else {
            (record.accurate_count as f64) / (record.used_in_count as f64)
        };

        self.repo.upsert(input.tenant_id, &record).await?;

        Ok(AccuracyUpdateResult {
            updated: true,
            new_rate: record.accuracy_rate,
        })
    }
}

// ============================================================================
// SR_INT_15 -- VectorSemanticSearchService
// ============================================================================

/// Vector-index reader abstraction (SR_DM_18 consumer side).
///
/// Returns `(source_id, similarity)` pairs ordered by similarity descending.
///
/// Implements: SR_INT_15
#[async_trait]
pub trait VectorIndex: Send + Sync {
    async fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(String, f64)>, PrismError>;
}

/// Compartment access checker (SR_GOV_33 consumer side).
///
/// Returns `true` if the principal is allowed to see the resource, `false`
/// otherwise.
///
/// Implements: SR_INT_15
#[async_trait]
pub trait CompartmentAccessChecker: Send + Sync {
    async fn is_allowed(
        &self,
        principal_id: UserId,
        principal_roles: &[RoleId],
        resource_id: &str,
    ) -> Result<bool, PrismError>;
}

/// Vector semantic search with compartment post-filter per IL-7 / BP-101.
///
/// The post-filter flow is: over-fetch `top_k * 2` candidates from the vector
/// index, drop anything the compartment checker denies, then return the top
/// `top_k` survivors. This ordering prevents leaking forbidden documents
/// through similarity ranking.
///
/// Implements: SR_INT_15
pub struct VectorSemanticSearchService {
    index: Arc<dyn VectorIndex>,
    checker: Arc<dyn CompartmentAccessChecker>,
    audit: AuditLogger,
}

impl VectorSemanticSearchService {
    pub fn new(
        index: Arc<dyn VectorIndex>,
        checker: Arc<dyn CompartmentAccessChecker>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            index,
            checker,
            audit,
        }
    }

    /// Execute the over-fetch + filter + truncate flow and audit the search.
    ///
    /// Implements: SR_INT_15
    pub async fn search(
        &self,
        input: SemanticSearchInput,
    ) -> Result<SemanticSearchResult, PrismError> {
        let over_fetch = input.top_k.saturating_mul(2).max(input.top_k);
        let raw = self.index.search(&input.query_vector, over_fetch).await?;
        let raw_count = raw.len();

        let mut survivors: Vec<SearchResult> = Vec::new();
        let mut dropped: usize = 0;

        for (source_id, similarity) in raw {
            let allowed = self
                .checker
                .is_allowed(input.principal_id, &input.principal_roles, &source_id)
                .await?;
            if allowed {
                survivors.push(SearchResult {
                    source_id,
                    similarity,
                    compartment_allowed: true,
                });
            } else {
                dropped += 1;
            }
        }

        survivors.truncate(input.top_k);
        let filtered_count = raw_count.saturating_sub(dropped);

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.semantic_search_executed".into(),
                actor_id: *input.principal_id.as_uuid(),
                actor_type: ActorType::Human,
                target_id: None,
                target_type: Some("SemanticSearch".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "top_k": input.top_k,
                    "raw_count": raw_count,
                    "returned_count": survivors.len(),
                    "dropped_for_compartment_count": dropped,
                }),
            })
            .await?;

        Ok(SemanticSearchResult {
            results: survivors,
            filtered_count,
            dropped_for_compartment_count: dropped,
        })
    }
}

// ============================================================================
// SR_INT_16 -- CascadeImpactAnalysisService
// ============================================================================

/// Graph traversal abstraction shared by CIA (SR_INT_16) and the graph viz
/// subgraph query (SR_INT_20).
///
/// Implements: SR_INT_16, SR_INT_20
#[async_trait]
pub trait GraphTraversal: Send + Sync {
    /// Trace cascade impacts starting at `source` up to `depth` hops.
    async fn traverse_impacts(
        &self,
        source: &str,
        depth: u32,
    ) -> Result<Vec<CiaImpact>, PrismError>;

    /// Return nodes and directed `(from, to, edge_type)` edges within the
    /// given depth around `focal`.
    async fn traverse_nodes_and_edges(
        &self,
        focal: &str,
        depth: u32,
    ) -> Result<(Vec<String>, Vec<(String, String, String)>), PrismError>;
}

/// Cascade Impact Analysis (D-47): traces upstream, downstream, lateral and
/// second-order effects via `IMPACTS` edges and attaches a coverage
/// disclosure so downstream UIs can communicate incompleteness (per BP-103).
///
/// Implements: SR_INT_16
pub struct CascadeImpactAnalysisService {
    traversal: Arc<dyn GraphTraversal>,
    audit: AuditLogger,
}

impl CascadeImpactAnalysisService {
    pub fn new(traversal: Arc<dyn GraphTraversal>, audit: AuditLogger) -> Self {
        Self { traversal, audit }
    }

    /// Execute the CIA traversal and emit an audit event.
    ///
    /// Implements: SR_INT_16
    pub async fn analyze(&self, input: CiaRequest) -> Result<CiaResult, PrismError> {
        let impacts = self
            .traversal
            .traverse_impacts(&input.source_node, input.depth)
            .await?;

        // Overall confidence is the min of per-branch confidences (fail slow).
        let overall_confidence = impacts.iter().map(|i| i.confidence).fold(1.0_f64, f64::min);

        // Coverage disclosure: naive ratio of returned impacts to a
        // hypothetical ceiling (2 * depth). Keeps the service honest about
        // partial traversal without requiring exact graph counts.
        let ceiling = (input.depth as f64 * 2.0).max(1.0);
        let covered = ((impacts.len() as f64) / ceiling).min(1.0);
        let coverage_disclosure = format!(
            "Analysis covered {:.0}% of known dependencies",
            covered * 100.0
        );

        let tree = CiaTree {
            source_node: input.source_node.clone(),
            impacts: if input.include_confidence {
                impacts
            } else {
                // Strip confidence when caller has opted out.
                let mut stripped = impacts;
                for i in stripped.iter_mut() {
                    i.confidence = 1.0;
                }
                stripped
            },
            overall_confidence,
        };

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.cia_analyzed".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("CiaTree".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "source_node": input.source_node,
                    "depth": input.depth,
                    "impacts": tree.impacts.len(),
                    "overall_confidence": overall_confidence,
                }),
            })
            .await?;

        Ok(CiaResult {
            tree,
            coverage_disclosure,
        })
    }
}

// ============================================================================
// SR_INT_17 -- SemanticDisambiguationAgent
// ============================================================================

/// Persistence abstraction for the semantic dictionary maintained by SDA
/// (D-53).
///
/// Implements: SR_INT_17
#[async_trait]
pub trait SemanticDictionaryRepository: Send + Sync {
    /// Upsert a dictionary entry. Returns `true` if a new row was inserted,
    /// `false` if an existing row was modified.
    async fn upsert(
        &self,
        tenant_id: TenantId,
        entry: &SemanticDictionaryEntry,
    ) -> Result<bool, PrismError>;

    async fn list_for_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<SemanticDictionaryEntry>, PrismError>;
}

/// LLM abstraction used by SDA to discover synonyms and acronyms. Reuses the
/// T1 pattern established by SR_INT_03 with a discovery-specific shape.
///
/// Implements: SR_INT_17
#[async_trait]
pub trait SemanticDiscoveryLlm: Send + Sync {
    async fn discover(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<SemanticDictionaryEntry>, PrismError>;
}

/// Semantic Disambiguation Agent (D-53): periodically (or on-demand) discover
/// synonyms, acronyms, and contextual shorthand across tenant data and
/// maintain the `semantic_dictionary`.
///
/// Implements: SR_INT_17
pub struct SemanticDisambiguationAgent {
    repo: Arc<dyn SemanticDictionaryRepository>,
    llm: Arc<dyn SemanticDiscoveryLlm>,
    audit: AuditLogger,
}

impl SemanticDisambiguationAgent {
    pub fn new(
        repo: Arc<dyn SemanticDictionaryRepository>,
        llm: Arc<dyn SemanticDiscoveryLlm>,
        audit: AuditLogger,
    ) -> Self {
        Self { repo, llm, audit }
    }

    /// Execute one SDA pass for the tenant.
    ///
    /// Implements: SR_INT_17
    pub async fn run(&self, input: SdaRunRequest) -> Result<SdaResult, PrismError> {
        let discovered = self.llm.discover(input.tenant_id).await?;
        let mut added = 0u32;
        let mut modified = 0u32;

        for entry in discovered.iter() {
            let inserted = self.repo.upsert(input.tenant_id, entry).await?;
            if inserted {
                added += 1;
            } else {
                modified += 1;
            }
        }

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.sda_run".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("SemanticDictionary".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "added": added,
                    "modified": modified,
                }),
            })
            .await?;

        Ok(SdaResult { added, modified })
    }
}

// ============================================================================
// SR_INT_18 -- DecisionSupportDataGatheringService
// ============================================================================

/// Data freshness / quality check abstraction used by SR_INT_18.
///
/// Implements: SR_INT_18
#[async_trait]
pub trait DataFreshnessChecker: Send + Sync {
    /// Return `(freshness_seconds, gaps)` for a gather query.
    async fn check(
        &self,
        tenant_id: TenantId,
        query: &str,
    ) -> Result<(u64, Vec<String>), PrismError>;
}

/// Payload aggregator that actually fetches the combined data set.
///
/// Implements: SR_INT_18
#[async_trait]
pub trait DataAggregator: Send + Sync {
    async fn aggregate(
        &self,
        tenant_id: TenantId,
        query: &str,
        parameters: &serde_json::Value,
    ) -> Result<serde_json::Value, PrismError>;
}

/// CSA invocation gate (SR_GOV_24). Returns the decision string; "allow" means
/// the aggregation may proceed, anything else blocks the call.
///
/// Implements: SR_INT_18
#[async_trait]
pub trait CsaGate: Send + Sync {
    async fn check(
        &self,
        tenant_id: TenantId,
        data_collection_refs: &[String],
    ) -> Result<String, PrismError>;
}

/// Decision Support data gathering: composes freshness, quality and CSA
/// invocation into a single return so callers do not have to re-implement
/// the governance sequence (BP-100).
///
/// Implements: SR_INT_18
pub struct DecisionSupportDataGatheringService {
    freshness: Arc<dyn DataFreshnessChecker>,
    aggregator: Arc<dyn DataAggregator>,
    csa: Arc<dyn CsaGate>,
    audit: AuditLogger,
}

impl DecisionSupportDataGatheringService {
    pub fn new(
        freshness: Arc<dyn DataFreshnessChecker>,
        aggregator: Arc<dyn DataAggregator>,
        csa: Arc<dyn CsaGate>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            freshness,
            aggregator,
            csa,
            audit,
        }
    }

    /// Gather data for decision support.
    ///
    /// Implements: SR_INT_18
    pub async fn gather(
        &self,
        input: DataGatheringInput,
    ) -> Result<DataGatheringResult, PrismError> {
        let (freshness_seconds, gaps) = self.freshness.check(input.tenant_id, &input.query).await?;

        // CSA runs before aggregation so denied requests never touch data.
        let csa_decision = self
            .csa
            .check(input.tenant_id, std::slice::from_ref(&input.query))
            .await?;

        if csa_decision != "allow" {
            self.audit
                .log(AuditEventInput {
                    tenant_id: input.tenant_id,
                    event_type: "intelligence.data_gather_blocked".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: None,
                    target_type: Some("DataGathering".into()),
                    severity: Severity::Medium,
                    source_layer: SourceLayer::Llm,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "query": input.query,
                        "csa_decision": csa_decision,
                    }),
                })
                .await?;
            return Err(PrismError::Forbidden {
                reason: format!("CSA denied data gathering: {csa_decision}"),
            });
        }

        let data = self
            .aggregator
            .aggregate(input.tenant_id, &input.query, &input.parameters)
            .await?;

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.data_gathered".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("DataGathering".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "query": input.query,
                    "freshness_seconds": freshness_seconds,
                    "gaps": gaps,
                    "csa_decision": csa_decision,
                }),
            })
            .await?;

        Ok(DataGatheringResult {
            data,
            freshness_seconds,
            gaps,
            csa_decision,
        })
    }
}

// ============================================================================
// SR_INT_19 -- ResearchAgentService
// ============================================================================

/// External research source abstraction (weather, commodity, regulatory,
/// news). Returns a short blob per topic which is then persisted as a
/// `DataCollection` with `data_origin: ResearchAgent`.
///
/// Implements: SR_INT_19
#[async_trait]
pub trait ExternalResearchSource: Send + Sync {
    async fn fetch(&self, topic: &str) -> Result<String, PrismError>;
}

/// Research Agent (D-46): periodically gather external context and store each
/// result as a DataCollection in the intelligence graph.
///
/// Implements: SR_INT_19
pub struct ResearchAgentService {
    source: Arc<dyn ExternalResearchSource>,
    writer: Arc<dyn GraphWriter>,
    audit: AuditLogger,
}

impl ResearchAgentService {
    pub fn new(
        source: Arc<dyn ExternalResearchSource>,
        writer: Arc<dyn GraphWriter>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            source,
            writer,
            audit,
        }
    }

    /// Fetch all topics and persist each as a DataCollection node.
    ///
    /// Implements: SR_INT_19
    pub async fn research(&self, input: ResearchInput) -> Result<ResearchResult, PrismError> {
        let mut collection_ids = Vec::with_capacity(input.topics.len());

        for topic in input.topics.iter() {
            let blob = self.source.fetch(topic).await?;
            let properties = serde_json::json!({
                "topic": topic,
                "content": blob,
                "data_origin": "research_agent",
                "fetched_at": Utc::now().to_rfc3339(),
            });
            let node_id = self
                .writer
                .create_node(input.tenant_id, "DataCollection", properties)
                .await?;
            collection_ids.push(node_id);
        }

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.research_agent_run".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("DataCollection".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "topic_count": input.topics.len(),
                    "collections_created": collection_ids.len(),
                }),
            })
            .await?;

        Ok(ResearchResult { collection_ids })
    }
}

// ============================================================================
// SR_INT_20 -- GraphVizService
// ============================================================================

/// Graph visualization data source: returns a bounded subgraph around a focal
/// node, honoring the depth limit so the interface panel can render without
/// browser crashes.
///
/// Implements: SR_INT_20
pub struct GraphVizService {
    traversal: Arc<dyn GraphTraversal>,
    audit: AuditLogger,
}

/// Maximum depth enforced by SR_INT_20 regardless of caller request.
///
/// Implements: SR_INT_20
pub const GRAPH_VIZ_MAX_DEPTH: u32 = 5;

impl GraphVizService {
    pub fn new(traversal: Arc<dyn GraphTraversal>, audit: AuditLogger) -> Self {
        Self { traversal, audit }
    }

    /// Return the bounded subgraph around `focal_node`.
    ///
    /// Implements: SR_INT_20
    pub async fn query(&self, input: GraphVizRequest) -> Result<GraphVizResult, PrismError> {
        let depth = input.depth.min(GRAPH_VIZ_MAX_DEPTH);
        let (nodes, edges) = self
            .traversal
            .traverse_nodes_and_edges(&input.focal_node, depth)
            .await?;

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.graph_viz_queried".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("GraphViz".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "focal_node": input.focal_node,
                    "requested_depth": input.depth,
                    "effective_depth": depth,
                    "node_count": nodes.len(),
                    "edge_count": edges.len(),
                }),
            })
            .await?;

        Ok(GraphVizResult { nodes, edges })
    }
}

// ============================================================================
// SR_INT_21 -- AgentFeedbackLoopService
// ============================================================================

/// Per-agent metrics source used by the feedback loop (D-51).
///
/// Implements: SR_INT_21
#[async_trait]
pub trait AgentMetricsStore: Send + Sync {
    async fn get_metrics(&self, agent: AgentKind) -> Result<Vec<f64>, PrismError>;
}

/// All agent kinds the feedback loop evaluates in one cycle.
///
/// Implements: SR_INT_21
pub const FEEDBACK_LOOP_AGENTS: &[AgentKind] = &[
    AgentKind::Tagging,
    AgentKind::Routing,
    AgentKind::Research,
    AgentKind::Quality,
    AgentKind::Discovery,
];

/// Agent Performance Feedback Loop: score each agent, identify improvements,
/// and emit one audit event per cycle.
///
/// Implements: SR_INT_21
pub struct AgentFeedbackLoopService {
    metrics: Arc<dyn AgentMetricsStore>,
    audit: AuditLogger,
}

impl AgentFeedbackLoopService {
    pub fn new(metrics: Arc<dyn AgentMetricsStore>, audit: AuditLogger) -> Self {
        Self { metrics, audit }
    }

    /// Evaluate all five agent kinds and produce an improvement list.
    ///
    /// Implements: SR_INT_21
    pub async fn evaluate_cycle(
        &self,
        input: AgentFeedbackCycleRequest,
    ) -> Result<AgentFeedbackResult, PrismError> {
        let mut improvements: Vec<String> = Vec::new();
        for agent in FEEDBACK_LOOP_AGENTS.iter().copied() {
            let samples = self.metrics.get_metrics(agent).await?;
            if samples.is_empty() {
                continue;
            }
            let avg = samples.iter().sum::<f64>() / samples.len() as f64;
            // Any agent scoring below 0.8 contributes an improvement note.
            if avg < 0.8 {
                improvements.push(format!(
                    "{:?} avg {:.2} -- schedule prompt/model review",
                    agent, avg
                ));
            }
        }

        let agents_evaluated = FEEDBACK_LOOP_AGENTS.len() as u32;

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.agent_feedback_cycle".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("AgentFeedback".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "agents_evaluated": agents_evaluated,
                    "improvements": improvements,
                }),
            })
            .await?;

        Ok(AgentFeedbackResult {
            agents_evaluated,
            improvements,
        })
    }
}

// ============================================================================
// SR_INT_22 -- CrossTenantLearningService
// ============================================================================

/// Opt-in verification (SR_GOV_59 consumer side). Returns `true` only when
/// every tenant in the set has a live opt-in on record.
///
/// Implements: SR_INT_22
#[async_trait]
pub trait OptInVerifier: Send + Sync {
    async fn is_verified(&self, tenant_ids: &[TenantId]) -> Result<bool, PrismError>;
}

/// Aggregation worker abstraction. Reads only materialized aggregates, never
/// raw nodes, per the Q&A #3 architectural boundary.
///
/// Implements: SR_INT_22
#[async_trait]
pub trait PatternAggregator: Send + Sync {
    async fn aggregate(&self, tenant_ids: &[TenantId]) -> Result<Vec<String>, PrismError>;
}

/// Maximum age for an opt-in verification before the current cycle must
/// re-verify (per BP-126).
///
/// Implements: SR_INT_22
pub const OPT_IN_MAX_AGE_HOURS: i64 = 24;

/// Cross-tenant aggregate learning service (BP-31). Rejects aggregation when
/// opt-in is stale or unverified.
///
/// Implements: SR_INT_22
pub struct CrossTenantLearningService {
    verifier: Arc<dyn OptInVerifier>,
    aggregator: Arc<dyn PatternAggregator>,
    audit: AuditLogger,
}

impl CrossTenantLearningService {
    pub fn new(
        verifier: Arc<dyn OptInVerifier>,
        aggregator: Arc<dyn PatternAggregator>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            verifier,
            aggregator,
            audit,
        }
    }

    /// Run one aggregation cycle (at most once per 24h per BP-31).
    ///
    /// Implements: SR_INT_22
    pub async fn aggregate(
        &self,
        input: CrossTenantAggregationInput,
    ) -> Result<CrossTenantAggregationResult, PrismError> {
        let age = Utc::now() - input.opt_in_verified_at;
        if age > Duration::hours(OPT_IN_MAX_AGE_HOURS) {
            return Err(PrismError::Forbidden {
                reason: "opt-in verification is stale (>24h)".into(),
            });
        }

        let verified = self.verifier.is_verified(&input.tenant_ids).await?;
        if !verified {
            return Err(PrismError::Forbidden {
                reason: "one or more tenants have not opted in".into(),
            });
        }

        let patterns = self.aggregator.aggregate(&input.tenant_ids).await?;

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_ids.first().copied().unwrap_or_default(),
                event_type: "intelligence.cross_tenant_aggregated".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("CrossTenantAggregation".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "tenant_count": input.tenant_ids.len(),
                    "pattern_count": patterns.len(),
                }),
            })
            .await?;

        Ok(CrossTenantAggregationResult {
            patterns,
            opt_in_state: "verified".into(),
        })
    }
}

// ============================================================================
// SR_INT_23 -- IntelligenceQueryRewriteService
// ============================================================================

/// Query rewriter abstraction. Production implementations delegate to
/// `SR_DM_27`; tests use a fake.
///
/// Implements: SR_INT_23
#[async_trait]
pub trait QueryRewriter: Send + Sync {
    async fn rewrite(
        &self,
        raw_cypher: &str,
        tenant_id: TenantId,
        principal_id: UserId,
    ) -> Result<String, PrismError>;
}

/// Raw Cypher substrings that are forbidden at the interface boundary (IL-1):
/// no database drops, no admin calls, no apoc sweeps.
const FORBIDDEN_CYPHER_PATTERNS: &[&str] = &[
    "CALL db.drop",
    "CALL dbms.",
    "CALL apoc.periodic",
    "DETACH DELETE *",
];

/// Query rewrite entry point: reject forbidden constructs before rewriting,
/// then ensure the rewrite still references the tenant_id.
///
/// Implements: SR_INT_23
pub struct IntelligenceQueryRewriteService {
    rewriter: Arc<dyn QueryRewriter>,
    audit: AuditLogger,
}

impl IntelligenceQueryRewriteService {
    pub fn new(rewriter: Arc<dyn QueryRewriter>, audit: AuditLogger) -> Self {
        Self { rewriter, audit }
    }

    /// Validate, rewrite, and record an audit trail for the query.
    ///
    /// Implements: SR_INT_23
    pub async fn rewrite(&self, input: QueryInput) -> Result<QueryIntelligenceRewrite, PrismError> {
        for pat in FORBIDDEN_CYPHER_PATTERNS.iter() {
            if input.raw_cypher.contains(pat) {
                return Err(PrismError::Forbidden {
                    reason: format!("forbidden cypher construct: {pat}"),
                });
            }
        }

        let rewritten = self
            .rewriter
            .rewrite(&input.raw_cypher, input.tenant_id, input.principal_id)
            .await?;

        let tenant_token = format!("tenant_id:{}", input.tenant_id);
        if !rewritten.contains("tenant_id") {
            return Err(PrismError::Validation {
                reason: "rewriter did not inject tenant filter".into(),
            });
        }
        let applied_filters = vec![tenant_token, "role_based_access".into()];

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.query_rewritten".into(),
                actor_id: *input.principal_id.as_uuid(),
                actor_type: ActorType::Human,
                target_id: None,
                target_type: Some("QueryRewrite".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "raw_length": input.raw_cypher.len(),
                    "rewritten_length": rewritten.len(),
                }),
            })
            .await?;

        Ok(QueryIntelligenceRewrite {
            rewritten_query: rewritten,
            applied_filters,
        })
    }
}

// ============================================================================
// SR_INT_24 -- ProactiveTriggerService
// ============================================================================

/// Per-trigger-type evaluator (D-60). Returns how many triggers fired.
///
/// Implements: SR_INT_24
#[async_trait]
pub trait TriggerEvaluator: Send + Sync {
    async fn check(
        &self,
        tenant_id: TenantId,
        trigger_type: ProactiveTriggerType,
    ) -> Result<u32, PrismError>;
}

/// Recommendation request sender (BP-103). Fires one recommendation per
/// trigger hit, ensuring coverage-disclosure remains attached downstream.
///
/// Implements: SR_INT_24
#[async_trait]
pub trait RecommendationRequestSender: Send + Sync {
    async fn send(
        &self,
        tenant_id: TenantId,
        trigger_type: ProactiveTriggerType,
    ) -> Result<(), PrismError>;
}

/// Proactive trigger service: evaluates one trigger type per call and fires a
/// recommendation request for each detected hit.
///
/// Implements: SR_INT_24
pub struct ProactiveTriggerService {
    evaluator: Arc<dyn TriggerEvaluator>,
    sender: Arc<dyn RecommendationRequestSender>,
    audit: AuditLogger,
}

impl ProactiveTriggerService {
    pub fn new(
        evaluator: Arc<dyn TriggerEvaluator>,
        sender: Arc<dyn RecommendationRequestSender>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            evaluator,
            sender,
            audit,
        }
    }

    /// Evaluate the trigger and fire recommendations as needed.
    ///
    /// Implements: SR_INT_24
    pub async fn evaluate(
        &self,
        input: ProactiveTriggerRequest,
    ) -> Result<ProactiveTriggerFireResult, PrismError> {
        let fired = self
            .evaluator
            .check(input.tenant_id, input.trigger_type)
            .await?;

        for _ in 0..fired {
            self.sender
                .send(input.tenant_id, input.trigger_type)
                .await?;
        }

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.proactive_trigger_evaluated".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("ProactiveTrigger".into()),
                severity: if fired > 0 {
                    Severity::Medium
                } else {
                    Severity::Low
                },
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "trigger_type": serde_json::to_value(input.trigger_type)
                        .map_err(|e| PrismError::Serialization(e.to_string()))?,
                    "triggers_fired": fired,
                }),
            })
            .await?;

        Ok(ProactiveTriggerFireResult {
            triggers_fired: fired,
        })
    }
}

// ============================================================================
// SR_INT_25 -- IntelligenceMaintenanceService
// ============================================================================

/// Intelligence-layer operational entry point for graph maintenance (BP-130
/// operation vs substrate split). Delegates the actual store-level
/// operations to `SR_DM_24` via `GraphMaintenanceWorker`.
///
/// Implements: SR_INT_25
pub struct IntelligenceMaintenanceService {
    worker: Arc<dyn prism_graph::data_model::GraphMaintenanceWorker>,
    audit: AuditLogger,
}

impl IntelligenceMaintenanceService {
    pub fn new(
        worker: Arc<dyn prism_graph::data_model::GraphMaintenanceWorker>,
        audit: AuditLogger,
    ) -> Self {
        Self { worker, audit }
    }

    /// Run one maintenance cycle of the specified type.
    ///
    /// Implements: SR_INT_25
    pub async fn run_cycle(
        &self,
        input: IntelligenceMaintenanceRequest,
    ) -> Result<IntelligenceMaintenanceResult, PrismError> {
        let affected = self
            .worker
            .execute_cycle(input.tenant_id, input.cycle_type)
            .await?;

        // Any cycle that affects more than 100k entities is surfaced as a
        // potential anomaly for the scheduler to investigate.
        let mut anomalies = Vec::new();
        if affected > 100_000 {
            anomalies.push(format!(
                "cycle {:?} affected {} entities (>100k threshold)",
                input.cycle_type, affected
            ));
        }

        let tenant_for_audit = input.tenant_id.unwrap_or_default();
        self.audit
            .log(AuditEventInput {
                tenant_id: tenant_for_audit,
                event_type: "intelligence.maintenance_cycle".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("IntelligenceMaintenance".into()),
                severity: if anomalies.is_empty() {
                    Severity::Low
                } else {
                    Severity::Medium
                },
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "cycle_type": serde_json::to_value(input.cycle_type)
                        .map_err(|e| PrismError::Serialization(e.to_string()))?,
                    "affected": affected,
                    "anomalies": anomalies,
                }),
            })
            .await?;

        Ok(IntelligenceMaintenanceResult {
            cycles_run: 1,
            anomalies,
        })
    }
}

// ============================================================================
// SR_INT_26 -- QueryCostEstimatorService
// ============================================================================

/// Static cost estimator (IL-6). Production implementations parse and weight
/// the query; tests use fixed values.
///
/// Implements: SR_INT_26
#[async_trait]
pub trait CostEstimator: Send + Sync {
    async fn estimate(&self, query: &str) -> Result<u64, PrismError>;
}

/// Per-tenant quota enforcer (SR_SCALE_25 consumer side). Returns the
/// remaining quota in whatever unit the caller is tracking.
///
/// Implements: SR_INT_26
#[async_trait]
pub trait QuotaEnforcer: Send + Sync {
    async fn check_quota(&self, tenant_id: TenantId) -> Result<u64, PrismError>;
}

/// Hard cap on estimated query cost (ms) before the estimator rejects.
///
/// Implements: SR_INT_26
pub const QUERY_COST_TIMEOUT_MS: u64 = 30_000;

/// Query Cost Estimator: rejects expensive queries and quota-exhausted
/// tenants before execution (IL-6 + BP-128).
///
/// Implements: SR_INT_26
pub struct QueryCostEstimatorService {
    estimator: Arc<dyn CostEstimator>,
    quota: Arc<dyn QuotaEnforcer>,
    audit: AuditLogger,
}

impl QueryCostEstimatorService {
    pub fn new(
        estimator: Arc<dyn CostEstimator>,
        quota: Arc<dyn QuotaEnforcer>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            estimator,
            quota,
            audit,
        }
    }

    /// Decide whether the query may be run.
    ///
    /// Implements: SR_INT_26
    pub async fn estimate(
        &self,
        input: CostEstimateInput,
    ) -> Result<CostEstimateResult, PrismError> {
        let cost = self.estimator.estimate(&input.query).await?;
        let quota_remaining = self.quota.check_quota(input.tenant_id).await?;

        let allowed = cost <= QUERY_COST_TIMEOUT_MS && quota_remaining > 0;

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.query_cost_estimated".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("QueryCost".into()),
                severity: if allowed {
                    Severity::Low
                } else {
                    Severity::Medium
                },
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "estimated_cost_ms": cost,
                    "quota_remaining": quota_remaining,
                    "allowed": allowed,
                }),
            })
            .await?;

        Ok(CostEstimateResult {
            allowed,
            estimated_cost_ms: cost,
            quota_remaining,
        })
    }
}

// ============================================================================
// SR_INT_27 -- BulkImportWorkerService
// ============================================================================

/// Dedicated bulk-import queue (IL-8). Separate from the interactive queue
/// so large imports do not starve reads.
///
/// Implements: SR_INT_27
#[async_trait]
pub trait BulkImportQueue: Send + Sync {
    async fn enqueue(&self, tenant_id: TenantId, import_id: uuid::Uuid) -> Result<u64, PrismError>;
}

/// Bulk import worker front-door (IL-8): the heavy lifting is in
/// `prism-adapters`; this service just dispatches onto the dedicated queue
/// and records the audit trail.
///
/// Implements: SR_INT_27
pub struct BulkImportWorkerService {
    queue: Arc<dyn BulkImportQueue>,
    audit: AuditLogger,
}

impl BulkImportWorkerService {
    pub fn new(queue: Arc<dyn BulkImportQueue>, audit: AuditLogger) -> Self {
        Self { queue, audit }
    }

    /// Enqueue the import and return the count the queue reports.
    ///
    /// Implements: SR_INT_27
    pub async fn process(
        &self,
        input: BulkImportProcessingInput,
    ) -> Result<BulkImportProcessingResult, PrismError> {
        let processed = self.queue.enqueue(input.tenant_id, input.import_id).await?;

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.bulk_import_enqueued".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: Some(input.import_id),
                target_type: Some("BulkImport".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "import_id": input.import_id.to_string(),
                    "processed": processed,
                }),
            })
            .await?;

        Ok(BulkImportProcessingResult { processed })
    }
}

// ============================================================================
// SR_INT_28 -- ReadThroughCacheService
// ============================================================================

/// A single cache entry: payload plus the monotonic time (seconds since epoch)
/// at which it was written.
///
/// Implements: SR_INT_28
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub payload: serde_json::Value,
    pub written_at_epoch: i64,
}

/// Cache store abstraction. `get` returns `Some(entry)` when the key is
/// present (regardless of freshness); the service decides whether to use it.
///
/// Implements: SR_INT_28
#[async_trait]
pub trait CacheStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<CacheEntry>, PrismError>;
    async fn set(&self, key: &str, entry: CacheEntry) -> Result<(), PrismError>;
    async fn invalidate(&self, key: &str) -> Result<(), PrismError>;
}

/// Data source invoked on cache miss.
///
/// Implements: SR_INT_28
#[async_trait]
pub trait CacheDataSource: Send + Sync {
    async fn fetch(&self, key: &str) -> Result<serde_json::Value, PrismError>;
}

/// Extended TTL multiplier under graceful-degradation (BP-120).
pub const DEGRADATION_TTL_MULTIPLIER: u64 = 10;

/// Read-through cache with TTL (IL-9) + Neo4j-reads degradation participation
/// (BP-120). Under degradation, the effective TTL is multiplied by
/// `DEGRADATION_TTL_MULTIPLIER` and returned results are marked stale.
///
/// Implements: SR_INT_28
pub struct ReadThroughCacheService {
    cache: Arc<dyn CacheStore>,
    source: Arc<dyn CacheDataSource>,
    audit: AuditLogger,
}

impl ReadThroughCacheService {
    pub fn new(
        cache: Arc<dyn CacheStore>,
        source: Arc<dyn CacheDataSource>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            cache,
            source,
            audit,
        }
    }

    /// Resolve the cache-or-source read.
    ///
    /// Implements: SR_INT_28
    pub async fn query_with_cache(&self, input: CacheRequest) -> Result<CacheResponse, PrismError> {
        let now = Utc::now().timestamp();
        let effective_ttl = if input.degradation_mode {
            input.ttl_seconds.saturating_mul(DEGRADATION_TTL_MULTIPLIER)
        } else {
            input.ttl_seconds
        };

        let (source, freshness_seconds) = match self.cache.get(&input.key).await? {
            Some(entry) => {
                let age = (now - entry.written_at_epoch).max(0) as u64;
                if age <= effective_ttl {
                    ("cache", age)
                } else {
                    // Stale: refresh from source.
                    let fresh = self.source.fetch(&input.key).await?;
                    self.cache
                        .set(
                            &input.key,
                            CacheEntry {
                                payload: fresh,
                                written_at_epoch: now,
                            },
                        )
                        .await?;
                    ("source", 0)
                }
            }
            None => {
                let fresh = self.source.fetch(&input.key).await?;
                self.cache
                    .set(
                        &input.key,
                        CacheEntry {
                            payload: fresh,
                            written_at_epoch: now,
                        },
                    )
                    .await?;
                ("source", 0)
            }
        };

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.cache_query".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("ReadThroughCache".into()),
                severity: Severity::Low,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "key": input.key,
                    "source": source,
                    "freshness_seconds": freshness_seconds,
                    "degradation_active": input.degradation_mode,
                }),
            })
            .await?;

        Ok(CacheResponse {
            source: source.into(),
            freshness_seconds,
            degradation_active: input.degradation_mode,
        })
    }
}

// ============================================================================
// SR_INT_29 -- DisasterRecoveryDrillService
// ============================================================================

/// DR drill executor: plays out the scenario against the test environment
/// and returns `(measured_rto, measured_rpo)` in seconds.
///
/// Implements: SR_INT_29
#[async_trait]
pub trait DrExecutor: Send + Sync {
    async fn execute(&self, scenario: DrScenario) -> Result<(u64, u64), PrismError>;
}

/// Escalation channel used when a drill fails (SR_GOV_67).
///
/// Implements: SR_INT_29
#[async_trait]
pub trait DrEscalator: Send + Sync {
    async fn escalate(&self, scenario: DrScenario, reason: &str) -> Result<String, PrismError>;
}

/// DR drill service: runs the scenario, compares measured against SR_SCALE_40
/// targets, and escalates on failure (BP-121).
///
/// Implements: SR_INT_29
pub struct DisasterRecoveryDrillService {
    executor: Arc<dyn DrExecutor>,
    escalator: Arc<dyn DrEscalator>,
    audit: AuditLogger,
}

impl DisasterRecoveryDrillService {
    pub fn new(
        executor: Arc<dyn DrExecutor>,
        escalator: Arc<dyn DrEscalator>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            executor,
            escalator,
            audit,
        }
    }

    /// Run one DR drill.
    ///
    /// Implements: SR_INT_29
    pub async fn run_drill(&self, input: DrDrillRequest) -> Result<DrDrillResult, PrismError> {
        let (measured_rto, measured_rpo) = self.executor.execute(input.scenario).await?;
        let passed =
            measured_rto <= input.target_rto_seconds && measured_rpo <= input.target_rpo_seconds;

        let escalation_id = if passed {
            None
        } else {
            let reason = format!(
                "drill {:?} failed: rto {}s > {}s or rpo {}s > {}s",
                input.scenario,
                measured_rto,
                input.target_rto_seconds,
                measured_rpo,
                input.target_rpo_seconds
            );
            Some(self.escalator.escalate(input.scenario, &reason).await?)
        };

        self.audit
            .log(AuditEventInput {
                tenant_id: TenantId::default(),
                event_type: "intelligence.dr_drill".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("DrDrill".into()),
                severity: if passed {
                    Severity::Low
                } else {
                    Severity::High
                },
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "scenario": serde_json::to_value(input.scenario)
                        .map_err(|e| PrismError::Serialization(e.to_string()))?,
                    "target_rto_seconds": input.target_rto_seconds,
                    "target_rpo_seconds": input.target_rpo_seconds,
                    "measured_rto_seconds": measured_rto,
                    "measured_rpo_seconds": measured_rpo,
                    "passed": passed,
                }),
            })
            .await?;

        Ok(DrDrillResult {
            passed,
            measured_rto_seconds: measured_rto,
            measured_rpo_seconds: measured_rpo,
            escalation_id,
        })
    }
}

// ============================================================================
// SR_INT_30 -- TenantOffboardingService
// ============================================================================

/// Crypto-shred abstraction (SR_GOV_52 consumer side). Returns one certificate
/// per shredded key.
///
/// Implements: SR_INT_30
#[async_trait]
pub trait CryptoShredService: Send + Sync {
    async fn shred_all(&self, tenant_id: TenantId) -> Result<Vec<String>, PrismError>;
}

/// Bulk data remover: purges the tenant's rows/nodes from every store (PG,
/// Neo4j, vector embeddings, event bus streams, object storage, model state).
///
/// Implements: SR_INT_30
#[async_trait]
pub trait DataRemover: Send + Sync {
    async fn remove(&self, tenant_id: TenantId) -> Result<u64, PrismError>;
}

/// Offboarding verifier: runs a scan and returns `true` only when every
/// store confirms deletion.
///
/// Implements: SR_INT_30
#[async_trait]
pub trait OffboardingVerifier: Send + Sync {
    async fn verify(&self, tenant_id: TenantId) -> Result<bool, PrismError>;
}

/// Certificate issuer for deletion proofs.
///
/// Implements: SR_INT_30
#[async_trait]
pub trait CertificateIssuer: Send + Sync {
    async fn issue(&self, tenant_id: TenantId) -> Result<String, PrismError>;
}

/// Tenant Offboarding Service (BP-102): crypto-shred first, then bulk delete,
/// then verify, then issue a deletion certificate.
///
/// Implements: SR_INT_30
pub struct TenantOffboardingService {
    shred: Arc<dyn CryptoShredService>,
    remover: Arc<dyn DataRemover>,
    verifier: Arc<dyn OffboardingVerifier>,
    issuer: Arc<dyn CertificateIssuer>,
    audit: AuditLogger,
}

impl TenantOffboardingService {
    pub fn new(
        shred: Arc<dyn CryptoShredService>,
        remover: Arc<dyn DataRemover>,
        verifier: Arc<dyn OffboardingVerifier>,
        issuer: Arc<dyn CertificateIssuer>,
        audit: AuditLogger,
    ) -> Self {
        Self {
            shred,
            remover,
            verifier,
            issuer,
            audit,
        }
    }

    /// Offboard a tenant end-to-end.
    ///
    /// Implements: SR_INT_30
    pub async fn offboard(
        &self,
        input: OffboardingRequest,
    ) -> Result<OffboardingResult, PrismError> {
        if !input.confirm_all_subjects {
            return Err(PrismError::Validation {
                reason: "confirm_all_subjects must be true to offboard".into(),
            });
        }

        // Step 1: crypto-shred per-subject keys (SR_GOV_52).
        let shred_certificates = self.shred.shred_all(input.tenant_id).await?;

        // Step 2: bulk delete across all stores.
        self.remover.remove(input.tenant_id).await?;

        // Step 3: automated verification scan.
        let verified = self.verifier.verify(input.tenant_id).await?;
        if !verified {
            self.audit
                .log(AuditEventInput {
                    tenant_id: input.tenant_id,
                    event_type: "intelligence.tenant_offboard_failed".into(),
                    actor_id: uuid::Uuid::nil(),
                    actor_type: ActorType::System,
                    target_id: None,
                    target_type: Some("TenantOffboarding".into()),
                    severity: Severity::High,
                    source_layer: SourceLayer::Llm,
                    governance_authority: None,
                    payload: serde_json::json!({
                        "shred_count": shred_certificates.len(),
                        "verified": false,
                    }),
                })
                .await?;
            return Ok(OffboardingResult {
                certificate_url: String::new(),
                shred_certificates,
                verified: false,
            });
        }

        // Step 4: issue the deletion certificate.
        let certificate_url = self.issuer.issue(input.tenant_id).await?;

        self.audit
            .log(AuditEventInput {
                tenant_id: input.tenant_id,
                event_type: "intelligence.tenant_offboarded".into(),
                actor_id: uuid::Uuid::nil(),
                actor_type: ActorType::System,
                target_id: None,
                target_type: Some("TenantOffboarding".into()),
                severity: Severity::Medium,
                source_layer: SourceLayer::Llm,
                governance_authority: None,
                payload: serde_json::json!({
                    "shred_count": shred_certificates.len(),
                    "certificate_url": certificate_url,
                    "verified": true,
                }),
            })
            .await?;

        Ok(OffboardingResult {
            certificate_url,
            shred_certificates,
            verified: true,
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

    // ------------------------------------------------------------------
    // SR_INT_09 -- CoverageCalculator
    // ------------------------------------------------------------------

    struct FixedCoverageSource {
        counts: CoverageCounts,
    }

    #[async_trait]
    impl CoverageSource for FixedCoverageSource {
        async fn get_counts(&self, _tenant_id: TenantId) -> Result<CoverageCounts, PrismError> {
            Ok(self.counts)
        }
    }

    #[tokio::test]
    async fn int09_full_coverage_has_no_limitations() {
        let source = Arc::new(FixedCoverageSource {
            counts: CoverageCounts {
                systems_known: 10,
                systems_total: 10,
                processes_known: 5,
                processes_total: 5,
                data_collections_known: 20,
                data_collections_total: 20,
                departments_known: 3,
                departments_total: 3,
                relationships_known: 40,
                relationships_total: 40,
            },
        });
        let (audit_repo, audit) = make_audit();
        let svc = CoverageCalculator::new(source, audit);

        let result = svc
            .compute(CoverageRequest {
                tenant_id: tenant(),
            })
            .await
            .unwrap();

        assert_eq!(result.dimensions.len(), 5);
        for (_dim, pct) in &result.dimensions {
            assert!((pct - 1.0).abs() < f64::EPSILON);
        }
        assert!(result.limitations.is_empty());
        assert!(audit_repo
            .event_types()
            .contains(&"intelligence.coverage_computed".to_string()));
    }

    #[tokio::test]
    async fn int09_partial_coverage_records_limitations() {
        let source = Arc::new(FixedCoverageSource {
            counts: CoverageCounts {
                systems_known: 10,
                systems_total: 10,
                processes_known: 2,
                processes_total: 10,
                data_collections_known: 4,
                data_collections_total: 10,
                departments_known: 3,
                departments_total: 3,
                relationships_known: 1,
                relationships_total: 10,
            },
        });
        let (_audit_repo, audit) = make_audit();
        let svc = CoverageCalculator::new(source, audit);

        let result = svc
            .compute(CoverageRequest {
                tenant_id: tenant(),
            })
            .await
            .unwrap();

        // Process (20%), Data (40%), Relationship (10%) are below 50%.
        assert_eq!(result.limitations.len(), 3);
    }

    // ------------------------------------------------------------------
    // SR_INT_10 -- ProcessEmergenceDetector
    // ------------------------------------------------------------------

    struct StubMatcher {
        candidates: Vec<ProcessCandidate>,
    }

    #[async_trait]
    impl ProcessPatternMatcher for StubMatcher {
        async fn find_process_candidates(
            &self,
            _tenant_id: TenantId,
        ) -> Result<Vec<ProcessCandidate>, PrismError> {
            Ok(self.candidates.clone())
        }
    }

    #[tokio::test]
    async fn int10_creates_candidate_nodes() {
        let t = tenant();
        let matcher = Arc::new(StubMatcher {
            candidates: vec![
                ProcessCandidate {
                    id: uuid::Uuid::new_v4(),
                    tenant_id: t,
                    suggested_name: "Onboarding".into(),
                    component_ids: vec![uuid::Uuid::new_v4(), uuid::Uuid::new_v4()],
                    confidence: 0.8,
                    created_at: Utc::now(),
                    status: "draft".into(),
                },
                ProcessCandidate {
                    id: uuid::Uuid::new_v4(),
                    tenant_id: t,
                    suggested_name: "Offboarding".into(),
                    component_ids: vec![uuid::Uuid::new_v4()],
                    confidence: 0.75,
                    created_at: Utc::now(),
                    status: "draft".into(),
                },
            ],
        });
        let writer = Arc::new(MockGraphWriter::new());
        let (audit_repo, audit) = make_audit();
        let svc = ProcessEmergenceDetector::new(matcher, writer.clone(), audit);

        let result = svc
            .discover(ProcessDiscoveryInput { tenant_id: t })
            .await
            .unwrap();

        assert_eq!(result.candidates.len(), 2);
        assert!(result
            .candidates
            .iter()
            .all(|c| c.status == "pending_confirmation"));
        assert_eq!(writer.count_of("ProcessCandidate"), 2);
        assert_eq!(
            audit_repo
                .event_types()
                .iter()
                .filter(|t| *t == "intelligence.process_discovered")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn int10_handles_empty_results() {
        let matcher = Arc::new(StubMatcher { candidates: vec![] });
        let writer = Arc::new(MockGraphWriter::new());
        let (_audit_repo, audit) = make_audit();
        let svc = ProcessEmergenceDetector::new(matcher, writer.clone(), audit);

        let result = svc
            .discover(ProcessDiscoveryInput {
                tenant_id: tenant(),
            })
            .await
            .unwrap();

        assert!(result.candidates.is_empty());
        assert_eq!(writer.total(), 0);
    }

    // ------------------------------------------------------------------
    // SR_INT_11 -- DataGroupMembershipService
    // ------------------------------------------------------------------

    struct MockGroupRepo {
        members: Mutex<Vec<DataGroupMembership>>,
    }

    impl MockGroupRepo {
        fn new() -> Self {
            Self {
                members: Mutex::new(Vec::new()),
            }
        }

        fn count(&self) -> usize {
            self.members.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl DataGroupRepository for MockGroupRepo {
        async fn add_member(&self, membership: &DataGroupMembership) -> Result<bool, PrismError> {
            let mut members = self.members.lock().unwrap();
            if members.iter().any(|m| {
                m.group_id == membership.group_id && m.collection_id == membership.collection_id
            }) {
                return Ok(false);
            }
            members.push(membership.clone());
            Ok(true)
        }

        async fn get_members(
            &self,
            tenant_id: TenantId,
            group_id: uuid::Uuid,
        ) -> Result<Vec<DataGroupMembership>, PrismError> {
            Ok(self
                .members
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.tenant_id == tenant_id && m.group_id == group_id)
                .cloned()
                .collect())
        }

        async fn remove_member(
            &self,
            tenant_id: TenantId,
            group_id: uuid::Uuid,
            collection_id: uuid::Uuid,
        ) -> Result<(), PrismError> {
            self.members.lock().unwrap().retain(|m| {
                !(m.tenant_id == tenant_id
                    && m.group_id == group_id
                    && m.collection_id == collection_id)
            });
            Ok(())
        }
    }

    #[tokio::test]
    async fn int11_adds_members_and_creates_edges() {
        let repo = Arc::new(MockGroupRepo::new());
        let writer = Arc::new(MockGraphWriter::new());
        let svc = DataGroupMembershipService::new(repo.clone(), writer.clone());

        let group_id = uuid::Uuid::new_v4();
        let collections = vec![
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        ];

        let result = svc
            .add_to_group(DataGroupingInput {
                tenant_id: tenant(),
                group_id,
                collection_ids: collections.clone(),
            })
            .await
            .unwrap();

        assert_eq!(result.members_added, 3);
        assert_eq!(repo.count(), 3);
        assert_eq!(writer.count_of("MemberOfEdge"), 3);
    }

    #[tokio::test]
    async fn int11_skips_duplicate_memberships() {
        let repo = Arc::new(MockGroupRepo::new());
        let writer = Arc::new(MockGraphWriter::new());
        let svc = DataGroupMembershipService::new(repo.clone(), writer.clone());

        let t = tenant();
        let group_id = uuid::Uuid::new_v4();
        let collection_id = uuid::Uuid::new_v4();

        let first = svc
            .add_to_group(DataGroupingInput {
                tenant_id: t,
                group_id,
                collection_ids: vec![collection_id],
            })
            .await
            .unwrap();
        assert_eq!(first.members_added, 1);

        let second = svc
            .add_to_group(DataGroupingInput {
                tenant_id: t,
                group_id,
                collection_ids: vec![collection_id],
            })
            .await
            .unwrap();
        assert_eq!(second.members_added, 0);
        assert_eq!(repo.count(), 1);
        assert_eq!(writer.count_of("MemberOfEdge"), 1);
    }

    // ------------------------------------------------------------------
    // SR_INT_12 -- TagWeightService
    // ------------------------------------------------------------------

    struct MockWeightConfig {
        overrides: Vec<(TagCategory, f64)>,
    }

    #[async_trait]
    impl TagWeightConfigRepository for MockWeightConfig {
        async fn get_overrides(
            &self,
            _tenant_id: TenantId,
        ) -> Result<Vec<(TagCategory, f64)>, PrismError> {
            Ok(self.overrides.clone())
        }
    }

    #[tokio::test]
    async fn int12_returns_default_weights_when_no_overrides() {
        let repo = Arc::new(MockWeightConfig { overrides: vec![] });
        let svc = TagWeightService::new(repo);

        let result = svc
            .compute_weights(TagWeightInput {
                tenant_id: tenant(),
                tag_categories: vec![
                    TagCategory::Security,
                    TagCategory::Business,
                    TagCategory::Technical,
                ],
            })
            .await
            .unwrap();

        assert_eq!(result.weights[0], (TagCategory::Security, 1.0));
        assert_eq!(result.weights[1], (TagCategory::Business, 0.7));
        assert_eq!(result.weights[2], (TagCategory::Technical, 0.5));
    }

    #[tokio::test]
    async fn int12_applies_tenant_override() {
        let repo = Arc::new(MockWeightConfig {
            overrides: vec![(TagCategory::Business, 0.9)],
        });
        let svc = TagWeightService::new(repo);

        let result = svc
            .compute_weights(TagWeightInput {
                tenant_id: tenant(),
                tag_categories: vec![TagCategory::Security, TagCategory::Business],
            })
            .await
            .unwrap();

        assert_eq!(result.weights[0], (TagCategory::Security, 1.0));
        assert_eq!(result.weights[1], (TagCategory::Business, 0.9));
    }

    // ------------------------------------------------------------------
    // SR_INT_13 -- CompletenessTagService
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn int13_applies_full_status() {
        let writer = Arc::new(MockGraphWriter::new());
        let svc = CompletenessTagService::new(writer.clone());

        let result = svc
            .apply(CompletenessTagInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
                status: CompletenessStatus::Full,
                missing_fields: vec![],
            })
            .await
            .unwrap();

        assert!(result.tagged);
        assert_eq!(writer.count_of("DataCollectionCompleteness"), 1);
    }

    #[tokio::test]
    async fn int13_applies_partial_status_with_missing_fields() {
        let writer = Arc::new(MockGraphWriter::new());
        let svc = CompletenessTagService::new(writer.clone());

        let result = svc
            .apply(CompletenessTagInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
                status: CompletenessStatus::Partial,
                missing_fields: vec!["ssn".into(), "dob".into()],
            })
            .await
            .unwrap();

        assert!(result.tagged);
        let nodes = writer.nodes.lock().unwrap();
        let (_, props) = nodes.last().unwrap();
        assert_eq!(props["completeness_status"], "partial");
        assert_eq!(props["missing_fields"][0], "ssn");
        assert_eq!(props["missing_fields"][1], "dob");
    }

    // ------------------------------------------------------------------
    // SR_INT_14 -- RecommendationAccuracyService
    // ------------------------------------------------------------------

    struct MockAccuracyRepo {
        store: Mutex<Vec<(TenantId, RecommendationAccuracy)>>,
    }

    impl MockAccuracyRepo {
        fn new() -> Self {
            Self {
                store: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl RecommendationAccuracyRepository for MockAccuracyRepo {
        async fn get(
            &self,
            tenant_id: TenantId,
            collection_id: uuid::Uuid,
        ) -> Result<Option<RecommendationAccuracy>, PrismError> {
            Ok(self
                .store
                .lock()
                .unwrap()
                .iter()
                .find(|(t, a)| *t == tenant_id && a.collection_id == collection_id)
                .map(|(_, a)| a.clone()))
        }

        async fn upsert(
            &self,
            tenant_id: TenantId,
            accuracy: &RecommendationAccuracy,
        ) -> Result<(), PrismError> {
            let mut store = self.store.lock().unwrap();
            if let Some(existing) = store
                .iter_mut()
                .find(|(t, a)| *t == tenant_id && a.collection_id == accuracy.collection_id)
            {
                existing.1 = accuracy.clone();
            } else {
                store.push((tenant_id, accuracy.clone()));
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn int14_first_update_creates_record() {
        let repo = Arc::new(MockAccuracyRepo::new());
        let svc = RecommendationAccuracyService::new(repo.clone());

        let result = svc
            .update_accuracy(AccuracyUpdateInput {
                tenant_id: tenant(),
                collection_id: uuid::Uuid::new_v4(),
                was_accurate: true,
            })
            .await
            .unwrap();

        assert!(result.updated);
        assert!((result.new_rate - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn int14_increments_on_accurate() {
        let repo = Arc::new(MockAccuracyRepo::new());
        let svc = RecommendationAccuracyService::new(repo.clone());
        let t = tenant();
        let c = uuid::Uuid::new_v4();

        // First: accurate.
        svc.update_accuracy(AccuracyUpdateInput {
            tenant_id: t,
            collection_id: c,
            was_accurate: true,
        })
        .await
        .unwrap();
        // Second: accurate.
        let result = svc
            .update_accuracy(AccuracyUpdateInput {
                tenant_id: t,
                collection_id: c,
                was_accurate: true,
            })
            .await
            .unwrap();

        assert!((result.new_rate - 1.0).abs() < f64::EPSILON);
        let stored = repo.get(t, c).await.unwrap().unwrap();
        assert_eq!(stored.used_in_count, 2);
        assert_eq!(stored.accurate_count, 2);
    }

    #[tokio::test]
    async fn int14_increments_on_inaccurate() {
        let repo = Arc::new(MockAccuracyRepo::new());
        let svc = RecommendationAccuracyService::new(repo.clone());
        let t = tenant();
        let c = uuid::Uuid::new_v4();

        svc.update_accuracy(AccuracyUpdateInput {
            tenant_id: t,
            collection_id: c,
            was_accurate: true,
        })
        .await
        .unwrap();
        let result = svc
            .update_accuracy(AccuracyUpdateInput {
                tenant_id: t,
                collection_id: c,
                was_accurate: false,
            })
            .await
            .unwrap();

        assert!((result.new_rate - 0.5).abs() < f64::EPSILON);
        let stored = repo.get(t, c).await.unwrap().unwrap();
        assert_eq!(stored.used_in_count, 2);
        assert_eq!(stored.accurate_count, 1);
    }

    // ------------------------------------------------------------------
    // SR_INT_15 -- VectorSemanticSearchService
    // ------------------------------------------------------------------

    struct StubVectorIndex {
        hits: Vec<(String, f64)>,
    }

    #[async_trait]
    impl VectorIndex for StubVectorIndex {
        async fn search(
            &self,
            _query: &[f32],
            top_k: usize,
        ) -> Result<Vec<(String, f64)>, PrismError> {
            let mut out = self.hits.clone();
            out.truncate(top_k);
            Ok(out)
        }
    }

    struct AllowList {
        allowed: Vec<String>,
    }

    #[async_trait]
    impl CompartmentAccessChecker for AllowList {
        async fn is_allowed(
            &self,
            _principal_id: UserId,
            _principal_roles: &[RoleId],
            resource_id: &str,
        ) -> Result<bool, PrismError> {
            Ok(self.allowed.iter().any(|a| a == resource_id))
        }
    }

    fn search_input(top_k: usize) -> SemanticSearchInput {
        SemanticSearchInput {
            tenant_id: tenant(),
            principal_id: UserId::from(uuid::Uuid::new_v4()),
            principal_roles: vec![],
            query_vector: vec![0.1, 0.2, 0.3],
            top_k,
        }
    }

    #[tokio::test]
    async fn int15_returns_allowed_results() {
        let index = Arc::new(StubVectorIndex {
            hits: vec![
                ("doc-a".into(), 0.95),
                ("doc-b".into(), 0.90),
                ("doc-c".into(), 0.85),
                ("doc-d".into(), 0.80),
            ],
        });
        let checker = Arc::new(AllowList {
            allowed: vec![
                "doc-a".into(),
                "doc-b".into(),
                "doc-c".into(),
                "doc-d".into(),
            ],
        });
        let (audit_repo, audit) = make_audit();
        let svc = VectorSemanticSearchService::new(index, checker, audit);

        let result = svc.search(search_input(2)).await.unwrap();

        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].source_id, "doc-a");
        assert_eq!(result.dropped_for_compartment_count, 0);
        assert!(audit_repo
            .event_types()
            .contains(&"intelligence.semantic_search_executed".to_string()));
    }

    #[tokio::test]
    async fn int15_filters_out_denied_results() {
        let index = Arc::new(StubVectorIndex {
            hits: vec![
                ("secret-1".into(), 0.99), // denied
                ("doc-a".into(), 0.90),
                ("secret-2".into(), 0.85), // denied
                ("doc-b".into(), 0.80),
            ],
        });
        let checker = Arc::new(AllowList {
            allowed: vec!["doc-a".into(), "doc-b".into()],
        });
        let (_audit_repo, audit) = make_audit();
        let svc = VectorSemanticSearchService::new(index, checker, audit);

        let result = svc.search(search_input(2)).await.unwrap();

        assert_eq!(result.results.len(), 2);
        assert!(result.results.iter().all(|r| r.compartment_allowed));
        assert_eq!(result.dropped_for_compartment_count, 2);
        assert!(result
            .results
            .iter()
            .all(|r| !r.source_id.starts_with("secret")));
    }

    #[tokio::test]
    async fn int15_handles_empty_index() {
        let index = Arc::new(StubVectorIndex { hits: vec![] });
        let checker = Arc::new(AllowList { allowed: vec![] });
        let (_audit_repo, audit) = make_audit();
        let svc = VectorSemanticSearchService::new(index, checker, audit);

        let result = svc.search(search_input(5)).await.unwrap();

        assert!(result.results.is_empty());
        assert_eq!(result.filtered_count, 0);
        assert_eq!(result.dropped_for_compartment_count, 0);
    }

    // ------------------------------------------------------------------
    // SR_INT_16 -- CascadeImpactAnalysisService
    // ------------------------------------------------------------------

    struct StubTraversal {
        impacts: Vec<CiaImpact>,
        nodes: Vec<String>,
        edges: Vec<(String, String, String)>,
        max_depth_seen: Mutex<u32>,
    }

    #[async_trait]
    impl GraphTraversal for StubTraversal {
        async fn traverse_impacts(
            &self,
            _source: &str,
            depth: u32,
        ) -> Result<Vec<CiaImpact>, PrismError> {
            *self.max_depth_seen.lock().unwrap() = depth;
            Ok(self
                .impacts
                .iter()
                .filter(|i| i.depth <= depth)
                .cloned()
                .collect())
        }

        async fn traverse_nodes_and_edges(
            &self,
            _focal: &str,
            depth: u32,
        ) -> Result<(Vec<String>, Vec<(String, String, String)>), PrismError> {
            *self.max_depth_seen.lock().unwrap() = depth;
            Ok((self.nodes.clone(), self.edges.clone()))
        }
    }

    #[tokio::test]
    async fn int16_traces_impacts_with_disclosure() {
        let traversal = Arc::new(StubTraversal {
            impacts: vec![
                CiaImpact {
                    target_node: "b".into(),
                    impact_type: CiaImpactType::Downstream,
                    depth: 1,
                    confidence: 0.9,
                },
                CiaImpact {
                    target_node: "c".into(),
                    impact_type: CiaImpactType::Lateral,
                    depth: 1,
                    confidence: 0.8,
                },
            ],
            nodes: vec![],
            edges: vec![],
            max_depth_seen: Mutex::new(0),
        });
        let (_repo, audit) = make_audit();
        let svc = CascadeImpactAnalysisService::new(traversal, audit);

        let result = svc
            .analyze(CiaRequest {
                tenant_id: tenant(),
                source_node: "a".into(),
                depth: 2,
                include_confidence: true,
            })
            .await
            .unwrap();

        assert_eq!(result.tree.source_node, "a");
        assert_eq!(result.tree.impacts.len(), 2);
        assert!(result.coverage_disclosure.contains("%"));
    }

    #[tokio::test]
    async fn int16_respects_depth_limit() {
        let traversal = Arc::new(StubTraversal {
            impacts: vec![
                CiaImpact {
                    target_node: "b".into(),
                    impact_type: CiaImpactType::Downstream,
                    depth: 1,
                    confidence: 0.9,
                },
                CiaImpact {
                    target_node: "d".into(),
                    impact_type: CiaImpactType::SecondOrder,
                    depth: 3,
                    confidence: 0.5,
                },
            ],
            nodes: vec![],
            edges: vec![],
            max_depth_seen: Mutex::new(0),
        });
        let (_repo, audit) = make_audit();
        let svc = CascadeImpactAnalysisService::new(traversal.clone(), audit);

        let result = svc
            .analyze(CiaRequest {
                tenant_id: tenant(),
                source_node: "a".into(),
                depth: 1,
                include_confidence: true,
            })
            .await
            .unwrap();

        assert_eq!(result.tree.impacts.len(), 1);
        assert_eq!(*traversal.max_depth_seen.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn int16_includes_confidence_computation() {
        let traversal = Arc::new(StubTraversal {
            impacts: vec![CiaImpact {
                target_node: "b".into(),
                impact_type: CiaImpactType::Upstream,
                depth: 1,
                confidence: 0.42,
            }],
            nodes: vec![],
            edges: vec![],
            max_depth_seen: Mutex::new(0),
        });
        let (_repo, audit) = make_audit();
        let svc = CascadeImpactAnalysisService::new(traversal, audit);

        let result = svc
            .analyze(CiaRequest {
                tenant_id: tenant(),
                source_node: "a".into(),
                depth: 1,
                include_confidence: true,
            })
            .await
            .unwrap();

        assert!((result.tree.overall_confidence - 0.42).abs() < 1e-9);
    }

    // ------------------------------------------------------------------
    // SR_INT_17 -- SemanticDisambiguationAgent
    // ------------------------------------------------------------------

    struct MockDict {
        existing: Mutex<Vec<SemanticDictionaryEntry>>,
    }

    impl MockDict {
        fn new(existing: Vec<SemanticDictionaryEntry>) -> Self {
            Self {
                existing: Mutex::new(existing),
            }
        }
    }

    #[async_trait]
    impl SemanticDictionaryRepository for MockDict {
        async fn upsert(
            &self,
            _tenant_id: TenantId,
            entry: &SemanticDictionaryEntry,
        ) -> Result<bool, PrismError> {
            let mut ex = self.existing.lock().unwrap();
            if let Some(slot) = ex.iter_mut().find(|e| e.term == entry.term) {
                *slot = entry.clone();
                Ok(false)
            } else {
                ex.push(entry.clone());
                Ok(true)
            }
        }

        async fn list_for_tenant(
            &self,
            _tenant_id: TenantId,
        ) -> Result<Vec<SemanticDictionaryEntry>, PrismError> {
            Ok(self.existing.lock().unwrap().clone())
        }
    }

    struct StubDiscovery {
        entries: Vec<SemanticDictionaryEntry>,
    }

    #[async_trait]
    impl SemanticDiscoveryLlm for StubDiscovery {
        async fn discover(
            &self,
            _tenant_id: TenantId,
        ) -> Result<Vec<SemanticDictionaryEntry>, PrismError> {
            Ok(self.entries.clone())
        }
    }

    #[tokio::test]
    async fn int17_discovers_new_terms() {
        let repo = Arc::new(MockDict::new(vec![]));
        let llm = Arc::new(StubDiscovery {
            entries: vec![
                SemanticDictionaryEntry {
                    term: "revenue".into(),
                    synonyms: vec!["rev".into()],
                    acronyms: vec![],
                    context: None,
                },
                SemanticDictionaryEntry {
                    term: "customer".into(),
                    synonyms: vec!["client".into()],
                    acronyms: vec![],
                    context: None,
                },
            ],
        });
        let (_r, audit) = make_audit();
        let svc = SemanticDisambiguationAgent::new(repo, llm, audit);

        let result = svc
            .run(SdaRunRequest {
                tenant_id: tenant(),
            })
            .await
            .unwrap();
        assert_eq!(result.added, 2);
        assert_eq!(result.modified, 0);
    }

    #[tokio::test]
    async fn int17_updates_existing_entry() {
        let existing = SemanticDictionaryEntry {
            term: "revenue".into(),
            synonyms: vec![],
            acronyms: vec![],
            context: None,
        };
        let repo = Arc::new(MockDict::new(vec![existing]));
        let llm = Arc::new(StubDiscovery {
            entries: vec![SemanticDictionaryEntry {
                term: "revenue".into(),
                synonyms: vec!["rev".into(), "turnover".into()],
                acronyms: vec!["REV".into()],
                context: Some("finance".into()),
            }],
        });
        let (_r, audit) = make_audit();
        let svc = SemanticDisambiguationAgent::new(repo, llm, audit);

        let result = svc
            .run(SdaRunRequest {
                tenant_id: tenant(),
            })
            .await
            .unwrap();
        assert_eq!(result.added, 0);
        assert_eq!(result.modified, 1);
    }

    // ------------------------------------------------------------------
    // SR_INT_18 -- DecisionSupportDataGatheringService
    // ------------------------------------------------------------------

    struct StubFreshness {
        freshness: u64,
        gaps: Vec<String>,
    }

    #[async_trait]
    impl DataFreshnessChecker for StubFreshness {
        async fn check(
            &self,
            _tenant_id: TenantId,
            _query: &str,
        ) -> Result<(u64, Vec<String>), PrismError> {
            Ok((self.freshness, self.gaps.clone()))
        }
    }

    struct StubAggregator {
        payload: serde_json::Value,
    }

    #[async_trait]
    impl DataAggregator for StubAggregator {
        async fn aggregate(
            &self,
            _tenant_id: TenantId,
            _query: &str,
            _parameters: &serde_json::Value,
        ) -> Result<serde_json::Value, PrismError> {
            Ok(self.payload.clone())
        }
    }

    struct StubCsa {
        decision: String,
    }

    #[async_trait]
    impl CsaGate for StubCsa {
        async fn check(
            &self,
            _tenant_id: TenantId,
            _refs: &[String],
        ) -> Result<String, PrismError> {
            Ok(self.decision.clone())
        }
    }

    #[tokio::test]
    async fn int18_gathers_successfully() {
        let freshness = Arc::new(StubFreshness {
            freshness: 42,
            gaps: vec!["missing_q4".into()],
        });
        let aggregator = Arc::new(StubAggregator {
            payload: serde_json::json!({"rows": 12}),
        });
        let csa = Arc::new(StubCsa {
            decision: "allow".into(),
        });
        let (_r, audit) = make_audit();
        let svc = DecisionSupportDataGatheringService::new(freshness, aggregator, csa, audit);

        let result = svc
            .gather(DataGatheringInput {
                tenant_id: tenant(),
                query: "SELECT".into(),
                parameters: serde_json::json!({}),
            })
            .await
            .unwrap();

        assert_eq!(result.freshness_seconds, 42);
        assert_eq!(result.csa_decision, "allow");
        assert_eq!(result.gaps.len(), 1);
    }

    #[tokio::test]
    async fn int18_blocks_on_csa_deny() {
        let freshness = Arc::new(StubFreshness {
            freshness: 0,
            gaps: vec![],
        });
        let aggregator = Arc::new(StubAggregator {
            payload: serde_json::json!({}),
        });
        let csa = Arc::new(StubCsa {
            decision: "deny".into(),
        });
        let (_r, audit) = make_audit();
        let svc = DecisionSupportDataGatheringService::new(freshness, aggregator, csa, audit);

        let err = svc
            .gather(DataGatheringInput {
                tenant_id: tenant(),
                query: "SELECT".into(),
                parameters: serde_json::json!({}),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, PrismError::Forbidden { .. }));
    }

    // ------------------------------------------------------------------
    // SR_INT_19 -- ResearchAgentService
    // ------------------------------------------------------------------

    struct StubSource;

    #[async_trait]
    impl ExternalResearchSource for StubSource {
        async fn fetch(&self, topic: &str) -> Result<String, PrismError> {
            Ok(format!("content-for:{topic}"))
        }
    }

    #[tokio::test]
    async fn int19_fetches_topics_and_creates_collections() {
        let source = Arc::new(StubSource);
        let writer = Arc::new(MockGraphWriter::new());
        let (_r, audit) = make_audit();
        let svc = ResearchAgentService::new(source, writer.clone(), audit);

        let result = svc
            .research(ResearchInput {
                tenant_id: tenant(),
                topics: vec!["weather".into(), "regulations".into()],
            })
            .await
            .unwrap();

        assert_eq!(result.collection_ids.len(), 2);
        assert_eq!(writer.count_of("DataCollection"), 2);
    }

    #[tokio::test]
    async fn int19_handles_empty_topics() {
        let source = Arc::new(StubSource);
        let writer = Arc::new(MockGraphWriter::new());
        let (_r, audit) = make_audit();
        let svc = ResearchAgentService::new(source, writer.clone(), audit);

        let result = svc
            .research(ResearchInput {
                tenant_id: tenant(),
                topics: vec![],
            })
            .await
            .unwrap();

        assert!(result.collection_ids.is_empty());
        assert_eq!(writer.total(), 0);
    }

    // ------------------------------------------------------------------
    // SR_INT_20 -- GraphVizService
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn int20_returns_bounded_subgraph() {
        let traversal = Arc::new(StubTraversal {
            impacts: vec![],
            nodes: vec!["a".into(), "b".into(), "c".into()],
            edges: vec![("a".into(), "b".into(), "FEEDS".into())],
            max_depth_seen: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = GraphVizService::new(traversal, audit);

        let result = svc
            .query(GraphVizRequest {
                tenant_id: tenant(),
                focal_node: "a".into(),
                depth: 2,
            })
            .await
            .unwrap();

        assert_eq!(result.nodes.len(), 3);
        assert_eq!(result.edges.len(), 1);
    }

    #[tokio::test]
    async fn int20_respects_depth_cap() {
        let traversal = Arc::new(StubTraversal {
            impacts: vec![],
            nodes: vec![],
            edges: vec![],
            max_depth_seen: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = GraphVizService::new(traversal.clone(), audit);

        svc.query(GraphVizRequest {
            tenant_id: tenant(),
            focal_node: "a".into(),
            depth: 999,
        })
        .await
        .unwrap();

        assert_eq!(
            *traversal.max_depth_seen.lock().unwrap(),
            GRAPH_VIZ_MAX_DEPTH
        );
    }

    // ------------------------------------------------------------------
    // SR_INT_21 -- AgentFeedbackLoopService
    // ------------------------------------------------------------------

    struct StubMetrics {
        scores: std::collections::HashMap<AgentKind, Vec<f64>>,
    }

    #[async_trait]
    impl AgentMetricsStore for StubMetrics {
        async fn get_metrics(&self, agent: AgentKind) -> Result<Vec<f64>, PrismError> {
            Ok(self.scores.get(&agent).cloned().unwrap_or_default())
        }
    }

    #[tokio::test]
    async fn int21_evaluates_all_five_agents() {
        let mut scores = std::collections::HashMap::new();
        for a in FEEDBACK_LOOP_AGENTS.iter().copied() {
            scores.insert(a, vec![0.95, 0.92, 0.9]);
        }
        let metrics = Arc::new(StubMetrics { scores });
        let (_r, audit) = make_audit();
        let svc = AgentFeedbackLoopService::new(metrics, audit);

        let result = svc
            .evaluate_cycle(AgentFeedbackCycleRequest {
                tenant_id: tenant(),
            })
            .await
            .unwrap();

        assert_eq!(result.agents_evaluated, 5);
        assert!(result.improvements.is_empty());
    }

    #[tokio::test]
    async fn int21_captures_low_score_improvements() {
        let mut scores = std::collections::HashMap::new();
        scores.insert(AgentKind::Tagging, vec![0.5, 0.6]);
        scores.insert(AgentKind::Routing, vec![0.95, 0.93]);
        scores.insert(AgentKind::Research, vec![0.6, 0.65]);
        scores.insert(AgentKind::Quality, vec![0.9, 0.92]);
        scores.insert(AgentKind::Discovery, vec![0.91, 0.93]);
        let metrics = Arc::new(StubMetrics { scores });
        let (_r, audit) = make_audit();
        let svc = AgentFeedbackLoopService::new(metrics, audit);

        let result = svc
            .evaluate_cycle(AgentFeedbackCycleRequest {
                tenant_id: tenant(),
            })
            .await
            .unwrap();

        assert_eq!(result.agents_evaluated, 5);
        assert_eq!(result.improvements.len(), 2);
    }

    // ------------------------------------------------------------------
    // SR_INT_22 -- CrossTenantLearningService
    // ------------------------------------------------------------------

    struct StubVerifier {
        verified: bool,
    }

    #[async_trait]
    impl OptInVerifier for StubVerifier {
        async fn is_verified(&self, _tenant_ids: &[TenantId]) -> Result<bool, PrismError> {
            Ok(self.verified)
        }
    }

    struct StubPatternAggregator;

    #[async_trait]
    impl PatternAggregator for StubPatternAggregator {
        async fn aggregate(&self, _tenant_ids: &[TenantId]) -> Result<Vec<String>, PrismError> {
            Ok(vec!["template.kyc".into(), "benchmark.uptime".into()])
        }
    }

    #[tokio::test]
    async fn int22_opt_in_verified_aggregates() {
        let verifier = Arc::new(StubVerifier { verified: true });
        let aggregator = Arc::new(StubPatternAggregator);
        let (_r, audit) = make_audit();
        let svc = CrossTenantLearningService::new(verifier, aggregator, audit);

        let result = svc
            .aggregate(CrossTenantAggregationInput {
                tenant_ids: vec![tenant(), tenant()],
                opt_in_verified_at: Utc::now(),
            })
            .await
            .unwrap();

        assert_eq!(result.patterns.len(), 2);
        assert_eq!(result.opt_in_state, "verified");
    }

    #[tokio::test]
    async fn int22_rejects_stale_verification() {
        let verifier = Arc::new(StubVerifier { verified: true });
        let aggregator = Arc::new(StubPatternAggregator);
        let (_r, audit) = make_audit();
        let svc = CrossTenantLearningService::new(verifier, aggregator, audit);

        let err = svc
            .aggregate(CrossTenantAggregationInput {
                tenant_ids: vec![tenant()],
                opt_in_verified_at: Utc::now() - Duration::hours(25),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, PrismError::Forbidden { .. }));
    }

    #[tokio::test]
    async fn int22_rejects_unverified_opt_in() {
        let verifier = Arc::new(StubVerifier { verified: false });
        let aggregator = Arc::new(StubPatternAggregator);
        let (_r, audit) = make_audit();
        let svc = CrossTenantLearningService::new(verifier, aggregator, audit);

        let err = svc
            .aggregate(CrossTenantAggregationInput {
                tenant_ids: vec![tenant()],
                opt_in_verified_at: Utc::now(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, PrismError::Forbidden { .. }));
    }

    // ------------------------------------------------------------------
    // SR_INT_23 -- IntelligenceQueryRewriteService
    // ------------------------------------------------------------------

    struct StubRewriter;

    #[async_trait]
    impl QueryRewriter for StubRewriter {
        async fn rewrite(
            &self,
            raw: &str,
            tenant_id: TenantId,
            _principal_id: UserId,
        ) -> Result<String, PrismError> {
            Ok(format!("{raw} WHERE tenant_id = '{tenant_id}'"))
        }
    }

    #[tokio::test]
    async fn int23_rewrites_query() {
        let rewriter = Arc::new(StubRewriter);
        let (_r, audit) = make_audit();
        let svc = IntelligenceQueryRewriteService::new(rewriter, audit);

        let result = svc
            .rewrite(QueryInput {
                tenant_id: tenant(),
                raw_cypher: "MATCH (n) RETURN n".into(),
                principal_id: UserId::from(uuid::Uuid::new_v4()),
            })
            .await
            .unwrap();

        assert!(result.rewritten_query.contains("tenant_id"));
        assert_eq!(result.applied_filters.len(), 2);
    }

    #[tokio::test]
    async fn int23_rejects_forbidden_construct() {
        let rewriter = Arc::new(StubRewriter);
        let (_r, audit) = make_audit();
        let svc = IntelligenceQueryRewriteService::new(rewriter, audit);

        let err = svc
            .rewrite(QueryInput {
                tenant_id: tenant(),
                raw_cypher: "CALL db.drop()".into(),
                principal_id: UserId::from(uuid::Uuid::new_v4()),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, PrismError::Forbidden { .. }));
    }

    // ------------------------------------------------------------------
    // SR_INT_24 -- ProactiveTriggerService
    // ------------------------------------------------------------------

    struct StubTriggerEvaluator {
        fires: u32,
    }

    #[async_trait]
    impl TriggerEvaluator for StubTriggerEvaluator {
        async fn check(
            &self,
            _tenant_id: TenantId,
            _trigger_type: ProactiveTriggerType,
        ) -> Result<u32, PrismError> {
            Ok(self.fires)
        }
    }

    struct CountingSender {
        count: Mutex<u32>,
    }

    #[async_trait]
    impl RecommendationRequestSender for CountingSender {
        async fn send(
            &self,
            _tenant_id: TenantId,
            _trigger_type: ProactiveTriggerType,
        ) -> Result<(), PrismError> {
            *self.count.lock().unwrap() += 1;
            Ok(())
        }
    }

    #[tokio::test]
    async fn int24_fires_on_threshold_crossing() {
        let evaluator = Arc::new(StubTriggerEvaluator { fires: 2 });
        let sender = Arc::new(CountingSender {
            count: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = ProactiveTriggerService::new(evaluator, sender.clone(), audit);

        let result = svc
            .evaluate(ProactiveTriggerRequest {
                tenant_id: tenant(),
                trigger_type: ProactiveTriggerType::ThresholdCrossing,
            })
            .await
            .unwrap();

        assert_eq!(result.triggers_fired, 2);
        assert_eq!(*sender.count.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn int24_fires_on_anomaly() {
        let evaluator = Arc::new(StubTriggerEvaluator { fires: 1 });
        let sender = Arc::new(CountingSender {
            count: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = ProactiveTriggerService::new(evaluator, sender.clone(), audit);

        let result = svc
            .evaluate(ProactiveTriggerRequest {
                tenant_id: tenant(),
                trigger_type: ProactiveTriggerType::Anomaly,
            })
            .await
            .unwrap();

        assert_eq!(result.triggers_fired, 1);
        assert_eq!(*sender.count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn int24_no_fires_when_conditions_clear() {
        let evaluator = Arc::new(StubTriggerEvaluator { fires: 0 });
        let sender = Arc::new(CountingSender {
            count: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = ProactiveTriggerService::new(evaluator, sender.clone(), audit);

        let result = svc
            .evaluate(ProactiveTriggerRequest {
                tenant_id: tenant(),
                trigger_type: ProactiveTriggerType::DataQualityIssue,
            })
            .await
            .unwrap();

        assert_eq!(result.triggers_fired, 0);
        assert_eq!(*sender.count.lock().unwrap(), 0);
    }

    // ------------------------------------------------------------------
    // SR_INT_25 -- IntelligenceMaintenanceService
    // ------------------------------------------------------------------

    struct StubMaintenanceWorker {
        affected: u64,
    }

    #[async_trait]
    impl prism_graph::data_model::GraphMaintenanceWorker for StubMaintenanceWorker {
        async fn execute_cycle(
            &self,
            _tenant_id: Option<TenantId>,
            _cycle_type: MaintenanceCycleType,
        ) -> Result<u64, PrismError> {
            Ok(self.affected)
        }
    }

    #[tokio::test]
    async fn int25_runs_cycle() {
        let worker = Arc::new(StubMaintenanceWorker { affected: 42 });
        let (_r, audit) = make_audit();
        let svc = IntelligenceMaintenanceService::new(worker, audit);

        let result = svc
            .run_cycle(IntelligenceMaintenanceRequest {
                tenant_id: Some(tenant()),
                cycle_type: MaintenanceCycleType::StalePrune,
            })
            .await
            .unwrap();

        assert_eq!(result.cycles_run, 1);
        assert!(result.anomalies.is_empty());
    }

    #[tokio::test]
    async fn int25_records_anomalies_for_high_volume() {
        let worker = Arc::new(StubMaintenanceWorker { affected: 200_000 });
        let (_r, audit) = make_audit();
        let svc = IntelligenceMaintenanceService::new(worker, audit);

        let result = svc
            .run_cycle(IntelligenceMaintenanceRequest {
                tenant_id: None,
                cycle_type: MaintenanceCycleType::OrphanCleanup,
            })
            .await
            .unwrap();

        assert_eq!(result.anomalies.len(), 1);
    }

    // ------------------------------------------------------------------
    // SR_INT_26 -- QueryCostEstimatorService
    // ------------------------------------------------------------------

    struct StubEstimator {
        cost: u64,
    }

    #[async_trait]
    impl CostEstimator for StubEstimator {
        async fn estimate(&self, _query: &str) -> Result<u64, PrismError> {
            Ok(self.cost)
        }
    }

    struct StubQuota {
        remaining: u64,
    }

    #[async_trait]
    impl QuotaEnforcer for StubQuota {
        async fn check_quota(&self, _tenant_id: TenantId) -> Result<u64, PrismError> {
            Ok(self.remaining)
        }
    }

    #[tokio::test]
    async fn int26_allows_cheap_query() {
        let estimator = Arc::new(StubEstimator { cost: 50 });
        let quota = Arc::new(StubQuota { remaining: 100 });
        let (_r, audit) = make_audit();
        let svc = QueryCostEstimatorService::new(estimator, quota, audit);

        let result = svc
            .estimate(CostEstimateInput {
                tenant_id: tenant(),
                query: "MATCH (n) RETURN n LIMIT 10".into(),
            })
            .await
            .unwrap();

        assert!(result.allowed);
        assert_eq!(result.estimated_cost_ms, 50);
    }

    #[tokio::test]
    async fn int26_rejects_expensive_query() {
        let estimator = Arc::new(StubEstimator { cost: 99_999 });
        let quota = Arc::new(StubQuota { remaining: 100 });
        let (_r, audit) = make_audit();
        let svc = QueryCostEstimatorService::new(estimator, quota, audit);

        let result = svc
            .estimate(CostEstimateInput {
                tenant_id: tenant(),
                query: "MATCH (n) RETURN n".into(),
            })
            .await
            .unwrap();

        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn int26_rejects_over_quota() {
        let estimator = Arc::new(StubEstimator { cost: 10 });
        let quota = Arc::new(StubQuota { remaining: 0 });
        let (_r, audit) = make_audit();
        let svc = QueryCostEstimatorService::new(estimator, quota, audit);

        let result = svc
            .estimate(CostEstimateInput {
                tenant_id: tenant(),
                query: "MATCH (n) RETURN n".into(),
            })
            .await
            .unwrap();

        assert!(!result.allowed);
    }

    // ------------------------------------------------------------------
    // SR_INT_27 -- BulkImportWorkerService
    // ------------------------------------------------------------------

    struct DedicatedBulkQueue {
        calls: Mutex<Vec<uuid::Uuid>>,
    }

    #[async_trait]
    impl BulkImportQueue for DedicatedBulkQueue {
        async fn enqueue(
            &self,
            _tenant_id: TenantId,
            import_id: uuid::Uuid,
        ) -> Result<u64, PrismError> {
            self.calls.lock().unwrap().push(import_id);
            Ok(1)
        }
    }

    #[tokio::test]
    async fn int27_enqueues_to_dedicated_queue() {
        let queue = Arc::new(DedicatedBulkQueue {
            calls: Mutex::new(Vec::new()),
        });
        let (_r, audit) = make_audit();
        let svc = BulkImportWorkerService::new(queue.clone(), audit);

        let import_id = uuid::Uuid::new_v4();
        let result = svc
            .process(BulkImportProcessingInput {
                tenant_id: tenant(),
                import_id,
            })
            .await
            .unwrap();

        assert_eq!(result.processed, 1);
        assert_eq!(queue.calls.lock().unwrap().len(), 1);
        assert_eq!(queue.calls.lock().unwrap()[0], import_id);
    }

    // ------------------------------------------------------------------
    // SR_INT_28 -- ReadThroughCacheService
    // ------------------------------------------------------------------

    struct MockCacheStore {
        entries: Mutex<Vec<(String, CacheEntry)>>,
        sets: Mutex<u32>,
    }

    impl MockCacheStore {
        fn new() -> Self {
            Self {
                entries: Mutex::new(Vec::new()),
                sets: Mutex::new(0),
            }
        }

        fn with_entry(key: &str, entry: CacheEntry) -> Self {
            let s = Self::new();
            s.entries.lock().unwrap().push((key.to_string(), entry));
            s
        }
    }

    #[async_trait]
    impl CacheStore for MockCacheStore {
        async fn get(&self, key: &str) -> Result<Option<CacheEntry>, PrismError> {
            Ok(self
                .entries
                .lock()
                .unwrap()
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, e)| e.clone()))
        }

        async fn set(&self, key: &str, entry: CacheEntry) -> Result<(), PrismError> {
            let mut entries = self.entries.lock().unwrap();
            entries.retain(|(k, _)| k != key);
            entries.push((key.to_string(), entry));
            *self.sets.lock().unwrap() += 1;
            Ok(())
        }

        async fn invalidate(&self, key: &str) -> Result<(), PrismError> {
            self.entries.lock().unwrap().retain(|(k, _)| k != key);
            Ok(())
        }
    }

    struct CountingSource {
        fetches: Mutex<u32>,
    }

    #[async_trait]
    impl CacheDataSource for CountingSource {
        async fn fetch(&self, _key: &str) -> Result<serde_json::Value, PrismError> {
            *self.fetches.lock().unwrap() += 1;
            Ok(serde_json::json!({"rows": 1}))
        }
    }

    #[tokio::test]
    async fn int28_cache_hit_returns_cached() {
        let now = Utc::now().timestamp();
        let cache = Arc::new(MockCacheStore::with_entry(
            "k",
            CacheEntry {
                payload: serde_json::json!({"cached": true}),
                written_at_epoch: now,
            },
        ));
        let source = Arc::new(CountingSource {
            fetches: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = ReadThroughCacheService::new(cache, source.clone(), audit);

        let result = svc
            .query_with_cache(CacheRequest {
                tenant_id: tenant(),
                key: "k".into(),
                ttl_seconds: 60,
                degradation_mode: false,
            })
            .await
            .unwrap();

        assert_eq!(result.source, "cache");
        assert_eq!(*source.fetches.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn int28_cache_miss_fetches_source() {
        let cache = Arc::new(MockCacheStore::new());
        let source = Arc::new(CountingSource {
            fetches: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = ReadThroughCacheService::new(cache, source.clone(), audit);

        let result = svc
            .query_with_cache(CacheRequest {
                tenant_id: tenant(),
                key: "nope".into(),
                ttl_seconds: 60,
                degradation_mode: false,
            })
            .await
            .unwrap();

        assert_eq!(result.source, "source");
        assert_eq!(*source.fetches.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn int28_degradation_extends_ttl() {
        // Entry is stale relative to the normal TTL (20s) but within the
        // extended TTL (20s * 10 = 200s), so degradation_mode must return it.
        let stale = Utc::now().timestamp() - 100;
        let cache = Arc::new(MockCacheStore::with_entry(
            "k",
            CacheEntry {
                payload: serde_json::json!({"cached": true}),
                written_at_epoch: stale,
            },
        ));
        let source = Arc::new(CountingSource {
            fetches: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = ReadThroughCacheService::new(cache, source.clone(), audit);

        let result = svc
            .query_with_cache(CacheRequest {
                tenant_id: tenant(),
                key: "k".into(),
                ttl_seconds: 20,
                degradation_mode: true,
            })
            .await
            .unwrap();

        assert_eq!(result.source, "cache");
        assert!(result.degradation_active);
        assert_eq!(*source.fetches.lock().unwrap(), 0);
    }

    // ------------------------------------------------------------------
    // SR_INT_29 -- DisasterRecoveryDrillService
    // ------------------------------------------------------------------

    struct StubDrExecutor {
        rto: u64,
        rpo: u64,
    }

    #[async_trait]
    impl DrExecutor for StubDrExecutor {
        async fn execute(&self, _scenario: DrScenario) -> Result<(u64, u64), PrismError> {
            Ok((self.rto, self.rpo))
        }
    }

    struct StubEscalator {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl DrEscalator for StubEscalator {
        async fn escalate(
            &self,
            _scenario: DrScenario,
            _reason: &str,
        ) -> Result<String, PrismError> {
            *self.calls.lock().unwrap() += 1;
            Ok("INC-42".into())
        }
    }

    #[tokio::test]
    async fn int29_drill_passes() {
        let executor = Arc::new(StubDrExecutor { rto: 100, rpo: 10 });
        let escalator = Arc::new(StubEscalator {
            calls: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = DisasterRecoveryDrillService::new(executor, escalator.clone(), audit);

        let result = svc
            .run_drill(DrDrillRequest {
                scenario: DrScenario::Neo4jPrimaryFail,
                target_rto_seconds: 300,
                target_rpo_seconds: 30,
            })
            .await
            .unwrap();

        assert!(result.passed);
        assert!(result.escalation_id.is_none());
        assert_eq!(*escalator.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn int29_drill_fails_triggers_escalation() {
        let executor = Arc::new(StubDrExecutor { rto: 600, rpo: 60 });
        let escalator = Arc::new(StubEscalator {
            calls: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = DisasterRecoveryDrillService::new(executor, escalator.clone(), audit);

        let result = svc
            .run_drill(DrDrillRequest {
                scenario: DrScenario::Neo4jPrimaryFail,
                target_rto_seconds: 300,
                target_rpo_seconds: 30,
            })
            .await
            .unwrap();

        assert!(!result.passed);
        assert_eq!(result.escalation_id.as_deref(), Some("INC-42"));
        assert_eq!(*escalator.calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn int29_records_measured_times() {
        let executor = Arc::new(StubDrExecutor { rto: 42, rpo: 7 });
        let escalator = Arc::new(StubEscalator {
            calls: Mutex::new(0),
        });
        let (_r, audit) = make_audit();
        let svc = DisasterRecoveryDrillService::new(executor, escalator, audit);

        let result = svc
            .run_drill(DrDrillRequest {
                scenario: DrScenario::PostgresFailover,
                target_rto_seconds: 120,
                target_rpo_seconds: 10,
            })
            .await
            .unwrap();

        assert_eq!(result.measured_rto_seconds, 42);
        assert_eq!(result.measured_rpo_seconds, 7);
    }

    // ------------------------------------------------------------------
    // SR_INT_30 -- TenantOffboardingService
    // ------------------------------------------------------------------

    struct StubShred {
        certs: Vec<String>,
    }

    #[async_trait]
    impl CryptoShredService for StubShred {
        async fn shred_all(&self, _tenant_id: TenantId) -> Result<Vec<String>, PrismError> {
            Ok(self.certs.clone())
        }
    }

    struct StubRemover {
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl DataRemover for StubRemover {
        async fn remove(&self, _tenant_id: TenantId) -> Result<u64, PrismError> {
            *self.calls.lock().unwrap() += 1;
            Ok(1000)
        }
    }

    struct StubVerifierInt30 {
        verified: bool,
    }

    #[async_trait]
    impl OffboardingVerifier for StubVerifierInt30 {
        async fn verify(&self, _tenant_id: TenantId) -> Result<bool, PrismError> {
            Ok(self.verified)
        }
    }

    struct StubIssuer;

    #[async_trait]
    impl CertificateIssuer for StubIssuer {
        async fn issue(&self, tenant_id: TenantId) -> Result<String, PrismError> {
            Ok(format!("https://certs.example/{tenant_id}"))
        }
    }

    #[tokio::test]
    async fn int30_offboards_successfully() {
        let shred = Arc::new(StubShred {
            certs: vec!["shred-1".into(), "shred-2".into()],
        });
        let remover = Arc::new(StubRemover {
            calls: Mutex::new(0),
        });
        let verifier = Arc::new(StubVerifierInt30 { verified: true });
        let issuer = Arc::new(StubIssuer);
        let (_r, audit) = make_audit();
        let svc = TenantOffboardingService::new(shred, remover.clone(), verifier, issuer, audit);

        let result = svc
            .offboard(OffboardingRequest {
                tenant_id: tenant(),
                confirm_all_subjects: true,
            })
            .await
            .unwrap();

        assert!(result.verified);
        assert!(result.certificate_url.starts_with("https://"));
        assert_eq!(result.shred_certificates.len(), 2);
        assert_eq!(*remover.calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn int30_fails_if_verification_fails() {
        let shred = Arc::new(StubShred {
            certs: vec!["shred-1".into()],
        });
        let remover = Arc::new(StubRemover {
            calls: Mutex::new(0),
        });
        let verifier = Arc::new(StubVerifierInt30 { verified: false });
        let issuer = Arc::new(StubIssuer);
        let (_r, audit) = make_audit();
        let svc = TenantOffboardingService::new(shred, remover, verifier, issuer, audit);

        let result = svc
            .offboard(OffboardingRequest {
                tenant_id: tenant(),
                confirm_all_subjects: true,
            })
            .await
            .unwrap();

        assert!(!result.verified);
        assert!(result.certificate_url.is_empty());
    }

    #[tokio::test]
    async fn int30_issues_shred_certificates() {
        let shred = Arc::new(StubShred {
            certs: vec!["a".into(), "b".into(), "c".into()],
        });
        let remover = Arc::new(StubRemover {
            calls: Mutex::new(0),
        });
        let verifier = Arc::new(StubVerifierInt30 { verified: true });
        let issuer = Arc::new(StubIssuer);
        let (_r, audit) = make_audit();
        let svc = TenantOffboardingService::new(shred, remover, verifier, issuer, audit);

        let result = svc
            .offboard(OffboardingRequest {
                tenant_id: tenant(),
                confirm_all_subjects: true,
            })
            .await
            .unwrap();

        assert_eq!(result.shred_certificates, vec!["a", "b", "c"]);
    }
}
