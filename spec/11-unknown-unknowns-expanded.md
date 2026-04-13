# Spec 11 Expanded: Unknown Unknowns Register

**Source:** `001/spec/11-unknown-unknowns.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Reserved SR range:** `SR_UU_01` through `SR_UU_99`
**Last updated:** 2026-04-10

---

## Purpose

Spec 11 is a register, not an operational pipeline. The architectural responses to UU-1 through UU-13 and the layer-specific discoveries (IL-1 through IL-10, LR-1 through LR-9, DS-1 through DS-12, INT-1 through INT-15, CC-1 through CC-14) are all resolved through SRs in other specs. Spec 11 here provides a reference table mapping each unknown to its resolving SR(s) and the operational SRs that maintain the register over time.

## Reference Table — UU to Resolving SR

| UU | Unknown | Resolving SR(s) |
|----|---------|----------------|
| UU-1 | Data quality assessment | `SR_INT_06` (Stage 5), `SR_DM_25`, `SR_DS_09` |
| UU-2 | Cold start problem | `SR_INT_01` (empty graph init), `SR_FW_06` (quick-win features) |
| UU-3 | Hallucination risk | `SR_LLM_30`-`SR_LLM_36` (DBE v2), `SR_GOV_53` (default-restrictive), `SR_INT_08` (review queue) |
| UU-4 | Human correction workflow | `SR_CONN_38` / `SR_CONN_39` (override map), `SR_DS_15` (rejection), `SR_INT_21` (feedback loop) |
| UU-5 | Data freshness | `SR_DM_07` (DataCollection freshness_policy), `SR_DS_09` (freshness in confidence formula), `SR_DS_03` (refresh if stale) |
| UU-6 | Cross-company learning boundary | `SR_GOV_59` (opt-in), `SR_INT_22` (aggregate without raw) |
| UU-7 | Explainability chain | `SR_LLM_35` (V4 check), `SR_DS_07` (assembly), `SR_GOV_71` (coverage disclosure) |
| UU-8 | Conflicting data sources | `SR_DS_10` (conflict detection), `SR_INT_04` (semantic equivalence), `SR_DS_31` (source comparison) |
| UU-9 | Structural change detection | `SR_CONN_35` (schema change), `SR_GOV_58` (severity response) |
| UU-10 | Permission inheritance | `SR_DM_27` (query rewrite), `SR_GOV_77` (intelligence query gate), `SR_GOV_33` (compartment check) |
| UU-11 | Credential lifecycle | `SR_SA_20`-`SR_SA_22` (rotation), `SR_SA_51` (IAM integration), `SR_GOV_13` (termination cascade) |
| UU-12 | Tribal knowledge / shadow IT | `SR_CONN_18` (Type 7 upload), `SR_CONN_19`-`SR_CONN_24` (Type 8 logs), `SR_DS_35` (Event Calendar) |
| UU-13 | Partial-map decision quality | `SR_INT_09` (coverage calculator), `SR_GOV_71` (coverage enforcement), `SR_DS_08` (confidence threshold) |

## Reference Table — Layer Discoveries to Resolving SR

| ID | Discovery | Resolving SR(s) |
|----|-----------|----------------|
| IL-1 | Graph query authorization | `SR_INT_23`, `SR_DM_27` |
| IL-2 | Concurrent write conflicts | `SR_DM_07`-`SR_DM_08` (MERGE) |
| IL-3 | Graph schema evolution | `SR_DM_24` (maintenance), additive-only changes |
| IL-4 | Embedding storage limits | `SR_DM_24` (retention), `SR_DM_19` (dual storage purging) |
| IL-5 | Feedback loop bias | `SR_INT_21` (random sampling audits in feedback tracker) |
| IL-6 | Graph query performance cliff | `SR_INT_26` (cost estimator) |
| IL-7 | Semantic search governance filtering | `SR_INT_15` (post-filter) |
| IL-8 | Bulk import performance | `SR_INT_27` (dedicated queue) |
| IL-9 | Caching strategy | `SR_INT_28` (read-through cache) |
| IL-10 | Graph query explainability | `SR_INT_16` (CIA returns traversal path) |
| LR-1 | Conversation persistence | `SR_LLM_27` |
| LR-2 | Output normalization | `SR_LLM_28` |
| LR-3 | Prompt injection in uploads | `SR_LLM_25` (MLAID), `SR_CONN_18_BE-01` |
| LR-4 | Cost attribution multi-model | `SR_LLM_29` |
| LR-5 | Provider deprecation | `SR_CONN_41` |
| LR-6 | Streaming + verification conflict | `SR_LLM_22`, `SR_LLM_23` |
| LR-7 | Rate limit coordination | `SR_CONN_36`, `SR_LLM_24` |
| LR-8 | Tenant prompt vs safety | `SR_LLM_20`, `SR_LLM_21` |
| LR-9 | Fine-tuning consent | `SR_GOV_60`, `SR_LLM_51` |
| DS-1 | Recommendation prerequisites | `SR_DS_24` |
| DS-2 | A/B testing recommendations | `SR_DS_24` (with explicit consent mode flag) |
| DS-3 | Hierarchical decision escalation | `SR_GOV_41`-`SR_GOV_45` |
| DS-4 | Recommendation triggers | `SR_DS_24` |
| DS-5 | Cooling-off periods | `SR_DS_24` |
| DS-6 | Batch recommendations | `SR_DS_22` |
| DS-7 | Recommendation rollback | `SR_DS_14` (auto-invalidation), reverse-rec capability |
| DS-8 | Time zone handling | UTC storage in all SRs, locale display in `SR_UI_42` |
| DS-9 | Recommendation noise | `SR_DS_30` |
| DS-10 | Explanation depth personalization | `SR_DS_23` |
| DS-11 | Confidence calibration | `SR_DS_26` |
| DS-12 | Multi-stakeholder | `SR_DS_29` |
| INT-1 | Offline mode | `SR_UI_36` |
| INT-2 | Multi-tab behavior | `SR_UI_37` |
| INT-3 | Print and export views | `SR_UI_30` (audit export pattern) |
| INT-4 | Feature flags | `SR_GOV_68`, `SR_UI_*` |
| INT-5 | Browser compatibility | Spec 07 documented baseline |
| INT-6 | Large list virtualization | `SR_UI_31` |
| INT-7 | Slow network handling | `SR_UI_32`, `SR_UI_34` |
| INT-8 | Error boundary | Spec 07 documented |
| INT-9 | XSS injection protection | `SR_UI_38`, `SR_UI_39` |
| INT-10 | Session theft protection | `SR_UI_38`, `SR_UI_04` |
| INT-11 | Frontend RUM | `SR_UI_44` |
| INT-12 | Undo/redo for admin | `SR_UI_35`, `SR_GOV_69` |
| INT-13 | Contextual help | Spec 07 admin help touchpoint |
| INT-14 | User shortcuts | Spec 07 personal workspace |
| INT-15 | Notification center | `SR_UI_25` |
| CC-1 | Component versioning | `SR_CAT_44`, `SR_CAT_08` |
| CC-2 | Component deprecation | `SR_CAT_38` |
| CC-3 | Credential scope drift | `SR_CAT_43` |
| CC-4 | Dependency hell | `SR_CAT_34` |
| CC-5 | Component failure containment | `SR_CAT_31` |
| CC-6 | Resource limits | `SR_CAT_31` |
| CC-7 | AI-generated code security | `SR_CAT_22`, `SR_GOV_62` |
| CC-8 | Documentation freshness | `SR_CAT_24` |
| CC-9 | Custom component SDK | `SR_CAT_25` |
| CC-10 | Component observability | `SR_CAT_42`, `SR_DM_14` |
| CC-11 | Fast rollback | `SR_CAT_35` |
| CC-12 | Component audit trail | `SR_CAT_40` |
| CC-13 | License and legal | `SR_CAT_41` |
| CC-14 | Generation quality tracking | `SR_CAT_23` |

## Operational SRs for the Register

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_UU_01` | --- | governance | Add a new unknown unknown to the register: documented description, status (open / resolved), architectural response, affected specs, back-propagation results. | `REUSABLE_PgWriter`, register table | Discovery during implementation or operations | `UuRegisterInput` | New row in `unknowns_register`; cross-references updated | JSON | `Result { uu_id }` | If new decision required: HALT and ask Nick. | Provides a controlled intake for newly discovered gaps. |
| `SR_UU_02` | --- | governance | Resolve an existing UU by linking it to one or more SRs that implement the response. | `REUSABLE_PgWriter` | After implementation | `UuResolveInput { uu_id, resolving_sr_ids }` | Status set to resolved; cross-references stored | JSON | `Result` | End | Closes the loop on a discovered unknown. |
| `SR_UU_03` | --- | governance | Run regression back-propagation when a new UU is added: scan all expanded specs for impact and produce BP entries as needed. | Cross-reference validator | After `SR_UU_01` | `BackPropagationInput { uu_id }` | List of BP entries | JSON | `Result` | Apply BP fixes to affected specs. | Mirrors the BP pattern used for new architectural decisions. |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Some original UUs are resolved by multiple SRs. Are they fully covered? | The reference tables list all resolving SRs. Coverage is determined by the resolving SRs, not by the count of references. |
| 2 | What about UUs that are partially resolved? | None remain partial. All UU-1 through UU-13 are listed as resolved in 001/STATE.md. |
| 3 | New UUs may be discovered during build. | `SR_UU_01` provides the controlled intake. |

## Spec 11 Summary

| Metric | Value |
|--------|-------|
| Reference tables | 2 |
| Operational SRs | 3 |
| Total SR rows | 3 |
| BP entries created | 0 (Spec 11 is reference; no new SRs trigger BP) |
| New decisions | 0 |

**Status:** Self-audit complete.
