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

## Phase 3 — First vertical slice (COMPLETE — see reports/phase-reports/phase-3.md)

| ID | Task | Status |
|---|---|---|
| P3-T01 | Domain types (UpstreamFailure/FailureAction, capabilities, RoutingDecision) + migration 0002 | PASS |
| P3-T02 | Repos + route snapshot loader; OpenAI-compatible adapter (body-aware classification) + wiremock contract tests | PASS |
| P3-T03 | Routing engine v0 + RotationState (ADR-012) + concurrent test | PASS |
| P3-T04 | /v1/chat/completions + /v1/models + quarantine/rotation/cooldown + 7 E2E tests | PASS |
| P3-T05 | Management IPC commands (25) + Providers/Aliases UI | PASS |
| P3-T06 | Gate: 88 tests, clippy clean, live smoke, report | PASS |

## Phase 4 — Rotation & health (next; decompose at phase entry)

## Phase 4 — Rotation & health (COMPLETE — see reports/phase-reports/phase-4.md)

| ID | Task | Status |
|---|---|---|
| P4-T01 | Plan + independent review (R4-01..R4-09 resolved) | PASS |
| P4-T02 | Account-group cooldown (quota truth) + monotonic writes | PASS |
| P4-T03 | FR-021 per-deployment semaphores + actual-attempt accounting | PASS |
| P4-T04 | Health-event mapping fix (R4-01) + read-back tests + summary commands | PASS |
| P4-T05 | Retry-After header parse + caps (R4-04); credential dedupe (R4-02) | PASS |
| P4-T06 | Deterministic simulated failure suite (5 scenarios) + gate | PASS |

## Phase 5 — OpenRouter + aliases (next; decompose at phase entry)

## Phase 5 — OpenRouter + aliases (epic)

- OpenRouter adapter (attribution headers per docs/04 research), alias/capability resolution matrix tests.

## Phase 6 — Anthropic + Gemini native surfaces (COMPLETE — see reports/phase-reports/phase-6.md)

| ID | Task | Status |
|---|---|---|
| P6-T01 | Plan + independent review (F-1..F-11 resolved, external API verifications) | PASS |
| P6-T02 | Anthropic adapter (messages + count_tokens + classification matrix) | PASS |
| P6-T03 | Gemini adapter (generateContent + RESOURCE_EXHAUSTED quota-truth + RetryInfo) | PASS |
| P6-T04 | Family-parameterized handler (envelopes per family, status preservation F-1, pre-route validation F-6) | PASS |
| P6-T05 | Surface E2E suite (9 tests) + gate | PASS |

## Phase 7 — Streaming (COMPLETE — see reports/phase-reports/phase-7.md)

| ID | Task | Status |
|---|---|---|
| P7-T01 | Plan + independent review (REVISE BEFORE IMPLEMENTATION; F-1/F-2 High resolved) | PASS |
| P7-T02 | Streaming clients (no total deadline) + shared stream_request + 3 adapter variants | PASS |
| P7-T03 | Handler commit boundary + PermitStream (FR-021) + streamed records (F-5) | PASS |
| P7-T04 | 5 E2E streaming tests + gate | PASS |

## Phase 8 — Agent profiles (COMPLETE — see reports/phase-reports/phase-8.md)

| ID | Task | Status |
|---|---|---|
| P8-T01 | Plan + independent review (2 High resolved: Codex removal, apply hardening) | PASS |
| P8-T02 | Profile generators (Claude Code / Gemini CLI / generic; Codex documented-unsupported) | PASS |
| P8-T03 | Consent-gated apply (allowlist, hash precondition, symlink policy, atomic write, backup) | PASS |
| P8-T04 | Agent Integrations UI + gate | PASS |

## Phase 9 — UI completion (COMPLETE — see reports/phase-reports/phase-9.md)

| ID | Task | Status |
|---|---|---|
| P9-T01 | Plan + independent review (REVISE BEFORE IMPLEMENTATION; F-1/F-2 resolved) | PASS |
| P9-T02 | Records projection (secret-safe) + usage aggregates + no-route recording | PASS |
| P9-T03 | Logs/Usage/Health screens + a11y (focus rings, aria, manual checklist) | PASS |

## Phase 10 — Backup/restore (COMPLETE — see reports/phase-reports/phase-10.md)

| ID | Task | Status |
|---|---|---|
| P10-T01 | Plan + independent review (AMEND BEFORE IMPLEMENTATION; F-1/F-2 High + 8 resolved) | PASS |
| P10-T02 | Envelope + crypto (Argon2id + AES-256-GCM, AAD struct, integrity) | PASS |
| P10-T03 | Export (config allowlist / encrypted with vault secrets) | PASS |
| P10-T04 | Import (tx staging, natural-key conflicts, vault re-store, INCOMPLETE opt-out) | PASS |
| P10-T05 | Gate (135 tests incl. crypto roundtrip/tamper/size) | PASS |

## Phase 11 — Hardening/release (epic)

- Perf budget measurement, full security review pass, clean-install checklist, SBOM; signing requires human-provided certs (see BLOCKED.md when reached).
