//! Phase 10: import staging (F-1 natural-key conflicts, F-9 single
//! transaction). Separate module for reviewability.

use sair_persistence::Db;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum StageError {
    #[error("staging failed: {0}")]
    Staging(String),
}

/// Stages the decrypted payload into the DB inside ONE transaction with
/// natural-key conflict skipping (F-1). FK order pinned (F-4). Returns the
/// per-entity report.
///
/// # Errors
/// [`StageError`] on any failure (transaction rolls back).
#[allow(clippy::too_many_lines)]
pub async fn stage_import(db: &Db, payload: &Value) -> Result<Value, StageError> {
    let mut tx = db
        .pool()
        .begin()
        .await
        .map_err(|e| StageError::Staging(e.to_string()))?;
    let mut report = serde_json::Map::new();
    let json = |added: u64, skipped: u64| serde_json::json!({ "added": added, "skipped": skipped });

    // Providers (skip existing ids).
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("providers").and_then(Value::as_array) {
        for item in items {
            let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers WHERE id = ?1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO providers (id, name, provider_type, base_url, enabled, allow_loopback, notes, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'), datetime('now'))",
            )
            .bind(id)
            .bind(item.get("name").and_then(Value::as_str).unwrap_or("imported"))
            .bind(item.get("provider_type").and_then(Value::as_str).unwrap_or("openai_compatible"))
            .bind(item.get("base_url").and_then(Value::as_str).unwrap_or(""))
            .bind(item.get("enabled").and_then(Value::as_i64).unwrap_or(1))
            .bind(item.get("allow_loopback").and_then(Value::as_i64).unwrap_or(0))
            .bind(item.get("notes").and_then(Value::as_str))
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
    }
    report.insert("providers".to_string(), json(added, skipped));

    // Accounts.
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("provider_accounts").and_then(Value::as_array) {
        for item in items {
            let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM provider_accounts WHERE id = ?1")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO provider_accounts (id, provider_id, label, group_key, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'), datetime('now'))",
            )
            .bind(id)
            .bind(item.get("provider_id").and_then(Value::as_str).unwrap_or(""))
            .bind(item.get("label").and_then(Value::as_str).unwrap_or("account"))
            .bind(item.get("group_key").and_then(Value::as_str))
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
    }
    report.insert("provider_accounts".to_string(), json(added, skipped));

    // Credentials (natural key provider_id+label; imported incomplete — F-2
    // re-stores secrets after commit when the encrypted payload carries them).
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("credentials").and_then(Value::as_array) {
        for item in items {
            let provider_id = item
                .get("provider_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let label = item.get("label").and_then(Value::as_str).unwrap_or("");
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM credentials WHERE provider_id = ?1 AND label = ?2",
            )
            .bind(provider_id)
            .bind(label)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
            let provider_exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM providers WHERE id = ?1")
                    .bind(provider_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
            if provider_exists == 0 {
                skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO credentials (id, provider_id, account_id, label, secret_ref, status, priority, weight, created_at, updated_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, 'sair.cred.imported.' || lower(hex(randomblob(8))), ?4, ?5, ?6, datetime('now'), datetime('now'))",
            )
            .bind(provider_id)
            .bind(item.get("account_id").and_then(Value::as_str))
            .bind(label)
            .bind(item.get("status").and_then(Value::as_str).unwrap_or("incomplete"))
            .bind(item.get("priority").and_then(Value::as_i64).unwrap_or(0))
            .bind(item.get("weight").and_then(Value::as_i64).unwrap_or(1))
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
    }
    report.insert("credentials".to_string(), json(added, skipped));

    // Models (natural key provider_id+model_id).
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("models").and_then(Value::as_array) {
        for item in items {
            let provider_id = item
                .get("provider_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let model_id = item.get("model_id").and_then(Value::as_str).unwrap_or("");
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM models WHERE provider_id = ?1 AND model_id = ?2",
            )
            .bind(provider_id)
            .bind(model_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO models (id, provider_id, model_id, display_name, protocol_family, capabilities, created_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, datetime('now'))",
            )
            .bind(provider_id)
            .bind(model_id)
            .bind(item.get("display_name").and_then(Value::as_str))
            .bind(item.get("protocol_family").and_then(Value::as_str).unwrap_or("openai_chat"))
            .bind(item.get("capabilities").map(|c| c.to_string()).unwrap_or_else(|| "{}".to_string()))
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
    }
    report.insert("models".to_string(), json(added, skipped));

    // Deployments (unique provider+model). Resolution uses the models-table
    // NATURAL name carried in `model` (exports before the audit fix put the
    // models-row PK in `model_id`, which never resolved — audit P1 fix).
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("deployments").and_then(Value::as_array) {
        for item in items {
            let provider_id = item
                .get("provider_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let model_name = item
                .get("model")
                .and_then(Value::as_str)
                .or_else(|| item.get("model_id").and_then(Value::as_str))
                .unwrap_or("");
            let model_row_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM models WHERE provider_id = ?1 AND model_id = ?2",
            )
            .bind(provider_id)
            .bind(model_name)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            let Some(model_row_id) = model_row_id else {
                skipped += 1;
                continue;
            };
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM deployments WHERE provider_id = ?1 AND model_id = ?2",
            )
            .bind(provider_id)
            .bind(&model_row_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO deployments (id, provider_id, model_id, endpoint_override, enabled, created_at, updated_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, datetime('now'), datetime('now'))",
            )
            .bind(provider_id)
            .bind(&model_row_id)
            .bind(item.get("endpoint_override").and_then(Value::as_str))
            .bind(item.get("enabled").and_then(Value::as_i64).unwrap_or(1))
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
    }
    report.insert("deployments".to_string(), json(added, skipped));

    // Policies (natural key name).
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("routing_policies").and_then(Value::as_array) {
        for item in items {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM routing_policies WHERE name = ?1")
                    .bind(name)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO routing_policies (id, name, strategy, max_attempts, retry_budget_ms, timeout_ms, cooldown_seconds, created_at, updated_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), datetime('now'))",
            )
            .bind(name)
            .bind(item.get("strategy").and_then(Value::as_str).unwrap_or("priority"))
            .bind(item.get("max_attempts").and_then(Value::as_i64).unwrap_or(3))
            .bind(item.get("retry_budget_ms").and_then(Value::as_i64).unwrap_or(130_000))
            .bind(item.get("timeout_ms").and_then(Value::as_i64).unwrap_or(120_000))
            .bind(item.get("cooldown_seconds").and_then(Value::as_i64).unwrap_or(60))
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
    }
    report.insert("routing_policies".to_string(), json(added, skipped));

    // Aliases (natural key alias) + bindings resolved by (provider name,
    // model natural name). Alias rows are exported UNIQUE with a `bindings`
    // array; bindings are processed on every row so multi-deployment aliases
    // restore completely, duplicate-guarded against pre-existing aliases
    // (UNIQUE(alias_id, deployment_id) would abort the transaction otherwise).
    let mut added = 0u64;
    let mut skipped = 0u64;
    let mut seen_aliases: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(items) = payload.get("aliases").and_then(Value::as_array) {
        for item in items {
            let alias = item.get("alias").and_then(Value::as_str).unwrap_or("");
            if seen_aliases.contains(alias) {
                skipped += 1;
            } else {
                seen_aliases.insert(alias.to_string());
                let exists: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM model_aliases WHERE alias = ?1")
                        .bind(alias)
                        .fetch_one(&mut *tx)
                        .await
                        .map_err(|e| StageError::Staging(e.to_string()))?;
                if exists > 0 {
                    skipped += 1;
                } else {
                    sqlx::query(
                        "INSERT INTO model_aliases (id, alias, description, policy_id, created_at, updated_at)
                         VALUES (lower(hex(randomblob(16))), ?1, ?2,
                                 (SELECT id FROM routing_policies WHERE name = ?3),
                                 datetime('now'), datetime('now'))",
                    )
                    .bind(alias)
                    .bind(item.get("description").and_then(Value::as_str))
                    .bind(item.get("policy_name").and_then(Value::as_str))
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
                    added += 1;
                }
            }
            // Bindings (also for pre-existing aliases; missing ones restored).
            if let Some(bindings) = item.get("bindings").and_then(Value::as_array) {
                for binding in bindings {
                    let (Some(provider_name), Some(model_name)) = (
                        binding.get("provider_name").and_then(Value::as_str),
                        binding.get("model").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    let priority = binding.get("priority").and_then(Value::as_i64).unwrap_or(0);
                    let resolvable: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM deployments d
                         JOIN models m ON m.id = d.model_id
                         JOIN providers p ON p.id = d.provider_id
                         WHERE p.name = ?1 AND m.model_id = ?2",
                    )
                    .bind(provider_name)
                    .bind(model_name)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
                    if resolvable == 0 {
                        continue;
                    }
                    let already: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM alias_deployments ad
                         JOIN model_aliases ma ON ma.id = ad.alias_id
                         JOIN deployments d ON d.id = ad.deployment_id
                         JOIN models m ON m.id = d.model_id
                         JOIN providers p ON p.id = d.provider_id
                         WHERE ma.alias = ?1 AND p.name = ?2 AND m.model_id = ?3",
                    )
                    .bind(alias)
                    .bind(provider_name)
                    .bind(model_name)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
                    if already > 0 {
                        continue;
                    }
                    sqlx::query(
                        "INSERT INTO alias_deployments (id, alias_id, deployment_id, priority)
                         VALUES (lower(hex(randomblob(16))),
                                 (SELECT id FROM model_aliases WHERE alias = ?1),
                                 (SELECT d.id FROM deployments d JOIN models m ON m.id = d.model_id
                                   JOIN providers p ON p.id = d.provider_id
                                   WHERE p.name = ?2 AND m.model_id = ?3),
                                 ?4)",
                    )
                    .bind(alias)
                    .bind(provider_name)
                    .bind(model_name)
                    .bind(priority)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
                }
            }
        }
    }
    report.insert("aliases".to_string(), json(added, skipped));

    // Agent profiles (natural key agent_id+endpoint). Alias names re-resolve
    // to the freshly staged alias row ids; unresolvable names become NULL.
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("agent_profiles").and_then(Value::as_array) {
        for item in items {
            let agent_id = item.get("agent_id").and_then(Value::as_str).unwrap_or("");
            let endpoint = item.get("endpoint").and_then(Value::as_str).unwrap_or("");
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM agent_profiles WHERE agent_id = ?1 AND endpoint = ?2",
            )
            .bind(agent_id)
            .bind(endpoint)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO agent_profiles (id, agent_id, agent_version, protocol_family, endpoint,
                                             auth_method, model_alias, fast_alias, env_json,
                                             config_rules_json, notes, created_at, updated_at)
                 VALUES (lower(hex(randomblob(16))), ?1, ?2, ?3, ?4, ?5,
                         (SELECT id FROM model_aliases WHERE alias = ?6),
                         (SELECT id FROM model_aliases WHERE alias = ?7),
                         ?8, ?9, ?10, datetime('now'), datetime('now'))",
            )
            .bind(agent_id)
            .bind(item.get("agent_version").and_then(Value::as_str))
            .bind(item.get("protocol_family").and_then(Value::as_str).unwrap_or("openai_chat"))
            .bind(endpoint)
            .bind(item.get("auth_method").and_then(Value::as_str).unwrap_or("bearer"))
            .bind(item.get("model_alias").and_then(Value::as_str))
            .bind(item.get("fast_alias").and_then(Value::as_str))
            .bind(item.get("env_json").map(|v| v.to_string()).unwrap_or_else(|| "{}".to_string()))
            .bind(item.get("config_rules_json").map(|v| v.to_string()).unwrap_or_else(|| "{}".to_string()))
            .bind(item.get("notes").and_then(Value::as_str))
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
    }
    report.insert("agent_profiles".to_string(), json(added, skipped));

    // Settings: same 3-key allowlist as export (token refs can never appear).
    // Stored as JSON text, matching get_setting/set_setting.
    if let Some(settings) = payload.get("settings").and_then(Value::as_object) {
        let mut added = 0u64;
        for key in ["gateway", "retention_days", "openrouter_attribution"] {
            let Some(value) = settings.get(key) else {
                continue;
            };
            sqlx::query(
                "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
            )
            .bind(key)
            .bind(value.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| StageError::Staging(e.to_string()))?;
            added += 1;
        }
        report.insert("settings".to_string(), json(added, 0));
    }

    tx.commit()
        .await
        .map_err(|e| StageError::Staging(e.to_string()))?;
    Ok(Value::Object(report))
}
