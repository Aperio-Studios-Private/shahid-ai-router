# Phase 12 Plan — Cloud Auto-Backup (FR-024)

**Date:** 2026-10-31 · **Status:** AMENDED per independent review (`phase12-plan-review.md`, verdict AMEND BEFORE IMPLEMENTATION → all 10 findings resolved below)
**Owner decision defaults** (asked 2026-10-31; user may override before implementation):
**Neon first** · **both payload modes, encrypted default** · **daily + on-change schedule**.

## 1. Goal (FR-024 draft)

Opt-in automatic upload of the app's configuration to a user-chosen free-tier cloud
backend, and in-app restore from cloud snapshots — so a dead/changed machine recovers
its setup without manual `.sair` files.

- **In scope:** providers, provider_accounts, credential metadata, models, deployments,
  routing_policies, aliases, agent_profiles, allowlisted settings — i.e. exactly the
  existing `.sair` payload. Schedule, snapshot list, prune, restore, disable+purge.
- **Out of scope (v1):** request records / health events / logs (never leave the machine);
  real-time sync; merge/conflict UI beyond the existing natural-key staging report;
  end-to-end multi-user accounts (each user brings their own backend).

## 2. Research summary (verified 2026-10-31)

| Backend | Free tier | Sleep behavior | Client | Verdict |
|---|---|---|---|---|
| **Neon** | 1 GB/project, 100 CU-h/mo, no card (plans doc) | Compute autosuspends; **data never frozen**, wakes on connect | SQLx Postgres (`sslmode=require`, `-pooler` host) — app already ships SQLx | **Primary target** |
| Supabase | 2 projects | **Pauses after 7 idle days**; dashboard resume; deletion after extended pause per Supabase docs — recheck at implementation | Pure HTTPS (PostgREST), zero new deps | Secondary |
| Cloudflare D1 | 5 GB, 100k writes/day | Never sleeps | Pure HTTPS (REST + scoped API token) | Tertiary (user runs CF elsewhere) |
| Turso / GitHub Gist | ~5 GB / free w/ PAT | Never | HTTP / REST | Backlog extras |

Sizing: one envelope ≈ 5–50 KB; 10 retained versions ≈ ≤500 KB ≈ 0.05% of Neon free storage.

**Chosen: Neon first.** Rationale: the failure mode of a backup target must not be
"needs a dashboard visit after 7 idle days" (Supabase pause) — Neon's autosuspend wakes
on connect, so restore works after months offline. SQLx is already a dependency.

## 3. Design

### 3.1 Payload = the existing `.sair` envelope (no new crypto)
Encrypted mode (Argon2id m=64MiB/t=3/p=1 + AES-256-GCM, AAD, KDF bounds — Phase 10,
hardened by the audit) or config-only mode. The cloud layer is transport + schedule +
listing only. Restore downloads the envelope, writes it to a **temp file**
(`app_data/tmp/cloud-<id>.sair`, 0600) and passes its PATH through the **unchanged**
path-keyed `preview_backup` → `commit_backup` consent flow (staging, natural keys, vault
re-store, conflict report) — no changes to the held-slot machinery in
`commands_rotation.rs`. Temp-file hygiene: created at download, deleted on commit /
cancel / TTL expiry / app start (sweep at startup); encrypted envelopes are ciphertext
and config envelopes are secret-free by construction, so on-disk staging is acceptable.

### 3.2 Provider abstraction
```rust
// Dyn-compatible WITHOUT a new direct dependency (async-trait is already transitive
// via the lockfile; the rule is "no new direct dep"): methods return pinned boxed
// futures. All impls: vault reads/writes and envelope build (Argon2/GCM are CPU-bound,
// SecretStore is sync) run via tokio::task::spawn_blocking — precedent
// commands_rotation.rs:274.
pub trait CloudBackupProvider: Send + Sync {
    async fn test(&self) -> Result<(), String>;
    async fn upload(&self, envelope: &str, sha256: &str, device: &str, mode: &str) -> Result<(), String>;
    async fn list(&self) -> Result<Vec<SnapshotMeta>, String>;      // id, device, mode, version, created_at, bytes
    async fn download(&self, id: &str) -> Result<String, String>;    // envelope JSON
    async fn prune(&self, keep: usize) -> Result<u64, String>;       // per device
    async fn purge_all(&self) -> Result<u64, String>;                // disable flow
}
```
v1 impls: `NeonProvider` (SQLx `PgPool`, lazy connect). Table (setup SQL shipped in the
app's guide, user runs it once in the Neon SQL editor):

```sql
create table if not exists sair_backups (
  id          text primary key,
  device_id   text not null,
  app_version text not null,
  mode        text not null check (mode in ('encrypted','config')),
  created_at  timestamptz not null default now(),
  payload     text not null,
  sha256      text not null
);
create index if not exists sair_backups_device_idx on sair_backups (device_id, created_at desc);
```

### 3.3 Schedule ("auto")
- **Scheduler wiring (F-12-4):** a dedicated task spawned **after** `app.manage(AppState)`
  (unlike the retention task, which spawns earlier with only a `Db` clone — this one needs
  the dirty flag and last-upload state, reached via `app.state::<AppState>()` + AppHandle).
  It ticks every **60 s** and uploads when: (daily forced: last upload > 24 h) OR
  (dirty AND quiet ≥ 5 min AND ≥ 30 min since last upload). Vault reads and envelope
  build inside the task run via `spawn_blocking` (F-12-3).
- **Dirty flag:** single helper `AppState::mark_cloud_dirty()` (AtomicBool, alongside the
  existing locks). Call sites enumerated: every NON-`list_*` command in
  `commands_management.rs` (add/update/delete provider, credential, model, deployment,
  alias + set_*_enabled) plus `set_credential_priority_weight`, `add_provider_account`,
  `assign_credential_account`, `apply_agent_profile`, `commit_backup` in
  `commands_rotation.rs` (`test_provider` excluded — read-only).
- Manual "Back up now"; auto-upload after a successful restore; retry on next start on
  failure. Failures NEVER block the gateway or the UI — status field + Logs entry.

### 3.4 Security (docs/06 stance + ADR-019)
- Only the envelope crosses the wire. Encrypted mode: ciphertext + metadata; the
  passphrase never leaves the device (lost passphrase = cloud backup unrecoverable —
  explicit UI warning, same as docs/10).
- **Passphrase strategy (resolves review F-12-1):** at cloud-configure time the user
  enters — or lets the app generate — the backup passphrase; it is stored **in the OS
  vault** via the existing `SecretStore` (opaque ref in `app_settings`, exactly like the
  destination credential). Unattended auto-uploads read it from the vault on-device
  (spawn_blocking) and derive on device — the passphrase itself is never uploaded, never
  synced, never logged. Consequence (explicit UI warning): cloud-encrypted backups are
  recoverable only with that passphrase; restoring on a new machine requires re-entering
  it (user keeps it in their password manager).
- Destination credentials (Neon connection string, later Supabase key / CF token / PAT)
  stored **only in the OS vault** via the existing `SecretStore`; DB keeps an opaque ref.
  **DSN-free errors (F-12-6):** connect/query failures are mapped to messages that cannot
  contain the URL ("cloud connect failed: <class>") before logging or surfacing — sqlx
  errors can embed the DSN; field-name redaction cannot catch free text. Keep the
  `SENSITIVE_FIELDS` extension (`connection_string`, `dsn`) as defense-in-depth; the
  redaction sweep test seeds a fake DSN and asserts it appears in no sink.
- **Settings keys (F-12-8):** `cloud_provider`, `cloud_mode`, `cloud_schedule_enabled`,
  `cloud_dsn_ref`, `cloud_passphrase_ref`, `cloud_device_id` (constants next to
  `SETTING_GATEWAY_TOKEN_REF`). No schema migration is required — `app_settings` is KV.
  **Invariant:** none of these keys may ever join the backup-export allowlist
  (backup.rs — explicit allowlist `gateway`/`retention_days`/`openrouter_attribution`);
  asserted by a test so a future refactor cannot leak the DSN/passphrase refs into
  exports.
- TLS always; Neon host fixed by user config — still validated with https-only checks.
- Enable = explicit consent dialog naming the destination + mode; **Disable** offers
  "also delete all cloud rows" (`purge_all`).
- Device id = first 16 hex of SHA-256(hostname)+random — no PII beyond a stable label.

### 3.5 UI
New "Cloud backup" section in the existing Backup & Restore screen: provider picker
(Neon v1), connection-string field (vault-backed), Test, mode toggle (encrypted default,
passphrase field — stored to the OS vault at configure time per §3.4), schedule toggle, snapshot table (time · device · mode · app version ·
size), Restore buttons (reuse preview→commit), last-sync status, Back-up-now, Disable.

## 4. Tasks

| ID | Task | Acceptance |
|---|---|---|
| P12-T02 | `apps/desktop/src-tauri/src/cloud_backup.rs`: trait (pinned BoxFuture) + Neon impl (spawn_blocking for vault/argon2, DSN-free errors) + app-crate `sqlx = { workspace = true, features = ["postgres", "tls-rustls-ring"] }` (workspace manifest untouched; cargo unifies features workspace-wide at build time — compile weight is honest workspace-wide) | unit tests with in-memory trait impl roundtrip envelope |
| P12-T03 | Scheduler + dirty flag (`state.rs`/`lib.rs`); IPC: `cloud_configure/test/status/list/download/restore/disable`; secrets→vault; telemetry fields | debounce clock-injection tests; restore path reuses `stage_import` untouched |
| P12-T04 | BackupScreen "Cloud" section wired to the 7 commands | vitest smoke; a11y per docs/08 |
| P12-T05 | Supabase (PostgREST) impl behind the same trait | wiremock contract test |
| P12-T06 | Cloudflare D1 impl (optional this phase) | wiremock contract test |
| P12-T07 | Opt-in live Neon test (`#[ignore]` + `SAIR_LIVE_NEON_URL` env-gated — skipped when unset, NEVER in CI; no implemented precedent exists, this pins the mechanism); redaction sweep incl. seeded fake DSN; settings-allowlist-exclusion invariant test | |
| P12-T08 | Docs: ADR-019, docs/10 §Cloud, docs/06 amendment, TRACEABILITY FR-024, CHANGELOG, README | |

## 5. Risks / notes
- `sqlx` postgres+tls features: unified workspace-wide at build time (cargo feature
  unification) — acceptable; sqlite+postgres coexist in sqlx 0.9.
- **No CSP or capability change:** outbound TLS is Rust-side (SQLx/reqwest), not governed
  by the webview CSP; custom `#[tauri::command]`s need no capability entries.
- CI impact: new unit/wiremock tests join `cargo test --workspace`; the live Neon test is
  env-gated off by default (never runs in CI).
- Postgres static-string rule applies: `&'static str` queries only (SQLx 0.9), same as SQLite layer.
- A hostile/failed backend can only receive what was already consented; download path
  validates the envelope with the existing `parse_envelope` (size cap + integrity) BEFORE preview.

## 6. Review resolution (per repo convention)

Independent review: `phase12-plan-review.md`, verdict AMEND BEFORE IMPLEMENTATION
(2026-10-31). Disposition — all 10 findings resolved in this revision:

| Finding | Resolution |
|---|---|
| F-12-1 (High) | Passphrase stored in the OS vault at configure time (user-entered or generated); auto-uploads read it on-device via spawn_blocking; explicit recoverability warning. §3.4 |
| F-12-2 | Cloud restore = download → temp `.sair` file → UNCHANGED path-keyed preview/commit; temp-file hygiene specified. §3.1 |
| F-12-3 | Trait pinned to manual BoxFuture (no new direct dep); spawn_blocking mandated for vault + envelope build. §3.2/§3.3 |
| F-12-4 | Scheduler spawns after `app.manage`, 60 s tick, daily-forced rule, `mark_cloud_dirty()` with enumerated call sites. §3.3 |
| F-12-5 | docs/01 FR-024 amendment added to P12-T08; docs/06 wording pinned; threat-model entry; docs/07 note. P12-T08 |
| F-12-6 | DSN-free error mapping + seeded-DSN sweep test. §3.4, P12-T07 |
| F-12-7 | `postgres` + `tls-rustls-ring`, app-crate manifest only; workspace-unification stated honestly. P12-T02, §5 |
| F-12-8 | Settings-key list + no-migration statement + allowlist-exclusion invariant test. §3.4 |
| F-12-9 | No CSP/capability change; `#[ignore]`+env-gated live test, never in CI. §5, P12-T07 |
| F-12-10 | Supabase retention figure corrected to the verified fact. §2 |

READY FOR IMPLEMENTATION (P12-T02 may start).
