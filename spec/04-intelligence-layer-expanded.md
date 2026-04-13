# Spec 04 Expanded: Intelligence Layer

**Source:** `001/spec/04-intelligence-layer.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** 2 (core value)
**Reserved SR range:** `SR_INT_01` through `SR_INT_99`
**Last updated:** 2026-04-10

---

## Purpose

Turn the Intelligence Layer architectural decisions into implementation-ready SR rows: incremental graph growth, six-stage tagging pipeline, process emergence, coverage calculation, temporal intelligence, graph maintenance, cross-tenant aggregate learning, Cascade Impact Analysis (CIA), Semantic Disambiguation Agent (SDA), Agent Performance Feedback Loop, query governance, query performance safeguards, proactive triggers, disaster recovery, tenant offboarding.

## Architectural Decisions Covered

D-2 (incremental intelligence), D-46 (Research Agent), D-47 (CIA), D-48 (DataGroup), D-49 (tag weights), D-51 (agent feedback loop), D-53 (SDA), D-60 (proactive triggers), plus IL-1 through IL-10 unknown-unknown resolutions.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| Visibility Compartment | GAP-77 | `SR_INT_15` (semantic search post-filter) |
| Audit | GAP-71 | `SR_INT_30` (tenant offboarding certificate) |

## Integration Map

| Consumer Spec | Depends On |
|--------------|------------|
| Spec 01 Governance | `SR_INT_15` (query rewrite hook), `SR_INT_18` (CSA invocation), `SR_INT_22` (cross-tenant opt-in scope) |
| Spec 02 Data Model | `SR_INT_05` (DataSnapshot), `SR_INT_07` (TrendAnalysis), `SR_INT_09` (vector index), `SR_INT_25` (graph maintenance) |
| Spec 03 Connection | `SR_INT_02` (DataCollection ingest), `SR_INT_03` (semantic relationship inference) |
| Spec 05 LLM Routing | `SR_INT_03` (T1 invocation), `SR_INT_19` (Research Agent calls T2/T3) |
| Spec 06 Decision Support | `SR_INT_18` (data gathering), `SR_INT_24` (proactive trigger to recommendation) |
| Spec 07 Interface | `SR_INT_15` (semantic search), `SR_INT_20` (graph viz query path), `SR_INT_18` (CIA panel) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_GraphTraversal` | Cypher traversal with cost estimator, 30s timeout, 10K node result limit (IL-6) |
| `REUSABLE_CoverageCalculator` | Computes per-tenant coverage across 5 dimensions (system, process, data, department, relationship) |
| `REUSABLE_AgentFeedbackTracker` | D-51 — execution tracking, outcome scoring, weekly regression for every agent |

---

## Section 1 — Graph Growth and Six-Stage Tagging Pipeline (SR_INT_01 through SR_INT_08)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_INT_01` | --- | intelligence | Initialize an empty intelligence graph for a new tenant. The graph contains only the Tenant node and seed compartment/role nodes after `SR_DM_01` and `SR_DM_03`. | `REUSABLE_GraphWriter` | Called from `SR_DM_01` | `GraphInitInput { tenant_id }` | Empty per-tenant subgraph ready to receive nodes | JSON | `Result { ready }` | Graph grows from `SR_INT_02` onward. | Per D-2, the graph starts empty and grows through real work. |
| `SR_INT_02` | --- | intelligence | Receive a `DataCollection` (from `SR_DM_07`) and trigger Stages 3-6 of the tagging pipeline asynchronously per Spec 03. | Tagging worker pool | Triggered by `SR_DM_07` write | `DataCollectionRef { collection_id }` | Stage 3-6 jobs queued for the collection | JSON | `Result { jobs_queued }` | `SR_INT_03` (Stage 3) | Asynchronous tagging keeps the synchronous gate fast while ensuring full enrichment. |
| `SR_INT_03` | --- | intelligence | Stage 3 (semantic): T1 LLM invocation via Spec 05 router to infer semantic_type, business domain, unit, context for each DataField. | T1 LLM via `SR_LLM_` | Async from `SR_INT_02` | `SemanticTaggingInput { collection_id, fields[] }` | DataField properties updated | JSON | `Result { fields_tagged }` | `SR_INT_04` | Semantic tagging is the foundation for all later cross-system analysis (e.g., "product" vs "item" via SDA). |
| `SR_INT_04` | --- | intelligence | Stage 4 (relationship inference): pattern matching + T1 LLM proposes `SEMANTICALLY_EQUIVALENT`, `FEEDS`, `IMPACTS` candidate edges with confidence scores per D-27. | Pattern matcher, T1 LLM | After `SR_INT_03` | `RelationshipInferenceInput { collection_id }` | Candidate edges added with `confidence < 1.0`; high-confidence queued for review | JSON | `Result { edges_added, edges_queued }` | `SR_INT_05` | AI-inferred relationships enable cross-system insights but must be confidence-aware. |
| `SR_INT_05` | --- | intelligence | Create `DataSnapshot` nodes per the freshness policy (D-24) for trend analysis. | `REUSABLE_GraphWriter` | Scheduled per collection per policy | `SnapshotInput { collection_id, timestamp }` | DataSnapshot node + checksum + retention_until | JSON | `Result { snapshot_id }` | `SR_INT_07` (trend analysis when sufficient snapshots) | Without history, trend analysis is impossible (UU-1, IL-3). |
| `SR_INT_06` | --- | intelligence | Stage 5 (quality assessment): completeness, consistency, timeliness, uniqueness, accuracy estimation; produces `DataQualityReport` per D-25. | Quality computer | Async from `SR_INT_04` | `QualityInput { collection_id }` | `DataQualityReport` node with overall_score | JSON | `Result { report_id, score }` | `SR_INT_08` (low confidence to review queue) | Quality scoring feeds confidence weighting in Decision Support. |
| `SR_INT_07` | --- | intelligence | Compute `TrendAnalysis` from successive DataSnapshots: direction, magnitude, statistical significance. | Trend computer | Per schedule + on demand | `TrendInput { metric, snapshots[] }` | `TrendAnalysis` node with results | JSON | `Result { trend_id }` | End | Trends are core inputs to forecasting and proactive recommendations. |
| `SR_INT_08` | --- | intelligence | Stage 6 (human review queue): items with confidence <0.7 and security classifications go to a human review queue. | Review queue manager, `REUSABLE_AlertRouter` | After Stages 3-5 | `ReviewQueueInput { item_type, item_ref, confidence }` | Queue entry created; reviewer notified | JSON | `Result { queue_id }` | Reviewer acts; on resolution, the original SR re-fires. | Low-confidence items must not silently become facts. |

## Section 2 — Coverage, Process Mapping, Vector Search, and Maintenance (SR_INT_09 through SR_INT_15)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_INT_09` | --- | intelligence | Compute coverage across 5 dimensions per `REUSABLE_CoverageCalculator`: system, process, data, department, relationship. | `REUSABLE_CoverageCalculator` | On-demand + periodic refresh | `CoverageRequest { tenant_id }` | Per-dimension percentages and limitations | JSON | `CoverageResult { dimensions, limitations }` | Used by `SR_GOV_71` and `SR_DS_` for confidence weighting. | Without coverage transparency, recommendations can be silently overconfident on partial data (UU-13). |
| `SR_INT_10` | --- | intelligence | Discover processes from emergent patterns: when components and DataCollections share field overlap, FEEDS edges, and timing patterns, propose a `Process` node candidate. | Pattern matcher, `REUSABLE_GraphWriter` | Scheduled background job | `ProcessDiscoveryInput { tenant_id }` | `Process` node candidates queued for human confirmation | JSON | `Result { candidates }` | Reviewer confirms or rejects via `SR_UI_`. | Process emergence per D-2: processes are not manually defined; they appear from work patterns. |
| `SR_INT_11` | --- | intelligence | Add `MEMBER_OF` edges grouping DataCollections into `DataGroup` nodes per D-48. Groups are user-defined or AI-suggested. | `REUSABLE_GraphWriter` | User action or AI suggestion | `DataGroupingInput` | `MEMBER_OF` edges created | JSON | `Result` | End | DataGroups enable cross-collection analysis and tag inheritance. |
| `SR_INT_12` | --- | intelligence | Apply tag weights per D-49: security 1.0 > business 0.7 > technical 0.5; weights affect routing, confidence, and search ranking. Configurable per tenant. | Tag weight evaluator | Inline during recommendation generation | `TagWeightInput { tag_categories[] }` | Effective weights for the operation | JSON | `Result { weights }` | Caller proceeds. | Per-tenant weights let verticals tune the influence of different signals. |
| `SR_INT_13` | --- | intelligence | Apply completeness tags (D-50): each DataCollection carries `completeness_status` (full/partial/sampled) and `missing_fields[]` so Decision Support can weight partial data lower. | `REUSABLE_GraphWriter` | After Stage 5 quality assessment | `CompletenessTagInput { collection_id, status, missing_fields }` | Properties updated | JSON | `Result { tagged }` | End | Partial data must not be treated as full data. |
| `SR_INT_14` | --- | intelligence | Track recommendation accuracy on data: DataCollections accumulate `recommendation_track_record` (used_in_count, accurate_count, accuracy_rate) per D-56. | `REUSABLE_GraphWriter` | After recommendation outcome known | `AccuracyUpdateInput { collection_id, outcome }` | Counters incremented | JSON | `Result { updated }` | End | Data trust scoring helps decision support prefer reliable sources. |
| `SR_INT_15` | --- | intelligence | Vector semantic search with post-filter per IL-7: query vector → top N candidates from vector index (`SR_DM_18`) → apply governance filter via `SR_GOV_33` compartment check (and `SR_GOV_77` query rewrite for the underlying graph fetches) → return survivors. Invoked from `SR_UI_22`. | `REUSABLE_VectorIndexer` reader, `SR_GOV_33` (compartment check), `SR_GOV_77` (query rewrite) | User search action via `SR_UI_22` | `SemanticSearchInput { tenant_id, principal, query_vector, top_k }` | Filtered top results with provenance and per-result compartment-check trace | JSON | `SemanticSearchResult { results[], filtered_count, dropped_for_compartment_count }` | Returned to `SR_UI_22`. | Post-filter approach prevents leaking forbidden documents through similarity matches. Bidirectional reference to `SR_GOV_33` and `SR_UI_22` completes BP-101. |

## Section 3 — Cascade Impact Analysis, SDA, Research Agent, Cross-Tenant Learning, Triggers, DR (SR_INT_16 through SR_INT_30)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_INT_16` | --- | intelligence | Cascade Impact Analysis (CIA) per D-47: traces upstream, downstream, lateral, and second-order effects of any change or event through `IMPACTS` edges. | `REUSABLE_GraphTraversal` | User-initiated or proactive | `CiaRequest { tenant_id, source_node, depth, include_confidence }` | Impact tree with confidence per branch | JSON | `CiaResult { tree, confidence, coverage_disclosure }` | Returned to caller. | CIA is the named feature that answers "how does this affect everything else" — a primary value proposition. |
| `SR_INT_17` | --- | intelligence | Semantic Disambiguation Agent (SDA) per D-53: discover synonyms, shorthand, acronyms across systems and users; maintain `semantic_dictionary`. | `REUSABLE_GraphWriter`, T1 LLM | Periodic + on-demand | `SdaRunRequest { tenant_id }` | Updated semantic_dictionary | JSON | `SdaResult { added, modified }` | Used by query expansion. | Without SDA, cross-system queries silently fail due to terminology mismatches. |
| `SR_INT_18` | --- | intelligence | Decision Support data gathering: identify required data, check freshness, refresh if stale, check quality, include source system predictions, pull external context from Research Agent, check Event Calendar, run CSA via `SR_GOV_24`. | All Intelligence Layer SRs above + Spec 06 callers | Inline call from Decision Support | `DataGatheringInput { query, parameters }` | Aggregated data set with freshness, quality, provenance | JSON | `DataGatheringResult { data, freshness, gaps }` | Decision Support proceeds. | Centralizes the "gather data for analysis" pattern so it consistently applies all governance and quality checks. |
| `SR_INT_19` | --- | intelligence | Research Agent (D-46): periodically gather external context (market, news, regulatory, commodity, weather) and store as DataCollections with `data_origin: research_agent`. | External APIs (via Spec 03 connections), T2/T3 LLM via Spec 05 | Periodic + on-demand | `ResearchInput { tenant_id, topics[] }` | New DataCollection with external context | JSON | `ResearchResult { collection_id }` | Data flows into normal Intelligence Layer pipeline. | External context grounds forecasts in reality (weather, commodity prices, regulations). |
| `SR_INT_20` | --- | intelligence | Process map graph visualization data: return nodes and edges within a depth limit for the Interface graph viz panel. | `REUSABLE_GraphTraversal`, `SR_DM_27` query rewrite | Interface request | `GraphVizRequest { tenant_id, focal_node, depth }` | Bounded subgraph | JSON | `GraphVizResult { nodes[], edges[] }` | Returned to interface. | Graph viz must be fast and bounded to prevent browser crashes. |
| `SR_INT_21` | --- | intelligence | Agent Performance Feedback Loop per D-51: tracking, outcome scoring, weekly regression testing, improvement loop, cross-validation across all agents (tagging, routing, research, quality, discovery). | `REUSABLE_AgentFeedbackTracker` | Continuous + weekly batch | `AgentFeedbackCycleRequest` | Per-agent quality scores; rules/prompts/models updated; cross-validation report | JSON | `Result { agents_evaluated, improvements }` | End | The platform improves its own agents the way it improves model routing. |
| `SR_INT_22` | --- | intelligence | Cross-tenant aggregate learning per BP-31: opt-in only (governed by `SR_GOV_59`); share patterns (vertical benchmarks, common process templates, model performance benchmarks, data quality benchmarks) — never raw data. Workers read the opt-in state from `SR_GOV_59` before each aggregation cycle and respect toggles within one cycle (max 24 hours). | `REUSABLE_TenantFilter`, `SR_GOV_59` opt-in check | Periodic aggregation (max once per 24 hours) | `CrossTenantAggregationInput { tenant_set, opt_in_verified_at }` | Aggregated patterns; no raw data leakage; rejected if opt-in toggled off since last verification | JSON | `Result { patterns, opt_in_state }` | End | Provides cross-tenant insights without violating tenant isolation. Bidirectional reference to `SR_GOV_59` completes BP-126. |
| `SR_INT_23` | --- | intelligence | Query rewriter (IL-1): all Cypher queries pass through `SR_DM_27` to inject tenant_id and role-based filters; raw Cypher from interface is forbidden. | `SR_DM_27` | Inline call from any query path | `QueryInput { raw_cypher, principal }` | Rewritten query or rejection | JSON | `QueryRewriteResult` | Caller executes rewritten query. | Tenant isolation is enforced at the rewrite boundary. |
| `SR_INT_24` | --- | intelligence | Proactive trigger evaluation per D-60: scheduled jobs detect threshold crossings, pattern changes, anomalies, external events, forecast horizons, data quality issues, coverage gap impact, learning loop insights. On detection, fire a recommendation request to Decision Support. | Trigger evaluators (8 types) | Scheduled per trigger type | `ProactiveTriggerRequest { trigger_type }` | New recommendation request issued to Decision Support | JSON | `Result { triggers_fired }` | `SR_DS_` recommendation generation. | Proactive surfacing of insights is a core value proposition; the platform does not just answer questions. |
| `SR_INT_25` | --- | intelligence | Graph maintenance per `SR_DM_24`: stale data pruning daily; orphan node cleanup weekly; index optimization weekly; embedding refresh on model change; AI-inferred edge confidence decay monthly; trend recomputation per schedule; backup daily incremental + weekly full; tenant isolation audit weekly. This SR is the operational entry point; `SR_DM_24` is the data-model substrate. | Maintenance scheduler, `REUSABLE_GraphTraversal`, `SR_DM_24` | Scheduled jobs | `MaintenanceRequest { cycle_type }` | Per-cycle counts and any anomalies; results delegated to `SR_DM_24` for the actual store-level operations | JSON | `MaintenanceResult` | End | Maintenance preserves performance, isolation, and data hygiene. The substrate/operation split is recorded in BP-130. |
| `SR_INT_26` | --- | intelligence | Query cost estimator (IL-6): rejects expensive queries before execution; 30s timeout; 10K node result limit; caching with 5-minute TTL for common dashboards. Also enforces per-tenant resource quotas via `REUSABLE_QuotaEnforcer` (per `SR_SCALE_25`) so a single tenant cannot starve the cluster with cheap-but-numerous queries. | Cost estimator, query cache, `REUSABLE_QuotaEnforcer` | Inline before query execution | `CostEstimateInput { tenant_id, query }` | ALLOW with estimated cost OR REJECT (with reason: cost \| quota) | JSON | `CostEstimateResult { allowed, estimated_cost_ms, quota_remaining }` | Caller proceeds or fails fast. | Performance cliff prevention. The `REUSABLE_QuotaEnforcer` reference makes the per-tenant quota gate explicit (BP-128). |
| `SR_INT_27` | --- | intelligence | Bulk import worker integration with Intelligence Layer per IL-8: dedicated background queue ensures bulk imports do not starve interactive workloads. | Bulk worker pool | Triggered from `SR_CONN_17` | `BulkImportProcessing` | Imports proceed without affecting interactive queries | JSON | `Result` | End | Performance isolation between batch and interactive paths. |
| `SR_INT_28` | --- | intelligence | Read-through cache with TTL per IL-9: common queries cached for the dashboard; cache invalidated on relevant writes. Serves as the Neo4j-read fallback in `SR_SCALE_30` graceful degradation chain — when reads are degraded the cache TTL is extended automatically and users are informed. | In-memory cache + invalidator, `REUSABLE_DegradationChain` participation | Inline | `CacheRequest { key, query, ttl, degradation_mode? }` | Cached result or fresh result; under degradation, cached result with stale-data badge | JSON | `CacheResult { source, freshness_seconds, degradation_active }` | End | Reduces graph load; freshness guaranteed by invalidation. Graceful degradation participation completes BP-120 for the Neo4j-reads role. |
| `SR_INT_29` | --- | intelligence | Disaster recovery drills: scheduled DR tests verifying the specific RTO/RPO targets defined in `SR_SCALE_40` — Neo4j primary fails <5 min RTO / <30 sec RPO; Neo4j cluster loss <2 hr / <24 hr; PostgreSQL failover <2 min / <10 sec; both lost <4 hr / <24 hr; event bus failure <5 min / events in-flight. Failed drills escalate per `SR_GOV_67` alert routing. | DR runbook + test environment, `SR_SCALE_40` target table, `REUSABLE_Alerter` | Scheduled (quarterly) | `DrDrillRequest { scenario, target_rto, target_rpo }` | Drill outcome with measured RTO/RPO and pass/fail per `SR_SCALE_40` targets | JSON | `DrDrillResult { passed, measured, target, escalation_id? }` | End. Failed drill triggers incident ticket and remediation plan; next drill must pass before previous is closed. | Drills validate that the documented DR targets are real. Bidirectional reference to `SR_SCALE_40` completes BP-121. |
| `SR_INT_30` | --- | intelligence | Tenant offboarding: complete data removal across Neo4j, PostgreSQL, vector embeddings, event bus streams, object storage backups, model state. Verified by automated scan. Certificate of deletion issued. The crypto-shredding step is delegated to `SR_GOV_52` (which invokes `SR_DM_30`) for all per-subject encryption keys belonging to the tenant before the bulk delete proceeds. | All store delete paths, scanner, signing key, `SR_GOV_52` for crypto-shred, `SR_DM_30` for marking | Admin offboarding | `OffboardingRequest { tenant_id, confirm_all_subjects }` | Crypto-shred via `SR_GOV_52` first; all data removed; verification scan PASS; certificate of deletion issued | JSON | `OffboardingResult { certificate_url, shred_certificates[] }` | End | Provides defensible erasure for tenants leaving the platform. The explicit `SR_GOV_52` and `SR_DM_30` invocations complete BP-102. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-100 | `SR_INT_18` (data gathering invokes CSA) | `SR_GOV_24` | Confirmed `SR_GOV_24` accepts inline data-gathering invocation. Added documentation: data gathering must run CSA before returning combined data. | 1 |
| BP-101 | `SR_INT_15` (semantic search post-filter) | `SR_GOV_33` (compartment check) | Confirmed: semantic search relies on `SR_GOV_33` for compartment filtering as the second step (after similarity match, before return). | 1 |
| BP-102 | `SR_INT_30` (tenant offboarding) | `SR_GOV_52` (crypto-shredding) | Confirmed: offboarding invokes crypto-shredding for all per-subject keys belonging to the tenant before deletion. | 1 |
| BP-103 | `SR_INT_24` (proactive trigger fires recommendations) | `SR_GOV_71` (coverage disclosure) | Confirmed: proactively triggered recommendations also require coverage disclosure; no special-case bypass. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Cache invalidation (`SR_INT_28`) — what triggers invalidation? | Implementation: write events on tenant subgraph publish to a "cache invalidation" topic; subscribers in the cache layer evict matching keys. Acknowledged as detail. |
| 2 | Process discovery (`SR_INT_10`) creates candidates for human confirmation — what if confirmation never happens? | Candidates expire after 30 days unconfirmed and are demoted to `discarded`. Documented as default. |
| 3 | Cross-tenant aggregation (`SR_INT_22`) — how is "no raw data" enforced? | The aggregation worker reads only from materialized aggregates, never from raw nodes. Hard architectural boundary. |
| 4 | DR drills (`SR_INT_29`) — how are drill failures escalated? | Failed drill results in incident ticket and remediation plan; next drill must pass before the previous one is closed. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_INT_02` | `SR_DM_07` | DataCollection creation |
| `SR_INT_05` | `SR_DM_24` | Snapshot maintenance |
| `SR_INT_15` | `SR_GOV_33` | Compartment check on search results |
| `SR_INT_18` | `SR_GOV_24` | CSA invocation in data gathering |
| `SR_INT_23` | `SR_DM_27` | Query rewrite |
| `SR_INT_24` | `SR_DS_` | Proactive recommendations |
| `SR_INT_30` | `SR_GOV_52` + `SR_DM_30` | Crypto-shredding during offboarding |

## Spec 04 Summary

| Metric | Value |
|--------|-------|
| Sections | 3 |
| Main-flow SRs | 30 |
| Exception SRs | 0 (intelligence layer ops are mostly idempotent and bounded; failures cascade through the SE rows in the underlying SR_DM_ and SR_LLM_ layers) |
| Total SR rows | 30 |
| BP entries created | 4 (BP-100 through BP-103) |
| New decisions | 0 |

**Status:** Self-audit complete.

---

## Back-Propagation Received from Later Specs (applied retroactively in Session 2)

| BP | Originating Spec | Edit Applied to Spec 04 |
|----|-----------------|-------------------------|
| BP-101 | Spec 07 (`SR_UI_22` semantic search caller) | `SR_INT_15` updated with bidirectional reference and explicit `SR_GOV_33` post-filter call |
| BP-126 | Spec 01 (`SR_GOV_59` opt-in) | `SR_INT_22` updated with bidirectional reference and per-cycle verification |
| BP-130 | Spec 02 (`SR_DM_24` substrate) | `SR_INT_25` updated to mark substrate/operation split |
| BP-128 | Spec 13 (`SR_SCALE_25` per-tenant quotas) | `SR_INT_26` updated to invoke `REUSABLE_QuotaEnforcer` |
| BP-120 | Spec 13 (`SR_SCALE_30` graceful degradation) | `SR_INT_28` updated with degradation participation as the Neo4j-reads cache fallback |
| BP-121 | Spec 13 (`SR_SCALE_40` DR targets) | `SR_INT_29` updated with explicit RTO/RPO numbers |
| BP-102 | Spec 01 (`SR_GOV_52`) and Spec 02 (`SR_DM_30`) | `SR_INT_30` updated to explicitly invoke crypto-shred chain during offboarding |

**Total retroactive edits to Spec 04: 7 SR row updates.**
