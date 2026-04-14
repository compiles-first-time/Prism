# Build Log

Reverse-chronological record of implementation sessions.

---

## Session 2026-04-14 -- Day 11 (Week 2): Rule rollback, export, query analytics (SR_GOV_21-22, SR_GOV_37-40)

### Implemented
- SR_GOV_21 RuleRollbackService: atomic rollback to a prior ruleset version
  - Validates target version exists and is not already active
  - Requires non-empty rollback reason
  - Atomically promotes the target version (deactivates current)
  - Audit trail: `governance.rule_rolled_back` at HIGH severity
  - 4 new tests: rollback succeeds, nonexistent version, already-active, empty reason
- SR_GOV_22 RuleExportService: signed export of rules in effect for regulatory review
  - RuleExportSigner trait for pluggable signing
  - Exports in JSON Lines, CSV, or PDF (canonical JSON) formats
  - Audit trail: `governance.rules_exported` at MEDIUM severity
  - 5 new tests: JSON lines export, CSV format, PDF format, no active version, signature determinism
- SR_GOV_37 QueryAnalyticsService.capture(): privacy-level stripping for query events
  - Anonymous: user_id, role, department all stripped
  - Role: user_id stripped, role/department retained
  - Individual: all fields retained
  - StoredAnalyticsEvent entity, QueryAnalyticsRepository trait
  - 3 new tests: anonymous strips all, role strips user_id, individual retains all
- SR_GOV_38 QueryAnalyticsService.aggregate(): periodic aggregation into summaries
  - AnalyticsAggregate entity, AnalyticsAggregateRepository trait
  - MVP tenant-level aggregation (per-role/dept deferred to PG impl)
  - 1 new test: aggregation writes summary
- SR_GOV_39 AnalyticsAccessService: access control matrix per D-17
  - Anonymous: visible to anyone
  - RoleBased: visible to department_head, c_suite, platform_admin, tenant_admin
  - Individual: visible only to self or designated admin (admin access audited)
  - 6 new tests: anonymous allows, role allows dept head, role denies regular, individual allows self, individual allows admin, individual denies other
- SR_GOV_40 AnalyticsExportService: signed analytics export inheriting SR_GOV_39 access control
  - Access check (SR_GOV_39) runs before export generation
  - AnalyticsExportSigner trait for pluggable signing
  - 2 new tests: export succeeds with access, export denied without access

### Design Decisions
- RuleRollbackService is separate from RulePublicationService -- different authorization requirements (rollback is emergency, publication is planned)
- RuleExportSigner is a separate trait from the audit ExportSigner -- rule exports may use a different key pair
- QueryAnalyticsService._audit is reserved for future use -- capture events are high-volume inline writes; per-event audit logging deferred to PG-level triggers
- ELEVATED_ROLES constant defines who can access role-scoped analytics; modifiable via governance rules later
- Admin access to individual analytics generates a HIGH-severity audit event (surveillance prevention per D-17)
- PrivacyLevel, AnalyticsScope, ComplexityTier, QueryOutcome enums added to prism-core

### Files Changed
- `crates/prism-core/src/types/enums.rs` -- added PrivacyLevel, AnalyticsScope, ComplexityTier, QueryOutcome enums
- `crates/prism-core/src/types/requests.rs` -- added 10 request/result types for SR_GOV_37-40
- `crates/prism-governance/src/rule_versioning.rs` -- added RuleRollbackService, RuleExportService, RuleExportSigner trait, 9 tests
- `crates/prism-governance/src/query_analytics.rs` -- new file, 4 services + 5 traits + 12 tests
- `crates/prism-governance/src/lib.rs` -- registered query_analytics module

### Test Summary
- 21 new tests (4 rollback + 5 export + 3 capture + 1 aggregation + 6 access + 2 analytics export)
- 161 total workspace tests, all passing
- All quality gates green: fmt, clippy, test, check

---

## Session 2026-04-14 -- Day 10 (Week 2): Compartment revocation, alert routing, rule versioning

### Implemented
- SR_GOV_34 revoke_member() on CompartmentService: compartment membership revocation with session termination
  - SessionTerminator trait: pluggable session termination on revocation
  - Validates exactly-one-of person/role, compartment existence, tenant isolation
  - Terminates active sessions exposing compartment-bound data (via SessionTerminator)
  - Audit trail: `compartment.member_removed` at HIGH severity
  - CompartmentMembershipRemoveRequest/Result types in prism-core
  - remove_member() added to CompartmentRepository trait
  - 8 new tests: remove person/role, not-found returns false, validation, nonexistent compartment, access denied after revocation, session termination, graceful without terminator
- SR_GOV_67 AlertRoutingService: severity-based alert routing per BP-29
  - Severity matrix: CRITICAL→page+SMS+in-app+email, HIGH→in-app+email, MEDIUM→in-app+digest, LOW→digest only
  - AlertChannelDispatcher trait: pluggable channel implementations
  - AlertHistoryRepository trait: dispatch history for acknowledgement tracking
  - Channel failure resilience: failed channels logged, remaining channels still dispatched
  - AlertEvent/AlertDispatchResult/AlertHistoryEntry types in prism-core
  - AlertChannel enum (Page, Sms, InApp, Email, Digest) in prism-core
  - 8 new tests: all four severity levels, channel failure resilience, history recording, matrix validation, missing channel handling
- SR_GOV_19 RulePublicationService: governance ruleset versioning with dry-run
  - Grammar validation: non-empty rules, valid action patterns, JSON object conditions
  - Dry-run engine: re-evaluates proposed rules against recent decision sample
  - SR_GOV_19_BE-01: promotion blocked when >5% of decisions change (delta threshold)
  - Atomic version promotion: deactivates old version, activates new
  - Version numbering: monotonically increasing per tenant
  - Conflict detection gate: SR_GOV_20 runs before dry-run, HIGH conflicts block
  - RulesetVersionRepository + DecisionSampleRepository traits in prism-core
  - RulesetVersion entity, RulePublishRequest/Result, DryRunReport types
  - 8 new tests: low delta promotes, high delta blocks, empty ruleset, invalid grammar, no history promotes, version incrementing, contradiction blocks
- SR_GOV_20 ConflictDetector: static analysis of governance rulesets
  - Contradiction detection: ENFORCE vs ADVISE with overlapping conditions → HIGH severity, blocks promotion
  - Subsumption detection: one rule's conditions are a strict subset of another's → MEDIUM
  - Overlap detection: partial condition overlap → MEDIUM
  - Wildcard action matching (e.g., "*" overlaps with any specific action)
  - ConflictType enum, ConflictScanRequest, RuleConflictReport types
  - 6 new tests: contradiction, subsumption, non-overlapping, wildcard overlap, contradiction blocks, empty ruleset

### Design Decisions
- SessionTerminator is optional on CompartmentService (via `with_session_terminator` constructor) for backward compatibility with existing services that don't need session kill
- AlertRoutingService dispatches all available channels even if some fail -- alert delivery is best-effort per channel but the overall route succeeds
- RulePublicationService integrates ConflictDetector as a gate BEFORE dry-run -- no point computing dry-run if rules are self-contradictory
- Delta threshold is a constant (5.0%) per SR_GOV_19_BE-01; can be made per-tenant later via governance_rules
- ConflictDetector is a pure-function analyzer (no I/O) -- can be called standalone or as part of the publication pipeline
- Condition matching logic is shared between RuleEngine and RulePublicationService (same simple attribute-matching model)

### Files Changed
- `crates/prism-core/src/types/enums.rs` -- added ConflictType, AlertChannel enums
- `crates/prism-core/src/types/requests.rs` -- added 16 request/result types for SR_GOV_34, SR_GOV_67, SR_GOV_19-22
- `crates/prism-core/src/types/entities.rs` -- added RulesetVersion, AlertHistoryEntry entities
- `crates/prism-core/src/repository.rs` -- added remove_member() to CompartmentRepository, RulesetVersionRepository, DecisionSampleRepository traits
- `crates/prism-compliance/src/compartment.rs` -- added revoke_member(), SessionTerminator trait, 8 tests
- `crates/prism-governance/src/alert_routing.rs` -- new file, AlertRoutingService + AlertChannelDispatcher + AlertHistoryRepository + 8 tests
- `crates/prism-governance/src/rule_versioning.rs` -- new file, RulePublicationService + ConflictDetector + 14 tests
- `crates/prism-governance/src/lib.rs` -- registered alert_routing + rule_versioning modules

### Test Summary
- 30 new tests (8 compartment revocation + 8 alert routing + 8 publication + 6 conflict detection)
- 15 new types in prism-core (enums, entities, request/result structs)
- 140 total workspace tests, all passing
- All quality gates green: fmt, clippy, test, check

---

## Session 2026-04-14 -- Day 9 (Week 2): ADVISE override justification (SR_GOV_18)

### Implemented
- SR_GOV_18 capture_justification() on RuleEngine: validates and records ADVISE override justifications
  - JustificationValidator: reusable pure-logic validator with:
    - Empty/whitespace rejection
    - Minimum 20-character length (BP-134)
    - Filler-word blocklist: "because", "ok", "n/a", "idk", "i don't know", "nope", etc.
    - Repeated-character detection (e.g., "aaaaaaa...")
    - Blocklist words allowed when embedded in longer meaningful text
  - Audit trail: `governance.advise_override_justified` on accept, `governance.justification_rejected` on reject
  - SR_GOV_18_BE-01: rejected justifications return specific guidance to the user
- OverrideJustificationRequest/Result types in prism-core
- 13 new tests: 7 integration (accept/reject/empty/whitespace/short/filler/repeated/category) + 6 pure validator unit tests

### Design Decisions
- JustificationValidator is a standalone struct with a static validate() method -- reusable by SR_GOV_72 (rejection justification) later
- Blocklist matching is exact-match on the full trimmed text; "because" alone is blocked, but "I'm overriding because..." passes
- 20-character minimum per BP-134 prevents trivially short non-blocklisted text
- Repeated-character check catches padding attempts (e.g., "xxxxxxxxxxxxxxxxxxxx")

### Files Changed
- `crates/prism-core/src/types/requests.rs` -- added OverrideJustificationRequest/Result
- `crates/prism-governance/src/rule_engine.rs` -- added capture_justification(), JustificationValidator, FILLER_BLOCKLIST, 13 tests

### Test Summary
- 13 new tests in prism-governance (7 integration + 6 validator)
- 95 total workspace tests, all passing
- All quality gates green: fmt, clippy, test, check

---

## Session 2026-04-13 -- Day 8 (Week 2): Governance rule engine (SR_GOV_16-17)

### Implemented
- RuleEngine (SR_GOV_16, SR_GOV_17): governance rule evaluation engine
  - SR_GOV_16 evaluate_enforce(): non-overridable ENFORCE rules, DENY on any match, default-DENY on repo failure (SE-01)
  - SR_GOV_17 evaluate_advise(): overridable ADVISE rules returning ALLOW, ALLOW_WITH_WARNING, or REQUIRE_JUSTIFICATION
  - Simple attribute-matching condition evaluator (MVP; replaceable with JSONLogic)
  - GovernanceRule struct with rule_class, action_pattern, condition JSON, advisory_message
  - GovernanceRuleRepository trait for rule persistence
  - RuleClass, EnforceDecision, AdviseDecision enums
  - Audit trail integration for all evaluation outcomes
  - 11 unit tests: ENFORCE allow/deny/failsafe/multi-rule/tenant-isolation, ADVISE allow/warning/justification, condition matcher
- Request/result types: RuleEvaluationRequest, EnforceEvaluationResult, AdviseEvaluationResult

### Design Decisions
- Condition matching is simple key-value equality (MVP); JSONLogic can be plugged in later via same interface
- SR_GOV_16_SE-01 failsafe: DENY by default when rule repo is unavailable (fail-closed)
- ADVISE `requires_justification` is a rule-level flag in the condition JSON (not a separate field)
- ENFORCE denials are audited at HIGH severity; ADVISE at LOW
- Rule actions use pattern matching (e.g., "automation.activate", "data.export")
- Tenant isolation: rules are always scoped to the requesting tenant_id

### Files Changed
- `crates/prism-core/src/types/enums.rs` -- added RuleClass, EnforceDecision, AdviseDecision
- `crates/prism-core/src/types/requests.rs` -- added GovernanceRule, RuleEvaluationRequest, EnforceEvaluationResult, AdviseEvaluationResult
- `crates/prism-core/src/repository.rs` -- added GovernanceRuleRepository trait
- `crates/prism-governance/src/rule_engine.rs` -- new file, RuleEngine + 11 tests
- `crates/prism-governance/src/lib.rs` -- registered rule_engine module

### Test Summary
- 11 new tests in prism-governance (7 ENFORCE + 4 ADVISE/condition)
- 82 total workspace tests, all passing
- All quality gates green: fmt, clippy, test, check

---

## Session 2026-04-13 -- Day 7 (Week 2): Visibility compartments (SR_GOV_31-33)

### Implemented
- CompartmentService (SR_GOV_31, SR_GOV_32, SR_GOV_33): visibility compartment engine for criminal-penalty data isolation
  - SR_GOV_31 create(): compartment with classification level, purpose, initial members, criminal_penalty_isolation flag
  - SR_GOV_32 add_member(): add person or role to compartment with validation (exactly one of person/role)
  - SR_GOV_33 check_access(): principal must be member of ALL resource compartments (direct or via role)
  - SR_GOV_31_BE-01: criminal penalty isolation requires Restricted or CriminalPenalty classification
  - Audit trail integration for create and add_member operations
- Compartment + CompartmentMembership entities in prism-core
- ClassificationLevel + AccessDecision enums in prism-core
- CompartmentRepository trait in prism-core
- Request/result types: CompartmentCreateRequest/Result, CompartmentMembershipAddRequest/Result, CompartmentAccessCheckRequest/Result
- Migration 009_create_compartments.sql: compartments + compartment_members tables with indexes
- 15 unit tests covering creation, validation, member management, and access checks

### Design Decisions
- Compartments live in prism-compliance (not prism-governance) since they are a compliance mechanism
- Access check is ALL-compartments (principal must be member of every compartment the resource belongs to)
- Role-based membership: if a person holds a role that is a compartment member, they get access
- Default-allow for non-compartment-bound resources (empty compartment list = allow)
- Criminal-penalty flag only valid for Restricted or CriminalPenalty classification levels
- Initial members are required at creation time (no empty compartments)

### Files Changed
- `crates/prism-core/src/types/entities.rs` -- added Compartment + CompartmentMembership
- `crates/prism-core/src/types/enums.rs` -- added ClassificationLevel + AccessDecision
- `crates/prism-core/src/types/requests.rs` -- added 6 request/result types for SR_GOV_31-33
- `crates/prism-core/src/repository.rs` -- added CompartmentRepository trait
- `crates/prism-compliance/src/compartment.rs` -- CompartmentService + 15 tests
- `crates/prism-compliance/Cargo.toml` -- added prism-audit, async-trait, dev-deps
- `migrations/009_create_compartments.sql` -- new migration

### Test Summary
- 15 new tests in prism-compliance
- 71 total workspace tests, all passing
- All quality gates green: fmt, clippy, test, check

---

## Session 2026-04-13 -- Day 6 (Week 2): SR_GOV_50 audit export + SR_GOV_51 tamper response

### Implemented
- AuditExportService (SR_GOV_50): signed audit export with chain proof for regulatory review
  - ExportSigner trait for pluggable signing (HMAC, RSA, HSM)
  - ExportFormat enum: JsonLines, Csv, Pdf
  - Chain integrity verification before export (rejects tampered segments)
  - ChainProof struct: anchor_hash, tip_hash, segment_length, position_range
  - Time-range filtering via existing AuditEventRepository.query()
  - 6 unit tests: JSON lines export, CSV format, empty range rejection, tampered chain rejection, chain proof boundaries, signature determinism
- TamperResponseService (SR_GOV_51): incident response when chain verification detects tampering
  - TenantWriteFreeze trait: freeze/is_frozen for tenant governance writes
  - AlertDispatcher trait: dispatch_critical for platform security officer alerts
  - IncidentTracker trait: create_incident for security investigation tickets
  - Three-step workflow: freeze writes -> send CRITICAL alert -> open incident
  - Idempotent freeze (re-triggering same tenant does not double-freeze)
  - 6 unit tests: freeze activation, alert dispatch, incident creation, idempotent freeze, tenant isolation, mismatch details
- SR_GOV_48 -> SR_GOV_51 wiring: verify_and_respond() on AuditLogger
  - Composes chain verification with tamper response in a single call
  - 2 integration tests: triggers on tamper, skips response when valid
- ExportFormat enum and request/result types added to prism-core

### Design Decisions
- ExportSigner is a trait for testability; real impl will use Vault-backed HMAC or RSA
- TenantWriteFreeze is separate from audit writes -- audit chain must remain writable to record the freeze event itself
- TamperResponseService takes three trait objects (freeze, alerter, incidents) -- each can be replaced independently
- Recovery is intentionally manual per spec; no automated chain repair
- verify_and_respond() is the composed path (SR_GOV_48 -> SR_GOV_51); verify_chain() remains available standalone
- PDF export produces canonical JSON source (rendering is a view-layer concern)

### Files Changed
- `crates/prism-core/src/types/enums.rs` -- added ExportFormat enum
- `crates/prism-core/src/types/requests.rs` -- added AuditExportRequest, AuditExportResult, ChainProof, TimeRange, TamperResponseInput, TamperResponseResult
- `crates/prism-audit/src/audit_export.rs` -- new file, ExportSigner trait + AuditExportService + 6 tests
- `crates/prism-audit/src/tamper_response.rs` -- new file, 3 traits + TamperResponseService + 6 tests
- `crates/prism-audit/src/event_store.rs` -- added verify_and_respond() + 2 integration tests
- `crates/prism-audit/src/lib.rs` -- registered audit_export + tamper_response modules

### Test Summary
- 14 new tests in prism-audit (6 export + 6 tamper + 2 wiring)
- 56 total workspace tests, all passing
- All quality gates green: fmt, clippy, test, check

---

## Session 2026-04-13 -- Day 5: SyncCoordinator stub + quality gates

### Implemented
- SyncCoordinator (REUSABLE, SR_DM_22): trait + InMemorySyncCoordinator for PG/Neo4j eventual consistency tracking
- SyncState enum: Consistent, PgOnly, Neo4jOnly, Divergent, Compensating
- SyncRecord struct with dual-store timestamps and last-checked tracking
- 5 unit tests covering record creation, state transitions, pending list filtering
- Added `cargo fmt --check` to build verification pipeline
- Cross-verification agent confirmed all "Done" items have SR refs and real code

### Design Decisions
- SyncCoordinator is a trait for testability; InMemorySyncCoordinator for dev, PG-backed impl later
- PgOnly is the default state for MVP (all writes are PG-only until Neo4j integration)
- list_pending() enables future backfill worker to detect and sync missing graph nodes
- Compensating state supports SR_DM_01_SE-01 partial-failure rollback (future)

### Files Changed
- `crates/prism-graph/src/sync_coordinator.rs` -- new file, trait + impl + 5 tests
- `crates/prism-graph/src/lib.rs` -- added sync_coordinator module
- `crates/prism-graph/Cargo.toml` -- added async-trait, dev-deps

---

## Session 2026-04-13 -- Day 4: Identity + tenant isolation + event bus

### Implemented
- IdentityService (SR_GOV_10, FOUND S 1.3.1): user provisioning with email validation/normalization/dedup, service principal provisioning with kill switch, tenant isolation enforcement on deactivation
- PgUserRepository (SR_DM_02): create, get_by_id, get_by_email with constraint-based dedup
- PgServicePrincipalRepository (SR_DM_20): create, get_by_id, list_by_tenant, deactivate
- TenantContext/TenantFilter (SR_DM_27): programmatic tenant isolation at service layer, enforce() guard
- EventBusPublisher (REUSABLE): trait + InMemoryEventBus + NoOpEventBus implementations
- 16 new unit tests (10 identity, 3 tenant filter, 3 event bus)

### Design Decisions
- IdentityService owns both UserRepository and ServicePrincipalRepository (identity is one domain)
- Email normalization: trim + lowercase before persistence and dedup check
- SP kill switch verifies tenant ownership before deactivation (cross-tenant forbidden)
- EventBusPublisher is a trait -- Redis Streams impl deferred until infra is wired
- TenantContext is a simple value object; full query-rewrite RLS deferred to Week 2+

### Files Changed
- `crates/prism-core/src/tenant_filter.rs` -- new file, TenantContext + 3 tests
- `crates/prism-core/src/lib.rs` -- added tenant_filter module
- `crates/prism-identity/src/service_principal.rs` -- full IdentityService + 10 tests
- `crates/prism-identity/src/pg_repository.rs` -- new file, PG repos for User + SP
- `crates/prism-identity/src/lib.rs` -- added pg_repository module
- `crates/prism-identity/Cargo.toml` -- added prism-audit, sqlx, async-trait deps
- `crates/prism-runtime/src/event_bus.rs` -- new file, trait + 2 impls + 3 tests
- `crates/prism-runtime/src/lib.rs` -- added event_bus module
- `crates/prism-runtime/Cargo.toml` -- added async-trait dep

---

## Session 2026-04-13 -- Day 3: Tenant model + lifecycle state machine

### Implemented
- Lifecycle state machine (FOUND S 1.5.1, SR_DM_11): 10 Track-A states, deterministic transitions, `validate_transition()`, `allowed_transitions()`, credential status helpers
- Added `Rejected` variant to `LifecycleState` enum (was missing from Day 1 scaffold)
- TenantService (SR_GOV_01): onboard with validation (empty name, empty profiles, nonexistent parent), duplicate detection, audit trail integration
- PgTenantRepository (SR_DM_01 PG path): create, get_by_id, update, list_by_parent, constraint-based duplicate detection
- 22 unit tests: 12 for state machine, 10 for TenantService (mock repos)

### Design Decisions
- State machine is pure logic (no I/O) -- all transitions validated before any persistence
- `Rejected` state allows return to `Draft` for revision (spec supports this path)
- TenantService composes TenantRepository + AuditLogger per the proposed pattern from PATTERNS.md
- PG constraint violations mapped to `PrismError::Conflict` (BE-01)
- Neo4j dual-write deferred to Week 2 (needs SyncCoordinator)

### Files Changed
- `crates/prism-lifecycle/src/state_machine.rs` -- full implementation + 12 tests
- `crates/prism-governance/src/tenant.rs` -- new file, TenantService + 10 tests
- `crates/prism-governance/src/pg_tenant_repo.rs` -- new file, PG repository
- `crates/prism-governance/src/lib.rs` -- added tenant + pg_tenant_repo modules
- `crates/prism-governance/Cargo.toml` -- added prism-audit, sqlx, async-trait deps
- `crates/prism-core/src/types/enums.rs` -- added Rejected variant to LifecycleState

---

## Session 2026-04-13 -- Day 2: Merkle chain hasher + audit event store

### Implemented
- `MerkleChainHasher` (REUSABLE, D-22): SHA-256 hash chain with GENESIS salt, canonical byte serialization, chain verification
- `AuditLogger` (REUSABLE, SR_GOV_47/48/49): service composing repository + hasher, log/verify/query methods
- `PgAuditEventRepository` (SR_DM_05): PostgreSQL append/get_chain_head/query/get_chain_segment
- Updated `008_create_audit_events.sql`: added `chain_position`, `severity`, `source_layer` columns + unique chain index
- 14 unit tests: 7 for MerkleChainHasher, 7 for AuditLogger (mock repo)
- All clippy-clean, full workspace compiles

### Design Decisions
- Chain is per-tenant: each tenant has independent hash sequence anchored at position 0
- Genesis events hash against literal `"GENESIS"` salt instead of empty/null
- Canonical serialization uses NUL-separated fixed-order fields for determinism
- `AuditEventRow` intermediate type handles TEXT-to-enum mapping from Postgres
- Enum serialization uses serde_json round-trip to match `#[serde(rename_all = "snake_case")]`

### Files Changed
- `crates/prism-audit/src/merkle_chain.rs` -- full implementation + 7 tests
- `crates/prism-audit/src/event_store.rs` -- AuditLogger + mock repo + 7 tests
- `crates/prism-audit/src/pg_repository.rs` -- new file, PG implementation
- `crates/prism-audit/src/lib.rs` -- added pg_repository module
- `crates/prism-audit/Cargo.toml` -- added sqlx, async-trait, dev-deps
- `migrations/008_create_audit_events.sql` -- added missing columns + indexes

---

## Session 2026-04-13 -- Scaffold + Week 1 Planning

### Implemented
- Project scaffold: 12 Rust crates, workspace Cargo.toml, all compiling
- prism-core domain types: 9 ID types (UUIDv7), 13 enums, PrismError, 5 traits
- 8 PostgreSQL migrations (tenants, roles, users, compliance_profiles, service_principals, automations, approval_chains, audit_events)
- Docker Compose: PostgreSQL 16 (5433), Neo4j 5 (7474/7687), Redis 7 (6380), Vault 1.15 (8200)
- CI pipeline (.github/workflows/ci.yml)
- Git repo initialized, pushed to github.com/compiles-first-time/Prism (main + develop)
- Specs and validation workbook copied as read-only reference

### Blocked / Deferred
- Port conflict with rsf-* containers required remapping PG to 5433, Redis to 6380
- sudo password needed for build-essential install (no cc linker in WSL2 by default)

### Learnings
- WSL2 Ubuntu does not ship with build-essential; must install before cargo can compile anything
- Deploy keys on GitHub are repo-scoped; needed account-level SSH key for multi-repo push
- Cargo jobs=0 is invalid (unlike make -j0); removed from .cargo/config.toml

### Process Notes
- Scaffolding approach worked well: create structure first, verify compilation, then commit
- Parallel agent dispatch for stub file creation saved significant time
- Copying specs into the workspace (read-only) gives the implementation context without polluting the architecture repo

### Compiler/Clippy Issues
- Unused import warning on AuditEventId in traits.rs (fixed)
- cargo fmt disagreed on struct variant formatting (NotFound inline vs multi-line) and module ordering (alpha sort)
