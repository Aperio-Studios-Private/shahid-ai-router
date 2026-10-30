// ---- Phase 4 commands: rotation & health completion (FR-002/012) ----

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

#[tauri::command]
#[allow(clippy::too_many_arguments)] // none: 4 args only — kept symmetrical
pub async fn set_credential_priority_weight(
    state: State<'_, AppState>,
    id: String,
    priority: i64,
    weight: i64,
) -> Result<(), String> {
    state
        .db
        .repos()
        .set_credential_priority_weight(&id, priority, weight)
        .await
}

#[tauri::command]
pub async fn list_provider_accounts(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    state
        .db
        .repos()
        .list_provider_accounts(&provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_provider_account(
    state: State<'_, AppState>,
    provider_id: String,
    label: String,
    group_key: Option<String>,
) -> Result<String, String> {
    state
        .db
        .repos()
        .insert_provider_account(&provider_id, &label, group_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn assign_credential_account(
    state: State<'_, AppState>,
    credential_id: String,
    account_id: String,
) -> Result<(), String> {
    state
        .db
        .repos()
        .assign_credential_account(&credential_id, &account_id)
        .await
}

#[derive(Debug, Serialize)]
pub struct HealthEventView {
    pub ts: String,
    pub subject_type: String,
    pub subject_id: String,
    pub event: String,
    pub category: Option<String>,
    pub detail: Option<String>,
    pub correlation_id: Option<String>,
}

#[tauri::command]
pub async fn list_health_events(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<HealthEventView>, String> {
    Ok(state
        .db
        .repos()
        .list_health_events(limit.unwrap_or(100))
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|v| HealthEventView {
            ts: v["ts"].as_str().unwrap_or_default().to_string(),
            subject_type: v["subject_type"].as_str().unwrap_or_default().to_string(),
            subject_id: v["subject_id"].as_str().unwrap_or_default().to_string(),
            event: v["event"].as_str().unwrap_or_default().to_string(),
            category: v["category"].as_str().map(str::to_string),
            detail: v["detail"].as_str().map(str::to_string),
            correlation_id: v["correlation_id"].as_str().map(str::to_string),
        })
        .collect())
}

#[tauri::command]
pub async fn credential_health_summary(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    state
        .db
        .repos()
        .credential_health_summary()
        .await
        .map_err(|e| e.to_string())
}

// ---- Phase 8: agent profile generation + consent-gated application ----

use crate::profile_apply;

#[tauri::command]
pub async fn generate_agent_profile(
    state: State<'_, AppState>,
    agent_id: String,
    alias: String,
    fast_alias: Option<String>,
) -> Result<serde_json::Value, String> {
    // F-5: validate alias existence + same-protocol deployment presence.
    if !state
        .db
        .repos()
        .alias_exists(&alias)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err(format!("alias `{alias}` does not exist"));
    }
    let family = match agent_id.as_str() {
        "claude-code" => "anthropic_messages",
        "gemini-cli" => "gemini_generate_content",
        "generic-openai" | "codex-cli" => "openai_chat",
        _ => return Err("unknown agent".to_string()),
    };
    if !state
        .db
        .repos()
        .alias_has_family_deployments(&alias, family)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err(format!(
            "alias `{alias}` has no enabled {family} deployment"
        ));
    }

    let settings = state
        .db
        .get_setting(crate::state::SETTING_GATEWAY)
        .await
        .map_err(|e| e.to_string())?;
    let port = settings
        .as_ref()
        .and_then(|o| o.get("port"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(8787) as u16;

    let profile =
        sair_domain::profiles::generate_profile(&agent_id, port, alias.clone(), fast_alias)
            .ok_or("unknown agent")?;
    Ok(serde_json::json!({
        "agent_id": profile.agent_id,
        "config_text": profile.config_text,
        "files": profile.files,
        "rollback": profile.rollback,
        "limitations": profile.limitations,
        "applicable": profile.applicable,
        "verification": format!("{} on 2026-10-31 (contract tests)", sair_domain::profiles::VERIFICATION_PREFIX),
    }))
}

#[tauri::command]
pub async fn hash_profile_target(agent_id: String, file_index: usize) -> Result<String, String> {
    let profile = sair_domain::profiles::generate_profile(&agent_id, 8787, String::new(), None)
        .ok_or("unknown agent")?;
    let target = profile_apply::resolve_target(&profile, file_index)?;
    if !target.exists() {
        return Ok(String::new()); // new file: empty hash = no precondition
    }
    profile_apply::hash_file(&target)
}

#[tauri::command]
pub async fn apply_agent_profile(
    agent_id: String,
    file_index: usize,
    content: String,
    expected_existing: Option<String>,
) -> Result<String, String> {
    // Consent is the UI's confirm dialog; the backend enforces the rest.
    let profile = sair_domain::profiles::generate_profile(&agent_id, 8787, String::new(), None)
        .ok_or("unknown agent")?;
    let target = profile_apply::resolve_target(&profile, file_index)?;
    profile_apply::apply_content(&target, &content, expected_existing.as_deref())
        .map(|p| p.to_string_lossy().to_string())
}
