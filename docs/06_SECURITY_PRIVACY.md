# Security & Privacy

## Threat model
Credential leakage, malicious local clients, custom URL SSRF, compromised dependencies, unsafe imports, log leakage, unauthorized LAN access and unsafe agent integrations.

## Secrets
Store secrets only in OS secure storage. SQLite stores opaque references. Never log, commit, export plaintext, include in crash reports or send to analytics. Mask in UI. Clear buffers where practical.

## Gateway
Bind to 127.0.0.1 by default; require high-entropy local token; no wildcard CORS; enforce request/body/concurrency/time limits; keep admin endpoints separate. LAN mode, if ever added, requires explicit opt-in and strong warnings/authentication.

## SSRF
HTTPS by default. HTTP only for explicit local development. Block private, loopback, link-local and metadata destinations for remote custom URLs, with narrow allowlist for explicit local endpoints. Revalidate DNS and redirects. Prevent auth-header forwarding to untrusted redirect targets. Reject CRLF/header injection and non-HTTP schemes.

## Tauri
Least-privilege capabilities, narrow validated commands, restrictive CSP, no secrets in webview storage, review plugin permissions and remote content.

## Import
Strict size/file/path/schema limits; defend against path traversal and decompression bombs; authenticate encrypted backups; stage changes transactionally; never execute imported content.

## Privacy
No account, cloud sync or telemetry by default. Prompt/response bodies are not logged by default. Network requests only for configured providers and explicitly enabled update/diagnostic features.

## Threat-model addendum (Phase 2, 2026-10-31 — review finding F-12)
New surfaces introduced by the security/persistence phase, with their mitigations:
- **Local log files** — plaintext logs on disk are a leak vector. Mitigations: redaction-by-construction (secret values never enter spans), field-name rewrite layer (`[REDACTED]`) as defense-in-depth, logs under OS per-user app-data (inheriting user ACLs), daily rotation with 14-day pruning, no request/response bodies ever written, sweep tests asserting seeded key material never appears in any sink.
- **Token-reveal IPC command** — the UI can obtain the gateway token on demand. Mitigations: masked by default with explicit reveal, never persisted in webview storage, never logged (sweep-tested), regeneration is consent-gated with client-impact warning, and old vault items are deleted after a crash-safe swap (create → persist ref → delete old).
- **Degraded-start states** — vault or DB unavailable at startup. Mitigations: fail-closed (the gateway never starts without its token from the vault; startup aborts before bind on migration failure); explicit error state surfaced in the UI; no silent plaintext fallback (docs/06 Secrets).
- **Malicious local clients** (in-scope since Phase 0) — added concrete bounds: immediate 503 at concurrency capacity (no unbounded queueing), per-request timeout budget (408), body size limit (413), Host allowlist, auth on every route including `/healthz` and 404s.
- **Residual risks (documented, revalidated at Phase 11):** SSRF resolve-to-connect race minimized via validating-resolver design (ADR-015) but not provably eliminated; DNS-rebinding defended by Host allowlist for the *inbound* path.
