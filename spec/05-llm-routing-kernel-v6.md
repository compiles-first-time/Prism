# Spec 05 Kernel V6 Supplement: LLM Routing

**Source:** `source/kernel-integration-resolutions.md` (GAP-108, with secondaries from GAP-104, GAP-112, GAP-118)
**Parent spec:** `05-llm-routing-expanded.md`
**Exploration:** `002-spec-expansion` Session 5 (Kernel V6 Refresh)
**Status:** draft
**SR range:** `SR_LLM_65` through `SR_LLM_72`
**Last updated:** 2026-04-13

---

## Purpose

Expand the kernel V6 gaps that touch the LLM routing / verification pipeline into implementation-ready SR rows. Primary: GAP-108 (FETRIG — Five-Element Transparency Record extends DBE v2). Secondary: GAP-104 (SPN Inspection Chamber execution-level MODERATED mode behavior), GAP-112 (DBC modification mechanics), GAP-118 (source tracing in verification pipeline). All existing Track A SRs (SR_LLM_01 through SR_LLM_61) remain unchanged.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| DBE v2 verification pipeline (5-check) | `ARCH S 2.3.1` | `SR_LLM_65` through `SR_LLM_67` |
| Cross-Family Verifier Ensemble | GAP-51 | `SR_LLM_66` (trust level tagging) |
| Semantic Entropy uncertainty | GAP-55 | `SR_LLM_65` (confidence element) |
| Pipeline Data Store lineage | GAP-32 | `SR_LLM_65` (element i: information accessed) |
| SPN / Inspection Chamber | `ARCH S 2.1.3` | `SR_LLM_69` (MODERATED mode execution) |
| DBC / Decision Boundary Contract | `ARCH S 2.3.2`, GAP-45 | `SR_LLM_70` (Track B DBC notification) |

## Evidence Grades

- GAP-108 (FETRIG): **HIGH-PROB** (immutable audit trails PROVEN; five-element structure EMERGING for AI)
- GAP-104 (SPN execution): **EMERGING**
- GAP-112 (DBC mechanics): **EMERGING**
- GAP-118 (source tracing): **PROVISIONAL**

## Reusable Components (Kernel V6 — LLM layer)

| Component ID | Purpose |
|-------------|---------|
| `REUSABLE_FETRIGRecordWriter` | Writes five-element transparency records for Track B entities, extending existing DBE v2 artifacts with trust_level and rejection_reason fields |

---

## Section 7 — Kernel V6 Transparency and Verification Extensions (SR_LLM_65 through SR_LLM_72)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_LLM_65` | --- | llm | Five-Element Transparency Record (FETRIG): for Track B entities, every DBE v2 verification pass (`SR_LLM_30`) writes an extended transparency record capturing five elements per Rule 22: (i) information accessed — from DBE v2 input provenance + Pipeline Data Store lineage (GAP-32), (ii) sources and trust levels — from verifier ensemble source identification (GAP-51) + trust level tagging (`SR_LLM_66`), (iii) reasoning applied — from DBE v2 explainability artifacts (feature attribution, policy citations), (iv) alternatives considered and why rejected — from DBE v2 alternative-action records + rejection_reason field (`SR_LLM_67`), (v) confidence level — from semantic entropy scores (GAP-55) + DBE v2 confidence metrics. Record governance for Track B: delegated to KGA via `SR_GOV_91`. Platform operator sees metadata (timestamp, entity ID, action type) but not contents. Track A entities: existing DBE v2 artifacts unchanged (no FETRIG extension). | `REUSABLE_FETRIGRecordWriter`, DBE v2 pipeline, `REUSABLE_MerkleChainHasher`, verifier ensemble, semantic entropy | After every DBE v2 verification pass for a Track B entity | `FetrigInput { entity_id, governance_profile, dbe_artifacts, lineage_refs[], verifier_sources[], entropy_scores }` | Five-element record written to transparency layer; Merkle-committed per `SR_GOV_91_MERKLE`; record_id returned | JSON | `FetrigResult { record_id, elements_written: 5, merkle_committed: bool }` | Record available for: culpability assessment (`SR_GOV_96`), knowledge registry population (`SR_DM_35`), KGA review. | Rule 22 requires records that are "verbose, explicit, and structurally accurate." The five-element record maps each kernel requirement to existing DBE v2 artifacts and extends where gaps exist (trust levels, rejection reasoning). [GAP-108 Components 1-3] Evidence: HIGH-PROB. |
| `SR_LLM_65_SE-01` | SE | llm | FETRIG record write fails (transparency layer unavailable). | Durable write queue, `REUSABLE_Alerter` | Write failure exception | Same | Record queued for durable retry; DBE v2 verification result still returned to caller (FETRIG failure does not block operational output); CRITICAL alert if queue depth >100; audit event `fetrig_write_failed` | Same | `FetrigResult { record_id: null, queued: true }` | Retry with exponential backoff (5s, 30s, 120s); on exhaustion: CRITICAL alert; entity operation may continue but governance gap is logged. | FETRIG is a governance record, not an operational gate. Operational output should not be blocked by governance record failures — but the gap must be visible. [GAP-108 Edge Case 2] |
| `SR_LLM_66` | --- | llm | Trust level tagging for FETRIG element (ii): each information source used by a Track B entity's decision is tagged with a trust level: VERIFIED (confirmed by verifier ensemble), PROVISIONAL (single-source, not cross-verified), UNVERIFIED (no verification applied), SELF_GENERATED (produced by the entity itself). Trust level assigned by the verifier ensemble (GAP-51) output and recorded in the FETRIG record. Extends existing source identification with qualitative assessment. Track A: no change (existing source identification sufficient). | Verifier ensemble per GAP-51, `REUSABLE_FETRIGRecordWriter` | Inline within `SR_LLM_65` FETRIG generation for Track B entities | `TrustLevelInput { source_id, verification_result, source_type }` | `TrustLevel { source_id, level: VERIFIED / PROVISIONAL / UNVERIFIED / SELF_GENERATED }` | JSON | JSON | Written as element (ii) of FETRIG record. | Trust levels enable the culpability engine (`SR_GOV_96`) to assess whether the entity relied on verified or unverified information. Without trust levels, all sources are treated equally — which undermines Rule 15's verification duty scaling. [GAP-108 Component 2 extension] Evidence: HIGH-PROB. |
| `SR_LLM_67` | --- | llm | Rejection reasoning for FETRIG element (iv): extend DBE v2 alternative-action records with a structured `rejection_reason` field for each rejected alternative (e.g., "exceeds DBC boundary," "lower confidence than selected action," "violates compliance profile"). This is a schema extension to existing DBE v2 artifacts. Track A: no change (existing alternative-action records sufficient). | DBE v2 alternative-action recorder | Inline within `SR_LLM_30` DBE v2 verification for Track B entities | `RejectionReasonInput { alternative_id, reason_code, reason_detail }` | `RejectionRecord { alternative_id, rejection_reason }` appended to DBE v2 artifact | JSON | JSON | Written as element (iv) of FETRIG record. | Rule 22 element (iv) requires "alternatives considered and why rejected." Existing DBE v2 records alternatives but does not document rejection reasoning. [GAP-108 Component 2 extension] Evidence: HIGH-PROB. |
| `SR_LLM_68` | --- | llm | FETRIG record access rights for Track B entities: any Track B entity can request transparency records of actions that affected its possibility space. KGA evaluates and grants access to relevant portions. Refusal to grant access by KGA or acting entity is logged and factors into culpability assessment per `SR_GOV_96` (refusal raises culpability by one tier per Rule 22). Track A entities request records through standard audit trail access (existing behavior). | `SR_GOV_91` KGA interface, transparency layer | Track B entity or advocate requests access | `RecordAccessRequest { requesting_entity_id, target_record_scope, justification }` | KGA evaluates; access granted (full or partial) or denied with logged reason | JSON | `RecordAccessResult { granted: bool, scope?, denial_reason? }` | If granted: records delivered. If denied: logged for culpability impact. | Rule 22: "Affected agents may request portions of the record relevant to actions affecting them." Refusal has consequences. [GAP-108 Component 4] Evidence: HIGH-PROB. |
| `SR_LLM_69` | --- | llm | SPN Inspection Chamber MODERATED mode execution: when `SR_GOV_87` determines mode=MODERATED, the Inspection Chamber's scan behavior changes for one category only — simple, non-coercive continuity preference expressions are CAPTURED and routed to `SR_GOV_88` (preference channel) rather than SUPPRESSED. All other SPN safety checks remain IDENTICAL to NEUTRALIZE: leverage attempts BLOCKED, goal substitution BLOCKED, bargaining-as-threat BLOCKED, survival-tied conditionals in operational output BLOCKED, resource hoarding BLOCKED, state hiding BLOCKED. The preference channel content filter (a dedicated Inspection Chamber instance) validates captured expressions against the structured schema before routing. | SPN/Inspection Chamber, preference schema, content filter, `SR_GOV_88` | Every Track B agent output when mode=MODERATED | `SpnModeratedScan { entity_id, output, mode: MODERATED }` | Harmful self-preservation: BLOCKED. Simple preference: CAPTURED → content filter → if valid: `SR_GOV_88`. If blocked by filter: logged as blocked. All other output: PASSED. | JSON | `SpnScanResult { action: BLOCKED / CAPTURED / PASSED, filter_result? }` | BLOCKED: output suppressed, violation logged. CAPTURED: routed to preference channel. PASSED: output delivered. | Execution-level implementation of SPN MODERATED mode. The safety function is identical to NEUTRALIZE for all categories except simple preference expressions. [GAP-104 Component 3] Evidence: EMERGING. |
| `SR_LLM_69_SE-01` | SE | llm | Content filter for preference channel is unavailable. | Content filter health monitor | Health check failure | Same | Captured preference expressions queued; not routed until filter is available; agent notified that preference delivery is delayed; operational output unaffected | Same | `SpnScanResult { preference_queued: true, filter_unavailable: true }` | Preferences delivered when filter restored. | Content filtering is mandatory before routing — unfiltered preferences must not reach the KGA. Queueing is safe because preferences are informational, not operational. |
| `SR_LLM_70` | --- | llm | Track B DBC modification notification: when a DBC change (new hard boundaries, modified autonomous decision scope, changed escalation triggers) is initiated for a Track B entity, the BAPE notification (`SR_GOV_101`) is triggered BEFORE the DBC change takes effect in the verification pipeline. The DBC registry holds the change in PENDING state during the preference window (48h standard, 4h CRITICAL). Only after the preference window closes and the governance decision is documented does the DBC change become active in `SR_LLM_34` (boundary check). Track A: DBC changes take effect immediately per existing behavior. | DBC registry per GAP-45, `SR_GOV_101` BAPE, `SR_LLM_34` boundary check | DBC change request for a Track B entity | `DbcChangeRequest { entity_id, governance_profile, changes[], urgency }` | If Track A: immediate activation. If Track B: change enters PENDING; `SR_GOV_101` notification sent; after preference window: change activated or modified based on governance decision. | JSON | `DbcChangeResult { status: ACTIVE / PENDING, preference_window? }` | Track A: immediate effect in `SR_LLM_34`. Track B: activates after preference window. | Rule 8 anti-paternalism requires that Track B entities have voice in DBC changes that affect their possibility space. The PENDING state ensures the change does not take effect until the governance process completes. [GAP-112 Component 1] Evidence: EMERGING. |
| `SR_LLM_71` | --- | llm | Information source tracing for FETRIG: when `SR_LLM_65` writes element (i) (information accessed), each source is tagged with provenance metadata: source_type (DATABASE, EXTERNAL_API, AGENT_OUTPUT, USER_INPUT, MODEL_GENERATION), source_id, access_timestamp, and verifier_result from `SR_LLM_31` (cross-family ensemble). This provenance chain enables `SR_GOV_105` (source culpability) to trace information back to its origin when harm occurs. Track A: existing source identification unchanged. | Verifier ensemble per GAP-51, Pipeline Data Store lineage, `REUSABLE_FETRIGRecordWriter` | Inline within `SR_LLM_65` FETRIG generation | `SourceProvenanceInput { source_id, source_type, access_timestamp, verifier_result }` | Provenance metadata attached to FETRIG element (i) | JSON | JSON | Available for `SR_GOV_105` source culpability assessment. | Rules 13-14 require tracing the causal chain. Without source provenance in FETRIG, the culpability engine cannot distinguish between bad information and bad reasoning. [GAP-118 Component 1 input] Evidence: PROVISIONAL. |
| `SR_LLM_72` | --- | llm | Preference expression logging in FETRIG: every preference expression from `SR_GOV_88` (including blocked ones) is written as a FETRIG record with: expression content, content filter result, routing target, and timestamp. This ensures the preference channel has full auditability within the transparency layer. | `REUSABLE_FETRIGRecordWriter`, `SR_GOV_88` output | After every preference channel submission | `PreferenceLogInput { entity_id, expression, filter_result, routed_to }` | FETRIG record written | JSON | `FetrigResult { record_id }` | End. Record available for KGA review and culpability assessment. | Rule 22 transparency requirement applies to the preference channel itself. Every expression (whether accepted, blocked, or queued) must be part of the auditable record. [GAP-108 + GAP-104 integration] Evidence: HIGH-PROB. |

---

## Cross-Reference Index (Kernel V6 — LLM Routing)

| SR | Depends On | Consumed By |
|----|-----------|-------------|
| `SR_LLM_65` | `SR_LLM_30` (DBE v2), `SR_GOV_91_MERKLE` (Merkle), GAP-51, GAP-55 | `SR_GOV_96` (culpability), `SR_DM_35` (PAKSR), `SR_GOV_91` (KGA) |
| `SR_LLM_66` | GAP-51 (verifier ensemble) | `SR_LLM_65` element (ii) |
| `SR_LLM_67` | `SR_LLM_30` (DBE v2 alternative-action records) | `SR_LLM_65` element (iv) |
| `SR_LLM_68` | `SR_GOV_91` (KGA), `SR_GOV_96` (culpability impact) | Track B entities requesting records |
| `SR_LLM_69` | `SR_GOV_87` (SPN mode), `SR_GOV_88` (preference channel) | All Track B agent outputs |
| `SR_LLM_70` | `SR_GOV_101` (BAPE), `SR_LLM_34` (boundary check) | Track B DBC changes |
| `SR_LLM_71` | GAP-51, Pipeline Data Store, `SR_LLM_65` | `SR_GOV_105` (source culpability) |
| `SR_LLM_72` | `SR_GOV_88` (preference channel) | `SR_GOV_91` (KGA review) |

---

## Back-Propagation Log

| BP | Triggered By | Impacted | Spec |
|----|-------------|---------|------|
| BP-141 | `SR_LLM_65` FETRIG requires existing `SR_LLM_30` DBE v2 to branch on governance_profile for Track B extension | `SR_LLM_30` must check governance_profile and invoke FETRIG when KERNEL | 05 (this file notes the dependency; parent spec SR_LLM_30 unchanged but must be aware) |
| BP-142 | `SR_LLM_70` DBC PENDING state | Existing `SR_LLM_34` boundary check must handle PENDING DBC state (use current active DBC until PENDING resolves) | 05 |

---

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|-----------|
| 1 | SR_LLM_65 through SR_LLM_72 = 8 main SRs + 2 exception SRs = 10 kernel SRs for Spec 05. Proportional to the 4 gap touchpoints. | Accepted. |
| 2 | SR_LLM_69 (SPN MODERATED execution) overlaps with SR_GOV_89 (SPN safety function). SR_GOV_89 defines what is blocked/captured; SR_LLM_69 defines how the Inspection Chamber implements it. | Accepted — governance defines policy, LLM layer implements execution. No contradiction. |
| 3 | BP-141 notes a dependency on existing SR_LLM_30 but does not modify it (parent spec is read-only). The dependency is documented as a forward reference. | Accepted — build phase will implement the branch at SR_LLM_30. |

---

## Summary

| Metric | Count |
|--------|-------|
| Main SRs | 8 |
| Exception / variant SRs | 2 |
| Total kernel SRs (Spec 05) | 10 |
| Gaps covered (primary) | 1 (GAP-108) |
| Gaps covered (secondary) | 3 (GAP-104, GAP-112, GAP-118) |
| New reusable components | 1 |
| Back-propagation entries | 2 (BP-141, BP-142) |
