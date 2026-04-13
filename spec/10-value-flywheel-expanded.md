# Spec 10 Expanded: Value Flywheel

**Source:** `001/spec/10-value-flywheel.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** Strategic (product model)
**Reserved SR range:** `SR_FW_01` through `SR_FW_99`
**Last updated:** 2026-04-10

---

## Purpose

The Value Flywheel spec is strategic rather than operational. The SR rows here describe the measurement and observation operations that prove the flywheel is working: cohort metric capture, retention tracking, value-delivery phase signaling, and the quick-win delivery surface that prevents early churn. Architecturally locked from D-2, D-7, D-17, D-28, D-76, D-77.

## Architectural Decisions Covered

D-2, D-7, D-17, D-28, D-76, D-77.

## Trunk Inheritance

None (this spec consumes signals from every other spec).

## Integration Map

| Consumer | Depends On |
|----------|------------|
| Spec 06 Decision Support | `SR_FW_05` (cohort metrics inform recommendation noise management) |
| Spec 07 Interface | `SR_FW_10` (value-signal nudges in onboarding) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_CohortTracker` | Tracks day-1, week-1, week-4, month-3, month-6, year-1 active usage per tenant |
| `REUSABLE_PhaseSignaler` | Determines current value-delivery phase (Onboarding → Optimization) per tenant |

---

## SR Rows

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_FW_01` | --- | flywheel | Track Flywheel 1 (Data Intelligence): record every Work → Data → Intelligence → Recommendations event sequence so the cycle is observable and provable. | `REUSABLE_PgWriter`, `REUSABLE_CohortTracker` | Continuous | `FlywheelEvent { tenant_id, type, source, target }` | Row in `flywheel_events` | JSON | `Result` | End | Without measurement, the flywheel is an assertion not a fact. |
| `SR_FW_02` | --- | flywheel | Track Flywheel 2 (Organizational Intelligence): query analytics → patterns → insights per D-17. Source: `query_analytics` aggregation. | `SR_GOV_38` aggregates | Hourly | `OrgIntelInput` | Insights surfaced for tenant admins | JSON | `Result` | End | Meta-intelligence about platform usage helps customers understand their organization. |
| `SR_FW_03` | --- | flywheel | Track Flywheel 3 (Model Intelligence): ModelExecution → ModelOutcomeScore → routing improvements per D-28. | `SR_DM_17` | Hourly | `ModelIntelInput` | Routing rules tuned | JSON | `Result` | End | Outcome data drives better routing over time. |
| `SR_FW_05` | --- | flywheel | Cohort retention metrics per the targets table: day-1 90%, week-1 80%, week-4 70%, month-3 65%, month-6 60%, year-1 55%; connections per month increasing trend; queries per active user per week increasing; recommendations acted on >50%. | `REUSABLE_CohortTracker` | Daily | `CohortMetricsTick` | Per-tenant cohort metrics; alerts on miss | JSON | `Result` | If miss: alert customer success. | Quantitative leading indicators for retention. |
| `SR_FW_06` | --- | flywheel | Quick-win feature gate: ensure that the Week 1 quick-win features are deliverable on Day 1 (data consolidation dashboard, data quality alerts, basic automation, connection health monitoring, search across systems, simple Q&A). | Feature gate registry | Tenant onboarding | `QuickWinGateInput { tenant_id }` | Quick-win features enabled and surfaced in onboarding | JSON | `Result` | End | Quick wins prevent the "nothing works for 6 months" failure mode. |
| `SR_FW_10` | --- | flywheel | Value-delivery phase signaling per D-77: Onboarding → Quick Win → Expansion → Mapping → Intelligence → Decision Support → Optimization. Phase determined by graph size and feature usage; current phase surfaced to customer success and to tenant admin. | `REUSABLE_PhaseSignaler` | Daily evaluation | `PhaseEvalInput { tenant_id }` | Phase + signals + next-phase prerequisites | JSON | `PhaseResult` | End | Phase signals enable proactive customer success interventions. |
| `SR_FW_15` | --- | flywheel | Lock-in measurement: graph size, integration breadth, custom rule count, fine-tuned model count, cohort tenure. | `REUSABLE_PgReader` | Quarterly | `LockInInput` | Lock-in score | JSON | `Result` | End | Measures the healthy lock-in described in Spec 10 (lock-in via accumulated value, not contractual barriers). |
| `SR_FW_20` | --- | flywheel | Anti-churn signals: detect declining engagement, repeated rejections, unanswered notifications, feature abandonment; alert customer success. | Engagement monitor, `REUSABLE_Alerter` | Continuous | `AntiChurnInput` | Alerts to customer success | JSON | `Result` | End | Early warning system for at-risk tenants. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-122 | `SR_FW_05` cohort tracking | `SR_DS_30` (noise management) | Confirmed: noise management uses cohort engagement signals to throttle recommendations for at-risk users. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Phase signaling (`SR_FW_10`) — what graph size triggers each phase? | Per the source spec table: 50-200 nodes Onboarding → Quick Win; 500-1500 Expansion; 3000-8000 Mapping; 15000-40000 Intelligence; 40000-100000 Decision Support; 100000+ Optimization. Documented as default. |
| 2 | Quick-win feature gate (`SR_FW_06`) — what if a quick-win feature requires a connection that the tenant has not made yet? | The feature shows a "connect this system to enable" call-to-action. Documented as standard pattern. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_FW_02` | `SR_GOV_38` | Query analytics aggregation |
| `SR_FW_03` | `SR_DM_17` | Model performance aggregation |
| `SR_FW_05` | `SR_DS_30` | Noise management input |
| `SR_FW_20` | `SR_GOV_67` | Customer success alerts |

## Spec 10 Summary

| Metric | Value |
|--------|-------|
| Sections | 1 |
| Main-flow SRs | 9 |
| Exception SRs | 0 |
| Total SR rows | 9 |
| BP entries created | 1 (BP-122) |
| New decisions | 0 |

**Status:** Self-audit complete. Spec is intentionally lighter on SR density because the flywheel is a strategic measurement layer, not an operational pipeline.
