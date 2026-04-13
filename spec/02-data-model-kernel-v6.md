# Spec 02 Kernel V6 Supplement: Data Model

**Source:** `source/kernel-integration-resolutions.md` (GAP-114)
**Parent spec:** `02-data-model-expanded.md`
**Exploration:** `002-spec-expansion` Session 5 (Kernel V6 Refresh)
**Status:** draft
**SR range:** `SR_DM_35` through `SR_DM_39`
**Last updated:** 2026-04-13

---

## Purpose

Expand GAP-114 (Per-Agent Knowledge State Registry / PAKSR) into implementation-ready SR rows. The PAKSR is the persistent data store that tracks each Track B entity's knowledge state — what it knew, when, how often, at what stakes. This is the data model that enables knowledge lock-in (`SR_GOV_96_LOCKIN`), reconciliation (`SR_GOV_103`), and culpability assessment (`SR_GOV_96`). All existing Track A SRs (SR_DM_01 through SR_DM_31) remain unchanged.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| FETRIG transparency records | `ARCH S 2.3.1` (extended by `SR_LLM_65`) | `SR_DM_36` (automatic population) |
| RTVP verification profiles | GAP-52 | `SR_DM_35` (stakes level recording) |
| Crypto-shredding | `COMP S 3.3.3` | `SR_DM_38` (erasure with anonymized governance retention) |

## Evidence Grades

- GAP-114 (PAKSR): **PROVISIONAL** (medical recertification precedent HIGH-PROB; per-agent AI knowledge registry UNVERIFIED; Kernel Structural Gap #6 acknowledges computational expense)

---

## Section 6 — Kernel V6 Per-Agent Knowledge State Registry (SR_DM_35 through SR_DM_39)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DM_35` | --- | data-model | PAKSR record structure: for each Track B entity, maintain records indexed by knowledge domain (e.g., "BSA/AML regulations," "patient X treatment history," "lending risk factors"). Each record: (a) what the agent knew (summary from FETRIG records), (b) when accessed (timestamp range), (c) how often (access frequency), (d) at what stakes (RTVP tier in effect per GAP-52), (e) whether superseded by newer information. Storage: PostgreSQL table `knowledge_state_registry` with columns: entity_id, domain, knowledge_summary, first_accessed, last_accessed, access_count, stakes_tier, superseded_by, substrate_version. Indexed by (entity_id, domain) for fast lookup. Tenant-scoped via `REUSABLE_TenantFilter`. Governed by KGA for Track B records (`SR_GOV_91`). | `REUSABLE_PgWriter`, `REUSABLE_TenantFilter`, `REUSABLE_KGA_Interface` | Schema migration at kernel V6 deployment | `PaksrSchemaDefinition` | Table created with indexes; governed access established | DDL | `MigrationResult` | Available for `SR_DM_36` population. | Rule 23 requires a permanent external record of all knowledge an agent has held. The PAKSR is that record. Without it, knowledge lock-in cannot be enforced, culpability assessment cannot determine knowledge-at-time-of-action, and reconciliation cannot detect stale knowledge. [GAP-114 Components 1] Evidence: PROVISIONAL. |
| `SR_DM_36` | --- | data-model | PAKSR automatic population: after each FETRIG record (`SR_LLM_65`) is written for a Track B entity, extract knowledge referenced in the five-element record and upsert into PAKSR. If domain already exists: update last_accessed, increment access_count, update stakes_tier if higher. If new domain: insert new record. Post-processing step on FETRIG records — no new data collection required. Domain granularity: configurable per-tenant, default is "regulatory domain" level (e.g., "BSA/AML" not "individual transaction X"). | `SR_LLM_65` FETRIG output, `REUSABLE_PgWriter` | After every FETRIG record write | `PaksrPopulationInput { entity_id, fetrig_record, domain_classifier }` | PAKSR record upserted; access_count incremented; audit event `paksr_updated` | JSON | `PaksrResult { domain, access_count, stakes_tier }` | Available for `SR_GOV_96_LOCKIN` (knowledge lock-in) and `SR_GOV_103` (reconciliation). | Automatic population eliminates manual knowledge tracking. The FETRIG record already contains the information — PAKSR extracts and indexes it by domain. [GAP-114 Component 2] Evidence: PROVISIONAL. |
| `SR_DM_36_SE-01` | SE | data-model | PAKSR write fails (PostgreSQL unavailable). | Durable retry queue, `REUSABLE_Alerter` | Write failure exception | Same | FETRIG record still committed (transparency is primary); PAKSR write queued for retry; alert if queue depth >50 | Same | `PaksrResult { status: queued }` | Retry with backoff (5s, 30s, 120s). On exhaustion: CRITICAL alert; governance gap logged. | PAKSR population failure does not block operational output — but the governance gap (incomplete knowledge records) must be visible for culpability assessment. |
| `SR_DM_37` | --- | data-model | PAKSR point-in-time knowledge query: given an entity_id, domain, and timestamp, return the entity's knowledge state at that time. Used by culpability engine (`SR_GOV_96`) to determine "what did the entity know when it acted?" Query uses last_accessed <= timestamp to find the most recent record before the action. If no record exists for the domain at that time: entity had no recorded knowledge (supports Tier-1/Tier-2 classification). | `REUSABLE_PgReader`, `REUSABLE_TenantFilter` | Culpability assessment query | `PaksrQueryInput { entity_id, domain, as_of_timestamp }` | `PaksrQueryResult { knowledge_summary?, access_count, stakes_tier, last_accessed? }` or NULL if no record | JSON | JSON | Result fed to `SR_GOV_96` culpability classification. | The definitive record for knowledge-at-time-of-action. Without point-in-time query, culpability assessment cannot distinguish "knew and ignored" from "did not know." [GAP-114 Component 4 integration with GAP-107] Evidence: PROVISIONAL. |
| `SR_DM_38` | --- | data-model | PAKSR substrate reset handling: when a Track B entity undergoes substrate reset (model replacement, significant retraining, platform migration), all knowledge domains are marked `superseded_by: 'substrate_reset_{timestamp}'` for the new substrate instance. Entity must reconcile in each domain before resuming high-stakes operations per `SR_GOV_103`. Self-induced resets to evade prior knowledge: detected by comparing reset frequency to expected operational patterns; flagged as Tier-4/Tier-5 violation per `SR_GOV_96`. Erasure requests: standard crypto-shredding applies; entity's personal data is shredded but anonymized governance records (knowledge domains without content) are retained per extended retention policy. | `REUSABLE_PgWriter`, crypto-shredding per `COMP S 3.3.3`, `REUSABLE_AuditLogger` | Substrate reset event | `SubstrateResetInput { entity_id, reset_type, initiator, new_substrate_version }` | All PAKSR records for entity marked non-current; audit event `paksr_substrate_reset`; evasion pattern check triggered | JSON | `ResetResult { domains_marked_non_current, evasion_flag: bool }` | Entity must reconcile per `SR_GOV_103` before high-stakes action. | Substrate resets erase learned knowledge — the PAKSR preserves the record of what was known, enabling reconciliation and preventing reset-as-evasion. [GAP-114 Component 5] Evidence: PROVISIONAL. |
| `SR_DM_39` | --- | data-model | PAKSR staleness detection: periodic job (daily) checks each Track B entity's knowledge domains against external knowledge freshness indicators. If external knowledge in a domain has materially changed since the entity's last access (determined by domain-specific freshness heuristics — e.g., regulatory updates in BSA/AML domain, new clinical guidelines in healthcare), the domain is flagged as `stale`. Stale domains trigger reconciliation check on next high-stakes action per `SR_GOV_103`. | Staleness detector, domain freshness feeds, `REUSABLE_PgWriter` | Daily scheduled job | `StalenessCheckInput { entity_id, domains[] }` | Stale domains flagged; `paksr_staleness_detected` event written | JSON | `StalenessResult { stale_domains[] }` | Entity informed via preference channel (if MODERATED); reconciliation required on next high-stakes action. | Knowledge becomes outdated over time. Staleness detection ensures entities do not act on knowledge that has been superseded by new information. [GAP-114 Edge Case 2] Evidence: PROVISIONAL. |

---

## Cross-Reference Index

| SR | Depends On | Consumed By |
|----|-----------|-------------|
| `SR_DM_35` | Schema migration | `SR_DM_36`, `SR_DM_37`, `SR_DM_38`, `SR_DM_39` |
| `SR_DM_36` | `SR_LLM_65` (FETRIG records) | `SR_GOV_96_LOCKIN`, `SR_GOV_103` |
| `SR_DM_37` | `SR_DM_35` (registry data) | `SR_GOV_96` (culpability assessment) |
| `SR_DM_38` | Substrate reset events | `SR_GOV_103` (reconciliation), `SR_GOV_96` (evasion detection) |
| `SR_DM_39` | Domain freshness feeds | `SR_GOV_103` (reconciliation trigger) |

---

## Back-Propagation Log

| BP | Triggered By | Impacted | Spec |
|----|-------------|---------|------|
| BP-143 | `SR_DM_36` automatic population from FETRIG | `SR_LLM_65` must include a post-write hook to trigger PAKSR population | 05 |

---

## Summary

| Metric | Count |
|--------|-------|
| Main SRs | 5 |
| Exception SRs | 1 |
| Total kernel SRs (Spec 02) | 6 |
| Gaps covered (primary) | 1 (GAP-114) |
| New reusable components | 0 (uses existing REUSABLE_PgWriter, REUSABLE_TenantFilter) |
| Back-propagation entries | 1 (BP-143) |
