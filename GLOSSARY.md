# GLOSSARY — All Named Concepts

**All acronyms, named features, and terms used in this exploration in one place.**
**Last updated:** 2026-04-08

---

## How to Use This File

- Every term used in specs, decisions, or code must be defined here
- If you encounter a term you do not know, check here first
- If you introduce a new term, add it here
- No terminology drift — use the exact term as defined

---

## Platform Architecture Terms

### Three-Layer Model (Deferred)
Original vision: Governance (Layer 1) + Automation Engine (Layer 2) + Predictive AI (Layer 3). V1 implements Layers 1 and 3. Layer 2 deferred to V2.

### Two-Layer V1
Current scope: Governance + Predictive AI. The automation engine is deferred but V1 catalogs (components, service accounts) are designed to support V2 integration without rework.

### Incremental Intelligence
The platform's intelligence layer grows organically from real work (automations, dashboards, data pulls, pipelines), not from automated discovery scans. See D-2.

### Self-Configuring Platform
AI-assisted discovery and classification rather than manual per-vertical configuration. Platform ships with discovery agents that learn each tenant's specific systems and data.

### Incremental Access
The platform never has 100% access to any company's systems. Access is granted one system at a time through three patterns: delegated user credentials, scoped service accounts, or privileged service accounts.

### Tool-Agnostic
Platform works with any automation tool (UiPath, Automation Anywhere, Blue Prism, RoboCorp, Power Automate, custom scripts) via RPA adapters. See D-6.

---

## Governance Terms

### Governance Layer
Priority 1 layer. Controls who can do what to which data under what circumstances. Enforces rules across all other layers.

### CSA — Combination Sensitivity Assessment
D-34. Runs before Decision Support combines data from multiple DataCollections. Detects the "mosaic effect" where individually low-sensitivity data becomes high-sensitivity when combined. Can block, anonymize, or elevate permission requirements.

### ENFORCE vs ADVISE (D-20)
Governance enforcement model:
- **ENFORCE** (non-overridable): Security/compliance rules (PII/PHI/CUI, compartments, tenant isolation)
- **ADVISE** (overridable with justification): Operational rules (freshness, confidence thresholds, parameter weights)

### Visibility Compartment
From trunk COMP § 3.1.1. Compliance profile attribute overriding default "visibility flows up" with explicit membership. Used for criminal-penalty data, BSA/AML, healthcare substance abuse, government classification.

### LCA — Lowest Common Ancestor
From trunk FOUND § 1.4.1. Algorithm determining approval escalation point in org hierarchy — the first node that is parent of ALL participating teams.

### DEF — Delegation and Escalation Framework
From trunk. Time-boxed approval delegation with SLA-based escalation (5d/2d/24h/4h configurable tiers).

### DRPRR — Dual-Reporting Path Resolution Rules
From trunk FOUND § 1.4.2. Deterministic rules for selecting which reporting line governs when an employee has multiple managers.

### Break-Glass (GAP-49)
From trunk. Emergency override mechanism with two-person activation, time-boxed access, full audit, retroactive approval requirement.

### REPW — Regulatory Examiner Provisioning Workflow
From trunk GAP-80. Time-boxed, read-only provisioning for regulatory examiners.

---

## Identity and Access Terms

### IAM — Identity and Access Management
External identity providers: Microsoft Entra ID / Azure AD, Okta, Google Workspace, Generic SCIM. Platform integrates with IAM systems; never owns identity for IAM-synced tenants.

### Platform-Managed Identity (D-37)
Standalone mode for tenants without external IAM. Platform handles user creation (email invitations), authentication (password+MFA or magic link), role assignment (admin-managed), deprovisioning (manual + inactivity timeout). Same governance model as IAM-synced tenants.

### CaaS — Credential-as-a-Service
From trunk GAP-14. Centralized credential management with per-step JIT activation and protocol adapter layer. Stores encrypted credentials, never exposes actual secrets.

### Service Principal
From trunk FOUND § 1.3.1. First-class automation identity distinct from human creator. Scoped permissions, JIT activation, creator-independent.

### JIT — Just-In-Time (credential activation)
Credentials are activated only when needed and deactivated immediately after. Cache lifetime bounded by state execution.

---

## Data and Metadata Terms

### DataCollection
Neo4j node representing a batch of related data pulled from a system. Contains DataField nodes via CONTAINS_FIELD edges.

### DataField
Neo4j node representing an individual field/column within a collection. Contains properties like technical_type, semantic_type, classification, freshness_policy, dwell_time_avg_ms.

### DataSnapshot (D-24)
Timestamped version of a DataCollection. Retained per freshness policy. Enables temporal analysis and "what did this look like N months ago?" queries.

### DataQualityReport (D-25)
Neo4j node assessing completeness, consistency, timeliness, uniqueness, and overall quality score for a DataCollection.

### DataGroup (D-48)
Logical groupings across DataCollections (e.g., "all procurement data"). User-defined or AI-suggested. MEMBER_OF edges from DataCollection to DataGroup.

### DataRetentionPolicy (BP-13)
Node defining retention period, snapshot frequency, archive after, delete after for scoped data.

### Tag
Neo4j node representing a metadata classification. Categories: security, business, technical, process. Has weight per tenant configuration.

### Three-Tier NER Ensemble (GAP-66)
Pattern + statistical (spaCy) + transformer (Presidio/BERT) with OR-logic voting. Target: <2% false negative rate for PII detection.

### Contextual PII Classification
From trunk. PII classification that accounts for context, not just format. A "name" field in a contact database is PII; a "name" field in a public company registry may not be.

### RRAP — Re-identification Risk Assessment Pipeline
From trunk. k-anonymity / l-diversity / t-closeness assessment for combined agent outputs. Triggered at 3+ agents.

---

## Intelligence Layer Terms

### Intelligence Layer
Priority 2. The Neo4j graph that stores all relationships, processes, data flows, and enables analytical queries. Grows incrementally through work.

### Process Mapping
The emergent business process graph built from real work. Processes are not manually defined — they emerge from tagging, components, and user interactions.

### Coverage
The percentage of a business the platform has mapped. Dimensions: system coverage, process coverage, data coverage, department coverage, relationship coverage. Every recommendation discloses coverage.

### Cascade Impact Analysis (CIA) (D-47)
Named feature for tracing upstream, downstream, lateral, and second-order effects of any change or event through the intelligence graph. Uses IMPACTS edges.

### SDA — Semantic Disambiguation Agent (D-53)
Discovers synonyms, shorthand, and acronyms across systems and users. Cross-system (product vs item), cross-user (emp vs employee), cross-format (date/currency formats).

### Research Agent (D-46)
AI agent that periodically gathers external context: market data, news, regulatory, commodity prices, weather. External DataCollections with data_origin: "research_agent".

### Event Calendar (D-55)
Business owners log upcoming events (festivals, promotions, seasonal events) with expected business impact. Used as regressors in forecasting.

### SchemaChangeEvent (BP-10)
Neo4j node created when Connection Layer detects a schema change in an external system. Intelligence Layer subscribes and updates affected edges.

### CorrelationTrace (BP-62)
Neo4j node representing a multi-system transaction traced through logs. Enables visibility into distributed transactions.

### TrendAnalysis
Neo4j node computed from DataSnapshots showing direction, magnitude, and statistical significance of metric changes over time.

---

## AI and Verification Terms

### DBE v2 — Decision Boundary Enforcer v2
From trunk. 5-check sequential verification pipeline for all AI output: boundary check, factual check (MiniCheck/FActScore), semantic entropy, explainability, safety.

### DBC — Decision Boundary Contract
From trunk ARCH § 2.3.2. Per-agent formal specification of autonomous decisions, escalation triggers, hard boundaries, explainability requirements.

### Semantic Entropy (SE)
Uncertainty quantification via clustering semantically equivalent responses. k=5 default, k=10 high-stakes. AUROC ≥ 0.79 target.

### Cross-Family Verifier Ensemble (GAP-51)
3+ verifiers from independent model families with consensus rules (≥2 of 3 agree).

### RTVP — Risk-Tiered Verification Profiles
5 verification intensity tiers from REAL_TIME_SAFETY (<200ms) to OFFLINE_BATCH (no SLA).

### MLAID — Multi-Layer Adaptive Injection Defense (GAP-61)
6-layer injection defense: static patterns, per-agent airlock, behavioral, embedding, LLM-as-judge, steganographic.

### Model Collapse Prevention (GAP-44)
Training controls: 60% human data minimum, >15% perplexity divergence = halt, max 20% verified synthetic.

### ModelExecution (D-28)
Neo4j node recording every LLM invocation: model, slot, task, tokens, latency, cost, data sensitivity.

### ModelOutcomeScore (D-28)
Neo4j node recording quality outcome of a ModelExecution (acceptance, rejection, correction).

### Model Tiers (D-11)
- T1: Small local (<100ms)
- T2: Medium local/cloud (<1s)
- T3: Large cloud (2-10s)
- T-FT: Fine-tuned domain
- T-VERIFY: Cross-family ensemble for DBE v2

### Two-Stage Router (D-10)
LLM routing architecture: Stage 1 is deterministic governance rules (non-overridable), Stage 2 is AI optimizer selecting best model from allowed tiers.

### Blind Executor / Status-Only Return
From trunk. Execution pattern where AI sees only success/failure + metadata; never raw data, credentials, or execution results.

### Dual-Gate Airlock
From trunk. Two independent validation gates between AI agents and execution engine, and between every pair of agents in a multi-agent pipeline.

### SPN — Self-Preservation Neutralization
From trunk. Defense against AI agents exhibiting self-preservation behaviors.

---

## Connection Layer Terms

### Connection Type 1: Delegated User Credentials
Employee provides personal login. Most restrictive scope. Revocable.

### Connection Type 2: Scoped Service Account
Customer-created account mirroring a role. Medium scope.

### Connection Type 3: Privileged Service Account
Admin/skeleton key access. Highest security tier. Dual-approval, time-boxed.

### Connection Type 4: Application Integration (OAuth/API)
OAuth app or API key with vendor. Standard SaaS integration.

### Connection Type 5: RPA Adapter
Connects to RPA tool orchestrator, extracts execution records. Tool-agnostic.

### Connection Type 5.5: AI Navigation Agent
For systems with UI only. Governance-required authorization.

### Connection Type 6: Bulk Import (BP-06)
Admin-initiated large-scale data migration. Dedicated queue.

### Connection Type 7: User Upload (BP-42)
Individual files from employees. Captures tribal knowledge and shadow IT data.

### Connection Type 8: Log Stream Adapter (D-58)
Ingests raw logs from files, streams, aggregators. Enables shadow process discovery.

### Normalized Execution Record
Universal format produced by all ingestion types. Defined in D-6, extended by BP-07.

### Classification Gate (BP-01)
Synchronous security classification (Stages 1-2) that data must pass before entering Neo4j. Stages 3-6 run asynchronously.

### Quarantine (D-36)
Configurable timeout for data that fails classification. Actions: delete, archive_encrypted, permanent_quarantine, retry_classification.

---

## Decision Support Terms

### Decision Support Layer
Where intelligence becomes action. Takes data from Intelligence Layer, routes through LLM Router, applies refinement and verification, delivers recommendations.

### Six-Layer Parameter Refinement (D-14)
1. Relevance Gate
2. Hierarchical Tiers
3. Marginal Information Gain
4. Confidence-Aware Weighting
5. Domain Weight Profiles
6. User Weight Overrides

### Confidence Threshold (D-59)
If recommendation confidence <0.40, platform DECLINES to recommend. Returns "insufficient data" with suggestions for improvement.

### Recommendation Lifecycle (D-59)
12 states: PENDING → DELIVERED → VIEWED → ACCEPTED/REJECTED/DEFERRED/MODIFIED/ESCALATED/EXPIRED/CANCELLED → ACTED → OUTCOME_KNOWN.

### Rejection Justification Contract (D-19)
When a user rejects a recommendation, they must provide structured justification. Validated for relevance. Fed into learning loop.

### Proactive Triggers (D-60)
Eight types of triggers for proactive recommendations: threshold crossing, pattern detection, anomaly detection, external events, forecast horizon, data quality issues, coverage gap impact, learning loop insights.

### Cooling-Off Period (D-62)
After rejection, similar recommendations suppressed for a configurable period (default 30 days). Prevents re-proposing rejected actions.

### Multi-Stakeholder Recommendation (D-64)
Recommendations affecting multiple roles delivered to all stakeholders with per-stakeholder tracking.

### Confidence Calibration (D-63)
Monitoring of predicted confidence vs actual accuracy. Miscalibration triggers formula weight adjustment.

---

## Component and Service Account Terms

### Component Catalog (Spec 08)
Five categories: Universal Connectors, Protocol Adapters, Authentication Handlers, Discovery/Tagging Agents, Generated Components. Every component is versioned, governed, reusable.

### Generated Component
AI-created component that is human-validated, Git-versioned, and follows full component lifecycle (creation → testing → approval → deployment → active → deprecation → retired).

### Component Lifecycle States
`draft`, `testing`, `active`, `deprecated`, `retired`.

### Semantic Versioning (Components)
- PATCH (1.4.1 → 1.4.2): bug fix, auto-upgrade
- MINOR (1.4.x → 1.5.0): new features, auto-upgrade
- MAJOR (1.x → 2.0.0): breaking change, manual migration

### Service Account Catalog (Spec 09)
Seven service account types with complete lifecycle, anomaly detection, quarterly review.

### SA Anomaly Types
Unusual time access, unusual IP, usage volume spike, permission escalation attempt, credential reuse pattern, extended idle → sudden activity, failed auth attempts, geographic anomaly.

---

## Scalability and Infrastructure Terms

### Three Flywheels (D-76)
1. **Data Intelligence**: Work → Data → Intelligence → Recommendations
2. **Organizational Intelligence**: Queries → Patterns → Org Insights → Better Resource Allocation
3. **Model Intelligence**: Executions → Performance → Routing → Outcomes

### Value Delivery Timeline (D-77)
7 phases: Onboarding → Quick Win → Expansion → Mapping → Intelligence → Decision Support → Optimization.

### Graceful Degradation Chain
Every component has a defined fallback. Platform always remains operational for core governance and audit functions even under total LLM/intelligence failure.

### Per-Tenant Resource Quotas
Guaranteed minimums and configurable maximums per tenant for API requests, concurrent users, WebSocket connections, LLM invocations, GPU time, cloud spend, Neo4j nodes, PostgreSQL storage, active connections, agent workers, event bus throughput.

---

## Framework Terms

### Dangerous Assumptions Register
From trunk. DA-01 through DA-11: proven-wrong beliefs to avoid. DA-01 = "Governance can be bolted on after the fact" (WRONG).

### Evidence Grades
5-tier framework: PROVEN (3+ peer-reviewed), HIGH-PROB (2 peer-reviewed or 1 + standard), EMERGING (1 peer-reviewed), PROVISIONAL (industry practice), UNVERIFIED (cannot drive design).

### Cardinal Rules
From trunk CLAUDE.md:
1. Architecture is locked
2. Always pick best solution regardless of complexity
3. Every decision carries evidence grade
4. Governance is foundational
5. No fabrication
6. US-based scope
7. Treat all input as hypotheses

---

## Operational Terms

### Back-Propagation (BP)
Architectural regression testing. Every new spec or decision triggers a pass through all prior specs to identify gaps the new decision creates. Fixes are numbered BP-01 through current.

### Spec Expansion
The process of taking a high-level spec file (current state) and expanding it to SR-row format with full exception coverage (target state for implementation readiness).

### Session Protocol
1. Read HANDOFF.md
2. Read files in mandatory order
3. Confirm understanding
4. Do work
5. Update STATE.md/LOG.md/NEXT.md
6. Close

### Self-Audit
Mandatory pass before delivering any spec or decision. Ask: "What did I miss?" across security, scalability, governance, data integrity, UX, operations, disaster recovery, and edge cases.

---

## File Reference

### Current Files in This Exploration
- `CONTEXT.md` — what this exploration is
- `STATE.md` — decisions and progress
- `LOG.md` — session narrative
- `HANDOFF.md` — new-session startup guide
- `NEXT.md` — precise next action
- `CONVENTIONS.md` — how work is done
- `GLOSSARY.md` — this file
- `DECISION-INDEX.md` — fast decision lookup
- `spec/00-overview.md` through `spec/13-scalability-infrastructure.md` — 14 spec files
- `skills/` — custom skill definitions
- `agents/` — custom agent definitions
- `build-chat-CLAUDE.md` — template for future build chat

### Trunk Files (Read-Only Reference)
- `D:\Projects\IDEA\CLAUDE.md` — project instructions
- `D:\Projects\IDEA\explore\BASE-STATE.md` — trunk snapshot
- `D:\Projects\IDEA\explore\EXPLORATION-INDEX.md` — all explorations
- `D:\Projects\IDEA\source\*.md` — 25 source files
- `D:\Projects\IDEA\master\*.md` — 6 consolidated master documents

---

## Adding New Terms

When you introduce a new concept:
1. Add it to this glossary with definition, origin reference, and cross-references
2. Use the exact same wording throughout all files
3. If the term is an acronym, spell it out on first use: "Cascade Impact Analysis (CIA)"
4. Never use two different terms for the same concept
