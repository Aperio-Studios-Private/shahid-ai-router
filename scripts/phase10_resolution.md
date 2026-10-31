
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
