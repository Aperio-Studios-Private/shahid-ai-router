# Phase 0 Research Notes — Stack, Toolchain, Provider APIs

Researcher: orchestrator agent. Access date for all URLs: **2026-10-30**. Facts are separated from assumptions per `.agent/RESEARCH_PROTOCOL.md`.

## 1. Local toolchain audit (verified on this machine)

| Tool | Status | Version / Evidence |
|---|---|---|
| Windows | Present | Windows 11, build 26300 (includes WebView2 Evergreen) |
| MSVC Build Tools | Present | VS 2019 + VS 2022 BuildTools with `VC.Tools.x86.x64` (verified via `vswhere`) |
| Rust | **Was missing; installed this session** | rustup → `stable-x86_64-pc-windows-msvc`, rustc/cargo **1.99.1** |
| Node.js | Present | v26.3.1, npm 11.16.0, pnpm 12.4.2 |
| git | Present | Git for Windows (`C:\Program Files\Git`) |

Assumption: WebView2 runtime present on this machine (standard on Win 11 26300). Will be verified by the first Tauri debug run.

## 2. Rust crates — current stable versions (crates.io API, 2026-10-30)

| Crate | Max stable | Note for plan |
|---|---|---|
| tauri | 2.12.5 | 2.x is current stable; 3.0.0-alpha exists but is pre-release → use 2.x |
| axum | 0.8.9 | 0.8 semantics: `{param}` path syntax, no `#[async_trait]` needed |
| sqlx | 0.9.0 | See breaking changes below |
| keyring | 4.2.0 | Facade over `keyring-core`; platform stores are separate crates |
| reqwest | 0.13.5 | rustls now default TLS (aws-lc provider) |
| tokio | 1.54.1 | — |
| argon2 | 0.6.0 | RustCrypto, Argon2id supported, `zeroize` feature |

### SQLx 0.9 breaking changes that affect our design (from github.com/launchbadge/sqlx CHANGELOG, main branch)
- All `query*()` fns take `impl SqlSafeStr` — static strs OK; dynamic SQL must be wrapped in `AssertSqlSafe`.
- MSRV 1.94.0 (we have 1.99.1 — OK).
- `SqliteValue` is `!Sync` / `SqliteValueRef` is `!Send`.
- `no_tx` migration support for SQLite; `Migrator::with_migrations()`.
- `libsqlite3-sys` version range may bump in any release.
Impact: our persistence layer must not hold rows across awaits anyway (docs/02 requirement). Compile-time macros used with static SQL only. No dynamic SQL anywhere without `AssertSqlSafe` review.

### keyring 4.x (docs.rs/keyring, README)
- v4 is a facade on `keyring-core`; per-platform credential stores are separate crates: `windows-native-keyring-store` (Windows Credential Manager), `apple-native-keyring-store` (macOS Keychain), `linux-keyutils-keyring-store`, etc.
- Maintainers' recommendation for apps wanting fine-grained control: depend on `keyring-core` + the specific store crates directly.
- v1 compat API exists via `v1` feature.
Impact: credential vault abstraction will wrap `keyring-core` + the two platform store crates (Windows/macOS), keeping our own `SecretStore` trait so fallbacks/tests use an in-memory mock. No plaintext fallback in production builds (docs/06 rule).

### reqwest 0.13 (GitHub releases)
- rustls default (aws-lc); `rustls-tls` feature renamed `rustls`; rustls-platform-verifier default roots.
- Redirect policy: no breaking change; 0.13.4 strips sensitive headers on scheme change across redirects (good for our SSRF posture, but we still enforce our own redirect policy).
- Streaming response bodies: unchanged (bytes_stream / chunked).

## 3. Frontend stack — current stable (npm registry, 2026-10-30)

| Package | Version |
|---|---|
| @tauri-apps/cli / api | 2.12.4 |
| react / react-dom | 19.3.1 |
| vite | 8.3.5 |
| typescript | 7.0.2 |
| tailwindcss | 4.3.3 |
| vitest | 5.0.5 |
| @playwright/test | 1.64.1 |

Risk note: TypeScript 7 is the new compiler line; most tooling supports it, but if ESLint/parser or other tooling lags, fall back to the 5.9.x line and record in DECISION_LOG. Decision at scaffold time; default to latest stable that passes the full toolchain.

## 4. Provider API facts (official docs, 2026-10-30)

### OpenRouter — VERIFIED (openrouter.ai/docs/api-reference/overview)
- `POST https://openrouter.ai/api/v1/chat/completions`, OpenAI-compatible schema.
- Auth: `Authorization: Bearer <key>`.
- Optional attribution headers: `HTTP-Referer`, `X-OpenRouter-Title` (also accepts `X-Title`), `X-OpenRouter-Categories`.
- Streaming: `stream: true` → SSE; may include comment payloads to ignore; usage arrives in final chunk before `[DONE]` with non-empty choices array (differs from OpenAI).

### Anthropic — VERIFIED (docs.anthropic.com → platform.claude.com/docs/en/api/messages, 301 redirect observed)
- `POST https://api.anthropic.com/v1/messages`.
- Required body: `model`, `max_tokens`, `messages` (role user/assistant, content string or blocks).
- `stream: true` → SSE raw event stream.
- Auth header `x-api-key` (documented in the API headers reference; the messages page itself only surfaced optional headers) + pinned `anthropic-version` (value `2023-06-01` remains the pinned stable version). Status: **fact, high confidence; exact header section re-verified at Phase 6 implementation.**

### Google Gemini — PARTIALLY VERIFIED (ai.google.dev/gemini-api/docs/text-generation)
- Base `https://generativelanguage.googleapis.com/v1beta/`, auth header `x-goog-api-key`.
- Docs now feature a new **Interactions API** (`POST /v1beta/interactions`, `?alt=sse` + `stream: true` for streaming).
- Classic `models/{model}:generateContent` / `:streamGenerateContent` surfaces still exist (long-stable; re-verify exact request schema at Phase 6). Status: base URL + auth header are facts; choice between Interactions vs generateContent is a Phase 6 decision.

### OpenAI — PARTIALLY VERIFIED (platform.openai.com blocks automated fetches: HTTP 403)
- Facts (long-stable, cross-corroborated): `POST https://api.openai.com/v1/chat/completions`, `Authorization: Bearer`, SSE `data:` frames terminated by `[DONE]`, `GET /v1/models`. Newer `/v1/responses` API exists. Status: re-verify against docs at Phase 3 implementation; risk low.

## 5. Candidate MCP servers / skills assessment (per RESEARCH_PROTOCOL)
- No MCP server is needed for Phase 0–1. Cargo, npm, rustup, and WebFetch/WebSearch cover verification needs. Installing extra tools would add provenance risk without value → **not installed**.
- The `graphify` skill (knowledge graphs) is available but not relevant to correctness of this work.

## 6. Open questions for the plan review
1. SQLx 0.9 vs pinning 0.8: 0.9 is current stable and MSRV-compatible; accept 0.9 with `AssertSqlSafe` discipline? (Recommendation: yes, 0.9.)
2. TypeScript 7.0.2 vs 5.9.x fallback.
3. keyring-core direct vs keyring 4 facade (recommendation: keyring 4 with explicit platform store features — less API surface churn, maintained facade).
