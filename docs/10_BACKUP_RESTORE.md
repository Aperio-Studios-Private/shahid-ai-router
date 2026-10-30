# Backup & Restore

Suggested file extension: `.sair`. It is a versioned data container, never executable content.

Export modes:
1. Configuration-only: provider definitions, aliases, routing policies, agent profiles and settings; excludes API secrets and local gateway token.
2. Encrypted full backup: includes encrypted credential payloads protected by user passphrase and vetted KDF + authenticated encryption. Select exact algorithms/parameters only after security review.

Manifest: format ID/version, app version, schema version, timestamp, included sections, encryption metadata and integrity metadata. Never put secrets in manifest.

Import: size check; structure/manifest validation; integrity/authentication; passphrase; decrypt in memory; schema/reference validation; preview; conflict choice; transactional staging; re-store secrets in destination OS vault; commit; report skipped entries.

Never export plaintext secrets, silently overwrite, permit path traversal or execute imported content. Generate a new gateway token on destination by default. Lost passphrase means encrypted backup may be unrecoverable; state this clearly.
