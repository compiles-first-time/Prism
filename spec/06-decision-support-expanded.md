# Spec 06 Expanded: Decision Support

**Source:** `001/spec/06-decision-support.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** Primary user-facing value
**Reserved SR range:** `SR_DS_01` through `SR_DS_99`
**Last updated:** 2026-04-10

---

## Purpose

Implementation-readiness for Decision Support: 9-step pipeline (request classification → recipient identification → data gathering → 6-layer parameter refinement → analysis → recommendation assembly → conflict detection → delivery → response tracking), recommendation lifecycle (12 states, D-59), confidence threshold (<0.40 = decline), expiration, confidence formula, proactive triggers (D-60), conflict detection, recipient preferences (D-61), personalization, dependencies/chains (D-62), confidence calibration (D-63), multi-stakeholder (D-64), noise management (D-65), forecast component feedback (D-45), source system prediction comparison (D-44), Cascade Impact Analysis integration (D-47), response format template.

## Architectural Decisions Covered

D-14, D-19, D-34, D-44, D-45, D-46, D-47, D-52, D-55, D-56, D-59, D-60, D-61, D-62, D-63, D-64, D-65, plus DS-1 through DS-12.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| RTFMC fairness monitoring | GAP-59 | `SR_DS_30` (per-recommendation fairness check sample) |
| DBE v2 verification | trunk | `SR_DS_25` (calls `SR_LLM_30`) |

## Integration Map

| Consumer | Depends On |
|----------|------------|
| Spec 01 Governance | `SR_DS_05` (CSA call), `SR_DS_15` (rejection capture), `SR_DS_27` (justification) |
| Spec 02 Data Model | `SR_DS_20` (Recommendation node), `SR_DS_15` (Rejection node) |
| Spec 04 Intelligence | `SR_DS_03` (data gathering), `SR_DS_28` (CIA) |
| Spec 05 LLM Routing | `SR_DS_06` (analysis), `SR_DS_25` (verification) |
| Spec 07 Interface | `SR_DS_22` (delivery), `SR_DS_29` (notification) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_RecommendationStateMachine` | Validates and applies all 12-state transitions |
| `REUSABLE_ConfidenceCalculator` | Computes overall confidence per the formula in Spec 06 |
| `REUSABLE_ParameterRefiner` | 6-layer refinement pipeline |
| `REUSABLE_RecipientResolver` | Resolves the recipient set per role/process/escalation |

---

## Section 1 — Pipeline Steps 1-3 (SR_DS_01 through SR_DS_05)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DS_01` | --- | decision | Step 1 — Request classification: assign request type (reactive query, proactive trigger, scheduled, triggered, escalation). | Request classifier | Inline at request entry | `RequestInput { tenant_id, principal, query?, trigger?, schedule? }` | Classified request type | JSON | `ClassifiedRequest` | `SR_DS_02` | Different request types follow different downstream handling. |
| `SR_DS_02` | --- | decision | Step 2 — Recipient identification: resolve direct user / role-based / process owner / approval chain / escalation path; permission check, preference check, context adjustment. | `REUSABLE_RecipientResolver` | After `SR_DS_01` | `RecipientResolveInput` | Resolved recipient set with per-recipient preferences | JSON | `RecipientSet` | `SR_DS_03` | Personalized delivery requires knowing the audience. |
| `SR_DS_03` | --- | decision | Step 3 — Data gathering: identify required data, check freshness, refresh if stale, check quality, include source system predictions (D-44), pull external context (D-46 Research Agent), check Event Calendar (D-55), run CSA (D-34). | `SR_INT_18` | After `SR_DS_02` | `DataGatheringInput` | Aggregated data set with provenance, freshness, gaps | JSON | `DataGatheringResult` | `SR_DS_04` | Centralizes the data preparation pattern with all governance and quality checks. |
| `SR_DS_04` | --- | decision | Step 4 — Six-layer parameter refinement per D-14: Relevance Gate → Hierarchical Tiers → Marginal Information Gain → Confidence-Aware Weighting → Domain Weight Profiles → User Weight Overrides. Returns parameters used + parameters excluded with reasons. | `REUSABLE_ParameterRefiner` | After `SR_DS_03` | `RefinementInput` | Refined parameter set with transparency record | JSON | `RefinementResult { used[], excluded[] }` | `SR_DS_06` | Prevents parameter dilution; supports the transparency requirement. |
| `SR_DS_05` | --- | decision | Step 4b — Decision Support preflight per `SR_GOV_74`: invokes `SR_GOV_24` (CSA), `SR_GOV_71` (coverage), and `SR_GOV_18` (override justification when needed) before generation. The CSA call within preflight runs only if refined parameters cross multiple collections. | `SR_GOV_74`, `SR_GOV_24`, `SR_GOV_71`, `SR_GOV_18` | Inline before generation | `CsaCallInput { refined_parameters, collections[] }` | ALLOW / BLOCK / ANONYMIZE / ELEVATE per CSA + coverage check | JSON | `PreflightResult { allowed, blocked_reasons[] }` | If ALLOW: `SR_DS_06`. Else: handle action. | CSA must run before any combine analysis. The full preflight via `SR_GOV_74` ensures all three governance preconditions are checked together (BP-107). |

## Section 2 — Steps 4-6: Analysis, Verification, Assembly (SR_DS_06 through SR_DS_15)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DS_06` | --- | decision | Step 5 — Analysis: route through LLM Router (`SR_LLM_01`); use multi-model decomposition for complex queries (D-39 / `SR_LLM_18`). | `SR_LLM_01`, `SR_LLM_18` | After refinement | `AnalysisInput` | Verified analysis output | JSON | `AnalysisResult` | `SR_DS_07` | Analysis is delegated to the router per single-source-of-truth principle. |
| `SR_DS_07` | --- | decision | Step 6 — Recommendation assembly: text, justification, data sources, parameters used/excluded, confidence, coverage, market assumptions, alternatives, cascade impact, source system comparison, expiration, action options. | Assembly templater, `REUSABLE_ConfidenceCalculator` | After analysis | `AssemblyInput` | Full structured recommendation | JSON | `RecommendationDraft` | `SR_DS_08` (confidence threshold check) | Aligns with the response format template (D-14 transparency). |
| `SR_DS_08` | --- | decision | Confidence threshold rule per D-59: if overall confidence <0.40, platform DECLINES to recommend; returns "Insufficient data. Consider connecting these systems: [...]." | `REUSABLE_ConfidenceCalculator` | After assembly | `ConfidenceCheckInput { confidence }` | DELIVER or DECLINE | JSON | `ConfidenceCheckResult` | If DELIVER: `SR_DS_09`. Else: respond with insufficient-data message. | Prevents trust-destroying overconfident recommendations on weak data. |
| `SR_DS_09` | --- | decision | Compute overall confidence per the formula: weighted average of data_quality (0.25), data_freshness (0.20), coverage (0.15), semantic_entropy (0.20), relationship_confidence (0.10), historical_accuracy (0.10). | `REUSABLE_ConfidenceCalculator` | Inline within `SR_DS_07`/`SR_DS_08` | `ConfidenceFormulaInput` | Overall confidence + tier (HIGH/MODERATE/LOW/VERY_LOW/INSUFFICIENT) | JSON | `ConfidenceResult` | Returned. | Composite confidence captures all sources of uncertainty. |
| `SR_DS_10` | --- | decision | Step 7 — Conflict detection: check the new recommendation against other active recommendations, recent acceptances, company policy, active automations, user preferences, stale-vs-fresh. | Conflict detector | After confidence pass | `ConflictDetectInput` | Conflict report or clean | JSON | `ConflictDetectResult` | If conflict: handle per type. Else: `SR_DS_11`. | Conflict detection prevents inconsistent guidance. |
| `SR_DS_11` | --- | decision | Step 8 — Delivery: create Recommendation node via `SR_DM_09`; log to `recommendation_audit`; deliver per recipient preference (real-time / email / digest / weekly); set expiration timer. | `SR_DM_09`, delivery channel router | After conflict-free | `DeliveryInput` | Recommendation delivered; expiration scheduled | JSON | `DeliveryResult` | `SR_DS_12` (response tracking) | Multi-channel delivery respects recipient preferences. |
| `SR_DS_12` | --- | decision | Step 9 — Response tracking: capture user response (Accept / Reject (D-19) / Defer / Modify / Ignore / Escalate); transition state via `SR_DM_11`. | `REUSABLE_RecommendationStateMachine`, `SR_DM_11` | User response inbound | `ResponseInput` | State transitioned | JSON | `ResponseResult` | If Reject: `SR_DS_15`. Otherwise: state updated. | Closes the loop and feeds the learning system. |
| `SR_DS_13` | --- | decision | Lifecycle expiration handler: when `expires_at` passes without response, transition to EXPIRED. | Scheduled task runner | At `expires_at` | `ExpirationInput { rec_id }` | State EXPIRED; user notified if appropriate | JSON | `Result` | End | Bounded lifecycle prevents stale recommendations from accumulating. |
| `SR_DS_14` | --- | decision | Auto-invalidation on significant data change: when underlying data shifts materially, the recommendation becomes obsolete and is moved to CANCELLED with explanation. | Data-change subscriber | Triggered by Intelligence Layer change events | `InvalidationInput` | State CANCELLED; new recommendation may be generated | JSON | `Result` | If new rec: re-enter pipeline. | Prevents users from acting on outdated recommendations. |
| `SR_DS_15` | --- | decision | Capture rejection per `SR_GOV_72` (D-19): structured justification, category, free-text. Validate, store as Rejection node via `SR_DM_10`, feed learning loop. | `SR_GOV_72`, `SR_DM_10` | User submits rejection | `RejectionCaptureInput` | Rejection persisted; learning loop notified | JSON | `Result` | End | Aligns with D-19 quality requirement. |

## Section 3 — Triggers, Personalization, Calibration, Stakeholders, Noise, Feedback (SR_DS_20 through SR_DS_30)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DS_20` | --- | decision | Persist Recommendation node + audit row via `SR_DM_09`. | `SR_DM_09` | Inline from `SR_DS_11` | `RecommendationPersistInput` | Persisted | JSON | `Result` | Returned to `SR_DS_11`. | Centralizes persistence so the lifecycle and audit are always consistent. |
| `SR_DS_21` | --- | decision | Recommendation expiration timing matrix per D-59: urgent (4h), time-sensitive (24h), standard operational (7d), strategic (30d), informational (no expiration; may be superseded). | Expiration table | Inline at assembly | `ExpirationDecisionInput { category }` | Expiration timestamp | JSON | `Result { expires_at }` | Returned to assembly. | Bounded validity per category prevents stale guidance. |
| `SR_DS_22` | --- | decision | Recipient-centric delivery per D-61: each recipient's preferences (real-time push / email / digest / weekly / on-request, urgency filter, categories, quiet hours, escalation, format, confidence threshold) modulate delivery. | `REUSABLE_RecipientResolver` | Inline from `SR_DS_11` | `RecipientDeliveryInput` | Delivery routed per preferences | JSON | `Result` | End | Prevents recommendation fatigue while respecting urgency. |
| `SR_DS_23` | --- | decision | Recommendation personalization: same decision, different presentations per role (Procurement Manager → details + actions; CFO → cash flow impact + executive summary; CEO → one-line in weekly digest). | Personalizer | Inline from `SR_DS_22` | `PersonalizationInput` | Per-recipient view | JSON | `Result` | End | Per-role framing makes the same insight actionable for different roles. |
| `SR_DS_24` | --- | decision | Dependencies and chains per D-62: prerequisites (cannot deliver until prereq accepted), triggers (new recommendations on accept), cooling-off (default 30 days after rejection). | Dependency graph evaluator | Inline at delivery | `DependencyEvalInput` | Deliverable / deferred / blocked | JSON | `DependencyResult` | If deferred: re-evaluate when prereq state changes. | Real business decisions have dependencies; the platform must respect them. |
| `SR_DS_25` | --- | decision | Verified delivery: invoke `SR_LLM_30` DBE v2 if not already done; high-stakes recommendations use REAL_TIME_SAFETY profile per RTVP. | `SR_LLM_30` | Inline before delivery | `VerificationInput` | Pass/fail | JSON | `VerificationResult` | If pass: deliver. Else: regenerate or block. | DBE v2 is the gating verification layer. |
| `SR_DS_26` | --- | decision | Confidence calibration per D-63: track predicted confidence vs actual accuracy over time; miscalibration triggers formula weight adjustment via `REUSABLE_AgentFeedbackTracker`. | `REUSABLE_AgentFeedbackTracker`, `REUSABLE_PgWriter` | Continuous + weekly batch | `CalibrationCycleInput` | Updated formula weights | JSON | `Result` | End | Trustworthy recommendations require trustworthy confidence. |
| `SR_DS_27` | --- | decision | Override justification capture per `SR_GOV_18` when a user overrides an ADVISE recommendation. | `SR_GOV_18` | User override action | `OverrideInput` | Justification captured | JSON | `Result` | End | Aligns with D-19 / D-20. |
| `SR_DS_28` | --- | decision | CIA integration per D-47: every non-trivial recommendation can show upstream/downstream/lateral/second-order effects via `SR_INT_16`. | `SR_INT_16` | On user request or default for high-impact recs | `CiaInvocationInput` | CIA result attached to recommendation | JSON | `Result` | Returned to interface. | Transparency about cascading effects. |
| `SR_DS_29` | --- | decision | Multi-stakeholder recommendations per D-64: deliver to all stakeholders with per-stakeholder tracking; not "complete" until all respond. | `REUSABLE_RecipientResolver`, state machine | Recommendation affecting multiple roles | `MultiStakeholderInput` | Per-stakeholder delivery + tracking | JSON | `Result` | All responses required to mark complete. | Real organizational decisions are rarely single-person. |
| `SR_DS_30` | --- | decision | Noise management per D-65: per-recipient rate limits, batching for low-priority, pattern detection for ignored recommendations; user notified to adjust preferences. Cohort engagement signals from `SR_FW_05` are consumed here so at-risk users (declining engagement, repeated rejections) are throttled more aggressively. | Engagement monitor, `SR_FW_05` cohort signals | Continuous | `NoiseMgmtInput { tenant_id, recipient_id, cohort_state }` | Rate limit applied or alert; throttle rate adjusted by cohort risk | JSON | `Result { throttle_applied, cohort_state }` | End | Prevents recommendation fatigue, the most common cause of user disengagement. The cohort signal integration completes BP-122 by making the consumption explicit at the noise-management end. |

## Section 4 — Source Comparison, Forecast Feedback, Triggers, Format (SR_DS_31 through SR_DS_38)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DS_31` | --- | decision | Source system prediction comparison per D-44: compare platform's analysis to source-system predictions (Ekos forecast, Salesforce Einstein) by querying for DataCollections with `data_origin = system_prediction` (set in `SR_CONN_25` and stored in `SR_DM_07`); divergence >15% triggers alert via `SR_GOV_67`; both predictions shown to user. | Comparator, `data_origin` filter on DataCollection nodes | Inline within assembly | `SourceComparisonInput { tenant_id, query_context }` | Comparison summary including source predictions | JSON | `Result { platform_prediction, source_predictions[], divergence_pct }` | If divergence high: alert via `SR_GOV_67`. | Platform must acknowledge other prediction sources, not silently override. The dependency on `data_origin` field added in BP-129 makes the source predictions queryable. |
| `SR_DS_32` | --- | decision | Forecast component feedback per D-45: users provide feedback on individual components (demand, pricing, lead time, inventory); platform validates and quality-scores. Invalid feedback logged but not applied. | Feedback validator | User submits | `ForecastFeedbackInput` | Validated and quality-scored | JSON | `Result` | If valid: applied. Else: logged. | Prevents low-quality feedback from polluting forecasts. |
| `SR_DS_33` | --- | decision | Proactive triggers per D-60: 8 trigger types — threshold crossing, pattern detection, anomaly detection, external events, forecast horizon, data quality issues, coverage gap impact, learning loop insights. The trigger evaluators live in `SR_INT_24`; this SR is the Decision Support intake that converts trigger events into a new request entering at `SR_DS_01`. Proactively triggered recommendations are subject to coverage disclosure (`SR_GOV_71`) the same as user-initiated ones. | `SR_INT_24`, `SR_GOV_71` | Triggered by `SR_INT_24` | `TriggerInput { trigger_type, source_evidence }` | Recommendation request issued at `SR_DS_01` | JSON | `Result { request_id }` | `SR_DS_01` for the new request. | Proactive surfacing is a core value proposition. The bidirectional reference and explicit coverage check complete BP-103. |
| `SR_DS_34` | --- | decision | Configurable evaluation metrics per D-52: tenant-defined metrics, weighted, AI-monitored. AI recommends weight adjustments; decision makers approve. Conditional context (holiday vs off-season). | Metric evaluator, `REUSABLE_AgentFeedbackTracker` | Periodic + on-demand | `MetricEvalInput` | Metric report; weight adjustment proposals | JSON | `Result` | Decision maker approves or rejects. | Per-tenant metric flexibility supports vertical-specific decision making. |
| `SR_DS_35` | --- | decision | Event Calendar per D-55: business owners log upcoming events with expected business impact; events become regressors in forecasting. | `REUSABLE_GraphWriter` | User input + scheduled refresh | `EventCalendarInput` | Event nodes added; forecasts updated | JSON | `Result` | End | Manual context for events that affect forecasts but are not in operational data. |
| `SR_DS_36` | --- | decision | Recommendation accuracy tracking on data per D-56: update DataCollection.recommendation_track_record after outcome known. | `SR_INT_14` | After outcome known | `AccuracyUpdate` | Counters updated | JSON | `Result` | End | Data trust scoring. |
| `SR_DS_37` | --- | decision | Dashboard relevance tags per D-57: AI infers which dashboards a DataCollection is relevant to; admin confirms when adding to a dashboard. | T1 LLM | Periodic | `RelevanceInput` | Tags applied | JSON | `Result` | End | Enables proactive data-to-dashboard matching. |
| `SR_DS_38` | --- | decision | Response format template: every recommendation includes the structured fields from Spec 06 (justification, sources, parameters, confidence, alternatives, cascade impact, etc.). | Format validator | Inline at assembly | `FormatCheckInput` | Pass/fail | JSON | `FormatCheckResult` | If pass: deliver. Else: regenerate. | Consistent format builds user trust and enables transparency. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-107 | `SR_DS_03` (data gathering invokes CSA) | `SR_INT_18`, `SR_GOV_24` | Confirmed both: data gathering wraps CSA invocation. | 1 |
| BP-108 | `SR_DS_22` (recipient delivery) | `SR_DM_25` (notification_log) and `SR_DM_26` (preferences) | Confirmed: delivery writes notification rows and reads preference rows. | 1 |
| BP-109 | `SR_DS_31` (divergence alert >15%) | `SR_GOV_67` (alert routing) | Confirmed: divergence alerts use the standard severity matrix. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Insufficient-data response (`SR_DS_08`) — does it count as a recommendation for noise management? | No. It is informational and excluded from rate limits. |
| 2 | Multi-stakeholder recs (`SR_DS_29`) — what if one stakeholder leaves the company before responding? | Recipient set is recomputed on user-status changes; departed users are removed from the stakeholder list. |
| 3 | Forecast feedback validation (`SR_DS_32`) — what counts as "validated against system data"? | Implementation: numeric feedback within ±20% of system value = validated; 20-50% = documented; >50% = plausible only. Documented as default thresholds. |
| 4 | Auto-invalidation (`SR_DS_14`) — what counts as "significant data change"? | Implementation: any change to a parameter that influenced the recommendation by more than its weighted contribution. Documented as default. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_DS_03` | `SR_INT_18` | Data gathering |
| `SR_DS_05` | `SR_GOV_24` | CSA |
| `SR_DS_06` | `SR_LLM_01` | Analysis |
| `SR_DS_11` | `SR_DM_09` | Recommendation persistence |
| `SR_DS_15` | `SR_DM_10` + `SR_GOV_72` | Rejection persistence |
| `SR_DS_22` | `SR_DM_25` + `SR_DM_26` | Delivery + preferences |
| `SR_DS_25` | `SR_LLM_30` | DBE v2 |
| `SR_DS_28` | `SR_INT_16` | CIA |

## Spec 06 Summary

| Metric | Value |
|--------|-------|
| Sections | 4 |
| Main-flow SRs | 38 |
| Exception SRs | 0 (failures cascade through invoked SR_LLM_, SR_INT_, SR_GOV_) |
| Total SR rows | 38 |
| BP entries created | 3 (BP-107 through BP-109) |
| New decisions | 0 |

**Status:** Self-audit complete.

---

## Back-Propagation Received from Later Specs (applied retroactively in Session 2)

| BP | Originating Spec | Edit Applied to Spec 06 |
|----|-----------------|-------------------------|
| BP-107 | Spec 01 (`SR_GOV_74` preflight) | `SR_DS_05` updated to invoke full preflight via `SR_GOV_74` |
| BP-122 | Spec 10 (`SR_FW_05` cohort signals) | `SR_DS_30` updated to consume cohort engagement signals |
| BP-129 | Spec 02 (`SR_DM_07` `data_origin`) and Spec 03 (`SR_CONN_25` `data_origin`) | `SR_DS_31` updated to query for `data_origin = system_prediction` |
| BP-103 | Spec 04 (`SR_INT_24` proactive triggers) | `SR_DS_33` updated with bidirectional reference and explicit coverage check |

**Total retroactive edits to Spec 06: 4 SR row updates.**
