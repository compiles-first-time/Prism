# Build Log

Reverse-chronological record of implementation sessions.

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
