// ---- Phase 3 management commands (FR-001/002/003/004) ----
// Views never include secret material: credentials expose label/status/
// priority only. Secrets travel one way: UI → vault.

use std::time::Duration;

use sair_providers::http::{SsrfPolicy, build_client};
use sair_vault::{SecretBytes, SecretRef};
use serde::Serialize;
use serde_json::json;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub enabled: bool,
    pub allow_loopback: bool,
    pub notes: Option<String>,
}

#[tauri::command]
pub async fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderView>, String> {
    Ok(state
        .db
        .repos()
        .list_providers()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|p| ProviderView {
            id: p.id,
            name: p.name,
            provider_type: p.provider_type,
            base_url: p.base_url,
            enabled: p.enabled,
            allow_loopback: p.allow_loopback,
            notes: p.notes,
        })
        .collect())
}

#[tauri::command]
pub async fn add_provider(
    state: State<'_, AppState>,
    name: String,
    provider_type: String,
    base_url: String,
    allow_loopback: bool,
    notes: Option<String>,
) -> Result<String, String> {
    state
        .db
        .repos()
        .insert_provider(
            &name,
            &provider_type,
            &base_url,
            allow_loopback,
            notes.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_provider(
    state: State<'_, AppState>,
    id: String,
    base_url: String,
    allow_loopback: bool,
    notes: Option<String>,
) -> Result<(), String> {
    state
        .db
        .repos()
        .update_provider(&id, &base_url, allow_loopback, notes.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_provider_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .repos()
        .set_provider_enabled(&id, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_provider(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .db
        .repos()
        .delete_provider(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Harmless connectivity test: GET {base}/models with the first active
/// credential. Never returns credential material.
#[tauri::command]
pub async fn test_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let repos = state.db.repos();
    let providers = repos.list_providers().await.map_err(|e| e.to_string())?;
    let Some(provider) = providers.iter().find(|p| p.id == id) else {
        return Err("provider not found".to_string());
    };
    let credentials = repos
        .list_credentials(&id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(credential) = credentials.iter().find(|c| c.status == "active") else {
        return Ok(json!({"ok": false, "detail": "no active credential"}));
    };
    let secret_ref = SecretRef::parse(&credential.secret_ref).map_err(|e| e.to_string())?;
    let secret = state
        .vault
        .retrieve(&secret_ref)
        .map_err(|e| format!("vault: {e}"))?;
    let secret = SecretBytes::new(secret);

    let policy = if provider.allow_loopback {
        SsrfPolicy::allow_loopback()
    } else {
        SsrfPolicy::default()
    };
    let client = build_client(policy);
    let base = provider.base_url.trim_end_matches('/');
    let url = url::Url::parse(&format!("{base}/models")).map_err(|e| e.to_string())?;
    let request = client
        .request(reqwest::Method::GET, url)
        .map_err(|e| format!("SSRF policy: {e}"))?
        .bearer_auth(secret.expose_str().map_err(|_| "credential not UTF-8")?)
        .timeout(Duration::from_secs(15));
    match request.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            Ok(json!({"ok": (200..300).contains(&status), "status": status}))
        }
        Err(e) => Ok(json!({"ok": false, "detail": e.to_string()})),
    }
}

#[derive(Debug, Serialize)]
pub struct CredentialView {
    pub id: String,
    pub label: String,
    pub status: String,
    pub priority: i64,
    pub weight: i64,
    pub cooldown_until: Option<String>,
}

#[tauri::command]
pub async fn list_credentials(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<CredentialView>, String> {
    Ok(state
        .db
        .repos()
        .list_credentials(&provider_id)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|c| CredentialView {
            id: c.id,
            label: c.label,
            status: c.status,
            priority: c.priority,
            weight: c.weight,
            cooldown_until: c.cooldown_until,
        })
        .collect())
}

/// Stores the secret in the OS vault and persists only the opaque ref
/// (the vault mints the ref; the DB never sees the secret).
#[tauri::command]
pub async fn add_credential(
    state: State<'_, AppState>,
    provider_id: String,
    label: String,
    secret: String,
    priority: i64,
    weight: i64,
) -> Result<String, String> {
    let secret_ref = state
        .vault
        .store(secret.as_bytes())
        .map_err(|e| e.to_string())?;
    state
        .db
        .repos()
        .insert_credential(&provider_id, &label, secret_ref.as_str(), priority, weight)
        .await
        .map_err(|e| {
            // Best-effort rollback: do not orphan the vault item.
            let _ = state.vault.delete(&secret_ref);
            e.to_string()
        })
}

#[tauri::command]
pub async fn set_credential_status(
    state: State<'_, AppState>,
    id: String,
    status: String,
) -> Result<(), String> {
    state
        .db
        .repos()
        .set_credential_status(&id, &status)
        .await
        .map_err(|e| e.to_string())
}

/// Deletes the credential row AND the vault item (docs/07 secret lifecycle).
#[tauri::command]
pub async fn delete_credential(
    state: State<'_, AppState>,
    provider_id: String,
    id: String,
) -> Result<(), String> {
    let repos = state.db.repos();
    let credentials = repos
        .list_credentials(&provider_id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(credential) = credentials.iter().find(|c| c.id == id) else {
        return Err("credential not found".to_string());
    };
    if let Ok(secret_ref) = SecretRef::parse(&credential.secret_ref) {
        let _ = state.vault.delete(&secret_ref);
    }
    repos
        .delete_credential(&id)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct DeploymentView {
    pub id: String,
    pub model_id: String,
    pub protocol_family: String,
    pub enabled: bool,
    pub endpoint_override: Option<String>,
}

#[tauri::command]
pub async fn list_deployments(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<DeploymentView>, String> {
    Ok(state
        .db
        .repos()
        .list_deployments(&provider_id)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|d| DeploymentView {
            id: d.id,
            model_id: d.model_id,
            protocol_family: d.protocol_family,
            enabled: d.enabled,
            endpoint_override: d.endpoint_override,
        })
        .collect())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // mirrors the capabilities JSON fields
pub async fn add_model(
    state: State<'_, AppState>,
    provider_id: String,
    model_id: String,
    protocol_family: String,
    streaming: bool,
    tool_calling: bool,
    vision: bool,
    structured_output: bool,
    context_window: Option<i64>,
) -> Result<String, String> {
    let capabilities = json!({
        "streaming": streaming,
        "tool_calling": tool_calling,
        "vision": vision,
        "structured_output": structured_output,
        "context_window": context_window,
    });
    state
        .db
        .repos()
        .insert_model(
            &provider_id,
            &model_id,
            &protocol_family,
            &capabilities,
            None,
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_deployment(
    state: State<'_, AppState>,
    provider_id: String,
    model_pk: String,
    endpoint_override: Option<String>,
) -> Result<String, String> {
    state
        .db
        .repos()
        .insert_deployment(&provider_id, &model_pk, endpoint_override.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_deployment_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .db
        .repos()
        .set_deployment_enabled(&id, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_deployment(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .db
        .repos()
        .delete_deployment(&id)
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Serialize)]
pub struct AliasView {
    pub id: String,
    pub alias: String,
    pub model_ids: Vec<String>,
}

#[tauri::command]
pub async fn list_aliases(state: State<'_, AppState>) -> Result<Vec<AliasView>, String> {
    let aliases = state
        .db
        .repos()
        .list_aliases()
        .await
        .map_err(|e| e.to_string())?;
    let map = sair_persistence::repos::alias_map(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(aliases
        .into_iter()
        .map(|a| {
            let model_ids = map.get(&a.alias).cloned().unwrap_or_default();
            AliasView {
                id: a.id,
                alias: a.alias,
                model_ids,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn add_alias(
    state: State<'_, AppState>,
    alias: String,
    description: Option<String>,
) -> Result<String, String> {
    state
        .db
        .repos()
        .insert_alias(&alias, description.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn bind_alias_deployment(
    state: State<'_, AppState>,
    alias_id: String,
    deployment_id: String,
    priority: i64,
) -> Result<String, String> {
    state
        .db
        .repos()
        .bind_deployment(&alias_id, &deployment_id, priority)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_alias(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .db
        .repos()
        .delete_alias(&id)
        .await
        .map_err(|e| e.to_string())
}

/// Phase 5 (F-2): edits a model row's capability flags so the router's
/// capability requirements can match reality.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // mirrors the capabilities JSON fields
pub async fn update_model_capabilities(
    state: State<'_, AppState>,
    model_pk: String,
    streaming: bool,
    tool_calling: bool,
    vision: bool,
    structured_output: bool,
    context_window: Option<i64>,
) -> Result<(), String> {
    let capabilities = json!({
        "streaming": streaming,
        "tool_calling": tool_calling,
        "vision": vision,
        "structured_output": structured_output,
        "context_window": context_window,
    });
    state
        .db
        .repos()
        .update_model_capabilities(&model_pk, &capabilities)
        .await
        .map_err(|e| e.to_string())
}
