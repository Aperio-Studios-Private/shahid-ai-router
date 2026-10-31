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

    // Deployments (unique provider+model).
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("deployments").and_then(Value::as_array) {
        for item in items {
            let provider_id = item
                .get("provider_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let model_row_id: Option<String> = sqlx::query_scalar(
                "SELECT id FROM models WHERE provider_id = ?1 AND model_id = ?2",
            )
            .bind(provider_id)
            .bind(item.get("model_id").and_then(Value::as_str).unwrap_or(""))
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

    // Aliases (natural key alias) + links to imported deployments by
    // (provider name, model id) pairs carried in the link rows.
    let mut added = 0u64;
    let mut skipped = 0u64;
    if let Some(items) = payload.get("aliases").and_then(Value::as_array) {
        for item in items {
            let alias = item.get("alias").and_then(Value::as_str).unwrap_or("");
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM model_aliases WHERE alias = ?1")
                    .bind(alias)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| StageError::Staging(e.to_string()))?;
            if exists > 0 {
                skipped += 1;
                continue;
            }
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
            if let (Some(provider_name), Some(model_id), Some(priority)) = (
                item.get("provider_name").and_then(Value::as_str),
                item.get("model_id").and_then(Value::as_str),
                item.get("priority").and_then(Value::as_i64),
            ) {
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
                .bind(model_id)
                .bind(priority)
                .execute(&mut *tx)
                .await
                .map_err(|e| StageError::Staging(e.to_string()))?;
            }
            added += 1;
        }
    }
    report.insert("aliases".to_string(), json(added, skipped));

    tx.commit()
        .await
        .map_err(|e| StageError::Staging(e.to_string()))?;
    Ok(Value::Object(report))
}
