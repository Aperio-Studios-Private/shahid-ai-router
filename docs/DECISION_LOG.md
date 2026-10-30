# Architecture Decision Log

For each material decision record: ID/title, status, date, context, options, source evidence, decision, consequences, security/performance implications and revisit trigger.

## ADR-001: Desktop shell and frontend — Tauri 2 + React 19 + Vite + TypeScript + Tailwind 4
- Status: Accepted. Date: 2026-10-30.
- Context: docs/03 recommendation; cross-platform desktop with web UI, small binaries, OS integration.
- Evidence: crates.io tauri 2.12.5 stable; npm @tauri-apps/cli 2.12.4, react 19.3.1, vite 8.3.5, tailwindcss 4.3.3 (accessed 2026-10-30; see audits/planning/phase0-research.md).
- Decision: Tauri 2.x shell, React 19 + Vite 8 + TypeScript (7.0.2 preferred; 5.9.x fallback if tooling lags — recheck at scaffold) + Tailwind 4.
- Consequences: single process with webview UI; gateway embedded in-process.
- Revisit trigger: Tauri 3 stable release; TS7 tooling incompatibility.

## ADR-002: Backend — Rust workspace, Tokio, Axum 0.8
- Status: Accepted. Date: 2026-10-30.
- Context: concurrent local gateway + provider clients with memory safety.
- Evidence: crates.io axum 0.8.9, tokio 1.54.1.
- Decision: modular monolith as a 6-crate workspace (sair-domain, sair-persistence, sair-vault, sair-providers, sair-gateway, app) — see planning-audit §3; crates split only when a real boundary earns it.
- Consequences: Axum 0.8 `{param}` path syntax; router/state patterns per 0.8.
- Revisit trigger: sustained friction at a boundary.

## ADR-003: Gateway embedded in Tauri process; lifecycle explicit
- Status: Accepted. Date: 2026-10-30.
- Context: docs/03 warns Tauri does not guarantee background service behavior; FR-022.
- Decision: Axum server runs inside the Tauri app process on Tokio; gateway runs only while the app runs; UI states this; graceful drain on exit (FR-017). Tray/resident mode post-v1.
- Security: loopback bind only; no elevated privileges.
- Revisit trigger: user need for resident mode.

## ADR-004: Persistence — SQLite via SQLx 0.9, embedded migrations
- Status: Accepted. Date: 2026-10-30.
- Evidence: crates.io sqlx 0.9.0; MSRV 1.94.0 ≤ rustc 1.99.1; SQLx 0.9 changelog (SqlSafeStr; SQLite value !Sync; no_tx migrations).
- Decision: SQLx 0.9 + SQLite (WAL, foreign_keys ON); migrations embedded via `sqlx::migrate!`; static SQL only; no transaction spans an upstream network await.
- Consequences: any dynamic SQL requires `AssertSqlSafe` + review (none planned).
- Revisit trigger: sqlx 0.9 regression affecting SQLite paths.

## ADR-005: Secrets — OS vault via keyring 4.x facade + platform store crates
- Status: Accepted. Date: 2026-10-30 (corrected same day per review finding R-4).
- Context: docs/06 mandates OS secure storage, no plaintext fallback.
- Evidence: docs.rs keyring 4.2.0 (facade over keyring-core; platform stores are separate crates: windows-native-keyring-store, apple-native-keyring-store; keyring 4.2 features are only default/v1/cli).
- Decision: depend on `keyring` (v1-compatible API) + `windows-native-keyring-store` + `apple-native-keyring-store` directly; all access behind our `SecretStore` trait; in-memory store only for tests (feature-gated out of release builds); exact construction API verified at Phase 2 implementation.
- Security: DB stores opaque `secret_ref`; vault item deleted on credential deletion.
- Revisit trigger: keyring 4.x API churn breaking the platform stores.

## ADR-006: HTTP client — reqwest 0.13, rustls with ring provider
- Status: Accepted. Date: 2026-10-30 (per review finding R-3).
- Evidence: reqwest 0.13 release notes (rustls default, aws-lc provider, `rustls-no-provider` feature); aws-lc-rs requires CMake+NASM on Windows (absent on the dev machine).
- Decision: reqwest 0.13 with `rustls-no-provider` + ring as the rustls crypto provider. Avoids CMake/NASM; ring is audited and MSVC-friendly.
- Security implications: manual redirect policy with per-hop SSRF re-validation and auth-header stripping on host change; certificate verification always on (rustls-platform-verifier roots).
- Revisit trigger: need for a provider ring does not support.

## ADR-007: Encrypted backup crypto — Argon2id + AES-256-GCM (RustCrypto)
- Status: Proposed (final parameters locked at Phase 10 after security review). Date: 2026-10-30.
- Evidence: crates.io argon2 0.6.0; docs/10 requires vetted KDF + AEAD.
- Decision: Argon2id (baseline m=64 MiB, t=3, p=1) deriving an AES-256-GCM key; versioned algorithm identifiers stored in the `.sair` manifest; lost passphrase = unrecoverable (explicit UI warning).
- Revisit trigger: Phase 10 security review; RustCrypto advisories.

## ADR-008: Observability — tracing with redaction by construction
- Status: Accepted. Date: 2026-10-30.
- Decision: `tracing` structured logs; secret values and prompt/response bodies never enter spans (enforced by types + redaction layer); request records are metadata-only (FR: docs/07); redaction sweep tests in CI.
- Revisit trigger: none planned.

## ADR-009: Routing scope — same-protocol routing only in v1
- Status: Accepted. Date: 2026-10-30 (per review finding R-1).
- Decision: a request on a protocol surface routes only to deployments whose upstream natively speaks that protocol (passthrough). Cross-protocol translation is out of scope v1; capability model carries `protocol_family` so aliases filter correctly.
- Consequences: honest capability matching; no semantic-loss translation in v1.
- Revisit trigger: user demand + verified translation design post-v1.

## ADR-010: Protocol surfaces (v1) and agent constraints
- Status: Accepted. Date: 2026-10-30 (per review finding R-2).
- Decision: OpenAI surface = `POST /v1/chat/completions`, `GET /v1/models`; Anthropic surface = `POST /v1/messages`, `POST /v1/messages/count_tokens` (proxied for Anthropic upstreams; structured 501 otherwise); Gemini surface = `models/{model}:generateContent` (+`:streamGenerateContent`). `/v1/responses` out of scope v1; Codex CLI profiles pin `wire_api="chat"` (verified Phase 8).
- Revisit trigger: verified agent requirements at Phase 8.

## ADR-011: Gateway request hardening
- Status: Accepted. Date: 2026-10-30 (per review findings R-5/R-6).
- Decision: middleware stack = token auth → Host allowlist (127.0.0.1/localhost + configured loopback literals; DNS-rebinding defense) → body size limit (32 MiB default) → concurrency limit → timeout budget. SSRF enforced at a custom connect layer re-checking every resolved address immediately before connect; denylist per planning-audit §11 R-6; exception only for explicit loopback literals. No CORS headers emitted.
- Revisit trigger: new client classes (LAN mode remains out of scope).

## ADR-012: Rotation-state concurrency mechanism
- Status: Accepted. Date: 2026-10-30 (per review finding R-7).
- Decision: `tokio::sync::Mutex<RotationState>` for cooldown/health/last-used with short critical sections (never across network/DB awaits); health events applied through an ordered path; Phase 4 tests cover concurrent rotation deterministically.
- Revisit trigger: measured contention on the state lock.

## ADR-013: CI — GitHub Actions matrix (windows-latest, macos-latest)
- Status: Accepted. Date: 2026-10-30.
- Context: dev machine is Windows-only; docs/12 requires cross-platform evidence.
- Decision: CI compiles and tests on Windows and macOS runners; macOS runtime/vault claims in reports are limited to CI evidence until a macOS machine verifies interactively.
- Revisit trigger: availability of a macOS verification machine.

## ADR-014: Minimum supported platforms
- Status: Accepted. Date: 2026-10-30 (per review finding R-9).
- Decision: Windows 10+ x64 (WebView2 Evergreen); macOS 12+ x86_64/aarch64. Exact Tauri 2 minimums verified at Phase 1 build; docs/12 amended accordingly.
- Revisit trigger: Tauri 2 minimum OS change.

## ADR-017: Retry/failover semantics v0 (Phase 3)
- Status: Accepted. Date: 2026-10-31 (per Phase 3 review findings R3-01/R3-02).
- Decision: (1) Classification is body-aware on 4xx — 429 with `insufficient_quota` type/code is BillingOrQuota (rotation stops; shared-account keys are not burned), plain 429 is RateLimited; 404 disambiguates model availability; ContextLimit matched before generic InvalidRequest; malformed/oversized 200 bodies are classified errors, never passthrough. (2) Retry-After (seconds) → credential cooldown + immediate rotation; no in-request sleeping in v0 (no storms, bounded latency); no same-credential retry → backoff/jitter not applicable v0. (3) `timeout_ms` is per-attempt (request-builder override); `retry_budget_ms` is the wall-clock budget across attempts; policy rows are always written explicitly (schema defaults not relied on). (4) Timeout does NOT rotate: generation may have executed upstream (docs/05 ambiguous-timeout rule) — surface to client; rotation set = RateLimited/ProviderUnavailable/TransportFailure only.
- Consequences: quota-exhaustion storms and blind retries are structurally prevented; timeout latency is paid once per request.
- Revisit trigger: provider idempotency support enabling safe timeout retries (docs/05).

## ADR-018: Candidate composition and strategy scope (Phase 3)
- Status: Accepted. Date: 2026-10-31 (per review finding R3-03/R3-04).
- Decision: routing candidates are (alias-deployment, credential) pairs ordered by lexicographic priority (alias_deployments.priority, credentials.priority), weights from credential.weight; strategies priority/round_robin/weighted implemented; lru/manual_pin declared but rejected with structured 501 until implemented (honest incompleteness). Rotation state (per-alias round-robin counters, attempt bookkeeping) lives behind `tokio::sync::Mutex<RotationState>` with critical sections that never span awaits (ADR-012), verified by a deterministic 20-task concurrent test in Phase 3.
- Revisit trigger: user need for lru/pin strategies.

## ADR-015: SSRF enforcement via validating custom resolver
- Status: Accepted. Date: 2026-10-31 (per Phase 2 review finding F-3; supersedes the connector-rewrite wording in ADR-011).
- Context: reqwest 0.13 has no full connector replacement; rewriting the connect target to a validated IP breaks TLS SNI/certificate identity, and weakening verification is forbidden (SECURITY_RULES #6).
- Decision: enforce SSRF policy in a custom `Resolve` implementation so resolution and validation happen together and only validated addresses are returned (reqwest connects exactly to resolver output — TOCTOU window minimized while TLS identity stays on the hostname). Redirects governed by `redirect::Policy::custom` (stop on host/scheme change, ≤3 same-host hops). Scheme policy and extended denylist live in `SsrfPolicy` (multicast/reserved/NAT64/6to4 added; default-deny unclassified).
- Consequences: a residual resolve-to-connect race remains in the platform resolver path — documented, revalidated at Phase 11 security review. **Phase 3 rule (from security re-verification, 2026-10-31):** adapters must receive only `SsrfCheckedClient` (which enforces scheme + IP-literal policy per request — hyper-util short-circuits literals before any resolver); the raw reqwest client is never exposed. Enforced by construction at P3-T02.
- Revisit trigger: reqwest providing first-class validated-connect APIs.

## ADR-016: Vault contract-test evidence policy
- Status: Accepted. Date: 2026-10-31 (per Phase 2 review finding F-8).
- Decision: OS vault contract tests run on Windows locally and on CI (windows-latest Credential Manager, macos-latest Keychain); a green macOS CI run is accepted as macOS runtime evidence per ADR-013. If CI Keychain entitlement errors appear, the documented fallback is annotate + skip with the reason recorded in the phase report (never silently ignored). Reports state exactly which platform executed the vault contract tests. The owner's interactive macOS verification (scheduled 2026-11-01) remains the acceptance milestone for that platform.
- Revisit trigger: CI environment changes affecting keychain availability.
