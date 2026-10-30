
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
