# Phase 12 Plan Review — Cloud Auto-Backup / FR-024 (Independent)

Reviewer: independent plan reviewer (not a plan author). Date: 2026-10-31.
Plan under review: `audits/planning/phase12-plan.md` (DRAFT — awaiting independent review).
Inputs examined: docs/06_SECURITY_PRIVACY.md, docs/10_BACKUP_RESTORE.md, docs/07_DATA_STORAGE.md, docs/01_PRD.md, docs/TRACEABILITY_MATRIX.md, docs/DECISION_LOG.md (ADR-018 latest), docs/11_TESTING_QUALITY.md, `.agent/WORKFLOW_RULES.md`, `.agent/SECURITY_RULES.md`; and the code: `apps/desktop/src-tauri/src/{backup,backup_import,commands_rotation,state,lib,telemetry}.rs`, `crates/sair-vault/src/store.rs`, root + app `Cargo.toml`, `Cargo.lock`, `tauri.conf.json`, `capabilities/default.json`. External facts checked against neon.com/pricing, supabase.com docs, docs.rs/sqlx.

**VERDICT: AMEND BEFORE IMPLEMENTATION** (detail in the closing section; each finding below carries a concrete resolution).

---

## Claims verified sound (no action)

- **Payload reuse is real.** `build_config_payload` / `build_encrypted_envelope` / `parse_envelope` / `decrypt_envelope` (backup.rs:108-295) and the KDF bounds (backup.rs:199-217) are reusable as-is for cloud upload and download validation; `stage_import` (backup_import.rs:20) takes a payload value and needs no change. `backup.rs` and `backup_import.rs` need **no modifications** for this phase — the only reuse caveat is the path-keyed preview/commit commands (F-12-2).
- **Envelope-only transport does not violate the written prohibitions.** docs/06 Secrets forbids exporting *plaintext* and forbids secrets *outside OS secure storage* — the encrypted envelope is exactly the docs/10 export-mode-2 artifact already sanctioned for files, and passphrase derivation happens on device (backup.rs:85-95), so "the passphrase never leaves" is accurate. docs/06 Privacy's "no cloud sync **by default**" is satisfied by opt-in; the second sentence's network-request allowlist must still be amended (F-12-5). SECURITY_RULES #10 (no sync without product approval) is covered by the recorded owner decisions in the plan header.
- **Neon facts check out** (neon.com/pricing, 2026-10-31): 1 GB/project free storage (20 GB account total), 100 CU-h, no card, scale-to-zero after 5 min and wake-on-connect. Sizing math is fine.
- **SQLx claims check out**: workspace pins sqlx 0.9.0 with `default-features = false, features = ["runtime-tokio","sqlite","macros","migrate"]` (Cargo.toml:47; Cargo.lock:4260). sqlite and postgres features coexist; 0.9's static-query rule (`&'static str` / `AssertSqlSafe` for dynamic SQL) is respected by the plan — every proposed query is a literal with bound parameters.
- **Vault usage is consistent with the codebase**: destination credential in the OS vault + opaque ref in `app_settings` mirrors the `SETTING_GATEWAY_TOKEN_REF` pattern (state.rs:34-37, 98-107); fail-closed handling matches. `SecretStore` is **sync** (store.rs:58-76) — see F-12-3 for the runtime implication.
- **"Failures never block the gateway/UI"** matches the existing retention pattern (state.rs:305-315: warn-and-continue, never panic/fail the app).
- **Daily-timer pattern exists** (lib.rs:128-135) — with wiring caveats (F-12-4).
- **No docs/07 violation**: the envelope is fully built before any network call, and `stage_import` is local-only — no DB transaction spans an upstream call (docs/07:18).
- **Traceability**: the matrix ends at FR-023 (TRACEABILITY_MATRIX.md:31) so FR-024 numbering is available; ADR-019 is the next free ADR number (DECISION_LOG latest = ADR-018).
- **Telemetry mechanism is real**: `SENSITIVE_FIELDS` exists (telemetry.rs:28-40) and the planned `connection_string`/`dsn` extension is meaningful — subject to F-12-6.

## Findings summary

| ID | Severity | Finding | Resolution |
|---|---|---|---|
| F-12-1 | High | Passphrase availability for unattended *encrypted* auto-upload is unspecified | Pick: passphrase in OS vault (recommended) or restrict auto to config mode — write into plan |
| F-12-2 | Medium | Cloud restore cannot reuse the path-keyed `preview_backup`/`commit_backup` unchanged | Temp-file or shared held-slot commands; name the files that change |
| F-12-3 | Medium | Blocking vault/Argon2 calls on the async runtime; trait-object strategy left as "?" | `spawn_blocking` mandate + pin manual BoxFuture |
| F-12-4 | Medium | Scheduler wiring: spawn order vs `app.manage`, 24 h loop can't do a 5-min debounce, ~20 dirty-flag call sites | Spawn after manage; specify tick; centralize `mark_cloud_dirty` |
| F-12-5 | Medium | Docs amendments incomplete: docs/01 PRD has no FR-024; docs/06 amendment content unpinned | Add PRD amendment + pin exact wording |
| F-12-6 | Medium | Field-name redaction cannot catch a DSN inside an error string | DSN-free error mapping + sweep test |
| F-12-7 | Low | sqlx feature names and workspace feature-unification not stated | Pin `postgres` + `tls-rustls-ring`, app-crate manifest only |
| F-12-8 | Low | Cloud settings-key surface unpinned (constants, device_id, export-allowlist exclusion) | Define keys + exclusion invariant/test |
| F-12-9 | Low | CSP/capability and CI/live-test statements missing or precedent-free | One-line statements; pin the opt-in mechanism |
| F-12-10 | Low | Supabase "data kept 1 yr" unverified; Neon verified | Correct or drop the number |

---

## Findings

### F-12-1 — High — The default-path feature (encrypted auto-upload) has no passphrase-availability story for unattended uploads

- **Evidence:** Plan §3.5 makes encrypted the default mode; §3.3 schedules uploads daily and on-change — i.e. while no one is at the keyboard. Building the encrypted envelope requires the passphrase at upload time (`build_encrypted_envelope`, backup.rs:131-190). The plan says only "passphrase field" in the UI (§3.5) and "the passphrase never leaves the device" (§3.4). It never says where the passphrase lives between unattended uploads. Held in memory only, the scheduled path dies at the first app restart; silently persisted anywhere other than the OS vault would violate docs/06 Secrets ("store secrets only in OS secure storage").
- **Required resolution:** Pick one and write it into §3.4/§3.5: (a) **store the passphrase in the OS vault** via the existing `SecretStore` at cloud-configure time (DB keeps an opaque ref, exactly like the destination credential; the lost-passphrase warning extends to it; auto-encrypted backups are then recoverable only while the vault item exists); or (b) scope auto-upload to config mode and make encrypted upload manual-only (passphrase entered per action). Either is compliant; the plan as drafted is implementable in neither case for its stated default.

### F-12-2 — Medium — "Restore reuses the existing preview→commit flow" is not true of the commands as written; the held slot is private and path-keyed

- **Evidence:** `preview_backup` reads a **file path** and `commit_backup(path)` matches the held entry by path (commands_rotation.rs:524-533); the held slot is a module-private static keyed `(String path, HeldImport)` (commands_rotation.rs:301-306). A cloud restore holds downloaded **bytes**, not a path. P12-T03's claim "restore path reuses `stage_import` untouched" is correct for backup_import.rs, but §3.1/§3.5's "feeds the existing two-step `preview_backup` → `commit_backup` consent flow" implies unchanged commands, which is impossible without a decision the plan does not make. Any new cloud preview/commit pair must also replicate the held-slot semantics (mode captured at preview, commit never re-reads the source, TTL, SecretBytes — commands_rotation.rs:293-306) and preserve the `re_store_secrets` consent path (commands_rotation.rs:544-588).
- **Required resolution:** State the mechanism in §3.1: either (a) write the download to a temp file and pass its path through the unchanged commands (state temp-file hygiene: deleted after commit/expiry; config-mode envelopes are plaintext but secret-free, encrypted envelopes are ciphertext — acceptable on disk), or (b) move the held slot into `AppState` (or export it) and add `cloud_preview`/`cloud_commit` commands keyed by snapshot id. Either way **`commands_rotation.rs` is touched or its slot is relocated** — the plan must say so; it currently lists no changes to existing files.

### F-12-3 — Medium — Blocking work on the async runtime, and the object-safety decision is left as a sketch with a question mark

- **Evidence:** `SecretStore` is sync/blocking (store.rs:58-76) and Argon2id (64 MiB, t=3) is CPU-bound (backup.rs:85-95). The plan's `CloudBackupProvider` is async (§3.2), so the Neon impl will be tempted to read the connection string from the vault and (encrypted mode) build the envelope directly inside the async trait call / scheduler task — stalling a Tokio worker. The codebase precedent mandates `spawn_blocking` for exactly this (commands_rotation.rs:274-278; backup.rs doc comments "callers wrap in spawn_blocking"); the plan never mentions it. Separately, §3.2's `#[async_trait? no — manual BoxFuture, crate avoids deps]` is an unresolved sketch: native `async fn` in traits is **not dyn-compatible**, and `AppState` holds trait objects (`Arc<dyn SecretStore>`), so manual `Pin<Box<dyn Future>>` returns are the right call — but it must be pinned, and "crate avoids deps" should be phrased precisely: async-trait 0.1.92 is already a **transitive** dependency (Cargo.lock:245), the rule is "no new direct dependency".
- **Required resolution:** Add to §3.2/§3.3: (1) trait methods return pinned boxed futures (no `async_trait` dependency); (2) vault reads/writes and envelope build (Argon2/GCM) run via `spawn_blocking` in both the scheduler task and the IPC commands.

### F-12-4 — Medium — Scheduler wiring: the retention-task analogy breaks in three concrete ways

- **Evidence:** (1) The retention task is spawned **before** `app.manage(AppState)` and holds only a `Db` clone (lib.rs:128-137); the cloud scheduler needs the dirty flag and last-upload state in `AppState`, so "same pattern as the existing retention task" (§3.3) needs the explicit statement that this task is spawned **after** `manage` (or holds the `AppHandle`). (2) A 24 h loop cannot implement a "5 min quiet AND ≥30 min since last upload" debounce — the plan must specify a second, short tick (e.g. a 60 s poll evaluating dirty + quiet conditions). (3) "Mutating management IPC commands set a dirty flag" spans ~20 commands (16 non-list commands in commands_management.rs, plus `set_credential_priority_weight`, `add_provider_account`, `assign_credential_account`, `apply_agent_profile`, and `commit_backup` in commands_rotation.rs) — relying on each call site to remember the flag invites omissions.
- **Required resolution:** Specify in §3.3: spawn point; debounce tick interval; a single `AppState::mark_cloud_dirty()` helper (atomic flag, in `AppState` alongside the existing `RwLock` fields) with the command list enumerated in the plan (or a stated default: every non-`list_*` command in commands_management.rs + the five named rotation commands).

### F-12-5 — Medium — Docs amendments are listed but incomplete and unpinned: FR-024 does not exist in the PRD

- **Evidence:** docs/01_PRD.md ends at FR-023 (docs/01_PRD.md:34-37) — FR-024 exists nowhere; P12-T08 adds a TRACEABILITY_MATRIX row for FR-024 but does **not** amend docs/01, so the matrix would reference a requirement with no PRD definition. The docs/06 amendment is listed (P12-T08) but its content is unspecified: the sentence that must change is docs/06:22 ("Network requests only for configured providers and explicitly enabled update/diagnostic features") — a cloud backup destination is neither — and the threat-model addendum should gain a cloud-backup surface entry (envelope-only, TLS, consent, purge, no logs). docs/07_DATA_STORAGE.md needs at most a one-line note (config data now also at rest, encrypted envelope, in the user's own Neon project).
- **Required resolution:** Add docs/01 to P12-T08 (FR-024 drafted as in plan §1); pin the docs/06 amendment wording; add the addendum entry; state whether docs/07 gets a note.

### F-12-6 — Medium — "Never logged" for the DSN is weaker than the plan's redaction claim: error strings are free-text

- **Evidence:** `SENSITIVE_FIELDS` is an **exact-match field-NAME** list, and telemetry.rs:38-40 documents that free-text (message content) is unredactable. Adding `connection_string`/`dsn` field names (§3.4) catches `tracing` fields only — a sqlx connect error or a debug-formatted `PgPoolOptions` failure can embed the URL (which contains the Neon password) into an error **message**, which no field-name list catches, and such errors flow to logs and the UI error state.
- **Required resolution:** Add to §3.4: the Neon provider must map connect/query failures to DSN-free messages ("connect failed: <reason>" without the URL) before they are logged or returned; keep the field-name extension as defense-in-depth; extend the P12-T07 redaction sweep to seed a fake DSN and assert it appears in no sink.

### F-12-7 — Low — sqlx feature precision and the workspace-unification reality

- **Evidence:** P12-T02 says "sqlx postgres/rustls features" without names or placement. Exact names for sqlx 0.9: `postgres` + `tls-rustls-ring` (ring matches the workspace's rustls/ring posture, Cargo.toml:56, and avoids aws-lc-rs's toolchain weight), added in the **app crate's** Cargo.toml (`sqlx = { workspace = true, features = [...] }`, apps/desktop/src-tauri/Cargo.toml:34) — the workspace manifest (Cargo.toml:47) stays untouched. Note honestly: in a workspace build, cargo unifies features, so sqlx compiles once with sqlite+postgres for all members; "adds compile weight to the app crate" (§5) understates this.
- **Required resolution:** Pin the two feature names, state manifest placement (app crate only), and correct the compile-weight sentence to say workspace-wide unification applies at build time.

### F-12-8 — Low — The settings-key surface for cloud config is undefined

- **Evidence:** §3.4 says "DB keeps an opaque ref" but the plan defines no keys. Needed: `SETTING_`-style constants (provider kind, mode, schedule, dsn vault ref, persisted device_id) following state.rs:34-37. No SQL migration is required (`app_settings` is KV) — state that explicitly so "migrations" isn't assumed. Invariant to pin: these keys must **never** join the backup-export allowlist (backup.rs:530-537 — explicit allowlist, currently `gateway`/`retention_days`/`openrouter_attribution`), mirroring the `gateway_token_ref` exclusion; the allowlist is explicit so exclusion is the default, but it should be asserted by a test so a future refactor can't leak the dsn ref into exports.
- **Required resolution:** Add the key list + the no-migration statement + the allowlist-exclusion invariant with a test to §3.4/P12-T03.

### F-12-9 — Low — CSP/capabilities and CI/live-test statements are missing or without precedent

- **Evidence:** (1) The plan is silent on CSP/capabilities — verified: outbound TLS from the Rust side (SQLx/reqwest) is not governed by the webview CSP (tauri.conf.json:24) and custom `#[tauri::command]`s need no capability entries (`capabilities/default.json` governs core/plugin permissions) — so the true statement is "no CSP or capability change; all outbound TLS is Rust-side", worth one line in §5. (2) P12-T07 cites the "repo's opt-in live-test pattern": docs/11:4 names "opt-in live provider" as a test **layer** only — no implemented precedent exists in the repo. (3) No CI note: new unit/wiremock tests join `cargo test --workspace`; the live Neon test must be excluded from CI by default.
- **Required resolution:** Add the three one-liners: no CSP/capability change; the live test's concrete mechanism (`#[ignore]`-gated or `SAIR_LIVE_NEON_URL`-env-gated, skipped when unset, never run in CI); CI runs the new unit/contract tests only.

### F-12-10 — Low — Research-table accuracy: Neon verified, Supabase retention number unverified

- **Evidence:** Neon row verified (neon.com/pricing: 1 GB/project, 100 CU-h, no card, scale-to-zero, wake-on-connect). The Supabase row's "data kept 1 yr" could not be confirmed; Supabase documents the 7-day pause (verified) and dashboard restore, but paused-free-project deletion is commonly documented around 90 days. Supabase is a secondary target, but the number would propagate into ADR-019.
- **Required resolution:** Correct to the verified fact ("dashboard resume; deletion after extended pause per Supabase docs — recheck at implementation") or drop the retention figure.

---

## Disposition list — must be resolved in the plan before P12-T02

1. **F-12-1 (High):** choose and write the passphrase strategy for unattended encrypted auto-upload (OS-vault-backed recommended).
2. **F-12-2:** pick temp-file vs shared held-slot for cloud restore; name the files that change (commands_rotation.rs or new commands).
3. **F-12-3:** pin manual BoxFuture (no new direct dep) and mandate `spawn_blocking` for vault calls + envelope build.
4. **F-12-4:** scheduler spawn point (after `app.manage`), debounce tick, centralized dirty-flag helper with enumerated call sites.
5. **F-12-5:** add the docs/01 PRD amendment (FR-024) to P12-T08; pin the docs/06 wording + threat-model addendum entry.
6. **F-12-6:** DSN-free error mapping + seeded-DSN sweep test in P12-T07.
7. **F-12-7/F-12-8/F-12-9/F-12-10:** fold the precision items (feature names, settings keys + allowlist invariant, CSP/CI/live-test statements, Supabase number) into the same revision.

Convention note: per the repo pattern, append a "Review resolution" section to `phase12-plan.md` recording how each finding was disposed (see phase11-plan.md:36).

## Verdict

The plan's core is sound and well-grounded: the envelope is genuinely reusable with no new crypto (backup.rs/backup_import.rs verified), the security stance is consistent with docs/06/docs/10 as written, the Neon facts and SQLx 0.9 claims verify, the vault/`SETTING_` patterns match the codebase, and the provider abstraction is the right shape. But as drafted the flagship default path is unimplementable (no passphrase story for unattended encrypted uploads — F-12-1), the restore-reuse claim does not survive contact with the path-keyed preview/commit commands (F-12-2), and the async-runtime, scheduler-wiring, docs-amendment, and redaction details would otherwise be improvised at implementation time (F-12-3 through F-12-6). These are bounded plan amendments, not architectural rework: revise the plan per the disposition list above, then the phase is executable.

VERDICT: AMEND BEFORE IMPLEMENTATION — F-12-1 must be resolved in the plan before any implementation task; F-12-2 through F-12-6 must be folded into the same revision; F-12-7 through F-12-10 are non-blocking precision items to include while amending.
