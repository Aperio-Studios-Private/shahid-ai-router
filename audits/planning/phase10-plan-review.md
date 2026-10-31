# Phase 10 Plan — Independent Review

Reviewer: independent plan reviewer. Date: 2026-10-31. Head reviewed: 2cb79a2.
Subject: `audits/planning/phase10-plan.md` (Backup/Restore — `.sair`).

Reviewed against: docs/10_BACKUP_RESTORE.md (governing), docs/06_SECURITY_PRIVACY.md, docs/07_DATA_STORAGE.md, docs/01_PRD.md FR-014, docs/11_TESTING_QUALITY.md (mandatory scenarios), docs/DECISION_LOG.md ADR-007/ADR-016, audits/planning/planning-audit.md R-8, .agent/SECURITY_RULES.md; and current code: `crates/sair-persistence/src/repos.rs`, `crates/sair-persistence/src/lib.rs`, `crates/sair-persistence/migrations/0001_init.sql`, `crates/sair-vault/src/{store,os,secret_bytes,lib}.rs`, `apps/desktop/src-tauri/src/{state,profile_apply}.rs`. Crypto API shapes and advisories verified against docs.rs/rustsec.org (see verification section).

## Findings

### F-1 — HIGH / correctness (FR-014 conflict resolution) — "skip existing ids" is the wrong conflict key; natural-key UNIQUE constraints will hard-fail the whole import in exactly the merge scenario docs/10 targets

Evidence:
- The plan: "conflicts (existing ids → 'skip' default; v1 conflict policy: SKIP existing ids …)".
- Entity ids are 16 random hex bytes generated at insert time (`repos.rs:19-27`). Rows in a backup carry their source-machine ids; a destination machine that already has its own rows has *different* ids for semantically identical entries.
- The schema enforces natural-key uniqueness, not id uniqueness, on the entities being imported: `model_aliases.alias UNIQUE` (0001_init.sql:91), `routing_policies.name UNIQUE` (:78), `models (provider_id, model_id) UNIQUE` (:60), `deployments (provider_id, model_id, COALESCE(endpoint_override,''))` unique index (:73), `credentials (provider_id, label) UNIQUE` (:26). (`providers.name` is *not* unique — duplicate provider rows will import silently instead.)
- Consequence: importing machine A's backup into machine B that already has an alias named `fast` (different id) hits the UNIQUE constraint mid-transaction → the entire import rolls back with an opaque DB error. That is precisely the "conflict" case FR-014/docs/10 require preview + conflict resolution for. Conversely, `credentials.secret_ref UNIQUE` means INCOMPLETE placeholder refs must be minted collision-free per import.

Recommendation:
1. Define a per-entity natural conflict key in the plan: `model_aliases.alias`, `routing_policies.name`, `models (provider_id, model_id)`, `deployments (provider_id, model_id, endpoint_override)`, `credentials (provider_id, label)`, `alias_deployments (alias_id, deployment_id)`. Preview computes conflicts against these keys (existing row = same natural key, different id → skip; same id → skip), not against ids alone.
2. Preview and the commit report must both use this key; add a test for the natural-key collision case (import into a DB that already has the same alias name under a different id) alongside the existing same-id test.
3. Document that `providers` have no natural key: repeated config-only imports of the same provider duplicate rows. Either declare provider name de-duplication in preview or accept + document it.

### F-2 — HIGH / spec deviation (docs/07) + security surface (docs/06, SECURITY_RULES 9) — encrypted-mode import makes the user re-type secrets over the webview/IPC even though the backend already decrypted them; docs/07 mandates backend re-store

Evidence:
- docs/07 Secret lifecycle: "on import decrypt in memory and re-store in destination vault." docs/10 import steps likewise include "re-store secrets in destination OS vault" as a step of the encrypted flow, and the hard rule is only "never export plaintext secrets, silently overwrite…".
- The plan instead: "re-store only happens if the user supplies secrets at import (UI collect step) — else INCOMPLETE"; `commit_import(preview_id, secrets: {credential_label: value})`. At that point the backend has already authenticated and decrypted the payload (preview succeeded), so it already holds every secret value in memory.
- This is strictly worse than the spec'd path: user-typed secrets become JS strings in the webview (GC'd, never zeroized), then cross IPC as JSON — a plaintext secret channel that docs/06's design otherwise avoids ("accept secret in backend; store in OS vault", threat-model addendum keeps plaintext out of webview storage). It is also worse UX (re-typing N API keys right after proving knowledge of the passphrase) and it re-keys the map by `credential_label`, which is only unique per `(provider_id, label)` (0001_init.sql:26) — two credentials sharing a label across providers are indistinguishable.
- The plan cites R-8 for this, but R-8 (planning-audit.md:128) is about *config-only* import where no secret values exist. In encrypted mode they exist.

Recommendation:
1. Encrypted mode: at commit (after preview), the backend re-stores each decrypted secret directly via `SecretStore::store` and updates the credential's `secret_ref` — no UI collect step, no secrets in IPC payloads. Status stays `'active'`; the INCOMPLETE path applies only when the user declines re-store (offered as an explicit per-import option, default = re-store), or in config-only import if credential rows are ever included (see F-7).
2. If a partial re-store failure occurs, keep the credential row with the placeholder ref + `'incomplete'` and report it — never leave a row pointing at a ref that resolves to another credential's secret.
3. Fix the plan text so `collect_secrets`/secrets-map disappears from the IPC surface; anything the UI needs to display is labels/counts only.

### F-3 — MEDIUM-HIGH / security (docs/06 "clear buffers where practical") — the preview→commit contract forces decrypted secrets (or the derived key) to live in app state between IPC calls, with no stated TTL or zeroization

Evidence:
- `commit_import(preview_id, secrets)` carries no passphrase. For commit to complete without re-prompting, the natural implementation retains the decrypted payload — which in encrypted mode contains every plaintext API key — in the Tauri app state keyed by `preview_id`. The plan only says the preview token is "in-memory (state, not persisted)".
- There is no expiry, no drop-on-error/app-close statement, and no zeroization requirement for this retained buffer. An import the user starts and abandons leaves plaintext secrets in memory indefinitely (plus the SecretBytes-derived key, if retained). `SecretBytes` (sair-vault/src/secret_bytes.rs) already exists and is the right tool, but the plan never wires it to the preview state.

Recommendation:
1. State the retention contract explicitly: the validated payload (or derived key) is held only as `SecretBytes`, dropped (zeroized) on commit, error, new preview, or TTL expiry (e.g. 5 minutes; preview_id becomes invalid afterward with an explicit "re-run import" message).
2. Alternative that removes retention entirely: `commit_import` re-takes the passphrase and re-decrypts (one extra 64 MiB Argon2 derivation is acceptable for a manual operation). Pick one and write it into the plan.
3. Either way, derive the KDF output into `SecretBytes` and never into a plain `Vec<u8>`/`[u8;32]` local that outlives the call.

### F-4 — MEDIUM-HIGH / correctness (FK integrity) — `provider_accounts` is missing from the export payload; encrypted import breaks on `credentials.account_id`, and insert ordering is unpinned

Evidence:
- The plan's payload list (config and encrypted) names: providers, models, deployments, aliases, alias_deployments, routing policies, agent profiles, settings (+credentials in encrypted mode). The schema has `provider_accounts` (0001_init.sql:19-27) and `credentials.account_id REFERENCES provider_accounts (id)` (:32).
- An encrypted backup containing a credential with `account_id` set cannot be staged: the FK target row does not exist in the payload → insert fails → whole transaction rolls back.
- Related unpinned detail: import must insert in FK-dependency order (providers → provider_accounts → credentials; models → deployments; routing_policies → model_aliases → alias_deployments; model_aliases → agent_profiles). The plan never mentions ordering; a wrong order turns a valid file into a spurious validation failure.

Recommendation:
1. Add `provider_accounts` to both export modes (it is non-secret) and pin the insert order in the plan (or derive it from a fixed table-order constant shared with the tests).
2. Add a roundtrip test with an account-grouped credential (account_id set) to the mandatory test list.

### F-5 — MEDIUM / docs-10 compliance — manifest is missing required fields, and config-only mode has no integrity metadata at all

Evidence:
- docs/10: "Manifest: format ID/version, app version, **schema version**, timestamp, **included sections**, encryption metadata and **integrity metadata**."
- The plan's envelope: `{format, format_version, app_version, created_at, mode, data}` (+ kdf). No `schema_version` (the DB migration schema the payload was exported from — needed to gate imports into older app builds), no `included_sections`, and no integrity metadata. For config-only mode there is no authentication step at all in the import list, so a silently corrupted (or edited) cleartext file imports without notice — docs/10's import flow requires "integrity/authentication" *before* preview in all modes.

Recommendation:
1. Add `schema_version` (e.g. latest embedded migration id) and `sections: [...]` to the envelope; reject files whose schema_version is newer than the running app with an explicit "update the app" message.
2. Add integrity metadata that works in both modes: a SHA-256 of `data` stored in the manifest, verified before validation (for config mode it detects corruption/accidental edit; for encrypted mode the GCM tag remains the authenticator). State that it is not an anti-tamper control in cleartext mode.
3. Keep "never put secrets in manifest" — the hash covers data only.

### F-6 — MEDIUM / docs-10 & ADR-007 compliance — the required lost-passphrase warning is absent from the plan

Evidence:
- docs/10: "Lost passphrase means encrypted backup may be unrecoverable; **state this clearly**." ADR-007: "lost passphrase = unrecoverable (explicit UI warning)".
- The plan specifies the min-12-chars policy and wrong-passphrase messaging, but nowhere a warning at export time (or in the export UI copy). docs/10's "state this clearly" is a UI requirement on the export path, not just a report footnote.

Recommendation: add to the plan's UI section: encrypted export shows a persistent, non-dismissible warning ("If you lose this passphrase, this backup cannot be restored — there is no recovery") and requires an explicit acknowledgment (checkbox or typed confirmation) before the export runs. Add a UI test asserting the warning is rendered on the encrypted path.

### F-7 — MEDIUM / traceability — config-only export drops credential rows entirely, silently diverging from the accepted R-8 resolution and losing non-secret credential metadata

Evidence:
- R-8's accepted resolution (planning-audit.md:128): config-only import → "Import validation marks credentials without a vault item as state `INCOMPLETE`; surfaced in UI; user re-enters the secret; documented in Phase 10 tests." That presupposes credential *rows* (label, priority, weight — none of which are secrets) are present in the config-only payload.
- The plan instead excludes credential rows wholesale from config-only export and repurposes R-8 for encrypted mode. docs/10 mode 1 requires excluding "API secrets", not credential metadata. Net effect: a config-only backup cannot restore routing priorities/weights, and the reviewed-and-closed R-8 decision is not what ships.

Recommendation:
1. Either (a) include credential rows (minus real secret refs) in config-only export and import them as `'incomplete'` per R-8 — placeholder `secret_ref` values must be minted unique (F-1) and must pass `SecretRef::parse` (charset `[A-Za-z0-9._-]`, ≤256 chars, and *not* resolve to a real vault item), or (b) keep the exclusion but declare the deviation explicitly in the plan + DECISION_LOG/R-8 note and add the loss of credential metadata to the export UI copy.
2. Whichever is chosen: the status literal is `'incomplete'` (lowercase — schema CHECK, 0001_init.sql:35-36); the plan's "`INCOMPLETE`" casing would violate the CHECK and fail every insert. Fix the casing in the plan.

### F-8 — MEDIUM / test adequacy (docs/11 mandatory scenario) — cross-platform restore is asserted ("algorithm is platform-independent") rather than evidenced

Evidence:
- docs/11 Mandatory scenarios include "cross-platform restore". The plan: "Encrypted restore on macOS deferred to CI/owner (ADR-016) — algorithm is platform-independent (pure Rust)."
- The platform-dependent part of encrypted restore is exactly what the roundtrip touches: vault re-store (Keychain on macOS CI) and text/file handling — not the cipher. ADR-016 already establishes that a green macOS CI run is acceptable evidence; the plan just doesn't schedule the test.

Recommendation: add to the CI matrix (ADR-013 runs windows + macos already): the encrypted roundtrip test (export → wipe → import → re-store via the OS store → retrieve matches) parameterized over `OsSecretStore` on both runners, per the existing vault contract pattern (ADR-016). Report states which platform executed it. Manual owner verification remains optional extra, not the plan of record.

### F-9 — LOW-MEDIUM / feasibility (docs/02 transactions) — the staging transaction requires new repo plumbing the plan doesn't acknowledge, and vault re-store ordering inside it is unpinned

Evidence:
- "single SQL transaction for all inserts" — feasible with sqlx (`pool.begin()`; `Transaction<'_, Sqlite>` is an `Executor`), but every existing `Repos` method binds `self.pool` directly (repos.rs), so none can participate in a caller-owned transaction; `Db` exposes no `begin()` (lib.rs). The plan implies existing inserts get reused.
- docs/10's order is "transactional staging; re-store secrets in destination OS vault; commit" — i.e. re-store happens while the tx is open. `SecretStore::store` is a *blocking* keyring call (os.rs:81-89); issuing it inside the tx inside an async command stalls the Tokio worker and holds a SQLite write lock across N OS calls; a tx rollback after partial re-store leaves orphan vault items.
- Argon2id (64 MiB, t=3) is also CPU-bound blocking work (~0.1–1 s) that must not run on the async runtime directly.

Recommendation:
1. Plan states new tx-scoped insert functions (e.g. `fn insert_provider_tx(executor, …)` taking `&mut SqliteConnection`/the transaction) alongside the existing pool methods; no dynamic SQL (ADR-004).
2. Re-store secrets *before* opening the tx where possible (decrypt → store → collect refs → single tx that inserts rows with final refs; on tx failure, best-effort delete the freshly created vault items, reporting any leftovers as diagnostics — same precedent as the old-token cleanup in state.rs:137-143). If docs/10's literal order is kept, wrap keyring calls in `spawn_blocking` and document orphan cleanup.
3. Run Argon2/GCM/export-file writes via `tokio::task::spawn_blocking` from the Tauri commands; note it in the plan.

### F-10 — LOW / crypto & dependency precision — pin versions, AAD construction, nonce/tag placement, and a settings allowlist; document the key-commitment and GCM-misuse analysis

Evidence and recommendations (each small, but each will otherwise be improvised at implementation time):
1. **Versions**: `argon2 0.6.0` is current (updated 2025-12; deps base64ct ^1.7, blake2 ^0.11) — `Params::new(m_cost: u32 /* KiB */, t_cost: u32, p_cost: u32, output_len: Option<usize>)`, so 64 MiB = `m_cost = 65536`, `output_len = Some(32)`; `hash_password_into` needs the `alloc` feature. Note `aes-gcm` latest is now **0.11.1** (aead 0.6, `AeadInOut` API); the plan pins **0.10** (aead 0.5, `AeadInPlace::encrypt_in_place_detached(nonce, aad, &mut buf) -> Tag`). 0.10 is fine and stable — but ADR-007's evidence should be refreshed either way.
2. **Nonce generation**: aes-gcm 0.10's `generate_nonce` uses the *aead-reexported* rand_core 0.6, while the workspace locks rand_core 0.10 — version mismatch footgun. Simplest correct move: reuse the already-dependency `ring::rand::SystemRandom` (as `repos.rs` and `os.rs` already do) to fill 12 bytes and `Nonce::from_slice`. Drops the planned extra rand deps entirely.
3. **AAD**: "envelope metadata sans data" is underspecified. Define: AAD = serde serialization of a fixed-order metadata struct (format, format_version, app_version, schema_version, created_at, mode, sections, kdf{m_kib,t,p,salt}, nonce_b64, tag placement) — never a `serde_json::Value` map (unordered). Authenticated-but-unencrypted metadata prevents manifest desync; note that GCM-tag verification transitively binds the correct kdf params (wrong-params tampering = decryption failure, i.e. DoS-only, fail-fast is the benefit).
4. **Nonce/tag placement**: state explicitly `nonce_b64` (12 B, fresh OsRng/CSPRNG per export — fine at backup volumes) and where the 16 B GCM tag lives (appended to blob or `tag_b64` field). Random 96-bit nonces are acceptable here (manual, low-frequency exports; collision odds negligible); no counter management needed — say so once in the plan.
5. **Key commitment**: AES-GCM is not key-committing (Len et al. USENIX'21; Albertini et al. eprint 2020/1456). Not exploitable in this design: single key per file, decryption only happens locally on an explicit user action (no decryption oracle), and offline passphrase guessing is unaffected by commitment. Record this as considered-and-accepted in ADR-007 at Phase 10 lock-in.
6. **Advisories**: no open RustSec advisories on argon2/aes-gcm. RUSTSEC-2025-0009 (panic/DoS) affects **ring** < 0.17.12 — workspace locks ring 0.17.14, already patched; note it in the report. New transitive deps (blake2, base64ct, aes, ghash, cipher) require the SECURITY_RULES #3 provenance/maintenance/license check at implementation, listed in the phase report.
7. **Settings export = allowlist**: config-only export of app_settings must be an explicit key allowlist (`gateway`, `retention_days`, `openrouter_attribution`), never "all keys minus gateway_token_ref" — future setting keys may carry refs/sensitive values and a denylist silently exports them.
8. **Defense-in-depth sweep at runtime**: the no-secret sweep is currently only a test. Add a cheap runtime scan before writing a config-only export (reject if any vault value or `sk-`-like material appears in the serialized payload — mirror of `profile_apply.rs:83-88`) so the docs/10 "never export plaintext secrets" rule is enforced by construction, not just by test fixture choices.

## Verified as satisfactory (no findings)

- **JSON container**: `.sair` as a JSON envelope satisfies docs/10 "versioned data container, never executable content" — content is parsed as data, nothing executed, no paths inside the envelope (path-traversal surface n/a); size cap + serde_json recursion limits cover oversized/deep files.
- **Config-only secret exclusion (current keys)**: verified against code — `gateway_token_ref` excluded (state.rs:33); remaining settings keys (`gateway` port/limits, `retention_days`, `openrouter_attribution`) contain no secrets; agent profile `env_json`/generated content uses the `<<GATEWAY_TOKEN>>` placeholder by construction (sair-domain/src/profiles.rs:17,64,87; profile_apply.rs rejects `sk-`-like material). The destination always generates its own gateway token (state.rs `ensure_token`), satisfying docs/10's "new gateway token on destination" default; encrypted mode inherits the config payload, so it cannot leak the token ref either.
- **Wrong-passphrase UX**: GCM auth failure mapped to an explicit message; import applies no passphrase policy (correct — policy is an export-time concern).
- **INCOMPLETE routing behavior**: `status != 'active'` candidates are excluded from routing with a `credential_{status}` reason (repos.rs:626-628), so `'incomplete'` imported credentials are inert until re-stored — works with existing code unchanged.
- **Test list vs docs/10/11**: tampered (bit flip), oversized, schema-validation failures, conflict-skip, INCOMPLETE states, config-only secret sweep are all present and match the mandatory scenarios (cross-platform restore handled in F-8).

## Crypto verification (web, 2026-10-31)

- argon2 0.6.0 (docs.rs): `Argon2::new(Algorithm, Version, Params)`; `Params::new(m_cost /*KiB*/, t_cost, p_cost, Option<output_len>) -> Result`; `hash_password_into(pwd, salt, &mut [u8])` under `alloc`. Matches the plan's usage with `m_kib = 65536, t = 3, p = 1, out = 32`.
- aes-gcm: 0.10.3 (aead 0.5, `AeadInPlace::encrypt_in_place_detached`) as pinned; 0.11.1 (aead 0.6, `AeadInOut`) is the current line — see F-10.1. 96-bit nonce via `U12` alias confirmed.
- Advisories: RUSTSEC-2025-0009 = ring DoS panic, patched ≥ 0.17.12; workspace at 0.17.14. No open advisories for argon2/aes-gcm/zeroize. Key-commitment literature reviewed — not applicable to this threat model (F-10.5).

## VERDICT: AMEND BEFORE IMPLEMENTATION — the container/crypto core is sound and docs/10-compliant in shape, but F-1 (natural-key conflict detection) and F-2 (backend re-store per docs/07 instead of user re-entry over IPC) must be resolved in the plan first, together with F-3/F-4; F-5–F-8 are required amendments; F-9/F-10 non-blocking precision items.
