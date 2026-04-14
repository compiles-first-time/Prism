# Build Log

Reverse-chronological record of implementation sessions.

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
