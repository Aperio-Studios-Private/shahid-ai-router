//! Phase 12: Cloud backup IPC commands (FR-024, ADR-019). Thin layer over
//! [`crate::cloud_backup`] + the existing path-keyed preview/commit pair
//! (review F-12-2: cloud restores stage a temp `.sair` file and reuse the
//! unchanged flow). All errors are safe strings; the DSN/key never appear.

use tauri::State;

use crate::state::{
    AppState, SETTING_CLOUD_DSN_REF, SETTING_CLOUD_KEY_REF, SETTING_CLOUD_MODE,
    SETTING_CLOUD_PASSPHRASE_REF, SETTING_CLOUD_PROVIDER, SETTING_CLOUD_SCHEDULE,
};

const MIN_PASSPHRASE: usize = 12;

/// Configures (and tests) the cloud destination. The UI shows the consent
/// dialog BEFORE calling this (destination, mode, recoverability warning).
///
/// # Errors
/// Validation, connection-test, vault, or DB failures.
#[tauri::command]
pub async fn cloud_configure(
    state: State<'_, AppState>,
    provider: String,
    mode: String,
    dsn: String,
    key: Option<String>,
    passphrase: Option<String>,
    schedule_enabled: bool,
) -> Result<serde_json::Value, String> {
    if provider != "neon" && provider != "supabase" && provider != "github-gist" {
        return Err(format!("unsupported cloud provider: {provider}"));
    }
    if mode != "encrypted" && mode != "config" {
        return Err(format!("unknown cloud backup mode: {mode}"));
    }
    if mode == "encrypted"
        && passphrase
            .as_deref()
            .is_none_or(|p| p.len() < MIN_PASSPHRASE)
    {
        return Err(format!(
            "the backup passphrase is required for encrypted mode (min {MIN_PASSPHRASE} characters)"
        ));
    }
    if provider == "supabase" && key.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err("the Supabase API key is required".to_string());
    }
    if provider == "github-gist" && key.as_deref().map(str::trim).unwrap_or("").is_empty() {
        return Err("the GitHub token is required".to_string());
    }

    // Test BEFORE persisting anything.
    {
        let dsn = dsn.trim().to_string();
        let key = key.clone().unwrap_or_default();
        let provider_name = provider.clone();
        let test = tauri::async_runtime::spawn_blocking(
            move || -> Result<Arc<dyn crate::cloud_backup::CloudBackupProvider>, String> {
                Ok(match provider_name.as_str() {
                    "neon" => Arc::new(crate::cloud_backup::NeonProvider::new(&dsn)?),
                    "github-gist" => Arc::new(crate::cloud_backup::GistProvider::new(
                        key.trim(),
                        &dsn,
                        reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(30))
                            .build()
                            .map_err(|e| format!("client build failed: {e}"))?,
                    )?),
                    _ => Arc::new(crate::cloud_backup::SupabaseProvider::new(
                        &dsn,
                        &key,
                        reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(30))
                            .build()
                            .map_err(|e| format!("client build failed: {e}"))?,
                    )?),
                })
            },
        )
        .await
        .map_err(|e| format!("test task failed: {e}"))??;
        test.test().await?;
    }

    // Persist: secrets to the OS vault (opaque refs in app_settings).
    state
        .db
        .set_setting(SETTING_CLOUD_PROVIDER, &serde_json::json!(provider))
        .await
        .map_err(|e| format!("db unavailable: {e}"))?;
    state
        .db
        .set_setting(SETTING_CLOUD_MODE, &serde_json::json!(mode))
        .await
        .map_err(|e| format!("db unavailable: {e}"))?;
    state
        .db
        .set_setting(
            SETTING_CLOUD_SCHEDULE,
            &serde_json::json!(schedule_enabled.to_string()),
        )
        .await
        .map_err(|e| format!("db unavailable: {e}"))?;
    let dsn_ref = state.vault_store(&dsn).await?;
    state
        .db
        .set_setting(SETTING_CLOUD_DSN_REF, &serde_json::json!(dsn_ref))
        .await
        .map_err(|e| format!("db unavailable: {e}"))?;
    if provider == "supabase"
        && let Some(key) = key.as_deref().map(str::trim).filter(|k| !k.is_empty())
    {
        let key_ref = state.vault_store(key).await?;
        state
            .db
            .set_setting(SETTING_CLOUD_KEY_REF, &serde_json::json!(key_ref))
            .await
            .map_err(|e| format!("db unavailable: {e}"))?;
    }
    if mode == "encrypted"
        && let Some(passphrase) = passphrase
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
    {
        let passphrase_ref = state.vault_store(passphrase).await?;
        state
            .db
            .set_setting(
                SETTING_CLOUD_PASSPHRASE_REF,
                &serde_json::json!(passphrase_ref),
            )
            .await
            .map_err(|e| format!("db unavailable: {e}"))?;
    }
    let device_id = state.cloud_device_id().await?;
    tracing::info!(provider = %provider, mode = %mode, schedule = schedule_enabled, "cloud backup configured");
    Ok(serde_json::json!({
        "configured": true,
        "provider": provider,
        "mode": mode,
        "schedule_enabled": schedule_enabled,
        "device_id": device_id,
    }))
}

use std::sync::Arc;

/// Connectivity check for the configured destination.
///
/// # Errors
/// Configuration or transport failures.
#[tauri::command]
pub async fn cloud_test(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let provider = state.cloud_provider().await?;
    provider.test().await?;
    Ok(serde_json::json!({ "ok": true }))
}

/// Cloud status for the UI. Contains no secret material and resolves no
/// vault items.
///
/// # Errors
/// DB failures.
#[tauri::command]
pub async fn cloud_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let config = state.cloud_config(false).await?;
    let last_upload_at = state.cloud_last_upload_at();
    let last_error = state.cloud_last_error();
    let dirty = state.cloud_is_dirty();
    Ok(serde_json::json!({
        "configured": config.provider.is_some(),
        "provider": config.provider,
        "mode": config.mode,
        "schedule_enabled": config.schedule_enabled,
        "device_id": config.device_id,
        "has_connection": config.has_dsn,
        "has_passphrase": config.has_passphrase,
        "last_upload_at": last_upload_at,
        "last_error": last_error,
        "dirty": dirty,
    }))
}

/// Lists cloud snapshots (metadata only — never payloads).
///
/// # Errors
/// Configuration or transport failures.
#[tauri::command]
pub async fn cloud_list(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let provider = state.cloud_provider().await?;
    let snapshots = provider.list().await?;
    Ok(snapshots
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "device_id": s.device_id,
                "app_version": s.app_version,
                "mode": s.mode,
                "created_at": s.created_at,
                "sha256": s.sha256,
                "bytes": s.bytes,
            })
        })
        .collect())
}

/// Force an upload now (ignores the quiet-window rules; honors schedule
/// config for the envelope mode).
///
/// # Errors
/// Configuration, vault, DB, crypto, or transport failures.
#[tauri::command]
pub async fn cloud_backup_now(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    state.cloud_run_upload(true).await
}

/// Downloads a snapshot, validates it with the existing envelope parser, and
/// stages it as a temp `.sair` file; then runs the UNCHANGED preview flow
/// (held slot, conflict report, counts). The UI then calls `cloud_commit`.
///
/// # Errors
/// Configuration, transport, validation, or vault failures.
#[tauri::command]
pub async fn cloud_restore(
    state: State<'_, AppState>,
    id: String,
    passphrase: Option<String>,
) -> Result<serde_json::Value, String> {
    let (path, mode) = state.cloud_stage_download(&id, passphrase.clone()).await?;
    // Encrypted preview needs the passphrase: caller-supplied first, then the
    // vault copy (same-device re-restore).
    let effective_passphrase = match passphrase {
        Some(p) if !p.trim().is_empty() => Some(p.trim().to_string()),
        _ => {
            let config = state.cloud_config(true).await?;
            config.passphrase.map(|p| p.to_string())
        }
    };
    let preview = crate::commands_rotation::preview_backup(
        state,
        path.to_string_lossy().into_owned(),
        effective_passphrase,
    )
    .await?;
    Ok(serde_json::json!({
        "preview": preview,
        "mode": mode,
    }))
}

/// Commits the staged cloud restore (reuses the unchanged commit flow) and
/// cleans up the temp file.
///
/// # Errors
/// Staging or staging/commit failures (transaction rolls back).
#[tauri::command]
pub async fn cloud_commit(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let Some((path, mode)) = state.cloud_take_staged() else {
        return Err("no cloud restore staged — restore a snapshot first".to_string());
    };
    let re_store = mode == "encrypted";
    let report = crate::commands_rotation::commit_backup(
        state.clone(),
        path.to_string_lossy().into_owned(),
        re_store,
    )
    .await;
    state.cloud_clear_staged(&path);
    // The restored configuration is a change → schedule a fresh upload.
    state.mark_cloud_dirty();
    report
}

/// Disables cloud backup; optionally deletes THIS device's cloud rows.
/// Vault items (connection string / key / passphrase) are always removed.
///
/// # Errors
/// Transport (purge) or DB failures.
#[tauri::command]
pub async fn cloud_disable(
    state: State<'_, AppState>,
    purge: bool,
) -> Result<serde_json::Value, String> {
    let mut purged_rows = 0u64;
    if purge {
        // Best effort: config may already be half-broken; report but do not
        // block disabling.
        if let Ok(provider) = state.cloud_provider().await {
            let device = state.cloud_device_id().await.unwrap_or_default();
            purged_rows = provider.purge(device).await.unwrap_or(0);
        }
    }
    for key in [
        SETTING_CLOUD_PROVIDER,
        SETTING_CLOUD_MODE,
        SETTING_CLOUD_SCHEDULE,
        SETTING_CLOUD_DSN_REF,
        SETTING_CLOUD_KEY_REF,
        SETTING_CLOUD_PASSPHRASE_REF,
    ] {
        let _ = state.db.set_setting(key, &serde_json::json!(null)).await;
    }
    state.cloud_forget_secrets().await;
    tracing::info!(purge, purged_rows, "cloud backup disabled");
    Ok(serde_json::json!({ "disabled": true, "purged_rows": purged_rows }))
}
