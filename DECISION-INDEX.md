# DECISION-INDEX — Fast Lookup for All 77 Decisions

**Purpose:** One-line summary of every architectural decision with cross-references. For fast navigation without reading all of STATE.md.
**Last updated:** 2026-04-08

---

## Foundation Decisions (D-1 through D-15)

| ID | Summary | Affected Specs |
|----|---------|---------------|
| D-1 | Neo4j (intelligence) + PostgreSQL (governance/audit) hybrid with event-driven sync | 02, 04 |
| D-2 | Incremental intelligence model — graph grows through real work, not scans | 04, 10 |
| D-3 | Priority stack: Governance > Intelligence > LLM > Interface > Automation Engine (V2) | 00, 01 |
| D-4 | Component catalog 5 categories: Universal, Protocol, Auth, Discovery, Generated | 08 |
| D-5 | No single tool covers full requirement — custom integration layer needed | 00 |
| D-6 | Tool-agnostic via RPA adapters (UiPath, AA, Blue Prism, RoboCorp, Power Automate, custom) | 03 |
| D-7 | Value at every flywheel stage — quick wins from day 1 | 10 |
| D-8 | 5 connection types (delegated user, scoped SA, privileged SA, OAuth app, RPA adapter) all through CaaS | 03 |
| D-9 | AI navigation agents for UI-only systems; direct API/DB for APIs | 03 |
| D-10 | Two-stage LLM Router: Stage 1 deterministic governance, Stage 2 AI optimizer | 05 |
| D-11 | 4 model tiers (T1/T2/T3/T-FT) + T-VERIFY, all hot-swappable | 05 |
| D-12 | PII/PHI/CUI forces on-prem models only | 05 |
| D-13 | Custom web interface with Claude as embedded intelligence engine | 07 |
| D-14 | Six-layer parameter refinement for recommendations | 06 |
| D-15 | V2 Handoff Contract: 10 component fields, 7 SA fields, intelligence requirements | 08, 09, 12 |

## Connection and Data Decisions (D-16 through D-28)

| ID | Summary | Affected Specs |
|----|---------|---------------|
| D-16 | Connection layer tracks per-customer KPIs and health metrics | 03 |
| D-17 | Query analytics tracked by user/group/role/policy (privacy-controlled) | 01, 06 |
| D-18 | Governance integrates with Microsoft Entra / Active Directory (plus Okta, Google, SCIM) | 01 |
| D-19 | Rejection justification contract — structured, relevant, no empty rejections | 06 |
| D-20 | Governance: ENFORCE for security/compliance, ADVISE for operational | 01 |
| D-21 | IAM conflict resolution: platform wins for platform resources, Entra wins for identity | 01 |
| D-22 | Audit: event-sourced append-only with Merkle hash chain (no blockchain needed) | 01, 02 |
| D-23 | DataField enriched: semantic_type, co_collected_with, dwell_time, CHANGES_WITH edges | 02 |
| D-24 | DataSnapshot nodes for temporal versioning and trend analysis | 02 |
| D-25 | DataQualityReport as first-class entity | 02 |
| D-26 | Real-time streaming via Redis Streams + Socket.io, governance-filtered events | 02, 07 |
| D-27 | Confidence property on ALL relationship edges (human-confirmed vs AI-inferred) | 02 |
| D-28 | LLM Observability Layer: ModelExecution, ModelOutcomeScore, model_performance_analytics | 02, 05 |

## Governance and Security Decisions (D-29 through D-43)

| ID | Summary | Affected Specs |
|----|---------|---------------|
| D-29 | Model hot-swap is a privileged governance action with whitelist/blacklist per tenant | 01, 05 |
| D-30 | Full scalability architecture: load balancer → stateless API → WebSocket → event bus → pools | 13 |
| D-31 | Architectural regression testing (back-propagation) on every new spec | Process |
| D-32 | Pull lock per connection+scope (not per system); internal pipeline supports parallel ingestion via MERGE | 03 |
| D-33 | Embedding model rollback via dual storage during canary periods | 02, 05 |
| D-34 | Combination Sensitivity Assessment (CSA) — mosaic effect prevention | 01, 06 |
| D-35 | Employee permission lifecycle: new hire, promotion, lateral, departure, temporary elevation | 01 |
| D-36 | Quarantine timeout configurable per connection/system/classification with expiry actions | 03 |
| D-37 | Platform-managed identity mode for tenants without external IAM | 01 |
| D-38 | Prompt management system with versioned templates and immutable safety guardrails | 05 |
| D-39 | Multi-model query decomposition for complex queries | 05 |
| D-40 | Two-mode streaming: verified (recommendations) and live (conversational) | 05, 07 |
| D-41 | Token budget management: platform → tenant → query → user levels with graceful degradation | 05 |
| D-42 | Fine-tuned model lifecycle: need → data → training → validation → canary → monitoring → retraining | 05 |
| D-43 | Cloud LLM providers tracked as Connection nodes with deprecation monitoring | 03, 05 |

## Multi-Vertical Decisions (D-44 through D-58)

| ID | Summary | Affected Specs |
|----|---------|---------------|
| D-44 | External system predictions (Ekos, Einstein) ingested as data, compared with platform predictions | 06 |
| D-45 | Forecast component feedback with validation (per-component, quality-scored) | 06 |
| D-46 | Research Agent for external context (market, news, regulatory, commodity) | 04, 06 |
| D-47 | Cascade Impact Analysis (CIA) named feature for upstream/downstream/lateral/second-order | 04, 06 |
| D-48 | DataGroup node for logical groupings across DataCollections | 02 |
| D-49 | Tag weighting (security 1.0 > business 0.7 > technical 0.5) configurable per tenant | 02 |
| D-50 | Completeness tags on DataCollections with missing_fields[] | 02 |
| D-51 | Agent Performance Feedback Loop — backpropagation pattern for all agents | 04 |
| D-52 | Configurable evaluation metrics weighted and AI-monitored | 06 |
| D-53 | Semantic Disambiguation Agent (SDA) for cross-system/user/format synonyms | 04 |
| D-54 | Paywalled API policy: never unauthorized scraping, decision tree with customer consent | 03 |
| D-55 | Event Calendar for manually logged business events as forecast regressors | 06 |
| D-56 | Recommendation accuracy tracking on DataCollections (track record) | 02, 06 |
| D-57 | Dashboard relevance tags on DataCollections (AI-inferred + confirmed) | 02, 07 |
| D-58 | Connection Type 8: Log Stream Adapter for existing processes and shadow IT | 03 |

## Decision Support and Interface (D-59 through D-77)

| ID | Summary | Affected Specs |
|----|---------|---------------|
| D-59 | Recommendation lifecycle (12 states) with confidence threshold (<0.40 = decline) and expiration | 06 |
| D-60 | Eight proactive recommendation trigger types | 04, 06 |
| D-61 | Recipient-centric recommendation delivery with per-user preferences | 06 |
| D-62 | Recommendation dependencies, chains, cooling-off periods | 06 |
| D-63 | Confidence score calibration monitoring with weight adjustment | 06 |
| D-64 | Multi-stakeholder recommendations with per-stakeholder tracking | 06 |
| D-65 | Recommendation noise management (rate limits, batching, pattern detection) | 06 |
| D-66 | Interface: React+Next.js+TypeScript+shadcn/ui+Socket.io, 12-panel layout | 07 |
| D-67 | WCAG 2.1 AA accessibility compliance (non-negotiable) | 07 |
| D-68 | Frontend security: httpOnly cookies, CSRF, CSP, XSS sanitization, device fingerprinting | 07 |
| D-69 | Offline mode via IndexedDB caching (read-only, queue actions) | 07 |
| D-70 | Virtual scrolling for all long lists | 07 |
| D-71 | Component catalog complete schema with semantic versioning and multi-version coexistence | 08 |
| D-72 | AI component generation workflow with multi-layer validation | 08 |
| D-73 | Component isolation and resource limits (CPU/memory/network/IO/timeout) | 08 |
| D-74 | Component fast rollback (<5 min, all versions retained 30 days) | 08 |
| D-75 | Service Account Catalog with full schema, 7 types, lifecycle, anomaly detection | 09 |
| D-76 | Three flywheels (Data, Organizational, Model) operating simultaneously | 10 |
| D-77 | Seven-phase value delivery timeline with quick wins from week 1 | 10 |

---

## Decisions by Affected Spec

### Spec 00 — Overview
D-3, D-5

### Spec 01 — Governance Layer
D-3, D-17, D-18, D-20, D-21, D-22, D-29, D-34, D-35, D-37

### Spec 02 — Data Model
D-1, D-22, D-23, D-24, D-25, D-26, D-27, D-28, D-33, D-48, D-49, D-50, D-56, D-57

### Spec 03 — Connection Layer
D-6, D-8, D-9, D-16, D-32, D-36, D-43, D-54, D-58

### Spec 04 — Intelligence Layer
D-1, D-2, D-46, D-47, D-51, D-53, D-60

### Spec 05 — LLM Routing
D-10, D-11, D-12, D-28, D-29, D-33, D-38, D-39, D-40, D-41, D-42, D-43

### Spec 06 — Decision Support
D-14, D-17, D-19, D-34, D-44, D-45, D-46, D-47, D-52, D-55, D-56, D-59, D-60, D-61, D-62, D-63, D-64, D-65

### Spec 07 — Interface
D-13, D-26, D-40, D-57, D-66, D-67, D-68, D-69, D-70

### Spec 08 — Component Catalog
D-4, D-15, D-71, D-72, D-73, D-74

### Spec 09 — Service Account Catalog
D-15, D-75

### Spec 10 — Value Flywheel
D-2, D-7, D-76, D-77

### Spec 11 — Unknown Unknowns
(Register of UU-1 through UU-13 with responses)

### Spec 12 — V2 Handoff Contract
D-15

### Spec 13 — Scalability Infrastructure
D-30

---

## Decisions by Priority/Category

### CRITICAL (must-have for V1)
D-1, D-2, D-3, D-6, D-10, D-11, D-12, D-13, D-14, D-15, D-18, D-20, D-22, D-28, D-34, D-37, D-58, D-59, D-66, D-67, D-68, D-71

### HIGH (strongly recommended for V1)
Most other decisions — see individual entries in STATE.md

### MEDIUM (good to have, can defer some)
D-48, D-49, D-50, D-55, D-56, D-57, D-64

---

## How to Use This Index

When you need to find information about a decision:
1. Scan this index for the relevant keyword
2. Note the D-ID and affected specs
3. Read the full decision rationale in STATE.md
4. Read the affected spec files for implementation details
5. Check the GLOSSARY for any unfamiliar terms
