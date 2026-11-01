# Security Review — post-Phase 16 hardening pass (2026-11-01)

Frame: `security-and-hardening` skill — threat model first, STRIDE per boundary,
then control verification. Scope: every surface added or changed since the
Phase 2 security review (cloud backup, settings IPC, model fetch, wizard,
devtools regression).

## Trust boundaries & STRIDE verdicts

| Boundary | Threats | Controls verified in code | Verdict |
|---|---|---|---|
| Gateway HTTP (127.0.0.1:8787) | S/T/I/D/E | Constant-time token (subtle), Host allowlist, 32 MiB body cap, concurrency 503 (no queueing), 408 timeout, auth on /healthz+404s, no CORS, no devtools | PASS (unchanged from Phase 2/11 reviews) |
| Tauri IPC (webview→Rust) | I/D/E | New `save_settings` is allowlisted + validated (port/body-limit/retention bounds; consumption-side floors for timeout/concurrency); `get_public_settings` returns only tuning keys — token/DSN refs are separate keys never returned | PASS |
| Upstream fetch (`fetch_provider_models`) | S/I/D | SsrfCheckedClient only, scheme+loopback policy, 20 s timeout, 8 MiB bounded read, status-checked, DSN-free errors; upstream JSON parsed as data, model ids are React-escaped text (no innerHTML) | PASS |
| Gemini URL building | T | Model id percent-encoded into path segment (no path/query reshape) | PASS |
| Cloud backup transport | I/T | Envelope-only (AES-256-GCM or secret-free config), DSN/key/passphrase vault-only, DSN-free errors, export-allowlist invariant tested; cloud settings keys can never enter backups | PASS |
| Backup import | T/D | parse_envelope caps+integrity before disk; temp files under app-data/tmp, swept at startup; KDF bounds on attacker-supplied params | PASS |
| Secrets | I | OS vault only; vault-store/retrieve via spawn_blocking; sweep tests assert token absence in all sinks | PASS |
| Supply chain | — | CI runs cargo-audit + npm audit (0 known vulns tracked); UI deps have one install-script (fsevents — Apple-signed optional macOS watcher, standard) | PASS (note) |
| Privacy | I | Request records are metadata-only w/ 30-day retention (now editable in Settings); prompts/responses never stored; cloud sync never includes records | PASS |

## Findings

**F-16-1 (Medium, FIXED)** — `devtools` cargo feature had been enabled in
release builds during the white-screen debugging session, exposing the web
inspector in production. Removed along with the auto-open call; the global
error overlay (crash + capture-phase) remains the support diagnostic.

**F-16-2 (Informational)** — `fsevents` (macOS file-watcher, transitive via
Vite) is the only UI dependency with an install script; it is Apple's official
API wrapper and does not run in the shipped app (build-time only).

**F-16-3 (Informational)** — gateway rate limiting is by concurrency bound
(503) rather than request-rate; acceptable for a single-user loopback gateway
with one shared token, re-examine if LAN/multi-user mode is ever added.

## Residuals (documented, unchanged)
- SSRF resolve-to-connect race (ADR-015) — minimized, not provably eliminated.
- Telemetry field redaction is exact-match; DSN-free error mapping is the
  primary control on cloud paths.
- Cloud destinations are user-owned; the app trusts the envelope crypto, not
  the destination.

## Verdict
PASS WITH FINDINGS — one regression fixed this pass, no open High/Critical.
