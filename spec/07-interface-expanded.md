# Spec 07 Expanded: Interface

**Source:** `001/spec/07-interface.md`
**Exploration:** `002-spec-expansion`
**Status:** draft — pending self-audit and back-propagation passes
**Priority:** 4 (user touchpoint)
**Reserved SR range:** `SR_UI_01` through `SR_UI_99`
**Last updated:** 2026-04-10

---

## Purpose

Implementation-readiness for the React + Next.js + TypeScript + Tailwind + shadcn/ui interface: 12 panels (7 user + 5 admin), Claude touchpoints, real-time streaming via Socket.io, authentication (IAM and platform-managed), responsive/mobile/PWA, accessibility (WCAG 2.1 AA), internationalization, onboarding, security (httpOnly cookies, CSRF, CSP, XSS), offline mode, performance (virtual scrolling, skeleton screens), multi-tab/multi-window, feature flags, admin undo, notification center.

## Architectural Decisions Covered

D-13, D-26, D-40, D-57, D-66, D-67, D-68, D-69, D-70, plus INT-1 through INT-15.

## Trunk Inheritance

| Capability | Trunk Reference | Invoked by SRs |
|-----------|----------------|----------------|
| WCAG 2.1 AA accessibility | Industry standard | `SR_UI_45` |

## Integration Map

| Consumer | Depends On |
|----------|------------|
| Spec 01 Governance | `SR_UI_05` (UI visibility check), `SR_UI_30` (audit panel), `SR_UI_35` (admin undo) |
| Spec 02 Data Model | `SR_UI_25` (notification center), `SR_UI_40` (preferences) |
| Spec 03 Connection | `SR_UI_15` (connections panel) |
| Spec 04 Intelligence | `SR_UI_10` (graph viz), `SR_UI_22` (semantic search) |
| Spec 05 LLM Routing | `SR_UI_20` (chat streaming) |
| Spec 06 Decision Support | `SR_UI_08` (recommendations), `SR_UI_27` (response capture) |

## Reusable Components

| Component | Purpose |
|-----------|---------|
| `REUSABLE_VirtualScroller` | Virtual scrolling for any list >500 items |
| `REUSABLE_SkeletonLoader` | Skeleton placeholders during data fetch |
| `REUSABLE_StreamHandler` | WebSocket subscription per panel |
| `REUSABLE_TenantAwareRouter` | URL routing with tenant context preservation |

---

## Section 1 — Authentication and Session (SR_UI_01 through SR_UI_06)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_UI_01` | --- | interface | OAuth redirect login flow for IAM-synced tenants: redirect to IAM provider, return with token, exchange for platform JWT. | NextAuth.js, IAM provider | User clicks login | `LoginInput { tenant_hint?, return_url }` | Authenticated session with JWT in httpOnly cookie | JSON | `LoginResult` | `SR_UI_06` (post-login routing) | Standard SSO pattern. |
| `SR_UI_02` | --- | interface | Magic-link authentication for platform-managed tenants per `SR_GOV_08`: user enters email → email with one-time link → click → session. | Email provider, NextAuth.js | User submits email | `MagicLinkInput { email }` | Magic-link email sent; on click, session established | JSON | `Result` | `SR_UI_06` | Passwordless option for SMBs. |
| `SR_UI_03` | --- | interface | Password + MFA login for platform-managed tenants: password verification → MFA challenge → session. | NextAuth.js, MFA provider | User submits credentials | `PasswordLoginInput { email, password, mfa_code }` | Session established | JSON | `Result` | `SR_UI_06` | Standard authenticated login. |
| `SR_UI_03_BE-01` | BE | interface | MFA verification fails. | MFA verifier | Verification check | Same | Login refused | Same | `AuthError { reason: "mfa_failed" }` | User retries. | MFA is mandatory for admins. |
| `SR_UI_04` | --- | interface | Session token refresh: refresh JWT (1h) using refresh token (7d, rotation on use); both stored in httpOnly/Secure/SameSite cookies per D-68. | NextAuth.js refresh handler | Before access token expires | `RefreshInput` | New JWT issued | JSON | `Result` | End | Refresh rotation prevents stolen-token replay. |
| `SR_UI_05` | --- | interface | UI visibility check via `SR_GOV_75`: every panel, widget, action button asks governance whether the current user can see/use it. | `SR_GOV_75` | Inline at render | `VisibilityCheckInput { ui_element_id, principal }` | VISIBLE / HIDDEN / READ_ONLY | JSON | `Result` | Render accordingly. | Front-end suppression at render time, with back-end as authoritative defense in depth. |
| `SR_UI_06` | --- | interface | Post-login routing: send user to onboarding flow (first time), resumed location (returning), or default dashboard. | Router, user state | After login | `RoutingInput { user_state }` | Target route | JSON | `Result` | End | Smooth onboarding and resume UX. |

## Section 2 — User Panels (SR_UI_07 through SR_UI_18)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_UI_07` | --- | interface | Render Conversational (Chat) panel: Claude-powered streaming, artifact pattern (chat left, charts/tables right). | LLM Router, `REUSABLE_StreamHandler`, `SR_LLM_23` | Panel mount | `ChatInit` | Chat panel rendered with empty thread | JSON | `Result` | User interaction streams via `SR_UI_20`. | Primary conversational surface. |
| `SR_UI_08` | --- | interface | Render Recommendations panel: list active recommendations with sort/filter, response actions (Accept/Reject/Defer/Modify/Escalate), confidence breakdowns. | `SR_DS_22` delivery feed, `REUSABLE_VirtualScroller` | Panel mount | `RecsInit` | Recommendations list rendered | JSON | `Result` | Response actions invoke `SR_UI_27`. | Primary action surface. |
| `SR_UI_09` | --- | interface | Render Dashboard panel: customizable KPI cards, real-time updates via WebSocket. | `REUSABLE_StreamHandler` (kpi room) | Panel mount | `DashboardInit` | KPI cards rendered with live data | JSON | `Result` | KPI updates flow via `SR_UI_28`. | Customizable dashboards are core SMB value. |
| `SR_UI_10` | --- | interface | Render Process Map panel: Neo4j graph visualization via React Flow / Cytoscape; interactive (zoom, pan, click for detail). | `SR_INT_20`, React Flow | Panel mount | `MapInit` | Graph rendered | JSON | `Result` | Node clicks invoke `SR_UI_22`. | Visualizes the intelligence graph for users. |
| `SR_UI_11` | --- | interface | Render Catalog panel: browse data collections, components, connections; semantic search; virtual scrolling. | `REUSABLE_VirtualScroller`, search via `SR_INT_15` | Panel mount | `CatalogInit` | Catalog rendered | JSON | `Result` | Search invokes `SR_UI_22`. | Discovery surface. |
| `SR_UI_12` | --- | interface | Render Connections panel: list connections with health KPIs, manage actions (suspend, decommission). | `SR_CONN_44` | Panel mount | `ConnInit` | Connections list rendered | JSON | `Result` | Actions invoke `SR_CONN_` lifecycle SRs. | Connection visibility for users with that role. |
| `SR_UI_13` | --- | interface | Render Uploads panel: user upload (Type 7) workflow per `SR_CONN_18`; file picker + purpose declaration. | File upload handler, `SR_CONN_18` | Panel mount | `UploadsInit` | Upload form rendered | JSON | `Result` | Submit invokes `SR_CONN_18`. | Captures tribal knowledge data. |

## Section 3 — Admin Panels (SR_UI_14 through SR_UI_20)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_UI_14` | --- | interface | Render Governance admin panel: roles, compartments, classifications, policies, CSA rules. | `SR_GOV_*` admin APIs | Panel mount | `GovInit` | Governance admin rendered | JSON | `Result` | Actions invoke specific `SR_GOV_` rows. | Admin access to all governance functions. |
| `SR_UI_15` | --- | interface | Render User Management admin panel: list users (standalone or IAM view); create/invite (standalone), view (IAM). | `SR_GOV_07`, `SR_GOV_03` | Panel mount | `UmInit` | User list rendered | JSON | `Result` | Invite invokes `SR_GOV_07`. | Tenant admin user management. |
| `SR_UI_16` | --- | interface | Render Model Management admin panel: model registry, performance, swap workflow. | `SR_LLM_40`, `SR_LLM_41` | Panel mount | `ModelInit` | Model registry rendered | JSON | `Result` | Swap invokes `SR_GOV_66` + `SR_LLM_41`. | Privileged surface for model governance. |
| `SR_UI_17` | --- | interface | Render Audit Trail admin panel: full audit with search/filter/export per `SR_GOV_49` and `SR_GOV_50`. | `SR_GOV_49` | Panel mount | `AuditInit` | Audit panel rendered | JSON | `Result` | Search/export invoke governance SRs. | Compliance and forensic surface. |
| `SR_UI_18` | --- | interface | Render Settings panel: user preferences + tenant configuration. | `SR_DM_26` | Panel mount | `SettingsInit` | Settings rendered | JSON | `Result` | Save invokes `SR_DM_26` write. | User and tenant preferences. |

## Section 4 — Streaming, Real-Time, Notifications (SR_UI_20 through SR_UI_30)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_UI_20` | --- | interface | Live streaming chat response per `SR_LLM_23`. | `REUSABLE_StreamHandler`, Socket.io | User submits chat message | `ChatStreamInput { message }` | Streamed response | JSON | `Result` | If verification fails: replace per `SR_LLM_23`. | Conversational UX requires streaming. |
| `SR_UI_21` | --- | interface | Verified streaming for recommendations per `SR_LLM_22`: show "analyzing..." until verification passes. | `SR_LLM_22` | Recommendation generation | `VerifiedStreamInput` | Verified response delivered | JSON | `Result` | End | High-stakes outputs require verification before display. |
| `SR_UI_22` | --- | interface | Semantic search across catalog and process map: query → vector search via `SR_INT_15` → results filtered by governance compartment check (`SR_GOV_33` invoked from inside `SR_INT_15`) → display. | `SR_INT_15` (which itself invokes `SR_GOV_33`) | User search action from `SR_UI_11` Catalog or `SR_UI_10` Process Map | `SearchInput { tenant_id, principal, query }` | Filtered results with provenance and dropped-for-compartment count | JSON | `SemanticSearchResult { results[], dropped_for_compartment_count }` | End | Search must respect compartments. Bidirectional reference to `SR_INT_15` completes BP-101 from the interface side. |
| `SR_UI_23` | --- | interface | Real-time KPI streaming per D-26: dashboard panel subscribes to per-tenant KPI room via Redis pub/sub + Socket.io; sticky sessions. | `REUSABLE_StreamHandler` | Dashboard mount | `KpiSubscribeInput` | Live updates pushed | JSON | `Result` | End | Real-time dashboards build trust and engagement. |
| `SR_UI_24` | --- | interface | Real-time graph change streaming: process map panel subscribes to graph updates. | `REUSABLE_StreamHandler` | Map mount | `GraphSubscribeInput` | Graph updates pushed | JSON | `Result` | End | Live graph evolution shows the intelligence layer growing. |
| `SR_UI_25` | --- | interface | Notification center bell icon: shows all notifications, read/unread, archived; persistent across sessions via `SR_DM_25` (which is the data-model write path). Replayed offline-mode notifications from `SR_UI_36` are merged in chronological order using their preserved `original_timestamp` metadata. | `SR_DM_25` for read/write | Continuous | `NotificationFetchInput { tenant_id, person_id, since? }` | Notification list with read/unread state | JSON | `Result { notifications[], unread_count }` | End | Server-side state ensures consistency across tabs/sessions. Bidirectional reference to `SR_DM_25` and explicit handling of replayed notifications complete BP-111 and BP-112. |
| `SR_UI_26` | --- | interface | Toast notifications for transient events (recommendation arrived, system alert) — separate from notification center. | Toast library | Event arrival | `ToastInput` | Toast displayed | JSON | `Result` | End | Real-time signals without cluttering the persistent notification center. |
| `SR_UI_27` | --- | interface | Recommendation response capture: user clicks Accept/Reject/Defer/Modify/Escalate; payload sent to `SR_DS_12`. | `SR_DS_12` | User action | `ResponseInput` | Response forwarded | JSON | `Result` | End | Closes the loop from UI to Decision Support. |
| `SR_UI_28` | --- | interface | Subscribe to multiple panels via a shared SharedWorker per INT-2 (where supported); falls back to per-tab WebSocket otherwise. | SharedWorker, BroadcastChannel | App mount | `SharedSocketInput` | Single WS shared across tabs | JSON | `Result` | End | Reduces WebSocket count per user (max 5 concurrent). |
| `SR_UI_29` | --- | interface | Auto-reconnect with exponential backoff on WS disconnect. Serves as the WebSocket fallback role in the `SR_SCALE_30` graceful degradation chain — when the WebSocket server is overloaded the platform downgrades to short-poll mode and informs the user that real-time updates are temporarily paused. | Socket.io client, `REUSABLE_DegradationChain` participation | On disconnect or degradation event | `ReconnectInput { reason }` | Reconnection attempted; on persistent failure switches to short-poll fallback | JSON | `Result { mode: realtime \| polling, retry_at }` | End | Network resilience. Graceful degradation participation as the WebSocket-fallback role completes BP-120. |
| `SR_UI_30` | --- | interface | Audit Trail export: invoke `SR_GOV_50` and offer download. | `SR_GOV_50` | User action in audit panel | `ExportInput` | Signed export bundle | JSON | `Result` | End | Compliance export workflow. |

## Section 5 — Accessibility, Offline, Performance, Security (SR_UI_31 through SR_UI_45)

| ID | Type | Layer | Usecase | Assets / Cred | Input Source / Condition | Expected Input | Expected Output | Input Format | Output Format | Next Step | Why |
|----|------|-------|---------|---------------|-------------------------|----------------|-----------------|--------------|--------------|----------|-----|
| `SR_UI_31` | --- | interface | Virtual scrolling for any list >500 items per D-70 / INT-6. | `REUSABLE_VirtualScroller` | List render | `ListRenderInput` | Only visible rows + buffer rendered | JSON | `Result` | End | Prevents browser crashes on large tenants. |
| `SR_UI_32` | --- | interface | Skeleton screen rendering during fetch per INT-7 (no spinners). | `REUSABLE_SkeletonLoader` | Data fetch start | `SkeletonInput` | Skeleton displayed | JSON | `Result` | End | Skeleton screens improve perceived performance. |
| `SR_UI_33` | --- | interface | React Suspense for data fetching with bounded loading states. | React Suspense | Component data dependency | `SuspenseInput` | Loading or rendered | JSON | `Result` | End | Modern React data-loading pattern. |
| `SR_UI_34` | --- | interface | Prefetch likely next views on hover/idle. | Prefetch hints | User idle or hover | `PrefetchInput` | Resources prefetched | JSON | `Result` | End | Speeds up navigation. |
| `SR_UI_35` | --- | interface | Admin undo per `SR_GOV_69` and BP-77: 10-minute window for reversible admin actions. | `SR_GOV_69` | Admin action | `UndoInput` | Reverted | JSON | `Result` | End | Recovers from human error. |
| `SR_UI_36` | --- | interface | Offline mode per D-69: last-loaded dashboards/recommendations cached in IndexedDB; read-only; queue actions for resume; clear offline indicator. On reconnect, queued actions are replayed and each generates an audit event via `SR_GOV_47` with the `original_timestamp` metadata preserved (so the audit chain reflects when the user actually performed the action). | IndexedDB, `SR_GOV_47` for replayed audit events | On disconnect / on reconnect | `OfflineInput { tenant_id, person_id, action_queue }` | Cached UI shown; on reconnect, replayed actions audited with original timestamps | JSON | `Result { actions_replayed, original_timestamps_preserved }` | On reconnect, queued actions replayed via their original SR paths (each preserves `original_timestamp`). | Graceful degradation. The explicit timestamp-preservation contract completes BP-112. |
| `SR_UI_37` | --- | interface | Multi-tab BroadcastChannel sync per INT-2. | BroadcastChannel | App mount | `BroadcastInput` | State synced across tabs | JSON | `Result` | End | Consistent state across tabs. |
| `SR_UI_38` | --- | interface | Frontend security per D-68 / INT-9 / INT-10: JWT in httpOnly/Secure/SameSite cookies; CSRF tokens; CSP headers; XSS sanitization; device fingerprinting; refresh token rotation. | Security middleware | All requests | Various | Hardened request | JSON | `Result` | End | Frontend is a primary attack surface. |
| `SR_UI_39` | --- | interface | XSS sanitization for any rendered content from external systems or user input. | DOMPurify or similar | Inline | `SanitizeInput` | Sanitized content | JSON | `Result` | End | No `dangerouslySetInnerHTML`; Trusted Types where supported. |
| `SR_UI_40` | --- | interface | Per-user preferences persistence via `SR_DM_26`. | `SR_DM_26` | User saves settings | `PrefsInput` | Saved | JSON | `Result` | End | Recipient-centric delivery requires preferences. |
| `SR_UI_41` | --- | interface | Notification preferences flow: user picks delivery mode (real-time/email/digest/weekly), urgency filter, categories, quiet hours, format. | `SR_DM_26` | Settings panel | `NotifPrefsInput` | Saved | JSON | `Result` | End | Per D-61. |
| `SR_UI_42` | --- | interface | Internationalization: English (P1), Spanish (P2), externalized strings (JSON), locale-aware formatting; LLM responds in user's preferred language. | i18n library | Locale change | `LocaleInput` | Locale applied | JSON | `Result` | End | Supports the SMB market. |
| `SR_UI_43` | --- | interface | Onboarding flow: welcome → role confirm → dashboard preset → first query prompt → preferences → guided tour (optional) → ready. | Onboarding wizard | First login | `OnboardingInput` | Step-by-step | JSON | `Result` | End at ready. | Smooth onboarding is critical for retention. |
| `SR_UI_44` | --- | interface | Real User Monitoring (RUM) for frontend performance per INT-11. | RUM library | All sessions | `RumInput` | Metrics emitted | JSON | `Result` | End | Visibility into real user performance. |
| `SR_UI_45` | --- | interface | WCAG 2.1 AA accessibility per D-67: keyboard navigation, ARIA, color contrast 4.5:1 text / 3:1 UI, focus indicators, text scaling 200%, alt text, reduced motion, high contrast. | Accessibility framework | All renders | Various | Accessible UI | N/A | `Result` | End | Non-negotiable baseline; some customers require WCAG compliance. |

## Back-Propagation Log

| BP | Triggered By | Impacted SR | Remediation | Session |
|----|-------------|------------|-------------|---------|
| BP-110 | `SR_UI_05` (visibility check) | `SR_GOV_75` | Confirmed: governance is the source of truth; UI is the suppression layer. | 1 |
| BP-111 | `SR_UI_28` (shared WebSocket) | `SR_DM_22` (sync coordinator) | Confirmed: shared WS reduces socket count without affecting per-tenant event routing. | 1 |
| BP-112 | `SR_UI_36` (offline mode replay) | `SR_GOV_47` (audit) | Confirmed: queued actions replayed on reconnect each generate their own audit events with original timestamps preserved as metadata. | 1 |

## Self-Audit Findings

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Streaming chat (`SR_UI_20`) — what about partial messages on disconnect? | Implementation: partial messages cached client-side and re-displayed on reconnect; final response replaces. |
| 2 | Notification center (`SR_UI_25`) — pagination strategy? | Implementation: virtual scrolling with cursor pagination. |
| 3 | Admin undo (`SR_UI_35`) — what about race conditions during the 10-minute window? | Race conditions are bounded by the optimistic-lock pattern: undo fails if the resource has been modified by another action since. |
| 4 | Offline mode (`SR_UI_36`) — what data is cached? | Implementation: last-loaded dashboards, last 50 recommendations, last 20 chat messages. |
| 5 | i18n LLM responses (`SR_UI_42`) — how does the user-language preference reach the model? | The preference is part of the prompt context assembled by `REUSABLE_PromptAssembler`. |

## Cross-Reference Index

| From | To | Purpose |
|------|----|----|
| `SR_UI_05` | `SR_GOV_75` | UI visibility |
| `SR_UI_07` | `SR_LLM_23` | Live streaming chat |
| `SR_UI_08` | `SR_DS_22` | Recommendations delivery |
| `SR_UI_10` | `SR_INT_20` | Graph viz |
| `SR_UI_22` | `SR_INT_15` + `SR_GOV_33` | Semantic search with compartment filter |
| `SR_UI_25` | `SR_DM_25` | Notification log |
| `SR_UI_27` | `SR_DS_12` | Response capture |
| `SR_UI_30` | `SR_GOV_50` | Audit export |
| `SR_UI_35` | `SR_GOV_69` | Admin undo |

## Spec 07 Summary

| Metric | Value |
|--------|-------|
| Sections | 5 |
| Main-flow SRs | 45 |
| Exception SRs | 1 |
| Total SR rows | 46 |
| BP entries created | 3 (BP-110 through BP-112) |
| New decisions | 0 |

**Status:** Self-audit complete.

---

## Back-Propagation Received from Later Specs (applied retroactively in Session 2)

| BP | Originating Spec | Edit Applied to Spec 07 |
|----|-----------------|-------------------------|
| BP-101 | Spec 04 (`SR_INT_15` semantic search post-filter) | `SR_UI_22` updated with bidirectional reference and explicit dropped-for-compartment count |
| BP-111 | Spec 02 (`SR_DM_25` notification log) | `SR_UI_25` updated with bidirectional reference |
| BP-112 | Spec 01 (`SR_GOV_47` audit replay timestamps) | `SR_UI_25` and `SR_UI_36` updated to preserve `original_timestamp` |
| BP-120 | Spec 13 (`SR_SCALE_30` graceful degradation) | `SR_UI_29` updated with degradation participation as the WebSocket-fallback role |

**Total retroactive edits to Spec 07: 4 SR row updates.**
