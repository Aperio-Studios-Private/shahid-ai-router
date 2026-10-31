# Shahid AI Router

A local-first, cross-platform desktop application and local HTTP gateway that manages
multiple AI providers, multiple credentials per provider, model deployments and routing
policies — so AI coding agents connect to **one stable authenticated loopback endpoint**
while the gateway selects eligible upstream deployments, rotates credentials, and
survives provider failures.

**Status:** v1 code-complete (all 11 roadmap phases). See [HANDOFF.md](HANDOFF.md) for
the current state, residuals, and owner actions.

## What it does

| Surface | Endpoints |
|---|---|
| OpenAI-compatible (incl. OpenRouter) | `POST /v1/chat/completions`, `GET /v1/models` |
| Anthropic native | `POST /v1/messages`, `POST /v1/messages/count_tokens` |
| Gemini native | `POST /v1beta/models/{alias}:generateContent` (+ `:streamGenerateContent`) |

- Non-streaming and SSE streaming, with the failover boundary at stream commitment
- Capability-aware routing: aliases map to deployments filtered by protocol family and
  request capabilities; strategies: priority, round-robin, weighted
- Credential rotation with cooldowns, quarantine, and account-group quota truth
  (billing/quota failures cool the whole account group — never burn shared keys)
- Secrets only in the OS vault (Windows Credential Manager / macOS Keychain); the
  database stores opaque references
- Metadata-only request history with full routing-decision records ("why was this
  deployment unavailable") and a 30-day retention policy
- `.sair` config-only and encrypted (Argon2id + AES-256-GCM) backups — **plus optional cloud auto-backup** (Phase 12) to your own free-tier Neon or Supabase project: daily + on-change snapshots, in-app restore, secrets only inside the AES-256-GCM envelope, request logs never leave the machine
- Agent profiles for Claude Code, Gemini CLI, and generic OpenAI-compatible clients
- SSRF hardening (validating resolver + literal/scheme checks), token auth with
  constant-time comparison, Host allowlist, per-request timeout, per-deployment
  concurrency bounds, redacted telemetry

## Quick start

See [HANDOFF.md](HANDOFF.md) §5 for full setup. Short version:

```bash
cargo test --workspace                 # 142 tests
cd apps/desktop/ui && npm install && npm test && npm run build
cd ../.. && cargo run -p shahid-ai-router
```

The gateway autostarts on `127.0.0.1:8787` (loopback only, token-authenticated —
reveal/copy the token in the app's Local Gateway screen).

## Cloud auto-backup (optional)

Point the app at your own free-tier database and it snapshots your configuration
automatically — see [docs/10](docs/10_BACKUP_RESTORE.md) and ADR-019:

1. Create a Neon project ([neon.com](https://neon.com) — free, no card) or a Supabase project.
2. Run the `sair_backups` table SQL from [docs/04 §Cloud backup table](docs/04_PROVIDER_PROTOCOLS.md) in the project's SQL editor.
3. In the app: **Backup & Restore → Cloud backup (beta)** → paste the connection string (Neon) or project URL + API key (Supabase), choose encrypted (default) or config-only, enable the schedule.

Encrypted snapshots include your API keys but are AES-256-GCM ciphertext — useless without your passphrase, which lives only in this machine's OS vault (keep a copy in your password manager: restoring on a new machine needs it). Request logs are never uploaded.

## NVIDIA NIM

NVIDIA NIM works out of the box through the OpenAI-compatible provider type
(see [docs/04](docs/04_PROVIDER_PROTOCOLS.md) § NVIDIA NIM):

- **Hosted** (build.nvidia.com): add a provider with base URL
  `https://integrate.api.nvidia.com/v1` (keep the `/v1`), type
  `openai_compatible`, then a credential with your `nvapi-…` key. Add the
  model (e.g. `meta/llama-3.1-8b-instruct`, family `openai_chat`), deploy it,
  and bind an alias.
- **Self-hosted NIM container**: base URL `http://<host>:8000/v1` and enable
  "Allow loopback HTTP" for local targets.

## Layout

- `crates/` — Rust workspace: `sair-domain`, `sair-persistence`, `sair-vault`,
  `sair-providers`, `sair-gateway`; the Tauri shell lives in `apps/desktop/src-tauri`
- `apps/desktop/ui` — React 19 + Vite + Tailwind 4 frontend
- `docs/` — charter, PRD, specs, ADRs (`DECISION_LOG.md`), traceability matrix, SBOMs
- `reports/`, `audits/`, `tasks/` — phase-gate evidence, independent reviews, boards
- `.agent/` — binding agent workflow rules

## Platform support

Windows 10+ x64 and macOS 12+ (x86_64/aarch64). Linux is not a supported platform for v1.

## License

Proprietary — see [LICENSE](LICENSE). Choose a distribution license before publishing.
