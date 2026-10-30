-- Shahid AI Router — initial schema (docs/07 entities; FK delete-rule matrix per phase2-plan F-6).
-- Timestamps are ISO-8601 UTC text (lexicographically sortable).
-- schema_migrations is satisfied by sqlx's `_sqlx_migrations` table.

PRAGMA foreign_keys = ON;

CREATE TABLE providers (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    provider_type TEXT NOT NULL CHECK (provider_type IN
                      ('openai_compatible', 'openai', 'openrouter', 'anthropic', 'gemini')),
    base_url      TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    notes         TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE provider_accounts (
    id          TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    label       TEXT NOT NULL,
    group_key   TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (provider_id, label)
);

CREATE TABLE credentials (
    id                  TEXT PRIMARY KEY,
    provider_id         TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    account_id          TEXT REFERENCES provider_accounts (id) ON DELETE SET NULL,
    label               TEXT NOT NULL,
    secret_ref          TEXT NOT NULL UNIQUE,
    status              TEXT NOT NULL DEFAULT 'active' CHECK (status IN
                            ('active', 'invalid', 'quarantined', 'incomplete')),
    priority            INTEGER NOT NULL DEFAULT 0,
    weight              INTEGER NOT NULL DEFAULT 1 CHECK (weight > 0),
    cooldown_until      TEXT,
    last_used_at        TEXT,
    last_error_category TEXT,
    last_error_code     TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE INDEX idx_credentials_provider_status ON credentials (provider_id, status);
CREATE INDEX idx_credentials_cooldown ON credentials (cooldown_until) WHERE cooldown_until IS NOT NULL;

CREATE TABLE models (
    id              TEXT PRIMARY KEY,
    provider_id     TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    model_id        TEXT NOT NULL,
    display_name    TEXT,
    protocol_family TEXT NOT NULL CHECK (protocol_family IN
                        ('openai_chat', 'anthropic_messages', 'gemini_generate_content')),
    capabilities    TEXT NOT NULL DEFAULT '{}',
    verified_at     TEXT,
    created_at      TEXT NOT NULL,
    UNIQUE (provider_id, model_id)
);

CREATE TABLE deployments (
    id                TEXT PRIMARY KEY,
    provider_id       TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    model_id          TEXT NOT NULL REFERENCES models (id) ON DELETE CASCADE,
    endpoint_override TEXT,
    enabled           INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_deployments_unique ON deployments (provider_id, model_id, COALESCE(endpoint_override, ''));
CREATE INDEX idx_deployments_model ON deployments (model_id);

CREATE TABLE routing_policies (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    strategy        TEXT NOT NULL CHECK (strategy IN
                        ('priority', 'round_robin', 'weighted', 'lru', 'manual_pin')),
    max_attempts    INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts >= 1),
    retry_budget_ms INTEGER NOT NULL DEFAULT 10000 CHECK (retry_budget_ms > 0),
    timeout_ms      INTEGER NOT NULL DEFAULT 120000 CHECK (timeout_ms > 0),
    cooldown_seconds INTEGER NOT NULL DEFAULT 60 CHECK (cooldown_seconds >= 0),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE model_aliases (
    id          TEXT PRIMARY KEY,
    alias       TEXT NOT NULL UNIQUE,
    description TEXT,
    policy_id   TEXT REFERENCES routing_policies (id) ON DELETE SET NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE alias_deployments (
    id            TEXT PRIMARY KEY,
    alias_id      TEXT NOT NULL REFERENCES model_aliases (id) ON DELETE CASCADE,
    deployment_id TEXT NOT NULL REFERENCES deployments (id) ON DELETE CASCADE,
    priority      INTEGER NOT NULL DEFAULT 0,
    weight        INTEGER NOT NULL DEFAULT 1 CHECK (weight > 0),
    UNIQUE (alias_id, deployment_id)
);

CREATE INDEX idx_alias_deployments_alias ON alias_deployments (alias_id);

CREATE TABLE agent_profiles (
    id               TEXT PRIMARY KEY,
    agent_id         TEXT NOT NULL,
    agent_version    TEXT,
    protocol_family  TEXT NOT NULL CHECK (protocol_family IN
                         ('openai_chat', 'anthropic_messages', 'gemini_generate_content')),
    endpoint         TEXT NOT NULL,
    auth_method      TEXT NOT NULL,
    model_alias      TEXT REFERENCES model_aliases (id) ON DELETE SET NULL,
    fast_alias       TEXT REFERENCES model_aliases (id) ON DELETE SET NULL,
    env_json         TEXT NOT NULL DEFAULT '{}',
    config_rules_json TEXT NOT NULL DEFAULT '{}',
    verified_at      TEXT,
    notes            TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);

-- Metadata only: no request/response bodies are ever stored (docs/07, ADR-008).
-- Selection references are nullable with ON DELETE SET NULL so history and
-- retention never RESTRICT deletions; labels are denormalized for display.
CREATE TABLE request_records (
    id                TEXT PRIMARY KEY,
    ts                TEXT NOT NULL,
    correlation_id    TEXT NOT NULL,
    protocol_family   TEXT,
    operation         TEXT,
    alias             TEXT,
    deployment_id     TEXT REFERENCES deployments (id) ON DELETE SET NULL,
    credential_id     TEXT REFERENCES credentials (id) ON DELETE SET NULL,
    provider_label    TEXT,
    model_label       TEXT,
    deployment_label  TEXT,
    status            INTEGER,
    error_category    TEXT,
    latency_ms        INTEGER,
    tokens_in         INTEGER,
    tokens_out        INTEGER
);

CREATE INDEX idx_request_records_ts ON request_records (ts);
CREATE INDEX idx_request_records_correlation ON request_records (correlation_id);
CREATE INDEX idx_request_records_deployment ON request_records (deployment_id, ts);

CREATE TABLE health_events (
    id            TEXT PRIMARY KEY,
    ts            TEXT NOT NULL,
    subject_type  TEXT NOT NULL CHECK (subject_type IN ('provider', 'credential', 'deployment')),
    subject_id    TEXT NOT NULL,
    credential_id TEXT REFERENCES credentials (id) ON DELETE SET NULL,
    event         TEXT NOT NULL CHECK (event IN
                      ('ok', 'degraded', 'cooldown_start', 'cooldown_end',
                       'quarantine', 'invalid', 'recovered')),
    category      TEXT,
    detail        TEXT
);

CREATE INDEX idx_health_events_credential_ts ON health_events (credential_id, ts);
CREATE INDEX idx_health_events_subject ON health_events (subject_type, subject_id, ts);

CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
