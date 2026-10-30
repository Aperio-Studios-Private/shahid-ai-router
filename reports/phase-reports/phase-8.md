# Phase 8 Report — Agent Profiles

Feature ID: PHASE-8. Date: 2026-10-31. Status: **PASS** (local evidence; CI blocked per B-01).

## Requirements delivered
FR-013 (profile generation, copyable config, consent-gated external edits with backup, verification-date + limitations surfacing), docs/09 workflow steps 1–6 (select → settings → copyable config → [test = gateway contract] → optional consent-based edit → rollback instructions).

## Key findings from independent review (both High resolved)
1. **F-1 (High): Codex CLI dropped from v1.** External verification (openai/codex source) showed `wire_api="chat"` was REMOVED from current Codex — a hard config error — and `responses` (the only remaining value) requires `/v1/responses`, out of scope per FR-023/ADR-010. Codex ships as a documented-UNSUPPORTED profile stating exactly why; **ADR-010 amended**.
2. **F-2 (High): apply hardening.** The original "allowlist + user path" was an arbitrary-file-write primitive. Now: closed allowlist (Claude Code settings.json, Gemini .env — resolved under the OS home dir), canonicalization with symlink/junction rejection on target AND parents, two-step consent with a SHA-256 hash precondition (TOCTOU guard — the UI hashes the target, the backend refuses if it changed), atomic write (temp + rename), size cap, content validation (placeholder required; key-like material rejected).

## Delivered
- `sair-domain::profiles`: generators for **Claude Code** (env block incl. `ANTHROPIC_DEFAULT_HAIKU_MODEL` — F-6), **Gemini CLI** (bare `GOOGLE_GEMINI_BASE_URL` + `~/.gemini/.env` — F-4 corrected; header-auth limitation documented, `?key=` moot), **generic OpenAI-compatible**; Codex unsupported profile. Placeholder-first by construction (F-3): generated files never contain real tokens; apply content validation enforces it.
- **IPC**: `generate_agent_profile` (validates alias existence + same-protocol deployments — F-5), `hash_profile_target`, `apply_agent_profile` (consent-gated, backed-up, atomic).
- **UI**: Agent Integrations screen — agent selector, generated config with copy, apply-with-confirm flow showing backup path, verification language "contract-tested on <date>" (never "supported" — F-5), limitations verbatim, rollback instructions.

## Verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **132 passed, 0 failed** (profiles: JSON/TOML validity, placeholder enforcement, Codex-unsupported, no-token-material sweep; apply: backup + atomic write + stale-hash rejection)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; fmt clean; tsc clean; vitest 2/2; build ok

## Honest scope statements
- **No real CLI was executed** — profiles are contract-tested (config shape + gateway contract per family); docs/09 "supported" claims require interactive CLI verification (recorded in every profile's limitations text and the UI).
- Config formats drift: dated-citation fixture tests pin the formats; drift triggers regeneration (F-5).
- Backups are user-managed (one per apply, timestamped, path surfaced); they may contain the user's own pre-existing keys — disclosed in the consent dialog (F-3).
- CI remains blocked (B-01); macOS pending (ADR-016).

## Next eligible
Phase 9 (UI completion: logs/usage/health screens, a11y pass) — entry per workflow.
