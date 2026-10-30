//! Application state and backend initialization (Phase 2).
//!
//! Startup order (docs/02 failure isolation): open DB → run migrations →
//! purge expired request records (FR-020) → ensure gateway token in the OS
//! vault (fail-closed, F-9) → start gateway. Any failure leaves the gateway
//! stopped with an explicit error state — never a silently degraded auth.

use std::sync::Arc;
use std::time::Duration;

use sair_gateway::{GatewayConfig, GatewayHandle, TokenManager};
use sair_persistence::Db;
use sair_vault::{SecretRef, SecretStore};
use serde::Serialize;
use tokio::sync::RwLock;

/// Serializable view for the UI. Contains no secret material.
#[derive(Debug, Clone, Serialize)]
pub struct GatewayStateView {
    pub running: bool,
    pub addr: Option<String>,
    pub error: Option<String>,
}

pub struct AppState {
    pub db: Db,
    pub vault: Arc<dyn SecretStore>,
    gateway: RwLock<Option<GatewayHandle>>,
    gateway_error: RwLock<Option<String>>,
    token_ref: RwLock<Option<SecretRef>>,
}

pub const SETTING_GATEWAY_TOKEN_REF: &str = "gateway_token_ref";
pub const SETTING_GATEWAY: &str = "gateway";
pub const SETTING_RETENTION_DAYS: &str = "retention_days";
pub const DEFAULT_RETENTION_DAYS: i64 = 30;

impl AppState {
    pub fn new(db: Db, vault: Arc<dyn SecretStore>) -> Self {
        Self {
            db,
            vault,
            gateway: RwLock::new(None),
            gateway_error: RwLock::new(None),
            token_ref: RwLock::new(None),
        }
    }

    /// Gateway state snapshot for IPC/UI. Never includes the token.
    pub async fn gateway_state(&self) -> GatewayStateView {
        let gateway = self.gateway.read().await;
        let error = self.gateway_error.read().await;
        GatewayStateView {
            running: gateway.is_some(),
            addr: gateway.as_ref().map(|h| h.addr().to_string()),
            error: error.clone(),
        }
    }

    /// Loads (or creates) the gateway token. Fail-closed: errors propagate.
    ///
    /// # Errors
    /// Returns an error message when the vault or DB fails; the caller must
    /// not start the gateway without a token (review finding F-9).
    pub async fn ensure_token(&self) -> Result<Vec<u8>, String> {
        // In-memory cache.
        if let Some(secret_ref) = self.token_ref.read().await.clone() {
            match self.vault.retrieve(&secret_ref) {
                Ok(value) => return Ok(value),
                Err(sair_vault::VaultError::NotFound) => {
                    tracing::warn!("vault item for gateway token vanished; recreating");
                }
                Err(e) => return Err(format!("vault unavailable: {e}")),
            }
        }
        // Persisted ref.
        let stored = self
            .db
            .get_setting(SETTING_GATEWAY_TOKEN_REF)
            .await
            .map_err(|e| format!("db unavailable: {e}"))?
            .and_then(|v| v.as_str().map(str::to_string));
        if let Some(raw) = stored
            && let Ok(secret_ref) = SecretRef::parse(&raw)
        {
            match self.vault.retrieve(&secret_ref) {
                Ok(value) => {
                    *self.token_ref.write().await = Some(secret_ref);
                    return Ok(value);
                }
                Err(sair_vault::VaultError::NotFound) => {
                    tracing::warn!("persisted gateway token ref not in vault; recreating");
                }
                Err(e) => return Err(format!("vault unavailable: {e}")),
            }
        }
        // Create fresh: new vault item → persist ref (crash-safe order, F-10).
        let manager = TokenManager::new(self.vault.clone());
        let material = manager.create().map_err(|e| e.to_string())?;
        self.db
            .set_setting(
                SETTING_GATEWAY_TOKEN_REF,
                &serde_json::json!(material.secret_ref.as_str()),
            )
            .await
            .map_err(|e| format!("db unavailable: {e}"))?;
        *self.token_ref.write().await = Some(material.secret_ref);
        Ok(material.value)
    }

    /// Regenerates the token: create new item → persist ref → hot-swap the
    /// running gateway (SEC2-02: old token revoked immediately, no restart)
    /// → delete old item (F-10 order). Returns the new encoded token once.
    ///
    /// # Errors
    /// Returns an error message when the vault or DB fails.
    pub async fn regenerate_token(&self) -> Result<String, String> {
        let manager = TokenManager::new(self.vault.clone());
        let material = manager.create().map_err(|e| e.to_string())?;
        let previous = self
            .token_ref
            .write()
            .await
            .replace(material.secret_ref.clone());
        self.db
            .set_setting(
                SETTING_GATEWAY_TOKEN_REF,
                &serde_json::json!(material.secret_ref.as_str()),
            )
            .await
            .map_err(|e| format!("db unavailable: {e}"))?;
        // Hot-swap the live gateway before deleting the old vault item so a
        // crash between the steps never leaves a tokenless gateway.
        if let Some(handle) = self.gateway.read().await.as_ref() {
            handle.update_token(material.value.clone()).await;
        }
        if let Some(old_ref) = previous {
            // Best-effort cleanup; a leftover old item is harmless (no ref
            // points at it) and is reported for diagnostics.
            if let Err(e) = manager.delete(&old_ref) {
                tracing::warn!(error = %e, "could not delete replaced gateway token item");
            }
        }
        String::from_utf8(material.value).map_err(|_| "token bytes are not valid UTF-8".to_string())
    }

    /// The encoded token, for the explicit reveal action (UI masks by
    /// default; docs/06).
    ///
    /// # Errors
    /// Returns an error message when no token exists or the vault fails.
    pub async fn reveal_token(&self) -> Result<String, String> {
        let value = self.ensure_token().await?;
        String::from_utf8(value).map_err(|_| "token bytes are not valid UTF-8".to_string())
    }

    /// Starts the gateway. Bind failures and token failures are surfaced as
    /// errors (FR-018; F-9 fail-closed).
    ///
    /// # Errors
    /// Returns an error message when already running, the token is
    /// unavailable, or the bind fails.
    pub async fn start_gateway(&self) -> Result<GatewayStateView, String> {
        {
            let gateway = self.gateway.read().await;
            if gateway.is_some() {
                return Err("gateway is already running".to_string());
            }
        }
        let token = self.ensure_token().await?;
        let config = self.gateway_config(token).await?;
        let handle = match GatewayHandle::start(config).await {
            Ok(handle) => handle,
            Err(e) => {
                *self.gateway_error.write().await = Some(e.to_string());
                return Err(e.to_string());
            }
        };
        let view = GatewayStateView {
            running: true,
            addr: Some(handle.addr().to_string()),
            error: None,
        };
        *self.gateway_error.write().await = None;
        *self.gateway.write().await = Some(handle);
        tracing::info!(addr = ?view.addr, "gateway started");
        Ok(view)
    }

    /// Stops the gateway, draining in-flight requests within `grace`.
    ///
    /// # Errors
    /// Returns an error message when the drain exceeds the grace period.
    pub async fn stop_gateway(&self, grace: Duration) -> Result<(), String> {
        let handle = self.gateway.write().await.take();
        let Some(handle) = handle else {
            return Ok(());
        };
        let addr = handle.addr();
        if let Err(e) = handle.stop(grace).await {
            *self.gateway_error.write().await = Some(e.to_string());
            return Err(e.to_string());
        }
        tracing::info!(%addr, "gateway stopped");
        Ok(())
    }

    async fn gateway_config(&self, token: Vec<u8>) -> Result<GatewayConfig, String> {
        let settings = self
            .db
            .get_setting(SETTING_GATEWAY)
            .await
            .map_err(|e| format!("db unavailable: {e}"))?;
        let mut config = GatewayConfig {
            token: Arc::new(token),
            ..GatewayConfig::default()
        };
        if let Some(obj) = settings {
            if let Some(port) = obj.get("port").and_then(serde_json::Value::as_u64) {
                if port == 0 || port > u16::MAX as u64 {
                    return Err(format!("invalid gateway port: {port}"));
                }
                config.port = port as u16;
            }
            if let Some(limit) = obj
                .get("body_limit_bytes")
                .and_then(serde_json::Value::as_u64)
            {
                // Floor 1: a zero limit must not disable the bound (SEC2-09).
                config.body_limit_bytes = usize::try_from(limit)
                    .ok()
                    .filter(|v| *v >= 1)
                    .ok_or_else(|| "invalid body_limit_bytes".to_string())?;
            }
            if let Some(concurrency) = obj.get("concurrency").and_then(serde_json::Value::as_u64) {
                config.concurrency = usize::try_from(concurrency)
                    .ok()
                    .filter(|v| *v >= 1)
                    .ok_or_else(|| "invalid concurrency".to_string())?;
            }
            if let Some(timeout) = obj.get("timeout_ms").and_then(serde_json::Value::as_u64) {
                config.timeout_ms = timeout.max(1_000);
            }
            if let Some(header_timeout) = obj
                .get("header_timeout_ms")
                .and_then(serde_json::Value::as_u64)
            {
                config.header_timeout_ms = header_timeout.max(1_000);
            }
        }
        Ok(config)
    }

    /// Records a degraded-start failure so the UI shows an explicit error
    /// instead of a bare "Stopped" (SEC2-04).
    pub fn set_gateway_error(&self, error: String) {
        if let Ok(mut guard) = self.gateway_error.try_write() {
            *guard = Some(error);
        }
    }
}

/// Retention window in days (FR-020), floor 1.
pub async fn retention_days(db: &Db) -> i64 {
    match db.get_setting(SETTING_RETENTION_DAYS).await {
        Ok(Some(v)) => v.as_i64().unwrap_or(DEFAULT_RETENTION_DAYS).max(1),
        _ => DEFAULT_RETENTION_DAYS,
    }
}

/// Deletes request records older than the retention window.
pub async fn purge_expired_records(db: &Db) {
    let days = retention_days(db).await;
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
    match db.purge_request_records_before(cutoff).await {
        Ok(n) if n > 0 => {
            tracing::info!(removed = n, "retention purge removed old request records");
        }
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "retention purge failed"),
    }
}
