//! Log stream ingestion services: adapter, parser selection, PII redaction,
//! multi-system correlation, ingestion modes, and metrics.
//!
//! Implements: SR_CONN_19 through SR_CONN_24

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use prism_core::error::PrismError;
use prism_core::types::*;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Source of raw log events for the log stream adapter.
///
/// Implementations read events from syslog collectors, Kafka topics,
/// file tails, or any other event source.
///
/// Implements: SR_CONN_19
#[async_trait]
pub trait LogSource: Send + Sync {
    /// Read events starting from `since_offset`, returning at most `limit`.
    async fn read_events(
        &self,
        since_offset: u64,
        limit: usize,
    ) -> Result<Vec<RawLogEvent>, PrismError>;
}

/// A log parser that converts raw log text into structured JSON.
///
/// Each parser handles one format (syslog, JSON lines, nginx, CSV, etc.).
///
/// Implements: SR_CONN_20
#[async_trait]
pub trait LogParser: Send + Sync {
    /// Parse a raw log line into structured JSON.
    fn parse(&self, raw: &str) -> Result<serde_json::Value, PrismError>;
}

/// Detector for personally identifiable information in log text.
///
/// Implementations use regex, NER models, or lookup tables to find PII.
///
/// Implements: SR_CONN_21
#[async_trait]
pub trait PiiDetector: Send + Sync {
    /// Detect PII matches in the given text.
    fn detect_pii(&self, text: &str) -> Vec<PiiMatch>;
}

/// Persistence writer for log metrics.
///
/// Implementations write to PostgreSQL or another time-series store.
///
/// Implements: SR_CONN_24
#[async_trait]
pub trait PgWriter: Send + Sync {
    /// Persist a log metric row.
    async fn write_metric(&self, row: &LogMetricRow) -> Result<(), PrismError>;
}

// ---------------------------------------------------------------------------
// SR_CONN_19 -- Type 8: Log Stream Adapter
// ---------------------------------------------------------------------------

/// Adapter for log stream ingestion.
///
/// Reads raw events from a `LogSource`, tracks the offset watermark,
/// and returns the count of ingested events.
///
/// Implements: SR_CONN_19
pub struct LogStreamAdapter {
    source: Arc<dyn LogSource>,
}

impl LogStreamAdapter {
    pub fn new(source: Arc<dyn LogSource>) -> Self {
        Self { source }
    }

    /// Ingest log events from the configured source.
    ///
    /// Reads events starting from the given offset and returns
    /// the number of events ingested plus the new watermark offset.
    ///
    /// Implements: SR_CONN_19
    pub async fn ingest(&self, input: LogIngestInput) -> Result<LogIngestResult, PrismError> {
        let events = self.source.read_events(input.since_offset, 1000).await?;

        let count = events.len() as u64;
        let last_offset = if count > 0 {
            input.since_offset + count
        } else {
            input.since_offset
        };

        Ok(LogIngestResult {
            events_ingested: count,
            last_offset,
        })
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_20 -- Parser Selection
// ---------------------------------------------------------------------------

// A format detector entry: (name, heuristic function).
type FormatDetector = (&'static str, Box<dyn Fn(&str) -> bool + Send + Sync>);

/// Registry of known log parsers with automatic format detection.
///
/// Examines sample lines to select the best parser. If no known parser
/// matches, returns a candidate string for admin review.
///
/// Implements: SR_CONN_20
pub struct LogParserRegistry {
    known_formats: Vec<FormatDetector>,
}

impl LogParserRegistry {
    /// Create a registry with the four standard parsers.
    pub fn new() -> Self {
        let known_formats: Vec<FormatDetector> = vec![
            (
                "json_lines",
                Box::new(|line: &str| {
                    let trimmed = line.trim();
                    trimmed.starts_with('{') && trimmed.ends_with('}')
                }),
            ),
            (
                "syslog",
                Box::new(|line: &str| {
                    // Syslog lines typically start with a month abbreviation
                    let months = [
                        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct",
                        "Nov", "Dec",
                    ];
                    let trimmed = line.trim();
                    months.iter().any(|m| trimmed.starts_with(m))
                }),
            ),
            (
                "nginx",
                Box::new(|line: &str| {
                    // Nginx combined log format contains " - - [" pattern
                    line.contains(" - - [") || line.contains("\" 200 ") || line.contains("\" 404 ")
                }),
            ),
            (
                "csv",
                Box::new(|line: &str| {
                    let commas = line.matches(',').count();
                    commas >= 2 && !line.trim().starts_with('{')
                }),
            ),
        ];

        Self { known_formats }
    }

    /// Select a parser for the given sample lines.
    ///
    /// Returns the parser ID of the best match, or a candidate string
    /// for admin review if no known parser matches.
    ///
    /// Implements: SR_CONN_20
    pub fn select_parser(&self, input: &ParserSelectionInput) -> ParserSelectionResult {
        if input.sample_lines.is_empty() {
            return ParserSelectionResult {
                parser_id: None,
                candidate: Some("empty_sample".to_string()),
            };
        }

        // Try each known format -- select the first one where a majority of
        // sample lines match the detector heuristic.
        for (name, detector) in &self.known_formats {
            let matches = input
                .sample_lines
                .iter()
                .filter(|line| detector(line))
                .count();
            if matches > input.sample_lines.len() / 2 {
                return ParserSelectionResult {
                    parser_id: Some(name.to_string()),
                    candidate: None,
                };
            }
        }

        // No match -- return first line as candidate for admin review
        ParserSelectionResult {
            parser_id: None,
            candidate: Some(input.sample_lines.first().cloned().unwrap_or_default()),
        }
    }
}

impl Default for LogParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_21 -- PII Redaction
// ---------------------------------------------------------------------------

/// Service that redacts PII from log events before storage.
///
/// Uses a `PiiDetector` to find PII spans and replaces each with
/// `[REDACTED:{pii_type}]`.
///
/// Implements: SR_CONN_21
pub struct LogRedactionService {
    detector: Arc<dyn PiiDetector>,
}

impl LogRedactionService {
    pub fn new(detector: Arc<dyn PiiDetector>) -> Self {
        Self { detector }
    }

    /// Redact PII from a list of log event strings.
    ///
    /// Returns the redacted events and the total number of PII matches found.
    ///
    /// Implements: SR_CONN_21
    pub fn redact(&self, events: &[String]) -> (Vec<String>, usize) {
        let mut redacted_events = Vec::with_capacity(events.len());
        let mut total_redactions = 0;

        for event in events {
            let mut matches = self.detector.detect_pii(event);
            if matches.is_empty() {
                redacted_events.push(event.clone());
                continue;
            }

            // Sort matches by start position descending so we can replace
            // from the end without invalidating earlier byte offsets.
            matches.sort_by(|a, b| b.start.cmp(&a.start));
            total_redactions += matches.len();

            let mut redacted = event.clone();
            for m in &matches {
                let replacement = format!("[REDACTED:{}]", m.pii_type);
                let end = m.end.min(redacted.len());
                let start = m.start.min(end);
                redacted.replace_range(start..end, &replacement);
            }
            redacted_events.push(redacted);
        }

        (redacted_events, total_redactions)
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_22 -- Multi-System Correlation
// ---------------------------------------------------------------------------

/// Service that detects correlation IDs across log events from
/// multiple sources.
///
/// Looks for common correlation patterns (request_id, trace_id,
/// correlation_id) and creates `CorrelationTrace` records.
///
/// Implements: SR_CONN_22
pub struct CorrelationService;

impl CorrelationService {
    pub fn new() -> Self {
        Self
    }

    /// Detect correlation IDs in parsed log events.
    ///
    /// Each event is a (event_id, parsed_json) pair. The service scans
    /// for `request_id`, `trace_id`, and `correlation_id` fields and
    /// groups events sharing the same value.
    ///
    /// Implements: SR_CONN_22
    pub fn detect(&self, events: &[(uuid::Uuid, serde_json::Value)]) -> Vec<CorrelationTrace> {
        use std::collections::HashMap;

        let correlation_keys = ["request_id", "trace_id", "correlation_id"];
        let mut groups: HashMap<String, Vec<uuid::Uuid>> = HashMap::new();

        for (event_id, value) in events {
            if let Some(obj) = value.as_object() {
                for key in &correlation_keys {
                    if let Some(val) = obj.get(*key).and_then(|v| v.as_str()) {
                        let group_key = format!("{}:{}", key, val);
                        groups.entry(group_key).or_default().push(*event_id);
                    }
                }
            }
        }

        // Only create traces for groups with 2+ events (actual correlation)
        let now = Utc::now();
        groups
            .into_iter()
            .filter(|(_, ids)| ids.len() >= 2)
            .map(|(trace_id, source_events)| CorrelationTrace {
                trace_id,
                source_events,
                created_at: now,
            })
            .collect()
    }
}

impl Default for CorrelationService {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_23 -- Log Ingestion Modes
// ---------------------------------------------------------------------------

/// Service for configuring and managing log ingestion modes.
///
/// Each log source can be configured with an ingestion mode that
/// determines scheduling behavior.
///
/// Implements: SR_CONN_23
pub struct IngestionModeService;

impl IngestionModeService {
    pub fn new() -> Self {
        Self
    }

    /// Configure the ingestion mode for a log source.
    ///
    /// For scheduled mode, computes the next run time. For real-time
    /// and near-real-time modes, `next_run_at` is None (continuous).
    ///
    /// Implements: SR_CONN_23
    pub fn configure(&self, request: &LogIngestionModeRequest) -> LogIngestionModeResult {
        let next_run_at = match request.mode {
            IngestionMode::Scheduled => {
                // Default: next run in 1 hour
                Some(Utc::now() + chrono::Duration::hours(1))
            }
            IngestionMode::Batch => {
                // Default: next run at midnight
                Some(Utc::now() + chrono::Duration::hours(24))
            }
            IngestionMode::OnDemand => None,
            IngestionMode::RealTime | IngestionMode::NearRealTime => None,
        };

        LogIngestionModeResult {
            mode: request.mode,
            next_run_at,
        }
    }
}

impl Default for IngestionModeService {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// SR_CONN_24 -- Log Ingestion Metrics
// ---------------------------------------------------------------------------

/// Service for recording log ingestion metrics.
///
/// Persists operational telemetry about ingestion throughput,
/// parse failure rates, lag, and redaction counts.
///
/// Implements: SR_CONN_24
pub struct LogMetricsService {
    writer: Arc<dyn PgWriter>,
}

impl LogMetricsService {
    pub fn new(writer: Arc<dyn PgWriter>) -> Self {
        Self { writer }
    }

    /// Record a log metric row.
    ///
    /// Implements: SR_CONN_24
    pub async fn record(&self, row: &LogMetricRow) -> Result<(), PrismError> {
        self.writer.write_metric(row).await
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // -- Mock LogSource -------------------------------------------------------

    struct MockLogSource {
        events: Mutex<Vec<RawLogEvent>>,
    }

    impl MockLogSource {
        fn new(events: Vec<RawLogEvent>) -> Self {
            Self {
                events: Mutex::new(events),
            }
        }
    }

    #[async_trait]
    impl LogSource for MockLogSource {
        async fn read_events(
            &self,
            _since_offset: u64,
            _limit: usize,
        ) -> Result<Vec<RawLogEvent>, PrismError> {
            let events = self.events.lock().unwrap();
            Ok(events.clone())
        }
    }

    // -- Mock PiiDetector -----------------------------------------------------

    struct MockPiiDetector {
        matches: Mutex<Vec<Vec<PiiMatch>>>,
    }

    impl MockPiiDetector {
        fn new(matches: Vec<Vec<PiiMatch>>) -> Self {
            Self {
                matches: Mutex::new(matches),
            }
        }
    }

    #[async_trait]
    impl PiiDetector for MockPiiDetector {
        fn detect_pii(&self, _text: &str) -> Vec<PiiMatch> {
            let mut matches = self.matches.lock().unwrap();
            if matches.is_empty() {
                vec![]
            } else {
                matches.remove(0)
            }
        }
    }

    // -- Mock PgWriter --------------------------------------------------------

    struct MockPgWriter {
        rows: Mutex<Vec<LogMetricRow>>,
    }

    impl MockPgWriter {
        fn new() -> Self {
            Self {
                rows: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl PgWriter for MockPgWriter {
        async fn write_metric(&self, row: &LogMetricRow) -> Result<(), PrismError> {
            self.rows.lock().unwrap().push(row.clone());
            Ok(())
        }
    }

    // -- SR_CONN_19 tests -----------------------------------------------------

    #[tokio::test]
    async fn log_stream_adapter_ingests_events() {
        let events = vec![
            RawLogEvent {
                content: "event 1".to_string(),
                timestamp: Some(Utc::now()),
                source_id: "src1".to_string(),
            },
            RawLogEvent {
                content: "event 2".to_string(),
                timestamp: Some(Utc::now()),
                source_id: "src1".to_string(),
            },
        ];
        let source = Arc::new(MockLogSource::new(events));
        let adapter = LogStreamAdapter::new(source);

        let result = adapter
            .ingest(LogIngestInput {
                tenant_id: TenantId::new(),
                source_id: "src1".to_string(),
                mode: "real_time".to_string(),
                since_offset: 0,
            })
            .await
            .unwrap();

        assert_eq!(result.events_ingested, 2);
        assert_eq!(result.last_offset, 2);
    }

    #[tokio::test]
    async fn log_stream_adapter_tracks_offset() {
        let events = vec![RawLogEvent {
            content: "single event".to_string(),
            timestamp: None,
            source_id: "src2".to_string(),
        }];
        let source = Arc::new(MockLogSource::new(events));
        let adapter = LogStreamAdapter::new(source);

        let result = adapter
            .ingest(LogIngestInput {
                tenant_id: TenantId::new(),
                source_id: "src2".to_string(),
                mode: "batch".to_string(),
                since_offset: 100,
            })
            .await
            .unwrap();

        assert_eq!(result.events_ingested, 1);
        assert_eq!(result.last_offset, 101);
    }

    // -- SR_CONN_20 tests -----------------------------------------------------

    #[test]
    fn parser_registry_selects_known_format() {
        let registry = LogParserRegistry::new();
        let input = ParserSelectionInput {
            sample_lines: vec![
                r#"{"timestamp":"2026-01-01","level":"info","msg":"hello"}"#.to_string(),
                r#"{"timestamp":"2026-01-01","level":"warn","msg":"world"}"#.to_string(),
            ],
        };
        let result = registry.select_parser(&input);
        assert_eq!(result.parser_id, Some("json_lines".to_string()));
        assert!(result.candidate.is_none());
    }

    #[test]
    fn parser_registry_returns_candidate_for_unknown_format() {
        let registry = LogParserRegistry::new();
        let input = ParserSelectionInput {
            sample_lines: vec![
                "CUSTOM|2026-01-01|INFO|some message".to_string(),
                "CUSTOM|2026-01-01|WARN|another message".to_string(),
            ],
        };
        let result = registry.select_parser(&input);
        assert!(result.parser_id.is_none());
        assert!(result.candidate.is_some());
    }

    // -- SR_CONN_21 tests -----------------------------------------------------

    #[test]
    fn redaction_service_redacts_detected_pii() {
        let detector = Arc::new(MockPiiDetector::new(vec![vec![PiiMatch {
            field: "email".to_string(),
            pii_type: "email".to_string(),
            start: 5,
            end: 22,
        }]]));
        let service = LogRedactionService::new(detector);
        let events = vec!["User user@example.com logged in".to_string()];
        let (redacted, count) = service.redact(&events);
        assert_eq!(count, 1);
        assert!(redacted[0].contains("[REDACTED:email]"));
        assert!(!redacted[0].contains("user@example.com"));
    }

    #[test]
    fn redaction_service_passes_clean_content() {
        let detector = Arc::new(MockPiiDetector::new(vec![vec![]]));
        let service = LogRedactionService::new(detector);
        let events = vec!["System started successfully".to_string()];
        let (redacted, count) = service.redact(&events);
        assert_eq!(count, 0);
        assert_eq!(redacted[0], "System started successfully");
    }

    // -- SR_CONN_22 tests -----------------------------------------------------

    #[test]
    fn correlation_service_detects_correlation_ids() {
        let service = CorrelationService::new();
        let id1 = uuid::Uuid::now_v7();
        let id2 = uuid::Uuid::now_v7();
        let id3 = uuid::Uuid::now_v7();

        let events = vec![
            (
                id1,
                serde_json::json!({"request_id": "req-123", "msg": "start"}),
            ),
            (
                id2,
                serde_json::json!({"request_id": "req-123", "msg": "end"}),
            ),
            (
                id3,
                serde_json::json!({"request_id": "req-456", "msg": "solo"}),
            ),
        ];

        let traces = service.detect(&events);
        // req-123 has 2 events => 1 trace; req-456 has 1 event => no trace
        assert_eq!(traces.len(), 1);
        assert!(traces[0].trace_id.contains("req-123"));
        assert_eq!(traces[0].source_events.len(), 2);
    }

    #[test]
    fn correlation_service_handles_no_correlations() {
        let service = CorrelationService::new();
        let events = vec![
            (
                uuid::Uuid::now_v7(),
                serde_json::json!({"msg": "standalone event"}),
            ),
            (
                uuid::Uuid::now_v7(),
                serde_json::json!({"msg": "another standalone"}),
            ),
        ];

        let traces = service.detect(&events);
        assert!(traces.is_empty());
    }

    // -- SR_CONN_23 tests -----------------------------------------------------

    #[test]
    fn ingestion_mode_configures_scheduled() {
        let service = IngestionModeService::new();
        let result = service.configure(&LogIngestionModeRequest {
            source_id: "src1".to_string(),
            mode: IngestionMode::Scheduled,
        });
        assert_eq!(result.mode, IngestionMode::Scheduled);
        assert!(result.next_run_at.is_some());
    }

    #[test]
    fn ingestion_mode_configures_real_time() {
        let service = IngestionModeService::new();
        let result = service.configure(&LogIngestionModeRequest {
            source_id: "src1".to_string(),
            mode: IngestionMode::RealTime,
        });
        assert_eq!(result.mode, IngestionMode::RealTime);
        assert!(result.next_run_at.is_none());
    }

    // -- SR_CONN_24 tests -----------------------------------------------------

    #[tokio::test]
    async fn log_metrics_service_records_metrics() {
        let writer = Arc::new(MockPgWriter::new());
        let service = LogMetricsService::new(writer.clone());

        let row = LogMetricRow {
            tenant_id: TenantId::new(),
            source_id: "src1".to_string(),
            events_per_second: 42.5,
            parse_failure_rate: 0.01,
            lag_seconds: 3,
            redaction_count: 7,
        };

        service.record(&row).await.unwrap();

        let stored = writer.rows.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].source_id, "src1");
    }
}
