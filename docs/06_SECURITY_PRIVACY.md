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
