# Spec 12 Expanded: V2 Handoff Contract

**Source:** `001/spec/12-v2-handoff-contract.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Reserved SR range:** `SR_V2_01` through `SR_V2_99`
**Last updated:** 2026-04-10

---

## Purpose

Spec 12 is a contract document, not an operational pipeline. The SR rows here describe (a) the contract verification operations that prove the handoff fields are populated and the graph is queryable, and (b) the build-time guards that prevent V1 from emitting components or service accounts without the required V2 fields. V2 itself is out of scope; V1 must guarantee the substrate.

## Architectural Decisions Covered

D-15 (V2 handoff contract).

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| LCA approval algorithm | `FOUND § 1.4.1` | `SR_V2_15` (verification test) |
| Audit trail | GAP-71 | `SR_V2_18` (verification test) |

## Integration Map

| Consumer | Depends On |
|----------|------------|
| Spec 08 Component Catalog | `SR_V2_05` (component field validation gate) |
| Spec 09 SA Catalog | `SR_V2_10` (SA field validation gate) |
| Spec 04 Intelligence | `SR_V2_15` (graph queryability test) |

## SR Rows

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_V2_01` | --- | v2 | Component handoff field validation: every component_registry row must include `is_idempotent`, `expected_duration_ms`, `error_modes[]`, `rollback_procedure`, `credential_requirements`, `sequence_position`, `input_schema`, `output_schema`, `side_effects[]`, `concurrency_safe`. | Schema validator | At component activation per `SR_CAT_15` | `ComponentFieldsCheckInput` | Pass / fail with missing fields | JSON | `Result` | If pass: activation. Else: blocked. | V2 cannot plan automations without these fields; the validation gate prevents incomplete components from reaching active status. |
| `SR_V2_05` | --- | v2 | Component field auto-population helpers: where the platform can infer fields, AI-assisted defaults are proposed and human-approved. | T2 LLM, validators | At component creation | `AutofillInput` | Proposed defaults with confidence | JSON | `Result` | Owner reviews. | Reduces friction for component owners. |
| `SR_V2_10` | --- | v2 | Service account handoff field validation: `systems_accessible[]`, `permission_level`, `permission_scope`, `rate_limits`, `concurrent_sessions`, `rotation_schedule`, `shared_by_components[]`, `automation_eligible`. | Schema validator | At SA activation per `SR_SA_13` | `SaFieldsCheckInput` | Pass / fail | JSON | `Result` | If pass: activation. Else: blocked. | V2 cannot reuse SAs without these fields. |
| `SR_V2_15` | --- | v2 | Intelligence Layer queryability test: verify that V2-required graph queries work — process dependency graph (FEEDS), data flow direction + volume (SOURCED_FROM, FEEDS with volume), failure propagation paths (IMPACTS with blast_radius), historical execution timing (audit timestamps), process-to-department mapping (BELONGS_TO_DEPT), component composition patterns (DEPENDS_ON). | `REUSABLE_GraphTraversal` | Periodic verification | `QueryabilityTestInput` | All test queries return expected shape | JSON | `Result { passed_tests, failed_tests }` | If fail: alert; block V2 readiness flag. | The contract is verified independently of any V2 build. |
| `SR_V2_18` | --- | v2 | Audit integration test: confirm that hypothetical V2 actions would appear in the audit trail with the same chain integrity as V1 actions. | `SR_GOV_47` (synthetic invocation) | Periodic verification | `AuditIntegrationTestInput` | Pass / fail | JSON | `Result` | End | Audit must remain unbroken across the V1/V2 boundary. |
| `SR_V2_20` | --- | v2 | Multi-tenant isolation test: confirm that hypothetical V2 actions respect tenant boundaries. | Synthetic test, `SR_DM_28` | Periodic | `IsolationTestInput` | Pass / fail | JSON | `Result` | End | Isolation cannot break across the V1/V2 boundary. |
| `SR_V2_25` | --- | v2 | Blast radius computation test: confirm that V2 can compute blast radius from the graph using the IMPACTS edges. | `SR_INT_16` | Periodic | `BlastRadiusTestInput` | Pass / fail | JSON | `Result` | End | Approval level depends on blast radius; the computation must be consistent. |
| `SR_V2_30` | --- | v2 | V1 readiness gate for V2 build: gate that confirms (a) V1 is stable in production at least 3 months, (b) at least 10 active tenants, (c) catalogs have accumulated enough content, (d) intelligence layer is mature, (e) governance is battle-tested. | Readiness checker | Quarterly review | `V1ReadinessInput` | Ready / not-ready with rationale | JSON | `Result` | If ready: V2 development may begin. | Bounds the V2 timeline against operational maturity, not project optimism. |
| `SR_V2_35` | --- | v2 | Catalog completeness audit: scan all components and SAs for missing V2 fields; produce a remediation list. | Catalog scanner | Monthly | `CatalogAuditInput` | List of incomplete entries | JSON | `Result` | Owners remediate. | Ongoing visibility into the contract substrate. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-123 | `SR_V2_01` field validation gate | `SR_CAT_15` (component approval) | Confirmed: `SR_CAT_15` invokes `SR_V2_01` as a precondition. | 1 |
| BP-124 | `SR_V2_10` SA field validation gate | `SR_SA_13` (SA activation) | Confirmed: `SR_SA_13` invokes `SR_V2_10` as a precondition. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | The contract is validated by hypothetical V2 actions; what if V1 changes break the contract later? | The verification SRs run periodically and would catch any regression before V2 is built. |
| 2 | `SR_V2_30` readiness gate — who decides on the soft criteria like "battle-tested"? | Platform leadership + customer success consultation. Documented as a quarterly review. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_V2_01` | `SR_CAT_15` | Field validation gate |
| `SR_V2_10` | `SR_SA_13` | SA field validation |
| `SR_V2_15` | `SR_INT_*` | Graph queryability tests |
| `SR_V2_18` | `SR_GOV_47` | Audit integration |
| `SR_V2_25` | `SR_INT_16` | CIA / blast radius |

## Spec 12 Summary

| Metric | Value |
|--------|-------|
| Main-flow SRs | 9 |
| Exception SRs | 0 |
| Total SR rows | 9 |
| BP entries created | 2 (BP-123, BP-124) |
| New decisions | 0 |

**Status:** Self-audit complete.
