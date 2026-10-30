# Phase 4 Plan — Rotation & Health Completion

Feature ID: PHASE-4. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-002 (credential pool completion: priority/weight management), FR-008 (rotation with account grouping), FR-012 (health states surfacing), FR-021 (per-deployment concurrency bound), FR-002's cooldown display; docs/05 (Quota truth, Retry safety), ADR-012/017/018.

## Repository state
Phase 3 complete (e314a5d): routing engine v0 with rotation/quarantine/cooldown, OpenAI surface, management UI, 88 tests green.

## Design

### 1. Account grouping (R3 follow-up to docs/05 Quota truth)
- `provider_accounts` table exists (Phase 2 schema). Credentials get `account_id` (nullable FK, SET NULL).
- New behavior: when a failure classifies as **BillingOrQuota**, the cooldown applies to EVERY active credential in the same account group (same `provider_accounts.id`); credentials without an account group get individual cooldowns. Rationale recorded: keys under one account/project share quota (docs/05) — rotating inside the group burns keys.
- Implementation: on BillingOrQuota, resolve `account_id` of the failed credential, then `UPDATE credentials SET cooldown_until = ? WHERE (account_id = ? OR id = ?) AND status = 'active'` (single statement, no transaction across awaits).
- Health event per affected credential (`cooldown_start`, category billing_or_quota, correlation id).

### 2. FR-021 per-deployment concurrency bound
- `GatewayConfig`/app_settings gains `deployment_concurrency` (default 4, floor 1). Implementation: `Arc<Mutex<HashMap<String, Arc<Semaphore>>>>` in ApiDeps; the chat handler acquires the candidate's deployment semaphore before the upstream call (try_acquire → structured 429-style 503 `deployment_busy` for THAT candidate, router continues to next candidate — a busy deployment is a rotation trigger, not a client failure). Semaphores are per-deployment, configurable via settings; permit released when the upstream call completes.

### 3. Credential pool management completion (FR-002)
- New commands: `set_credential_priority_weight(id, priority, weight)`, `list_provider_accounts(provider_id)`, `add_provider_account(provider_id, label, group_key)`, `assign_credential_account(credential_id, account_id)`.
- UI: API Keys screen gains priority/weight editing and an account-group selector.

### 4. Health monitor surface (FR-012 completion)
- Command `list_health_events(limit)` (default 100) + `credential_health_summary()` (per credential: status, cooldown, last_error, last_used).
- UI Health Monitor screen: read-only table.

### 5. Simulated failure suite (gate, docs/11)
E2E via wiremock + two credentials on one provider (and a two-provider alias):
- primary credential fails with RateLimited+Retry-After → rotation serves from fallback; Retry-After sets cooldown (verified via DB).
- 429 insufficient_quota with account group → BOTH credentials cooling after one failure (quota truth test).
- cooldown expiry: cooldown_until in the past → candidate eligible again.
- concurrent rotation + cooldown: 20 parallel requests against 2 credentials with the first always 429-limited → assert no storm (each credential attempted at most once per request window) and fallback serves.
- FR-021: deployment concurrency 1 + slow upstream → second parallel request rotates or 503s `deployment_busy` (bounded, tested).
- InvalidCredential → quarantine only the failed credential (existing test retained).

## Acceptance / gate
All new tests + full suites green; clippy/fmt clean; frontend builds; report + traceability (FR-002/008/012/021 updates) + push.

## Rollback
Additive commits; migration none required (columns exist). Settings default additions are backward compatible.

## Review resolution (2026-10-31) — verdict: APPROVE WITH CHANGES
Report: `audits/planning/phase4-plan-review.md`. All findings incorporated:

| ID | Sev | Resolution |
|---|---|---|
| R4-01 | High | P3 bug fixed: `health_event_for_category()` maps classification → schema-legal event (`rate_limited`/`billing_or_quota`→`cooldown_start`, `invalid_credential`→`invalid`, `permission_denied`→`quarantine`, others→`degraded`); insert errors are no longer silently swallowed (logged at warn); **read-back regression tests** assert rows exist after failures. |
| R4-02 | High | Per-request attempted-credential `HashSet` in the rotation loop: candidates whose credential was already attempted are skipped with an exclusion reason; the "no storm" guarantee is engine-enforced (tested). |
| R4-03 | Medium | Cooldown writes are **monotonic** (single statement, `WHERE cooldown_until IS NULL OR cooldown_until < :until` guard) — a shorter Retry-After can never shorten a standing cooldown. |
| R4-04 | Medium | Retry-After parsed from the standard **HTTP header** (seconds form; HTTP-date ignored) OR body, capped at 3600 s at the source, saturating i64 conversion (no wrap/panic on hostile input); billing-path Retry-After follows the same cap rule. |
| R4-05 | Medium | Concurrency test reshaped: deterministic two-phase design (sequential rotation assertions first; then parallel phase asserting fallback serving + cooldown existence, not exact hit counts). |
| R4-06 | Medium | FR-021 semantics pinned: deployment-busy skips do **not** consume `max_attempts` (not an upstream attempt) but are bounded by the candidate list; all-busy terminal state → 503 `deployment_busy` envelope; permit is per-candidate-iteration, held across the upstream call only; setting applies at gateway (re)start — documented. |
| R4-07 | Medium | `assign_credential_account` validates same-provider; weight validated ≥ 1 before write; `list_health_events` limit clamped 1..=500; negative priority is legal (documented in UI hint). |
| R4-08 | Low | Single shared timestamp helper (`sair_persistence::time::now_iso`) used for cooldown writes and comparisons; cooldown writer routes through it. |
| R4-09 | Low | Gate includes frontend checks (typecheck, vitest, production build); no new dependencies introduced. |

Status: READY FOR IMPLEMENTATION.
