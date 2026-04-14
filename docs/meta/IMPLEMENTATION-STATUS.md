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
| SR_GOV_41 | Create approval request | Deferred | prism-governance | lca.rs | -- | Needs LCA |
| SR_GOV_42 | Compute LCA chain | Deferred | prism-governance | lca.rs | -- | Week 3+ |
| SR_GOV_43 | Execute approval chain | Deferred | prism-governance | lca.rs | -- | Week 3+ |
| SR_GOV_44 | Delegation (DEF) | Deferred | prism-governance | def.rs | -- | Week 3+ |
| SR_GOV_45 | SLA escalation | Deferred | prism-governance | def.rs | -- | Week 3+ |
| SR_GOV_47 | Audit event writing | **Done** | prism-audit | event_store.rs | 5 unit | Day 2 |
| SR_GOV_48 | Chain verification | **Done** | prism-audit | merkle_chain.rs | 4 unit | Day 2 |
| SR_GOV_49 | Audit query | **Done** | prism-audit | event_store.rs | 1 unit | Day 2 |
| SR_GOV_50 | Audit export | **Done** | prism-audit | audit_export.rs | 6 unit | Week 2 Day 6 |
| SR_GOV_51 | Tamper response | **Done** | prism-audit | tamper_response.rs | 6 unit | Week 2 Day 6 |
| SR_GOV_31 | Compartment creation | **Done** | prism-compliance | compartment.rs | 6 unit | Week 2 Day 7 |
| SR_GOV_32 | Compartment member add | **Done** | prism-compliance | compartment.rs | 4 unit | Week 2 Day 7 |
| SR_GOV_33 | Compartment access check | **Done** | prism-compliance | compartment.rs | 5 unit | Week 2 Day 7 |
| SR_GOV_52 | Crypto-shredding | Deferred | prism-compliance | crypto_shredding.rs | -- | Week 3+ (needs CaaS) |
| SR_DM_01 | Tenant node creation | **Done** | prism-governance | pg_tenant_repo.rs | -- | Day 3 (PG only; Neo4j Week 2) |
| SR_DM_02 | Person node creation | **Done** | prism-identity | pg_repository.rs | -- | Day 4 (PG only) |
| SR_DM_05 | Audit events table | **Done** | prism-audit | pg_repository.rs | -- | Day 2 (migration + PG repo) |
| SR_DM_11 | Lifecycle state machine | **Done** | prism-lifecycle | state_machine.rs | 12 unit | Day 3 |
| SR_DM_20 | Service account node | **Done** | prism-identity | pg_repository.rs | -- | Day 4 (PG only) |
| SR_DM_27 | Tenant isolation (query) | **Done** | prism-core | tenant_filter.rs | 3 unit | Day 4 (single-tenant stub) |
| **REUSABLE** | MerkleChainHasher | **Done** | prism-audit | merkle_chain.rs | 7 unit | Day 2 |
| **REUSABLE** | AuditLogger | **Done** | prism-audit | event_store.rs | 7 unit | Day 2 |
| **REUSABLE** | TenantFilter | **Done** | prism-core | tenant_filter.rs | 3 unit | Day 4 |
| **REUSABLE** | EventBusPublisher | **Done** | prism-runtime | event_bus.rs | 3 unit | Day 4 |
| **REUSABLE** | SyncCoordinator | **Done** | prism-graph | sync_coordinator.rs | 5 unit | Day 5 (stub only) |
