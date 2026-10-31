# Phase 10 Plan — Backup/Restore (.sair)

Feature ID: PHASE-10. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-014 (versioned export, validation, preview, conflict resolution), docs/10 (all rules), ADR-007 (Argon2id + AES-256-GCM, RustCrypto), review R-8 (INCOMPLETE credentials on config-only import).

## Repository state
Phase 9 complete (2cb79a2): 132 tests green.

## Design

### 1. Container format (versioned, never executable)
`.sair` = JSON envelope (no custom binary): `{format: "sair-backup", format_version: 1, app_version, created_at, mode: "config"|"encrypted", data: ...}`.
- **Config-only** (`data` = cleartext JSON): providers (no credential rows), models, deployments, aliases, alias_deployments, routing policies, agent profiles, app settings EXCEPT gateway token ref. No secrets by construction.
- **Encrypted full** (`data` = base64): Argon2id (salt 16B, m=64MiB t=3 p=1 — ADR-007) derives a 32-byte key from the user passphrase; AES-256-GCM (12B nonce, AAD = envelope metadata sans data) encrypts the same config payload PLUS credentials (secret VALUES from the vault, wrapped inside the encrypted blob only).

### 2. Crypto (ADR-007, RustCrypto)
`argon2 0.6` (Argon2id v19) + `aes-gcm 0.10` (+ `rand_core`/`getrandom` for nonce via OsRng). KDF params recorded IN the envelope (`kdf: {m_kib, t, p, salt_b64}`) for future migration; version gate on load.

### 3. Export flow
- Config-only: direct from DB (providers/aliases/policies/profiles/settings-filtered).
- Encrypted: passphrase (UI-provided, min 12 chars enforced) → vault retrieve of every credential secret → payload → encrypt → write. Passphrase never stored; zeroized after use (SecretBytes pattern).

### 4. Import flow (docs/10: size check → validate → integrity → passphrase → decrypt → validate → preview → conflict → transactional staging → re-store → commit → report)
- Size cap 64 MiB; strict envelope/schema validation; encrypted: Argon2id + GCM verify (wrong passphrase = auth failure, explicit message).
- Preview: counts per entity + conflicts (existing ids → "skip" default; v1 conflict policy: SKIP existing ids, report skipped — no silent overwrite per docs/10).
- Transactional staging: single SQL transaction for all inserts (docs/02: no upstream awaits inside), rollback on any error.
- Imported credentials (encrypted mode): status `INCOMPLETE` — secret stays INSIDE the encrypted blob only until the user re-enters per credential (R-8); re-store only happens if the user supplies secrets at import (UI collect step) — else INCOMPLETE with limitation text.

### 5. IPC/UI
`export_backup(path, mode, passphrase) -> counts`; `import_backup(path, passphrase, collect_secrets) -> preview`; `commit_import(preview_id, secrets: {credential_label: value}) -> report`. Preview/commit two-step with an in-memory preview token (state, not persisted). UI: Backup & Restore screen — export mode + file dialog (Tauri dialog plugin NOT added; use a path text input v0 — file-dialog plugin is a post-v1 add, documented), passphrase fields (masked), preview table, commit.

### 6. Tests
Roundtrip config + encrypted (export→wipe→import→verify entities), wrong-passphrase rejection, tampered envelope rejection (bit flip), oversized rejection, conflict-skip behavior, INCOMPLETE credential states, no-secret sweep of config-only exports, schema validation failures.

## Acceptance
All tests + suites green; report + traceability (FR-014) + push. Encrypted restore on macOS deferred to CI/owner (ADR-016) — algorithm is platform-independent (pure Rust).

## Review resolution (2026-10-31) — verdict: AMEND BEFORE IMPLEMENTATION (resolved)

Report: `audits/planning/phase10-plan-review.md` (external verifications: argon2 0.6 API, aes-gcm 0.11.1, ring RUSTSEC-2025-0009 already patched in 0.17.14). All incorporated:

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | Conflict detection by NATURAL keys: aliases by `alias`, policies by `name`, models by `(provider_id, model_id)` via remapped provider ids, deployments by composite unique index, credentials by `(provider_id, label)`. Skip-on-conflict with a per-entity skipped report. |
| F-2 | High | Encrypted import: the backend re-stores decrypted secrets into the destination vault AT COMMIT (docs/07 decrypt-in-memory-and-re-store). No secrets through IPC. INCOMPLETE is an explicit user opt-out only (skip re-store). |
| F-3 | Med-High | Decrypted payload held as `SecretBytes` in app state with a 10-minute TTL; re-decrypted at commit if expired (passphrase re-prompt). Never plain `String`. |
| F-4 | Med-High | `provider_accounts` added to the payload (encrypted mode), inserted BEFORE credentials; FK insert order pinned: providers → accounts → credentials → models → deployments → aliases/policies → links → profiles. |
| F-5 | Medium | Envelope gains `schema_version`, `included_sections` list, and `integrity: {sha256}` over the canonical data field for BOTH modes (config-only integrity = checksum, not authenticity — documented). |
| F-6 | Medium | Export UI states the lost-passphrase warning verbatim (docs/10 + ADR-007). |
| F-7 | Medium | Config-only export INCLUDES credential rows (metadata: label/priority/weight/account) with `status='incomplete'` (lowercase, schema CHECK) and NO secret_ref — matches R-8; secrets only in encrypted mode. |
| F-8 | Medium | Encrypted roundtrip + vault re-store added to the CI matrix (ADR-016); local Windows evidence + macOS CI evidence recorded; algorithm is pure Rust. |
| F-9 | Low-Med | `Db::begin_tx()` wrapper; a tx-scoped insert module takes `&mut Transaction`; keyring re-store AFTER the tx commits (order: commit DB → re-store vault → delete INCOMPLETE flags); Argon2/GCM run in `spawn_blocking`. |
| F-10 | Low | aes-gcm bumped to 0.11.1 (ADR-007 evidence refreshed); nonce from `ring::rand::SystemRandom`; AAD = fixed-order struct (format, version, mode, created_at, kdf params) serialized deterministically; nonce+tag concatenated per GCM convention and stored base64 in the envelope; settings export = explicit key allowlist; GCM non-key-commitment documented as considered-and-accepted; cargo audit re-run gates the phase. |

Status: READY FOR IMPLEMENTATION.
