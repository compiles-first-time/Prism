# Spec 02 Expanded: Data Model

**Source:** `001/spec/02-data-model.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** Foundation (referenced by every other spec)
**Reserved SR range:** `SR_DM_01` through `SR_DM_99`
**Last updated:** 2026-04-10

---

## Purpose

Turn the data model architectural decisions from Spec 02 into implementation-ready SR rows describing the schemas, sync layer, vector index, multi-tenant isolation, and the lifecycle operations that create, read, update, and retire schema artifacts. Implementation tasks for schema migration, query rewriting, sync workers, vector index management, and tenant isolation audits are all defined here.

## Architectural Decisions Covered

D-1 (Neo4j + PostgreSQL hybrid), D-22 (event-sourced + Merkle audit), D-23 (DataField enrichment), D-24 (DataSnapshot), D-25 (DataQualityReport), D-26 (real-time streaming), D-27 (confidence on edges), D-28 (LLM observability), D-33 (embedding rollback), D-48 (DataGroup), D-49 (tag weights), D-50 (completeness tags), D-56 (recommendation accuracy on data), D-57 (dashboard relevance tags), plus BPs BP-04, BP-08, BP-10, BP-11, BP-12, BP-13, BP-14, BP-15, BP-17, BP-18, BP-20, BP-21, BP-22, BP-25, BP-43, BP-44, BP-48, BP-49, BP-53, BP-54, BP-62, BP-63, BP-66, BP-68, BP-69, BP-70, BP-78, BP-79, BP-80, BP-83, BP-84, BP-85, BP-91.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| Crypto-shredding (per-subject AES-256 + KMS) | `COMP § 3.3.3` | `SR_DM_30`, `SR_DM_31` |
| Audit trail event-sourced + Merkle | GAP-71 | `SR_DM_05`, `SR_DM_06` |
| Visibility Compartment | GAP-77 | `SR_DM_03` (compartment node), `SR_DM_27` (queries) |

## Integration Map (Other Specs Depending on SR_DM_)

| Consumer Spec | Depends On |
|--------------|------------|
| Spec 01 Governance | `SR_DM_05`, `SR_DM_06`, `SR_DM_27`, `SR_DM_30`, `SR_DM_31` |
| Spec 03 Connection | `SR_DM_07` (DataCollection write), `SR_DM_08` (DataField write), `SR_DM_22` (sync), `SR_DM_28` (compartment check) |
| Spec 04 Intelligence | `SR_DM_18` (vector index), `SR_DM_19` (vector rollback), `SR_DM_24` (graph maintenance) |
| Spec 05 LLM Routing | `SR_DM_15` (ModelExecution), `SR_DM_16` (ModelOutcomeScore), `SR_DM_17` (model_performance_analytics), `SR_DM_13` (model_registry rows written by `SR_LLM_40`) |
| Spec 06 Decision Support | `SR_DM_09` (Recommendation node), `SR_DM_10` (Rejection node), `SR_DM_11` (rec lifecycle states) |
| Spec 07 Interface | `SR_DM_25` (notification_log), `SR_DM_26` (user_preferences), `SR_DM_29` (feature_flags) |
| Spec 08 Component Catalog | `SR_DM_12` (Component), `SR_DM_13` (component_registry), `SR_DM_14` (component_performance) |
| Spec 09 SA Catalog | `SR_DM_20` (ServiceAccount), `SR_DM_21` (sa_usage_log) |
| Spec 13 Scalability | `SR_DM_22` (sync layer), partition strategies for `SR_DM_06` |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_GraphWriter` | Idempotent Neo4j writer using MERGE; tagged with tenant_id |
| `REUSABLE_GraphReader` | Tenant-filtered Neo4j reader with cost estimator |
| `REUSABLE_PgWriter` | PostgreSQL transactional writer with explicit retry policy |
| `REUSABLE_PgReader` | PostgreSQL reader with row-level security enforcement |
| `REUSABLE_TenantBoundaryEnforcer` | Inserts TENANT_BOUNDARY edge on every node creation |
| `REUSABLE_VectorIndexer` | Embeds text via current embedding model and writes to Neo4j vector index |
| `REUSABLE_DualEmbeddingStore` | Stores embeddings under both old and new model identifiers during canary windows |
| `REUSABLE_SyncCoordinator` | Drives event-driven sync between Neo4j and PostgreSQL with eventual-consistency tracking |

---

## Section 1 — Core Entity Lifecycle (SR_DM_01 through SR_DM_10)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DM_01` | --- | data-model | Create a `Tenant` node in Neo4j and a paired `tenants` row in PostgreSQL inside a coordinated transaction; both rows carry identical `tenant_id` UUIDv7 values for cross-store correlation. | `REUSABLE_GraphWriter`, `REUSABLE_PgWriter`, `REUSABLE_SyncCoordinator` | Called from `SR_GOV_01` | `TenantNodeInput { tenant_id, name, vertical, iam_provider, onboard_date, compliance_profile }` | Tenant node + row created with TENANT_BOUNDARY self-edge; sync coordinator marks state CONSISTENT | `TenantNodeInput` (JSON) | `TenantNodeResult { tenant_id, neo4j_node_id, pg_row_id }` | Returned to `SR_GOV_01` | Hybrid store atomicity is impossible without coordination; the sync coordinator records the eventual-consistency window. |
| `SR_DM_01_SE-01` | SE | data-model | Either Neo4j or PostgreSQL write fails after the other has succeeded. | Compensating transaction worker, `REUSABLE_SyncCoordinator` | Driver exception in second write | Same `TenantNodeInput` | Compensating delete on the successful side; `SR_GOV_01_SE-01` retry semantics apply to the caller | Same | `TenantNodeResult { state: "rolled_back", reason }` | Caller retries `SR_GOV_01`. | Without compensation, partial state would create silent multi-tenant leakage. |
| `SR_DM_02` | --- | data-model | Create a `Person` node in Neo4j and a paired row in PostgreSQL `persons` table when an IAM webhook or platform-managed invitation accepts. | `REUSABLE_GraphWriter`, `REUSABLE_PgWriter`, `REUSABLE_TenantBoundaryEnforcer` | Called from `SR_GOV_03` (IAM webhook) or `SR_GOV_08` (standalone auth) | `PersonNodeInput { tenant_id, iam_object_id, email, display_name, status, roles[], compartments[], department }` | Person node + row created with TENANT_BOUNDARY edge | `PersonNodeInput` (JSON) | `PersonNodeResult { person_id }` | Returned to caller | Person is the identity anchor for every governance and audit action. |
| `SR_DM_03` | --- | data-model | Create a `Compartment` node and a paired `compartments` row when `SR_GOV_31` requests compartment creation. | `REUSABLE_GraphWriter`, `REUSABLE_PgWriter`, `REUSABLE_TenantBoundaryEnforcer` | Called from `SR_GOV_31` | `CompartmentNodeInput { tenant_id, name, classification_level, member_roles[], member_persons[], purpose, criminal_penalty_isolation }` | Compartment node + row | `CompartmentNodeInput` (JSON) | `CompartmentNodeResult { compartment_id }` | Returned to caller | Compartments must exist as first-class graph nodes so traversals can enforce membership rules per `SR_GOV_33`. |
| `SR_DM_04` | --- | data-model | Create a `Connection` node in Neo4j paired with `connections` PostgreSQL row when a connection is approved per `SR_GOV_70`. The same node schema is used for cloud LLM providers (`SR_CONN_40`, `SR_LLM_42`); the `metadata` bag carries provider-specific fields like `provider_type`, `deprecation_at`, `failover_chain[]`. | `REUSABLE_GraphWriter`, `REUSABLE_PgWriter`, `REUSABLE_TenantBoundaryEnforcer` | Called from `SR_GOV_70` (any external connection including cloud LLM providers via `SR_CONN_40`) | `ConnectionNodeInput { tenant_id, system_id, connection_type, auth_type, credential_caas_ref, status, scope, metadata: { provider_type?, deprecation_at?, failover_chain?[], paywall?: bool } }` | Connection node + row | `ConnectionNodeInput` (JSON) | `ConnectionNodeResult { connection_id }` | Returned to caller. Subsequent `SR_CONN_` rows manage lifecycle transitions. | Connections are governed first-class entities; storing both in Neo4j and PostgreSQL enables both relational queries and graph traversals over connection topology. The same node serves cloud LLM providers per BP-99. |
| `SR_DM_05` | --- | data-model | Append an `audit_events` row using the schema {`event_id`, `tenant_id`, `chain_position`, `prev_event_hash`, `event_hash`, `event_type`, `principal`, `target`, `severity`, `payload_jsonb`, `signed_by_kid`, `created_at`}; the table is append-only and partitioned by `tenant_id + month` per Spec 13 Scalability. | `REUSABLE_PgWriter`, `REUSABLE_MerkleChainHasher` (from Spec 01) | Called from `SR_GOV_47` | `AuditEventRow` | Row inserted; partition handled transparently | `AuditEventRow` (JSON) | `AuditEventResult { event_id, chain_position }` | Returned to `SR_GOV_47`. | The write path that powers `SR_GOV_47` must be a separate SR because the schema, partitioning, and chain integrity are data-model concerns. |
| `SR_DM_06` | --- | data-model | Maintain the `audit_events` partition strategy: monthly partition creation, retention pruning per tenant policy, and partition pruning for long-term archive offload. | Partition manager job, retention policy lookup | Scheduled monthly job | `AuditPartitionMaintenanceRequest { tenant_id, period }` | Old partitions archived or dropped per policy | `AuditPartitionMaintenanceRequest` (JSON) | `AuditPartitionMaintenanceResult { archived_count, dropped_count }` | End | Without partition maintenance, the audit table becomes unmanageable at scale; partitioning keeps queries bounded and enables efficient retention enforcement. |
| `SR_DM_07` | --- | data-model | Create a `DataCollection` node when a connection produces a normalized `ExecutionRecord` (per `SR_CONN_25`); the node carries `source_system`, `pull_timestamp`, `freshness_policy`, `record_count`, `ingestion_method`, `source_file_ref`, `training_consent`, and `data_origin` (one of: `connection_pull`, `user_upload`, `log_stream`, `bulk_import`, `system_prediction`, `research_agent`). | `REUSABLE_GraphWriter`, `REUSABLE_TenantBoundaryEnforcer` | Called from `SR_CONN_27` Stage 2 ALLOW (after security classification) | `DataCollectionInput { tenant_id, connection_id, source_system, pull_timestamp, freshness_policy, record_count, ingestion_method, source_file_ref, training_consent, data_origin }` | `DataCollection` node created with `SOURCED_FROM` edge to the System and `COLLECTED_BY` edge to the responsible Component | `DataCollectionInput` (JSON) | `DataCollectionResult { collection_id }` | `SR_INT_02` async tagging Stages 3-6 follow, also writing through `SR_DM_08`. | DataCollection is the unit that anchors freshness, retention, quality, and recommendation traceability — every later spec references it. The `data_origin` field supports `SR_DS_31` source-system prediction comparison (BP-129). |
| `SR_DM_08` | --- | data-model | Upsert `DataField` nodes for every field in a DataCollection with the enriched schema from D-23: `technical_type`, `semantic_type`, `format`, `unit`, `classification`, `sensitivity_level`, `source_context`, `collection_purpose`, `freshness_policy`, `completeness_pct`, `accuracy_score`, `volatility`, `dwell_time_avg_ms`, `schema_version`. | `REUSABLE_GraphWriter`, MERGE on `(collection_id, field_name)` | Called from `SR_DM_07` and on every subsequent pull (idempotent) | `DataFieldInputBatch { tenant_id, collection_id, fields[] }` | DataField nodes created or updated; `CONTAINS_FIELD` edges from collection; co-occurrence tracked via `CO_COLLECTED_WITH` edges (BP-14) | `DataFieldInputBatch` (JSON) | `DataFieldBatchResult { upserted_count }` | Triggers Stage 3-6 of the classification gate per Spec 03. | Fields are the queryable atoms of the intelligence layer; the enriched schema enables every later analytical use case. |
| `SR_DM_09` | --- | data-model | Create a `Recommendation` node and pair it with the `recommendation_audit` row in PostgreSQL when Decision Support delivers a recommendation. | `REUSABLE_GraphWriter`, `REUSABLE_PgWriter` | Called from `SR_DS_` delivery rows | `RecommendationNodeInput { tenant_id, content_hash, model_used, confidence, parameters_used[], state, expires_at, prerequisites[], triggers[], stakeholders[], cooling_off_until, category }` | Node + audit row | `RecommendationNodeInput` (JSON) | `RecommendationNodeResult { rec_id, audit_row_id }` | Returned to Decision Support. | Recommendation is a first-class entity tracked through 12 lifecycle states (D-59); both stores hold its identity for graph traversal and relational reporting. |
| `SR_DM_10` | --- | data-model | Persist a `Rejection` node when a user rejects a recommendation per `SR_GOV_72`. | `REUSABLE_GraphWriter` | Called from `SR_GOV_72` | `RejectionNodeInput { tenant_id, recommendation_id, category, justification_text, person_id, timestamp }` | Rejection node with `JUSTIFIED_BY` edge to recommendation, `RESPONDED_WITH` edge from person | `RejectionNodeInput` (JSON) | `RejectionNodeResult { rejection_id }` | Returned to `SR_GOV_72`. | Captures the structured rejection signal that drives D-19 learning loop. |

## Section 2 — Operational Entities (SR_DM_11 through SR_DM_21)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DM_11` | --- | data-model | Transition a `Recommendation` node through any of the 12 lifecycle states per D-59 with persistence and validation that the transition is legal. | State machine validator, `REUSABLE_GraphWriter` | Triggered by user response or expiration | `RecommendationStateChange { tenant_id, rec_id, from_state, to_state, reason }` | State updated; transition row written to `recommendation_outcomes` (BP-69); event published to bus | `RecommendationStateChange` (JSON) | `StateChangeResult { new_state }` | Returned to caller. | Strict state machine prevents impossible transitions (e.g., REJECTED → ACTED) and maintains auditability. |
| `SR_DM_11_BE-01` | BE | data-model | Attempted illegal transition (e.g., from EXPIRED back to PENDING). | State machine validator | Validation check | Same | Rejected with 409 | Same | `StateChangeResult { error: illegal_transition, allowed_transitions[] }` | End | Hard rule: lifecycle state machine is authoritative. Any caller making an illegal transition has a logic bug. |
| `SR_DM_12` | --- | data-model | Create a `Component` node when a component is registered (manual, AI-generated, or forked) per Spec 08. | `REUSABLE_GraphWriter`, `REUSABLE_TenantBoundaryEnforcer` | Called from `SR_CAT_` registration rows | `ComponentNodeInput { tenant_id, component_id, category, version, git_sha, status, metadata }` | Component node with `BELONGS_TO_PROCESS` and `DEPENDS_ON` edges as applicable | `ComponentNodeInput` (JSON) | `ComponentNodeResult { node_id }` | Returned to caller. | Components are first-class graph entities so dependency analysis and impact tracing work via graph queries. |
| `SR_DM_13` | --- | data-model | Persist `component_registry` row in PostgreSQL with full versioning, semantic version, git_sha, status, ownership, scope. | `REUSABLE_PgWriter` | Called from `SR_DM_12` | `ComponentRegistryRow` | Row inserted | `ComponentRegistryRow` (JSON) | `ComponentRegistryResult { row_id }` | End | Relational store provides fast version lookups and rollback queries. |
| `SR_DM_14` | --- | data-model | Persist `component_performance` rows after every component execution: execution_count, latency, success/failure, cost. | `REUSABLE_PgWriter` | Called from component runtime after each execution | `ComponentPerformanceRow` | Row inserted | `ComponentPerformanceRow` (JSON) | `ComponentPerformanceResult { row_id }` | End | Provides the time-series substrate for the auto-rollback rule (`success_rate < 95%`) and capacity planning. |
| `SR_DM_15` | --- | data-model | Create a `ModelExecution` node for every LLM invocation per D-28: model_id, slot, task_type (one of `inference`, `tagging`, `verification`, `training`, `evaluation`), input_tokens, output_tokens, latency_ms, cost_usd, data_sensitivity, training_run_id (when task_type=`training` per `SR_LLM_50`). | `REUSABLE_GraphWriter` | Called from `SR_LLM_15` after every invocation; also from `SR_LLM_50` for fine-tune training runs | `ModelExecutionInput { tenant_id, fields, training_run_id? }` | Node created with `PROCESSED_BY` edge from involved DataCollections, `PRODUCED` edge to outputs | `ModelExecutionInput` (JSON) | `ModelExecutionResult { execution_id }` | Returned to LLM Router or training pipeline. | Per-invocation telemetry is the foundation of Flywheel 3 (Model Intelligence). The `training` task_type is required so fine-tune lifecycle is observable through the same telemetry path (BP-106). |
| `SR_DM_16` | --- | data-model | Score a `ModelExecution` outcome later when the user response or verification result is known per D-28. | `REUSABLE_GraphWriter` | Called when outcome becomes known (user accept/reject, verification pass/fail) | `ModelOutcomeInput { tenant_id, execution_id, outcome_type, outcome_value, quality_score }` | `ModelOutcomeScore` node + `SCORED_BY` edge | `ModelOutcomeInput` (JSON) | `ModelOutcomeResult { score_id }` | End | Outcome scoring closes the loop between invocation and quality, feeding routing improvements. |
| `SR_DM_17` | --- | data-model | Periodically aggregate `ModelExecution` and `ModelOutcomeScore` into `model_performance_analytics` rows for fast lookup by Router Stage 2. | Aggregation worker, `REUSABLE_PgWriter` | Hourly scheduled job | `ModelAggregationRequest { tenant_id, period }` | Rows in `model_performance_analytics` updated | `ModelAggregationRequest` (JSON) | `ModelAggregationResult { rows_updated }` | End | Pre-aggregation makes routing decisions fast (read-optimized) without re-scanning per request. |
| `SR_DM_18` | --- | data-model | Embed text content (documents, memos, reports, recommendations, queries) into the Neo4j vector index using the active embedding model; tag the embedding with `embedding_model_id` and `embedding_version`. | `REUSABLE_VectorIndexer`, current embedding model | Called from Connection Layer (documents) and Decision Support (recommendations) | `EmbeddingInput { tenant_id, source_node_id, text, model_id }` | Vector property attached to source node; index updated | `EmbeddingInput` (JSON) | `EmbeddingResult { vector_dim, model_id, embedded_at }` | End | Vector index enables semantic search; per-node tagging supports rollback (BP-44, D-33). |
| `SR_DM_19` | --- | data-model | During an embedding model canary, store both old and new embeddings on every embedded node per `REUSABLE_DualEmbeddingStore`; rollback simply switches the active model_id pointer. | `REUSABLE_DualEmbeddingStore` | Active during canary windows | `DualEmbeddingInput { tenant_id, source_node_id, old_embedding, new_embedding, old_model, new_model }` | Both embeddings persisted; rollback meta updated | `DualEmbeddingInput` (JSON) | `DualEmbeddingResult { dual_active_until }` | End | D-33 zero-downtime rollback requires both embeddings to be present during the transition. |
| `SR_DM_20` | --- | data-model | Create a `ServiceAccount` node when an SA is provisioned per `SR_SA_` rows. | `REUSABLE_GraphWriter`, `REUSABLE_TenantBoundaryEnforcer` | Called from `SR_SA_` provisioning | `ServiceAccountNodeInput { tenant_id, sa_id, name, type, system_ref, credential_caas_ref, permission_level, status, health_score }` | `ServiceAccount` node + `ACCESSES_SYSTEM` edges | `ServiceAccountNodeInput` (JSON) | `ServiceAccountNodeResult { node_id }` | Returned to `SR_SA_`. | First-class graph node enables anomaly detection, blast-radius analysis, and quarterly review queries. |
| `SR_DM_21` | --- | data-model | Append rows to `sa_usage_log` and `sa_anomalies` tables in PostgreSQL for SA monitoring. | `REUSABLE_PgWriter` | Triggered by SA usage events and anomaly detector | `SaUsageEvent` / `SaAnomalyEvent` | Rows inserted | JSON | `Result { row_id }` | End | Time-series usage and anomaly history powers `SR_GOV_63` quarterly review. |

---

## Section 3 — Sync Layer, Vector Index, and Streaming (SR_DM_22 through SR_DM_26)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DM_22` | --- | data-model | Drive the event-driven sync between Neo4j and PostgreSQL: governance rule changes flow PG → Neo4j within 5 seconds; recommendation lifecycle and connection health flow Neo4j → PG within 5-10 seconds; tenant boundary changes are immediate (transactional). | `REUSABLE_SyncCoordinator`, event bus subscribers | Continuous worker | `SyncEvent { source_store, target_store, entity_type, entity_id, payload }` | Target store updated; latency tracked; consistency window logged | `SyncEvent` (JSON) | `SyncResult { applied_at, latency_ms }` | End | The hybrid store model requires explicit eventual-consistency tracking; ad-hoc sync would silently drift. |
| `SR_DM_22_SE-01` | SE | data-model | Sync target store unavailable; events accumulate in subscriber backlog. | Backlog monitor, `REUSABLE_Alerter` | Driver exception | Same `SyncEvent` | Event remains in subscriber position; backlog age monitored; alert at 60 seconds, escalate at 5 minutes | Same | `SyncResult { state: deferred }` | Resume on store recovery. | Ensures sync failures are visible and don't silently corrupt cross-store consistency. |
| `SR_DM_22_BE-01` | BE | data-model | Sync event references an entity that does not exist in the source store (deleted before sync). | Existence check | Lookup | Same | Event dropped with audit; if drops exceed 1% in any 5-minute window, alert | Same | `SyncResult { state: dropped, reason: source_missing }` | End | Race conditions during deletes are normal but must remain rare; the alert threshold catches systemic issues. |
| `SR_DM_23` | --- | data-model | Reject any direct write to a vector index that does not pass through `REUSABLE_VectorIndexer` (which enforces tagging and version metadata). | Vector index policy enforcer | On Neo4j vector write attempt | `VectorWriteAttempt { source, model_id?, vector }` | Rejected if untagged | Same | `Result { accepted: bool, reason? }` | End | Untagged embeddings cannot be rolled back per D-33; enforcement at the write boundary is the only reliable prevention. |
| `SR_DM_24` | --- | data-model | Run scheduled graph maintenance: stale data pruning (daily), orphan node cleanup (weekly), index optimization (weekly), embedding refresh on model change, AI-inferred edge confidence decay (monthly), graph backup (daily incremental + weekly full), tenant isolation audit (weekly). The cycle types map 1:1 to `SR_INT_25` Intelligence Layer maintenance — `SR_DM_24` is the data-model substrate, `SR_INT_25` is the operational entry point. | Maintenance scheduler, `REUSABLE_GraphReader` + writer | Scheduled jobs invoked by `SR_INT_25` | `MaintenanceCycleRequest { tenant_id?, cycle_type }` | Affected counts logged; alerts on anomalies | `MaintenanceCycleRequest` (JSON) | `MaintenanceCycleResult { affected_counts }` | End | Graph hygiene is required to maintain query performance and to satisfy retention and isolation requirements. Bidirectional reference to `SR_INT_25` makes the substrate/operation split explicit (BP-130). |
| `SR_DM_25` | --- | data-model | Persist `notification_log` rows for the interface notification center (`SR_UI_25`); one row per delivered notification with read/unread state, including replayed offline-mode actions per `SR_UI_36` which preserve original timestamps as metadata. | `REUSABLE_PgWriter` | Called from `SR_UI_25` notification dispatch and `SR_UI_36` offline replay | `NotificationRow { tenant_id, person_id, message, original_timestamp?, read_state }` | Row inserted | JSON | `Result { row_id }` | End | Server-side notification state is required so the unread count is consistent across browser tabs and sessions. Bidirectional references to `SR_UI_25` and `SR_UI_36` complete BP-111 and BP-112. |
| `SR_DM_26` | --- | data-model | Persist `user_preferences` and `feature_flags` rows for tenant-scoped configuration. | `REUSABLE_PgWriter` | Called from `SR_GOV_68`, `SR_UI_` settings | `PreferenceRow` / `FeatureFlagRow` | Rows upserted | JSON | `Result { row_id }` | End | Provides the storage substrate for D-66 interface and D-61 recipient preferences. |

---

## Section 4 — Multi-Tenant Isolation, Compartments, and Shredding (SR_DM_27 through SR_DM_31)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_DM_27` | --- | data-model | Inject the `tenant_id` predicate into every Cypher query and the row-level security filter into every PostgreSQL query; reject queries that bypass the rewrite. | Cypher query rewriter, PG row-level security policies | Inline call from any data path | `QueryRewriteContext { tenant_id, principal, raw_query }` | Rewritten query with tenant_id constraint applied; execution plan estimated | `QueryRewriteContext` (JSON) | `QueryRewriteResult { rewritten_query }` | Caller proceeds with rewritten query. | Tenant injection at the rewrite boundary is the only reliable way to enforce multi-tenant isolation for both stores. |
| `SR_DM_27_SE-01` | SE | data-model | Query rewrite fails (parser error or unsupported construct). | Parser, `REUSABLE_Alerter` | Rewriter exception | Same | Query rejected with detailed error to caller; SRE alerted | Same | `QueryRewriteResult { error }` | Caller fails fast. | Failsafe-deny posture: a query that cannot be safely scoped must not run. |
| `SR_DM_28` | --- | data-model | Run the weekly tenant isolation audit: scan for any edge or row that crosses tenant boundaries; if found, freeze writes on the affected tenant pair and alert security officer. | Audit scanner, write-freeze mechanism | Weekly scheduled job | `IsolationAuditRequest` | Either CLEAN result or violation report with affected entities | `IsolationAuditRequest` (JSON) | `IsolationAuditResult { result, violations[] }` | If violation: incident response. | Multi-tenant isolation must be verified independently of the rewrite layer; the audit is the second line of defense. |
| `SR_DM_29` | --- | data-model | Maintain `feature_flags` with cache invalidation on toggle so all stateless services see the new value within 60 seconds. | `REUSABLE_PgWriter`, cache invalidator | Called from `SR_GOV_68` | `FeatureFlagToggle` | Row updated; cache invalidated | JSON | `Result { active }` | End | Cache invalidation is the deciding factor for feature flag UX. |
| `SR_DM_30` | --- | data-model | Apply crypto-shredding for a subject erasure request: locate all events scoped to the subject's per-subject AES-256 key, request key destruction in CaaS per NIST 800-88, mark the events as cryptographically inaccessible, write a destruction certificate. | CaaS key destroy, `REUSABLE_PgWriter`, NIST 800-88 verifier | Called from `SR_GOV_52` | `ShredRequestInput { tenant_id, subject_id }` | Key destroyed; events marked inaccessible; destruction certificate written | `ShredRequestInput` (JSON) | `ShredResult { destroyed, certificate_ref }` | Returned to `SR_GOV_52`. | Crypto-shredding satisfies erasure without rewriting the immutable audit log. |
| `SR_DM_31` | --- | data-model | Verify that previously crypto-shredded data is unreadable on a periodic schedule (monthly) and produce a verification report. | KMS audit, `REUSABLE_PgReader` | Monthly scheduled job | `ShredVerificationRequest` | Report listing shredded subjects and verification status | JSON | `ShredVerificationResult { verified, anomalies[] }` | End | Periodic verification protects against silent reversal (e.g., accidental key restoration from backup). |

---

## Back-Propagation Log (originating in Spec 02)

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-94 | `SR_DM_22_BE-01` (sync drop on race condition) | `SR_GOV_47` (audit append) | Confirmed `SR_GOV_47` is PG-only and not sync-affected; no remediation needed but documented the boundary. | 1 |
| BP-95 | `SR_DM_27` (query rewrite enforcement) | `SR_GOV_77` (intelligence query rewrite) | Confirmed `SR_GOV_77` invokes `SR_DM_27`; the integration map already references this. No code change. | 1 |
| BP-96 | `SR_DM_30` (crypto-shred mark events inaccessible) | `SR_GOV_52` | Confirmed `SR_GOV_52` returns the destruction certificate to the rights officer; the marking step in `SR_DM_30` is the new explicit data-model action that must be invoked from `SR_GOV_52`. Spec 01 already references `SR_DM_30` via integration map. |  1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Atomicity story for hybrid writes is split between `SR_DM_01_SE-01` and the implicit happy path. | Confirmed; the SE row makes failure semantics explicit and the happy path implies a saga pattern. |
| 2 | Vector index untagged-write rejection (`SR_DM_23`) needs an explicit error path. | The SR returns `accepted: false`; caller is expected to log and surface to admin. Implementation will use a structured error type. |
| 3 | Tenant isolation audit (`SR_DM_28`) is weekly; what about real-time detection? | Real-time detection is enforced by `SR_DM_27` (query rewrite). The weekly audit is the second line of defense for storage-level violations. Both are required. |
| 4 | Sync coordinator (`SR_DM_22`) does not specify the dead-letter behavior. | Acknowledged. Dead-letter goes to a per-tenant DLQ in PostgreSQL with admin visibility. Implementation detail. |
| 5 | `SR_DM_11` lifecycle transitions are validated; ensure state machine is authoritative across both stores. | Confirmed: PG `recommendation_audit` and Neo4j `Recommendation.state` must agree; the sync layer (`SR_DM_22`) ensures eventual consistency. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_DM_05` | `SR_GOV_47` | Audit row insertion |
| `SR_DM_07` | `SR_CONN_` Connection Layer | DataCollection creation triggered from connection |
| `SR_DM_08` | `SR_CONN_` Stage 3-6 | Field upsert triggers downstream classification |
| `SR_DM_09` | `SR_DS_` | Recommendation node creation from Decision Support |
| `SR_DM_15` | `SR_LLM_` | ModelExecution from LLM Router |
| `SR_DM_18` | `SR_INT_` | Vector index from Intelligence Layer |
| `SR_DM_22` | All consumers | Sync events drive cross-store consistency |
| `SR_DM_27` | `SR_GOV_77` | Query rewrite invoked by governance |
| `SR_DM_30` | `SR_GOV_52` | Crypto-shred invoked by governance |

## Spec 02 Summary

| Metric | Value |
|--------|-------|
| Sections | 4 |
| Main-flow SRs | 31 |
| Exception SRs | 5 |
| Total SR rows | 36 |
| BP entries created | 3 (BP-94, BP-95, BP-96) |
| New decisions | 0 |

**Status:** Self-audit complete.

---

## Back-Propagation Received from Later Specs (applied retroactively in Session 2)

| BP | Originating Spec | Edit Applied to Spec 02 |
|----|-----------------|-------------------------|
| BP-99 | Spec 03 (`SR_CONN_40` cloud LLM as Connection) | `SR_DM_04` updated with provider-specific metadata fields |
| BP-129 | Spec 06 (`SR_DS_31` source comparison via `data_origin`) | `SR_DM_07` updated to include `data_origin` field |
| BP-106 | Spec 05 (`SR_LLM_50` fine-tune lifecycle) | `SR_DM_15` updated with `training` task type and `training_run_id` |
| BP-130 | Spec 04 (`SR_INT_25` graph maintenance) | `SR_DM_24` updated with bidirectional reference |
| BP-111 | Spec 07 (`SR_UI_25` notification center) | `SR_DM_25` updated with bidirectional reference |
| BP-112 | Spec 07 (`SR_UI_36` offline replay) | `SR_DM_25` updated to accept `original_timestamp` in NotificationRow |

**Total retroactive edits to Spec 02: 5 SR row updates across 6 source BPs.**
