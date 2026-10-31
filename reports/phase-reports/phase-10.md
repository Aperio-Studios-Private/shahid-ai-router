# Phase 10 Report — Backup/Restore

Feature ID: PHASE-10. Date: 2026-10-31. Status: **PASS** (local evidence; CI blocked per B-01).

## Requirements delivered
FR-014 (versioned export, validation, preview, conflict resolution), docs/10 rules, R-8 (INCOMPLETE credentials), ADR-007 (evidence refreshed to aes-gcm 0.11.1 — F-10).

## Delivered
1. **Container**: versioned JSON envelope — `format`/`format_version`/`schema_version`/`included_sections`/`integrity` (sha256 over canonical data, BOTH modes per F-5) / `kdf` params recorded / `data`. Size cap 64 MiB. Never executable content.
2. **Config-only mode**: providers, accounts, credential METADATA (status `incomplete`, no secret_ref — F-7), models, deployments, policies, aliases, agent profiles, settings via explicit key allowlist (token refs can never appear).
3. **Encrypted mode**: Argon2id (m=64 MiB, t=3, p=1; params in envelope) + AES-256-GCM (metadata AAD as a fixed-order struct — F-10; nonce from OS CSPRNG, nonce‖ciphertext‖tag layout). Secrets exist ONLY inside the authenticated ciphertext (verified by test).
4. **Import**: two-step preview→commit. Preview: structure/integrity checks, per-entity counts. Staging: ONE SQL transaction (F-9), **natural-key conflict skipping** (F-1 — alias/policy-name/provider+model/provider+label), FK insert order (F-4). Commit: backend **re-stores decrypted secrets into the destination vault** (F-2 — docs/07 decrypt-in-memory-and-re-store; NO secrets through IPC), held payload is a zeroized `SecretBytes` with 10-minute TTL (F-3). INCOMPLETE opt-out for skipped re-store.
5. **UI language**: lost-passphrase warning verbatim (F-6): "Losing the passphrase means the backup is UNRECOVERABLE (docs/10)."

## Verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **135 passed, 0 failed** (new: config roundtrip + tamper-integrity, encrypted roundtrip + wrong-passphrase AuthFailed + ciphertext-tamper rejection + no-secret-outside-ciphertext, size cap)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; fmt clean; tsc clean; vitest 2/2; build ok
- `cargo audit` (Phase 2 gate) re-verified: ring 0.17.14 already carries the RUSTSEC-2025-0009 patch (F-10)

## Honest scope statements
- Cross-platform encrypted restore: algorithm is pure Rust (platform-independent), but **CI evidence is pending B-01** (F-8) — recorded in traceability, macOS CI matrix ready.
- Backup file-dialog (native picker) deferred — path input v0 (documented post-v1 add).
- The raw-TCP/streaming hardening scenarios from Phase 7 F-4 remain Phase 11.
- UI for import is functional but minimal (path input + preview table + commit with re-store opt-out).

## Next eligible
Phase 11 (Hardening/release: perf budget, final security review, clean-install checklist, SBOM) — entry per workflow.
