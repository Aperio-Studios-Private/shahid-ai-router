# Data Model & Storage

SQLite stores non-secret metadata; OS vault stores secrets. Suggested entities:
- providers
- provider_accounts (optional account/project grouping)
- credentials (secret_ref, label, status, priority, weight, cooldown, timestamps)
- models (provider-specific model ID and capability metadata)
- deployments
- model_aliases
- alias_deployments
- routing_policies
- agent_profiles
- request_records (redacted metadata)
- health_events
- app_settings
- schema_migrations

Use foreign keys, indexes, transactions and migrations. Never keep DB transactions open during upstream network calls. Test upgrades from previous schemas.

Secret lifecycle: accept secret in backend; store in OS vault; persist opaque reference; retrieve only for outbound request; delete vault item on credential deletion; on import decrypt in memory and re-store in destination vault.

Config data may additionally live at rest in the user's own cloud backend (Phase 12, ADR-019) — always as the versioned `.sair` envelope (encrypted or config-only); request records never leave the machine.

Default to metadata-only request history with configurable retention. Do not store prompt/response bodies unless a future explicit opt-in design includes retention and privacy controls.
