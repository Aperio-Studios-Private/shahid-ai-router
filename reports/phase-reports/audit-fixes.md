# Audit Fixes Report — independent audit remediation (2026-10-31)

**Status: PASS** (verified locally on macOS arm64; see exact commands below).
Scope: an independent full-repository audit (11 reviewer agents; every source, doc, report and
board file) verified the Phase 1–11 evidence and found defects the gates had missed. All P1 and
P2 findings were fixed; findings are catalogued in `tasks/COMPLETED.md` (AF-01..AF-13) and
summarized in `CHANGELOG.md` [Unreleased].

## Commands and results (macOS 14 arm64, rustc stable-aarch64, Node 24)

| Command | Result |
|---|---|
| `cargo test --workspace` | 142 passed, 0 failed (local baseline measured 140 — the phase-11 claim of 139 undercounts by one; +2 new tests: backup roundtrip, KDF-bound; the mid-stream test was rewritten in place, not added) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `cargo check -p shahid-ai-router` (app crate, macOS) | compiles (was broken: `generate_context!` rejected non-RGBA icons — fixed, AF-13) |
| `npm run typecheck` (apps/desktop/ui) | clean |
| `npm test` | 2 passed (vitest) |
| `npm run build` | production build ok |

## What was fixed (P1)

1. **UI data-entry flows** (`ProvidersScreen.tsx`, `AliasesScreen.tsx`): snake_case invoke args →
   camelCase per the Tauri v2 contract; required `structuredOutput` added; read-path errors surfaced.
2. **Backup restore path** (`backup.rs`, `backup_import.rs`, `commands_rotation.rs`): config-mode
   payloads are now held for commit (encrypted-only dead end removed); deployments restore via the
   models natural name (`model` key); aliases export `policy_name` + `bindings[]` and import restores
   policy links and bindings (duplicate-guarded); settings + agent_profiles staged on import; real
   conflict report (natural keys compared against the live DB); commit no longer re-reads the file
   (mode captured at preview — TOCTOU removed).
3. **Unbounded upstream buffering** (`http.rs::read_body_bounded`): all four buffered adapter paths
   (openai chat, openai non-SSE stream fallback, anthropic messages, gemini generate) read
   incrementally with the 32 MiB cap enforced during the read; anthropic count_tokens now bounded too.
4. **Timeout retryability contract** (`sair-domain/src/error.rs`): `Timeout` removed from
   `is_safely_retryable`; test updated to assert the ADR-017 semantics.
5. **CI audit job** (`.github/workflows/ci.yml`): `security` job added (cargo-audit + npm audit) —
   the phase-11 evidence described exactly this job; the workflow now matches the claim.
6. **Mid-stream-drop test** (`api_phase11_tests.rs`): real raw-TCP scenario — SSE upstream emits 2
   frames then drops; asserts 200 + SSE headers + first frame relayed, exactly ONE upstream hit
   (AtomicUsize counter), credential still active with no cooldown.
7. **Non-Windows build**: placeholder icons regenerated as RGBA PNGs; missing `icon.icns` generated
   (`scripts/make-placeholder-icons.mjs`) — `tauri::generate_context!` compiled on macOS for the first time.

## What was fixed (P2 — selection)

- Family-faithful vault-error envelope on Anthropic/Gemini surfaces; credential dedupe inserted only
  after a deployment permit is acquired (busy-skip no longer poisons shared credentials); malformed-ref/
  missing-secret credentials rotate on instead of aborting the alias; correlation id surfaced as
  `shahid.request_id`; no-route records carry the real streamed flag; retry budget floored at 1 s.
- Drain window derives from `stop(grace)` via the shutdown channel (was hardcoded 30 s); in-flight
  counter wrapped in an RAII `InFlightGuard` (Drop-safe decrement); concurrency guard moved inside
  token auth; gateway token bytes zeroize-on-drop (`Zeroizing`); DEBUG `eprintln!` removed.
- Resolver: `getaddrinfo` via `spawn_blocking`; `localhost` by name must resolve loopback to inherit
  the cleartext exception; HTTP-date `Retry-After` parsed (chrono) in all adapters; Gemini reads the
  standard header and rounds fractional `retryDelay` up; Gemini model path segment percent-encoded.
- Profile-apply hash precondition is now REQUIRED when the target exists (backend-enforced, not
  UI-convention); profile JSON escapes aliases (serde_json); generic-profile curl example uses the
  documented placeholder.
- Exclusion "why unavailable" rows resolve each excluded credential's label via an id→label map (was
  the record's own label); telemetry sensitive-field list extended; verification date computed, not hardcoded.

## Honest residuals (unchanged or newly recorded)

- CI execution still requires the owner's billing fix (B-01); the new audit job's first run is pending.
- macOS runtime verification (Keychain/restore/app run) remains an owner action — note the workspace
  now compiles and its full test suite passes on macOS, which was previously impossible (icon bug).
- Streamed token counts remain FR-011 residual (post-v1).
- All 8 CycloneDX SBOMs embed the generating machine's Windows username/path and a windows-msvc
  triple; they should be regenerated on the release machine before publishing.
- ADR-018/latency-aware: documented as post-v1 in docs/05 (no 501 contract added — schema CHECK
  constrains strategy values; changing it would need a table-rebuild migration, judged not worth the risk pre-release).

## macOS vault verification (ADR-016 partial close, 2026-10-31)

`cargo test -p sair-vault --features os` against the **real macOS Keychain**: 8/8 passed, including
`os_store_passes_contract` (store/retrieve/delete roundtrip via keyring 4.2 apple-native store),
`availability_probe_returns`, and the malformed-ref rejection test. Keychain residue check after the
run: zero `shahid-ai-router-tests` entries remain (`security find-generic-password` / dump-keychain
both negative). This is the first runtime macOS vault evidence — the interactive owner verification
(full app run + encrypted restore) remains open but the vault layer itself is now proven on macOS.

## Release artifacts (2026-10-31)

- Release binary rebuilt on macOS arm64: `target/release/shahid-ai-router` (17.67 MB, 2m49s build),
  SHA-256 `bd8d1b61…db02319` recorded in `docs/sbom/release-checksum-sha256.txt` (the pre-audit
  Windows checksum is retained but marked STALE — rebuild required before tagging).
- All 7 CycloneDX SBOMs regenerated on the current machine (`cargo cyclonedx --all --format json`):
  the developer-machine paths and wrong target triple from the original pack are gone; `docs/sbom/`
  copies refreshed. SBOMs now reflect the macOS build dependency graph.
- One environment note in `audits/planning/planning-audit.md` redacted (local username/path and an
  unrelated project name) ahead of repository publication; annotation marks the redaction.

## Environment note

This verification ran on macOS with a fresh rustup stable toolchain. The Windows release binary and
checksum referenced in §3 predate these fixes and must be rebuilt before any release tag.
