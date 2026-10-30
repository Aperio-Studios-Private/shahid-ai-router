//! IPC commands exposed to the frontend. Views never contain secret
//! material; the token is only returned by the explicit reveal/regenerate
//! actions (docs/06; review finding F-10's sweep covers these paths).

use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::state::{AppState, GatewayStateView};

/// Information shown by the About screen; gateway flag reflects reality.
#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub gateway_running: bool,
}

#[tauri::command]
pub async fn get_app_info(state: State<'_, AppState>) -> Result<AppInfo, String> {
    let gateway = state.gateway_state().await;
    Ok(AppInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        gateway_running: gateway.running,
    })
}

#[tauri::command]
pub async fn get_gateway_state(state: State<'_, AppState>) -> Result<GatewayStateView, String> {
    Ok(state.gateway_state().await)
}

#[tauri::command]
pub async fn start_gateway(state: State<'_, AppState>) -> Result<GatewayStateView, String> {
    state.start_gateway().await
}

#[tauri::command]
pub async fn stop_gateway(state: State<'_, AppState>) -> Result<GatewayStateView, String> {
    state.stop_gateway(Duration::from_secs(3)).await?;
    Ok(state.gateway_state().await)
}

#[tauri::command]
pub async fn reveal_gateway_token(state: State<'_, AppState>) -> Result<String, String> {
    state.reveal_token().await
}

#[tauri::command]
pub async fn regenerate_gateway_token(state: State<'_, AppState>) -> Result<String, String> {
    state.regenerate_token().await
}
