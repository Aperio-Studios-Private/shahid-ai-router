# Phase 2 Plan — Security & Persistence

Feature ID: PHASE-2. Date: 2026-10-31. Author: orchestrator agent. Status: DRAFT for independent review.

## PRD refs
FR-005 (loopback gateway + auth), FR-016 (redacted diagnostics), FR-017 (drain), FR-018 (port conflict surfaced), FR-019 (token rotation), FR-020 (retention), FR-022 (lifecycle transparency); docs/06 (all), docs/07 (schema, secret lifecycle), docs/05 (cooldown state), ADR-004/005/006/011/012, review findings R-5/R-6 resolutions.

## Repository state
Phase 1 complete (commit afdfbdf, pushed to origin/master): 6-crate workspace, Tauri shell, React UI skeleton, all gates green. Repo now hosted at github.com/Aperio-Studios-Private/shahid-ai-router; first CI run in progress.

## Verified facts (research; access dates 2026-10-30/31)
- keyring 4.2.0 exposes a v1-compatible API (`Entry::new(service, user)`, `set_secret`/`get_secret`/`delete_credential`) and selects a default platform store; platform store crates `windows-native-keyring-store` 1.1.0 / `apple-native-keyring-store` 1.0.2 are separate dependencies (docs.rs; GitHub hwchen/keyring-rs). Exact signature confirmation happens at compile/test time this phase.
- ring 0.23 `SystemRandom` (OS CSPRNG) and `ring::constant_time::verify_slices_are_equal` provide token generation and constant-time comparison without new dependencies.
- axum 0.8 provides `DefaultBodyLimit`; tower provides `ConcurrencyLimitLayer`; reqwest 0.13 provides `dns_resolver2` and custom redirect policies (release notes 2026-10-30).
- SQLx 0.9 migrations embed via `sqlx::migrate!`; per-file transactions are atomic; `no_tx` exists but is not needed.

## Assumptions
- The Windows Credential Manager roundtrip is testable on this machine (writes only entries under service `shahid-ai-router`, cleaned up in test teardown; no reading of unrelated user credentials — SECURITY_RULES #2 respected).
- macOS vault behavior compiles under CI; **runtime verification is the owner's, scheduled by them for 2026-11-01**. Reports must not claim macOS runtime verification before then.

## Design

### Schema (P2-T01) — `migrations/0001_init.sql`
Entities per docs/07: `providers`, `provider_accounts` (quota-truth grouping), `credentials` (secret_ref, status active|invalid|quarantined|incomplete, priority, weight, cooldown_until, last_* metadata), `models` (provider-specific id + capabilities JSON + protocol_family), `deployments` (provider+model+optional endpoint override), `model_aliases`, `alias_deployments` (priority/weight), `routing_policies` (strategy, attempts, budgets, timeouts), `agent_profiles`, `request_records` (metadata only — correlation_id, status, latency, token counts, error_category; **no bodies**), `health_events`, `app_settings` (KV JSON). FKs with `ON DELETE` rules, indexes on hot lookups (credentials by provider+status, request_records by ts). `CHECK` constraints on enums/strategy values.
Tests: fresh apply; idempotent re-apply; FK enforcement probe; retention delete query; migration runs inside startup before gateway binds.

### Vault implementation (P2-T02) — `sair-vault`
`OsSecretStore` (feature `os`, default in app builds): keyring v1 API, service `shahid-ai-router`, account `cred:<uuid>` / `gateway-token`. Refs like `sair://cred:<uuid>` stored in DB. `InMemorySecretStore` stays compile-gated behind feature `mock` (off in app build — enforced by app crate not enabling the feature + a unit assertion). Contract tests shared by both impls (`store_retrieve_delete`, `overwrite`, `unicode`, `not_found`, `delete_twice`); OS tests `#[cfg(any(windows, target_os = "macos"))]`, tagged and cleaned up. No plaintext fallback (docs/06).

### Gateway token & middleware (P2-T03) — `sair-gateway` + app wiring
- Token: 32 bytes from `ring::rand::SystemRandom`, base64url; stored in OS vault; `app_settings` holds the vault ref only.
- Auth middleware: accept `Authorization: Bearer`, `x-api-key`, `x-goog-api-key`; constant-time compare (ring); 401 JSON error on miss; unknown routes behind auth too.
- Host allowlist middleware: `Host` must be `127.0.0.1[:port]`/`localhost[:port]`/`[::1][:port]` → else 403 (R-5).
- `DefaultBodyLimit` 32 MiB default (configurable via app_settings); `ConcurrencyLimitLayer` bounded.
- Lifecycle: `GatewayHandle` with `start(config) -> Result` (FR-018: bind failure surfaces error, no fallback port), `stop(grace_period)` draining in-flight requests (FR-017); state exposed to UI via Tauri commands (`get_gateway_state`, `regenerate_gateway_token`, `reveal_gateway_token` masked in UI).
- UI: Gateway screen shows running state, endpoint, masked token with reveal/copy, regenerate with confirm; Overview reflects state (FR-022).

### Redaction (P2-T04) — `telemetry` module in app + tests
tracing-subscriber fmt to stderr+file; custom `Layer` that rewrites fields named `secret`/`token`/`authorization`/`api_key`/`key` to `[REDACTED]` (defense in depth — primary rule is values never enter spans). Sweep test: run middleware stack with known fake keys against a test router, capture formatted logs via `MakeWriter`, assert key material absent from output.

### SSRF connect layer (P2-T05) — `sair-providers::http`
- `SsrfPolicy`: denylist per planning-audit R-6 (IPv4 loopback/private/link-local/0.0.0.0/8/CGNAT/broadcast; IPv6 ::1, ULA, link-local, v4-mapped; exception only explicit loopback literals opted in).
- Custom connector (`tower::Service<Uri>`): resolve → validate **every** address → connect to validated IP directly (TOCTOU mitigation, R-6); literal IPs validated without DNS.
- Redirect policy: stop on host/scheme change; follow same-host ≤3. Auth headers never attached cross-host (client-side assertion + test).
- Tests: denylist matrix unit tests (each range); connector test with literal private IPs (no DNS needed) must fail pre-connect with `SsrfBlocked`; loopback exception opt-in test.

## Affected files
`crates/sair-persistence/src/**` (+ `migrations/0001_init.sql`), `crates/sair-vault/src/**`, `crates/sair-gateway/src/**`, `crates/sair-providers/src/http.rs` (new), `apps/desktop/src-tauri/src/**` (state, commands, gateway wiring), `apps/desktop/ui/src/screens/GatewayScreen.tsx` (new) + App wiring, `apps/desktop/ui/src/lib/` additions. No migrations of user data exist yet (first schema).

## Security/privacy risks & mitigations
- Token in logs → redaction layer + sweep test. Token in frontend storage → shown transiently, never persisted in webview (docs/06).
- Vault misuse → OS store only; mock feature-gated; no plaintext fallback.
- SSRF residual: DNS rebinding mitigated at connect layer; complete mitigation cannot be proven offline — revalidated in Phase 11 security review.
- Failure modes: vault unavailable at startup → app starts with gateway stopped + explicit error state (degraded, not silent); DB migration failure → startup aborts before gateway binds (docs/02 failure isolation).

## Tasks (already seeded in backlog; refined)
P2-T01 schema+migrations+tests → P2-T02 vault impls+contract tests → P2-T03 token+middleware+lifecycle+UI → P2-T04 redaction+sweep → P2-T05 SSRF connector → P2-T06 gate (full suite, security review subagent, report). Sequential; one owner (orchestrator); each task lands with tests and a board update.

## Acceptance criteria & tests
All listed above; phase gate = full `cargo test`, clippy `-D warnings`, fmt, tsc/vitest/build, new security tests green, security review completed with no open Critical/High, report + boards + traceability updated, commit pushed.

## Rollback
Single commit per task on master; revert by git; no user data exists yet (fresh schema). CI guards regressions.

## Review resolution (2026-10-31) — reviewer verdict: APPROVE WITH CHANGES
Full report: `audits/planning/phase2-plan-review.md`. Keyring v1 API and platform-store auto-selection confirmed by reviewer on docs.rs. Disposition:

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | Middleware stack = auth → Host allowlist → body limit → concurrency limit (immediate 503 at capacity, no unbounded queueing) → per-request `TimeoutLayer` (configurable budget). Negative tests: slow-headers client dropped within budget; slot exhaustion yields observable 503. |
| F-2 | Medium | `/healthz` sits behind auth — invariant "no unauthenticated route exists". Host check: exact match, case-insensitive, optional port, missing Host → 403; `[::1]` dropped from allowlist (v1 binds 127.0.0.1 only). Negative tests: spoofed-suffix Host, missing Host, HEAD on protected route, oversized chunked body, 404-behind-auth. |
| F-3 | High | SSRF mechanism changed: **validating custom `Resolve`** (resolution+validation together; only validated addrs returned; SNI/cert identity stays on hostname) + `redirect::Policy::custom` (stop on host/scheme change, ≤3 same-host). No IP-rewrite connector. `SsrfPolicy` gains scheme enforcement (https default; http only explicit loopback literals; non-HTTP schemes rejected; CRLF rejected at adapter boundary) and extended denylist (multicast 224.0.0.0/4, reserved 240.0.0.0/4, NAT64 64:ff9b::/96, 6to4 2002::/16, default-deny unclassified). Injectable mock resolver enables offline rebinding-shaped tests. Residual TOCTOU documented; revalidated Phase 11. |
| F-4 | Medium | Ref format charset-safe: `sair.cred.<uuid>`, `sair.gateway-token` (matches `SecretRef::parse`); format pinned by unit test shared by vault+persistence consumers. |
| F-5 | Medium | Fact corrected: ring 0.17 (locked 0.17.14 via rustls 0.23); `ring = "0.17"` added as direct workspace dep. Token: 32 bytes → 43-char base64url; length check before constant-time compare (length-only early-out documented). |
| F-6 | Medium | `request_records` gains deployment_id/credential_id (nullable FK ON DELETE SET NULL) + denormalized provider/model/deployment labels; full FK delete-rule matrix with one probe test per rule; added indexes: alias_deployments(alias_id), deployments(model_id), health_events(credential_id, ts); `schema_migrations` = sqlx `_sqlx_migrations` (documented). |
| F-7 | Medium | FR-020 fully in phase: startup purge before gateway bind + daily tokio interval task owned by app state; retention_days configurable with floor of 1. |
| F-8 | Medium | Decision (a): macOS vault contract tests **run in CI** and constitute macOS runtime evidence per ADR-013; pre-agreed fallback if entitlement errors appear: annotate + skip with recorded reason in the report (never silent). Reports state exactly which platform executed vault tests. Owner's 2026-11-01 interactive check remains the acceptance milestone. |
| F-9 | Medium | FR-017/FR-018 tests required: graceful drain via axum `with_graceful_shutdown` (slow in-flight completes or is aborted cleanly within grace); occupied-port bind → `GatewayError::Bind`, no fallback attempt; Tauri `RunEvent::ExitRequested` hook named as the app-exit drain caller; gateway start fails closed when vault token read fails (test). |
| F-10 | Low | Regenerate = new uuid vault item → swap app_settings ref → delete old item (crash-safe). Compile-time guard: release build with `mock` feature fails via `compile_error!`. Sweep extends to vault roundtrip + reveal path. |
| F-11 | Low | Logs at OS per-user app-data dir (`%APPDATA%/<ident>/logs`, `~/Library/Logs/<ident>`), daily rotation, files older than 14 days pruned at startup, no bodies ever written (ADR-008); file-writer sweep test added. Field-name layer documented as defense-in-depth only. |
| F-12 | Low | P2-T06 gate adds `cargo audit` and a threat-model addendum to docs/06 (local log files, token-reveal IPC, degraded-start states). |

Status: READY FOR IMPLEMENTATION (P2-T01 first).
