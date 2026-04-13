# Spec 08 Expanded: Component Catalog

**Source:** `001/spec/08-component-catalog.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** Infrastructure (powers all functionality)
**Reserved SR range:** `SR_CAT_01` through `SR_CAT_99`
**Last updated:** 2026-04-10

---

## Purpose

Implementation-readiness for the Component Catalog: 5 categories, 50+ field metadata schema, full lifecycle (creation → testing → approval → deployment → active → deprecation → retired), Git integration, AI generation workflow, semantic versioning, multi-version coexistence, isolation/resource limits, fast rollback, sharing across tenants, marketplace future-proofing.

## Architectural Decisions Covered

D-4, D-15, D-71, D-72, D-73, D-74, plus CC-1 through CC-14.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| Service Principal | `FOUND § 1.3.1` | `SR_CAT_30` (every component runs as a Service Principal) |
| AI-generated code review | BP-86 | `SR_CAT_22` (security scan + human review) |

## Integration Map

| Consumer | Depends On |
|----------|------------|
| Spec 01 Governance | `SR_CAT_15` (approval), `SR_CAT_22` (AI gen review), `SR_CAT_35` (execution preflight) |
| Spec 02 Data Model | `SR_CAT_05` (Component node), `SR_CAT_06` (registry row) |
| Spec 03 Connection | `SR_CAT_50` (universal connectors and protocol adapters) |
| Spec 04 Intelligence | `SR_CAT_05` (DEPENDS_ON edges) |
| Spec 09 SA Catalog | `SR_CAT_30` (credential references via SA) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_GitVersionManager` | Wraps Git operations: tag, commit, revert, branch, history |
| `REUSABLE_SemverEvaluator` | Determines patch / minor / major from version delta |
| `REUSABLE_ComponentSandbox` | Isolated execution context with CPU/memory/network/IO/timeout limits |
| `REUSABLE_TestRunner` | Runs unit, integration, security, performance, regression, chaos, contract, e2e tests |

---

## Section 1 — Five Categories and Lifecycle (SR_CAT_01 through SR_CAT_15)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_CAT_01` | --- | component | Register a Universal Connector component (Microsoft 365, Salesforce, QuickBooks, Monday.com, Box, Google Workspace, Ekos, Epic/Cerner, SAP/Oracle, etc.). | `REUSABLE_GitVersionManager`, component_registry | Initial setup or new connector | `ConnectorRegistration { tenant_id?, vendor_system, version, code_artifact_ref }` | Component node + registry row + Git tag | JSON | `Result { component_id }` | `SR_CAT_10` (testing) | Pre-built integrations cover the most common SaaS systems out of the box. |
| `SR_CAT_02` | --- | component | Register a Protocol Adapter (REST, GraphQL, SOAP, gRPC, ODBC/JDBC, file system, email IMAP/SMTP/EWS, message queues, SSH/SFTP, WebSocket, log file). | Same | Initial setup | Same | Same | Same | Same | Generic adapters fill gaps left by universal connectors. |
| `SR_CAT_03` | --- | component | Register an Authentication Handler (OAuth 2.0 all flows, OAuth 1.0a, SAML 2.0, OIDC, API Key, Basic Auth, mTLS, JWT, session cookies, MFA handlers). | Same; security officer approval per `SR_GOV_61` | New auth flow needed | Same | Same | Same | Same | Auth handlers concentrate the security-sensitive code paths. |
| `SR_CAT_04` | --- | component | Register a Discovery / Tagging Agent (schema discovery, process discovery, metadata tagging Stages 1-6, SDA, Research Agent, data quality assessment, AI navigation, relationship inference, anomaly detection, coverage analysis). | Same; admin + security officer per `SR_GOV_61` | Agent capability needed | Same | Same | Same | Same | Agents are the highest-leverage components, hence the dual approval. |
| `SR_CAT_05` | --- | component | Register a Generated Component (AI-created, human-validated, Git-versioned). | Same; AI gen review per `SR_GOV_62` (BP-86) | New component need identified | `GenComponentInput { spec, tenant_id }` | Component generated → reviewed → registered | Same | `Result` | `SR_CAT_22` (AI gen workflow) | Generated components let the platform adapt to customer-specific systems. |
| `SR_CAT_06` | --- | component | Persist component_registry row per `SR_DM_13` with all 50+ metadata fields. | `SR_DM_13` | Inline from `SR_CAT_01-05` | `RegistryWriteInput` | Row inserted | JSON | `Result { row_id }` | End | Centralized registry powers lookups, governance, and rollback. |
| `SR_CAT_07` | --- | component | Create Component node per `SR_DM_12` with category, version, dependencies. | `SR_DM_12` | Inline from `SR_CAT_01-05` | `NodeWriteInput` | Node created | JSON | `Result { node_id }` | End | Graph node enables impact analysis and reuse mapping. |
| `SR_CAT_08` | --- | component | Apply semantic versioning per D-71: PATCH (bug fix, auto-upgrade), MINOR (new features, auto-upgrade), MAJOR (breaking change, manual migration). Dependency version ranges support multi-version coexistence. | `REUSABLE_SemverEvaluator` | Inline at registration | `VersioningInput { current, new, change_summary }` | semver assignment + upgrade path | JSON | `Result { semver }` | End | Predictable semver enables safe auto-upgrades. |
| `SR_CAT_09` | --- | component | Lifecycle state transition: draft → testing → approval → deployment → active → deprecation → retired. Each transition is logged and gated. | State machine | At each lifecycle event | `LifecycleTransitionInput` | State updated | JSON | `Result` | Next stage in lifecycle. | Strict lifecycle enforcement prevents skipping testing or approval. |
| `SR_CAT_10` | --- | component | Run testing pipeline per `REUSABLE_TestRunner`: unit + integration + security (SAST + dependency check) + performance benchmark + regression + chaos (critical) + contract + end-to-end. | `REUSABLE_TestRunner` | After draft state | `TestPipelineInput` | All test results | JSON | `TestPipelineResult` | If all pass: `SR_CAT_15`. | Comprehensive testing prevents broken components from reaching production. |
| `SR_CAT_10_BE-01` | BE | component | Any test fails. | TestRunner | Failure | Same | Component blocked from approval; failures reported to owner | Same | `Result { state: blocked }` | Owner fixes and retries. | No test failures permitted. |
| `SR_CAT_15` | --- | component | Approval per `SR_GOV_61` (category-specific chains). Precondition: `SR_V2_01` field validation must pass first to ensure all V2 handoff fields are populated; `SR_V2_05` AI-assisted autofill helper proposes defaults for missing fields if any are detected. | `SR_GOV_61`, `SR_V2_01` (precondition), `SR_V2_05` (autofill) | After tests pass | `ApprovalInput { component_id, category, version, change_summary }` | Approved or rejected; on approval, component status set to `active` | JSON | `Result { approved, v2_validation_passed, autofilled_fields[]? }` | If V2 validation fails: route to `SR_V2_05` autofill, then re-attempt validation. If passed: `SR_GOV_61` approval flow. If approved: `SR_CAT_20` (deployment). | Approval gates enforce category-specific scrutiny. The V2 field validation precondition ensures every active component is V2-ready (BP-123). |

## Section 2 — AI Generation Workflow (SR_CAT_20 through SR_CAT_25)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_CAT_20` | --- | component | Identify need for AI-generated component: cannot connect to system, unknown format, user request. | Need detector | Continuous + on user request | `NeedInput` | Need recorded with priority | JSON | `Result` | `SR_CAT_21` | Triggers the AI generation pipeline. |
| `SR_CAT_21` | --- | component | Spec gathering: admin provides API docs + sample data; AI prepares specification. | Admin panel, T2/T3 LLM | Need raised | `SpecGatheringInput` | Specification assembled | JSON | `Result` | `SR_CAT_22` | Clear spec is the prerequisite for code generation. |
| `SR_CAT_22` | --- | component | AI code generation per D-72: T3 model with templates produces code; automated validation (lint, types, unit tests, security, dependency); `SR_GOV_62` AI-code review (SAST + dependency check + supply-chain check + mandatory human review); real integration test (against actual target system with test creds); approval and deployment. Track `generation_success_rate`. | T3 LLM via `SR_LLM_13`, validators, `SR_GOV_62` (AI gen review), `REUSABLE_TestRunner` | After spec from `SR_CAT_21` | `CodeGenInput { spec, tenant_id }` | Generated artifact + validation report; on review pass, returns to `SR_CAT_15` for normal approval | JSON | `CodeGenResult { artifact_ref, gov_62_passed, findings? }` | If pass: `SR_CAT_15`. Else: blocked, owner notified. | Multi-layer validation prevents AI hallucinated vulnerabilities from reaching production. Bidirectional reference to `SR_GOV_62` makes the dual-gate (Spec 08 generation + Spec 01 governance review) explicit (BP-113). |
| `SR_CAT_22_BE-01` | BE | component | Validation finds security issues. | Validators | Inline | Same | Generation rejected; human reviewer notified | Same | `Result { state: rejected, findings }` | Owner fixes spec or generates again. | Security findings block all approvals. |
| `SR_CAT_23` | --- | component | Track `generation_success_rate` per CC-14: percentage of generation attempts that passed all validation and were approved. | `REUSABLE_PgWriter` | After each generation | `GenStatsInput` | Stats updated | JSON | `Result` | End | Improvement signal for the generation prompts and templates. |
| `SR_CAT_24` | --- | component | Documentation freshness check (CC-8): PR for component code requires accompanying doc updates. | Doc validator in CI | PR submission | `DocCheckInput` | Pass / fail | JSON | `Result` | If fail: PR blocked. | Documentation drift is the most common cause of operational confusion. |
| `SR_CAT_25` | --- | component | Component SDK for customer-developed components per CC-9. | SDK distribution | Customer downloads | `SdkInput` | SDK package | JSON | `Result` | End | Some customers want to build their own components. |

## Section 3 — Composition, Isolation, Versioning, Sharing (SR_CAT_30 through SR_CAT_50)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_CAT_30` | --- | component | Component execution preflight: invokes `SR_GOV_78` to verify status active, credentials available, principal permitted, and `SR_CAT_43` credential drift detection has not flagged the component. | `SR_GOV_78`, `SR_CAT_43` | Inline before execution | `ExecutionPreflightInput { tenant_id, principal, component_id, args }` | ALLOW / DENY | JSON | `Result { decision }` | If ALLOW: `SR_CAT_31`. Else: caller informed with reason. | Centralized gating. Bidirectional reference to `SR_GOV_78` completes BP-114. |
| `SR_CAT_31` | --- | component | Run a component in `REUSABLE_ComponentSandbox` with CPU/memory/network/IO/timeout limits per D-73 / CC-5 / CC-6. | `REUSABLE_ComponentSandbox`, `REUSABLE_CaaSCredentialRetriever` | After preflight | `ExecutionInput { component_id, args, deadline }` | Result with metrics | JSON | `ExecutionResult` | `SR_CAT_32` (metrics) | Isolation prevents bad components from destabilizing the runtime. |
| `SR_CAT_31_SE-01` | SE | component | Resource limit exceeded. | Sandbox | Limit hit | Same | Execution terminated; structured error returned | Same | `Result { error: limit_exceeded }` | Caller handles. | Component failure does not crash the process. |
| `SR_CAT_31_SE-02` | SE | component | Component execution timeout. | Sandbox | Deadline | Same | Execution terminated | Same | `Result { error: timeout }` | Caller handles. | Bounded execution times. |
| `SR_CAT_32` | --- | component | Persist component_performance row per `SR_DM_14` after every execution. | `SR_DM_14` | After execution | `PerfInput` | Row inserted | JSON | `Result` | End | Powers auto-rollback rule (success_rate < 95%). |
| `SR_CAT_33` | --- | component | Composition rules per the spec: no circular dependencies, max 10 levels of composition, each component independently testable, no shared state, credentials via CaaS only, every component handles errors from children. | Composition validator | Component composition | `CompositionInput` | Pass / fail with diagnostics | JSON | `Result` | End | Bounded composition prevents pathological cases. |
| `SR_CAT_34` | --- | component | Multi-version coexistence: dependency version ranges (e.g., "B@^1.2") allow components to coexist with different versions of shared dependencies. | Dependency resolver | At resolution time | `DepResolveInput` | Resolved versions | JSON | `Result` | End | Avoids "dependency hell" (CC-4). |
| `SR_CAT_35` | --- | component | Fast rollback per D-74 / CC-11 / `SR_GOV_69`: <5 minutes; all versions retained 30 days; one-click rollback; active executions complete current version, new executions use rolled-back version; cross-tenant awareness. | `REUSABLE_GitVersionManager`, scheduler | Admin click or automated trigger | `RollbackInput { component_id, target_version }` | Active version reverted | JSON | `Result` | End | Bounded recovery time. |
| `SR_CAT_36` | --- | component | Auto-rollback on success_rate <95% over recent window. | Performance monitor | Continuous | `AutoRollbackInput` | Rollback triggered | JSON | `Result` | `SR_CAT_35` | Automated containment of bad deployments. |
| `SR_CAT_37` | --- | component | Component sharing across tenants: scopes platform_universal / industry_shared / tenant_specific / customer_shared_opt_in. Sharing requires no tenant-specific data, stricter security review, sharing agreement with liability terms, maintainer responsibility, revocable with migration path. | Sharing governance | Admin shares component | `ShareInput` | Sharing record created | JSON | `Result` | End | Sharing accelerates customer onboarding without weakening tenant isolation. |
| `SR_CAT_38` | --- | component | Deprecation workflow per CC-2: 90-day notice minimum; replacement identified; migration path documented; used-by processes notified; timeline enforced. | `REUSABLE_Alerter` | Admin marks deprecated | `DeprecationInput` | Notices dispatched; timeline scheduled | JSON | `Result` | At end of timeline, retire. | Smooth deprecation prevents user surprises. |
| `SR_CAT_39` | --- | component | Component retirement: terminal state; audit retained; code archived; dependencies migrated. | Archival system | After deprecation period | `RetirementInput` | Retired | JSON | `Result` | End | Cleanly removes from active set without losing audit. |
| `SR_CAT_40` | --- | component | Component audit trail per CC-12: Git history + deployment events provide complete provenance. | `REUSABLE_GitVersionManager`, audit | On query | `AuditQueryInput` | Audit history | JSON | `Result` | End | Complete provenance of every component. |
| `SR_CAT_41` | --- | component | License and legal validation per CC-13: license field required and validated against allowed list. | License validator | Inline at registration | `LicenseCheckInput` | Pass / fail | JSON | `Result` | If fail: registration rejected. | Compliance with open-source license obligations. |
| `SR_CAT_42` | --- | component | Component observability per CC-10: per-execution metrics (count, latency, success/failure, cost). | `SR_DM_14` | Continuous | `ObsInput` | Metrics streamed | JSON | `Result` | End | Operational visibility. |
| `SR_CAT_43` | --- | component | Credential scope drift per CC-3: re-approval required if a component's credential requirements change. | Drift detector, `SR_GOV_61` | Periodic + on update | `DriftInput` | Re-approval triggered | JSON | `Result` | If approved: continue. Else: blocked. | Prevents silent permission expansion. |
| `SR_CAT_44` | --- | component | Component versioning compatibility per CC-1: semver auto-upgrade rules; PATCH/MINOR auto-upgrade; MAJOR manual migration. | `REUSABLE_SemverEvaluator` | Upgrade event | `UpgradeInput` | Upgrade path | JSON | `Result` | End | Predictable upgrade behavior. |
| `SR_CAT_50` | --- | component | Component marketplace future-proofing: architecture must not preclude browse, ratings, certifications, paid components, community contributions, forking. | Architecture review | Future V2+ | N/A | Architectural readiness | N/A | `Result` | End | Designed for marketplace evolution. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-113 | `SR_CAT_22` (AI generation review) | `SR_GOV_62` | Confirmed: AI gen review is a separate gate from `SR_GOV_61` and runs first. | 1 |
| BP-114 | `SR_CAT_31` (sandbox isolation) | `SR_GOV_78` (execution preflight) | Confirmed: preflight invokes both governance and component status checks. | 1 |
| BP-115 | `SR_CAT_43` (credential drift) | `SR_SA_` (SA scope changes) | Confirmed: SA scope changes trigger drift detection in component dependencies. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Auto-rollback (`SR_CAT_36`) — what window for "recent"? | Implementation: 1-hour rolling window, configurable. |
| 2 | Composition rules (`SR_CAT_33`) — how is "independently testable" enforced? | Each component must have unit tests that exercise its public surface without invoking children. Enforced via `REUSABLE_TestRunner`. |
| 3 | License validation (`SR_CAT_41`) — what is the allowed list? | Default: MIT, Apache 2.0, BSD-2/3, ISC, MPL 2.0; tenant-configurable. |
| 4 | Sharing across tenants (`SR_CAT_37`) — how is "no tenant-specific data" enforced? | Static analysis on the component code at sharing time; flagging strings or constants matching tenant identifiers. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_CAT_06` | `SR_DM_13` | Registry write |
| `SR_CAT_07` | `SR_DM_12` | Component node |
| `SR_CAT_15` | `SR_GOV_61` | Approval |
| `SR_CAT_22` | `SR_GOV_62` | AI gen review |
| `SR_CAT_30` | `SR_GOV_78` | Execution preflight |
| `SR_CAT_32` | `SR_DM_14` | Performance row |
| `SR_CAT_35` | `SR_GOV_69` | Admin undo (overlap with rollback) |

## Spec 08 Summary

| Metric | Value |
|--------|-------|
| Sections | 3 |
| Main-flow SRs | 38 |
| Exception SRs | 3 |
| Total SR rows | 41 |
| BP entries created | 3 (BP-113 through BP-115) |
| New decisions | 0 |

**Status:** Self-audit complete.

---

## Back-Propagation Received from Later Specs (applied retroactively in Session 2)

| BP | Originating Spec | Edit Applied to Spec 08 |
|----|-----------------|-------------------------|
| BP-123 | Spec 12 (`SR_V2_01` field validation, `SR_V2_05` autofill) | `SR_CAT_15` updated to invoke V2 field validation precondition and autofill helper |
| BP-113 | Spec 01 (`SR_GOV_62` AI gen review) | `SR_CAT_22` updated with bidirectional reference to make the dual-gate explicit |
| BP-114 | Spec 01 (`SR_GOV_78` execution preflight) | `SR_CAT_30` updated with bidirectional reference and `SR_CAT_43` drift check |

**Total retroactive edits to Spec 08: 3 SR row updates.**
