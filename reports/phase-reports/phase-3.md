# Phase 3 Report — First Vertical Slice

Feature ID: PHASE-3. Date: 2026-10-31. Status: **PASS** (local evidence; CI blocked per B-01).

## Requirements delivered
FR-001/002/003/004 (provider/credential/model/alias management), FR-006 (OpenAI-compatible surface, non-streaming per ADR-010), FR-007 (capability-aware routing v0), FR-009 (bounded rotation per ADR-017), FR-011 (request history incl. decision records), FR-012 (health diagnostics: exclusion reasons + health events).

## What works end-to-end (verified by tests + live smoke)
A client can `POST /v1/chat/completions` with `{"model": "<alias>", ...}` to the token-authenticated loopback gateway; the alias resolves through capability-filtered, strategy-ordered (deployment, credential) candidates; the credential secret is fetched from the OS vault at call time; the request passes through to the upstream with only `model` rewritten; the response passes back with usage recorded; failures are classified (body-aware), drive rotation/quarantine/cooldown per ADR-017, and every request leaves a metadata-only record with the full routing decision (FR-012).

## Tasks (all PASS)
| Task | Delivered | Evidence |
|---|---|---|
| P3-T01 | Domain types: `UpstreamFailure`+`FailureAction` (rotation semantics), `CapabilityMetadata`, `Strategy`, `RouteCandidate`, `RoutingDecision`; migration 0002 (decision_json, health correlation, allow_loopback) | domain tests 6; persistence upgrade test 0001→0002 with defaults |
| P3-T02 | Typed repositories + route snapshot loader (exclusion reasons per FR-012); OpenAI-compatible adapter with **body-aware classification** (R3-01: 429+`insufficient_quota`→BillingOrQuota) via `SsrfCheckedClient` only (ADR-015 rule) | persistence 14 tests; providers 19 tests incl. wiremock contract tests (request shape, quota-vs-ratelimit, malformed-200, streaming rejection) |
| P3-T03 | Routing engine v0: priority/round_robin/weighted (weighted shuffle), lru/pin → structured 501 (ADR-018), `RotationState` behind `tokio::sync::Mutex` (ADR-012) | gateway routing tests incl. **deterministic 20-task concurrent round-robin** (10/10 split, R3-04) |
| P3-T04 | `/v1/chat/completions` (non-streaming passthrough) + `/v1/models` (aliases), quarantine/rotation/cooldown flow, OpenAI error envelope + `shahid` diagnostics, request records + health events, correlation ids | 7 E2E tests through the live gateway + wiremock upstream (happy path, auth-on-API, alias 404, streaming rejection, models list, quarantine→no-eligible, quota cooldown) |
| P3-T05 | Management IPC commands (25) + UI: Providers (add/enable/disable/test/delete; per-provider API keys with vault-backed secrets; deployments) and Aliases (bind/unbind) | `cargo build` + `npm run build` green; secrets never returned by any command; credential delete removes the vault item |
| P3-T06 | Gate: full suites + live smoke | see verification |

## Exact verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **88 passed, 0 failed** (persistence 14, gateway 35, providers 19, vault 4, domain 6+units)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; `cargo fmt --all --check` → clean
- Frontend: tsc clean (TS 7), vitest 2/2, production build ok
- Live smoke on the real binary: gateway auto-start, `/v1/models` behind auth (401 without/with wrong token), graceful drain

## Honest scope statements
- **Streaming is not implemented** (Phase 7): `stream:true` → structured 400 `streaming_not_supported`. No stream-commit failover concerns exist yet.
- **Timeout does not rotate** (ADR-017): ambiguous-timeout generation requests surface to the client instead of risking duplicate cost.
- **`lru`/`manual_pin` strategies are declared but unimplemented** → structured 501 (visible incompleteness per PRD acceptance rule).
- **FR-021 (per-deployment concurrency bound)** remains Phase 4 — the global gateway bound applies in P3 (claim dropped per review R3-05).
- Anthropic/Gemini protocol surfaces are Phase 6; the UI marks them accordingly.
- CI remains blocked (B-01, GitHub Actions billing — owner action). All evidence is local Windows; macOS pending (ADR-016).
- Review residuals (Low): DB-open/migration degraded-start errors are log-only (app cannot show UI state that early); SEC2-02 ref-divergence on DB failure inside regenerate.

## Traceability
FR-001→004, FR-006 (openai surface), FR-007, FR-009, FR-011, FR-012 → Implemented (evidence: this report + named tests). Updated in docs/TRACEABILITY_MATRIX.md.

## Next eligible
Phase 4 (rotation & health completion: account grouping, per-deployment concurrency FR-021, cooldown race hardening) — entry per workflow: plan + independent review.
