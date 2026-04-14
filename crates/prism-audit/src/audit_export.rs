//! Signed audit export with chain proof for regulatory review (SR_GOV_50).
//!
//! The `AuditExportService` produces a signed export bundle containing
//! a contiguous slice of the tenant's audit trail, plus a `ChainProof`
//! that allows an external verifier to confirm the slice was not
//! redacted or fabricated.
//!
//! Implements: SR_GOV_50

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tracing::info;

use prism_core::error::PrismError;
use prism_core::repository::AuditEventRepository;
use prism_core::types::*;

use crate::merkle_chain::MerkleChainHasher;

// ---------------------------------------------------------------------------
// ExportSigner trait
// ---------------------------------------------------------------------------

/// Signs an export payload for regulatory integrity verification.
///
/// Implementations may use HMAC-SHA256, RSA, or HSM-backed keys.
/// The trait is intentionally minimal -- PRISM does not dictate the
/// signing algorithm; it only requires a deterministic, verifiable
/// signature over the payload bytes.
///
/// Implements: SR_GOV_50 (signing key asset)
#[async_trait]
pub trait ExportSigner: Send + Sync {
    /// Sign the given payload bytes and return a hex-encoded signature.
    async fn sign(&self, payload: &[u8]) -> Result<String, PrismError>;
}

// ---------------------------------------------------------------------------
// AuditExportService
// ---------------------------------------------------------------------------

/// Service that exports a signed audit slice with chain proof.
///
/// Composes:
/// - `AuditEventRepository` -- fetch events in range
/// - `MerkleChainHasher` -- verify chain integrity of the segment
/// - `ExportSigner` -- sign the serialized bundle
///
/// Implements: SR_GOV_50
pub struct AuditExportService {
    repo: Arc<dyn AuditEventRepository>,
    signer: Arc<dyn ExportSigner>,
}

impl AuditExportService {
    /// Create a new export service.
    pub fn new(repo: Arc<dyn AuditEventRepository>, signer: Arc<dyn ExportSigner>) -> Self {
        Self { repo, signer }
    }

    /// Export an audit slice for the requested tenant and time range.
    ///
    /// 1. Query events within the time range.
    /// 2. Verify the chain integrity of the segment.
    /// 3. Build the chain proof (anchor + tip hash, positions).
    /// 4. Serialize events according to the requested format.
    /// 5. Sign the serialized payload.
    /// 6. Return the signed bundle with chain proof.
    ///
    /// Implements: SR_GOV_50
    pub async fn export(
        &self,
        request: &AuditExportRequest,
    ) -> Result<AuditExportResult, PrismError> {
        // Step 1: fetch events in time range via the query interface.
        let query = AuditQueryRequest {
            tenant_id: request.tenant_id,
            event_type: None,
            actor_id: None,
            target_id: None,
            severity: None,
            from_time: Some(request.time_range.from),
            to_time: Some(request.time_range.to),
            page_size: i64::MAX, // export needs the full range
            page_token: None,
        };

        let query_result = self.repo.query(&query).await?;
        let mut events = query_result.events;

        if events.is_empty() {
            return Err(PrismError::Validation {
                reason: "no audit events found in the requested time range".into(),
            });
        }

        // Ensure ascending chain_position order for verification.
        events.sort_by_key(|e| e.chain_position);

        // Step 2: verify chain integrity of the exported segment.
        let verification = MerkleChainHasher::verify_chain(&events);
        if !verification.is_valid {
            return Err(PrismError::GovernanceViolation {
                rule: "SR_GOV_50".into(),
                detail: format!(
                    "chain integrity check failed at position {}; cannot produce a valid export",
                    verification.mismatch_at.unwrap_or(-1)
                ),
            });
        }

        // Step 3: build chain proof.
        let first = events.first().unwrap(); // safe: checked non-empty above
        let last = events.last().unwrap();

        let chain_proof = ChainProof {
            anchor_hash: first.event_hash.clone(),
            tip_hash: last.event_hash.clone(),
            segment_length: events.len() as u64,
            position_range: (first.chain_position, last.chain_position),
        };

        // Step 4: serialize according to requested format.
        let export_payload = Self::serialize_events(&events, request.format)?;

        // Step 5: sign the payload.
        let signature = self.signer.sign(&export_payload).await?;

        let event_count = events.len() as u64;

        info!(
            tenant_id = %request.tenant_id,
            event_count = event_count,
            format = ?request.format,
            anchor_position = first.chain_position,
            tip_position = last.chain_position,
            "audit export generated"
        );

        // Step 6: return the bundle.
        Ok(AuditExportResult {
            export_payload,
            signature,
            chain_proof,
            event_count,
        })
    }

    /// Serialize events into the requested format.
    ///
    /// Implements: SR_GOV_50 (format parameter)
    fn serialize_events(
        events: &[AuditEvent],
        format: ExportFormat,
    ) -> Result<Vec<u8>, PrismError> {
        match format {
            ExportFormat::JsonLines => {
                let mut buf = Vec::new();
                for event in events {
                    let line = serde_json::to_string(event).map_err(|e| {
                        PrismError::Serialization(format!("failed to serialize event: {e}"))
                    })?;
                    buf.extend_from_slice(line.as_bytes());
                    buf.push(b'\n');
                }
                Ok(buf)
            }
            ExportFormat::Csv => {
                let mut buf = Vec::new();
                // Header row
                buf.extend_from_slice(
                    b"id,tenant_id,event_type,actor_id,actor_type,severity,source_layer,chain_position,event_hash,created_at\n",
                );
                for event in events {
                    let row = format!(
                        "{},{},{},{},{:?},{:?},{:?},{},{},{}\n",
                        event.id,
                        event.tenant_id,
                        event.event_type,
                        event.actor_id,
                        event.actor_type,
                        event.severity,
                        event.source_layer,
                        event.chain_position,
                        event.event_hash,
                        event.created_at.to_rfc3339(),
                    );
                    buf.extend_from_slice(row.as_bytes());
                }
                Ok(buf)
            }
            ExportFormat::Pdf => {
                // PDF generation is a rendering concern -- produce JSON payload
                // and let the caller's PDF renderer consume it. The signed
                // payload is the canonical JSON; the PDF is derived.
                let wrapper = serde_json::json!({
                    "format": "pdf_source",
                    "events": events,
                    "generated_at": Utc::now().to_rfc3339(),
                });
                serde_json::to_vec_pretty(&wrapper).map_err(|e| {
                    PrismError::Serialization(format!("failed to serialize PDF source: {e}"))
                })
            }
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Duration;
    use std::sync::Mutex;

    // -- Mock ExportSigner --------------------------------------------------

    struct MockSigner;

    #[async_trait]
    impl ExportSigner for MockSigner {
        async fn sign(&self, payload: &[u8]) -> Result<String, PrismError> {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(payload);
            Ok(format!("{:x}", hash))
        }
    }

    // -- Mock AuditEventRepository ------------------------------------------

    struct MockAuditRepo {
        events: Mutex<Vec<AuditEvent>>,
    }

    impl MockAuditRepo {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }

        fn push(&self, event: AuditEvent) {
            self.events.lock().unwrap().push(event);
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
                .filter(|e| {
                    e.tenant_id == request.tenant_id
                        && request.from_time.map_or(true, |from| e.created_at >= from)
                        && request.to_time.map_or(true, |to| e.created_at <= to)
                })
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

    // -- Helpers -------------------------------------------------------------

    fn make_chained_event(
        tenant_id: TenantId,
        position: i64,
        prev_hash: Option<&str>,
        created_at: chrono::DateTime<Utc>,
    ) -> AuditEvent {
        let canonical = MerkleChainHasher::canonical_bytes(
            tenant_id.as_uuid(),
            "governance.action",
            &uuid::Uuid::nil(),
            "system",
            None,
            None,
            "low",
            "governance",
            &serde_json::json!({"pos": position}),
            &created_at,
        );
        let event_hash = MerkleChainHasher::compute_hash(prev_hash, &canonical);

        AuditEvent {
            id: AuditEventId::new(),
            tenant_id,
            event_type: "governance.action".into(),
            actor_id: uuid::Uuid::nil(),
            actor_type: ActorType::System,
            target_id: None,
            target_type: None,
            severity: Severity::Low,
            source_layer: SourceLayer::Governance,
            governance_authority: None,
            payload: serde_json::json!({"pos": position}),
            prev_event_hash: prev_hash.map(String::from),
            event_hash,
            chain_position: position,
            created_at,
        }
    }

    fn build_chain(tenant_id: TenantId, count: usize) -> Vec<AuditEvent> {
        let base = Utc::now() - Duration::hours(count as i64);
        let mut events: Vec<AuditEvent> = Vec::with_capacity(count);

        for i in 0..count {
            let prev = if i == 0 {
                None
            } else {
                Some(events[i - 1].event_hash.as_str())
            };
            let ts = base + Duration::hours(i as i64);
            events.push(make_chained_event(tenant_id, i as i64, prev, ts));
        }
        events
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn export_json_lines_produces_signed_bundle() {
        let tenant_id = TenantId::new();
        let repo = Arc::new(MockAuditRepo::new());
        let chain = build_chain(tenant_id, 3);
        for e in &chain {
            repo.push(e.clone());
        }

        let svc = AuditExportService::new(repo, Arc::new(MockSigner));
        let request = AuditExportRequest {
            tenant_id,
            time_range: TimeRange {
                from: chain[0].created_at - Duration::seconds(1),
                to: chain[2].created_at + Duration::seconds(1),
            },
            format: ExportFormat::JsonLines,
        };

        let result = svc.export(&request).await.unwrap();

        assert_eq!(result.event_count, 3);
        assert!(!result.signature.is_empty());
        assert_eq!(result.chain_proof.segment_length, 3);
        assert_eq!(result.chain_proof.anchor_hash, chain[0].event_hash);
        assert_eq!(result.chain_proof.tip_hash, chain[2].event_hash);
        assert_eq!(result.chain_proof.position_range, (0, 2));

        // Payload should have 3 JSON lines
        let payload_str = String::from_utf8(result.export_payload).unwrap();
        let lines: Vec<_> = payload_str.lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn export_csv_includes_header_and_rows() {
        let tenant_id = TenantId::new();
        let repo = Arc::new(MockAuditRepo::new());
        let chain = build_chain(tenant_id, 2);
        for e in &chain {
            repo.push(e.clone());
        }

        let svc = AuditExportService::new(repo, Arc::new(MockSigner));
        let request = AuditExportRequest {
            tenant_id,
            time_range: TimeRange {
                from: chain[0].created_at - Duration::seconds(1),
                to: chain[1].created_at + Duration::seconds(1),
            },
            format: ExportFormat::Csv,
        };

        let result = svc.export(&request).await.unwrap();
        let payload_str = String::from_utf8(result.export_payload).unwrap();
        let lines: Vec<_> = payload_str.lines().collect();
        // 1 header + 2 data rows
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("id,tenant_id"));
    }

    #[tokio::test]
    async fn export_rejects_empty_range() {
        let tenant_id = TenantId::new();
        let repo = Arc::new(MockAuditRepo::new());

        let svc = AuditExportService::new(repo, Arc::new(MockSigner));
        let request = AuditExportRequest {
            tenant_id,
            time_range: TimeRange {
                from: Utc::now() - Duration::hours(1),
                to: Utc::now(),
            },
            format: ExportFormat::JsonLines,
        };

        let err = svc.export(&request).await.unwrap_err();
        assert!(matches!(err, PrismError::Validation { .. }));
    }

    #[tokio::test]
    async fn export_rejects_tampered_chain() {
        let tenant_id = TenantId::new();
        let repo = Arc::new(MockAuditRepo::new());
        let mut chain = build_chain(tenant_id, 3);
        // Tamper with the middle event's hash
        chain[1].event_hash = "tampered".into();
        for e in &chain {
            repo.push(e.clone());
        }

        let svc = AuditExportService::new(repo, Arc::new(MockSigner));
        let request = AuditExportRequest {
            tenant_id,
            time_range: TimeRange {
                from: chain[0].created_at - Duration::seconds(1),
                to: chain[2].created_at + Duration::seconds(1),
            },
            format: ExportFormat::JsonLines,
        };

        let err = svc.export(&request).await.unwrap_err();
        assert!(matches!(err, PrismError::GovernanceViolation { .. }));
    }

    #[tokio::test]
    async fn export_chain_proof_matches_segment_boundaries() {
        let tenant_id = TenantId::new();
        let repo = Arc::new(MockAuditRepo::new());
        let chain = build_chain(tenant_id, 5);
        for e in &chain {
            repo.push(e.clone());
        }

        // Export only the middle 3 events (positions 1..3)
        let svc = AuditExportService::new(repo, Arc::new(MockSigner));
        let request = AuditExportRequest {
            tenant_id,
            time_range: TimeRange {
                from: chain[1].created_at - Duration::milliseconds(1),
                to: chain[3].created_at + Duration::milliseconds(1),
            },
            format: ExportFormat::JsonLines,
        };

        let result = svc.export(&request).await.unwrap();
        assert_eq!(result.event_count, 3);
        assert_eq!(result.chain_proof.anchor_hash, chain[1].event_hash);
        assert_eq!(result.chain_proof.tip_hash, chain[3].event_hash);
        assert_eq!(result.chain_proof.position_range, (1, 3));
    }

    #[tokio::test]
    async fn export_signature_is_deterministic_for_same_payload() {
        let tenant_id = TenantId::new();
        let repo = Arc::new(MockAuditRepo::new());
        let chain = build_chain(tenant_id, 2);
        for e in &chain {
            repo.push(e.clone());
        }

        let svc = AuditExportService::new(repo, Arc::new(MockSigner));
        let request = AuditExportRequest {
            tenant_id,
            time_range: TimeRange {
                from: chain[0].created_at - Duration::seconds(1),
                to: chain[1].created_at + Duration::seconds(1),
            },
            format: ExportFormat::JsonLines,
        };

        let r1 = svc.export(&request).await.unwrap();
        let r2 = svc.export(&request).await.unwrap();
        assert_eq!(r1.signature, r2.signature);
    }
}
