# Spec 05 Expanded: LLM Routing

**Source:** `001/spec/05-llm-routing.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** 3 (analysis and insight generation)
**Reserved SR range:** `SR_LLM_01` through `SR_LLM_99`
**Last updated:** 2026-04-10

---

## Purpose

Implementation-readiness for the LLM Router: Two-stage routing (D-10), Model Tiers T1/T2/T3/T-FT/T-VERIFY (D-11), PII/PHI/CUI on-prem rule (D-12), model hot-swap governance (D-29), embedding model rollback (D-33), LLM observability layer (D-28), 7-step request lifecycle, prompt management (D-38), multi-model query decomposition (D-39), two-mode streaming (D-40), token budget (D-41), fine-tuned model lifecycle (D-42), cloud LLM as Connection (D-43), DBE v2 verification, MLAID injection defense.

## Architectural Decisions Covered

D-10, D-11, D-12, D-28, D-29, D-33, D-38, D-39, D-40, D-41, D-42, D-43, plus LR-1 through LR-9.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| DBE v2 verification (5-check pipeline) | `ARCH § 2.3.1` | `SR_LLM_30` through `SR_LLM_35` |
| Cross-Family Verifier Ensemble | GAP-51 | `SR_LLM_31` |
| Semantic Entropy uncertainty quantification | Kuhn 2023 + Farquhar 2024 | `SR_LLM_32` |
| MLAID multi-layer injection defense | GAP-61 | `SR_LLM_25` |
| Model collapse prevention thresholds | GAP-44 (PROVEN) | `SR_LLM_50`, `SR_LLM_52` |
| Risk-Tiered Verification Profiles | GAP-52 | `SR_LLM_30` |

## Integration Map

| Consumer Spec | Depends On |
|--------------|------------|
| Spec 01 Governance | `SR_LLM_05` (Stage 1 invocation), `SR_LLM_44` (hot-swap approval) |
| Spec 02 Data Model | `SR_LLM_15` (ModelExecution), `SR_LLM_16` (ModelOutcomeScore), `SR_LLM_40` (model_registry write) |
| Spec 03 Connection | `SR_LLM_42` (cloud provider as connection) |
| Spec 04 Intelligence | `SR_LLM_25` (T1 invocations from tagging), `SR_LLM_18` (multi-model decomposition synthesis) |
| Spec 06 Decision Support | `SR_LLM_30` (DBE v2), `SR_LLM_18` (multi-model queries), `SR_LLM_22` (verified streaming) |
| Spec 07 Interface | `SR_LLM_22` (live streaming for chat), `SR_LLM_23` (verified streaming for recommendations) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_PromptAssembler` | Assembles prompts in fixed order: safety guardrail → system prompt → vertical → tenant customization → output format → context + history + query |
| `REUSABLE_TokenBudgetTracker` | Per-platform / per-tenant / per-query / per-user budget enforcement with degradation |
| `REUSABLE_ModelRegistry` | Slot → active model + candidate + rollback + constraints |
| `REUSABLE_ProviderFailover` | Primary → secondary → tertiary → local fallback chain per D-43 |

---

## Section 1 — Two-Stage Router (SR_LLM_01 through SR_LLM_10)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_LLM_01` | --- | llm | Receive an LLM request from any caller; populate context (tenant, principal, data attributes, task type, sensitivity, complexity hint). | Request middleware | Inline call from any caller | `LlmRequest { tenant_id, principal, task_type, data_attrs, prompt_template, payload }` | Enriched request ready for Stage 1 | `LlmRequest` (JSON) | `EnrichedLlmRequest` | `SR_LLM_05` | Centralized request preparation ensures every invocation has consistent context. |
| `SR_LLM_02` | --- | llm | CSA preflight: invoke `SR_GOV_24` if the request combines data from multiple collections. | `SR_GOV_24` | Inline if combine detected | `CsaPreflightInput` | ALLOW / BLOCK / ANONYMIZE / ELEVATE | JSON | Forwarded result | If ALLOW: `SR_LLM_05`. Else: terminate. | CSA must run before any combine reaches the model. |
| `SR_LLM_05` | --- | llm | Router Stage 1 (governance, deterministic): apply 8 rules per D-10 to produce `allowed_models[]`. R1 PII/PHI/CUI → on-prem only. R2 compartment restrictions. R3 tenant blacklist. R4 tenant whitelist. R5 CSA elevation. R6 strip data the user lacks permission for. R7 regulatory audit-grade logging. R8 if empty → DENY. This SR is the LLM-side caller; the deterministic rule evaluation lives in `SR_GOV_73` which is invoked here. | `governance_rules` via `SR_GOV_73`, `REUSABLE_TenantFilter` | Inline from `SR_LLM_01` | `Stage1Input { context, candidate_models }` | `allowed_models[]` or DENY (delegated to `SR_GOV_73`) | JSON | `Stage1Result { allowed_models, reasoning }` | If non-empty: `SR_LLM_10`. Else: DENY. | The deterministic gate is the single point where governance is applied to every model invocation. Bidirectional reference to `SR_GOV_73` makes the caller/callee split explicit (BP-104). |
| `SR_LLM_05_BE-01` | BE | llm | All candidate models filtered out (empty `allowed_models`). | Stage 1 evaluator | After R1-R7 application | Same | Request rejected with explainable reason | Same | `Stage1Result { allowed: [], reason }` | End | Failsafe-deny preserves compliance even when all models are restricted (e.g., PII data + no on-prem models available). |
| `SR_LLM_10` | --- | llm | Router Stage 2 (optimization, AI-assisted): classify complexity (T1 classifier) → check performance history → check token budget → select best model. | T1 classifier, `model_performance_analytics`, `REUSABLE_TokenBudgetTracker` | After Stage 1 | `Stage2Input { allowed_models, request, complexity_hint }` | `routing_decision { model, parameters, budget_impact }` | JSON | `RoutingDecision` | `SR_LLM_15` (start ModelExecution) | Optimization within the allowed set balances quality, cost, latency. |

## Section 2 — Model Tiers and Hot-Swap (SR_LLM_11 through SR_LLM_18)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_LLM_11` | --- | llm | T1 invocation: small local model (<100ms, <8GB VRAM) for tagging, simple extraction, classification. Hard timeout: **10 seconds** (per BR_LLM_04_Timeout_SE-02). | Local T1 inference container | From router or tagging pipeline | `T1Request { prompt, parameters }` | T1 response within 10s timeout | JSON | `T1Response` | Returned to caller. | Fast, cheap, local — supports the high-volume tagging pipeline. Explicit 10s timeout per BP-131. |
| `SR_LLM_12` | --- | llm | T2 invocation: medium model (<1s typical) for reasoning, summarization. Hard timeout: **30 seconds**. | Local or private cloud T2 inference | From router | `T2Request` | `T2Response` within 30s timeout | JSON | Returned | Most general queries route here. Explicit 30s timeout per BP-131. |
| `SR_LLM_13` | --- | llm | T3 invocation: large cloud model (2-10s typical) for complex multi-factor analysis. Hard timeout: **120 seconds**. | Cloud provider via Spec 03 connection, `REUSABLE_ProviderFailover` | From router when complexity is high and data permits | `T3Request` | `T3Response` within 120s timeout | JSON | Returned | Complex queries justify higher cost; failover chain handles outages. Explicit 120s timeout per BP-131. |
| `SR_LLM_14` | --- | llm | T-FT invocation: fine-tuned domain model. Used when the task is industry-specific. Hard timeout: **60 seconds**. | Local or private cloud T-FT inference | From router when domain fit | `TftRequest` | `TftResponse` within 60s timeout | JSON | Returned | Fine-tuned models give domain accuracy uplift. Explicit 60s timeout per BP-131. |
| `SR_LLM_15` | --- | llm | Create a `ModelExecution` node per `SR_DM_15` for the invocation. | `SR_DM_15` | Inline before invocation | `ModelExecutionInput` | Execution node id | JSON | `ExecutionId` | Returned to caller; updated after invocation completes with metrics. | Per-invocation tracking is the foundation of Flywheel 3. |
| `SR_LLM_16` | --- | llm | After response, populate `ModelExecution` with metrics (tokens, latency, cost) and create `ModelOutcomeScore` when outcome is known via `SR_DM_16`. | `SR_DM_16` | After response and after outcome | `MetricsInput` | Updated execution node | JSON | `Result` | End | Metrics close the loop on routing optimization. |
| `SR_LLM_17` | --- | llm | Aggregate model performance per `SR_DM_17` for fast routing lookup. | `SR_DM_17` | Hourly | Same | Same | Same | Same | End | Pre-aggregation makes Stage 2 routing fast. |
| `SR_LLM_18` | --- | llm | Multi-model query decomposition per D-39: complex queries split into sub-queries routed in parallel; synthesis step combines results. | Decomposer, parallel runner | When `SR_LLM_10` flags complex query | `DecompositionInput` | Sub-query results + synthesized response | JSON | `DecompositionResult` | Each sub-query passes through `SR_LLM_15`-`SR_LLM_16`. | Multi-perspective queries get parallel sub-analysis with cost attribution. |

## Section 3 — Verification Pipeline DBE v2 (SR_LLM_30 through SR_LLM_36)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_LLM_30` | --- | llm | DBE v2 5-check sequential verification: V1 boundary, V2 factual (MiniCheck/FActScore), V3 semantic entropy, V4 explainability, V5 safety. RTVP profiles select intensity per task. | DBE v2 components | After every model response | `VerificationInput { response, context, profile }` | Pass / fail per check; aggregate decision | JSON | `VerificationResult { decision, scores }` | If pass: response delivered. Else: regenerate or block. | DBE v2 makes AI output trustworthy in regulated environments (HIGH-PROB evidence; cited from Min EMNLP 2023, Farquhar Nature 2024). |
| `SR_LLM_31` | --- | llm | Cross-family verifier ensemble per GAP-51: 3+ verifiers from independent model families with consensus rules (≥2 of 3 agree). | Verifier model pool, `REUSABLE_ModelRegistry` | Inline within `SR_LLM_30` | `EnsembleVerificationInput` | Consensus result with per-verifier votes | JSON | `EnsembleResult` | Returned to `SR_LLM_30`. | Cross-family ensemble protects against single-model verifier blind spots (Dietterich 2000 ensemble theory PROVEN). |
| `SR_LLM_32` | --- | llm | Semantic entropy check (V3): cluster semantically equivalent responses with k=5 default, k=10 high-stakes; AUROC ≥ 0.79 target. | Semantic entropy computer | Inline within `SR_LLM_30` | `EntropyInput { response, k }` | Entropy score with uncertainty interpretation | JSON | `EntropyResult { score, interpretation }` | Returned. | Quantifies model uncertainty per Kuhn ICLR 2023 + Farquhar Nature 2024. |
| `SR_LLM_33` | --- | llm | Factual check (V2): MiniCheck/FActScore against retrieved context; flag unsupported claims. | MiniCheck / FActScore implementation | Inline within `SR_LLM_30` | `FactualInput { response, source_context }` | Per-claim support score | JSON | `FactualResult` | Returned. | Catches hallucinated facts (Min EMNLP 2023 PROVEN). |
| `SR_LLM_34` | --- | llm | Boundary check (V1): does the response cross any decision boundary defined by the agent's DBC? | DBC registry | Inline within `SR_LLM_30` | `BoundaryInput { response, agent_id }` | Pass/fail with violations | JSON | `BoundaryResult` | Returned. | Per-agent boundaries prevent agents from exceeding their authority. |
| `SR_LLM_35` | --- | llm | Explainability check (V4): does the response include the required explainability chain (data sources, parameters, reasoning)? | Explainability validator | Inline within `SR_LLM_30` | `ExplainabilityInput` | Pass/fail with missing fields | JSON | `ExplainabilityResult` | Returned. | Per UU-7 — every recommendation requires full explainability. |
| `SR_LLM_36` | --- | llm | Safety check (V5): scan response for harmful content, prompt-injection echoing, PII leakage. | Safety scanner + MLAID | Inline within `SR_LLM_30` | `SafetyInput` | Pass/fail with categories | JSON | `SafetyResult` | Returned. If fail: response blocked. | Safety is the final gate before delivery. |

## Section 4 — Streaming, Token Budget, Prompt Management (SR_LLM_20 through SR_LLM_29)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_LLM_20` | --- | llm | Prompt assembly per D-38 in fixed order: safety guardrail (immutable, first), system prompt (task-specific), vertical, tenant customization (sandboxed/validated), output format, data context + conversation history + user query. | `REUSABLE_PromptAssembler`, prompt template registry | Inline before model invocation | `PromptAssemblyInput { task_type, tenant_id, customizations }` | Assembled final prompt | JSON | `PromptResult` | Used in `SR_LLM_11`-`SR_LLM_14`. | Fixed-order assembly with safety-first prevents tenant customizations from overriding safety. |
| `SR_LLM_21` | --- | llm | Validate a tenant prompt customization in a sandbox before activation per BP-57; ensure it does not override safety guardrails. | Sandbox tester, `SR_GOV_65` | Admin submits customization | `CustomizationValidationInput` | Pass/fail with findings | JSON | `Result` | If pass: `SR_GOV_65` approval. | Sandboxing catches harmful customizations before they reach production. |
| `SR_LLM_22` | --- | llm | Verified streaming (default for recommendations): buffer → verify → stream. User sees "analyzing..." for 2-10 seconds. | DBE v2 (`SR_LLM_30`), streaming server | Recommendation generation | `VerifiedStreamInput { response_stream }` | Verified response streamed | JSON | `Result` | End | Prevents unverified content from reaching users for high-stakes outputs. |
| `SR_LLM_23` | --- | llm | Live streaming (conversational queries): stream immediately with disclaimer; if verification fails, replace with regenerated verified response. | DBE v2, streaming server | Chat queries | `LiveStreamInput` | Streamed response with potential replacement | JSON | `Result` | End | Maintains chat UX latency without sacrificing verification. |
| `SR_LLM_24` | --- | llm | Token budget tracker per D-41: platform → tenant → per-query → per-user; degradation at 75/90/95/100% (alert / prefer cheap / local only / local only + alert). Token budget is the LLM-specific manifestation of the per-tenant resource quota model in `SR_SCALE_25`; both the budget AND the broader `REUSABLE_QuotaEnforcer` are checked before invocation. | `REUSABLE_TokenBudgetTracker`, `REUSABLE_QuotaEnforcer` | Inline before invocation | `BudgetCheckInput { tenant_id, estimated_tokens, model_tier }` | ALLOW with model preference, or DEGRADE (with reason: token_budget \| tenant_quota) | JSON | `BudgetCheckResult { decision, degradation_reason? }` | Caller proceeds with possibly downgraded model. | Prevents runaway cloud costs without breaking platform availability. The combined token-budget + quota check ensures both cost and resource fairness are enforced (BP-128). |
| `SR_LLM_25` | --- | llm | MLAID injection defense per GAP-61: 6 layers — static patterns, per-agent airlock, behavioral, embedding, LLM-as-judge, steganographic. Applied to user-supplied content before it reaches the model. This is the second MLAID pass for user uploads (the first runs at `SR_CONN_18_BE-01` upload time); for non-upload content (chat queries) this is the only MLAID gate. | MLAID component (six-layer pipeline) | Inline before model invocation when content is user-supplied | `InjectionScanInput { content, source: upload \| chat \| api }` | Pass/fail with detected layer | JSON | `InjectionScanResult` | If pass: invocation proceeds. Else: blocked + audit. | Per HIGH-PROB evidence (Liu USENIX Sec 2024, Chen USENIX Sec 2025, Chen CCS 2025, Lee ACL 2025). Bidirectional reference to `SR_CONN_18_BE-01` documents the defense-in-depth pattern (BP-105). |
| `SR_LLM_26` | --- | llm | Constant-time and jittered response patterns per CTJR (GAP-64): timing-side-channel mitigation for sensitive responses. | Timing controller | Inline for sensitive responses | `CtjrInput` | Response delivered with constant-time / jittered envelope | JSON | `Result` | End | Prevents timing side channels on classification responses. |
| `SR_LLM_27` | --- | llm | Conversation persistence (LR-1): Redis short-term + PostgreSQL backup; sliding context window. | Redis + `REUSABLE_PgWriter` | After each turn | `ConversationTurnInput` | Turn persisted | JSON | `Result` | End | Long conversations require persistent context that survives Redis evictions. |
| `SR_LLM_28` | --- | llm | Output normalization (LR-2): translate model-specific output formats into a normalized response schema. | Output normalizer | Inline after response | `NormalizationInput { model, raw_response }` | Normalized response | JSON | `NormalizedResponse` | `SR_LLM_30` verification. | Downstream consumers see one schema regardless of model. |
| `SR_LLM_29` | --- | llm | Per-ModelExecution cost attribution for multi-model queries (LR-4): each sub-execution carries its own cost; aggregated to the parent query. | `REUSABLE_PgWriter` | Inline | `CostAttributionInput` | Cost rows linked | JSON | `Result` | End | Without per-sub-execution attribution, multi-model queries cannot be cost-budgeted. |

## Section 5 — Hot-Swap, Fine-Tuned Lifecycle, Provider Connection (SR_LLM_40 through SR_LLM_50)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_LLM_40` | --- | llm | Model registry write per `REUSABLE_ModelRegistry`: slot, active, candidate, rollback, constraints. | `REUSABLE_ModelRegistry` | Admin or hot-swap workflow | `RegistryWriteInput` | Registry updated | JSON | `Result` | End | Centralized registry is the single source of truth for model selection. |
| `SR_LLM_41` | --- | llm | Hot-swap workflow: only platform admin + security admin per D-29 / `SR_GOV_66`; canary deployment 5% traffic 7 days; auto-rollback if quality drops >10%. | `SR_GOV_66`, canary controller | Approval flow | `HotSwapInput { slot, candidate, canary_config }` | Canary deployment started | JSON | `HotSwapResult` | `SR_LLM_43` (canary monitor) | Bounded blast radius for model swaps. |
| `SR_LLM_42` | --- | llm | Cloud LLM provider as Connection per D-43 and `SR_CONN_40`: track health, rate limits, API keys in CaaS; deprecation alerts at 90/30/0 days. The provider failover chain (Anthropic → OpenAI → Google → local T2 fallback) is the LLM-side participation in `SR_SCALE_30` graceful degradation chain. | `SR_CONN_40`, `REUSABLE_ProviderFailover`, `REUSABLE_DegradationChain` participation | Provider lifecycle and degradation events | `ProviderRoutingInput { tenant_id, request, primary_provider }` | Routed to primary or failover; degradation flag set if not primary | JSON | `ProviderRoutingResult { provider_used, failover_depth }` | Reuses Spec 03 lifecycle. | Bidirectional reference to `SR_CONN_40` and graceful degradation participation completes BP-99 and BP-120 for the cloud-LLM role. |
| `SR_LLM_43` | --- | llm | Canary monitor: compare candidate model quality against active using outcome scores; promote on success, rollback on quality drop >10%. | `model_performance_analytics`, canary controller | Continuous during canary window | `CanaryMonitorInput { canary_id }` | Promotion or rollback decision | JSON | `CanaryDecision` | `SR_LLM_44` (promote) or `SR_LLM_45` (rollback) | Auto-rollback contains the blast radius of bad swaps. |
| `SR_LLM_44` | --- | llm | Promote a canary to active after the canary window passes quality check. | `REUSABLE_ModelRegistry` | After canary success | `PromoteInput` | Slot updated; old model moved to rollback | JSON | `Result` | End | Promotion is the explicit transition to production traffic. |
| `SR_LLM_45` | --- | llm | Rollback a canary on quality regression. | `REUSABLE_ModelRegistry` | After canary failure | `RollbackInput` | Slot reverts to previous active | JSON | `Result` | End | Rollback completes within minutes. |
| `SR_LLM_46` | --- | llm | Embedding model rollback per D-33 and `SR_DM_19`: dual embedding storage during canary; rollback = switch active model_id; old embeddings purged after 30 days stable. | `REUSABLE_DualEmbeddingStore` | Embedding model swap | `EmbeddingRollbackInput` | Active model_id reverted | JSON | `Result` | End | Zero-downtime rollback for embedding models. |
| `SR_LLM_50` | --- | llm | Fine-tuned model lifecycle per D-42 + GAP-44: identify need → data prep (consent-checked via `SR_LLM_51` and `SR_GOV_60`, de-identified, 60% human data minimum, max 20% verified synthetic) → train (creates `ModelExecution` nodes with `task_type=training` per `SR_DM_15`; monitor perplexity divergence >15% halt) → validate (outperform general, pass safety/bias/regression) → canary (10% traffic, 7 days) → A/B → promote or rollback → monitor with drift detection → retraining every 6 months. | Training pipeline (sandboxed), `REUSABLE_AgentFeedbackTracker`, `SR_GOV_60` consent, `SR_DM_15` training telemetry | Identified need | `FineTuneLifecycleInput` | New T-FT slot or rejection; training runs tracked via `SR_DM_15` | JSON | `FineTuneResult { slot, training_run_id, status }` | `SR_LLM_43` canary monitor. | Per PROVEN evidence (Shumailov Nature 2024, Dohmatob ICLR 2025, Alemohammad ICLR 2024, Shumailov ICML 2024). The explicit `SR_GOV_60` and `SR_DM_15` references complete BP-106 and BP-127. |
| `SR_LLM_51` | --- | llm | Training data consent check (BP-56): verify every DataCollection used for fine-tuning has `training_consent: true` per `SR_GOV_60`. Re-verifies on every batch (not just lifecycle start) so a mid-training consent revocation halts the run. | `SR_GOV_60` | Inline at training start AND per training batch | `TrainingConsentCheck { collection_ids, batch_id }` | Pass/fail per batch; failure aborts training | JSON | `Result` | If fail: training aborted, audit row written, rollback to last-good checkpoint. | Required for compliant training data acquisition. Per-batch re-verification protects against mid-training consent revocation (BP-127 amendment). |
| `SR_LLM_52` | --- | llm | Model collapse detection during fine-tuning: monitor perplexity divergence vs reference dataset; halt at >15% with rollback. | Training monitor | Continuous during training | `CollapseMonitorInput` | HALT or CONTINUE | JSON | `Result` | If HALT: training rolled back. | Per GAP-44: collapse onset detectable within 5-9 iterative training generations. |

## Section 6 — Response Cache (SR_LLM_60 through SR_LLM_61)

Added in Session 3 via back-propagation from workbook (BP-132). The response cache is a performance optimization that did not appear in the initial expansion but is required per the workbook BR_LLM_10.

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_LLM_60` | --- | llm | Response cache lookup and write for repeated identical queries. Cache key = `SHA256(query_hash + data_versions + model_id + tenant_id)`. TTL 5 minutes for dashboard queries, configurable per task type. Never cache PII responses or audit-grade tasks (evaluated via `SR_LLM_61`). | Response cache (Redis), PII detector, cache key builder | Inline before `SR_LLM_15` ModelExecution creation | `CacheLookupInput { tenant_id, query, context, model_id }` | On hit: return cached response with provenance and cache_hit flag; on miss: proceed to invocation, then on successful verification, write to cache (unless excluded) | `CacheLookupInput` (JSON) | `CacheLookupResult { hit: bool, response?, cached_at? }` | On hit: skip `SR_LLM_15`-`SR_LLM_36` and return to caller. On miss: proceed to `SR_LLM_15`. | Caching reduces cost and latency for repeated queries without violating governance. Cache key includes tenant_id to prevent cross-tenant cache leakage. |
| `SR_LLM_61` | BE | llm | Attempt to cache a response that contains PII data (detected via three-tier NER ensemble applied to the response itself). | PII detector on response, cache key builder | Inline before cache write in `SR_LLM_60` | `CacheWriteAttempt { response, tenant_id, key }` | Cache write skipped; response returned directly to caller without storage; audit event `cache_skipped_pii` written | `CacheWriteAttempt` (JSON) | `CacheWriteResult { written: false, reason: 'pii_detected' }` | End | PII responses cannot be cached. Caching them would create a side-channel that bypasses `SR_GOV_33` compartment checks on read. Per BP-132. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-104 | `SR_LLM_05` (Stage 1 deny on empty allowed_models) | `SR_GOV_73` | Confirmed `SR_GOV_73` returns the empty set; LLM Router treats this as DENY and informs caller. No code change. | 1 |
| BP-105 | `SR_LLM_25` (MLAID injection defense) | `SR_CONN_18` (user upload) | Confirmed `SR_CONN_18_BE-01` invokes MLAID at the connection boundary; `SR_LLM_25` invokes it again at model invocation time for defense in depth. | 1 |
| BP-106 | `SR_LLM_50` fine-tune lifecycle | `SR_GOV_60` (training consent), `SR_DM_15` (ModelExecution tracking for training runs) | Confirmed both: `SR_GOV_60` provides consent, `SR_DM_15` tracks training as a special task_type. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | DBE v2 sequential checks — what if V1 boundary fails but V5 safety is what should drive the action? | Both checks fire; the aggregate decision is FAIL on any check failure. The order is a performance optimization (cheaper checks first), not a logical order. |
| 2 | Two-mode streaming — what if verification fails for live mode mid-stream? | `SR_LLM_23` documents the replacement behavior: failed live response is replaced with a regenerated verified response. UX shows a transition. |
| 3 | Token budget degradation — what if local models cannot handle the request complexity? | At 100% the request is rate-limited rather than quality-degraded; the user is informed and can wait or upgrade tier. |
| 4 | Hot-swap during fine-tuned model active use — what about queries in flight? | In-flight queries complete on the old model; new queries route to the new model. Tracked via `ModelExecution.model_version`. |
| 5 | Embedding rollback — what about queries served from new embeddings during the rollback window? | Dual storage (`SR_DM_19`) means both old and new embeddings exist; rollback switches the active pointer. Queries served during the switch use whichever pointer is active at the time. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_LLM_02` | `SR_GOV_24` | CSA preflight |
| `SR_LLM_05` | `SR_GOV_73` | Stage 1 governance gate |
| `SR_LLM_15` | `SR_DM_15` | ModelExecution write |
| `SR_LLM_22` | `SR_DS_` | Verified streaming for recommendations |
| `SR_LLM_25` | `SR_CONN_18` | MLAID at upload + invocation |
| `SR_LLM_41` | `SR_GOV_66` | Hot-swap approval |
| `SR_LLM_42` | `SR_CONN_40` | Cloud provider as connection |
| `SR_LLM_50` | `SR_GOV_60` | Training consent |

## Spec 05 Summary

| Metric | Value |
|--------|-------|
| Sections | 5 |
| Main-flow SRs | 32 |
| Exception SRs | 1 |
| Total SR rows | 33 |
| BP entries created | 3 (BP-104 through BP-106) |
| New decisions | 0 |

**Status:** Self-audit complete.

---

## Back-Propagation Received from Later Specs (applied retroactively in Session 2)

| BP | Originating Spec | Edit Applied to Spec 05 |
|----|-----------------|-------------------------|
| BP-104 | Spec 01 (`SR_GOV_73` governance gate) | `SR_LLM_05` updated with bidirectional reference |
| BP-128 | Spec 13 (`SR_SCALE_25` per-tenant quotas) | `SR_LLM_24` updated to invoke `REUSABLE_QuotaEnforcer` alongside token budget |
| BP-105 | Spec 03 (`SR_CONN_18_BE-01` upload MLAID) | `SR_LLM_25` updated with defense-in-depth bidirectional reference |
| BP-99 | Spec 03 (`SR_CONN_40` cloud LLM as Connection) | `SR_LLM_42` updated with bidirectional reference |
| BP-120 | Spec 13 (`SR_SCALE_30` graceful degradation) | `SR_LLM_42` updated with degradation participation as the cloud-LLM failover role |
| BP-106 | Spec 02 (`SR_DM_15` training task type) | `SR_LLM_50` updated to record training runs via `SR_DM_15` |
| BP-127 | Spec 01 (`SR_GOV_60` consent gate) | `SR_LLM_50` and `SR_LLM_51` updated with bidirectional reference; `SR_LLM_51` extended with per-batch re-verification |

**Total retroactive edits to Spec 05 (Session 2): 6 SR row updates.**

## Session 3 Additions — Back-Propagation from Workbook

Session 3 cross-verified the expanded specs against `Platform_Requirements_and_Exceptions.xlsx` and applied these additions:

| BP | Originating | Edit Applied to Spec 05 |
|----|-------------|-------------------------|
| BP-131 | Workbook BR_LLM_04_Timeout_SE-02 (per-tier timeout values) | `SR_LLM_11` through `SR_LLM_14` updated with explicit 10s/30s/120s/60s hard timeouts |
| BP-132 | Workbook BR_LLM_10 (response cache) + BR_LLM_10_CachePII_BE-01 | **New SRs `SR_LLM_60` and `SR_LLM_61` added** as Section 6 Response Cache. Implements the cache gap Nick's workbook exposed. |

**Total Session 3 edits to Spec 05: 4 existing SR row updates + 2 new SR rows (60, 61).**
