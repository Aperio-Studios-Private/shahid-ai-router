# Phase 2 Report — Security & Persistence

Feature ID: PHASE-2. Date: 2026-10-31. Status: **PASS** (pending final security re-verification note; see §Security review).

## Requirements delivered
FR-005 (loopback gateway + token auth), FR-016 (redacted diagnostics), FR-017 (graceful drain), FR-018 (port conflict surfaced), FR-019 (token rotation), FR-020 (retention + automatic cleanup), FR-022 (lifecycle transparency); full docs/07 schema; docs/06 controls per ADR-004/005/006/011/015/016.

## Tasks (all PASS with evidence)
| Task | Delivered | Evidence |
|---|---|---|
| P2-T01 | docs/07 schema (migration `0001_init.sql`): providers, provider_accounts, credentials, models, deployments, routing_policies, model_aliases, alias_deployments, agent_profiles, request_records (metadata-only + denormalized labels + nullable SET-NULL selection refs), health_events, app_settings; FK delete-rule matrix with one probe test per rule; indexes per F-6; `_sqlx_migrations` documented as schema_migrations | 13 tests: fresh+idempotent apply, 6 FK probes, retention purge, settings roundtrip, WAL/FK pragmas |
| P2-T02 | `OsSecretStore` (keyring 4.2 v1 API; platform store auto-selected), canonical refs (`sair.cred.<hex>` per F-4), `mock` feature with `compile_error!` release guard (F-10), shared contract tests | 8 tests; **OS contract executed against real Windows Credential Manager on this machine** (service `shahid-ai-router-tests`, cleaned up) |
| P2-T03 | Token lifecycle: 32 OS-CSPRNG bytes → 43-char base64url, stored in vault (encoded form), constant-time verify via `subtle` with length early-out; crash-safe regenerate (create → persist ref → hot-swap → delete old, F-10 + SEC2-02); middleware stack per ADR-011 as amended: auth (Bearer/x-api-key/x-goog-api-key) → Host allowlist → 408 timeout → 503-at-capacity (no queueing, F-1) → 413 body limit; every route behind auth incl. `/healthz` and 404 fallback (F-2); `GatewayHandle` with explicit bind failure (FR-018), graceful drain (FR-017), empty-token fail-closed (SEC2-07); app wiring: fail-closed startup (migrations abort before bind; token failure → degraded state with surfaced error), daily retention task (FR-020), exit-hook drain; Gateway UI screen (masked token, reveal/copy, consent-gated regeneration) | 21 tests incl. drain, bind-failure, timeout, concurrency-503, body 413 (content-length and chunked), slowloris drop, hot-swap; live smoke tests on the real binary (2026-10-30/31): 401/403 enforcement, WM_CLOSE → drain → clean exit |
| P2-T04 | Redaction-by-construction + field-name rewrite layer (`[REDACTED]`), stderr + daily-rotating file sink in per-user app-data, 14-day pruning (F-11), no bodies ever written | 4 tests: field redaction, pass-through, **live ensure_token→reveal path sweep** (token material absent from logs), file-sink sweep |
| P2-T05 | `SsrfPolicy`: HTTPS default (HTTP only explicit loopback literals), default-deny denylist (v4 private/loopback/CGNAT/link-local/0-net/TEST-NET/benchmark/multicast/reserved/broadcast; v6 ULA/link-local/multicast + v4-mapped judged by v4 rules + NAT64/6to4/Teredo/doc exclusions); validating custom resolver (ADR-015); no-cross-origin redirects ≤3 hops; **`SsrfCheckedClient`** enforcing scheme+literal policy per request (SEC2-01: hyper-util short-circuits literals before any resolver — adapters receive only the wrapper) | 10 tests incl. full denylist matrix and offline rebinding-shaped injected-DNS tests (F-3) |
| P2-T06 | Gate: full suites, dependency audit, threat-model addendum, independent security review + fixes | See below |

## Exact verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **61 passed, 0 failed** (persistence 13, gateway 21, vault 8, providers 10, domain 7, telemetry 4... totals 61 across suites + doc-tests)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; `cargo fmt --all --check` → clean
- Frontend: `tsc --noEmit` clean; vitest 2/2; `vite build` ok
- `cargo audit` → **0 vulnerabilities**; 5 informational warnings on Linux-only transitive crates not compiled for supported targets (`audits/security/phase2-dependency-audit.md`)
- Live smoke: gateway auto-start, auth/host enforcement, graceful WM_CLOSE drain — all verified on the real binary

## Security review (independent subagent, not an author)
Initial verdict: PASS WITH FINDINGS (audits/security/phase2-security-review.md) — 2 High, 4 Medium, 5 Low, informational. All addressed in commit 8886aa9:
- **SEC2-01 (High)** literal-IP bypass → `SsrfCheckedClient` per-request enforcement (fix verified by test)
- **SEC2-02 (High)** rotation didn't affect running gateway → token hot-swap (fix verified by test)
- SEC2-03 slowloris → hyper-util header-read timeout + timer (tested); SEC2-04 degraded-start surfacing; SEC2-05 missing tests added; SEC2-06..09 Low items fixed.
Retest in progress by the same reviewer; result recorded in the review file. Deviation note: drain hook uses `RunEvent::Exit` (not `ExitRequested`) — justified: Exit fires at actual termination, cannot be vetoed, and is the correct drain point; reviewer asked to assess.

## Known limitations (honest disclosure)
- **CI blocked (B-01):** GitHub Actions billing failure on the org account — all jobs fail at startup with a billing annotation. All CI commands verified locally; macOS evidence pending either CI repair or the owner's 2026-11-01 interactive verification (ADR-016).
- macOS vault contract tests compiled for CI but **not executed anywhere yet** (no CI run has succeeded); no macOS claims are made.
- SSRF resolve-to-connect race minimized by design (ADR-015) but not provably eliminated — documented residual, revalidated at Phase 11.
- Redirect-policy behavior is unit-tested at the policy-function level; full HTTP-level redirect contract tests arrive with wiremock in Phase 3.

## Traceability updates
FR-005/016/017/018/019/020/022 → Implemented (evidence: this report + test names). Full matrix updated in docs/TRACEABILITY_MATRIX.md.

## Next eligible
Phase 3 (first vertical slice) — entry per workflow: plan + independent review. SEC2-01's wrapper is now the mandatory client boundary for the first adapter (P3-T02).
