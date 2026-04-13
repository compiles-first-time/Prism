# CONVENTIONS — How Work Is Done in This Exploration

**Read this before starting any work. These conventions are mandatory.**
**Last updated:** 2026-04-08

---

## Identifier Naming Conventions

### Decisions
Format: `D-{number}`
- `D-1` through `D-77` (current range, grows with new decisions)
- Referenced in STATE.md, LOG.md, spec files
- Example: "Per D-34, CSA runs before every combined-data analysis"

### Use Cases
Format: `UC-{number}`
- `UC-01` through `UC-180+` (current range)
- Referenced in STATE.md, spec files
- Example: "UC-111 tests CSA mosaic effect prevention"

### Back-Propagation Fixes
Format: `BP-{number}`
- `BP-01` through `BP-93` (current range)
- Referenced in spec files where fix was applied
- Example: "Per BP-12, every layer emits Alert nodes through the polymorphic ALERTED_ABOUT edge"

### Unknown Unknowns
Format: `UU-{number}`
- `UU-1` through `UU-13` (stable)
- Plus layer-specific discoveries: `IL-1` through `IL-10`, `LR-1` through `LR-9`, `DS-1` through `DS-12`, `INT-1` through `INT-15`, `CC-1` through `CC-14`
- Example: "UU-12 is resolved by user upload (Type 7) and log ingestion (Type 8)"

### Spec Requirements (NEW — for expanded specs)
Format: `SR_{LAYER}_{number}_{qualifier}`

Base requirement:
- `SR_GOV_01` = Spec Requirement, Governance layer, #1
- `SR_CONN_01` = Spec Requirement, Connection layer, #1
- `SR_INT_01` = Spec Requirement, Intelligence layer, #1
- `SR_LLM_01` = Spec Requirement, LLM Routing layer, #1
- `SR_DS_01` = Spec Requirement, Decision Support, #1
- `SR_UI_01` = Spec Requirement, Interface layer, #1
- `SR_CAT_01` = Spec Requirement, Component Catalog, #1
- `SR_SA_01` = Spec Requirement, Service Account Catalog, #1
- `SR_DM_01` = Spec Requirement, Data Model, #1
- `SR_SCALE_01` = Spec Requirement, Scalability Infrastructure, #1

Component within requirement:
- `SR_CONN_01_OAuth2Handler` = OAuth2 handler component within SR_CONN_01
- `SR_CONN_01_ConnectionRequestValidator` = Validator component

Exception handlers:
- `SR_CONN_01_OAuth2Handler_SE-01` = System Exception #1 for OAuth2Handler
- `SR_CONN_01_OAuth2Handler_BE-01` = Business Exception #1 for OAuth2Handler
- `SR_CONN_01_OAuth2Handler_SE-02` = Second SE case
- `SR_CONN_01_OAuth2Handler_BE-02` = Second BE case

Reusable components:
- `REUSABLE_CaaSCredentialRetriever` = Shared across specs
- `REUSABLE_AuditLogger` = Shared across specs
- `REUSABLE_TenantFilter` = Shared across specs

### Type Column Values

- `---` = Main flow (happy path)
- `SE` = System Exception (infrastructure failure: network, DB, API, auth)
- `BE` = Business Exception (logic/validation failure: bad input, rule violation, policy violation)

---

## SR-Row Format (Mandatory for Expanded Specs)

Every row in an expanded spec has EXACTLY these 12 columns:

| Column | Required? | Description |
|--------|-----------|-------------|
| **ID** | Yes | Per naming convention above |
| **Type** | Yes | `---`, `SE`, or `BE` |
| **Layer** | Yes | governance / connection / intelligence / llm / decision / interface / component / sa / data-model / scalability |
| **Usecase** | Yes | Full sentence describing what happens. Present tense. Verbose. Explicit. |
| **Assets/Cred/Other** | Yes (or `None`) | Resources needed: assets, credentials, config, database, etc. |
| **Input Source or Condition** | Yes | Where input comes from OR when this fires (for exception rows) |
| **Expected Input** | Yes | Structured description of what arrives |
| **Expected Output** | Yes | Structured description of what leaves |
| **Input Data Format** | Yes | Type specification (e.g., `ConnectionRequest`, `String`, `JSON Schema ref`) |
| **Output Data Format** | Yes | Type specification |
| **Next Step** | Yes | Flow control: next SR ID, state transition, or `End` |
| **Why** | Yes | Justification — why does this exist, what does it prevent, what does it enable |

**NO NULLS.** Every cell is filled. Use `N/A` only if truly not applicable, and explain in "Why" column.

---

## Exception Coverage Rules

For every main-flow SR row (`---` type), you must enumerate:

### System Exceptions (at minimum)
- `SE-01`: Infrastructure failure (network, DB, service unavailable)
- `SE-02`: Authentication/credential failure
- `SE-03`: Timeout
- `SE-04`: Resource exhaustion (quota exceeded, memory, GPU)

Only include SE rows that are actually possible for this step. Do not pad.

### Business Exceptions (as applicable)
- `BE-01`: Input validation failure
- `BE-02`: Policy/rule violation
- `BE-03`: Permission denied
- `BE-04`: Business state invalid (e.g., trying to approve an already-rejected item)

Only include BE rows that are actually possible for this step. Do not pad.

### Required Responses
Every exception row must include:
- How is it detected?
- What is the recovery action?
- Who is notified?
- Is it retryable?
- What is the max retry count?
- What happens on retry exhaustion?

These go in the `Next Step` and `Why` columns.

---

## Cross-Reference Format

When referencing other parts of the architecture:

| Reference Type | Format | Example |
|----------------|--------|---------|
| Decision | `D-{number}` | `D-34` |
| Use case | `UC-{number}` | `UC-111` |
| Back-propagation fix | `BP-{number}` | `BP-12` |
| Unknown unknown | `UU-{number}` | `UU-12` |
| Spec file | `Spec {number}` | `Spec 03` or `Spec 03-connection-layer.md` |
| Spec section | `Spec {number} S {section}` | `Spec 01 S 2.1` |
| Spec requirement | `{SR ID}` | `SR_CONN_01` |
| Trunk gap | `GAP-{number}` | `GAP-14` |
| Trunk scenario | `Scenario {number}` | `Scenario 88` |
| Trunk section | `[{ID} § {section}]` | `[FOUND § 1.4.1]` |

---

## File Naming Conventions

| File Type | Convention | Example |
|-----------|-----------|---------|
| Spec (current) | `{NN}-{name}.md` | `01-governance-layer.md` |
| Expanded spec | `{NN}-{name}-expanded.md` | `01-governance-layer-expanded.md` |
| Exploration directory | `{NNN}-{category}-{slug}` | `002-spec-expansion` |
| Skill | `{verb-noun}.md` | `verify-against-spec.md` |
| Agent | `{noun-agent}.md` | `spec-gap-detector.md` |

---

## Decision Logging Format

Every new decision added to STATE.md follows this format:

```
| D-{N} | {one-sentence description} | {HIGH/MEDIUM/LOW confidence} | {session number} | {rationale with source references} |
```

Rules:
- Description is a complete sentence, not a fragment
- Confidence must be justified
- Rationale references specific files, gaps, or prior decisions
- If the decision is based on evidence, include evidence grade (PROVEN/HIGH-PROB/EMERGING)

---

## Back-Propagation Rules

When a new decision or spec change is made:

1. **Scan all prior specs** for impact
2. **Create a BP entry** for every gap the change creates
3. **Update affected specs** with the fix
4. **Document in LOG.md** which specs were affected and why
5. **Renumber nothing** — BP numbers are append-only

Example: If D-34 (CSA) is added, it creates BP-43 (data model needs csa_rules table), BP-45 (governance needs CSA enforcement), and BP-47 (decision support must invoke CSA). All three BPs must be created before the new decision is considered complete.

---

## Documentation Format

### For Specs
- Markdown with ASCII box-drawing for inline diagrams
- Tables for comparisons and catalogs
- Short paragraphs (3-5 sentences max)
- Every section starts with a one-sentence purpose statement
- Gap/decision/SR IDs always hyperlinked or referenced with type

### For Decisions
- Full sentences
- Active voice
- "Must" for requirements, "should" for recommendations, "may" for optional
- Evidence grades where applicable

### Language Rules
- No emojis
- No casual language
- Consulting-grade but accessible
- "The platform" not "our platform" or "the system"
- Active voice preferred
- Verbose but explicit — say more, not less

---

## Commit Message Format (for when we reach implementation)

```
{type}: {short description}

{longer description if needed}

Implements: {SR IDs or Decision IDs}
Tests: {Test IDs or UC IDs}
Spec: {spec file section}
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `spec`

Example:
```
feat: implement CSA rule engine

Adds PostgreSQL csa_rules table and rule evaluation logic.
Runs before Decision Support combines data from multiple collections.

Implements: SR_GOV_34, D-34, BP-43
Tests: UC-111, UC-112
Spec: 01-governance-layer-expanded.md section 3.2.1
```

---

## Session Protocol

### Starting a Session
1. Read HANDOFF.md first
2. Read files in mandatory reading order (see HANDOFF.md)
3. Confirm understanding to user
4. Wait for explicit direction before proceeding

### During a Session
1. Reference files for all claims
2. Log decisions immediately in STATE.md
3. Log narrative in LOG.md
4. Run self-audits before delivering work
5. Update NEXT.md if the next action changes

### Ending a Session
1. Update STATE.md (decisions, status, open questions)
2. Update LOG.md (session narrative, Nick's thinking)
3. Update NEXT.md (precise next action)
4. Ask Nick: "Before we close, is there anything about your thinking I should capture?"
5. Save all files
6. Confirm to Nick: "Session state saved. Next session should start by reading HANDOFF.md."

---

## What Is NOT Negotiable

- File naming conventions
- Identifier formats (D-, UC-, BP-, UU-, SR-)
- Exception coverage rules (every main flow has SE and BE)
- No-nulls rule in SR rows
- Cross-reference format
- No emojis
- No fabrication
- Mandatory reading order at session start
- Mandatory state update at session end

If you think a convention is wrong, STOP and ask Nick to change it. Do not silently diverge.
