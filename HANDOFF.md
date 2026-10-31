# Shahid AI Router — Agent/Developer Handoff

**Read this first if you are a new agent session, a new developer, or on a new machine.**
Last updated: 2026-10-31 · v1 code-complete · head: `master` on
`github.com/Aperio-Studios-Private/shahid-ai-router` (private; head commit `4e98682`).

---

## 1. What this project is

A local-first, cross-platform desktop app (Windows + macOS) that manages multiple AI
providers, multiple credentials per provider, model deployments and routing policies, and
exposes one authenticated loopback HTTP gateway that AI coding agents point at:

- **OpenAI surface**: `POST /v1/chat/completions`, `GET /v1/models` (incl. OpenRouter with attribution headers)
- **Anthropic surface**: `POST /v1/messages`, `POST /v1/messages/count_tokens`
- **Gemini surface**: `POST /v1beta/models/{alias}:generateContent` (+ `:streamGenerateContent`)
- Non-streaming + SSE streaming on all three, with the stream-commit failover boundary (no failover after bytes flow)
- Capability-aware routing (priority / round-robin / weighted strategies), credential rotation, cooldowns, account-group quota truth
- Secrets ONLY in the OS vault (Windows Credential Manager / macOS Keychain) — the DB stores opaque refs
- `.sair` config-only + encrypted (Argon2id + AES-256-GCM) backups with consent-gated import
- Agent profiles for Claude Code, Gemini CLI, generic OpenAI clients (Codex CLI documented-unsupported — see ADR-010 amendment)
- Tauri 2 desktop shell, React 19 UI, SQLite via SQLx

## 2. Governing process (mandatory for agents)

The `.agent/` directory contains the binding workflow. An agent continuing this work MUST:

1. Read `.agent/AGENT_INSTRUCTIONS.md`, `.agent/WORKFLOW_RULES.md`, `.agent/QUALITY_GATES.md`, `.agent/SECURITY_RULES.md` before any code change.
2. Follow the per-feature state machine: DISCOVERY → PLAN_DRAFT → PLAN_REVIEW (independent subagent) → TASK_BREAKDOWN → IMPLEMENT → INTEGRATE → TEST → CODE_AUDIT → FIXES → SECURITY_REVIEW → REGRESSION → FINAL_REVIEW → SHIPPED.
3. Write a plan under `audits/planning/phaseN-plan.md`, have an **independent reviewer agent** write `phaseN-plan-review.md`, resolve every finding, then implement. (Every phase so far has such a pair — follow the pattern.)
4. Never mark PASS without evidence (test names + results in `reports/phase-reports/`).
5. Never self-waive a failed gate. BLOCKED items go in `tasks/BLOCKED.md` and require the OWNER.
6. Update per change: `docs/TRACEABILITY_MATRIX.md`, `tasks/BACKLOG.md`, `tasks/IN_PROGRESS.md`, `CHANGELOG.md`; commit + push after each gate.
7. Statuses are PASS / FAIL / BLOCKED. Unimplemented requirements stay visibly incomplete (docs/01 acceptance rule).

## 3. Current state (post-Phase 11 + audit fixes)

All **11 roadmap phases are COMPLETE** with gate evidence in `reports/phase-reports/phase-1..11.md`
(Phase 0 evidence: `audits/planning/planning-audit.md`). An independent full-repository audit
(2026-10-31) then found and FIXED UI/backup/CI defects the gates had missed — see
`reports/phase-reports/audit-fixes.md` and `tasks/COMPLETED.md` (AF-01..AF-13).
Release claim: **"code-complete with residuals"** — no release tag until the owner waives the BLOCKED items (below).

- **Tests**: 142 Rust (`cargo test --workspace`) + 2 vitest — all green (verified locally on macOS 2026-10-31).
- **Gates**: clippy `-D warnings` clean, `cargo fmt --check` clean, tsc clean, production build ok (re-verified on macOS).
- **Release build**: `target/release/shahid-ai-router.exe` (20.4 MB), SHA-256 archived in `docs/sbom/release-checksum-sha256.txt`; CycloneDX SBOMs per crate in `docs/sbom/` (note: SBOMs embed the generating machine's Windows path — regenerate before release).
- **Live smoke**: gateway autostart on 127.0.0.1:8787, 401/403 enforcement, graceful drain, clean exit — verified on the release binary.
- Traceability: 23 FR rows (FR-001..FR-023) at Implemented; residuals explicitly listed in §6.

## 4. Repository layout

```text
.agent/            binding workflow rules (read first)
docs/              charter, PRD, architecture, specs, DECISION_LOG (ADR-001..018), TRACEABILITY_MATRIX
docs/sbom/         CycloneDX SBOMs + release checksum
audits/planning/   per-phase plan + independent review pairs
audits/security/   security reviews + dependency audits
reports/phase-reports/  per-phase PASS evidence
tasks/             BACKLOG / IN_PROGRESS / COMPLETED / BLOCKED boards
crates/
  sair-domain/       pure types: error taxonomy, protocol families, routing, profiles
  sair-persistence/  SQLite (SQLx), migrations/, repos
  sair-vault/        OS secret store + SecretBytes (zeroize) + refs
  sair-providers/    adapters: openai(+openrouter), anthropic, gemini; SSRF layer
  sair-gateway/      Axum surfaces, routing engine, middleware, lifecycle
apps/desktop/
  src-tauri/         Tauri shell: state, commands, telemetry, backup, profile apply
  ui/                React 19 + Vite + Tailwind 4 frontend
.github/workflows/   CI matrix (windows/macos)
scripts/             one-shot patch/helper scripts used during phases (historical)
```

## 5. Building, testing, running (new machine setup)

Prerequisites: **Rust stable-msvc** (rustup; MSVC Build Tools on Windows), **Node 22+** (Node 26 used),
**WebView2** (Windows 11 has it), git. No other system deps.

```bash
git clone https://github.com/Aperio-Studios-Private/shahid-ai-router.git
cd shahid-ai-router

# Rust side (workspace root)
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Frontend
cd apps/desktop/ui
npm install
npm run typecheck && npm test && npm run build

# Desktop app (dev)
cd ../..            # repo root
cargo run -p shahid-ai-router
# or the full Tauri pipeline:
cd apps/desktop && tauri build --no-bundle   # CLI: npx tauri or cargo install tauri-cli
```

First run creates: SQLite DB + OS-vault entries (gateway token minted automatically) under the
OS per-user app-data dir; gateway autostarts on `127.0.0.1:8787`; logs under app-data `/logs`
(daily rotation, 14-day prune).

## 6. Residuals & owner actions (the honest list)

| Item | Who | Detail |
|---|---|---|
| **CI execution** | OWNER (blocking) | GitHub Actions fails at startup: *"recent account payments have failed or your spending limit needs to be increased"* (org billing). Fix Billing & plans → the pipeline runs fmt/clippy/tests + frontend + the Phase-11 security audit job (cargo-audit/npm audit) on push. |
| **macOS verification** | OWNER | Vault (Keychain), encrypted-backup restore, full app run. CI job ready; the workspace now also compiles on macOS (audit fix AF-13: RGBA icons + icns) and 142 tests pass there. |
| **Signing/notarization** | OWNER | Owner certificates required. |
| **Clean-machine install check** | OWNER | Fresh VM/second user — env overrides CANNOT isolate the Windows data dir (uses SHGetKnownFolderPath). |
| Streamed token counts (FR-011 residual) | Agent | SSE frame parsing, post-v1. |
| Live CLI/provider verification | OWNER | Opt-in live tests need real credentials. |
| `/v1/responses` surface (Codex CLI) | Agent, post-v1 | Codex removed `wire_api="chat"` — ADR-010 amended; without it Codex is unsupported. |

Closed by the 2026-10-31 audit fixes: mid-stream-drop full assertion (now a real raw-TCP test asserting
one upstream hit + partial relay + untouched credential); backup-restore correctness cluster (config-mode
commit, deployment natural-key restore, alias bindings, settings/profiles staging, real conflict report).

## 7. Gotchas (learned the hard way — read before coding)

- **reqwest total `.timeout()` covers the whole body read** — it would truncate SSE streams. Streaming uses dedicated clients with connect + idle-read bounds only (ADR-006/Phase 7).
- **hyper-util short-circuits IP literals before custom resolvers** — SSRF policy for literal IPs is enforced by `SsrfCheckedClient` (the ONLY client type adapters may receive; ADR-015).
- **tower-http's TimeoutLayer can hold the middleware future beyond the response** — never put counters inside the tower stack; the in-flight counter lives in the per-connection `service_fn` wrapper (Phase 11 leak).
- **axum `Router::merge` of `Router<()>` collapses the outer state type** — the API surface carries its own `Arc<ApiDeps>`; middleware state via `from_fn_with_state`.
- **SQLx 0.9** query fns take `&'static str` (`SqlSafeStr`); no dynamic SQL without `AssertSqlSafe` + review.
- **tauri-build is versioned independently of tauri** (pin `"2"`, not the tauri version).
- **wiremock 0.6 cannot stream bodies** and `set_body_string` forces its own content-type — use `set_body_raw(bytes, "text/event-stream")`; raw-TCP scenarios use hand-rolled `TcpListener` mocks.
- **reqwest 0.13 requires a rustls crypto provider installed even for plain-HTTP test clients** (app/tests install ring — ADR-006).
- **Match-arm order matters in axum middleware chains**: guard-ed streaming arms must precede unguarded non-streaming arms.
- **Gemini CLI/SDK appends `/v1beta` itself** — `GOOGLE_GEMINI_BASE_URL` must be the bare origin.
- **Bash heredocs truncate in this environment** — write patch scripts with the Write tool, never inline heredocs.
- Backticks inside double-quoted bash/python strings get command-substituted — use file-based patches.

## 8. Key documents index

| Doc | Content |
|---|---|
| `docs/00..14` | Charter, PRD (+amendments), architecture, specs, roadmap |
| `docs/DECISION_LOG.md` | ADR-001…018 (stack, crypto, SSRF, retry semantics, agent constraints) |
| `docs/TRACEABILITY_MATRIX.md` | FR → task → evidence → status (kept current) |
| `audits/planning/phaseN-plan.md` + `phaseN-plan-review.md` | Every phase's plan and its independent review |
| `audits/security/` | Phase-2 security review + retest, dependency audits |
| `reports/phase-reports/phase-0..11.md` | Per-phase PASS evidence with exact commands |
| `tasks/BLOCKED.md` | Owner-action blockers (CI billing B-01) |
| `CHANGELOG.md` | Versioned history |

## 9. If you are the OWNER (aperiostudios8)

1. Fix GitHub Actions billing → push/`gh run rerun` → first CI run is the macOS/Windows evidence.
2. Verify macOS interactively (vault + encrypted restore + app run) — closes ADR-016.
3. Provide signing certs for installers (NSIS/bundle currently unverifiable).
4. Run the clean-VM install check (fresh user/VM).
5. Waive/accept BLOCKED items per docs/11 — then tag v1.0.0.
