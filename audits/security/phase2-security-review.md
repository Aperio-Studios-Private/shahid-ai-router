# Phase 2 Security Review — Shahid AI Router (independent)

Reviewer: independent security reviewer (did not author the code). Date: 2026-10-31.
Code under review: commit `f86be40` ("feat(phase-2): security & persistence").

> **Retest status (2026-10-31):** fixes retested against commit `8886aa9` — see the
> **Retest** section at the end of this file. Both High findings are **FIXED** and
> tested; SEC2-04 is PARTIAL (residual scope reduced to Low). Final verdict:
> **PASS WITH FINDINGS** (Low residuals and Phase 3 watch items only).
Inputs: docs/06 (incl. Phase 2 threat-model addendum), .agent/SECURITY_RULES.md, .agent/QUALITY_GATES.md, docs/DECISION_LOG.md ADR-004/005/006/011/015/016, audits/planning/phase2-plan.md (§Review resolution), audits/planning/phase2-plan-review.md (F-1…F-12).

## Method and verification performed

All in-scope files read in full: `crates/sair-gateway/src/{lib,handle,token}.rs`, `crates/sair-vault/src/{os,store,refs,mock,lib}.rs`, `crates/sair-providers/src/http.rs` (+ `lib.rs`, `adapter.rs`), `apps/desktop/src-tauri/src/{state,commands,telemetry,lib,main}.rs`, `crates/sair-persistence/src/{lib,time}.rs`, `migrations/0001_init.sql`, `.github/workflows/ci.yml`, all crate manifests, `tauri.conf.json`, `capabilities/default.json`, and the UI (`GatewayScreen.tsx`, `lib/tauri.ts`). Cross-checked against vendored dependency sources (hyper-util 0.1.21, hyper 1.12.0, reqwest 0.13.5, axum 0.8.9, tauri 2.12.5) in the local cargo registry, and `Cargo.lock`.

Test execution (no files modified; existing suites only):
- `cargo test -p sair-gateway -p sair-vault -p sair-persistence -p sair-providers`: **43 tests pass** (gateway 17, persistence 13, providers 9, vault 4).
- `cargo test -p sair-vault --features os`: **6 tests pass**, including the live OS contract against Windows Credential Manager on this machine (`os_store_passes_contract`, `store_delete_credential_ref_roundtrip`) — keyring 4.2.0 v1 usage is empirically confirmed on Windows.

Claims that matter most were verified against dependency source, not assumed: the reqwest/hyper-util resolver wiring (SEC2-01), hyper's header-read-timeout default and its Timer requirement (SEC2-03), axum's layer ordering (SEC2-11), and Tauri's `state()` panic behavior (SEC2-08). Each such claim below cites the vendored file.

---

## Findings summary

| ID | Severity | Category | Component |
|---|---|---|---|
| SEC2-01 | **High** | SSRF control gap | `sair-providers::http` (ValidatingResolver / build_client / SsrfPolicy) |
| SEC2-02 | **High** | Token lifecycle / revocation | app `state.rs` + `commands.rs` + gateway runtime state |
| SEC2-03 | Medium | Local DoS (slowloris) | gateway server construction (`handle.rs`), TimeoutLayer scope |
| SEC2-04 | Medium | Degraded-start transparency | app `lib.rs` init_backend / `state.rs` gateway_error |
| SEC2-05 | Medium | Review-resolution gaps (claimed negative tests absent) | gateway tests, telemetry tests, app exit hook |
| SEC2-06 | Low | CI least privilege | `.github/workflows/ci.yml` |
| SEC2-07 | Low | Auth defense-in-depth (empty expected token) | `token.rs`, `handle.rs`, app `state.rs` |
| SEC2-08 | Low | Exit-path panic | app `lib.rs` RunEvent::Exit hook |
| SEC2-09 | Low | Config validation gaps | app `state.rs` gateway_config |
| SEC2-10 | Low | Redaction layer completeness | `telemetry.rs` |
| SEC2-11 | Informational | Doc accuracy | gateway `lib.rs` header comment |
| SEC2-12 | Informational | Interop (fails closed) | gateway `extract_token` |
| SEC2-13 | Informational | Misc hardening notes | several |

No Critical findings. **Two High findings (SEC2-01, SEC2-02) are open**; per `.agent/QUALITY_GATES.md` ("Critical and High block shipping") they must be resolved before the phase exit criterion "security review completed with no open Critical/High" is met.

---

## Detailed findings

### SEC2-01 — High — SSRF — ValidatingResolver is never invoked for literal-IP destinations; scheme policy has no production caller

**Affected component:** `crates/sair-providers/src/http.rs` (`ValidatingResolver`, `build_client`, `SsrfPolicy::validate_url`).

**Description.** The entire SSRF mechanism of ADR-015/F-3 is the custom resolver handed to reqwest. reqwest 0.13.5 wires it as `HttpConnector::new_with_resolver(resolver.clone())` (`reqwest-0.13.5/src/async_impl/client.rs:452`), where `HttpConnector` is hyper-util's legacy connector. hyper-util 0.1.21 short-circuits IP-literal hosts **before** any resolver is consulted — `hyper-util-0.1.21/src/client/legacy/connect/http.rs:556-560`:

```rust
// If the host is already an IP addr (v4 or v6),
// skip resolving the dns and start connecting right away.
let addrs = if let Some(addrs) = dns::SocketAddrs::try_parse(host, port) {
    addrs
} else { … resolve(&mut self.resolver, …) … };
```

Consequently, for any URL whose host is an IP literal — `http://10.0.0.5:8080`, `http://169.254.169.254/latest/meta-data/`, `http://192.168.1.1/`, `http://[fd00::1]/`, or v4-mapped forms — **the `ValidatingResolver` and the whole `SsrfPolicy` denylist are bypassed and the connection proceeds directly**. The code comment asserting the opposite (`http.rs:193` "Literal IPs bypass DNS entirely but still go through the policy") is true only for direct `validate_host` calls, which is exactly what the unit tests exercise (`literal_ips_bypass_dns_but_not_policy`, `http.rs:421-432`) — giving false confidence: the test never runs through reqwest, where the bypass exists.

Additionally, `SsrfPolicy::validate_url` (scheme enforcement: HTTPS by default, HTTP only for loopback literals, non-HTTP rejected — docs/06 and F-3/F-12 resolution) **has no production caller**: grep shows it referenced only from tests and the module export. Nothing in any request path enforces the scheme policy today.

**Impact.** The phase's headline security control does not cover an entire class of destinations, and the documented scheme policy is not wired. Today this is latent — grep confirms `build_client` has no production caller yet (adapters arrive in P3-T02) — but the schema already carries user-configurable endpoints (`providers.base_url`, `deployments.endpoint_override`), so the moment Phase 3 wires a custom URL, a user- or attacker-supplied literal-IP URL reaches internal/metadata targets unvalidated. ADR-015's claim that "only validated addresses are returned (reqwest connects exactly to resolver output)" is false for the literal-IP class.

**Evidence.**
- `crates/sair-providers/src/http.rs:193-199` — literal path assumed covered via resolver; `http.rs:220-232` — `Resolve` impl (hostnames only, in practice).
- hyper-util 0.1.21 `src/client/legacy/connect/http.rs:556-560` (quote above) — locked in `Cargo.lock` (hyper-util 0.1.21, reqwest 0.13.5, hyper 1.12.0).
- `crates/sair-providers/src/http.rs:421-432` — test validates `validate_host` directly, never via `build_client`.
- No caller of `validate_url` outside tests (grep over workspace).

**Recommendation.** Enforce the policy at a layer that always executes, not only inside the DNS resolver:
1. At the adapter boundary, before dispatch: parse the URL; if the host is an IP literal, apply `SsrfPolicy::is_allowed` directly (and apply `validate_url` for the scheme on every request, not just at config time).
2. Optionally also wrap the connect stage via reqwest `connector_layer` to re-check the destination host string before each connect (covers literal IPs without touching TLS identity — the layer sees the destination Uri, not resolved addrs).
3. Add an integration test through `build_client`/reqwest itself (local TCP listener for allowed loopback with `allow_loopback` opt-in; assert literal private IP fails pre-connect) so the test exercises the real connector path. Amend ADR-015 to state where literal-IP enforcement lives.

---

### SEC2-02 — High — Token lifecycle — regeneration never reaches the running gateway: the old token stays valid and the new token does not work until manual restart

**Affected component:** `apps/desktop/src-tauri/src/state.rs` (`regenerate_token`), `apps/desktop/src-tauri/src/commands.rs`, `crates/sair-gateway/src/lib.rs` (`GatewayRuntimeState`), UI `GatewayScreen.tsx`.

**Description.** `regenerate_token` (state.rs:116-139) creates a new vault item, swaps the persisted ref, deletes the old item, and returns the new token. It **never touches the gateway**. The running server's expected token was captured once at router build time:

```rust
// crates/sair-gateway/src/lib.rs:58-62
let state = GatewayRuntimeState {
    expected_token: config.token.clone(),
    permits: Arc::new(tokio::sync::Semaphore::new(config.concurrency)),
};
```

There is no mechanism to update `expected_token` on a live router, and neither the IPC command (`commands.rs:51-54`) nor the UI (`GatewayScreen.tsx:59-75`, which only invokes the command and refreshes) stops/starts the gateway.

**Impact.**
1. **Revocation is broken while the gateway runs.** A leaked token remains valid indefinitely after the user regenerates (a desktop gateway session can persist for days). This defeats FR-019 (token rotation) as a security control in exactly the scenario rotation exists for.
2. The UI's own consent dialog misstates reality: "Existing clients configured with the current token will stop working until you update them" (`GatewayScreen.tsx:60-63`) — they will **not** stop working; the old token keeps authenticating. The user believes rotation took effect when it did not.
3. The newly revealed token returns 401 against the running gateway until the user manually stops and starts the gateway (or restarts the app); nothing tells them this.
4. In the DB-write-failure path of `regenerate_token` (state.rs:124-130: in-memory ref already swapped, DB update fails), the in-memory ref points to the new item while the DB keeps the old; subsequent `reveal_token` returns the new token while the gateway still expects the old — a permanently inconsistent view.

**Evidence.** state.rs:116-139 (no gateway interaction); commands.rs:51-54; `crates/sair-gateway/src/lib.rs:52-55,59-62` (Arc captured once); GatewayScreen.tsx:59-75. Verified by reading all callers — no restart or token hot-swap exists.

**Recommendation.** Either (a) make the command atomic end-to-end: `stop_gateway(grace)` → regenerate → `start_gateway()` inside `regenerate_gateway_token` (simplest, matches the UI text), or (b) hot-swap: hold the expected token in `ArcSwap<Vec<u8>>`/`watch` and update the router state on regeneration. Additionally reconcile the in-memory `token_ref` with the DB on failure (roll back the in-memory swap), and align the UI warning with actual behavior.

---

### SEC2-03 — Medium — Local DoS — slow-headers (slowloris) connections have no read timeout; the timeout budget does not cover the header phase

**Affected component:** `crates/sair-gateway/src/handle.rs:73-77` (server construction), middleware stack in `lib.rs`.

**Description.** The per-request `TimeoutLayer` (lib.rs:90-93) only starts when a fully-parsed request reaches the middleware — hyper parses request headers before any tower middleware runs. Partial-header connections therefore never enter the timeout budget. hyper's own `header_read_timeout` would cover this, but (a) its default (30 s, `hyper-1.12.0/src/server/conn/http1.rs:253`) only takes effect "Requires a [`Timer`] set by [`Builder::timer`] to take effect" (`hyper-1.12.0/src/server/conn/http1.rs:364`), and (b) axum 0.8.9's `serve` builds the connection builder with **no timer**: `hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())` (`axum-0.8.9/src/serve/mod.rs:391`) with no `.timer()` call. Result: a malicious local client can open connections and drip request bytes forever, never completing headers, never consuming a concurrency permit, and never being dropped. There is also no connection cap (the concurrency limit only counts post-header requests). docs/06 explicitly lists "malicious local clients" as in-scope.

**Impact.** Unbounded accumulation of idle/partial connections (file descriptors, memory) by an unauthenticated local process — the exact threat docs/06 scopes in. Bounded practical impact for a desktop app, hence Medium, not High.

**Note.** This is also the promised-but-missing F-1 negative test ("slow-headers client dropped within budget" — phase2-plan.md §F-1 resolution); the implemented timeout test covers a slow *handler*, not slow *headers* (`handle.rs:210-222`).

**Recommendation.** Replace `axum::serve` with an explicit `hyper_util::server::conn::auto::Builder` + `serve_connection_with_graceful_shutdown`, setting `.http1().timer(TokioTimer::new())` (activating hyper's 30 s header-read default) and an explicit `header_read_timeout` consistent with the configured budget; add the missing slow-headers negative test. Revisit trigger documented in ADR-011 already covers "new client classes"; this closes the gap for the current one.

---

### SEC2-04 — Medium — Degraded-start failures are logged but never surfaced in the UI error state

**Affected component:** `apps/desktop/src-tauri/src/lib.rs:96-114` (`init_backend`), `state.rs` (`gateway_error`), `GatewayScreen.tsx:116-120`.

**Description.** The threat-model addendum (docs/06:25-30) commits to "explicit error state surfaced in the UI" for degraded starts. The only writer of `gateway_error` is `start_gateway`'s bind/token failure path (state.rs:166-171). Startup failures that occur **before** that — DB open failure (lib.rs:77-79), migration failure (lib.rs:82-84), and `ensure_token` failure (lib.rs:101-104) — `return Err(...)` and log via `tracing::error!` only; `gateway_error` is never set. The UI then shows a normal "Stopped" gateway with `error: null` (`GatewayStateView`), indistinguishable from a user-initiated stop. The user cannot see that the DB is broken or the vault is unavailable, and cannot act on it.

**Evidence.** lib.rs:100-114 (errors returned/logged, no `gateway_error` write); state.rs:50-58 (`GatewayStateView` from `gateway_error`); GatewayScreen.tsx:116-120 (renders `state.error` only).

**Impact.** Silent-ish degraded state contradicting docs/06 addendum and FR-022 lifecycle transparency; the only trace is in log files the user was never asked to open.

**Recommendation.** Manage `AppState` before backend init (or add an init-error field to `AppState`), set `gateway_error` for every startup failure class, and surface it in `GatewayStateView`.

---

### SEC2-05 — Medium — Plan-review resolutions claimed incorporated but absent from the code (evidence gaps)

**Affected component:** test suites and exit hook.

Per the review contract, findings claimed resolved that are not actually in the code are findings. Itemized:

1. **F-11 file-writer sweep test — absent.** The resolution records "file-writer sweep test added" (phase2-plan.md F-11). All redaction sweeps (`telemetry.rs:266-330`) capture through the in-memory `CapturedLogs` `MakeWriter`; no test exercises the rolling **file** appender path (`telemetry.rs:207-218`) asserting no key material lands in the log file. P2-T04's BACKLOG claim ("sweep tests (fields, vault roundtrip, reveal path)") matches what exists, but the F-11 resolution text overstates it.
2. **F-1 slow-headers negative test — absent** (see SEC2-03; the mechanism itself is also missing).
3. **F-2 oversized *chunked* body negative test — absent.** `oversized_body_is_rejected_with_413` (handle.rs:259-273) uses a content-length body; the chunked-encoding variant from the F-2 resolution was not implemented (low risk — `DefaultBodyLimit` applies at body-read time either way).
4. **F-9 exit-hook deviation.** The resolution names `RunEvent::ExitRequested` as the drain caller; the implementation uses `RunEvent::Exit` (lib.rs:55-65). Functionally similar (drain still runs), but the documented resolution does not match the code — record the deviation or switch.

**Recommendation.** Add the file-writer sweep (write through a real temp-file appender in a test, read back, assert no seeded material), the chunked-body test, and reconcile F-9's wording. Update the phase report to claim only what exists (QUALITY_GATES: "state exactly what was tested").

---

### SEC2-06 — Low — CI workflow has no `permissions:` block (least privilege)

`.github/workflows/ci.yml` defines `rust` (push to master + PRs) and `frontend` jobs with no top-level or job-level `permissions:`. The GITHUB_TOKEN therefore carries the repository default (which may include write scopes depending on repo settings). No workflow step needs anything beyond `contents: read`.

**Recommendation.** Add `permissions: { contents: read }` at workflow top level (tighten further per job if needed). (Review scope explicitly includes ci.yml least privilege; no secrets are currently consumed in CI, hence Low.)

---

### SEC2-07 — Low — An empty expected token would authenticate an empty Bearer (defense-in-depth gap; unreachable through current startup paths)

`verify_token` (`crates/sair-gateway/src/token.rs:33-41`) returns `true` when both sides are empty: `provided.len() != expected.len()` passes (0 == 0) and `"".ct_eq("")` folds to true. `GatewayConfig::default()` carries an empty token (`handle.rs:33-34`), and `extract_token` yields `Some("")` for `Authorization: Bearer ` (the empty-filter at lib.rs:202-208 applies only to the `x-api-key`/`x-goog-api-key` branch). So a router built from a default/empty config accepts an empty Bearer. In the shipped app this is unreachable — `start_gateway` always loads the token via `ensure_token`, and `TokenManager::create` always writes 43 bytes — but neither the gateway crate nor the app validates non-emptiness before starting (`state.rs:202-235` builds the config from whatever the vault returned; the vault contract explicitly permits empty secrets, store.rs:110-113).

**Recommendation.** Fail closed at the boundary: `GatewayHandle::start` (or `verify_token`) rejects an empty expected token; `ensure_token`/`gateway_config` validate the loaded token is exactly `TOKEN_B64_LEN` bytes before starting. This preserves the F-9 fail-closed invariant under vault corruption/tampering.

---

### SEC2-08 — Low — Exit hook panics if `AppState` was never managed

`apps/desktop/src-tauri/src/lib.rs:57-58` resolves `app_handle.state::<AppState>()` in the `RunEvent::Exit` closure. Tauri's `Manager::state` panics when the type is unmanaged (`tauri-2.12.5/src/lib.rs:754-766`). If `init_backend` fails before `app.manage(...)` (lib.rs:96) — data-dir, DB-open, or migration failure — app exit then panics instead of draining cleanly. Cosmetic in outcome (the process is exiting) but it is a crash on a fail path; use `try_state()` and skip the drain when absent. (Note: IPC commands are safe — `State` extraction for commands returns an error, not a panic — so the early-IPC race during startup is only transient "state not managed" errors, acceptable.)

---

### SEC2-09 — Low — Gateway settings lack floors: `concurrency=0`, `body_limit_bytes=0`, `timeout_ms=0` are accepted

`gateway_config` (state.rs:202-235) validates only the port. `concurrency: 0` → `Semaphore::new(0)` → every request 503; `body_limit_bytes: 0` → every bodied request 413; `timeout_ms: 0` → every request 408. A corrupted or hand-edited `app_settings` row silently bricks the gateway. F-7 added a floor for `retention_days` (`.max(1)`, state.rs:239-244); apply the same treatment here (≥1, and sane minimums/maximums for timeout/body limit).

---

### SEC2-10 — Low — Redaction field-name layer matches exact names only

`SENSITIVE_FIELDS` (telemetry.rs:28-35) is `secret | token | authorization | api_key | password | credential`, matched exactly (`is_sensitive`, telemetry.rs:66-68). Fields such as `gateway_token`, `access_token`, `refresh_token`, `x_api_key`, `bearer`, or `secret_value` would pass through unredacted. This layer is documented defense-in-depth only (the primary rule — values never enter spans — holds today: grep confirms no token/secret material is logged anywhere, and the sweep tests cover the vault/reveal path), but substring-based matching (name contains `token`/`secret`/`key`/`password`/`authorization`) would make the second wall materially stronger at trivial cost.

---

### SEC2-11 — Informational — Middleware-order doc comment misstates the body-limit position

`crates/sair-gateway/src/lib.rs:4-11` claims request order "1. Concurrency guard … 5. Body size limit". Verified against axum 0.8.9 semantics (`axum-0.8.9/src/docs/middleware.md:85-101`: each newly added layer wraps all previous; the last `.layer` runs first), the actual request order is: DefaultBodyLimit (outermost) → concurrency guard → timeout → host allowlist → token auth. The difference is immaterial to security (arguably better: 413 is returned before a concurrency permit is acquired), but the comment should match reality so future edits don't rely on it.

### SEC2-12 — Informational — Bearer scheme matching is case-limited

`extract_token` (lib.rs:190-209) accepts only `"Bearer "`/`"bearer "` prefixes. RFC 6750 schemes are case-insensitive (`BEARER <token>` → 401). Fails closed — a compatibility nit, not a hole.

### SEC2-13 — Informational — Assorted hardening notes

- `reveal_token` calls `ensure_token`, so revealing can *create* a token as a side effect when none exists (state.rs:146-149) — surprising but fail-closed.
- `ensure_token` create-path races: two concurrent calls can both create vault items; the loser's item is orphaned (harmless — no ref points to it; consistent with the F-10 rationale, worth a debug assertion or single-flight).
- `concurrency_guard` releases the permit when the response *head* returns (lib.rs:127-131); when Phase 7 streaming lands, long-lived SSE bodies will run without a permit — revisit then.
- CSP allows `style-src 'unsafe-inline'` (tauri.conf.json) — typical for Tailwind, still the weakest line of the otherwise restrictive CSP (`default-src 'self'; script-src 'self'` is good).
- `OsSecretStore::availability()` (os.rs:40-45) is never called by the app; the degraded-start decision relies solely on error paths. Either wire it into startup diagnostics or remove it.
- `random_hex` panics on CSPRNG failure (`os.rs:72-78`, `.expect("OS CSPRNG must not fail…")`) — documented as unrecoverable, but an error return would avoid a process abort during credential writes.
- Retention timestamps are lexicographic ISO-8601; `now_iso()`/`purge_request_records_before` both use `to_rfc3339()` (`+00:00` form) consistently — keep future writers on `time::now_iso` so comparisons never mix `Z`/`+00:00` forms within a comparable range.
- IPv6 allowlist nit: ORCHID (`2001:10::/28`, `2001:20::/28`) and decommissioned `3ffe::/16` fall inside the `2000::/3` allow window, and `192.88.99.0/24` (deprecated 6to4 relay anycast) is not blocked — all non-routable in practice; no action needed, noted for the Phase 11 denylist re-review.

---

## Prior plan-review findings (F-1…F-12) — incorporation check

| ID | Claimed resolution | Actually in code? |
|---|---|---|
| F-1 (High) | Timeout layer + immediate 503 at capacity | **Yes**, with gaps: 503 (`concurrency_guard`, lib.rs:121-139, tested) and 408 timeout (lib.rs:90-93, tested) present; the promised **slow-headers negative test is absent** and the header phase is outside any timeout (→ SEC2-03/SEC2-05) |
| F-2 (Medium) | healthz behind auth; exact case-insensitive host match; missing Host → 403; `[::1]` dropped; negative tests | **Yes** — tests prove spoofed suffix/missing Host/404-behind-auth/HEAD; `[::1]` absent from allowlist; **chunked-body test absent** (→ SEC2-05) |
| F-3 (High) | Validating resolver + custom redirects + scheme policy + extended denylist + injectable mock resolver | **Partially** — resolver, redirect policy, denylist, and mock `AddressSource` all present and tested (http.rs); **but literal-IP URLs bypass the resolver entirely via hyper-util, and `validate_url` has no production caller** (→ SEC2-01). The mechanism as documented in ADR-015 is incomplete |
| F-4 (Medium) | Charset-safe refs `sair.cred.<hex>` / `sair.gateway-token`, pinned tests | **Yes** (refs.rs + tests; SecretRef::parse charset enforced; os.rs enforces canonical prefix) |
| F-5 (Medium) | ring 0.17 direct dep; 43-char token; length check before constant-time compare | **Yes** (workspace Cargo.toml `ring = "0.17"`; `TOKEN_B64_LEN`; token.rs:33-41; tests) |
| F-6 (Medium) | request_records selection refs + denormalized labels; FK matrix with per-rule probes; extra indexes | **Yes** (migrations/0001_init.sql:130-151; per-rule FK tests in lib.rs:282-391) |
| F-7 (Medium) | Startup purge + daily task + retention floor 1 | **Yes** (app lib.rs:87-94; state.rs:239-257; `.max(1)`) |
| F-8 (Medium) | macOS vault tests run in CI as evidence (ADR-016) | **Yes** — cfg gates permit execution on both platforms; `cargo test --workspace` includes the app crate, which enables the `os` feature (feature unification), so the contract runs on windows-latest/macos-latest; locally confirmed passing on Windows |
| F-9 (Medium) | FR-017/FR-018 tests; exit hook named; gateway fails closed on token failure | **Mostly** — drain test, occupied-port/no-fallback test, fail-closed start all present and passing; **hook is `RunEvent::Exit`, not the named `ExitRequested`** (→ SEC2-05/SEC2-08) |
| F-10 (Low) | Regenerate order create→swap→delete; release compile guard; sweep to vault+reveal | **Yes** (state.rs:116-139; sair-vault lib.rs:35-36 `compile_error!`; telemetry.rs:299-330) — but the deeper issue the order was meant to prevent (a token the gateway doesn't know about) exists anyway at the gateway-runtime level (→ SEC2-02) |
| F-11 (Low) | Log dir/rotation/prune specified; **file-writer sweep test added** | **Partially** — dir, daily rotation, 14-day prune all implemented (telemetry.rs:107-150, 203-241); **file-writer sweep test absent** (→ SEC2-05) |
| F-12 (Low) | cargo audit in gate + threat-model addendum | **Yes** — `audits/security/phase2-dependency-audit.md` (0 vulns, 5 informational Linux-only warnings) and docs/06:24-30 addendum present; CI-integrated `cargo audit`/`cargo-deny` deferred to Phase 11 (documented in ci.yml comment) |

---

## Verified sound (no action)

- **Auth surface:** no unauthenticated route exists — `/healthz` and the 404 fallback both sit behind token auth (tests: `healthz_requires_auth`, `fallback_is_behind_auth`); HEAD and method-not-allowed paths cross the same middleware; host-allowlist precedes auth so spoofed-Host probing can't even reach token comparison. Token is 32 ring-CSPRNG bytes, base64url(43), compared constant-time (`subtle::ct_eq`) after a documented length-only early-out; no token material appears in any log statement (grep + sweep test).
- **Host allowlist edge cases:** exact-match with optional port, case-insensitive; trailing dot, suffix/prefix spoofs, IPv6 literals (with/without port), empty port all handled or rejected; multiple Host headers rejected by hyper; absolute-form request targets cannot bypass the Host-header check in any scenario that matters (browsers send origin-form; crafted clients still need the token). v4-only bind (`handle.rs:58`) matches the dropped `[::1]` allowlist entry (F-2).
- **Concurrency/limits:** `try_acquire_owned` gives immediate observable 503 (no unbounded queueing, F-1); body limit 32 MiB default → 413 (tested); drain via `with_graceful_shutdown` with bounded grace and abort (tested).
- **Vault:** OS store live-verified on this Windows machine (contract 6/6 with `--features os`); refs are charset-safe and canonical-prefix-enforced; mock store release-blocked by `compile_error!`; service namespacing isolates test entries; `delete` treats `NotFound` as idempotent success.
- **Persistence:** static, fully-bound SQL only (ADR-004 honored); WAL + `foreign_keys=ON` (tested); FK delete-rule matrix exactly as resolved in F-6 with per-rule probe tests; `request_records` metadata-only (no bodies, ADR-008); retention delete query tested; purge runs at startup and daily; retention floor 1.
- **Startup/exit:** fail-closed ordering (DB → migrations → purge → token → bind) holds; migration failure aborts before bind; bind failure surfaces `GatewayError::Bind` with no fallback port (tested); ring provider installed before any client exists.
- **SSRF denylist itself** (for hostname URLs): default-deny v6 via a `2000::/3` allowlist window (NAT64/6to4/Teredo/documentation/ULA/link-local/multicast all blocked, v4-mapped delegated to v4 rules); the v4 denylist covers loopback/private/link-local/CGNAT/0-net/benchmarking/TEST-NETs/multicast/reserved; loopback opt-in does not extend to other private ranges (tested); redirects never cross host or scheme, ≤3 hops, each hop re-resolves through the validating resolver, so auth headers can never cross origins.
- **Tauri hardening:** capabilities `core:default` only (no shell/fs/http plugins); restrictive CSP with no remote origins; token masked by default, held only in React state, never persisted in webview storage, never logged; regeneration consent-gated with impact warning (UI text itself is wrong — SEC2-02); log files under per-user app-data with daily rotation and 14-day startup prune.

---

## Verdict

The phase delivers real, tested security value — the auth/Host/limit stack, the vault lifecycle, the schema, and the redaction pipeline are sound and evidenced, and the keyring/OS-contract claims are empirically confirmed. However, the two High findings are genuine defects in committed security properties: the SSRF layer's core mechanism does not cover literal-IP destinations (and its scheme policy is unwired), and token rotation silently fails to revoke the old token on a running gateway while the UI tells the user it did. Per `.agent/QUALITY_GATES.md`, Critical and High block shipping.

**Must fix before the phase gate closes / before Phase 3 wiring begins:**
1. **SEC2-01 (High)** — enforce SSRF policy for literal-IP URLs and wire scheme validation into the request path; add a through-reqwest integration test; amend ADR-015. Hard blocker for P3-T02 (first adapter/custom-URL usage).
2. **SEC2-02 (High)** — make token regeneration effective on the running gateway (restart or hot-swap) and fix the UI warning text.
3. **SEC2-03 (Medium)** — install a hyper Timer / header-read timeout and add the missing slow-headers negative test (promised by F-1).
4. **SEC2-04 (Medium)** — surface degraded-start failures in the UI error state (promised by the threat-model addendum).

**VERDICT: PASS WITH FINDINGS** (first review — superseded by the retest verdict at the end of this file)

Open High items at first review: SEC2-01 (SSRF literal-IP bypass + unwired scheme policy), SEC2-02 (token regeneration not applied to the running gateway). Both were retested against commit `8886aa9` and are FIXED — see the Retest section below for the per-finding disposition and the final verdict.

---

## Retest (2026-10-31) — commit `8886aa9` ("fix(security): resolve SEC2-01..09 from independent phase-2 review")

Reviewer: same independent reviewer. Method: read every changed file in full (`crates/sair-providers/src/http.rs`, `crates/sair-gateway/src/{lib,handle}.rs`, `apps/desktop/src-tauri/src/{state,lib,telemetry}.rs`, `.github/workflows/ci.yml`, manifests), then re-ran the suite on this Windows machine: `cargo test --workspace` → **61 passed / 0 failed** (gateway 21, providers 10, vault 8 — including the live OS Credential Manager contract, persistence 13, domain 5, app 4), `cargo clippy --workspace --all-targets -- -D warnings` → clean. The five new security tests were confirmed executing by name (listed per finding below). Files were not modified except this report.

### Per-finding disposition

**SEC2-01 (High, SSRF) — FIXED.**
`build_client`/`build_client_with_timeout` now return an opaque `SsrfCheckedClient` (http.rs:243-319) whose only request entry point, `request(method, url)`, runs `SsrfPolicy::validate_url` (scheme) **and** an IP-literal `is_allowed` check per call (http.rs:305-318) before handing out a `RequestBuilder`. The inner `reqwest::Client` field is private and there is no raw-client accessor, so the hyper-util literal-IP resolver bypass (the original finding) can no longer be reached through this crate's public surface: literal and scheme enforcement now happens at the request boundary where it always executes, exactly as recommended. The hostname path retains the `ValidatingResolver` per-resolved-address check, and the redirect policy (no cross-host/scheme, ≤3 hops) is unchanged, so redirect hops stay covered (same-host hops reuse the already-validated host; cross-host hops are stopped).
New test `checked_client_blocks_literal_and_scheme_violations` (http.rs:487-548) exercises the real `build_client` boundary: blocks `https://169.254.169.254/...` and `https://10.0.0.5/...` with `AddressBlocked`, blocks `http://api.example.com` with `SchemeBlocked`, admits public literal + public hostname, and scopes the loopback opt-in correctly (`http://127.0.0.1` allowed, `http://10.0.0.1` blocked). Passes. ADR-015's claim is now true as implemented — the resolver covers hostnames and the wrapper covers everything.
*Watch item for Phase 3 (not a defect):* the sanctioned boundary is by-construction in this crate, but `reqwest` remains a direct dependency, so adapters must be reviewed at P3-T02 to receive only `SsrfCheckedClient` and never construct their own client. Record that rule in ADR-015/docs/04 when adapters land.

**SEC2-02 (High, token lifecycle) — FIXED.**
`GatewayRuntimeState.expected_token` is now `Arc<tokio::sync::RwLock<Arc<Vec<u8>>>>` (gateway lib.rs:54-58); `token_auth` reads the current value per request (lib.rs:190); `GatewayHandle` holds the same Arc and `update_token` swaps it live (handle.rs:166-169). `regenerate_token` (state.rs:117-145) now: create new vault item → persist ref → **hot-swap the running gateway** → delete old item — the swap happens before the old vault item is deleted, so a crash between steps never leaves a tokenless gateway, and the old token is revoked for every subsequent request without a restart. Test `update_token_hot_swaps_without_restart` (handle.rs:376-412) proves old token → 401 and new token → 200 after the swap on a live server. The UI warning text ("existing clients will stop working") is now accurate.
*Minor residual (Low):* if the DB `set_setting` inside `regenerate_token` fails, the in-memory `token_ref` has already been swapped while the DB keeps the old ref; the command returns Err (visible to the UI), the gateway keeps the old token (which the DB ref still names — consistent after restart or a successful retry), but `reveal_token` would return the new token in that window. Consider rolling back the in-memory swap on DB failure.

**SEC2-03 (Medium, slowloris) — FIXED.**
The gateway no longer uses `axum::serve`; it runs a manual accept loop with `hyper_util::server::conn::auto::Builder` and per connection `.http1().timer(TokioTimer::new()).header_read_timeout(header_timeout)` (handle.rs:98-127). `GatewayConfig.header_timeout_ms` defaults to 10 s, is floored at use (`handle.rs:95`) and at settings load (≥1000 ms, state.rs:244-249) — the Timer requirement identified in the original finding is satisfied, so the header-read timeout actually takes effect. The tower `TimeoutLayer` continues to bound post-header processing (408, tested), so both phases of a request are now budgeted. Test `slowloris_partial_headers_are_dropped` (handle.rs:429-458) opens a raw TCP connection, sends half a request line, and asserts the server closes/408s within 5 s at a 500 ms header timeout — a genuine through-hyper negative test, and it passes. Verified empirically: the full suite including graceful-drain tests remains green with the new server loop.

**SEC2-04 (Medium, degraded-start transparency) — PARTIAL (residual now Low).**
The token/vault failure path — the primary degraded-start scenario in the docs/06 addendum — is fixed: `init_backend` calls `state.set_gateway_error(...)` (app lib.rs:105-109, state.rs:254-260), so the UI shows an explicit error instead of a bare "Stopped", and `start_gateway` failures continue to set it (state.rs:174-177).
*Remaining gap:* DB-open and migration failures (app lib.rs:73-86) still return Err **before** `AppState` is managed, so there is nothing to record the error in — those failures remain log-only. Narrower than the original finding (the failure also disables all persistence), but the addendum's "vault or DB unavailable at startup → explicit error state surfaced in the UI" is only half-true. Fix by managing an `AppState` (with an optional/errored DB) before the DB step, or by recording the init failure in a separate always-managed state object. Escalated as a Low item for the Phase 3/9 UI work; not blocking.

**SEC2-05 (Medium, resolution gaps) — FIXED.**
File-writer sweep: `file_sink_contains_no_secret_material` (telemetry.rs) now writes through a real file sink and reads the file back from disk, asserting no seeded key material and correct redaction — closes the F-11 gap. Chunked oversized body: `chunked_oversized_body_rejected` (handle.rs:463-489) sends a raw chunked body over the limit on a raw socket and asserts 413 — closes the F-2 gap. Both pass.

**SEC2-06 (Low, CI least privilege) — FIXED.** `.github/workflows/ci.yml` now carries top-level `permissions: contents: read`.

**SEC2-07 (Low, empty token) — FIXED.** `GatewayHandle::start` rejects an empty token with `GatewayError::Token` before binding (handle.rs:69-74, fail-closed at the boundary — the verified_token empty==empty path is now unreachable for a running gateway), `update_token` asserts non-empty, and app-side `gateway_config` builds only from vault-loaded tokens. Test `empty_token_is_rejected_fail_closed` passes.

**SEC2-08 (Low, exit panic) — FIXED.** The `RunEvent::Exit` hook uses `app_handle.try_state::<AppState>()` (app lib.rs:60-66); no panic when backend init never managed the state.

**SEC2-09 (Low, settings floors) — FIXED.** `gateway_config` now rejects `body_limit_bytes`/`concurrency` < 1 and floors `timeout_ms`/`header_timeout_ms` at 1000 ms (state.rs:225-249); port validation unchanged. Zero-value brick configs are no longer possible.

**SEC2-10/11/12/13 (Low/Informational) — unchanged by this commit** (redaction substring matching, doc-comment ordering note, Bearer case-sensitivity, misc hardening notes). These remain as recorded; none blocks.

### F-9 drain-caller justification — ACCEPTED ON ITS MERITS

The implementation keeps `RunEvent::Exit` instead of the review's suggested `RunEvent::ExitRequested`. The submitted justification is sound: `ExitRequested` fires when exit is *requested* and can be declined/prevented (windows may veto close); draining there could stop the gateway while the application keeps running — tearing down a live service contrary to user intent. `RunEvent::Exit` fires at actual loop termination, cannot be vetoed, and is the correct last-chance drain point. The substantive F-9 requirement ("name the Tauri exit hook that triggers drain") is met, and the deviation is now an improvement over the original resolution text. Recorded residual (inherent to ADR-003's in-process gateway, not new): hard kill or process crash bypasses any drain hook on either event.

### Retest verdict

Both High findings are fixed with regression tests through the real boundary (reqwest client wrapper; live-server token swap). All committed Medium/Low fixes verified in source and by test execution on Windows; the only open item is the narrowed SEC2-04 residual (Low, escalated). No Critical or High findings remain open.

**VERDICT: PASS WITH FINDINGS** — open items are Low only (SEC2-04 partial: DB/migration degraded-start errors still log-only; SEC2-02 DB-failure ref divergence residual) plus Phase 3 watch items (adapter client discipline per SEC2-01; ADR-015 amendment to record the wrapper boundary). Nothing blocks the phase-2 gate.
