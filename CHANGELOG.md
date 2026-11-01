# Changelog

All notable changes to Shahid AI Router. Format: Keep a Changelog; versioning: SemVer.

## [0.1.1] — 2026-11-01 (audit fixes + Phases 12-16)

Independent full-repository audit (every file, 11 reviewer agents); all P1/P2 findings fixed.
Evidence: `reports/phase-reports/audit-fixes.md`; details in `tasks/COMPLETED.md` (AF-01..AF-13).

### Fixed
- Management UI: add provider / add model+deployment / bind alias forms always failed (snake_case IPC args vs Tauri v2 camelCase contract; required `structuredOutput` never sent). All data-entry flows now work.
- Backup restore: config-mode imports could never commit (held slot was encrypted-only); every deployment was silently skipped on import (row-PK vs natural-name mismatch); alias policy links and bindings were never restored (export/import field mismatch); settings and agent_profiles exported but never imported; the import conflict report was a stub that always reported zero conflicts. Roundtrip regression test added.
- Backup crypto: attacker-supplied Argon2id parameters (memory/iterations) are now bounded before derive — a crafted 3 KB file can no longer exhaust RAM/CPU during preview.
- Adapters: response bodies are read incrementally with the 32 MiB cap enforced DURING the read (previously buffered fully first; count_tokens had no cap at all) — a hostile upstream can no longer exhaust memory.
- Gateway: vault failures on the Anthropic/Gemini surfaces now answer in the family's own envelope (was OpenAI-shaped); a busy deployment no longer poisons its shared credential for later deployments; a broken credential rotates on instead of aborting the alias; error envelopes surface the correlation id (`shahid.request_id`); no-route records record the real `streamed` flag.
- Gateway lifecycle: drain window now equals the caller's `stop(grace)` (was hardcoded 30 s); the in-flight counter uses an RAII guard so early future drops can no longer leak it upward; the concurrency permit is acquired only after token auth; removed a leftover debug `eprintln!` from the drain path.
- Streaming: real mid-stream-drop test (raw-TCP upstream, exactly one upstream hit, partial frames relayed, credential untouched) replacing a zero-assertion stub.
- Domain: `Timeout` is no longer "safely retryable" — the exported contract now matches ADR-017's timeout-never-rotates rule.
- SSRF resolver: DNS runs on the blocking pool (no longer stalls tokio workers); `localhost` by name must resolve to a loopback address to inherit the cleartext exception.
- Retry-After: HTTP-date form now parsed in all adapters; Gemini additionally reads the standard header; fractional Gemini `retryDelay` ("1.5s") rounds up instead of being dropped; Gemini model ids are percent-encoded in the URL path.
- Icons: placeholder PNGs regenerated as RGBA + `icon.icns` generated — non-Windows `tauri::generate_context!` builds were broken with the shipped icons.
- Telemetry: sensitive-field redaction list extended (secret_b64, refresh_token, access_token, bearer, passphrase, client_secret); profile verification date no longer hardcoded.
- Bookkeeping: ADR-007 → Accepted; PRD FR-023 Codex amendment note; docs/05 v1-strategy/backoff status notes; docs/08 Screens section reconciled with the Models/Deployments fold; ROADMAP completion status; HANDOFF corrections (23 FR rows, phase-0 evidence pointer); task boards repaired (literal `\n` artifacts removed; stale "current phase" headers fixed); LICENSE artifact fixed.

### Added (Phase 12 — Cloud auto-backup, ADR-019)
- Opt-in cloud sync of the `.sair` envelope to a user-owned free-tier backend: **Neon** (SQLx Postgres, `sslmode=require`) and **Supabase** (PostgREST) — the app's first internet-connected feature, transport-only over the Phase 10 crypto.
- Auto schedule: daily + 5-min-quiet on-change debounce (60 s tick, spawn-after-manage per the plan review); failures surface as status + Logs entries, never block the gateway.
- Encrypted (default; passphrase held in the OS vault, never uploaded) and config-only modes; snapshot list across devices; prune to last 10 per device; restore reuses the unchanged consent-gated preview/commit flow via a temp `.sair` file; disable with optional cloud purge.
- 8 new IPC commands (`cloud_configure/test/status/list/backup_now/restore/commit/disable`), a Cloud section in the Backup & Restore screen, DSN-free error mapping (connection strings can never reach a log), and a tested invariant that cloud settings keys never join the export allowlist.
- Docs: ADR-019, PRD FR-024, docs/06 network-allowlist + threat-model addendum, docs/07 note.

### Added
- UI: Backup & Restore screen (export/preview/commit) and About screen; Models/Deployments nav entries now route to Providers per the docs/08 amendment; provider type options for Anthropic/Gemini; NVIDIA NIM documented as a supported OpenAI-compatible target (README, docs/04).
- CI: `security` job (cargo-audit + npm audit) — the phase-11 evidence described an audit job the workflow never contained.
- Tests: backup export→restore roundtrip, KDF-bound, real mid-stream-drop (142 cargo tests total, up from 139).

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
