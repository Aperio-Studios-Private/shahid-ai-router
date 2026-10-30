# Phase 9 Plan — Independent Review

Reviewer: independent plan reviewer. Date: 2026-10-30. Head reviewed: 8559b6a.
Subject: `audits/planning/phase9-plan.md` (UI Completion — Logs, Usage, Health, Deployments polish).

Reviewed against: docs/08_UX_SPEC.md (governing), docs/01_PRD.md FR-011/012/020, docs/07_DATA_STORAGE.md, docs/13_ROADMAP.md, .agent/SECURITY_RULES.md, .agent/QUALITY_GATES.md, and current code: `crates/sair-persistence/src/{repos,repos_rotation,lib,time}.rs`, `migrations/0001_init.sql`+`0002_routing_decisions.sql`, `crates/sair-gateway/src/api.rs`, `apps/desktop/src-tauri/src/{state,lib}.rs`, `apps/desktop/ui/src/{App.tsx,lib/{screens,tauri}.tsx?ts,screens/ProvidersScreen.tsx}`.

## Findings

### F-1 — HIGH / docs-08 & FR-012 compliance — decision_json exclusions are collected but never surfaced; the "explain why unavailable" promise is not met by the plan as written

Evidence:
- docs/08 Interaction: "Explain why a deployment is unavailable." FR-012: "explain unavailable, invalid or cooling-down states."
- The data exists per request: `RoutingDecision.exclusions: Vec<Exclusion{deployment_id, credential_id, reason}>` (`crates/sair-domain/src/routing.rs:144-162`), persisted into `request_records.decision_json` (`crates/sair-gateway/src/api.rs:706-712`, migration `0002_routing_decisions.sql`). Reasons include `provider_disabled`, `deployment_disabled`, `credential_{status}`, `credential_cooling_down` (`crates/sair-persistence/src/repos.rs:623-633`).
- The plan says `list_request_records(...) -> Vec<RecordView>` returns "metadata + decision reasons", but the Logs screen design lists columns (ts, family, alias, model, status, latency, error category, correlation id) with **no exclusion/decision display anywhere** — the reasons are fetched and dropped.
- The plan's claimed satisfaction path (Health Monitor "map statuses to human reasons") covers **credential** states only. `provider_disabled` / `deployment_disabled` exclusions are not credential statuses, produce no health events (the only `insert_health_event` call site is `api.rs:1062`, upstream failures), and have no other explanation surface. A user with a disabled deployment gets zero explanation anywhere in the UI.
- Worse: the single most diagnostic moment — a request failing 503 `no_eligible_deployment` — is **never recorded at all**. The handler returns early at `api.rs:664-702` (error path) before any `insert_request_record`; the only insert sites (record_attempt/record_failure_minimal, `api.rs:1190-1257`) run after a successful route. So Requests Logs will show nothing for exactly the "why is everything unavailable" case.

Recommendation (plan amendments):
1. Logs screen: add an expandable row / detail panel showing `exclusions` (deployment, credential, reason) parsed from decision_json for each record.
2. Health Monitor: the status→reason map stays, but add per-deployment availability (enabled/disabled + provider enabled) — `credential_health_summary` alone cannot explain a disabled deployment.
3. Either add an additive gateway insert on the `NoEligibleDeployment` path (status=503, error_category=`no_eligible_deployment`, decision_json=`{"exclusions":[...]}` — no schema change, helpers exist) **or** explicitly document in the plan and TRACEABILITY_MATRIX that no-route requests are absent from Logs, with manual-acceptance evidence per docs/01. Do not silently ship the gap.

### F-2 — MEDIUM-HIGH / security (SECURITY_RULES 9, docs/08 "never expose secrets in logs") — raw decision_json contains credential_secret_ref; the plan must mandate a projection, not pass-through

Evidence:
- `RouteCandidate` derives `Serialize` and includes `credential_secret_ref` (`crates/sair-domain/src/routing.rs:112-134`). `RoutingDecision{selected, ordered, exclusions}` therefore serializes every ordered candidate's vault reference, and `decision_json = json!({"decision": decision, "streamed": ...})` (`api.rs:706`) persists those refs into `request_records.decision_json`.
- The plan's `list_request_records` returns "metadata + decision reasons" without specifying the DTO. If the IPC layer echoes decision_json (or a parsed map containing `selected`/`ordered`) to the webview, vault refs land in the UI layer — a redaction violation in spirit (`secret_ref` is opaque, not key material, but it is credential material in a log surface; SECURITY_RULES 9, docs/06 lifecycle).

Recommendation: build `RecordView` in Rust with an explicit projection — record metadata + `exclusions[]` (deployment_id, credential_id, reason) + selected labels only. Never return raw `decision_json` text. Add a vitest/Rust test asserting `secret_ref` never appears in the IPC payload. Note this also bounds payload size (ordered lists can be long).

### F-3 — MEDIUM / correctness — usage_summary time windows: pin the comparison semantics and the aggregate shape

Evidence:
- All persisted timestamps are `chrono::Utc::now().to_rfc3339()` = RFC-3339 UTC with `+00:00` and **variable fractional precision** (`crates/sair-persistence/src/time.rs:5-7`). Lexicographic comparison of same-offset RFC-3339 strings is actually correct with variable precision (`'+' < '.'` orders whole-second before fractional; prefix rule handles digit truncation) — the codebase already relies on this (`repos.rs:621` cooldown compare; `purge_request_records_before` binds `cutoff.to_rfc3339()`, `lib.rs:86-95`). So the stored format is not the hazard.
- The hazard is **mixed formats**: if the 24h/7d cutoff is computed in the frontend (`new Date().toISOString()` → `Z` suffix, `'Z' > '+'`) or in any non-`+00:00` representation, boundary records are misclassified. The plan says "via SQL aggregates" without saying where the cutoff comes from.
- Shape: the DTO `{requests_24h, requests_7d, errors_24h, avg_latency_ms, tokens_in, tokens_out}` is inconsistent with the screen ("24h + 7d windows" for requests, errors, error rate, avg latency, tokens) — errors_7d, avg_latency and tokens are ambiguous (which window?).

Recommendation:
- Compute cutoffs in Rust from the same helper family (`Utc::now() - Duration` → `to_rfc3339()`) and bind as parameters; forbid JS-computed ISO strings. (Alternative: `WHERE unixepoch(ts) >= ?1` with epoch seconds, which is format-agnostic — slightly more robust, also fine.)
- Return a per-window struct, e.g. `Vec<WindowSummary{window:"24h"|"7d", requests, errors, error_rate, avg_latency_ms, tokens_in, tokens_out}>`.
- Pin "error" semantics (`error_category IS NOT NULL` — matches how failures are written at `api.rs:1219-1220` — or `status >= 400`; choose one, test it), and document AVG/SUM NULL handling (tokens nullable per FR-011 "when supplied").

### F-4 — MEDIUM / spec deviation — Models/Deployments → Providers navigation merge contradicts docs/08 Navigation and drops required fields

Evidence:
- docs/08 Navigation lists `Models; Deployments` as separate items (the `screens.ts` header comment even says "Navigation structure mandated by docs/08_UX_SPEC.md"). docs/08's own Screens section, however, groups them: "Models/deployments: provider IDs, aliases, capabilities, verification date." The doc is internally inconsistent; the plan picks the merged reading but treats it as settled.
- The existing merged panel does not satisfy docs/08's content list: `DeploymentsPanel` (ProvidersScreen.tsx:222-289) shows only `model_id`, enabled toggle, endpoint override — **no capabilities, no verification date**; `list_deployments` returns `DeploymentRow.capabilities` (`repos.rs:83-92`) but the TS interface drops it. The plan's "Deployments polish" doesn't mention adding them.
- Precedent: `api-keys` is already merged into ProvidersScreen (App.tsx:78), so merging is directionally consistent.

Recommendation: either (a) merge and amend docs/08 Navigation in the same commit (one-line doc change + update the screens.ts comment), and add capabilities + verified_at to the merged deployment rows, or (b) keep the two nav items routed to the same underlying view. Option (a) is preferred; "documented" in the plan must mean the docs/08 edit itself, not a plan note. Remove `models`/`deployments` from `ScreenId`/SCREENS and cover the change in tests.

### F-5 — MEDIUM / test plan adequacy — docs/13 gate for Phase 9 is "E2E/accessibility"; the plan has neither E2E nor documented manual acceptance, and misses the limit clamp

Evidence:
- docs/13_ROADMAP: "Phase 9 UI, logs, usage and diagnostics. Gate: E2E/accessibility." The plan's test section has vitest component tests + a11y assertions + Rust repo tests, but no E2E and no statement of manual-acceptance evidence (docs/01 permits "documented manual acceptance evidence" as the alternative).
- QUALITY_GATES frontend requires accessibility checks (plan covers roles/aria) and production build (covered via gate line).
- `list_request_records(limit, ...)` has no stated clamp; the house rule is R4-07 (see `list_health_events`, clamp 1..=500, repos_rotation.rs:59-60). Unbounded IPC fetches are also a payload-size risk for the webview.
- Minor: `alias_filter` as SQL `LIKE '%…%'` is unindexed — fine at desktop scale, but say so in the plan.

Recommendation: add one line stating the E2E position (minimal Tauri/WebDriver smoke of the three screens, or explicit manual-acceptance evidence with the checklist); clamp limit (e.g., 1..=500, default 100) and test the clamp; note the LIKE performance envelope.

### F-6 — LOW / docs-08 interaction rules — remaining unmapped corners

Evidence / gaps:
- "Every async action has loading/success/failure states": the plan spells empty/loading/error for Logs and covers all three screens in tests, but the **copy-correlation-id action** has no stated success feedback (accessible, not color-only — e.g., transient text "Copied" / aria-live, not a tooltip).
- "Do not use color alone": plan covers badges with text; also ensure the cooling badge text ("cooling") appears even when color is stripped (it does if text-based).
- Existing bug to fix while touching health UI: ProvidersScreen.tsx:181 renders `cooling` for **any** non-null `cooldown_until`, including expired cooldowns (no comparison with now). The plan's "live cooling badge" computes against now — reuse that component in the Providers screen or the stale-badge bug persists.
- Keyboard/visible focus: `index.css` has no focus styles today; the plan's global `:focus-visible` ring is the right fix — keep it global (nav buttons included).
- Retention footer claim verified accurate: purge runs at startup + every 24h (`src-tauri/src/lib.rs:121-130`), 30d default with floor 1 (`state.rs:288-306`).

Recommendation: fold the copy feedback state and the expired-cooldown badge fix into the plan's screen work; otherwise the mapping is adequate.

## Non-findings (checked, OK)

- Metadata-only history: schema has no body columns (0001_init.sql:130-151 + decision_json in 0002); "nothing to leak" holds for bodies, but see F-2 for decision_json.
- Correlation ids in the UI are fine (local random hex, record.id == correlation_id, api.rs:1202-1204); exclusion reason strings and internal ids are fine to display.
- Index coverage: 24h/7d aggregates and `ORDER BY ts DESC LIMIT` are served by `idx_request_records_ts`; retention keeps the table bounded (30d). No new index needed for the planned queries; correlation-id and deployment lookups already indexed.
- FR-020 claim in the plan is accurate (verified above).
- vitest + @testing-library/react + jest-dom already present in apps/desktop/ui/package.json; a11y-by-assertion is viable without axe.

## VERDICT: REVISE BEFORE IMPLEMENTATION — blocking: F-1 (exclusions must be surfaced; no-route records addressed or documented), F-2 (secret_ref projection + payload test). F-3/F-4/F-5 are required plan amendments (time semantics, docs/08 nav amendment + capabilities/verification date, E2E position + clamp); F-6 should be folded in during implementation.
