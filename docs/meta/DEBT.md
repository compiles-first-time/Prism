# Technical Debt Register

Intentional shortcuts taken during implementation with justification, impact, and resolution plan.

| Item | Why Deferred | SR Impact | Target Resolution |
|------|-------------|-----------|-------------------|
| Neo4j dual-write stubbed | Focus PG-first to prove domain logic | SR_DM_01 partial (PG only) | Week 2 |
| Single-tenant TenantFilter | MVP is single-tenant per decision #5 | SR_DM_27 simplified | V1.1 |
| EventBusPublisher optional in AuditLogger | Redis integration secondary to hash chain correctness | SR_GOV_47 partial | Day 4 |
| No audit partition management | Monthly job not needed during dev | SR_DM_06 | Week 2 |
| SyncCoordinator is logging-only stub | Neo4j writes deferred | REUSABLE_SyncCoordinator | Week 2 |
