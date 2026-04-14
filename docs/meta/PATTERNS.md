# Implementation Patterns

Reusable patterns discovered during PRISM implementation. Updated as we learn.

---

## Pattern: Repository + Service + AuditLogger

*Status: Proposed (Day 1). Will validate during Days 2-4.*

Every domain entity follows:
1. Define struct in `prism-core/src/types/entities.rs`
2. Define async repository trait in `prism-core/src/repository.rs`
3. Implement PG repository in the domain crate (`pg_*.rs`)
4. Create service struct that composes repository + AuditLogger
5. Write unit tests for pure logic (mock repository)
6. Write integration tests against real PG

### Why This Pattern
- Repository trait enables testing without a database
- Service composition keeps audit trail wiring consistent
- Domain types in prism-core prevent circular dependencies

### Open Questions
- Should PG repository files live in the domain crate or in a shared `prism-persistence` crate?
- Is `async-trait` acceptable overhead or should we use manual `impl Future`?

---

## Pattern: SR Reference in Comments

Every function, struct, or impl block that implements business logic carries an SR reference.

```rust
/// Implements: SR_GOV_47 audit event write path
pub async fn log(&self, input: AuditEventInput) -> Result<AuditCaptureResult, PrismError> {
```

### Why
- Traceability chain: code -> SR -> BR -> scenario -> gap -> evidence
- Enables automated compliance verification (future pre-commit hook)
