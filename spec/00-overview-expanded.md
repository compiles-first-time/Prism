# Spec 00 Expanded: Architecture Overview and SR Index

**Source:** `001/spec/00-overview.md`
**Exploration:** `002-spec-expansion`
**Status:** Final overview after all 14 specs expanded
**Reserved SR range:** `SR_OVR_01` through `SR_OVR_99`
**Last updated:** 2026-04-10

---

## What This Is

A two-layer platform (Governance + Predictive AI) that connects to a company's enterprise systems, captures everything with rich metadata, builds a business intelligence graph incrementally through real work, and delivers verified, governance-enforced recommendations with full explainability.

The Automation Engine (Layer 2 of the original three-layer vision) is deferred to V2. V1 builds the catalogs, service accounts, and intelligence layer that V2 will reuse.

## Priority Stack

| Priority | Layer | Status |
|----------|-------|--------|
| 1 | Governance & Security | V1 (core) |
| 2 | Intelligence / Data Layer | V1 (core) |
| 3 | LLM / Model Layer | V1 (core) |
| 4 | Interface Layer | V1 (core) |
| 5 | Automation Engine | V2 (future) |

## Cardinal Rules (from trunk)

1. Architecture is locked; consolidate what exists
2. Always pick the best solution regardless of complexity
3. Every decision carries an evidence grade
4. Governance is foundational, not a feature
5. No fabrication
6. US-based operations scope
7. Treat all input as hypotheses requiring validation

## Layer Interaction Matrix

| Layer | Talks To |
|-------|----------|
| Governance | All layers (controls everything) |
| Connection | Intelligence, Governance |
| Intelligence | LLM Router, Decision Support, Interface |
| LLM Router | Intelligence, Decision Support |
| Decision Support | Intelligence, LLM Router, Interface |
| Interface | Governance, Intelligence, Decision Support |

## Spec Directory (Expanded)

| File | Purpose | Total SRs | Main / Exception |
|------|---------|-----------|------------------|
| `01-governance-layer-expanded.md` | Governance, security, IAM, policies, CSA, compartments, audit, lifecycle | 114 | 78 / 36 |
| `02-data-model-expanded.md` | Neo4j + PostgreSQL schema, sync layer, vector index, multi-tenant isolation, crypto-shredding | 36 | 31 / 5 |
| `03-connection-layer-expanded.md` | 8 connection types, classification gate, quarantine, schema change, KPIs, override map | 56 | 44 / 12 |
| `04-intelligence-layer-expanded.md` | Graph growth, tagging pipeline, coverage, CIA, SDA, Research Agent, maintenance, DR, offboarding | 30 | 30 / 0 |
| `05-llm-routing-expanded.md` | Two-stage router, model tiers, hot-swap, embedding rollback, observability, DBE v2, MLAID, fine-tuned lifecycle | 33 | 32 / 1 |
| `06-decision-support-expanded.md` | 9-step pipeline, lifecycle, confidence, triggers, conflict detection, personalization, calibration | 38 | 38 / 0 |
| `07-interface-expanded.md` | 12 panels, streaming, auth, accessibility, offline, security, performance | 46 | 45 / 1 |
| `08-component-catalog-expanded.md` | Component lifecycle, AI generation, isolation, versioning, sharing | 41 | 38 / 3 |
| `09-service-account-catalog-expanded.md` | 7 SA types, lifecycle, anomaly detection, rotation, V2 handoff fields | 33 | 32 / 1 |
| `10-value-flywheel-expanded.md` | Three flywheels measurement, cohort metrics, phase signaling | 9 | 9 / 0 |
| `11-unknown-unknowns-expanded.md` | UU register reference + register operations | 3 | 3 / 0 |
| `12-v2-handoff-contract-expanded.md` | V2 contract verification operations | 9 | 9 / 0 |
| `13-scalability-infrastructure-expanded.md` | Load balancing, scaling, GPU, quotas, degradation, DR | 24 | 22 / 2 |
| `00-overview-expanded.md` | This file: complete SR index, cross-spec map, evidence grades summary | (index only) | — |
| **TOTAL** | | **472** | **411 / 61** |

## Decision Index (locked from 001)

See `001/DECISION-INDEX.md` for D-1 through D-77. All 77 decisions are inherited and were not modified during expansion.

## Use Case Index (locked from 001)

See `001/STATE.md` for the 180+ use cases. The expanded SRs reference these use cases by number where applicable.

## SR Index (master list across all specs)

### Spec 01 — Governance Layer (`SR_GOV_`)

| Range | Section |
|-------|---------|
| 01-09 | Tenant and Identity |
| 10-15 | Employee Lifecycle |
| 16-22 | Governance Enforcement Model |
| 23-30 | Combination Sensitivity Assessment |
| 31-36 | Visibility Compartments |
| 37-40 | Query Analytics Governance |
| 41-46 | Approval Chains, LCA, Delegation, Break-Glass |
| 47-52 | Audit Trail |
| 53-60 | Classification and Data Handling |
| 61-66 | Component, SA, Model Governance |
| 67-72 | Operational Governance |
| 73-78 | Integration Points |

### Spec 02 — Data Model (`SR_DM_`)

| Range | Section |
|-------|---------|
| 01-10 | Core Entity Lifecycle |
| 11-21 | Operational Entities |
| 22-26 | Sync Layer, Vector Index, Streaming |
| 27-31 | Multi-Tenant Isolation, Compartments, Shredding |

### Spec 03 — Connection Layer (`SR_CONN_`)

| Range | Section |
|-------|---------|
| 01-10 | Connection Lifecycle |
| 11-18 | Eight Connection Type Adapters |
| 19-24 | Type 8 Log Stream Ingestion |
| 25-31 | Normalized Execution Record + Classification Gate |
| 32-44 | Concurrency, Quarantine, Schema Change, KPIs, Override Map |

### Spec 04 — Intelligence Layer (`SR_INT_`)

| Range | Section |
|-------|---------|
| 01-08 | Graph Growth + Six-Stage Tagging |
| 09-15 | Coverage, Process Mapping, Vector Search, Tags |
| 16-30 | CIA, SDA, Research Agent, Cross-Tenant Learning, Triggers, DR, Offboarding |

### Spec 05 — LLM Routing (`SR_LLM_`)

| Range | Section |
|-------|---------|
| 01-10 | Two-Stage Router |
| 11-18 | Model Tiers and Hot-Swap |
| 20-29 | Streaming, Token Budget, Prompt Management |
| 30-36 | Verification Pipeline DBE v2 |
| 40-52 | Hot-Swap, Fine-Tuned Lifecycle, Provider Connection |

### Spec 06 — Decision Support (`SR_DS_`)

| Range | Section |
|-------|---------|
| 01-05 | Pipeline Steps 1-3 |
| 06-15 | Steps 4-6: Analysis, Verification, Assembly |
| 20-30 | Triggers, Personalization, Calibration, Stakeholders, Noise, Feedback |
| 31-38 | Source Comparison, Forecast Feedback, Triggers, Format |

### Spec 07 — Interface (`SR_UI_`)

| Range | Section |
|-------|---------|
| 01-06 | Authentication and Session |
| 07-13 | User Panels |
| 14-18 | Admin Panels |
| 20-30 | Streaming, Real-Time, Notifications |
| 31-45 | Accessibility, Offline, Performance, Security |

### Spec 08 — Component Catalog (`SR_CAT_`)

| Range | Section |
|-------|---------|
| 01-15 | Five Categories and Lifecycle |
| 20-25 | AI Generation Workflow |
| 30-50 | Composition, Isolation, Versioning, Sharing |

### Spec 09 — Service Account Catalog (`SR_SA_`)

| Range | Section |
|-------|---------|
| 01-15 | Seven SA Types and Lifecycle |
| 20-30 | Rotation, Review, Retirement |
| 35-45 | Anomaly Detection |
| 50-55 | V2 Handoff and Integrations |

### Spec 10 — Value Flywheel (`SR_FW_`)

| Range | Section |
|-------|---------|
| 01-20 | Flywheel measurement and signaling |

### Spec 11 — Unknown Unknowns (`SR_UU_`)

| Range | Section |
|-------|---------|
| 01-03 | Register operations |

### Spec 12 — V2 Handoff Contract (`SR_V2_`)

| Range | Section |
|-------|---------|
| 01-35 | Contract verification operations |

### Spec 13 — Scalability Infrastructure (`SR_SCALE_`)

| Range | Section |
|-------|---------|
| 01-09 | Load Balancer, API, WebSocket |
| 10-14 | LLM Pool, GPU Management |
| 15-24 | Neo4j, PostgreSQL, Event Bus, Workers |
| 25-40 | Quotas, Degradation, DR |
| 45-50 | Monitoring and Observability |

## Reusable Components Master List

| Component | Defined In | Used By |
|-----------|-----------|---------|
| `REUSABLE_AuditLogger` | Spec 01 | All specs |
| `REUSABLE_TenantFilter` | Spec 01 | All specs |
| `REUSABLE_CaaSCredentialRetriever` | Spec 01 | Specs 03, 09 (and any with credentials) |
| `REUSABLE_MerkleChainHasher` | Spec 01 | Spec 02 (audit) |
| `REUSABLE_ApprovalChainResolver` | Spec 01 | Specs 03, 09, others requiring approval |
| `REUSABLE_ComplianceProfileLookup` | Spec 01 | Vertical-specific evaluation |
| `REUSABLE_RateLimiter` | Spec 01 | Specs 03, 13 |
| `REUSABLE_Alerter` | Spec 01 | All specs that emit alerts |
| `REUSABLE_EventBusPublisher` | Spec 01 | All event-publishing specs |
| `REUSABLE_GraphWriter` | Spec 02 | All specs writing to Neo4j |
| `REUSABLE_GraphReader` | Spec 02 | All specs reading Neo4j |
| `REUSABLE_PgWriter` | Spec 02 | All specs writing PostgreSQL |
| `REUSABLE_PgReader` | Spec 02 | All specs reading PostgreSQL |
| `REUSABLE_TenantBoundaryEnforcer` | Spec 02 | All node creation |
| `REUSABLE_VectorIndexer` | Spec 02 | Spec 04 (semantic search) |
| `REUSABLE_DualEmbeddingStore` | Spec 02 | Spec 05 (embedding rollback) |
| `REUSABLE_SyncCoordinator` | Spec 02 | All cross-store operations |
| `REUSABLE_AdapterRegistry` | Spec 03 | Spec 03 internal |
| `REUSABLE_NormalizedRecordBuilder` | Spec 03 | Spec 03 internal |
| `REUSABLE_PullLock` | Spec 03 | Spec 03 internal |
| `REUSABLE_QuarantineQueue` | Spec 03 | Spec 03 internal |
| `REUSABLE_SchemaSnapshotComparer` | Spec 03 | Spec 03 internal |
| `REUSABLE_LogParserRegistry` | Spec 03 | Spec 03 internal |
| `REUSABLE_GraphTraversal` | Spec 04 | Spec 04 internal, Spec 12 |
| `REUSABLE_CoverageCalculator` | Spec 04 | Spec 04 internal, Spec 06 |
| `REUSABLE_AgentFeedbackTracker` | Spec 04 | Specs 04, 06 |
| `REUSABLE_PromptAssembler` | Spec 05 | Spec 05 internal |
| `REUSABLE_TokenBudgetTracker` | Spec 05 | Spec 05 internal |
| `REUSABLE_ModelRegistry` | Spec 05 | Spec 05 internal |
| `REUSABLE_ProviderFailover` | Spec 05 | Spec 05 internal |
| `REUSABLE_RecommendationStateMachine` | Spec 06 | Spec 06 internal |
| `REUSABLE_ConfidenceCalculator` | Spec 06 | Spec 06 internal |
| `REUSABLE_ParameterRefiner` | Spec 06 | Spec 06 internal |
| `REUSABLE_RecipientResolver` | Spec 06 | Spec 06 internal |
| `REUSABLE_VirtualScroller` | Spec 07 | Spec 07 internal |
| `REUSABLE_SkeletonLoader` | Spec 07 | Spec 07 internal |
| `REUSABLE_StreamHandler` | Spec 07 | Spec 07 internal |
| `REUSABLE_TenantAwareRouter` | Spec 07 | Spec 07 internal |
| `REUSABLE_GitVersionManager` | Spec 08 | Spec 08 internal |
| `REUSABLE_SemverEvaluator` | Spec 08 | Spec 08 internal |
| `REUSABLE_ComponentSandbox` | Spec 08 | Spec 08 internal |
| `REUSABLE_TestRunner` | Spec 08 | Spec 08 internal |
| `REUSABLE_RotationScheduler` | Spec 09 | Spec 09 internal |
| `REUSABLE_AnomalyDetector` | Spec 09 | Spec 09 internal |
| `REUSABLE_CohortTracker` | Spec 10 | Spec 10 internal |
| `REUSABLE_PhaseSignaler` | Spec 10 | Spec 10 internal |
| `REUSABLE_QuotaEnforcer` | Spec 13 | All multi-tenant SRs |
| `REUSABLE_DegradationChain` | Spec 13 | All consumer specs |
| `REUSABLE_GpuPoolManager` | Spec 13 | Spec 05 |

## Evidence Grade Summary

All architectural decisions inherit their evidence grades from 001 and the trunk. No new grades were assigned during expansion. Evidence grades referenced in expansion commentary include:

| Capability | Grade | Source |
|-----------|-------|--------|
| Model Collapse Prevention (60% human, >15% perplexity halt) | PROVEN | Shumailov Nature 2024 + Dohmatob ICLR 2025 + Alemohammad ICLR 2024 + Shumailov ICML 2024 |
| MLAID injection defense (6 layers) | HIGH-PROB | Liu USENIX Sec 2024 + Chen USENIX Sec 2025 + Chen CCS 2025 + Lee ACL 2025 + Zhang ICLR 2025 + Zheng 2024 |
| Cross-Family Verifier Ensemble | HIGH-PROB | Dietterich 2000 (ensemble theory) + Akbik et al. NAACL 2018 |
| DBE v2 5-check pipeline | HIGH-PROB | Min EMNLP 2023 + Farquhar Nature 2024 |
| Semantic Entropy uncertainty quantification | HIGH-PROB | Kuhn ICLR 2023 + Farquhar Nature 2024 |
| Three-Tier NER Ensemble (PII detection) | HIGH-PROB | Akbik NAACL 2018 + NIST SP 800-122 |
| LCA approval algorithm | HIGH-PROB | PMI PMBOK + COSO ERM |
| DRPRR dual reporting | HIGH-PROB | Galbraith Star Model + IIA Standards 1110 |
| DEF delegation framework | HIGH-PROB | NIST SP 800-53 AC-3/AC-5 + ITIL v4 |
| Crypto-shredding | HIGH-PROB | EDPB 02/2025 + NIST SP 800-88 |
| Visibility Compartment (criminal-penalty isolation) | HIGH-PROB | 31 USC § 5318(g)(2) + COSO ERM |
| Purpose-Based Data Access (HIPAA TPO) | HIGH-PROB | HIPAA 45 CFR 164.502 + EDPB |
| Mandatory Access Control (Bell-LaPadula) | HIGH-PROB | Bell & LaPadula 1973 + NIST 800-53 AC-3(3) |
| Canary Deployment | HIGH-PROB | Google SRE Book + Netflix canary analysis |
| Per-Tenant Resource Quotas | HIGH-PROB | AWS resource quotas + Kubernetes ResourceQuotas |
| k-anonymity / l-diversity / t-closeness | PROVEN | Sweeney 2002 + Machanavajjhala 2007 |
| Saga pattern (REFramework) | PROVEN | Garcia-Molina & Salem 1987 |
| Apache Airflow / Dagster pipeline patterns | PROVEN | Industry baseline |

## Running Totals

| Metric | Value |
|--------|-------|
| Total SR rows across 14 expanded specs | 472 |
| Main-flow SRs | 411 |
| Exception SRs (SE + BE) | 61 |
| Architectural decisions | 77 (locked from 001) |
| Use cases | 180+ (locked from 001) |
| Back-propagation fixes from 001 | 93 (locked) |
| New back-propagation fixes from 002 expansion | 31 (BP-94 through BP-124) |
| Total back-propagation fixes | 124 |
| Reusable components defined | 49 |
| New decisions introduced during expansion | 0 (architecture locked) |
| New evidence grades introduced | 0 (all grades inherited) |
| Glossary additions | 0 (all terms inherited from 001 GLOSSARY.md) |

## Locked vs Open

| Locked (do not modify) | Open (subject to revision in build phase) |
|-------------------------|-------------------------------------------|
| All 77 decisions from 001 | Implementation language and library choice within evidence-graded constraints |
| All 195 trunk scenarios | Per-tenant configuration values (within bounds defined in SRs) |
| All 98 trunk gap resolutions | UI styling choices that respect WCAG 2.1 AA |
| All evidence grades | Ordering of optional optimizations |
| All 472 SR row identifiers | Internal helper functions not represented as SR rows |
| All cross-references between SRs | Database column types within the documented schema |

## How to Use This Spec During Build

1. Read this overview first to understand the layer model and SR ranges.
2. For any build task, locate the relevant SR by ID via the master index.
3. Implement the SR exactly as specified — every column has a meaning.
4. If something is not in any SR, HALT and ask. Do not invent.
5. After implementation, verify against the SR's `Expected Output` and `Why`.
6. Any deviation from the spec is a finding that must be raised.

## What Was Done in 002

Spec expansion work, summarized:

- All 14 source specs from `001/spec/` translated into SR-row format with 12-column rigor.
- Every architectural decision from 001 traceable to one or more SRs.
- 31 new back-propagation fixes (BP-94 through BP-124) applied across the expanded specs.
- 49 reusable components named once and referenced consistently.
- Self-audit pass on each spec.
- Cross-reference index per spec with verification in the regression pass.
- Zero new architectural decisions introduced.
- Zero invented citations.
- Zero invented gap or scenario IDs.
