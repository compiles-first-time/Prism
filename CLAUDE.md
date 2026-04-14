# PRISM -- Claude Code Instructions

## Project

PRISM is a governance control plane for enterprise automation in regulated financial services. It registers, approves, tracks, and audits automations -- it does not execute them.

- **Code:** `~/projects/prism` (this directory)
- **Architecture specs (read-only):** `/mnt/d/Projects/IDEA/build/spec/`
- **Implementation tracker:** `docs/meta/IMPLEMENTATION-STATUS.md`
- **Build log:** `docs/meta/BUILD-LOG.md`
- **Patterns:** `docs/meta/PATTERNS.md`

## Cardinal Rules

1. **Read the spec before writing code.** Every SR being implemented must be read from the expanded spec files at `/mnt/d/Projects/IDEA/build/spec/` BEFORE implementation begins. Do not implement from memory or assumptions.
2. **Every function implementing business logic must reference its SR ID** in a doc comment (e.g., `/// Implements: SR_GOV_47`).
3. **Track A only.** Track B / Kernel V6 has schema placeholders only -- do not implement Track B logic.
4. **Append-only audit trail.** Never issue UPDATE or DELETE on the `audit_events` table.
5. **Specs are read-only.** Never modify files under `/mnt/d/Projects/IDEA/`.

## Quality Gates (mandatory before every commit)

Run ALL of these and fix any failures before committing:

```bash
cargo fmt --check        # formatting
cargo clippy -- -D warnings   # lint (zero warnings)
cargo test --workspace   # full test suite
cargo check --workspace  # cross-crate type safety
```

If any gate fails, fix the issue and re-run. Do not commit with failures.

## Implementation Workflow

For each SR or batch of SRs:

1. **Read** the expanded spec for the SR from `/mnt/d/Projects/IDEA/build/spec/`
2. **Plan** the implementation (types, traits, files)
3. **Implement** with SR references in doc comments
4. **Test** -- write unit tests with mock repositories (no live DB required)
5. **Quality gates** -- run all four checks above
6. **Update status** -- mark items Done in `docs/meta/IMPLEMENTATION-STATUS.md`
7. **Update build log** -- add session entry to `docs/meta/BUILD-LOG.md`

## Established Patterns

- **Repository + Service + AuditLogger:** Domain entity -> trait in prism-core -> PG impl in domain crate -> Service composing repo + AuditLogger
- **Enum serialization:** Use `serde_json::to_value()` / `from_value()` for TEXT column round-trips (matches `#[serde(rename_all = "snake_case")]`)
- **Mock repos:** `Mutex<Vec<T>>` behind `#[async_trait]` impl for unit tests
- **ID types:** UUIDv7 via `define_id!` macro in `prism-core/src/types/identifiers.rs`
- **Tenant isolation:** `TenantContext` threaded through service calls

## Infrastructure

| Service | Host Port | Container Port |
|---------|-----------|----------------|
| PostgreSQL | 5433 | 5432 |
| Redis | 6380 | 6379 |
| Neo4j HTTP | 7474 | 7474 |
| Neo4j Bolt | 7687 | 7687 |
| Vault | 8200 | 8200 |

## Cross-Verification

After completing a day's work, run a background agent to verify:
- Every "Done" item in IMPLEMENTATION-STATUS.md has its SR ID referenced in the claimed source file
- Every claimed source file has real implementation code (not just a doc comment stub)
- Test counts in the status table match actual `#[test]` / `#[tokio::test]` counts
