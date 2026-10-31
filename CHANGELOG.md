# Changelog

All notable changes to Shahid AI Router. Format: Keep a Changelog; versioning: SemVer.

## [0.1.0] — 2026-10-30

### Added (Phase 0 — Discovery)
- Planning audit with PRD/architecture/stack review and finalized roadmap (`audits/planning/planning-audit.md`).
- Phase 0 research notes with verified stack versions and provider API facts (`audits/planning/phase0-research.md`).
- Independent plan review report and resolution of all findings (`audits/planning/plan-review-report.md`).
- Architecture Decision Records ADR-001…ADR-014 (`docs/DECISION_LOG.md`).
- PRD amendments FR-017…FR-023 (`docs/01_PRD.md`); traceability matrix populated (`docs/TRACEABILITY_MATRIX.md`); task boards seeded (`tasks/`).

### Added (Phase 1 — Skeleton)
- Rust workspace: `sair-domain` (error taxonomy, protocol families), `sair-persistence` (SQLite/WAL/foreign-keys via SQLx 0.9), `sair-vault` (SecretStore abstraction + test mock), `sair-providers` (adapter contract + capabilities + mock), `sair-gateway` (Axum `/healthz` skeleton).
- Tauri 2 desktop shell with least-privilege capabilities, locked-down CSP, `get_app_info` IPC command, ring rustls provider install.
- React 19 + Vite 8 + TypeScript 7 + Tailwind 4 frontend with docs/08 navigation skeleton, light/dark theme, vitest component tests.
- Placeholder icon set and generator script (`scripts/make-placeholder-icons.mjs`).
- GitHub Actions CI matrix (Windows/macOS): fmt, clippy `-D warnings`, cargo test, frontend typecheck/test/build.

### Added (Phase 2 — Security & persistence)
- Full docs/07 schema (migrations 0001/0002) with FK delete-rule matrix and probe tests; OS vault (keyring 4.x; Windows Credential Manager verified live) with test-mock release guard; gateway token lifecycle (32-byte CSPRNG, constant-time verify, crash-safe regeneration with live hot-swap); middleware stack (auth, Host allowlist, 503-at-capacity, 408 timeout, 413 body limit, slowloris header-read timeout); redacted telemetry with sweep tests and rotating file sink; SSRF policy + validating resolver + SsrfCheckedClient boundary (ADR-015).
- Independent security review pass with all High/Medium findings fixed and re-verified (audits/security/).

### Added (Phase 3 — First vertical slice)
- OpenAI-compatible `/v1/chat/completions` (non-streaming passthrough) and `/v1/models`; body-aware error classification (ADR-017: 429+insufficient_quota = billing, not rate-limit); routing engine v0 (priority/round-robin/weighted, cooldown, quarantine; lru/pin → structured 501) with deterministic concurrent-rotation tests; request records with full routing decisions (FR-012); management IPC + Providers/Aliases UI; secrets vault-backed, deleted with credentials.

### Added (Phase 4 — Rotation & health)
- Account-group cooldown for billing/quota failures (docs/05 quota truth; monotonic single-statement writes); per-deployment concurrency semaphores (FR-021; busy deployments rotate past without consuming attempts); Retry-After from HTTP header + body, capped 3600 s; per-request credential dedupe; health-event persistence fix + read-back tests; deterministic simulated failure suite.

### Added (Phase 5 — OpenRouter + capability matrix)
- OpenRouter attribution headers (documented `HTTP-Referer` + `X-Title`, dispatched only to openrouter providers); request-derived capability requirements (tools → tool_calling; response_format json_object/json_schema → structured_output) applied by the router with exclusion reasons in 503s; model capability editing.

### Added (Phase 6 — Native Anthropic + Gemini surfaces)
- Anthropic `/v1/messages` + `/v1/messages/count_tokens` (client anthropic-version forwarded, pinned legacy default); Gemini `models/{alias}:generateContent` (+ `:streamGenerateContent`); per-family error envelopes; same-protocol upstream status preservation; RESOURCE_EXHAUSTED defaults to billing/quota (with RetryInfo retryDelay parsing); DEADLINE_EXCEEDED/timeout_error never rotate; `?key=` query auth rejected.

### Added (Phase 7 — Streaming)
- SSE pass-through for all three surfaces via dedicated streaming clients (no total deadline — connect + idle-read bounds); stream-commit boundary structural (no failover after bytes flow); deployment permit held for the stream lifetime; streamed records flagged; 5 E2E streaming tests.

### Added (Phase 8 — Agent profiles)
- Profile generators for Claude Code, Gemini CLI, and generic OpenAI-compatible clients; OpenAI Codex CLI documented-UNSUPPORTED in v1 (ADR-010 amendment: upstream removed the chat wire API); consent-gated config application with closed allowlist, SHA-256 hash precondition (TOCTOU), symlink rejection, atomic writes, timestamped backups; Agent Integrations UI.

### Added (Phase 9 — UI completion)
- Request Logs (filters, correlation IDs, expandable routing-decision exclusions, retention footer), Usage aggregates (24h/7d), Health Monitor with human status explanations; no-route requests recorded; secret-safe server-side decision projection (vault refs never reach the webview); focus-visible + aria improvements.

### Added (Phase 10 — Backup/restore)
- `.sair` versioned container: config-only mode (credential metadata incomplete, no secret_ref; settings allowlist) and encrypted mode (Argon2id 64 MiB/t=3/p=1 + AES-256-GCM, metadata AAD); tx-scoped import with natural-key conflict skipping and backend vault re-store (no secrets through IPC); lost-passphrase warning; size cap + tamper/integrity rejection.

### Added (Phase 11 — Hardening)
- Deferred raw-TCP streaming scenarios (stalled upstream closed within configurable idle-read timeout; DrainTimedOut abort path); paired-delta p99 latency budget (< 25 ms advisory); in-flight counter relocated outside the tower stack (timeout-layer leak fix) with hyper-util graceful drain of idle keep-alive connections; CycloneDX SBOMs + release checksum archived; `npm audit` + `cargo audit` (0 vulnerabilities) in the pipeline.

### Known limitations (v1, all recorded in reports/phase-reports/)
- Streaming token counts are NULL (SSE frame parsing post-v1).
- OpenAI Codex CLI unsupported (requires `/v1/responses`); `/v1/responses` surface post-v1.
- Vision requirement detection and context_window enforcement deferred.
- Live opt-in tests against real providers/CLIs not executed (no external credentials in the build environment).
- CI execution pending GitHub Actions billing fix (tasks/BLOCKED.md B-01); macOS verification pending (ADR-016).
