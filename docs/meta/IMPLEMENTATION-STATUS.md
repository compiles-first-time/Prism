# Implementation Status -- Layer 0 Core Entities

| SR ID | Title | Status | Crate | File | Tests | Notes |
|-------|-------|--------|-------|------|-------|-------|
| **FOUND S 1.2** | Tenant Model | **Done** | prism-governance | tenant.rs | 10 unit | Day 3 |
| **FOUND S 1.3.1** | Service Principal | **Done** | prism-identity | service_principal.rs | 10 unit | Day 4 |
| **FOUND S 1.4.1** | LCA Algorithm | Deferred | prism-governance | lca.rs | -- | Week 3+ (long-pole) |
| **FOUND S 1.5.1** | Lifecycle State Machine | **Done** | prism-lifecycle | state_machine.rs | 12 unit | Day 3 |
| **D-22** | Audit Trail (Merkle chain) | **Done** | prism-audit | merkle_chain.rs | 7 unit | Day 2 |
| SR_GOV_01 | Tenant onboarding | **Done** | prism-governance | tenant.rs | 10 unit | Day 3 |
| SR_GOV_10 | Person/identity creation | **Done** | prism-identity | service_principal.rs | 10 unit | Day 4 |
| SR_GOV_16 | ENFORCE rule evaluation | **Done** | prism-governance | rule_engine.rs | 7 unit | Week 2 Day 8 |
| SR_GOV_17 | ADVISE rule evaluation | **Done** | prism-governance | rule_engine.rs | 4 unit | Week 2 Day 8 |
| SR_GOV_18 | ADVISE override justification | **Done** | prism-governance | rule_engine.rs | 13 unit | Week 2 Day 9 |
| SR_GOV_41 | Create approval request | **Done** | prism-governance | approval_chain.rs | 3 unit | Week 3 Day 15 |
| SR_GOV_42 | Compute LCA chain | **Done** | prism-governance | approval_chain.rs | 4 unit | Week 3 Day 15 |
| SR_GOV_43 | Execute approval chain | **Done** | prism-governance | approval_chain.rs | 4 unit | Week 3 Day 15 |
| SR_GOV_44 | Delegation (DEF) | **Done** | prism-governance | approval_chain.rs | 4 unit | Week 3 Day 16 |
| SR_GOV_45 | SLA escalation | **Done** | prism-governance | approval_chain.rs | 3 unit | Week 3 Day 16 |
| SR_GOV_46 | Approval break-glass | **Done** | prism-governance | approval_chain.rs | 4 unit | Week 3 Day 16 |
| SR_GOV_46_REVIEW | Break-glass review | **Done** | prism-governance | approval_chain.rs | 3 unit | Week 3 Day 16 |
| SR_GOV_47 | Audit event writing | **Done** | prism-audit | event_store.rs | 5 unit | Day 2 |
| SR_GOV_48 | Chain verification | **Done** | prism-audit | merkle_chain.rs | 4 unit | Day 2 |
| SR_GOV_49 | Audit query | **Done** | prism-audit | event_store.rs | 1 unit | Day 2 |
| SR_GOV_50 | Audit export | **Done** | prism-audit | audit_export.rs | 6 unit | Week 2 Day 6 |
| SR_GOV_51 | Tamper response | **Done** | prism-audit | tamper_response.rs | 6 unit | Week 2 Day 6 |
| SR_GOV_31 | Compartment creation | **Done** | prism-compliance | compartment.rs | 6 unit | Week 2 Day 7 |
| SR_GOV_32 | Compartment member add | **Done** | prism-compliance | compartment.rs | 4 unit | Week 2 Day 7 |
| SR_GOV_33 | Compartment access check | **Done** | prism-compliance | compartment.rs | 5 unit | Week 2 Day 7 |
| SR_GOV_34 | Compartment member revocation | **Done** | prism-compliance | compartment.rs | 8 unit | Week 2 Day 10 |
| SR_GOV_67 | Alert routing by severity | **Done** | prism-governance | alert_routing.rs | 8 unit | Week 2 Day 10 |
| SR_GOV_19 | Rule publication with dry-run | **Done** | prism-governance | rule_versioning.rs | 8 unit | Week 2 Day 10 |
| SR_GOV_20 | Rule conflict detection | **Done** | prism-governance | rule_versioning.rs | 6 unit | Week 2 Day 10 |
| SR_GOV_21 | Rule rollback | **Done** | prism-governance | rule_versioning.rs | 4 unit | Week 2 Day 11 |
| SR_GOV_22 | Rule export | **Done** | prism-governance | rule_versioning.rs | 5 unit | Week 2 Day 11 |
| SR_GOV_37 | Query analytics capture | **Done** | prism-governance | query_analytics.rs | 3 unit | Week 2 Day 11 |
| SR_GOV_38 | Query analytics aggregation | **Done** | prism-governance | query_analytics.rs | 1 unit | Week 2 Day 11 |
| SR_GOV_39 | Analytics access control | **Done** | prism-governance | query_analytics.rs | 6 unit | Week 2 Day 11 |
| SR_GOV_40 | Analytics export | **Done** | prism-governance | query_analytics.rs | 2 unit | Week 2 Day 11 |
| SR_GOV_35 | Criminal-penalty override | **Done** | prism-compliance | compartment.rs | 4 unit | Week 2 Day 12 |
| SR_GOV_36 | Compartment audit report | **Done** | prism-compliance | compartment.rs | 3 unit | Week 2 Day 12 |
| SR_GOV_68 | Feature flag toggle | **Done** | prism-governance | feature_flags.rs | 5 unit | Week 2 Day 12 |
| SR_GOV_69 | Admin undo | **Done** | prism-governance | admin_undo.rs | 5 unit | Week 2 Day 12 |
| SR_GOV_72 | Rejection justification | **Done** | prism-governance | rejection_validation.rs | 5 unit | Week 2 Day 12 |
| SR_GOV_70 | Connection consent | **Done** | prism-governance | connection_consent.rs | 4 unit | Week 3 Day 13 |
| SR_GOV_71 | Coverage disclosure | **Done** | prism-governance | coverage_enforcement.rs | 4 unit | Week 3 Day 13 |
| SR_GOV_23 | CSA rule registration | **Done** | prism-governance | csa_engine.rs | 4 unit | Week 3 Day 13 |
| SR_GOV_24 | CSA assessment trigger | **Done** | prism-governance | csa_engine.rs | 4 unit | Week 3 Day 13 |
| SR_GOV_25 | CSA evaluator | **Done** | prism-governance | csa_engine.rs | 4 unit | Week 3 Day 13 |
| SR_GOV_26 | CSA BLOCK action | **Done** | prism-governance | csa_engine.rs | 2 unit | Week 3 Day 13 |
| SR_GOV_27 | CSA ANONYMIZE action | **Done** | prism-governance | csa_engine.rs | 2 unit | Week 3 Day 14 |
| SR_GOV_28 | CSA ELEVATE action | **Done** | prism-governance | csa_engine.rs | 2 unit | Week 3 Day 14 |
| SR_GOV_29 | CSA break-glass | **Done** | prism-governance | csa_engine.rs | 4 unit | Week 3 Day 14 |
| SR_GOV_29_REVIEW | Break-glass review | **Done** | prism-governance | csa_engine.rs | 3 unit | Week 3 Day 14 |
| SR_GOV_30 | CSA assessment persistence | **Done** | prism-governance | csa_engine.rs | 2 unit | Week 3 Day 14 |
| SR_GOV_73 | LLM Router Stage 1 | **Done** | prism-governance | governance_hooks.rs | 3 unit | Week 3 Day 14 |
| SR_GOV_74 | DS preflight | **Done** | prism-governance | governance_hooks.rs | 3 unit | Week 3 Day 14 |
| SR_GOV_75 | UI visibility check | **Done** | prism-governance | governance_hooks.rs | 3 unit | Week 3 Day 14 |
| SR_GOV_76 | Connection pull preflight | **Done** | prism-governance | governance_hooks.rs | 4 unit | Week 3 Day 15 |
| SR_GOV_77 | Query rewrite | **Done** | prism-governance | governance_hooks.rs | 4 unit | Week 3 Day 15 |
| SR_GOV_78 | Component preflight | **Done** | prism-governance | governance_hooks.rs | 4 unit | Week 3 Day 15 |
| SR_GOV_52 | Crypto-shredding | Deferred | prism-compliance | crypto_shredding.rs | -- | Week 3+ (needs CaaS) |
| SR_DM_01 | Tenant node creation | **Done** | prism-governance | pg_tenant_repo.rs | -- | Day 3 (PG only; Neo4j Week 2) |
| SR_DM_02 | Person node creation | **Done** | prism-identity | pg_repository.rs | -- | Day 4 (PG only) |
| SR_DM_03 | Compartment node | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 17 |
| SR_DM_04 | Connection node | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 17 |
| SR_DM_05 | Audit events table | **Done** | prism-audit | pg_repository.rs | -- | Day 2 (migration + PG repo) |
| SR_DM_06 | Audit partition maintenance | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 17 |
| SR_DM_07 | DataCollection node | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 17 |
| SR_DM_08 | DataField nodes | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 17 |
| SR_DM_09 | Recommendation node | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 17 |
| SR_DM_10 | Rejection node | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 17 |
| SR_DM_11 | Lifecycle state machine | **Done** | prism-lifecycle | state_machine.rs | 12 unit | Day 3 |
| SR_DM_12 | Component node | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_13 | Component registry | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_14 | Component performance | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_15 | ModelExecution node | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_16 | ModelOutcomeScore | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_17 | Model aggregation | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_18 | Vector embedding | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_19 | Dual embedding store | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_20 | Service account node | **Done** | prism-identity | pg_repository.rs | -- | Day 4 (PG only) |
| SR_DM_21 | SA usage/anomaly log | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 18 |
| SR_DM_22 | Event-driven sync | **Done** | prism-graph | sync_service.rs | 3 unit | Week 3 Day 19 |
| SR_DM_23 | Vector write enforcer | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 19 |
| SR_DM_24 | Graph maintenance | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 19 |
| SR_DM_25 | Notification log | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 19 |
| SR_DM_26 | User preferences | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 19 |
| SR_DM_27 | Tenant isolation (query) | **Done** | prism-core | tenant_filter.rs | 3 unit | Day 4 (single-tenant stub) |
| SR_DM_28 | Tenant isolation audit | **Done** | prism-graph | data_model.rs | 3 unit | Week 3 Day 19 |
| SR_DM_29 | Feature flag cache | **Done** | prism-graph | data_model.rs | 2 unit | Week 3 Day 19 |
| SR_CONN_01 | Connection request | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_02 | Connection approval | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_03 | Connection consent | **Done** | prism-adapters | connection_lifecycle.rs | 1 unit | Week 3 Day 20 |
| SR_CONN_04 | Credential provisioning | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_05 | Connection test | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_06 | Connection activation | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_07 | Mark degraded | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_08 | Suspend connection | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_09 | Decommission | **Done** | prism-adapters | connection_lifecycle.rs | 2 unit | Week 3 Day 20 |
| SR_CONN_10 | Recovery | **Done** | prism-adapters | connection_lifecycle.rs | 3 unit | Week 3 Day 20 |
| SR_CONN_11 | Delegated user adapter | **Done** | prism-adapters | connection_adapters.rs | 1 unit | Day 21 |
| SR_CONN_12 | Scoped SA adapter | **Done** | prism-adapters | connection_adapters.rs | 1 unit | Day 21 |
| SR_CONN_13 | Privileged SA adapter | **Done** | prism-adapters | connection_adapters.rs | 1 unit | Day 21 |
| SR_CONN_14 | OAuth adapter | **Done** | prism-adapters | connection_adapters.rs | 1 unit | Day 21 |
| SR_CONN_15 | RPA adapter | **Done** | prism-adapters | connection_adapters.rs | 1 unit | Day 21 |
| SR_CONN_16 | AI navigation adapter | **Done** | prism-adapters | connection_adapters.rs | 1 unit | Day 21 |
| SR_CONN_17 | Bulk import adapter | **Done** | prism-adapters | connection_adapters.rs | 1 unit | Day 21 |
| SR_CONN_18 | User upload adapter | **Done** | prism-adapters | connection_adapters.rs | 2 unit | Day 21 |
| SR_CONN_19 | Log stream adapter | **Done** | prism-adapters | log_ingestion.rs | 2 unit | Day 21 |
| SR_CONN_20 | Parser selection | **Done** | prism-adapters | log_ingestion.rs | 2 unit | Day 21 |
| SR_CONN_21 | PII redaction | **Done** | prism-adapters | log_ingestion.rs | 2 unit | Day 21 |
| SR_CONN_22 | Multi-system correlation | **Done** | prism-adapters | log_ingestion.rs | 2 unit | Day 21 |
| SR_CONN_23 | Ingestion modes | **Done** | prism-adapters | log_ingestion.rs | 2 unit | Day 21 |
| SR_CONN_24 | Log ingestion metrics | **Done** | prism-adapters | log_ingestion.rs | 1 unit | Day 21 |
| SR_CONN_25 | Normalized record builder | **Done** | prism-adapters | classification_gate.rs | 1 unit | Day 22 |
| SR_CONN_26 | Stage 1 technical classification | **Done** | prism-adapters | classification_gate.rs | 1 unit | Day 22 |
| SR_CONN_27 | Classification gate orchestrator | **Done** | prism-adapters | classification_gate.rs | 2 unit | Day 22 |
| SR_CONN_28 | Stage 2 security classification | **Done** | prism-adapters | classification_gate.rs | 2 unit | Day 22 |
| SR_CONN_29 | Stage 3 semantic classification | **Done** | prism-adapters | classification_gate.rs | 1 unit | Day 22 |
| SR_CONN_30 | Stage 4 relationship inference | **Done** | prism-adapters | classification_gate.rs | 1 unit | Day 22 |
| SR_CONN_31 | Stage 5 quality assessment | **Done** | prism-adapters | classification_gate.rs | 1 unit | Day 22 |
| SR_CONN_32 | Quarantine | **Done** | prism-adapters | connection_operations.rs | 2 unit | Day 22 |
| SR_CONN_33 | Quarantine expiry | **Done** | prism-adapters | connection_operations.rs | 2 unit | Day 22 |
| SR_CONN_34 | Pull lock | **Done** | prism-adapters | connection_operations.rs | 2 unit | Day 22 |
| SR_CONN_35 | Schema change detection | **Done** | prism-adapters | connection_operations.rs | 2 unit | Day 22 |
| SR_CONN_36 | Rate budget check | **Done** | prism-adapters | connection_operations.rs | 2 unit | Day 22 |
| SR_CONN_37 | Connection KPIs | **Done** | prism-adapters | connection_operations.rs | 2 unit | Day 22 |
| SR_CONN_38 | Classification override store | **Done** | prism-adapters | connection_operations.rs | 1 unit | Day 22 |
| SR_CONN_39 | Apply overrides | **Done** | prism-adapters | connection_operations.rs | 1 unit | Day 22 |
| SR_CONN_40 | Cloud LLM provider | **Done** | prism-adapters | connection_operations.rs | 1 unit | Day 22 |
| SR_CONN_41 | Deprecation alerting | **Done** | prism-adapters | connection_operations.rs | 1 unit | Day 22 |
| SR_CONN_42 | Paywall governance | **Done** | prism-adapters | connection_operations.rs | 2 unit | Day 22 |
| SR_CONN_43 | Bulk import logging | **Done** | prism-adapters | connection_operations.rs | 1 unit | Day 22 |
| SR_CONN_44 | Health dashboard | **Done** | prism-adapters | connection_operations.rs | 1 unit | Day 22 |
| SR_INT_01 | Tenant graph init | **Done** | prism-llm | intelligence.rs | 2 unit | Day 23 |
| SR_INT_02 | Tagging pipeline trigger | **Done** | prism-llm | intelligence.rs | 2 unit | Day 23 |
| SR_INT_03 | Stage 3 semantic tagging | **Done** | prism-llm | intelligence.rs | 2 unit | Day 23 |
| SR_INT_04 | Stage 4 relationship inference | **Done** | prism-llm | intelligence.rs | 3 unit | Day 23 |
| SR_INT_05 | DataSnapshot service | **Done** | prism-llm | intelligence.rs | 2 unit | Day 23 |
| SR_INT_06 | Stage 5 quality assessment | **Done** | prism-llm | intelligence.rs | 2 unit | Day 23 |
| SR_INT_07 | TrendAnalysis | **Done** | prism-llm | intelligence.rs | 3 unit | Day 23 |
| SR_INT_08 | Human review queue | **Done** | prism-llm | intelligence.rs | 2 unit | Day 23 |
| SR_INT_09 | Coverage calculator | **Done** | prism-llm | intelligence.rs | 2 unit | Day 24 |
| SR_INT_10 | Process emergence | **Done** | prism-llm | intelligence.rs | 2 unit | Day 24 |
| SR_INT_11 | DataGroup membership | **Done** | prism-llm | intelligence.rs | 2 unit | Day 24 |
| SR_INT_12 | Tag weights | **Done** | prism-llm | intelligence.rs | 2 unit | Day 24 |
| SR_INT_13 | Completeness tags | **Done** | prism-llm | intelligence.rs | 2 unit | Day 24 |
| SR_INT_14 | Recommendation accuracy | **Done** | prism-llm | intelligence.rs | 3 unit | Day 24 |
| SR_INT_15 | Vector semantic search | **Done** | prism-llm | intelligence.rs | 3 unit | Day 24 |
| SR_INT_16 | Cascade Impact Analysis | **Done** | prism-llm | intelligence.rs | 3 unit | Day 25 |
| SR_INT_17 | Semantic Disambiguation | **Done** | prism-llm | intelligence.rs | 2 unit | Day 25 |
| SR_INT_18 | DS data gathering | **Done** | prism-llm | intelligence.rs | 2 unit | Day 25 |
| SR_INT_19 | Research Agent | **Done** | prism-llm | intelligence.rs | 2 unit | Day 25 |
| SR_INT_20 | Graph viz | **Done** | prism-llm | intelligence.rs | 2 unit | Day 25 |
| SR_INT_21 | Agent feedback loop | **Done** | prism-llm | intelligence.rs | 2 unit | Day 25 |
| SR_INT_22 | Cross-tenant learning | **Done** | prism-llm | intelligence.rs | 3 unit | Day 25 |
| SR_INT_23 | Query rewrite | **Done** | prism-llm | intelligence.rs | 2 unit | Day 25 |
| SR_INT_24 | Proactive triggers | **Done** | prism-llm | intelligence.rs | 3 unit | Day 25 |
| SR_INT_25 | Maintenance orchestrator | **Done** | prism-llm | intelligence.rs | 2 unit | Day 25 |
| SR_INT_26 | Query cost estimator | **Done** | prism-llm | intelligence.rs | 3 unit | Day 25 |
| SR_INT_27 | Bulk import worker | **Done** | prism-llm | intelligence.rs | 1 unit | Day 25 |
| SR_INT_28 | Read-through cache | **Done** | prism-llm | intelligence.rs | 3 unit | Day 25 |
| SR_INT_29 | DR drills | **Done** | prism-llm | intelligence.rs | 3 unit | Day 25 |
| SR_INT_30 | Tenant offboarding | **Done** | prism-llm | intelligence.rs | 3 unit | Day 25 |
| **REUSABLE** | MerkleChainHasher | **Done** | prism-audit | merkle_chain.rs | 7 unit | Day 2 |
| **REUSABLE** | AuditLogger | **Done** | prism-audit | event_store.rs | 7 unit | Day 2 |
| **REUSABLE** | TenantFilter | **Done** | prism-core | tenant_filter.rs | 3 unit | Day 4 |
| **REUSABLE** | EventBusPublisher | **Done** | prism-runtime | event_bus.rs | 3 unit | Day 4 |
| **REUSABLE** | SyncCoordinator | **Done** | prism-graph | sync_coordinator.rs | 5 unit | Day 5 (stub only) |
