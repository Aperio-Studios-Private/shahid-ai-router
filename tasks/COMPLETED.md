# Completed

Archive of finished implementation tasks (format per `.agent/TASK_EXECUTION_PROTOCOL.md`).

## 2026-10-31 — Post-audit fixes (audit-fixes report: reports/phase-reports/audit-fixes.md)

An independent full-repository audit (11 reviewer agents, every file) verified the phase-1..11
evidence and found defects the phase gates had not caught. All P1/P2 findings were fixed and
verified locally (cargo test/clippy/fmt + tsc/vitest/vite build on macOS):

| ID | Task | Objective / acceptance | Status |
|---|---|---|---|
| AF-01 | UI IPC arg casing | add_provider / add_model / add_deployment / bind_alias_deployment send camelCase args (Tauri v2 contract); required `structuredOutput` supplied | PASS |
| AF-02 | Backup restore correctness | config-mode import can commit; deployments restore via natural model name; alias policy links + bindings restore; settings + agent_profiles staged; real conflict report | PASS (roundtrip regression test `config_export_roundtrips_through_import`) |
| AF-03 | Backup crypto hardening | attacker-supplied KDF params bounded before Argon2id derive | PASS (`kdf_params_from_the_file_are_bounded`) |
| AF-04 | Bounded upstream reads | incremental 32 MiB cap on all four buffered adapter paths incl. count_tokens (no unbounded buffering) | PASS |
| AF-05 | Timeout retryability contract | `Timeout.is_safely_retryable() == false`, matching ADR-017 timeout-never-rotates | PASS |
| AF-06 | Gateway rotation-loop fixes | family-faithful vault-error envelope; credential dedupe only after permit; broken credentials rotate on; correlation id surfaced as `shahid.request_id`; no-route records carry the real streamed flag | PASS |
| AF-07 | Gateway lifecycle fixes | drain window = caller's grace (was hardcoded 30 s); in-flight counter RAII guard (no leak on early drop); concurrency permit acquired after auth; DEBUG eprintln removed; token bytes zeroize-on-drop | PASS |
| AF-08 | Mid-stream-drop test | real raw-TCP test: partial frames relayed, exactly one upstream hit, credential untouched (was a zero-assertion stub) | PASS |
| AF-09 | CI audit job | `security` job (cargo-audit + npm audit) added so phase-11 evidence claims match the pipeline | PASS (file complete; execution needs B-01 billing fix) |
| AF-10 | SSRF resolver | DNS off the async workers (spawn_blocking); `localhost` by name requires resolved loopback; HTTP-date Retry-After parsed (all adapters); Gemini fractional retryDelay rounds up; Gemini model path segment encoded | PASS |
| AF-11 | UI hardening | read-path error handling everywhere; Logs debounce + sequence guard; success states distinct from errors; Date-parsed cooldown compare; a11y labels; Backup + About screens implemented (9 more IPC commands wired) | PASS |
| AF-12 | Bookkeeping | ADR-007 → Accepted; PRD FR-023 amendment note; docs/05 v1-status notes; docs/08 fold reconciled; ROADMAP status; HANDOFF corrections; boards repaired (literal `\n` artifacts removed) | PASS |
| AF-13 | Icons | placeholder icons regenerated as RGBA PNGs + icon.icns generated (non-Windows `tauri::generate_context!` builds were broken) | PASS (workspace compiles on macOS) |

Earlier: Phases 1–11 implementation tasks — see `tasks/BACKLOG.md` tables and
`reports/phase-reports/phase-1..11.md` for gate evidence.

## 2026-10-31 — Phase 12: Cloud auto-backup (FR-024, ADR-019)

| ID | Task | Status |
|---|---|---|
| P12-T02 | `cloud_backup.rs` — dyn-safe provider trait, Neon (SQLx Postgres) + Supabase (PostgREST) impls, envelope builder (spawn_blocking), DSN-free errors, schedule predicate | PASS |
| P12-T03 | Scheduler (spawn-after-manage, 60 s tick, daily+debounce), 8 IPC commands, 20 `mark_cloud_dirty()` call sites, vault-backed DSN/key/passphrase | PASS |
| P12-T04 | Backup & Restore → Cloud section (consent dialog, status, snapshots, restore via unchanged preview/commit, disable+purge) | PASS |
| P12-T07 | 7 new tests incl. DSN-leak sweep, Supabase wiremock contract, export-allowlist invariant | PASS |
| P12-T08 | ADR-019 + PRD FR-024 + docs/06/07 + traceability + CHANGELOG/README | PASS |
