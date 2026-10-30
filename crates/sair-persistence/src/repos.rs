//! Typed repositories over the Phase 2 schema (docs/07 entities). No method
//! holds a transaction across an upstream network await. Route-snapshot
//! loading assembles [`RouteCandidate`] pairs per ADR-018.

use std::collections::HashMap;

use sair_domain::protocol::ProtocolFamily;
use sair_domain::routing::{CapabilityMetadata, RouteCandidate};
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::{Db, DbError};

fn now() -> String {
    crate::time::now_iso()
}

fn new_id() -> String {
    // 16 random bytes hex from the OS CSPRNG (no extra dependency).
    use ring::rand::SecureRandom;
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes)
        .expect("OS CSPRNG must not fail; unrecoverable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Provider view (secrets never included).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderRow {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub enabled: bool,
    pub allow_loopback: bool,
    pub notes: Option<String>,
}

fn provider_from_row(row: &SqliteRow) -> ProviderRow {
    ProviderRow {
        id: row.get("id"),
        name: row.get("name"),
        provider_type: row.get("provider_type"),
        base_url: row.get("base_url"),
        enabled: row.get::<i64, _>("enabled") == 1,
        allow_loopback: row.get::<i64, _>("allow_loopback") == 1,
        notes: row.get("notes"),
    }
}

/// Credential view (secret_ref only — never the secret).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CredentialRow {
    pub id: String,
    pub provider_id: String,
    pub account_id: Option<String>,
    pub label: String,
    pub secret_ref: String,
    pub status: String,
    pub priority: i64,
    pub weight: i64,
    pub cooldown_until: Option<String>,
}

fn credential_from_row(row: &SqliteRow) -> CredentialRow {
    CredentialRow {
        id: row.get("id"),
        provider_id: row.get("provider_id"),
        account_id: row.get("account_id"),
        label: row.get("label"),
        secret_ref: row.get("secret_ref"),
        status: row.get("status"),
        priority: row.get("priority"),
        weight: row.get("weight"),
        cooldown_until: row.get("cooldown_until"),
    }
}

/// Deployment view joined with its model row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeploymentRow {
    pub id: String,
    pub provider_id: String,
    pub model_pk: String,
    pub model_id: String,
    pub protocol_family: String,
    pub capabilities: Value,
    pub endpoint_override: Option<String>,
    pub enabled: bool,
}

fn deployment_from_row(row: &SqliteRow) -> DeploymentRow {
    DeploymentRow {
        id: row.get("id"),
        provider_id: row.get("provider_id"),
        model_pk: row.get("model_pk"),
        model_id: row.get("model_id"),
        protocol_family: row.get("protocol_family"),
        capabilities: serde_json::from_str(&row.get::<String, _>("capabilities"))
            .unwrap_or(Value::Null),
        endpoint_override: row.get("endpoint_override"),
        enabled: row.get::<i64, _>("enabled") == 1,
    }
}

/// Alias view.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AliasRow {
    pub id: String,
    pub alias: String,
    pub policy_id: Option<String>,
}

pub struct Repos<'a> {
    pool: &'a SqlitePool,
}

impl Db {
    /// Typed repository access.
    #[must_use]
    pub fn repos(&self) -> Repos<'_> {
        Repos { pool: &self.pool }
    }
}

impl<'a> Repos<'a> {
    // ---- providers ----

    pub async fn list_providers(&self) -> Result<Vec<ProviderRow>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, provider_type, base_url, enabled, allow_loopback, notes
             FROM providers ORDER BY created_at",
        )
        .fetch_all(self.pool)
        .await?;
        Ok(rows.iter().map(provider_from_row).collect())
    }

    /// # Errors
    /// DB failures.
    pub async fn insert_provider(
        &self,
        name: &str,
        provider_type: &str,
        base_url: &str,
        allow_loopback: bool,
        notes: Option<&str>,
    ) -> Result<String, DbError> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO providers (id, name, provider_type, base_url, enabled, allow_loopback, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?7)",
        )
        .bind(&id)
        .bind(name)
        .bind(provider_type)
        .bind(base_url)
        .bind(i64::from(allow_loopback))
        .bind(notes)
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    pub async fn set_provider_enabled(&self, id: &str, enabled: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE providers SET enabled = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id)
            .bind(i64::from(enabled))
            .bind(now())
            .execute(self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_provider(
        &self,
        id: &str,
        base_url: &str,
        allow_loopback: bool,
        notes: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE providers SET base_url = ?2, allow_loopback = ?3, notes = ?4, updated_at = ?5 WHERE id = ?1",
        )
        .bind(id)
        .bind(base_url)
        .bind(i64::from(allow_loopback))
        .bind(notes)
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_provider(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM providers WHERE id = ?1")
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    // ---- credentials ----

    pub async fn list_credentials(&self, provider_id: &str) -> Result<Vec<CredentialRow>, DbError> {
        let rows = sqlx::query(
            "SELECT id, provider_id, account_id, label, secret_ref, status, priority, weight, cooldown_until
             FROM credentials WHERE provider_id = ?1 ORDER BY priority DESC, created_at",
        )
        .bind(provider_id)
        .fetch_all(self.pool)
        .await?;
        Ok(rows.iter().map(credential_from_row).collect())
    }

    /// # Errors
    /// DB failures.
    pub async fn insert_credential(
        &self,
        provider_id: &str,
        label: &str,
        secret_ref: &str,
        priority: i64,
        weight: i64,
    ) -> Result<String, DbError> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO credentials (id, provider_id, label, secret_ref, status, priority, weight, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7, ?7)",
        )
        .bind(&id)
        .bind(provider_id)
        .bind(label)
        .bind(secret_ref)
        .bind(priority)
        .bind(weight)
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    pub async fn set_credential_status(&self, id: &str, status: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE credentials SET status = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id)
            .bind(status)
            .bind(now())
            .execute(self.pool)
            .await?;
        Ok(())
    }

    /// Sets cooldown_until (RFC-3339) or clears it with `None`.
    pub async fn set_credential_cooldown(
        &self,
        id: &str,
        cooldown_until: Option<String>,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE credentials SET cooldown_until = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id)
            .bind(cooldown_until)
            .bind(now())
            .execute(self.pool)
            .await?;
        Ok(())
    }

    pub async fn touch_credential_used(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE credentials SET last_used_at = ?2 WHERE id = ?1")
            .bind(id)
            .bind(now())
            .execute(self.pool)
            .await?;
        Ok(())
    }

    /// Records the last classified failure for diagnostics (category + code).
    pub async fn touch_credential_error(
        &self,
        id: &str,
        category: &str,
        code: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE credentials SET last_error_category = ?2, last_error_code = ?3, updated_at = ?4 WHERE id = ?1",
        )
        .bind(id)
        .bind(category)
        .bind(code)
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_credential(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM credentials WHERE id = ?1")
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    // ---- models & deployments ----

    /// # Errors
    /// DB failures.
    pub async fn insert_model(
        &self,
        provider_id: &str,
        model_id: &str,
        protocol_family: &str,
        capabilities: &Value,
        display_name: Option<&str>,
    ) -> Result<String, DbError> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO models (id, provider_id, model_id, display_name, protocol_family, capabilities, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(&id)
        .bind(provider_id)
        .bind(model_id)
        .bind(display_name)
        .bind(protocol_family)
        .bind(capabilities.to_string())
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    pub async fn list_deployments(&self, provider_id: &str) -> Result<Vec<DeploymentRow>, DbError> {
        let rows = sqlx::query(
            "SELECT d.id, d.provider_id, m.id AS model_pk, m.model_id, m.protocol_family,
                    m.capabilities, d.endpoint_override, d.enabled
             FROM deployments d JOIN models m ON m.id = d.model_id
             WHERE d.provider_id = ?1 ORDER BY d.created_at",
        )
        .bind(provider_id)
        .fetch_all(self.pool)
        .await?;
        Ok(rows.iter().map(deployment_from_row).collect())
    }

    /// # Errors
    /// DB failures.
    pub async fn insert_deployment(
        &self,
        provider_id: &str,
        model_pk: &str,
        endpoint_override: Option<&str>,
    ) -> Result<String, DbError> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO deployments (id, provider_id, model_id, endpoint_override, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)",
        )
        .bind(&id)
        .bind(provider_id)
        .bind(model_pk)
        .bind(endpoint_override)
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    pub async fn set_deployment_enabled(&self, id: &str, enabled: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE deployments SET enabled = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id)
            .bind(i64::from(enabled))
            .bind(now())
            .execute(self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_deployment(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM deployments WHERE id = ?1")
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    // ---- aliases ----

    pub async fn list_aliases(&self) -> Result<Vec<AliasRow>, DbError> {
        let rows = sqlx::query("SELECT id, alias, policy_id FROM model_aliases ORDER BY alias")
            .fetch_all(self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| AliasRow {
                id: r.get("id"),
                alias: r.get("alias"),
                policy_id: r.get("policy_id"),
            })
            .collect())
    }

    /// # Errors
    /// DB failures.
    pub async fn insert_alias(
        &self,
        alias: &str,
        description: Option<&str>,
    ) -> Result<String, DbError> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO model_aliases (id, alias, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
        )
        .bind(&id)
        .bind(alias)
        .bind(description)
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    /// # Errors
    /// DB failures.
    pub async fn bind_deployment(
        &self,
        alias_id: &str,
        deployment_id: &str,
        priority: i64,
    ) -> Result<String, DbError> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO alias_deployments (id, alias_id, deployment_id, priority) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&id)
        .bind(alias_id)
        .bind(deployment_id)
        .bind(priority)
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    pub async fn delete_alias(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM model_aliases WHERE id = ?1")
            .bind(id)
            .execute(self.pool)
            .await?;
        Ok(())
    }

    // ---- routing policies ----

    /// Loads the default policy (name 'default'), creating it if absent with
    /// explicit, consistent values (ADR-017: schema defaults not relied on).
    ///
    /// # Errors
    /// DB failures.
    pub async fn ensure_default_policy(&self) -> Result<PolicyParams, DbError> {
        let row = sqlx::query(
            "SELECT strategy, max_attempts, retry_budget_ms, timeout_ms, cooldown_seconds
             FROM routing_policies WHERE name = 'default'",
        )
        .fetch_optional(self.pool)
        .await?;
        if let Some(r) = row {
            return Ok(PolicyParams {
                strategy: r.get("strategy"),
                max_attempts: r.get("max_attempts"),
                retry_budget_ms: r.get("retry_budget_ms"),
                timeout_ms: r.get("timeout_ms"),
                cooldown_seconds: r.get("cooldown_seconds"),
            });
        }
        let id = new_id();
        sqlx::query(
            "INSERT INTO routing_policies (id, name, strategy, max_attempts, retry_budget_ms, timeout_ms, cooldown_seconds, created_at, updated_at)
             VALUES (?1, 'default', 'priority', 3, 130000, 120000, 60, ?2, ?2)",
        )
        .bind(&id)
        .bind(now())
        .execute(self.pool)
        .await?;
        Ok(PolicyParams {
            strategy: "priority".to_string(),
            max_attempts: 3,
            retry_budget_ms: 130_000,
            timeout_ms: 120_000,
            cooldown_seconds: 60,
        })
    }

    // ---- request records & health events ----

    /// # Errors
    /// DB failures.
    pub async fn insert_request_record(&self, record: &RequestRecordInsert) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO request_records (id, ts, correlation_id, protocol_family, operation, alias,
                deployment_id, credential_id, provider_label, model_label, deployment_label,
                status, error_category, latency_ms, tokens_in, tokens_out, decision_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        )
        .bind(&record.id)
        .bind(&record.ts)
        .bind(&record.correlation_id)
        .bind(&record.protocol_family)
        .bind(&record.operation)
        .bind(&record.alias)
        .bind(&record.deployment_id)
        .bind(&record.credential_id)
        .bind(&record.provider_label)
        .bind(&record.model_label)
        .bind(&record.deployment_label)
        .bind(record.status)
        .bind(&record.error_category)
        .bind(record.latency_ms)
        .bind(record.tokens_in)
        .bind(record.tokens_out)
        .bind(&record.decision_json)
        .execute(self.pool)
        .await?;
        Ok(())
    }

    /// # Errors
    /// DB failures.
    #[allow(clippy::too_many_arguments)] // mirrors the health_events columns
    pub async fn insert_health_event(
        &self,
        subject_type: &str,
        subject_id: &str,
        credential_id: Option<&str>,
        event: &str,
        category: Option<&str>,
        detail: Option<&str>,
        correlation_id: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO health_events (id, ts, subject_type, subject_id, credential_id, event, category, detail, correlation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(new_id())
        .bind(now())
        .bind(subject_type)
        .bind(subject_id)
        .bind(credential_id)
        .bind(event)
        .bind(category)
        .bind(detail)
        .bind(correlation_id)
        .execute(self.pool)
        .await?;
        Ok(())
    }
}

/// Routing policy parameters (explicit values, ADR-017).
#[derive(Debug, Clone)]
pub struct PolicyParams {
    pub strategy: String,
    pub max_attempts: i64,
    pub retry_budget_ms: i64,
    pub timeout_ms: i64,
    pub cooldown_seconds: i64,
}

/// New request record (metadata only; decision JSON for FR-012).
#[derive(Debug, Clone, Default)]
pub struct RequestRecordInsert {
    pub id: String,
    pub ts: String,
    pub correlation_id: String,
    pub protocol_family: Option<String>,
    pub operation: Option<String>,
    pub alias: Option<String>,
    pub deployment_id: Option<String>,
    pub credential_id: Option<String>,
    pub provider_label: Option<String>,
    pub model_label: Option<String>,
    pub deployment_label: Option<String>,
    pub status: Option<i64>,
    pub error_category: Option<String>,
    pub latency_ms: Option<i64>,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
    pub decision_json: Option<String>,
}

/// Loads the ordered route snapshot for an alias (ADR-018 composition):
/// eligible (alias-deployment, credential) pairs.
///
/// # Errors
/// DB failures.
pub async fn load_route_snapshot(db: &Db, alias: &str) -> Result<Vec<RouteCandidate>, DbError> {
    let (candidates, _excluded) = load_route_snapshot_detailed(db, alias).await?;
    Ok(candidates)
}

/// Loads ALL raw candidate rows (including ineligible) so the router can
/// produce exclusion reasons (FR-012).
///
/// # Errors
/// DB failures.
pub async fn load_route_snapshot_detailed(
    db: &Db,
    alias: &str,
) -> Result<(Vec<RouteCandidate>, Vec<ExcludedCandidate>), DbError> {
    let all = raw_snapshot_rows(db, alias).await?;
    let now_iso = crate::time::now_iso();
    let mut candidates = Vec::new();
    let mut excluded = Vec::new();
    for (row, family, capabilities) in all {
        let status = row.credential_status.clone();
        let cooling = row
            .cooldown_until
            .as_ref()
            .is_some_and(|until| until.as_str() > now_iso.as_str());
        let reason = if row.provider_enabled != 1 {
            Some("provider_disabled".to_string())
        } else if row.deployment_enabled != 1 {
            Some("deployment_disabled".to_string())
        } else if status != "active" {
            Some(format!("credential_{status}"))
        } else if cooling {
            Some("credential_cooling_down".to_string())
        } else {
            None
        };
        let candidate = RouteCandidate {
            deployment_id: row.deployment_id.clone(),
            provider_id: row.provider_id.clone(),
            provider_label: row.provider_label.clone(),
            model_id: row.model_id.clone(),
            base_url: row.base_url.clone(),
            allow_loopback: row.allow_loopback == 1,
            endpoint_override: row.endpoint_override.clone(),
            protocol_family: family,
            capabilities,
            credential_id: row.credential_id.clone(),
            credential_secret_ref: row.secret_ref.clone(),
            alias_priority: row.alias_priority,
            credential_priority: row.credential_priority,
            credential_weight: row.credential_weight,
        };
        match reason {
            Some(reason) => excluded.push(ExcludedCandidate {
                deployment_id: candidate.deployment_id.clone(),
                credential_id: candidate.credential_id.clone(),
                reason,
            }),
            None => candidates.push(candidate),
        }
    }
    Ok((candidates, excluded))
}

/// An ineligible candidate with its exclusion reason (FR-012).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExcludedCandidate {
    pub deployment_id: String,
    pub credential_id: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
struct RawRow {
    deployment_id: String,
    provider_id: String,
    provider_label: String,
    model_id: String,
    base_url: String,
    allow_loopback: i64,
    endpoint_override: Option<String>,
    deployment_enabled: i64,
    provider_enabled: i64,
    credential_id: String,
    secret_ref: String,
    credential_priority: i64,
    credential_weight: i64,
    credential_status: String,
    cooldown_until: Option<String>,
    alias_priority: i64,
}

/// Raw (unfiltered) snapshot rows with parsed protocol family + capabilities.
async fn raw_snapshot_rows(
    db: &Db,
    alias: &str,
) -> Result<Vec<(RawRow, ProtocolFamily, CapabilityMetadata)>, DbError> {
    let alias_row = sqlx::query("SELECT id FROM model_aliases WHERE alias = ?1")
        .bind(alias)
        .fetch_optional(db.pool())
        .await?;
    let Some(alias_row) = alias_row else {
        return Ok(Vec::new());
    };
    let alias_id: String = alias_row.get("id");
    let rows = sqlx::query(
        "SELECT ad.priority AS alias_priority,
                d.id AS deployment_id, d.provider_id, p.base_url, p.allow_loopback,
                d.endpoint_override, d.enabled AS deployment_enabled, p.enabled AS provider_enabled,
                p.name AS provider_label,
                m.model_id, m.protocol_family, m.capabilities,
                c.id AS credential_id, c.secret_ref, c.priority AS credential_priority,
                c.weight AS credential_weight, c.status AS credential_status,
                c.cooldown_until
         FROM alias_deployments ad
         JOIN deployments d ON d.id = ad.deployment_id
         JOIN providers p ON p.id = d.provider_id
         JOIN models m ON m.id = d.model_id
         JOIN credentials c ON c.provider_id = d.provider_id
         WHERE ad.alias_id = ?1
         ORDER BY ad.priority DESC, c.priority DESC",
    )
    .bind(&alias_id)
    .fetch_all(db.pool())
    .await?;
    let mut out = Vec::new();
    for row in rows {
        let family = match row.get::<String, _>("protocol_family").as_str() {
            "openai_chat" => ProtocolFamily::OpenAiChat,
            "anthropic_messages" => ProtocolFamily::AnthropicMessages,
            "gemini_generate_content" => ProtocolFamily::GeminiGenerateContent,
            _ => continue,
        };
        let caps_value: Value =
            serde_json::from_str(&row.get::<String, _>("capabilities")).unwrap_or(Value::Null);
        let capabilities = capabilities_from_json(&caps_value, family);
        out.push((
            RawRow {
                deployment_id: row.get("deployment_id"),
                provider_id: row.get("provider_id"),
                provider_label: row.get("provider_label"),
                model_id: row.get("model_id"),
                base_url: row.get("base_url"),
                allow_loopback: row.get("allow_loopback"),
                endpoint_override: row.get("endpoint_override"),
                deployment_enabled: row.get("deployment_enabled"),
                provider_enabled: row.get("provider_enabled"),
                credential_id: row.get("credential_id"),
                secret_ref: row.get("secret_ref"),
                credential_priority: row.get("credential_priority"),
                credential_weight: row.get("credential_weight"),
                credential_status: row.get("credential_status"),
                cooldown_until: row.get("cooldown_until"),
                alias_priority: row.get("alias_priority"),
            },
            family,
            capabilities,
        ));
    }
    Ok(out)
}

/// Parses capability metadata from the models.capabilities JSON column.
#[must_use]
pub fn capabilities_from_json(value: &Value, family: ProtocolFamily) -> CapabilityMetadata {
    let mut caps = CapabilityMetadata {
        protocol_families: vec![family],
        ..Default::default()
    };
    caps.streaming = value
        .get("streaming")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    caps.tool_calling = value
        .get("tool_calling")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    caps.vision = value
        .get("vision")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    caps.structured_output = value
        .get("structured_output")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    caps.context_window = value.get("context_window").and_then(Value::as_i64);
    caps
}

/// Convenience: map of alias -> candidates for `/v1/models` and dashboards.
///
/// # Errors
/// DB failures.
pub async fn alias_map(db: &Db) -> Result<HashMap<String, Vec<String>>, DbError> {
    let rows = sqlx::query(
        "SELECT ma.alias, m.model_id FROM model_aliases ma
         JOIN alias_deployments ad ON ad.alias_id = ma.id
         JOIN deployments d ON d.id = ad.deployment_id
         JOIN models m ON m.id = d.model_id
         JOIN providers p ON p.id = d.provider_id
         WHERE d.enabled = 1 AND p.enabled = 1
         ORDER BY ma.alias, ad.priority DESC",
    )
    .fetch_all(db.pool())
    .await?;
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        map.entry(row.get::<String, _>("alias"))
            .or_default()
            .push(row.get("model_id"));
    }
    Ok(map)
}
