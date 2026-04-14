//! Cryptographic hash chain for tamper evidence (D-22, REUSABLE_MerkleChainHasher).
//!
//! Computes `SHA-256(prev_event_hash || canonical_event_bytes)` to produce
//! each event's `event_hash`. The chain is per-tenant: each tenant has its own
//! independent hash sequence anchored at chain_position 0.

use prism_core::types::{AuditEvent, ChainVerificationResult};
use sha2::{Digest, Sha256};

/// Pure-logic Merkle chain hasher. No I/O -- operates on in-memory data only.
///
/// Implements: D-22 (event-sourced audit with Merkle hash chain)
pub struct MerkleChainHasher;

impl MerkleChainHasher {
    /// Compute the hash for a new audit event.
    ///
    /// `prev_hash` is `None` for the very first event in a tenant's chain
    /// (chain_position = 0). For all subsequent events it must be `Some`.
    ///
    /// `event_bytes` is the canonical serialization of the event content
    /// (produced by [`Self::canonical_bytes`]).
    ///
    /// Implements: SR_GOV_47 (hash computation step)
    pub fn compute_hash(prev_hash: Option<&str>, event_bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();

        // Chain link: include the previous hash if present.
        match prev_hash {
            Some(h) => hasher.update(h.as_bytes()),
            None => hasher.update(b"GENESIS"),
        }

        hasher.update(event_bytes);

        format!("{:x}", hasher.finalize())
    }

    /// Produce canonical bytes for an [`AuditEvent`].
    ///
    /// Deterministic serialization: fields are written in a fixed order so
    /// that the same logical event always yields the same hash. We concatenate
    /// the fields with a NUL separator to avoid ambiguity.
    #[allow(clippy::too_many_arguments)]
    pub fn canonical_bytes(
        tenant_id: &uuid::Uuid,
        event_type: &str,
        actor_id: &uuid::Uuid,
        actor_type: &str,
        target_id: Option<&uuid::Uuid>,
        target_type: Option<&str>,
        severity: &str,
        source_layer: &str,
        payload: &serde_json::Value,
        created_at: &chrono::DateTime<chrono::Utc>,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(512);
        let sep = b'\0';

        buf.extend_from_slice(tenant_id.to_string().as_bytes());
        buf.push(sep);
        buf.extend_from_slice(event_type.as_bytes());
        buf.push(sep);
        buf.extend_from_slice(actor_id.to_string().as_bytes());
        buf.push(sep);
        buf.extend_from_slice(actor_type.as_bytes());
        buf.push(sep);
        buf.extend_from_slice(
            target_id
                .map(|id| id.to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        buf.push(sep);
        buf.extend_from_slice(target_type.unwrap_or("").as_bytes());
        buf.push(sep);
        buf.extend_from_slice(severity.as_bytes());
        buf.push(sep);
        buf.extend_from_slice(source_layer.as_bytes());
        buf.push(sep);
        // serde_json::to_string is deterministic for the same Value.
        buf.extend_from_slice(
            serde_json::to_string(payload)
                .unwrap_or_default()
                .as_bytes(),
        );
        buf.push(sep);
        buf.extend_from_slice(created_at.to_rfc3339().as_bytes());

        buf
    }

    /// Convenience: compute canonical bytes from a fully-formed [`AuditEvent`].
    pub fn canonical_bytes_from_event(event: &AuditEvent) -> Vec<u8> {
        Self::canonical_bytes(
            event.tenant_id.as_uuid(),
            &event.event_type,
            &event.actor_id,
            &format!("{:?}", event.actor_type).to_lowercase(),
            event.target_id.as_ref(),
            event.target_type.as_deref(),
            &format!("{:?}", event.severity).to_lowercase(),
            &format!("{:?}", event.source_layer).to_lowercase(),
            &event.payload,
            &event.created_at,
        )
    }

    /// Verify a contiguous chain segment.
    ///
    /// `events` must be ordered by `chain_position` **ascending**. The first
    /// event in the slice is treated as the trust anchor -- its stored hash is
    /// assumed correct and every subsequent event is verified against it.
    ///
    /// Implements: SR_GOV_48 (chain verification)
    pub fn verify_chain(events: &[AuditEvent]) -> ChainVerificationResult {
        if events.is_empty() {
            return ChainVerificationResult {
                is_valid: true,
                verified_count: 0,
                mismatch_at: None,
                anchor_hash: String::new(),
            };
        }

        let anchor_hash = events[0].event_hash.clone();

        for window in events.windows(2) {
            let prev = &window[0];
            let curr = &window[1];

            let canonical = Self::canonical_bytes_from_event(curr);
            let expected = Self::compute_hash(Some(&prev.event_hash), &canonical);

            if expected != curr.event_hash {
                return ChainVerificationResult {
                    is_valid: false,
                    verified_count: (curr.chain_position - events[0].chain_position) as u32,
                    mismatch_at: Some(curr.chain_position),
                    anchor_hash,
                };
            }
        }

        ChainVerificationResult {
            is_valid: true,
            verified_count: events.len() as u32,
            mismatch_at: None,
            anchor_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use prism_core::types::*;
    use uuid::Uuid;

    fn make_event(position: i64, prev_hash: Option<&str>) -> AuditEvent {
        let tenant_id = TenantId::from_uuid(Uuid::nil());
        let actor_id = Uuid::nil();
        let created_at = Utc::now();

        let canonical = MerkleChainHasher::canonical_bytes(
            tenant_id.as_uuid(),
            "test.event",
            &actor_id,
            "system",
            None,
            None,
            "low",
            "governance",
            &serde_json::json!({}),
            &created_at,
        );

        let event_hash = MerkleChainHasher::compute_hash(prev_hash, &canonical);

        AuditEvent {
            id: AuditEventId::new(),
            tenant_id,
            event_type: "test.event".into(),
            actor_id,
            actor_type: ActorType::System,
            target_id: None,
            target_type: None,
            severity: Severity::Low,
            source_layer: SourceLayer::Governance,
            governance_authority: None,
            payload: serde_json::json!({}),
            prev_event_hash: prev_hash.map(String::from),
            event_hash,
            chain_position: position,
            created_at,
        }
    }

    #[test]
    fn genesis_event_uses_genesis_salt() {
        let hash = MerkleChainHasher::compute_hash(None, b"test-data");
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars
    }

    #[test]
    fn chained_event_differs_from_genesis() {
        let data = b"same-data";
        let genesis = MerkleChainHasher::compute_hash(None, data);
        let chained = MerkleChainHasher::compute_hash(Some(&genesis), data);
        assert_ne!(genesis, chained);
    }

    #[test]
    fn hash_is_deterministic() {
        let data = b"determinism-test";
        let h1 = MerkleChainHasher::compute_hash(Some("abc"), data);
        let h2 = MerkleChainHasher::compute_hash(Some("abc"), data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn verify_empty_chain_is_valid() {
        let result = MerkleChainHasher::verify_chain(&[]);
        assert!(result.is_valid);
        assert_eq!(result.verified_count, 0);
    }

    #[test]
    fn verify_single_event_is_valid() {
        let e0 = make_event(0, None);
        let result = MerkleChainHasher::verify_chain(&[e0]);
        assert!(result.is_valid);
        assert_eq!(result.verified_count, 1);
    }

    #[test]
    fn verify_valid_chain() {
        let e0 = make_event(0, None);
        let e1 = make_event(1, Some(&e0.event_hash));
        let e2 = make_event(2, Some(&e1.event_hash));

        let result = MerkleChainHasher::verify_chain(&[e0, e1, e2]);
        assert!(result.is_valid);
        assert_eq!(result.verified_count, 3);
        assert!(result.mismatch_at.is_none());
    }

    #[test]
    fn verify_tampered_chain_detects_mismatch() {
        let e0 = make_event(0, None);
        let e1 = make_event(1, Some(&e0.event_hash));
        let mut e2 = make_event(2, Some(&e1.event_hash));
        // Tamper with the hash
        e2.event_hash = "tampered_hash_value".into();

        let result = MerkleChainHasher::verify_chain(&[e0, e1, e2]);
        assert!(!result.is_valid);
        assert_eq!(result.mismatch_at, Some(2));
    }

    #[test]
    fn canonical_bytes_are_deterministic() {
        let tenant = Uuid::nil();
        let actor = Uuid::nil();
        let now = Utc::now();
        let payload = serde_json::json!({"key": "value"});

        let b1 = MerkleChainHasher::canonical_bytes(
            &tenant,
            "evt",
            &actor,
            "human",
            None,
            None,
            "low",
            "governance",
            &payload,
            &now,
        );
        let b2 = MerkleChainHasher::canonical_bytes(
            &tenant,
            "evt",
            &actor,
            "human",
            None,
            None,
            "low",
            "governance",
            &payload,
            &now,
        );
        assert_eq!(b1, b2);
    }
}
