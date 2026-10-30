# Phase 9 Plan — UI Completion (Logs, Usage, Health, Deployments polish)

Feature ID: PHASE-9. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-011/012 UI surfaces, docs/08 (Logs: filters, correlation IDs, redaction; Usage; Health Monitor: explain unavailable states; interaction rules: loading/success/failure states, keyboard navigation, visible focus, no color-only status), FR-012 (explain why a deployment is unavailable).

## Repository state
Phase 8 complete (8559b6a): 132 tests green. Remaining placeholder screens: Models, Deployments, Routing Policies (partially), Request Logs, Usage, Health Monitor.

## Design

### 1. Data surface (sair-gateway IPC, no new HTTP surface — UI-only data)
- `list_request_records(limit, alias_filter, error_filter) -> Vec<RecordView>` (metadata + decision reasons; bodies never stored — nothing to leak).
- `usage_summary() -> {requests_24h, requests_7d, errors_24h, avg_latency_ms, tokens_in, tokens_out}` via SQL aggregates.
- Existing: `list_health_events`, `credential_health_summary`, `list_deployments`, `list_aliases`.

### 2. Screens
- **Request Logs**: table (ts, family, alias, model, status, latency, error category, correlation id), filters (alias text, error-only toggle), limit selector; correlation id copyable; empty/loading/error states (docs/08 interaction rules).
- **Usage**: aggregate cards (requests, errors, error rate, avg latency, tokens) — 24h + 7d windows.
- **Health Monitor**: credential summary table (status, cooldown_until with live "cooling" badge, last error category+code, last used) + health event list (event, subject, category, correlation id); explanation text per status (docs/08: explain why unavailable — map statuses to human reasons).
- **Deployments polish**: enabled/disabled + endpoint override display already exists in Providers screen; Models screen remains part of Providers (docs/08 groups models/deployments) — remove the standalone Models/Deployments placeholder by routing to Providers screen (navigation merge documented).
- Keyboard navigation/focus: native buttons/inputs (already focusable); add `:focus-visible` ring styles (Tailwind) globally; aria labels on status dots (aria-label, not color-only).

### 3. Retention note
FR-020 purge (30d) already active server-side; the Logs screen notes the retention window in its footer.

## Tests
- vitest: Logs/Usage/Health screens render states (loading/empty/error) with mocked invokeOr; a11y: roles + aria-labels asserted; no color-only status (badge has text).
- Rust: repo query tests for usage_summary/records filters.
- Gate: full suites + clippy + tsc + build.

## Rollback
Additive screens + one repo fn; no schema change.

## Review resolution (2026-10-31) — verdict: REVISE BEFORE IMPLEMENTATION (resolved)

Report: `audits/planning/phase9-plan-review.md`. All incorporated:

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | (a) `process_chat` inserts a no-route record on NoEligibleDeployment/AliasNotFound-with-candidates paths (correlation, alias, family, operation, error code, decision reasons — no upstream attempt occurred, so no candidate fields); (b) Logs rows render exclusion reasons from decision_json (expandable per-row "why unavailable" list). |
| F-2 | High-Hazard fix | decision_json NEVER goes to the webview raw. Rust-side projection: RecordView carries `exclusions: Vec<{deployment, credential, reason}>` parsed server-side from decision_json, with credential refs replaced by credential LABELS (join at query time); new no-secret_ref assertion test (SECURITY_RULES 9). |
| F-3 | Medium | usage_summary cutoffs computed in Rust chrono (never JS); DTO pinned: {requests_24h, errors_24h, avg_latency_ms_24h, tokens_in_24h, tokens_out_24h, requests_7d, errors_7d}; "error" = error_category IS NOT NULL. |
| F-4 | Medium | docs/08 amended in the same commit (Navigation: Models/Deployments fold into Providers; Screens section already grouped them — inconsistency resolved in favor of grouping); DeploymentsPanel gains capability + verified_at display. |
| F-5 | Medium | Manual a11y acceptance checklist executed and recorded in the phase report (keyboard traversal, visible focus, screen-reader labels on all dynamic states) + vitest role/aria assertions; list_request_records limit clamped 1..=500 (R4-07 precedent). |
| F-6 | Low | Copy-correlation-id shows inline "Copied" confirmation; ProvidersScreen cooling badge reuses the live now-comparison (fixes the expired-cooldown display bug). |

Status: READY FOR IMPLEMENTATION.
