import io

# ---- backup.rs: export/import staging over the DB ----
p = 'apps/desktop/src-tauri/src/backup.rs'
src = io.open(p, encoding='utf-8').read()
addition = '''
// ---- Export/import staging over the DB (Phase 10) ----

use sair_persistence::Db;

/// Settings keys NEVER exported (secrets/refs).
const EXCLUDED_SETTINGS: &[&str] = &["gateway_token_ref"];

#[derive(Debug, thiserror::Error)]
pub enum StageError {
    #[error("staging failed: {0}")]
    Staging(String),
}

/// Builds the config-only payload (F-7: credential metadata included with
/// status='incomplete' and NO secret_ref; settings key allowlist).
pub async fn build_config_payload(db: &Db) -> Result<(Value, Vec<String>), String> {
    let mut sections = Vec::new();
    let providers: Vec<Value> = sqlx::query(
        "SELECT id, name, provider_type, base_url, enabled, allow_loopback, notes FROM providers",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "name": r.get::<String, _>("name"),
            "provider_type": r.get::<String, _>("provider_type"),
            "base_url": r.get::<String, _>("base_url"),
            "enabled": r.get::<i64, _>("enabled"),
            "allow_loopback": r.get::<i64, _>("allow_loopback"),
            "notes": r.get::<Option<String>, _>("notes"),
        })
    })
    .collect();
    sections.push("providers".to_string());

    let accounts: Vec<Value> = sqlx::query(
        "SELECT id, provider_id, label, group_key FROM provider_accounts",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "provider_id": r.get::<String, _>("provider_id"),
            "label": r.get::<String, _>("label"),
            "group_key": r.get::<Option<String>, _>("group_key"),
        })
    })
    .collect();
    sections.push("provider_accounts".to_string());

    // F-7: credential metadata WITHOUT secret_ref; status forced incomplete.
    let credentials: Vec<Value> = sqlx::query(
        "SELECT id, provider_id, account_id, label, priority, weight FROM credentials",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "provider_id": r.get::<String, _>("provider_id"),
            "account_id": r.get::<Option<String>, _>("account_id"),
            "label": r.get::<String, _>("label"),
            "status": "incomplete",
            "priority": r.get::<i64, _>("priority"),
            "weight": r.get::<i64, _>("weight"),
        })
    })
    .collect();
    sections.push("credentials".to_string());

    let models: Vec<Value> = sqlx::query(
        "SELECT id, provider_id, model_id, display_name, protocol_family, capabilities FROM models",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "provider_id": r.get::<String, _>("provider_id"),
            "model_id": r.get::<String, _>("model_id"),
            "display_name": r.get::<Option<String>, _>("display_name"),
            "protocol_family": r.get::<String, _>("protocol_family"),
            "capabilities": serde_json::from_str::<Value>(&r.get::<String, _>("capabilities")).unwrap_or(Value::Null),
        })
    })
    .collect();
    sections.push("models".to_string());

    let deployments: Vec<Value> = sqlx::query(
        "SELECT id, provider_id, model_id, endpoint_override, enabled FROM deployments",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "provider_id": r.get::<String, _>("provider_id"),
            "model_id": r.get::<String, _>("model_id"),
            "endpoint_override": r.get::<Option<String>, _>("endpoint_override"),
            "enabled": r.get::<i64, _>("enabled"),
        })
    })
    .collect();
    sections.push("deployments".to_string());

    let policies: Vec<Value> = sqlx::query(
        "SELECT id, name, strategy, max_attempts, retry_budget_ms, timeout_ms, cooldown_seconds FROM routing_policies",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "name": r.get::<String, _>("name"),
            "strategy": r.get::<String, _>("strategy"),
            "max_attempts": r.get::<i64, _>("max_attempts"),
            "retry_budget_ms": r.get::<i64, _>("retry_budget_ms"),
            "timeout_ms": r.get::<i64, _>("timeout_ms"),
            "cooldown_seconds": r.get::<i64, _>("cooldown_seconds"),
        })
    })
    .collect();
    sections.push("routing_policies".to_string());

    let aliases: Vec<Value> = sqlx::query(
        "SELECT ma.id, ma.alias, ma.description, ma.policy_id,
                ad.deployment_id, ad.priority
         FROM model_aliases ma
         LEFT JOIN alias_deployments ad ON ad.alias_id = ma.id",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "alias": r.get::<String, _>("alias"),
            "description": r.get::<Option<String>, _>("description"),
            "policy_id": r.get::<Option<String>, _>("policy_id"),
            "deployment_id": r.get::<Option<String>, _>("deployment_id"),
            "priority": r.get::<Option<i64>, _>("priority"),
        })
    })
    .collect();
    sections.push("aliases".to_string());

    let profiles: Vec<Value> = sqlx::query(
        "SELECT agent_id, agent_version, protocol_family, endpoint, auth_method, model_alias, fast_alias, env_json, config_rules_json, notes FROM agent_profiles",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "agent_id": r.get::<String, _>("agent_id"),
            "agent_version": r.get::<Option<String>, _>("agent_version"),
            "protocol_family": r.get::<String, _>("protocol_family"),
            "endpoint": r.get::<String, _>("endpoint"),
            "auth_method": r.get::<String, _>("auth_method"),
            "model_alias": r.get::<Option<String>, _>("model_alias"),
            "fast_alias": r.get::<Option<String>, _>("fast_alias"),
            "env_json": serde_json::from_str::<Value>(&r.get::<String, _>("env_json")).unwrap_or(Value::Null),
            "config_rules_json": serde_json::from_str::<Value>(&r.get::<String, _>("config_rules_json")).unwrap_or(Value::Null),
            "notes": r.get::<Option<String>, _>("notes"),
        })
    })
    .collect();
    sections.push("agent_profiles".to_string());

    // Settings: explicit key allowlist (F-10).
    let mut settings = serde_json::Map::new();
    for key in ["gateway", "retention_days", "openrouter_attribution"] {
        let value = db.get_setting(key).await.map_err(|e| e.to_string())?;
        if let Some(value) = value {
            settings.insert(key.to_string(), value);
        }
    }
    sections.push("settings".to_string());

    Ok((
        serde_json::json!({
            "providers": providers,
            "provider_accounts": accounts,
            "credentials": credentials,
            "models": models,
            "deployments": deployments,
            "routing_policies": policies,
            "aliases": aliases,
            "agent_profiles": profiles,
            "settings": settings,
        }),
        sections,
    ))
}

/// Export summary returned to the UI.
#[derive(Debug, Serialize)]
pub struct ExportSummary {
    pub path: String,
    pub mode: String,
    pub sections: Vec<String>,
}

/// Encrypted payload build: config payload + credential secrets (F-2: values
/// only inside the ciphertext).
pub async fn build_encrypted_payload(db: &Db, vault: &dyn SecretStore) -> Result<(Value, Vec<String>), String> {
    let (mut payload, mut sections) = build_config_payload(db).await?;
    // Re-fetch credentials WITH secret refs for vault retrieval; replace the
    // incomplete rows with full rows carrying secrets (encrypted only).
    let providers = db.repos().list_providers().await.map_err(|e| e.to_string())?;
    let mut full_credentials = Vec::new();
    for provider in &providers {
        for credential in db.repos().list_credentials(&provider.id).await.map_err(|e| e.to_string())? {
            let secret_ref = sair_vault::SecretRef::parse(&credential.secret_ref)
                .map_err(|e| format!("malformed secret ref: {e}"))?;
            let secret = vault
                .retrieve(&secret_ref)
                .map_err(|e| format!("vault unavailable: {e}"))?;
            full_credentials.push(serde_json::json!({
                "id": credential.id,
                "provider_id": credential.provider_id,
                "account_id": credential.account_id,
                "label": credential.label,
                "status": credential.status,
                "priority": credential.priority,
                "weight": credential.weight,
                // F-3: base64 of the raw secret; exists ONLY inside ciphertext.
                "secret_b64": b64(&secret),
            }));
        }
    }
    payload["credentials"] = Value::Array(full_credentials);
    sections.retain(|s| s != "credentials");
    sections.push("credentials+secrets".to_string());
    let _ = &mut sections;
    Ok((payload, sections))
}
'''
src = src.rstrip() + '\n' + addition
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('staging added')
