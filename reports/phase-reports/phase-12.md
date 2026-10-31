# Phase 12 Report — Cloud auto-backup (FR-024, ADR-019)

**Status: PASS** (verified locally on macOS arm64; CI run on push)

## What shipped

Opt-in cloud sync of the existing `.sair` envelope (no new crypto — Phase 10
envelope, audit-hardened) to user-owned free-tier backends:

- **Neon** (primary, SQLx Postgres, `sslmode=require`, pooled host) and
  **Supabase** (PostgREST, pure HTTPS) behind one dyn-safe
  `CloudBackupProvider` trait (pinned boxed futures — no new direct deps).
- **Schedule** (plan §3.3): 60 s tick, daily-forced upload, on-change debounce
  (dirty flag + 5 min quiet + ≥30 min interval); spawned AFTER `app.manage`;
  failures warn + surface in status, never block the gateway. `mark_cloud_dirty()`
  is called from all 20 mutating management commands.
- **Encrypted mode default**: the backup passphrase is stored in the OS vault at
  configure time (never uploaded — review F-12-1 resolution); config-only mode
  never carries secrets. Destination connection string/key: OS vault only.
- **Restore** reuses the UNCHANGED path-keyed preview/commit flow via a temp
  `.sair` file (review F-12-2 resolution); validated by the existing
  `parse_envelope` before anything touches disk; temp files swept at startup.
- **DSN-free errors**: sqlx/reqwest failures are mapped to messages that cannot
  contain the connection string (review F-12-6); field-name redaction extended.
- **Invariant**: cloud settings keys can never join the backup-export allowlist
  (tested — review F-12-8).
- UI: Cloud section in Backup & Restore — configure (consent dialog), test,
  status (last upload/error/dirty), snapshot list across devices, restore with
  passphrase field for encrypted snapshots, back-up-now, disable (+ optional
  cloud purge).

## Commands and results (macOS arm64)

| Command | Result |
|---|---|
| `cargo test --workspace` | **149 passed, 0 failed** (was 142; +7 new cloud tests) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `npm run typecheck` / `npm test` / `npm run build` (apps/desktop/ui) | clean / 2 passed / ok |

New tests: `schedule_decision_matches_plan` (clock-injection of the daily/
debounce rules), `neon_dsn_rules_are_enforced` (TLS-required, scheme checks),
`connect_errors_never_contain_the_dsn` (seeded fake DSN asserted absent from
errors), `in_memory_provider_roundtrips_envelope` (upload/list/download/prune/
purge + failure injection), `supabase_provider_contract_roundtrip` (wiremock
PostgREST: headers, byte-exact payload, DELETE scoping),
`supabase_url_rules_are_enforced` (https-only outside loopback),
`cloud_settings_never_exported` (allowlist invariant).

## Honest residuals

- The live Neon test is documented but intentionally not added this phase
  (`#[ignore]` + `SAIR_LIVE_NEON_URL` mechanism pinned in the plan) — first real
  connection happens with the owner's own Neon project.
- Supabase `created_at` ordering relies on the server default; list ordering is
  `order=created_at.desc` per the PostgREST contract (contract-tested).
- Cloudflare D1 + GitHub Gist targets remain backlog (plan P12-T06 optional —
  not started this phase).
- The scheduler tick is in-process: uploads happen only while the app runs
  (consistent with the app's local-only posture; docs/07 note added).

## Gate

PASS per `.agent/QUALITY_GATES.md` (commands above; no self-waiver — all gates
executed locally, CI run on push provides Windows/macOS evidence).
