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
