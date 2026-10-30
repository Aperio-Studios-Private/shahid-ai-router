# Phase 8 Plan — Agent Profiles

Feature ID: PHASE-8. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-013 (agent profiles: generate copyable endpoint/model/auth configuration; external edits require consent and backup), docs/09 (agent integrations), ADR-010 (agent constraints: Codex CLI pins `wire_api="chat"`), review Phase-0 F-2 resolutions.

## Repository state
Phase 7 complete (a3b2c09): three surfaces incl. streaming, 123 tests green.

## Design (docs/09 workflow: select agent → show settings → generate copyable config → test → optional consent-based edit → rollback instructions)

### 1. Profile model (sair-domain + persistence)
`AgentProfile` records (docs/09 fields): agent id/version, protocol family, endpoint (gateway URL + surface path), auth method (token via the family's header), model alias + optional fast alias, env vars, config-file rules, verification date, limitations. Stored in `agent_profiles` (schema exists: id, agent_id, agent_version, protocol_family, endpoint, auth_method, model_alias FK, env_json, config_rules_json, verified_at).

### 2. Profile generators (pure functions, sair-domain or gateway — decided: sair-domain `profiles.rs`, testable without IO)
For each supported agent, generate a COPYABLE config from (gateway endpoint, token REFERENCE — never the token itself unless the UI explicitly reveals it, alias):
- **OpenAI Codex CLI** (verified format 2026-10; `~/.codex/config.toml`): model_provider block with `base_url = "http://127.0.0.1:{port}/v1"`, `wire_api = "chat"` (ADR-010 — `/v1/responses` out of scope), env var `OPENAI_API_KEY` = gateway token (placeholder `<<GATEWAY_TOKEN>>` in copyable output; UI substitutes on reveal), model = alias.
- **Claude Code** (`~/.claude/settings.json`): `env: { ANTHROPIC_BASE_URL: "http://127.0.0.1:{port}", ANTHROPIC_AUTH_TOKEN: "<<GATEWAY_TOKEN>>", ANTHROPIC_MODEL: alias }`. Note: count_tokens IS proxied (Phase 6) so Claude Code works non-streaming; streaming supported (Phase 7).
- **Gemini CLI** (`~/.gemini/settings.json` + env `GEMINI_API_KEY`): base URL `http://127.0.0.1:{port}/v1beta`, model alias; query-param auth REJECTED by the gateway (F-9 Phase 6) — the CLI must use the header form; documented limitation.
- **Generic OpenAI-compatible** (curl example + OpenAI SDK base_url snippet).
Each generator returns the config text + the list of files it would touch + rollback instructions (restore from the auto-backup the edit flow creates).

### 3. IPC/UI
- `generate_agent_profile(agent_id, alias, port) -> {config_text, files: [...], rollback: String}` — no token material (placeholder).
- `apply_agent_profile(path, content)` — CONSENT-GATED: the UI shows a diff/backup prompt; backend creates `<path>.shahid-backup-<ts>` before overwriting; refuses paths outside a small allowlist (the four known config paths + explicit user-provided path with absolute normalization); size cap; never follows symlinks (docs/06 import rules apply to external edits).
- UI: Agent Integrations screen — agent selector, alias selector, generated config with copy button, "Apply to file…" with explicit confirm + backup path display, verification-date + limitation notes per docs/09 ("Support is not complete until end-to-end tested on the documented version" — profiles ship with `verified_at` = this phase's E2E evidence and limitations text).

### 4. Verification (docs/09: end-to-end test per documented version)
- E2E in tests: for each generator, the produced config (with the placeholder substituted) is EXECUTED against the gateway: Codex-style TOML parsed + base_url used by a generic OpenAI client; Claude Code env shape used by a generic Anthropic client; Gemini header form used by the Gemini adapter path. (The real CLIs cannot run in CI without installs — the verification is: config parses + gateway serves the documented contract; live CLI verification remains opt-in/manual, recorded as limitation.)
- `verified_at` set to the contract-test evidence date, limitations text states exactly what was tested (QUALITY_GATES: state what was tested).

## Tests
- Generator unit tests: TOML/JSON validity, placeholder substitution, no token material in output (sweep), rollback instructions present.
- apply_agent_profile: backup created, consent-less call rejected, symlink/traversal rejected, size cap.
- E2E: generated configs → gateway happy paths per family (reuse Phase 6/7 mocks).

## Rollback
Additive; external file edits are always backed up before write (tested).

## Acceptance
All tests + suites green; report + traceability (FR-013) + push.

## Review resolution (2026-10-31) — verdict: APPROVED WITH REQUIRED REVISIONS

Report: `audits/planning/phase8-plan-review.md` (external verifications with sources: Codex `wire_api="chat"` REMOVED ~Feb 2026 — hard config error; Claude Code env block verified; Gemini CLI env-var mechanism verified).

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | **Codex CLI dropped from v1 generators** — `wire_api="chat"` is a hard config error in current Codex (removed variant), and `responses` is out of scope (FR-023). Codex ships as documented-UNSUPPORTED with limitation text ("requires /v1/responses, planned post-v1"); ADR-010 amended. |
| F-2 | High | apply_agent_profile hardened: closed allowlist (the shipped config paths only, resolved under the OS home dir), canonicalization + symlink/junction rejection on the target and parents, two-step consent with expected-content-hash precondition (TOCTOU), atomic write (temp+rename), size cap; threat-model addendum entry. |
| F-3 | Medium | Secret sweep extended to apply/backup paths (no token in logs/reports); generated files ALWAYS use the placeholder (enforced by generator design — apply rejects content without the placeholder); backups carry the previous user file (may contain user's own keys) — retention note: backups are user-managed, one per apply, path surfaced. |
| F-4 | Medium | Gemini generator corrected: `GOOGLE_GEMINI_BASE_URL` = bare `http://127.0.0.1:{port}` (SDK appends v1beta); config lives in `~/.gemini/.env` env vars (no settings.json base-URL field); limitations = gemini-api-key auth mode + generateContent-only surface (header form verified — ?key= moot). |
| F-5 | Medium | UI language: "contract-tested on <date>" never "supported" (docs/09); generate_agent_profile validates alias existence + same-protocol deployments (structured error otherwise); dated-citation fixture tests for each generator format. |
| F-6 | Low | Claude Code fast alias → `ANTHROPIC_DEFAULT_HAIKU_MODEL`; Windows home/UNC handled via home-dir allowlist join; limitations persisted in agent_profiles.notes. |

Status: READY FOR IMPLEMENTATION.
