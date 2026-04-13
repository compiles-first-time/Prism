# Spec 09 Expanded: Service Account Catalog

**Source:** `001/spec/09-service-account-catalog.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** Infrastructure (credential management)
**Reserved SR range:** `SR_SA_01` through `SR_SA_99`
**Last updated:** 2026-04-10

---

## Purpose

Implementation-readiness for the Service Account Catalog: 7 SA types, 30+ field metadata, full lifecycle (request → approval → provisioning → testing → active → review → rotation → retirement), anomaly detection, CaaS integration, IAM integration for delegated creds, V2 handoff fields, governance rules.

## Architectural Decisions Covered

D-15, D-75, BP-91, BP-92, plus trunk GAP-14 (CaaS).

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| CaaS credential storage | GAP-14 | All SR_SA_ rows |
| Break-glass | GAP-49 | `SR_SA_13` (privileged activation) |

## Integration Map

| Consumer | Depends On |
|----------|------------|
| Spec 01 Governance | `SR_SA_30` (quarterly review), `SR_SA_35` (anomaly response) |
| Spec 02 Data Model | `SR_SA_05` (ServiceAccount node), `SR_SA_06` (registry row) |
| Spec 03 Connection | `SR_SA_15` (credential lookup for connections) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_RotationScheduler` | Schedules and executes credential rotations |
| `REUSABLE_AnomalyDetector` | Continuous monitoring for SA anomalies |

---

## Section 1 — Seven SA Types and Lifecycle (SR_SA_01 through SR_SA_15)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SA_01` | --- | sa | Register a Delegated User Credential SA: tied to an employee's IAM identity, revocable. | `SR_SA_05` (node), `SR_SA_06` (row) | Employee provides credential | `SaRegisterInput { tenant_id, type: delegated, person_id, system_id, scope }` | SA registered, lifecycle state PENDING | JSON | `Result { sa_id }` | `SR_SA_10` (approval) | Most restrictive scope class. |
| `SR_SA_02` | --- | sa | Register a Scoped Service Account: customer-created, role-mirroring, medium scope. | Same | Customer admin provides | `SaRegisterInput { type: scoped, role_mirror, system_id }` | SA registered | JSON | `Result` | `SR_SA_10` | Standard pattern for most enterprise integrations. |
| `SR_SA_03` | --- | sa | Register a Privileged Service Account: admin-level, dual-approval, time-boxed. | Same; `SR_GOV_46` break-glass for activation | Customer admin + security officer | `SaRegisterInput { type: privileged }` | SA registered with strict policies | JSON | `Result` | `SR_SA_10` | Highest-risk class. |
| `SR_SA_04` | --- | sa | Register an OAuth App SA: platform as OAuth app at customer's vendor. | Same | OAuth flow | Same | Same | Same | `SR_SA_10` | OAuth-based integrations. |
| `SR_SA_05` | --- | sa | Register an API Key SA: header / query param key. | Same | Customer provides key | Same | Same | Same | `SR_SA_10` | API key-based integrations. |
| `SR_SA_06` | --- | sa | Register a Certificate SA: mTLS certificate. | Same; certificate management | Customer IT provides cert | Same | Same | Same | `SR_SA_10` | Certificate-based authentication. |
| `SR_SA_07` | --- | sa | Register a Shared Service Account: one account, multiple processes. | Same; usage tracking | Customer IT designates | Same | SA with multi-process tracking | JSON | `Result` | `SR_SA_10` | Shared SAs require special anomaly handling. |
| `SR_SA_10` | --- | sa | Approval chain per type: delegated (user + admin), scoped (admin + security officer), privileged (admin + security officer + dual approval), oauth (admin + customer admin). | `SR_GOV_41`, `SR_GOV_46` | After registration | `ApprovalInput` | Approved or rejected | JSON | `Result` | If approved: `SR_SA_11`. | Type-specific approval depth. |
| `SR_SA_11` | --- | sa | Provision the credential in CaaS via `REUSABLE_CaaSCredentialRetriever`. | `REUSABLE_CaaSCredentialRetriever` | After approval | `ProvisionInput { raw_credential }` | Encrypted in CaaS; ref returned | JSON | `Result { credential_caas_ref }` | `SR_SA_12` (testing) | Centralized credential storage. |
| `SR_SA_12` | --- | sa | Test the SA via a small read against the target system. | `REUSABLE_AdapterRegistry` | After provisioning | `TestInput` | Pass / fail | JSON | `Result` | If pass: `SR_SA_13` (active). Else: rejected. | Catches misconfiguration before active use. |
| `SR_SA_13` | --- | sa | Activate the SA: status ACTIVE; usage permitted; quarterly review timer started. Precondition: `SR_V2_10` SA field validation must pass first to ensure all V2 handoff fields (`systems_accessible[]`, `permission_level`, `permission_scope`, `rate_limits`, `concurrent_sessions`, `rotation_schedule`, `shared_by_components[]`, `automation_eligible`) are populated. | `REUSABLE_PgWriter`, scheduler, `SR_V2_10` (precondition) | After test pass | `ActivationInput { sa_id }` | Status ACTIVE; V2 fields validated | JSON | `Result { active, v2_validation_passed }` | End | Active SAs are monitored continuously. The V2 field validation precondition ensures every active SA is V2-ready (BP-124). |
| `SR_SA_14` | --- | sa | Suspend an SA on anomaly or admin action. Invoked by `SR_SA_45` auto-suspension or by `SR_GOV_64` anomaly response. | `SR_GOV_64`, `REUSABLE_AnomalyDetector` | Triggered by `SR_SA_45` (auto) or `SR_GOV_64` (governance response) or admin action | `SuspendInput { sa_id, reason, source: auto \| governance \| admin }` | Status SUSPENDED; admin notified via `SR_GOV_67` | JSON | `Result { suspended, notification_id }` | Admin reviews; reactivate or retire. | Containment for compromised or anomalous SAs. Bidirectional reference to `SR_SA_45` and `SR_GOV_64` completes BP-117. |
| `SR_SA_15` | --- | sa | SA credential retrieval for a connection: connection layer requests, JIT activation, returned to caller, destroyed on use. | `REUSABLE_CaaSCredentialRetriever` | Inline from `SR_CONN_*` adapter | `RetrievalInput { connection_id }` | Credential returned briefly | JSON | `Result { credential, expires_in }` | Caller uses immediately. | JIT activation per trunk model. |

## Section 2 — Rotation, Review, Retirement (SR_SA_20 through SR_SA_30)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SA_20` | --- | sa | Schedule rotation per `rotation_schedule`: 30-day default for privileged, 90-day for scoped, etc. | `REUSABLE_RotationScheduler` | At registration | `ScheduleInput` | Rotation cron set | JSON | `Result` | At each interval: `SR_SA_21`. | Automated rotation prevents stale credentials. |
| `SR_SA_21` | --- | sa | Pre-rotation alerts: 14-day, 3-day, day-of notifications to admin. | `REUSABLE_Alerter` | Cron triggers | `PreAlertInput` | Notifications sent | JSON | `Result` | At day-of: `SR_SA_22`. | Bounded surprise. |
| `SR_SA_22` | --- | sa | Rotate the credential: new credential provisioned in CaaS, old retained briefly for rollback, components automatically updated to use new credential ref. | CaaS, `REUSABLE_CaaSCredentialRetriever` | At rotation date | `RotateInput` | New ref active; old ref archived for 24h | JSON | `Result { new_ref }` | End | Zero-downtime rotation. |
| `SR_SA_22_SE-01` | SE | sa | New credential provisioning fails. | CaaS exception | Rotation attempt | Same | Old credential retained; admin alerted; manual remediation | Same | `Result { state: rotation_failed }` | Manual fix. | Failsafe: do not break working credential when new one cannot be provisioned. |
| `SR_SA_30` | --- | sa | Quarterly governance review per `SR_GOV_63` and BP-92: still needed?, scope still correct?, anomalous usage?, mark for retirement / scope change / continuation. Invokes `SR_GOV_63` for the actual review-task creation; this SR is the SA-side scheduler. | `SR_GOV_63` | Quarterly scheduler firing | `ReviewInput { tenant_id }` | Review tasks created via `SR_GOV_63` | JSON | `Result { review_tasks_created }` | End | Regular pruning of stale and over-permissioned SAs. Bidirectional reference to `SR_GOV_63` completes BP-116. |

## Section 3 — Anomaly Detection (SR_SA_35 through SR_SA_45)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SA_35` | --- | sa | Continuous SA anomaly monitoring per `REUSABLE_AnomalyDetector`: usage timing, IP, volume, permission escalation, credential reuse, dormancy patterns, failed auth, geographic. | `REUSABLE_AnomalyDetector`, `sa_usage_log` | Continuous | `MonitorTickInput` | Anomalies detected | JSON | `AnomalyEvent[]` | For each anomaly: `SR_SA_36`-`SR_SA_44`. | Compromised or misused SAs are the most common breach vector. |
| `SR_SA_36` | --- | sa | Detect unusual time access (outside normal time windows). | `REUSABLE_AnomalyDetector` | Continuous | Same | Anomaly if outside window | JSON | `AnomalyEvent` | `SR_GOV_64` | Pattern-based detection. |
| `SR_SA_37` | --- | sa | Detect unusual IP (unexpected range). | Same | Same | Same | Anomaly if unexpected | JSON | Same | `SR_GOV_64` (potentially suspend per `SR_SA_14`). | Geographic and ASN-based detection. |
| `SR_SA_38` | --- | sa | Detect usage volume spike (10x normal). | Same | Continuous | Same | Anomaly if 10x | JSON | Same | `SR_GOV_64` | Volume spikes signal compromise or misuse. |
| `SR_SA_39` | --- | sa | Detect permission escalation attempt: SA tries to access scope outside its grant. | Same | On every API call | Same | Anomaly + BLOCK | JSON | Same | Immediate block; critical alert. | Active probing of credential scope. |
| `SR_SA_40` | --- | sa | Detect credential reuse pattern: unexpected component using the credential. | Same | On every retrieval | Same | Anomaly | JSON | Same | `SR_GOV_64` | Credential leakage indicator. |
| `SR_SA_41` | --- | sa | Detect extended idle → sudden activity: dormant credential becomes active. | Same | Continuous | Same | Anomaly | JSON | Same | `SR_GOV_64` | Dormant accounts are common attack vectors. |
| `SR_SA_42` | --- | sa | Detect failed auth attempts: progressive response (alert → suspend → lock). | Same | Failed auth events | Same | Progressive action | JSON | Same | `SR_SA_14` after threshold | Bruteforce protection. |
| `SR_SA_43` | --- | sa | Detect geographic anomaly: unexpected country / region. | Same | Continuous | Same | Anomaly | JSON | Same | `SR_GOV_64` (potentially suspend) | Geographic IP intelligence. |
| `SR_SA_44` | --- | sa | Track all anomalies in `sa_anomalies` table per `SR_DM_21`. | `SR_DM_21` | After detection | Same | Row inserted | JSON | `Result` | End | Persistent record for review. |
| `SR_SA_45` | --- | sa | Auto-suspension criteria: critical anomalies (permission escalation, repeated failed auth, geographic + IP spike) trigger immediate suspension; lesser anomalies queue for human review. | Auto-suspend rules | After anomaly | `AutoSuspendInput` | Suspended or queued | JSON | `Result` | If suspended: `SR_SA_14`. | Automated containment of high-confidence threats. |

## Section 4 — V2 Handoff and Integrations (SR_SA_50 through SR_SA_55)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SA_50` | --- | sa | V2 handoff fields per D-15: systems_accessible, permission_level, rate_limits, concurrent_sessions, rotation_schedule, shared_by_components, automation_eligible. | `REUSABLE_PgWriter` | Inline at registration and on changes | `V2FieldsInput` | Fields stored | JSON | `Result` | End | V2 automation engine reads these to plan automations. |
| `SR_SA_51` | --- | sa | IAM integration for delegated credentials: SA tied to IAM identity; auto-suspend when user disabled in IAM; alert admin for replacement when employee leaves. | `SR_GOV_03` (IAM webhook) | IAM event | `IamSyncInput` | SA suspended or flagged | JSON | `Result` | `SR_SA_14` | Identity changes propagate to SAs. |
| `SR_SA_52` | --- | sa | Orphaned SA detection: SAs with no components using them flagged for review after 30 days of zero usage. | Orphan scanner | Periodic | `OrphanScanInput` | Orphans flagged | JSON | `Result` | `SR_SA_30` review | Pruning unused SAs reduces attack surface. |
| `SR_SA_53` | --- | sa | Health score computation: composite of usage anomaly count, last successful auth, rotation freshness. | Health computer | Continuous | `HealthInput` | Score updated | JSON | `Result` | End | Operational visibility. |
| `SR_SA_54` | --- | sa | Cost attribution per SA (if paid account): usage cost tracked. | `REUSABLE_PgWriter` | After billable events | `CostInput` | Cost row updated | JSON | `Result` | End | Per-SA cost visibility. |
| `SR_SA_55` | --- | sa | Retirement: credential destroyed in CaaS; dependent components migrated; audit retained permanently. | CaaS destroy, migration helper | After review | `RetirementInput` | SA retired | JSON | `Result` | End | Bounded lifecycle. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-116 | `SR_SA_30` (quarterly review) | `SR_GOV_63` | Confirmed bidirectional invocation. | 1 |
| BP-117 | `SR_SA_45` (auto-suspension) | `SR_GOV_64`, `SR_GOV_67` | Confirmed: suspension uses `SR_GOV_64` and alerts use `SR_GOV_67`. | 1 |
| BP-118 | `SR_SA_51` (IAM integration) | `SR_GOV_03`, `SR_GOV_13` | Confirmed: termination cascades to delegated SAs. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Auto-suspension thresholds (`SR_SA_45`) need explicit definition. | Implementation: permission escalation = immediate; >5 failed auth in 5min = immediate; geographic + IP spike together = immediate; isolated single-anomaly types = queue for review. |
| 2 | Orphan detection (`SR_SA_52`) — what is "zero usage"? | No `sa_usage_log` rows in 30 days. Configurable per tenant. |
| 3 | Shared SA usage tracking (`SR_SA_07`) — how is "multi-process" anomaly handled? | Each retrieval includes a calling-component identifier; anomaly detection considers the unique component set as the baseline. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_SA_05` (node) | `SR_DM_20` | ServiceAccount node creation |
| `SR_SA_15` | `SR_CONN_*` | Credential retrieval for connections |
| `SR_SA_30` | `SR_GOV_63` | Quarterly review |
| `SR_SA_35` | `SR_GOV_64` | Anomaly response |
| `SR_SA_44` | `SR_DM_21` | Anomaly persistence |
| `SR_SA_51` | `SR_GOV_03` | IAM integration |

## Spec 09 Summary

| Metric | Value |
|--------|-------|
| Sections | 4 |
| Main-flow SRs | 32 |
| Exception SRs | 1 |
| Total SR rows | 33 |
| BP entries created | 3 (BP-116 through BP-118) |
| New decisions | 0 |

**Status:** Self-audit complete.

---

## Back-Propagation Received from Later Specs (applied retroactively in Session 2)

| BP | Originating Spec | Edit Applied to Spec 09 |
|----|-----------------|-------------------------|
| BP-124 | Spec 12 (`SR_V2_10` SA field validation) | `SR_SA_13` updated to invoke V2 field validation precondition |
| BP-117 | Spec 01 (`SR_GOV_64` anomaly response) | `SR_SA_14` updated with bidirectional reference to `SR_SA_45` and `SR_GOV_64` |
| BP-116 | Spec 01 (`SR_GOV_63` quarterly review) | `SR_SA_30` updated with bidirectional reference |

**Total retroactive edits to Spec 09: 3 SR row updates.**
