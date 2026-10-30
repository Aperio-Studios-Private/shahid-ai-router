//! Tauri desktop shell for Shahid AI Router.
//!
//! Owns the UI process and the gateway lifecycle (ADR-003). Startup is
//! fail-closed: migrations abort before the gateway binds; token failures
//! leave the gateway stopped with an explicit error state. On app exit the
//! gateway is drained within a grace period (FR-017; the `RunEvent::Exit`
//! hook is the drain caller named by review finding F-9).

mod commands;
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
                let state = app_handle.state::<AppState>();
                if let Err(err) =
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
    // gateway stopped with an explicit error (degraded start, docs/06).
    let state = app.state::<AppState>();
    if let Err(err) = state.ensure_token().await {
        tracing::error!(error = %err, "gateway token unavailable; gateway will not start");
        return Err(err);
    }
    state
        .start_gateway()
        .await
        .map(|view| {
            tracing::info!(addr = ?view.addr, "gateway auto-started");
        })
        .map_err(|e| {
            tracing::error!(error = %e, "gateway auto-start failed");
            e
        })
}
