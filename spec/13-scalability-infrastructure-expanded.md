# Spec 13 Expanded: Scalability Infrastructure

**Source:** `001/spec/13-scalability-infrastructure.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** Non-functional (underlies everything)
**Reserved SR range:** `SR_SCALE_01` through `SR_SCALE_99`
**Last updated:** 2026-04-10

---

## Purpose

Implementation-readiness for the scaling architecture: load balancer, stateless API, WebSocket, event bus, component pools (LLM, agent workers, Neo4j cluster, PostgreSQL pool, connection workers, log workers), per-tenant resource quotas, graceful degradation chain, monitoring, disaster recovery.

## Architectural Decisions Covered

D-30, plus the SLA tables and DR matrix from the source spec.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| Per-Tenant Resource Quotas | `ARCH § 2.8.5` | `SR_SCALE_25` |
| Canary Deployment | `ARCH § 2.7.3` | `SR_SCALE_30` |
| Configuration Parity Service | `ARCH § 2.7.6` | `SR_SCALE_35` |

## Integration Map

| Consumer | Depends On |
|----------|------------|
| Spec 01 Governance | `SR_SCALE_25` (per-tenant quota enforcement) |
| Spec 02 Data Model | `SR_SCALE_15` (Neo4j cluster), `SR_SCALE_18` (PostgreSQL pool) |
| Spec 03 Connection | `SR_SCALE_22` (connection workers) |
| Spec 04 Intelligence | `SR_SCALE_15` (graph queries) |
| Spec 05 LLM Routing | `SR_SCALE_10` (LLM pool), `SR_SCALE_12` (GPU management) |
| Spec 07 Interface | `SR_SCALE_05` (load balancer), `SR_SCALE_08` (WebSocket) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_QuotaEnforcer` | Per-tenant resource quotas with guaranteed minimums |
| `REUSABLE_DegradationChain` | Graceful degradation per overloaded component |
| `REUSABLE_GpuPoolManager` | GPU lifecycle including spin-up, release, eviction |

---

## Section 1 — Load Balancer, API, WebSocket (SR_SCALE_01 through SR_SCALE_09)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SCALE_01` | --- | scalability | Layer 7 load balancer (NGINX, AWS ALB, or Cloudflare): TLS termination, per-tenant rate limiting, DDoS protection, health-check routing. | Load balancer | Inbound traffic | `HttpRequest` | Routed to healthy API server | HTTP | `RoutedRequest` | API server | First line of defense and traffic management. |
| `SR_SCALE_05` | --- | scalability | Stateless API server: 2-3 servers (SMB), 6-12+ (growth); auto-scale on request queue depth and response latency; target p95 <500ms API, <200ms auth. | Stateless API container | After load balancer | `ApiRequest` | Response | HTTP | `ApiResponse` | End | Stateless design enables horizontal scaling. |
| `SR_SCALE_05_SE-01` | SE | scalability | Auto-scaler fails to provision new server. | Auto-scaler logs | Provision failure | Same | Existing servers absorb load; alert SRE if backlog >5 min | Same | `Result { state: degraded }` | Manual intervention. | Bounded degradation. |
| `SR_SCALE_08` | --- | scalability | WebSocket server with sticky sessions + Redis pub/sub for cross-server messaging; Socket.io rooms per tenant; max 100 concurrent WS per tenant; auto-reconnect with exponential backoff. | Socket.io, Redis pub/sub | Client connection | `WsConnect` | Connection accepted; subscriptions registered | WS | `WsConnected` | End | Real-time UX requires WebSocket; sticky sessions + Redis pub/sub enable horizontal scaling. |
| `SR_SCALE_09` | --- | scalability | WebSocket failover: on server failure, clients reconnect to a healthy server; sessions are reconstructed from Redis pub/sub state. | Redis pub/sub, Socket.io | Server failure | `Reconnect` | Session restored | WS | `Result` | End | Bounded WS outage windows. |

## Section 2 — LLM Pool, GPU Management (SR_SCALE_10 through SR_SCALE_14)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SCALE_10` | --- | scalability | Local T1 (small) inference pool: CPU-only 7-8B quantized; horizontal scale via containers; FIFO queue with priority. | Container orchestrator | LLM Router request | `T1RequestEnqueue` | Inference result | JSON | `Result` | End | Tagging pipeline volume justifies a dedicated pool. |
| `SR_SCALE_11` | --- | scalability | Local T2 (medium) inference pool: GPU-bound; one inference per GPU; priority queuing; per-tenant fair scheduling. | `REUSABLE_GpuPoolManager` | LLM Router request | `T2RequestEnqueue` | Inference result | JSON | `Result` | End | T2 workload requires GPUs; fair scheduling prevents tenant starvation. |
| `SR_SCALE_12` | --- | scalability | GPU pool management: queue depth >10 for 30s → request additional GPU; utilization <20% for 15min → release GPU; unused fine-tuned model >1 hour → unload. | `REUSABLE_GpuPoolManager` | Continuous monitoring | `GpuPoolTick` | Scale up / down decisions | JSON | `GpuDecision` | End | Cost-efficient GPU utilization. |
| `SR_SCALE_13` | --- | scalability | Cloud T3 (large) routing: respect provider rate limits; multi-provider failover (Anthropic → OpenAI → Google → local T2); per-tenant budget caps. | `REUSABLE_ProviderFailover`, `REUSABLE_TokenBudgetTracker` | LLM Router T3 request | `T3RequestEnqueue` | Inference result | JSON | `Result` | End | Cloud failover ensures availability without breaking budget. |
| `SR_SCALE_14` | --- | scalability | T-FT (fine-tuned) routing: dedicated GPU per active model; LRU eviction for infrequent models; cold-start 30-60s for large models. | `REUSABLE_GpuPoolManager` | T-FT request | `TftRequestEnqueue` | Inference result | JSON | `Result` | End | Bounded GPU consumption for the long tail of fine-tuned models. |

## Section 3 — Neo4j, PostgreSQL, Event Bus, Workers (SR_SCALE_15 through SR_SCALE_24)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SCALE_15` | --- | scalability | Neo4j cluster: read replicas for query scaling; causal clustering for write consistency; label-based tenant partitioning (or separate DB per tenant at premium); query timeout 30s, result limit 10K nodes. SMB: 1-2 read replicas; growth: 3-5 replicas. | Neo4j cluster, query router | Query inbound | `Cypher` | Result | JSON | `QueryResult` | End | Hybrid store performance. |
| `SR_SCALE_15_SE-01` | SE | scalability | Read replica unavailable. | Cluster monitor | Replica failure | Same | Failover to next replica; alert | Same | `Result` | End | Bounded read failover. |
| `SR_SCALE_18` | --- | scalability | PostgreSQL pool: PgBouncer connection pooling (200 DB connections → thousands of client connections); read replica for analytics; audit table partitioned by tenant_id + month; Citus for horizontal sharding at extreme scale; automated partition pruning per retention. | PgBouncer, PostgreSQL cluster | Query inbound | `SQL` | Result | JSON | `QueryResult` | End | Connection pooling and partitioning are baseline scalability. |
| `SR_SCALE_20` | --- | scalability | Event bus (Redis Streams at SMB scale, Kafka at growth scale): stream per tenant per event type; consumer groups per service; backpressure alert at 1K pending, block at 10K pending; Redis AOF persistence. | Redis Streams or Kafka | Event publish | `Event` | Persisted; consumed by subscribers | JSON | `EventPublishResult` | End | Scales with tenant count and event volume. |
| `SR_SCALE_22` | --- | scalability | Connection layer workers: queue-based per tenant; cron + event-driven scheduling; per-external-system rate limits; max 5 simultaneous per external system; max 20 total per tenant; priority on-demand > critical > standard > background. | Worker pool, queue | Pull schedule | `WorkerJob` | Pulls executed | JSON | `Result` | End | Per-tenant isolation and per-system fairness. |
| `SR_SCALE_24` | --- | scalability | Log ingestion workers (BP-66): separate worker pool from connection workers; rate limiting per source; backpressure handling; priority ERROR > WARN > INFO > DEBUG. | Log worker pool | Log stream | `LogJob` | Ingested | JSON | `Result` | End | Performance isolation between log and connection paths. |

## Section 4 — Quotas, Degradation, DR (SR_SCALE_25 through SR_SCALE_40)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SCALE_25` | --- | scalability | Per-tenant resource quotas with default minimums and configurable maximums per the table in Spec 13: API requests/min, concurrent users, WS connections, LLM invocations/hour, GPU time/hour, cloud LLM spend/month, Neo4j nodes, PostgreSQL storage, active connections, concurrent agents, event bus throughput. | `REUSABLE_QuotaEnforcer` | All multi-tenant operations | `QuotaCheckInput { tenant_id, resource, amount }` | ALLOW or DENY | JSON | `QuotaCheckResult` | If DENY: caller backs off or fails. | Quotas guarantee minimums and enforce maximums; prevents noisy-neighbor problems. |
| `SR_SCALE_25_BE-01` | BE | scalability | Tenant exceeds maximum quota for a resource. | `REUSABLE_QuotaEnforcer` | Quota check | Same | DENY with retry-after; admin notified for upgrade option | Same | `Result { error: quota_exceeded, retry_after }` | Tenant waits or upgrades. | Enforces fair use without breaking the platform for other tenants. |
| `SR_SCALE_30` | --- | scalability | Graceful degradation chain: cloud LLM → secondary → local T2; local GPU pool → queue with wait time; Neo4j reads → cached; Neo4j writes → buffered to event bus; PostgreSQL → queued audit; WebSocket → polling fallback; event bus → backpressure prioritizing critical; connection workers → prioritize on-demand; API gateway → per-tenant rate limit; everything → emergency mode (core governance only). | `REUSABLE_DegradationChain` | Component overload | `DegradationInput { component, severity }` | Degradation applied; users informed | JSON | `Result` | End | Graceful degradation preserves core functionality during overload. |
| `SR_SCALE_35` | --- | scalability | Configuration Parity Service per `ARCH § 2.7.6`: PROD config changes propagate to STAGING within 1 hour; hourly drift detection. | Config sync worker | PROD config change | `ConfigSyncInput` | Propagated; drift verified | JSON | `Result` | End | Prevents environment drift that masks bugs in non-PROD environments. |
| `SR_SCALE_40` | --- | scalability | Disaster recovery RTO/RPO targets: Neo4j primary fails <5 min / <30 sec; Neo4j cluster loss <2 hr / <24 hr; PostgreSQL failover <2 min / <10 sec; both lost <4 hr / <24 hr; event bus failure <5 min / events in-flight. Drilled quarterly per `SR_INT_29`. | DR runbooks, drill scheduler | DR scenario | `DrTrigger` | Recovery initiated | JSON | `DrResult` | Recovery completed within RTO. | Documented and drilled DR ensures real-world recovery within SLA. |

## Section 5 — Monitoring and Observability (SR_SCALE_45 through SR_SCALE_50)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_SCALE_45` | --- | scalability | Platform-level metrics: API latency p50/p95/p99, LLM queue depth and wait times, Neo4j query performance, PostgreSQL connection pool utilization, WS connection count, event bus backpressure, per-tenant resource consumption, GPU utilization, cloud API costs (daily, monthly trend). | Prometheus or equivalent | Continuous | `MetricsTick` | Time-series metrics | JSON | `Result` | End | Operational visibility. |
| `SR_SCALE_46` | --- | scalability | Per-tenant metrics: request volume, query success rate, recommendation delivery latency, connection health, agent performance. | Same | Continuous | Same | Per-tenant rollups | JSON | `Result` | End | Tenant-level observability for support and capacity planning. |
| `SR_SCALE_47` | --- | scalability | Alert thresholds and routing per `SR_GOV_67`. | `SR_GOV_67` | Threshold breach | `AlertEvent` | Alert dispatched | JSON | `Result` | End | Connected to the governance alert routing matrix. |
| `SR_SCALE_50` | --- | scalability | Scalability targets: SMB (5-50 tenants) → 2-3 API servers, 100-500 req/s, 50-500 concurrent users, 1-2 Neo4j replicas, 1K-50K nodes per tenant; Growth (50-500 tenants) → 6-12 API servers, 500-5K req/s, 500-5K concurrent users, 3-5 Neo4j replicas, 50K-500K nodes per tenant. | Capacity planner | Quarterly review | `CapacityPlanInput` | Recommended scaling | JSON | `Result` | End | Bounded growth path. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-119 | `SR_SCALE_25` quota enforcement | `SR_GOV_71` (coverage), `SR_LLM_24` (token budget) | Confirmed: quota enforcement complements token budgets and coverage disclosure. | 1 |
| BP-120 | `SR_SCALE_30` degradation | All consumer specs | Confirmed: every consumer must handle the documented degradation states gracefully (fallback to cached, polling, etc.). | 1 |
| BP-121 | `SR_SCALE_40` DR targets | `SR_INT_29` DR drills | Confirmed bidirectional: scheduling and execution. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Per-tenant fair scheduling on GPUs (`SR_SCALE_11`) — algorithm? | Implementation: weighted fair queueing with weights from tenant tier. |
| 2 | Backpressure block at 10K pending (`SR_SCALE_20`) — what happens to events that overflow? | Spool to durable disk queue; replay when backpressure clears. |
| 3 | DR drill scheduling (`SR_SCALE_40`) — who reviews failed drills? | Platform SRE on-call + tenant security officer (for tenant-specific scenarios). |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_SCALE_25` | `SR_GOV_71`, `SR_LLM_24` | Quota integration |
| `SR_SCALE_40` | `SR_INT_29` | DR drill scheduling |
| `SR_SCALE_47` | `SR_GOV_67` | Alert routing |

## Spec 13 Summary

| Metric | Value |
|--------|-------|
| Sections | 5 |
| Main-flow SRs | 22 |
| Exception SRs | 2 |
| Total SR rows | 24 |
| BP entries created | 3 (BP-119 through BP-121) |
| New decisions | 0 |

**Status:** Self-audit complete.
