//! Tauri desktop shell for Shahid AI Router.
//!
//! Owns the UI process and the gateway lifecycle (ADR-003). Startup is
//! fail-closed: migrations abort before the gateway binds; token failures
//! leave the gateway stopped with an explicit error state. On app exit the
//! gateway is drained within a grace period (FR-017; the `RunEvent::Exit`
//! hook is the drain caller named by review finding F-9).

mod backup;
mod backup_import;
mod commands;
mod commands_management;
mod commands_rotation;
mod profile_apply;
mod state;
mod telemetry;

use std::sync::Arc;
use std::time::Duration;

use sair_persistence::Db;
use sair_vault::OsSecretStore;
use tauri::Manager;
use tauri::RunEvent;

use state::{AppState, purge_expired_records};

pub fn run() {
    // ADR-006: install the ring crypto provider once, before any HTTP client
    // is constructed. TLS is mandatory for all outbound provider calls.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("rustls ring crypto provider must install exactly once");

    // Keep the file-writer guard alive for the process lifetime.
    let _log_guard = telemetry::init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_gateway_state,
            commands::start_gateway,
            commands::stop_gateway,
            commands::reveal_gateway_token,
            commands::regenerate_gateway_token,
            commands_management::list_providers,
            commands_management::add_provider,
            commands_management::update_provider,
            commands_management::set_provider_enabled,
            commands_management::delete_provider,
            commands_management::test_provider,
            commands_management::list_credentials,
            commands_management::add_credential,
            commands_management::set_credential_status,
            commands_management::delete_credential,
            commands_management::list_deployments,
            commands_management::add_model,
            commands_management::update_model_capabilities,
            commands_management::add_deployment,
            commands_management::set_deployment_enabled,
            commands_management::delete_deployment,
            commands_management::list_aliases,
            commands_management::add_alias,
            commands_management::bind_alias_deployment,
            commands_management::delete_alias,
            commands_rotation::set_credential_priority_weight,
            commands_rotation::list_provider_accounts,
            commands_rotation::add_provider_account,
            commands_rotation::assign_credential_account,
            commands_rotation::list_health_events,
            commands_rotation::credential_health_summary,
            commands_rotation::generate_agent_profile,
            commands_rotation::hash_profile_target,
            commands_rotation::apply_agent_profile,
            commands_rotation::list_request_records,
            commands_rotation::usage_summary,
            commands_rotation::export_backup,
            commands_rotation::preview_backup,
            commands_rotation::commit_backup,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = init_backend(handle).await {
                    // Startup failure is contained and observable; the UI
                    // reflects the degraded state (gateway stopped).
                    tracing::error!(error = %err, "backend initialization failed");
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("tauri application build failed")
        .run(|app_handle, event| {
            if let RunEvent::Exit = event {
                // FR-017: drain in-flight gateway requests before exit.
                // try_state guards the failure path where backend init never
                // managed the state (SEC2-08: no panic at exit).
                if let Some(state) = app_handle.try_state::<AppState>()
                    && let Err(err) =
                        tauri::async_runtime::block_on(state.stop_gateway(Duration::from_secs(3)))
                {
                    tracing::warn!(error = %err, "gateway drain on exit did not finish cleanly");
                }
            }
        });
}

/// Opens the DB, applies migrations, starts retention, ensures the token and
/// starts the gateway. Every step is fail-closed with respect to the gateway.
async fn init_backend(app: tauri::AppHandle) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("cannot create data dir: {e}"))?;

    let db = Db::open(data_dir.join("shahid.db").to_str().ok_or("bad db path")?)
        .await
        .map_err(|e| format!("cannot open database: {e}"))?;
    // docs/02: migration failures must stop safely — the gateway never binds
    // on an unmigrated schema.
    db.run_migrations()
        .await
        .map_err(|e| format!("database migration failed: {e}"))?;

    // Retention pass at startup (FR-020) + daily task.
    purge_expired_records(&db).await;
    let retention_db = db.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
            purge_expired_records(&retention_db).await;
        }
    });

    app.manage(AppState::new(db, Arc::new(OsSecretStore::new())));

    // Ensure the token exists and start the gateway. Failures leave the
    // gateway stopped with an explicit, UI-visible error state (degraded
    // start, docs/06; security review SEC2-04: failures must not be
    // log-only).
    let state = app.state::<AppState>();
    if let Err(err) = state.ensure_token().await {
        tracing::error!(error = %err, "gateway token unavailable; gateway will not start");
        state.set_gateway_error(format!("gateway token unavailable: {err}"));
        return Err(err);
    }
    if let Err(err) = state.start_gateway().await {
        tracing::error!(error = %err, "gateway auto-start failed");
        return Err(err);
    }
    Ok(())
}
