# Phase 4 Report — Rotation & Health Completion

Feature ID: PHASE-4. Date: 2026-10-31. Status: **PASS** (local evidence; CI blocked per B-01).

## Requirements delivered
FR-002 completion (priority/weight management, account groups), FR-008 (rotation with account-group quota truth), FR-012 (health events persisted + readable; credential health summary), FR-021 (per-deployment concurrency bound).

## Key fixes and behaviors
1. **R4-01 (P3 bug found by review):** health-event writes used classification names that violate the schema CHECK — the table was silently empty since Phase 3. Fixed with a category→event mapping (`rate_limited`/`billing_or_quota`→`cooldown_start`, `invalid_credential`→`invalid`, `permission_denied`→`quarantine`, others→`degraded`); insert failures are logged; read-back regression tests prove rows exist.
2. **Quota truth (docs/05):** a BillingOrQuota failure cools **every active credential in the account group** (monotonic single-statement UPDATE — R4-03: never shortens a standing cooldown), so rotation cannot burn shared-account keys.
3. **Retry-After (R4-04):** parsed from the standard HTTP header (seconds; HTTP-date ignored) and body, capped at 3600 s with saturating conversion — hostile values can neither panic the handler nor freeze credentials long-term.
4. **Per-request credential dedupe (R4-02):** a cooled credential can never be re-attempted within one request via a second deployment — engine-enforced, not incidental.
5. **FR-021 (R4-06):** per-deployment semaphores (configurable, default 4, floor 1, applied at gateway restart). A busy deployment rotates past WITHOUT consuming an attempt slot (accounting tracks actual upstream calls); terminal all-busy → structured 503 `deployment_busy`; permit held only across the upstream call.
6. Deterministic failure suite (R4-05): no timing-based assertions — the deployment-busy test holds the permit from the test itself.

## Exact verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **93 passed, 0 failed** (gateway 40 incl. 5 new failure-suite tests)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; `cargo fmt --all --check` → clean
- Frontend: tsc clean (TS 7), vitest 2/2, production build ok (R4-09 gate items)

## Simulated failure suite (docs/11 mandatory scenarios covered here)
| Scenario | Test |
|---|---|
| Primary RateLimited+Retry-After → fallback serves; primary cooled, fallback eligible | `rotation_serves_fallback_and_cools_primary` |
| 429 insufficient_quota with account group → BOTH keys cooling (no rotation-burn) | `billing_failure_cools_whole_account_group` |
| Health events persisted with schema-legal values and readable | `health_events_are_persisted_and_readable` |
| 8 concurrent requests after primary cooldown → no storm; served + bounded 503s only | `concurrent_requests_do_not_storm_cooled_credentials` |
| Deployment at concurrency bound → rotates past without consuming attempts | `deployment_busy_rotates_to_next_candidate` |
| InvalidCredential → quarantine only that credential | retained from Phase 3 (`upstream_auth_failure_quarantines_and_stops`) |

## Honest scope statements
- Cooldown expiry eligibility is covered by the snapshot comparison (excluded while `cooldown_until` is in the future); a dedicated wall-clock expiry test would need mock-clock injection — deferred to Phase 11 with the clock-abstraction work, noted in traceability.
- Health Monitor UI screen is data-ready (commands shipped); the screen itself lands with the Phase 9 UI pass.
- CI remains blocked (B-01). macOS evidence still pending.
- Review residuals (Low): RFC-3339 string comparison is format-consistent today but pinned to the shared `now_iso` helper (R4-08); group UPDATE is last-write-wins under simultaneous billing failures (WAL serializes; consequence documented).

## Traceability
FR-002 → Implemented (pool metadata + account groups); FR-008 → Implemented (rotation + account grouping); FR-012 → Implemented (events + summary; UI Phase 9); FR-021 → Implemented (per-deployment bound). docs/TRACEABILITY_MATRIX.md updated.

## Next eligible
Phase 5 (OpenRouter adapter + alias/capability matrix) — entry per workflow: plan + independent review.
