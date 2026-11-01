//! Phase 15: public settings IPC — the Settings screen reads/writes the
//! same keys the backend already consumes (gateway tuning + retention).
//! Allowlisted and validated; the vault/token never appears here.

use serde_json::{Value, json};
use tauri::State;

use crate::state::AppState;

/// Reads the user-editable settings with safe defaults filled in.
#[tauri::command]
pub async fn get_public_settings(state: State<'_, AppState>) -> Result<Value, String> {
    let gateway = state
        .db
        .get_setting("gateway")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or(json!({}));
    let retention_days = state
        .db
        .get_setting("retention_days")
        .await
        .map_err(|e| e.to_string())?
        .and_then(|v| v.as_i64())
        .unwrap_or(30);
    Ok(json!({ "gateway": gateway, "retention_days": retention_days }))
}

/// Validates and persists settings. Gateway changes need a gateway restart to
/// apply — the Settings screen offers that button.
#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    gateway: Value,
    retention_days: i64,
) -> Result<(), String> {
    if !gateway.is_object() {
        return Err("gateway settings must be an object".to_string());
    }
    if let Some(port) = gateway.get("port").and_then(Value::as_u64)
        && (port == 0 || port > u16::MAX as u64)
    {
        return Err(format!("invalid gateway port: {port}"));
    }
    if let Some(limit) = gateway.get("body_limit_bytes").and_then(Value::as_u64)
        && limit < 1024
    {
        return Err("body limit must be at least 1 KiB".to_string());
    }
    if retention_days < 1 {
        return Err("retention must be at least 1 day".to_string());
    }
    state
        .db
        .set_setting("gateway", &gateway)
        .await
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_setting("retention_days", &json!(retention_days))
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!("settings updated via Settings screen");
    Ok(())
}
