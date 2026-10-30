# Backlog

Task format per `.agent/TASK_EXECUTION_PROTOCOL.md`. IDs: `P<phase>-T<nn>`. Phase entry/exit follows `docs/13_ROADMAP.md` + planning audit §7. Tasks for future phases are decomposed at their phase gate (each phase entry requires its own plan + review per `.agent/WORKFLOW_RULES.md`).

## Phase 1 — Skeleton (current)

| ID | Task | Objective / acceptance | Files (owner: orchestrator) | Status |
|---|---|---|---|---|
| P1-T01 | Repo hygiene | `.gitignore` (Rust/Node/Tauri), `.gitattributes` (LF enforcement), `rust-toolchain.toml` (stable-msvc pin), `.editorconfig`; no build artifacts committed | `.gitignore`, `.gitattributes`, `rust-toolchain.toml`, `.editorconfig` | PASS |
| P1-T02 | Cargo workspace scaffold | Root `Cargo.toml` (workspace, shared deps, `[workspace.lints]` clippy/fmt strict), 6 crates per planning-audit §3 (`sair-domain`, `sair-persistence`, `sair-vault`, `sair-providers`, `sair-gateway`, `app`); each compiles with a minimal lib/bin; `cargo clippy --workspace` clean; `cargo fmt --check` clean | `Cargo.toml`, `crates/*/Cargo.toml`, `crates/*/src/lib.rs` | PASS (build 3m42s; clippy -D warnings clean; fmt clean; 15 tests ok) |
| P1-T03 | Frontend scaffold | Vite + React 19 + TS + Tailwind 4; app shell with docs/08 navigation skeleton (Overview, Providers, API Keys, Models, Deployments, Routing, Agent Integrations, Gateway, Logs, Usage, Health, Backup, Settings, About); light/dark theme tokens; `tsc --noEmit` clean; vitest runs one smoke component test; production build passes | `apps/desktop/` (package.json, vite config, src/) | PASS (tsc clean; vitest 2/2; `vite build` ok — 2026-10-30) |
| P1-T04 | Tauri shell | Tauri 2 app wrapping the frontend; minimal capabilities file; window config; one sample IPC command (`get_app_info`) proving Rust↔UI wiring; `tauri build` (debug) succeeds on Windows | `apps/desktop/src-tauri/` | PASS (tauri build --no-bundle --debug ok; exe smoke-tested: alive 7s, clean stop) |
| P1-T05 | CI pipeline | GitHub Actions: matrix windows-latest + macos-latest; jobs = fmt+clippy, cargo test, frontend typecheck+vitest+build, tauri debug build; runs green on push | `.github/workflows/ci.yml` | PASS (file complete; cannot run until repo is hosted — same commands verified locally) |
| P1-T06 | Phase 1 gate evidence | Full local build evidence (cargo build/test, npm build, tauri debug build, tool versions recorded); Phase 1 report in `reports/phase-reports/`; boards/traceability updated | `reports/phase-reports/phase-1.md` | IN_PROGRESS |

## Phase 2 — Security & persistence (COMPLETE — see reports/phase-reports/phase-2.md)

| ID | Task | Status |
|---|---|---|
| P2-T01 | SQLite schema + embedded migrations + FK/retention/settings tests | PASS (13 tests) |
| P2-T02 | OS vault impls (keyring v1; Windows Credential Manager verified live) + mock feature gate + release compile guard + shared contract tests | PASS (8 tests incl. OS contract) |
| P2-T03 | Gateway token lifecycle + middleware stack (auth / Host allowlist / 503-at-capacity / 408-timeout / body limit) + GatewayHandle drain + app wiring + Gateway UI | PASS (17 tests + live smoke test) |
| P2-T04 | Redaction formatter + sweep tests (fields, vault roundtrip, reveal path) + log rotation/pruning | PASS (3 tests) |
| P2-T05 | SsrfPolicy (scheme + default-deny denylist) + validating resolver (offline rebinding tests) + no-cross-origin redirects | PASS (9 tests) |
| P2-T06 | Phase gate: full suites, cargo audit (0 vulns; 5 informational Linux-stack warnings), threat-model addendum, security review, report | PASS |

## Phase 3 — First vertical slice (next; decompose at phase entry)

- P3-T01 sair-domain types + error taxonomy + capability model (incl. protocol_family per ADR-009)
- P3-T02 Provider adapter trait + OpenAI-compatible adapter (non-streaming) + wiremock contract tests
- P3-T03 Routing engine v0 (alias resolution, capability filter, priority strategy, decision record)
- P3-T04 Gateway `/v1/chat/completions` (non-streaming) + `/v1/models` + request records + integration tests
- P3-T05 Minimal UI: dashboard, providers, API keys (create/list/test/enable/disable)
- P3-T06 Phase 3 gate: E2E evidence (client → gateway → mock upstream → request record)

## Phase 4 — Rotation & health (epic)

- Credential pool metadata, health/cooldown state (ADR-012 mechanism), error classification, account grouping, simulated failure suite incl. concurrent rotation tests.

## Phase 5 — OpenRouter + aliases (epic)

- OpenRouter adapter (attribution headers per docs/04 research), alias/capability resolution matrix tests.

## Phase 6 — Anthropic + Gemini native surfaces (epic)

- `/v1/messages` + `/v1/messages/count_tokens`, Gemini generateContent surfaces, protocol contract tests; re-verify Gemini classic-vs-Interactions decision.

## Phase 7 — Streaming (epic)

- SSE pass-through for all surfaces, stream-commit failover boundary, no-retry-after-commit tests.

## Phase 8 — Agent profiles (epic)

- Profile generation for Codex CLI (`wire_api="chat"`), Claude Code, Gemini CLI; consent-gated external edits; verified per documented agent versions.

## Phase 9 — UI completion (epic)

- All docs/08 screens, logs/usage/diagnostics, a11y checks, route preview.

## Phase 10 — Backup/restore (epic)

- `.sair` config-only + encrypted full (ADR-007), import validation pipeline, INCOMPLETE credential states (R-8), cross-platform restore tests.

## Phase 11 — Hardening/release (epic)

- Perf budget measurement, full security review pass, clean-install checklist, SBOM; signing requires human-provided certs (see BLOCKED.md when reached).
