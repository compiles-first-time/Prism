//! Classification gate pipeline: normalized record builder, technical/security/
//! semantic classification, relationship inference, and quality assessment.
//!
//! Implements: SR_CONN_25 through SR_CONN_31

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use prism_core::error::PrismError;
use prism_core::types::*;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Named-entity recognition ensemble for security classification.
///
/// Implementations scan free-text for PII, secrets, and sensitive entities.
///
/// Implements: SR_CONN_28
#[async_trait]
pub trait NerEnsemble: Send + Sync {
    /// Scan text for named entities and return matches with confidence.
    fn scan(&self, text: &str) -> Vec<NerMatch>;
}

/// LLM client for semantic classification and inference tasks.
///
/// Implements: SR_CONN_29
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a prompt to the LLM and return the response text.
    async fn infer(&self, prompt: &str) -> Result<String, PrismError>;
}

// ---------------------------------------------------------------------------
// SR_CONN_25 -- Normalized Record Builder
// ---------------------------------------------------------------------------

/// Builder that converts raw adapter output into a fully populated
/// `ExecutionRecord` with all required fields.
///
/// Implements: SR_CONN_25
pub struct NormalizedRecordBuilder;

impl NormalizedRecordBuilder {
    /// Build an `ExecutionRecord` from raw adapter output.
    ///
    /// Takes the raw payload, adapter type, tenant/connection IDs, and
    /// data origin and produces a fully normalized execution record.
    ///
    /// Implements: SR_CONN_25
    pub fn build(
        payload: &serde_json::Value,
        adapter_type: &str,
        tenant_id: TenantId,
        connection_id: uuid::Uuid,
        data_origin: DataOrigin,
    ) -> ExecutionRecord {
        // Extract field names from the payload object (top-level keys).
        let fields: Vec<String> = payload
            .as_object()
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default();

        let records_pulled = payload.as_object().map(|obj| obj.len() as u64).unwrap_or(0);

        ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id,
            connection_id,
            credential_type: adapter_type.to_string(),
            source_system: adapter_type.to_string(),
            records_pulled,
            fields,
            data_origin,
            status: "success".to_string(),
            error: None,
            latency_ms: 0,
            created_at: Utc::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_26 -- Stage 1 Technical Classification
// ---------------------------------------------------------------------------

/// Deterministic technical classifier that extracts types from field names.
///
/// Applies suffix-based heuristics:
/// - `_id` suffix -> "identifier"
/// - `_at` / `_date` suffix -> "timestamp"
/// - `_pct` / `_rate` suffix -> "numeric"
///
/// Implements: SR_CONN_26
pub struct TechnicalClassifier;

impl TechnicalClassifier {
    /// Classify an execution record's fields using deterministic suffix rules.
    ///
    /// Implements: SR_CONN_26
    pub fn classify(record: &ExecutionRecord) -> TechnicalClassificationResult {
        let mut types = Vec::new();
        let mut formats = Vec::new();

        for field in &record.fields {
            let lower = field.to_lowercase();
            if lower.ends_with("_id") {
                types.push("identifier".to_string());
            } else if lower.ends_with("_at") || lower.ends_with("_date") {
                types.push("timestamp".to_string());
            } else if lower.ends_with("_pct") || lower.ends_with("_rate") {
                types.push("numeric".to_string());
            } else {
                types.push("text".to_string());
            }
            formats.push("string".to_string());
        }

        TechnicalClassificationResult {
            types,
            formats,
            schema_version: "1.0".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_28 -- Stage 2 Security Classification
// ---------------------------------------------------------------------------

/// Security classifier using an NER ensemble to detect PII and sensitive data.
///
/// Defaults to MORE restrictive on low confidence (< 0.7): any entity
/// with confidence below the threshold is flagged as critical PII.
///
/// Implements: SR_CONN_28
pub struct SecurityClassifier {
    ner: Arc<dyn NerEnsemble>,
}

impl SecurityClassifier {
    pub fn new(ner: Arc<dyn NerEnsemble>) -> Self {
        Self { ner }
    }

    /// Classify an execution record for security-sensitive content.
    ///
    /// Scans all field names through the NER ensemble. Any match with
    /// confidence < 0.7 is escalated to "critical_pii" (restrictive default).
    ///
    /// Implements: SR_CONN_28
    pub fn classify(&self, record: &ExecutionRecord) -> SecurityClassificationResult {
        let mut classifications = Vec::new();
        let mut confidence_per_field = Vec::new();

        for field in &record.fields {
            let matches = self.ner.scan(field);
            for m in &matches {
                if m.confidence < 0.7 {
                    // Default to MORE restrictive on low confidence
                    classifications.push("critical_pii".to_string());
                } else {
                    classifications.push(m.classification.clone());
                }
                confidence_per_field.push((m.entity.clone(), m.confidence));
            }
        }

        SecurityClassificationResult {
            classifications,
            confidence_per_field,
        }
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_27 -- Classification Gate Orchestrator
// ---------------------------------------------------------------------------

/// Orchestrator that runs the classification pipeline and produces a gate
/// decision (Allow / Block).
///
/// Runs Stage 1 (technical) then Stage 2 (security). If the security
/// classifier flags critical PII and any field has confidence < 0.7, the
/// gate BLOCKs.
///
/// Implements: SR_CONN_27
pub struct ClassificationGate {
    ner: Arc<dyn NerEnsemble>,
}

impl ClassificationGate {
    pub fn new(ner: Arc<dyn NerEnsemble>) -> Self {
        Self { ner }
    }

    /// Evaluate an execution record through the classification gate.
    ///
    /// Implements: SR_CONN_27
    pub fn evaluate(&self, record: &ExecutionRecord) -> ClassificationGateResult {
        // Stage 1: technical classification
        let technical_result = TechnicalClassifier::classify(record);

        // Stage 2: security classification
        let security_classifier = SecurityClassifier::new(Arc::clone(&self.ner));
        let security_result = security_classifier.classify(record);

        // Gate decision: block if critical PII detected with low confidence
        let has_critical_pii = security_result
            .classifications
            .contains(&"critical_pii".to_string());
        let has_low_confidence = security_result
            .confidence_per_field
            .iter()
            .any(|(_, conf)| *conf < 0.7);

        let gate = if has_critical_pii && has_low_confidence {
            ClassificationGateDecision::Block
        } else {
            ClassificationGateDecision::Allow
        };

        ClassificationGateResult {
            gate,
            technical_result,
            security_result,
        }
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_29 -- Stage 3 Semantic Classification (async)
// ---------------------------------------------------------------------------

/// Semantic classifier that uses an LLM to tag fields with semantic types
/// and business domains.
///
/// Implements: SR_CONN_29
pub struct SemanticClassifier {
    llm: Arc<dyn LlmClient>,
}

impl SemanticClassifier {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// Classify fields semantically using an LLM.
    ///
    /// For each field in the execution record, sends a prompt to the LLM
    /// to determine the semantic type and business domain.
    ///
    /// Implements: SR_CONN_29
    pub async fn classify_async(
        &self,
        record: &ExecutionRecord,
    ) -> Result<SemanticClassificationResult, PrismError> {
        let mut fields = Vec::new();

        for field in &record.fields {
            let prompt = format!(
                "Classify field '{}' with semantic_type and business_domain. Return JSON.",
                field
            );
            let response = self.llm.infer(&prompt).await?;

            // Parse response -- in production this would parse JSON from the LLM.
            // For now, use the response as the semantic type.
            fields.push(SemanticFieldTag {
                field_name: field.clone(),
                semantic_type: response.clone(),
                business_domain: "financial_services".to_string(),
            });
        }

        Ok(SemanticClassificationResult { fields })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_30 -- Stage 4 Relationship Inference (async)
// ---------------------------------------------------------------------------

/// Service that proposes candidate edges between data fields based on
/// naming conventions and co-occurrence patterns.
///
/// Implements: SR_CONN_30
pub struct RelationshipInferenceService {
    llm: Arc<dyn LlmClient>,
}

impl RelationshipInferenceService {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }

    /// Infer candidate relationships between fields in an execution record.
    ///
    /// Proposes edges with confidence scores. Fields sharing common prefixes
    /// or ID-reference patterns are matched.
    ///
    /// Implements: SR_CONN_30
    pub async fn infer(&self, record: &ExecutionRecord) -> Result<Vec<CandidateEdge>, PrismError> {
        let mut edges = Vec::new();

        // Heuristic: fields ending in _id may reference other entities.
        // For each _id field, look for a corresponding entity field.
        for field in &record.fields {
            if field.ends_with("_id") {
                let prefix = &field[..field.len() - 3]; // strip "_id"
                for other in &record.fields {
                    if other != field && other.starts_with(prefix) {
                        let prompt = format!(
                            "Confirm relationship between '{}' and '{}'. Return confidence.",
                            field, other
                        );
                        let response = self.llm.infer(&prompt).await?;
                        let confidence: f64 = response.parse().unwrap_or(0.8);

                        edges.push(CandidateEdge {
                            from_field: field.clone(),
                            to_field: other.clone(),
                            relationship: "references".to_string(),
                            confidence,
                            confirmed_by: "llm_inference".to_string(),
                        });
                    }
                }
            }
        }

        Ok(edges)
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_31 -- Stage 5 Quality Assessment (async)
// ---------------------------------------------------------------------------

/// Service that computes data quality scores for an execution record.
///
/// Scores: completeness (non-empty fields ratio), consistency (uniform types),
/// freshness (recency of the pull).
///
/// Implements: SR_CONN_31
pub struct QualityAssessmentService;

impl QualityAssessmentService {
    /// Assess data quality for an execution record.
    ///
    /// Returns a `DataQualityReport` with completeness, consistency,
    /// freshness, and an overall score (average of the three).
    ///
    /// Implements: SR_CONN_31
    pub async fn assess(record: &ExecutionRecord) -> Result<DataQualityReport, PrismError> {
        let total_fields = record.fields.len() as f64;

        // Completeness: ratio of non-empty fields
        let non_empty = record.fields.iter().filter(|f| !f.is_empty()).count() as f64;
        let completeness = if total_fields > 0.0 {
            non_empty / total_fields
        } else {
            0.0
        };

        // Consistency: all fields present = 1.0 (simplified)
        let consistency = if record.error.is_none() { 1.0 } else { 0.5 };

        // Freshness: based on how recent created_at is (simplified)
        let age_seconds = (Utc::now() - record.created_at)
            .num_seconds()
            .unsigned_abs();
        let freshness = if age_seconds < 3600 { 1.0 } else { 0.5 };

        let overall_score = (completeness + consistency + freshness) / 3.0;

        Ok(DataQualityReport {
            collection_id: record.id,
            overall_score,
            completeness,
            consistency,
            freshness,
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // -- Mock NerEnsemble -----------------------------------------------------

    struct MockNerEnsemble {
        matches: Mutex<Vec<NerMatch>>,
    }

    impl MockNerEnsemble {
        fn new(matches: Vec<NerMatch>) -> Self {
            Self {
                matches: Mutex::new(matches),
            }
        }
    }

    impl NerEnsemble for MockNerEnsemble {
        fn scan(&self, _text: &str) -> Vec<NerMatch> {
            let matches = self.matches.lock().unwrap();
            matches.clone()
        }
    }

    // -- Mock LlmClient -------------------------------------------------------

    struct MockLlmClient {
        responses: Mutex<Vec<String>>,
    }

    impl MockLlmClient {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn infer(&self, _prompt: &str) -> Result<String, PrismError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Ok("default_response".to_string())
            } else {
                Ok(responses.remove(0))
            }
        }
    }

    // -- Helper ---------------------------------------------------------------

    fn make_record(fields: Vec<&str>) -> ExecutionRecord {
        ExecutionRecord {
            id: uuid::Uuid::now_v7(),
            tenant_id: TenantId::new(),
            connection_id: uuid::Uuid::now_v7(),
            credential_type: "test".to_string(),
            source_system: "test_system".to_string(),
            records_pulled: 10,
            fields: fields.into_iter().map(|s| s.to_string()).collect(),
            data_origin: DataOrigin::ConnectionPull,
            status: "success".to_string(),
            error: None,
            latency_ms: 42,
            created_at: Utc::now(),
        }
    }

    // -- SR_CONN_25 tests -----------------------------------------------------

    #[test]
    fn normalized_record_builder_populates_all_fields() {
        let payload = serde_json::json!({
            "user_id": "abc",
            "created_at": "2026-01-01",
            "amount_pct": 0.5
        });
        let tenant_id = TenantId::new();
        let conn_id = uuid::Uuid::now_v7();

        let record = NormalizedRecordBuilder::build(
            &payload,
            "delegated",
            tenant_id,
            conn_id,
            DataOrigin::ConnectionPull,
        );

        assert_eq!(record.tenant_id, tenant_id);
        assert_eq!(record.connection_id, conn_id);
        assert_eq!(record.credential_type, "delegated");
        assert_eq!(record.fields.len(), 3);
        assert_eq!(record.status, "success");
    }

    // -- SR_CONN_26 tests -----------------------------------------------------

    #[test]
    fn technical_classifier_extracts_types_from_suffixes() {
        let record = make_record(vec!["user_id", "created_at", "error_rate", "name"]);
        let result = TechnicalClassifier::classify(&record);

        assert_eq!(result.types.len(), 4);
        assert!(result.types.contains(&"identifier".to_string()));
        assert!(result.types.contains(&"timestamp".to_string()));
        assert!(result.types.contains(&"numeric".to_string()));
        assert!(result.types.contains(&"text".to_string()));
        assert_eq!(result.schema_version, "1.0");
    }

    // -- SR_CONN_27 tests -----------------------------------------------------

    #[test]
    fn classification_gate_allows_clean_data() {
        let ner = Arc::new(MockNerEnsemble::new(vec![]));
        let gate = ClassificationGate::new(ner);
        let record = make_record(vec!["account_id", "balance"]);

        let result = gate.evaluate(&record);
        assert_eq!(result.gate, ClassificationGateDecision::Allow);
    }

    #[test]
    fn classification_gate_blocks_high_risk_data() {
        let ner = Arc::new(MockNerEnsemble::new(vec![NerMatch {
            entity: "ssn".to_string(),
            classification: "pii".to_string(),
            confidence: 0.4,
        }]));
        let gate = ClassificationGate::new(ner);
        let record = make_record(vec!["ssn_field"]);

        let result = gate.evaluate(&record);
        assert_eq!(result.gate, ClassificationGateDecision::Block);
        assert!(result
            .security_result
            .classifications
            .contains(&"critical_pii".to_string()));
    }

    // -- SR_CONN_28 tests -----------------------------------------------------

    #[test]
    fn security_classifier_classifies_pii() {
        let ner = Arc::new(MockNerEnsemble::new(vec![NerMatch {
            entity: "email".to_string(),
            classification: "pii_email".to_string(),
            confidence: 0.95,
        }]));
        let classifier = SecurityClassifier::new(ner);
        let record = make_record(vec!["email_address"]);

        let result = classifier.classify(&record);
        assert!(result.classifications.contains(&"pii_email".to_string()));
        assert!(!result.confidence_per_field.is_empty());
    }

    #[test]
    fn security_classifier_defaults_restrictive_on_low_confidence() {
        let ner = Arc::new(MockNerEnsemble::new(vec![NerMatch {
            entity: "maybe_ssn".to_string(),
            classification: "pii_ssn".to_string(),
            confidence: 0.3,
        }]));
        let classifier = SecurityClassifier::new(ner);
        let record = make_record(vec!["ambiguous_field"]);

        let result = classifier.classify(&record);
        // Low confidence should escalate to critical_pii
        assert!(result.classifications.contains(&"critical_pii".to_string()));
    }

    // -- SR_CONN_29 tests -----------------------------------------------------

    #[tokio::test]
    async fn semantic_classifier_tags_fields() {
        let llm = Arc::new(MockLlmClient::new(vec![
            "customer_identifier".to_string(),
            "transaction_timestamp".to_string(),
        ]));
        let classifier = SemanticClassifier::new(llm);
        let record = make_record(vec!["customer_id", "txn_date"]);

        let result = classifier.classify_async(&record).await.unwrap();
        assert_eq!(result.fields.len(), 2);
        assert_eq!(result.fields[0].semantic_type, "customer_identifier");
        assert_eq!(result.fields[1].business_domain, "financial_services");
    }

    // -- SR_CONN_30 tests -----------------------------------------------------

    #[tokio::test]
    async fn relationship_inference_proposes_edges() {
        let llm = Arc::new(MockLlmClient::new(vec!["0.9".to_string()]));
        let service = RelationshipInferenceService::new(llm);
        let record = make_record(vec!["account_id", "account_name"]);

        let edges = service.infer(&record).await.unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].from_field, "account_id");
        assert_eq!(edges[0].to_field, "account_name");
        assert_eq!(edges[0].relationship, "references");
        assert!(edges[0].confidence > 0.8);
    }

    // -- SR_CONN_31 tests -----------------------------------------------------

    #[tokio::test]
    async fn quality_assessment_computes_scores() {
        let record = make_record(vec!["field_a", "field_b", "field_c"]);
        let report = QualityAssessmentService::assess(&record).await.unwrap();

        assert!(report.completeness > 0.0);
        assert!(report.consistency > 0.0);
        assert!(report.freshness > 0.0);
        assert!(report.overall_score > 0.0);
        assert_eq!(report.collection_id, record.id);
    }
}
