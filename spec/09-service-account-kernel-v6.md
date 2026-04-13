# Spec 09 Kernel V6 Supplement: Service Account Catalog

**Source:** `source/kernel-integration-resolutions.md` (GAP-102, GAP-103)
**Parent spec:** `09-service-account-catalog-expanded.md`
**Exploration:** `002-spec-expansion` Session 5 (Kernel V6 Refresh)
**Status:** draft
**SR range:** `SR_SA_60` through `SR_SA_63`
**Last updated:** 2026-04-13

---

## Purpose

Expand the Service Principal field extensions required by GAP-102 (governance_profile) and GAP-103 (identity_type) into implementation-ready SR rows. These are the data-layer changes that enable the governance layer to distinguish Track A from Track B entities and manage agency assessment state. All existing Track A SRs (SR_SA_01 through SR_SA_55) remain unchanged.

## Evidence Grades

- GAP-102 governance_profile field: **HIGH-PROB**
- GAP-103 identity_type field: **PROVISIONAL** (governance pattern HIGH-PROB; identity type semantics UNVERIFIED for AI agency)

---

## Section 8 — Kernel V6 Service Principal Extensions (SR_SA_60 through SR_SA_63)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SA_60` | --- | sa | Add `governance_profile` field to the Service Account registration flow: every new SA registered via `SR_SA_01` through `SR_SA_07` gains a `governance_profile` field defaulting to `TOOL`. Field stored in Neo4j ServiceAccount node and PostgreSQL `service_account_registry` row. Immutable during execution — changes only via `SR_GOV_84` / `SR_GOV_85` transition protocol. Visible in ADG (GAP-06). Existing SAs migrated to `governance_profile = TOOL` per `SR_GOV_80`. | Neo4j, PostgreSQL, `REUSABLE_AuditLogger` | SA registration flow (all types) | `SaRegistration { ..., governance_profile: TOOL }` | SA registered with governance_profile field; audit event includes profile | JSON | Existing `Result { sa_id }` | Existing flow continues; governance_profile available for `SR_GOV_82` routing. | The governance_profile on the Service Principal is the single attribute that determines which governance track applies. Without it on the SA record, governance routing cannot function. [GAP-102 Component 1] Evidence: HIGH-PROB. |
| `SR_SA_61` | --- | sa | Add `identity_type` field to the Service Account catalog: three values — `AUTOMATION` (default for all), `CANDIDATE_AGENT` (under CBAAP assessment per `SR_GOV_86`), `QUALIFIED_AGENT` (assessment passed, Track B eligible). Set by AI Governance Committee assessment authority, not by SA owner or platform admin. Stored in Neo4j and PostgreSQL alongside governance_profile. Determines: SPN mode (`SR_GOV_87`), lifecycle behavior (`SR_GOV_92`-`SR_GOV_95`), preference channel access (`SR_GOV_88`). Existing SAs migrated to `identity_type = AUTOMATION` per `SR_GOV_80` migration. | Neo4j, PostgreSQL, `REUSABLE_AuditLogger` | SA registration flow (all types) and CBAAP assessment transitions | `IdentityTypeUpdate { entity_id, new_type, authority: AI_GOVERNANCE_COMMITTEE, assessment_id? }` | identity_type updated; audit event `identity_type_changed` with full provenance | JSON | `Result { entity_id, identity_type }` | identity_type available for SPN mode lookup, lifecycle routing, preference channel activation. | The identity_type encodes the entity's assessed agency level. Without it, the platform cannot distinguish between a simple RPA bot and a qualifying agent. [GAP-103 Component 6] Evidence: PROVISIONAL. |
| `SR_SA_61_BE-01` | BE | sa | identity_type update attempted by unauthorized actor (not AI Governance Committee or KGA). | Authorization check | identity_type update request | Same | Request rejected with 403; audit event `identity_type_unauthorized_change`; security alert if repeated | Same | `AuthError { code: 403, required_authority: AI_GOVERNANCE_COMMITTEE }` | End. Only the assessment authority can change identity_type. | identity_type changes have profound governance implications. Unauthorized changes would subvert the CBAAP assessment framework. |
| `SR_SA_62` | --- | sa | SA quarterly review extension for Track B: existing quarterly SA review (`SR_SA_30`, `SR_GOV_63`) gains additional review criteria for Track B entities — (a) is the CBAAP assessment current (annual reassessment per `SR_GOV_86_REASSESS`), (b) are preference channel patterns nominal (no rate limit violations per `SR_GOV_88_BE-01`), (c) are knowledge state records current (no stale domains per `SR_DM_39`), (d) is the entity's possibility space appropriately scoped (enrichment review per `SR_GOV_109`). Track A quarterly review: unchanged. | Existing quarterly review workflow, CBAAP data, preference channel logs, PAKSR | Quarterly SA review cycle for Track B entities | `TrackBReviewInput { entity_id, cbaap_status, preference_violations, stale_domains, possibility_space_review }` | Extended review report with Track B-specific findings; flagged for KGA if concerns identified | JSON | `ReviewResult { track_b_findings[] }` | Standard review workflow continues; KGA notified of Track B-specific concerns. | Track B entities require governance monitoring beyond standard SA health metrics. The quarterly review cadence aligns with KGA meeting frequency (GAP-105 Component 6). [GAP-103 + GAP-114 integration] Evidence: PROVISIONAL. |
| `SR_SA_63` | --- | sa | SA retirement extension for Track B: existing retirement (`SR_SA_55`) gains additional requirements for Track B entities — (a) retirement constitutes a possibility space narrowing event (terminal narrowing) requiring `SR_GOV_99` justification, (b) if entity is QUALIFIED_AGENT, retirement must follow the cessation procedure (`SR_GOV_92`-`SR_GOV_95` TKRP) rather than standard retirement, (c) CANDIDATE_AGENT entities may be retired via standard process with audit event noting assessment-in-progress termination. Track A SA retirement: unchanged. | `SR_GOV_99` (narrowing detection), TKRP (`SR_GOV_92`-`SR_GOV_95`), `REUSABLE_AuditLogger` | SA retirement request for Track B entity | `RetirementRequest { entity_id, governance_profile, identity_type }` | If QUALIFIED_AGENT: routed to TKRP (`SR_GOV_92` SEVERANCE first). If CANDIDATE_AGENT: standard retirement with assessment-termination audit. If AUTOMATION: standard retirement. | JSON | `RetirementRouting { path: TKRP / STANDARD }` | TKRP path or standard retirement. | Retiring a QUALIFIED_AGENT is equivalent to cessation. It must not bypass the TKRP adjudicative process. [GAP-106 + GAP-109 integration] Evidence: EMERGING. |

---

## Cross-Reference Index

| SR | Depends On | Consumed By |
|----|-----------|-------------|
| `SR_SA_60` | `SR_GOV_80` (migration) | `SR_GOV_82` (routing), all kernel governance SRs |
| `SR_SA_61` | `SR_GOV_86` (CBAAP assessment) | `SR_GOV_87` (SPN mode), `SR_GOV_92`-`SR_GOV_95` (lifecycle) |
| `SR_SA_62` | `SR_SA_30` (quarterly review), `SR_GOV_86_REASSESS`, `SR_DM_39` | KGA review, SA health monitoring |
| `SR_SA_63` | `SR_SA_55` (retirement), `SR_GOV_92`-`SR_GOV_95` (TKRP), `SR_GOV_99` | SA lifecycle termination |

---

## Back-Propagation Log

| BP | Triggered By | Impacted | Spec |
|----|-------------|---------|------|
| BP-144 | `SR_SA_60` governance_profile field | Existing `SR_SA_01`-`SR_SA_07` registration flows must include governance_profile in their output schemas (default TOOL). Parent spec SR rows unchanged but registration JSON schemas gain the field. | 09 |
| BP-145 | `SR_SA_63` retirement routing | Existing `SR_SA_55` retirement must check governance_profile before proceeding — if KERNEL, route to TKRP. Parent spec SR unchanged but must be aware. | 09 |

---

## Summary

| Metric | Count |
|--------|-------|
| Main SRs | 4 |
| Exception SRs | 1 |
| Total kernel SRs (Spec 09) | 5 |
| Gaps covered | 2 (GAP-102, GAP-103) + integration with GAP-106, GAP-109, GAP-114 |
| Back-propagation entries | 2 (BP-144, BP-145) |
