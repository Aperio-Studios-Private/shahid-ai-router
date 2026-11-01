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
use zeroize::Zeroizing;

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
    // Cloud auto-backup schedule state (Phase 12). Short critical sections,
    // never held across an await.
    cloud_dirty: std::sync::atomic::AtomicBool,
    cloud_last_upload: std::sync::Mutex<Option<std::time::Instant>>,
    cloud_last_change: std::sync::Mutex<Option<std::time::Instant>>,
    cloud_last_error: std::sync::Mutex<Option<String>>,
    cloud_last_upload_at: std::sync::Mutex<Option<String>>,
    /// Temp file staged by `cloud_restore` for the (unchanged) path-keyed
    /// preview/commit pair, plus the previewed mode.
    cloud_restore_stage: std::sync::Mutex<Option<(std::path::PathBuf, String)>>,
    /// App-data temp directory for cloud restores (set once at startup).
    cloud_tmp_dir_cached: std::sync::Mutex<Option<std::path::PathBuf>>,
}

pub const SETTING_GATEWAY_TOKEN_REF: &str = "gateway_token_ref";
pub const SETTING_GATEWAY: &str = "gateway";
pub const SETTING_RETENTION_DAYS: &str = "retention_days";
pub const DEFAULT_RETENTION_DAYS: i64 = 30;
// Cloud backup (Phase 12, ADR-019). INVARIANT (review F-12-8): none of these
// keys may ever join the backup-export allowlist in backup.rs — asserted by
// `cloud_settings_never_exported`.
pub const SETTING_CLOUD_PROVIDER: &str = "cloud_provider";
pub const SETTING_CLOUD_MODE: &str = "cloud_mode";
pub const SETTING_CLOUD_SCHEDULE: &str = "cloud_schedule_enabled";
pub const SETTING_CLOUD_DSN_REF: &str = "cloud_dsn_ref";
pub const SETTING_CLOUD_KEY_REF: &str = "cloud_key_ref";
pub const SETTING_CLOUD_PASSPHRASE_REF: &str = "cloud_passphrase_ref";
pub const SETTING_CLOUD_DEVICE_ID: &str = "cloud_device_id";
pub const CLOUD_PRUNE_KEEP: usize = 10;

impl AppState {
    pub fn new(db: Db, vault: Arc<dyn SecretStore>) -> Self {
        Self {
            db,
            vault,
            gateway: RwLock::new(None),
            gateway_error: RwLock::new(None),
            token_ref: RwLock::new(None),
            cloud_dirty: std::sync::atomic::AtomicBool::new(false),
            cloud_last_upload: std::sync::Mutex::new(None),
            cloud_last_change: std::sync::Mutex::new(None),
            cloud_last_error: std::sync::Mutex::new(None),
            cloud_last_upload_at: std::sync::Mutex::new(None),
            cloud_restore_stage: std::sync::Mutex::new(None),
            cloud_tmp_dir_cached: std::sync::Mutex::new(None),
        }
    }

    /// Marks the configuration mutated so the cloud scheduler uploads the
    /// next quiet window (Phase 12). Call from every mutating management
    /// command (enumerated in the phase-12 plan §3.3).
    pub fn mark_cloud_dirty(&self) {
        self.cloud_dirty
            .store(true, std::sync::atomic::Ordering::SeqCst);
        *self.cloud_last_change.lock().unwrap() = Some(std::time::Instant::now());
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
    pub async fn ensure_token(&self) -> Result<Zeroizing<Vec<u8>>, String> {
        // In-memory cache.
        if let Some(secret_ref) = self.token_ref.read().await.clone() {
            match self.vault.retrieve(&secret_ref) {
                Ok(value) => return Ok(Zeroizing::new(value)),
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
                    return Ok(Zeroizing::new(value));
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
        String::from_utf8(material.value.to_vec())
            .map_err(|_| "token bytes are not valid UTF-8".to_string())
    }

    /// The encoded token, for the explicit reveal action (UI masks by
    /// default; docs/06).
    ///
    /// # Errors
    /// Returns an error message when no token exists or the vault fails.
    pub async fn reveal_token(&self) -> Result<String, String> {
        let value = self.ensure_token().await?;
        String::from_utf8(value.to_vec()).map_err(|_| "token bytes are not valid UTF-8".to_string())
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

    async fn gateway_config(&self, token: Zeroizing<Vec<u8>>) -> Result<GatewayConfig, String> {
        let settings = self
            .db
            .get_setting(SETTING_GATEWAY)
            .await
            .map_err(|e| format!("db unavailable: {e}"))?;
        // Protocol-surface dependencies: per-gateway ApiDeps (Phase 3+4).
        // FR-021: per-deployment concurrency bound; setting applies at
        // gateway (re)start (R4-06).
        let deployment_concurrency = settings
            .as_ref()
            .and_then(|o| o.get("deployment_concurrency"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| usize::try_from(v).ok().filter(|c| *c >= 1))
            .unwrap_or(4);
        let attribution = self
            .db
            .get_setting("openrouter_attribution")
            .await
            .ok()
            .flatten()
            .and_then(|v| sair_providers::openrouter::AttributionConfig::from_value(Some(&v)))
            .unwrap_or_default();
        let upstream_idle_read_ms = settings
            .as_ref()
            .and_then(|o| o.get("upstream_idle_read_ms"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(300_000);
        let api = Arc::new(sair_gateway::api::ApiDeps::new(
            self.db.clone(),
            self.vault.clone(),
            deployment_concurrency,
            attribution,
            upstream_idle_read_ms,
        ));
        let mut config = GatewayConfig {
            token: Arc::new(token),
            api: Some(api),
            ..GatewayConfig::default()
        };
        // (token type is Zeroizing<Vec<u8>> — moved into the Arc directly.)
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

// ---------------------------------------------------------------------------
// Cloud auto-backup (Phase 12, ADR-019) — config accessors, schedule decision,
// and the upload routine. Vault reads/writes and the envelope build run via
// spawn_blocking (SecretStore is sync; Argon2 is CPU-bound — review F-12-3).
// Failures never panic or block the app: they surface as status + warn logs.
// ---------------------------------------------------------------------------

/// Resolved cloud configuration. Secret material is resolved from the vault
/// on demand and never logged.
#[derive(Debug, Clone, Default)]
pub struct CloudConfig {
    pub provider: Option<String>,
    pub mode: Option<String>,
    pub schedule_enabled: bool,
    pub device_id: String,
    pub dsn: Option<Zeroizing<String>>,
    pub key: Option<Zeroizing<String>>,
    pub passphrase: Option<Zeroizing<String>>,
    pub has_dsn: bool,
    pub has_passphrase: bool,
}

impl AppState {
    /// Stable per-install device label (no PII beyond a random/hash label).
    pub async fn cloud_device_id(&self) -> Result<String, String> {
        if let Some(id) = self
            .db
            .get_setting(SETTING_CLOUD_DEVICE_ID)
            .await
            .map_err(|e| format!("db unavailable: {e}"))?
            .and_then(|v| v.as_str().map(str::to_string))
        {
            return Ok(id);
        }
        let hostname = hostname().unwrap_or_else(|| "device".to_string());
        let id = crate::cloud_backup::sha256_hex(format!("sair-device:{hostname}").as_bytes())
            [..16]
            .to_string()
            + "-"
            + &crate::cloud_backup::sha256_hex(
                chrono::Utc::now()
                    .timestamp_nanos_opt()
                    .unwrap_or_default()
                    .to_le_bytes()
                    .as_slice(),
            )[..4];
        self.db
            .set_setting(SETTING_CLOUD_DEVICE_ID, &serde_json::json!(id))
            .await
            .map_err(|e| format!("db unavailable: {e}"))?;
        Ok(id)
    }

    pub(crate) async fn vault_store(&self, value: &str) -> Result<String, String> {
        let vault = self.vault.clone();
        let bytes = Zeroizing::new(value.as_bytes().to_vec());
        tauri::async_runtime::spawn_blocking(move || {
            vault
                .store(bytes.as_slice())
                .map(|r| r.as_str().to_string())
                .map_err(|e| format!("vault unavailable: {e}"))
        })
        .await
        .map_err(|e| format!("vault task failed: {e}"))?
    }

    pub(crate) async fn vault_retrieve(
        &self,
        secret_ref: &SecretRef,
    ) -> Result<Zeroizing<String>, String> {
        let vault = self.vault.clone();
        let secret_ref = secret_ref.clone();
        tauri::async_runtime::spawn_blocking(move || {
            vault
                .retrieve(&secret_ref)
                .map(|bytes| Zeroizing::new(String::from_utf8_lossy(&bytes).to_string()))
                .map_err(|e| format!("vault unavailable: {e}"))
        })
        .await
        .map_err(|e| format!("vault task failed: {e}"))?
    }

    /// Reads the cloud configuration; resolves vault refs only when asked.
    pub async fn cloud_config(&self, resolve_secrets: bool) -> Result<CloudConfig, String> {
        let get = |k: &str| {
            let db = self.db.clone();
            let k = k.to_string();
            async move {
                match db.get_setting(&k).await {
                    Ok(v) => Ok(v.and_then(|v| v.as_str().map(str::to_string))),
                    Err(e) => Err(format!("db unavailable: {e}")),
                }
            }
        };
        let provider = get(SETTING_CLOUD_PROVIDER).await?;
        let mode = get(SETTING_CLOUD_MODE).await?;
        let schedule_enabled = get(SETTING_CLOUD_SCHEDULE)
            .await?
            .map(|s| s == "true")
            .unwrap_or(false);
        let device_id = self.cloud_device_id().await?;
        let dsn_ref = get(SETTING_CLOUD_DSN_REF).await?;
        let key_ref = get(SETTING_CLOUD_KEY_REF).await?;
        let passphrase_ref = get(SETTING_CLOUD_PASSPHRASE_REF).await?;
        let mut config = CloudConfig {
            provider,
            mode,
            schedule_enabled,
            device_id,
            has_dsn: dsn_ref.is_some(),
            has_passphrase: passphrase_ref.is_some(),
            ..CloudConfig::default()
        };
        if resolve_secrets {
            if let Some(raw) = dsn_ref
                && let Ok(secret_ref) = SecretRef::parse(&raw)
            {
                config.dsn = Some(self.vault_retrieve(&secret_ref).await?);
            }
            if let Some(raw) = key_ref
                && let Ok(secret_ref) = SecretRef::parse(&raw)
            {
                config.key = Some(self.vault_retrieve(&secret_ref).await?);
            }
            if let Some(raw) = passphrase_ref
                && let Ok(secret_ref) = SecretRef::parse(&raw)
            {
                config.passphrase = Some(self.vault_retrieve(&secret_ref).await?);
            }
        }
        Ok(config)
    }

    /// Builds the configured provider instance (Neon v1; Supabase supported).
    pub async fn cloud_provider(
        &self,
    ) -> Result<Arc<dyn crate::cloud_backup::CloudBackupProvider>, String> {
        let config = self.cloud_config(true).await?;
        let Some(provider) = config.provider.as_deref() else {
            return Err("cloud backup is not configured".to_string());
        };
        match provider {
            "neon" => {
                let dsn = config
                    .dsn
                    .as_ref()
                    .ok_or_else(|| "cloud connection string missing from vault".to_string())?;
                Ok(Arc::new(crate::cloud_backup::NeonProvider::new(
                    dsn.as_str(),
                )?))
            }
            "github-gist" => {
                let key = config
                    .key
                    .as_ref()
                    .ok_or_else(|| "GitHub token missing from vault".to_string())?;
                let dsn = config
                    .dsn
                    .as_ref()
                    .ok_or_else(|| "gist id missing from vault".to_string())?;
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| format!("client build failed: {e}"))?;
                Ok(Arc::new(crate::cloud_backup::GistProvider::new(
                    key.as_str(),
                    dsn.as_str(),
                    client,
                )?))
            }
            "supabase" => {
                let key = config
                    .key
                    .as_ref()
                    .ok_or_else(|| "supabase API key missing from vault".to_string())?;
                let dsn = config
                    .dsn
                    .as_ref()
                    .ok_or_else(|| "supabase project URL missing from vault".to_string())?;
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .map_err(|e| format!("client build failed: {e}"))?;
                Ok(Arc::new(crate::cloud_backup::SupabaseProvider::new(
                    dsn.as_str(),
                    key.as_str(),
                    client,
                )?))
            }
            other => Err(format!("unknown cloud provider: {other}")),
        }
    }

    /// Scheduler tick decision: pure predicate + stored Instants.
    pub async fn cloud_due_now(&self) -> bool {
        let (schedule_enabled, dirty, since_upload, since_change) = {
            let last_upload = *self.cloud_last_upload.lock().unwrap();
            let last_change = *self.cloud_last_change.lock().unwrap();
            let now = std::time::Instant::now();
            let since_upload = last_upload.map(|t| now - t);
            let since_change = last_change.map(|t| now - t);
            let dirty = self.cloud_dirty.load(std::sync::atomic::Ordering::SeqCst);
            let schedule_enabled = self
                .db
                .get_setting(SETTING_CLOUD_SCHEDULE)
                .await
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(str::to_string))
                .map(|s| s == "true")
                .unwrap_or(false);
            (schedule_enabled, dirty, since_upload, since_change)
        };
        crate::cloud_backup::should_upload(schedule_enabled, dirty, since_upload, since_change)
    }

    /// Builds the envelope for the configured mode and uploads it; prunes to
    /// [`CLOUD_PRUNE_KEEP`]. Returns the upload summary.
    ///
    /// # Errors
    /// Configuration, vault, DB, crypto, or transport failures (all mapped to
    /// safe strings; the DSN never appears — see `cloud_backup`).
    pub async fn cloud_run_upload(&self, force: bool) -> Result<serde_json::Value, String> {
        match self.cloud_run_upload_inner(force).await {
            Ok(v) => Ok(v),
            Err(e) => {
                *self.cloud_last_error.lock().unwrap() = Some(e.clone());
                Err(e)
            }
        }
    }

    async fn cloud_run_upload_inner(&self, force: bool) -> Result<serde_json::Value, String> {
        let config = self.cloud_config(true).await?;
        let Some(provider_name) = config.provider.clone() else {
            /* used in the log below */
            return Err("cloud backup is not configured".to_string());
        };
        if !force && !config.schedule_enabled {
            return Err("cloud schedule is disabled".to_string());
        }
        let provider = self.cloud_provider().await?;
        let mode = config
            .mode
            .clone()
            .unwrap_or_else(|| "encrypted".to_string());
        let passphrase = if mode == "encrypted" {
            Some(
                config
                    .passphrase
                    .as_ref()
                    .ok_or_else(|| "backup passphrase missing from vault".to_string())?
                    .to_string(),
            )
        } else {
            None
        };
        let envelope = crate::cloud_backup::build_upload_envelope(
            &self.db,
            self.vault.as_ref(),
            &mode,
            passphrase,
        )
        .await?;
        let device = config.device_id.clone();
        let sha = envelope.sha256.clone();
        let json = envelope.json.clone();
        let env_mode = envelope.mode.clone();
        let json_len = json.len();
        provider
            .upload(json, sha.clone(), device.clone(), env_mode.clone())
            .await?;
        let pruned = provider
            .prune(device.clone(), CLOUD_PRUNE_KEEP)
            .await
            .unwrap_or(0);
        *self.cloud_last_upload.lock().unwrap() = Some(std::time::Instant::now());
        *self.cloud_last_upload_at.lock().unwrap() = Some(chrono::Utc::now().to_rfc3339());
        self.cloud_dirty
            .store(false, std::sync::atomic::Ordering::SeqCst);
        *self.cloud_last_error.lock().unwrap() = None;
        tracing::info!(provider = %provider_name, mode = %env_mode, bytes = json_len, pruned, "cloud backup uploaded");
        Ok(serde_json::json!({
            "mode": env_mode,
            "bytes": json_len,
            "sha256": sha,
            "device_id": device,
            "pruned": pruned,
        }))
    }

    /// Stages a downloaded snapshot for the unchanged path-keyed
    /// preview/commit flow and returns (temp_path, mode).
    pub async fn cloud_stage_download(
        &self,
        snapshot_id: &str,
        passphrase: Option<String>,
    ) -> Result<(std::path::PathBuf, String), String> {
        let provider = self.cloud_provider().await?;
        let payload = provider.download(snapshot_id.to_string()).await?;
        // Validate with the existing parser (size cap + structure + config
        // integrity) BEFORE anything touches disk or the held slot.
        let envelope = crate::backup::parse_envelope(payload.as_bytes())
            .map_err(|e| format!("snapshot is not a valid backup: {e}"))?;
        if envelope.mode == "encrypted" {
            // The preview step needs the passphrase; fall back to the vault
            // (same-device re-restore) before failing.
            if passphrase.is_none() {
                let config = self.cloud_config(true).await?;
                if config.passphrase.is_none() {
                    return Err(
                        "this snapshot is encrypted — enter its backup passphrase".to_string()
                    );
                }
            }
        }
        let dir = self
            .cloud_tmp_dir()
            .await
            .ok_or_else(|| "cannot resolve app data dir".to_string())?;
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("mkdir failed: {e}"))?;
        let safe_id: String = snapshot_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
            .collect();
        let path = dir.join(format!("cloud-{safe_id}.sair"));
        tokio::fs::write(&path, payload.as_bytes())
            .await
            .map_err(|e| format!("temp write failed: {e}"))?;
        *self.cloud_restore_stage.lock().unwrap() = Some((path.clone(), envelope.mode.clone()));
        Ok((path, envelope.mode))
    }

    pub fn cloud_take_staged(&self) -> Option<(std::path::PathBuf, String)> {
        self.cloud_restore_stage.lock().unwrap().clone()
    }

    pub fn cloud_clear_staged(&self, path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        *self.cloud_restore_stage.lock().unwrap() = None;
    }

    /// Last successful upload time (RFC-3339), for the UI status.
    pub fn cloud_last_upload_at(&self) -> Option<String> {
        self.cloud_last_upload_at.lock().unwrap().clone()
    }

    /// Last cloud failure (already DSN-free), for the UI status.
    pub fn cloud_last_error(&self) -> Option<String> {
        self.cloud_last_error.lock().unwrap().clone()
    }

    pub fn cloud_is_dirty(&self) -> bool {
        self.cloud_dirty.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Deletes the cloud vault items (connection string / key / passphrase)
    /// best-effort on disable. Blocking OS calls — spawn_blocking.
    pub async fn cloud_forget_secrets(&self) {
        for key in [
            SETTING_CLOUD_DSN_REF,
            SETTING_CLOUD_KEY_REF,
            SETTING_CLOUD_PASSPHRASE_REF,
        ] {
            let raw = self
                .db
                .get_setting(key)
                .await
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(str::to_string));
            if let Some(raw) = raw
                && let Ok(secret_ref) = SecretRef::parse(&raw)
            {
                let vault = self.vault.clone();
                let label = key.to_string();
                let result =
                    tauri::async_runtime::spawn_blocking(move || vault.delete(&secret_ref))
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r.map_err(|e| e.to_string()));
                if let Err(e) = result {
                    tracing::warn!(key = %label, error = %e, "could not delete cloud vault item");
                }
            }
        }
    }

    async fn cloud_tmp_dir(&self) -> Option<std::path::PathBuf> {
        // Reuse the DB directory: app_data is already resolved for the DB in
        // lib.rs; the Db handle does not carry the path, so resolve via the
        // DB file convention by asking the caller (set once at startup).
        self.cloud_tmp_dir_cached.lock().unwrap().clone()
    }
}

impl AppState {
    pub fn set_cloud_tmp_dir(&self, dir: std::path::PathBuf) {
        *self.cloud_tmp_dir_cached.lock().unwrap() = Some(dir);
    }
}

fn hostname() -> Option<String> {
    hostname_impl()
}

#[cfg(target_os = "macos")]
fn hostname_impl() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "macos"))]
fn hostname_impl() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}
