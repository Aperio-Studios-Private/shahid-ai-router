//! Tauri desktop shell for Shahid AI Router.
//!
//! Owns the UI process and (from Phase 2/3 onward) the gateway lifecycle.
//! The install of the ring rustls crypto provider happens here, before any
//! outbound HTTP exists (ADR-006).

use serde::Serialize;

/// Information shown by the About screen and used by Phase 1 build evidence.
#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub gateway_running: bool,
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        gateway_running: false,
    }
}

pub fn run() {
    // ADR-006: install the ring crypto provider once, before any client is
    // constructed. Failing loudly here is correct: TLS is mandatory for all
    // outbound provider calls.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("rustls ring crypto provider must install exactly once");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_app_info])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
