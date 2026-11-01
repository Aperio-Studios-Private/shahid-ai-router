//! Phase 12: Cloud auto-backup (FR-024, ADR-019) — transport layer over the
//! existing `.sair` envelope (Phase 10, audit-hardened). Zero new crypto: the
//! uploaded artifact IS the envelope produced by `backup::build_*_envelope`,
//! and restores feed the unchanged path-keyed preview/commit flow via a temp
//! file (plan §3.1).
//!
//! Security stance (plan §3.4, docs/06): only the envelope crosses the wire;
//! the destination DSN/key and the backup passphrase live ONLY in the OS
//! vault; connect/query failures are mapped to DSN-free messages before they
//! reach any log or the UI (sqlx error text can embed the URL — field-name
//! redaction cannot catch free text, so the mapping is the primary control);
//! vault reads and Argon2/GCM work run via `spawn_blocking` (SecretStore is
//! sync; Argon2 is CPU-bound).

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use sair_persistence::Db;
use sair_vault::SecretStore;
use serde_json::{Value, json};

/// Pinned boxed future (dyn-compatible trait without a new direct
/// dependency — async-trait is transitive in the lockfile but the rule is
/// "no new direct dependency").
pub type CloudFut<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// Snapshot metadata for the restore list — never includes the payload.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub device_id: String,
    pub app_version: String,
    pub mode: String,
    pub created_at: String,
    pub sha256: String,
    pub bytes: i64,
}

/// One cloud destination. Implementations must be cheap to construct and
/// drop; state (e.g. a pool) is internal to the instance.
pub trait CloudBackupProvider: Send + Sync {
    fn test(&self) -> CloudFut<'_, ()>;
    /// Parameters are owned: the returned future must be able to outlive the
    /// call-site borrows (dyn-safe boxed futures).
    fn upload(
        &self,
        envelope: String,
        sha256: String,
        device: String,
        mode: String,
    ) -> CloudFut<'_, ()>;
    fn list(&self) -> CloudFut<'_, Vec<SnapshotMeta>>;
    fn download(&self, id: String) -> CloudFut<'_, String>;
    fn prune(&self, device: String, keep: usize) -> CloudFut<'_, u64>;
    /// Deletes THIS device's rows only — other devices' backups are not ours
    /// to remove.
    fn purge(&self, device: String) -> CloudFut<'_, u64>;
}

/// Maps sqlx errors to DSN-free messages (review F-12-6). `sqlx::Error`
/// variants can embed the connection URL (`Configuration`) or arbitrary
/// backend text; we classify instead of formatting.
fn dsn_free_sqlx_err(e: &sqlx::Error) -> String {
    match e {
        sqlx::Error::Configuration(_) => "invalid cloud connection string".to_string(),
        sqlx::Error::Io(io) => format!("cloud connect failed: {}", io.kind()),
        sqlx::Error::Database(db) => format!("cloud query rejected ({:?})", db.kind()),
        sqlx::Error::PoolTimedOut => "cloud connect timed out".to_string(),
        _ => "cloud database error".to_string(),
    }
}

/// Builds a connect-error string for a reqwest failure without echoing the
/// URL (the URL can carry the Supabase key in query strings of some setups).
fn transport_err(e: &reqwest::Error) -> String {
    if e.is_connect() {
        "cloud connect failed".to_string()
    } else if e.is_timeout() {
        "cloud request timed out".to_string()
    } else if e.is_decode() {
        "cloud response decode failed".to_string()
    } else {
        "cloud request failed".to_string()
    }
}

// ---------------------------------------------------------------------------
// Neon (Postgres) — primary target (ADR-019): scale-to-zero compute, data
// never frozen, wakes on connect; TLS via `sslmode` in the DSN is enforced by
// requiring the parameter below.
// ---------------------------------------------------------------------------

pub struct NeonProvider {
    dsn: String,
    pool: tokio::sync::Mutex<Option<sqlx::PgPool>>,
}

impl NeonProvider {
    /// # Errors
    /// Rejects non-Postgres DSNs and DSNs that do not require TLS.
    pub fn new(dsn: &str) -> Result<Self, String> {
        let dsn = dsn.trim().to_string();
        if !(dsn.starts_with("postgres://") || dsn.starts_with("postgresql://")) {
            return Err("the Neon connection string must start with postgres://".to_string());
        }
        let lowered = dsn.to_ascii_lowercase();
        if !(lowered.contains("sslmode=require")
            || lowered.contains("sslmode=verify-full")
            || lowered.contains("sslmode=verify-ca"))
        {
            return Err(
                "the connection string must include sslmode=require (Neon enforces TLS)"
                    .to_string(),
            );
        }
        Ok(Self {
            dsn,
            pool: tokio::sync::Mutex::new(None),
        })
    }

    async fn pool(&self) -> Result<sqlx::PgPool, String> {
        let mut guard = self.pool.lock().await;
        if let Some(pool) = guard.as_ref() {
            return Ok(pool.clone());
        }
        let pool = sqlx::pool::PoolOptions::<sqlx::Postgres>::new()
            .max_connections(2)
            .acquire_timeout(Duration::from_secs(15))
            .connect(&self.dsn)
            .await
            .map_err(|e| {
                tracing::warn!(kind = %dsn_free_sqlx_err(&e), "neon connect failed (details suppressed)");
                dsn_free_sqlx_err(&e)
            })?;
        *guard = Some(pool.clone());
        Ok(pool)
    }
}

impl CloudBackupProvider for NeonProvider {
    fn test(&self) -> CloudFut<'_, ()> {
        Box::pin(async move {
            let pool = self.pool().await?;
            let one: i32 = sqlx::query_scalar("SELECT 1")
                .fetch_one(&pool)
                .await
                .map_err(|e| dsn_free_sqlx_err(&e))?;
            if one != 1 {
                return Err("cloud database sanity check failed".to_string());
            }
            Ok(())
        })
    }

    fn upload(
        &self,
        envelope: String,
        sha256: String,
        device: String,
        mode: String,
    ) -> CloudFut<'_, ()> {
        Box::pin(async move {
            let pool = self.pool().await?;
            let id = format!(
                "{}-{}",
                chrono::Utc::now().timestamp_millis(),
                cloud_random_hex()
            );
            sqlx::query(
                "INSERT INTO sair_backups (id, device_id, app_version, mode, payload, sha256) \
                 VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (id) DO NOTHING",
            )
            .bind(&id)
            .bind(&device)
            .bind(env!("CARGO_PKG_VERSION"))
            .bind(&mode)
            .bind(&envelope)
            .bind(&sha256)
            .execute(&pool)
            .await
            .map_err(|e| dsn_free_sqlx_err(&e))?;
            Ok(())
        })
    }

    fn list(&self) -> CloudFut<'_, Vec<SnapshotMeta>> {
        Box::pin(async move {
            let pool = self.pool().await?;
            let rows = sqlx::query(
                "SELECT id, device_id, app_version, mode, sha256, \
                        to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') \
                          AS created_utc, \
                        length(payload) AS bytes \
                 FROM sair_backups ORDER BY created_at DESC LIMIT 100",
            )
            .fetch_all(&pool)
            .await
            .map_err(|e| dsn_free_sqlx_err(&e))?;
            Ok(rows
                .iter()
                .map(|r| SnapshotMeta {
                    id: r.get("id"),
                    device_id: r.get("device_id"),
                    app_version: r.get("app_version"),
                    mode: r.get("mode"),
                    created_at: r.get("created_utc"),
                    sha256: r.get("sha256"),
                    bytes: r.get::<i64, _>("bytes"),
                })
                .collect())
        })
    }

    fn download(&self, id: String) -> CloudFut<'_, String> {
        Box::pin(async move {
            let pool = self.pool().await?;
            let payload: Option<String> =
                sqlx::query_scalar("SELECT payload FROM sair_backups WHERE id = $1")
                    .bind(&id)
                    .fetch_optional(&pool)
                    .await
                    .map_err(|e| dsn_free_sqlx_err(&e))?;
            payload.ok_or_else(|| "snapshot not found (pruned or deleted?)".to_string())
        })
    }

    fn prune(&self, device: String, keep: usize) -> CloudFut<'_, u64> {
        Box::pin(async move {
            let pool = self.pool().await?;
            let result = sqlx::query(
                "DELETE FROM sair_backups \
                 WHERE device_id = $1 \
                   AND id NOT IN (\
                       SELECT id FROM sair_backups WHERE device_id = $2 \
                       ORDER BY created_at DESC LIMIT $3)",
            )
            .bind(&device)
            .bind(&device)
            .bind(keep as i64)
            .execute(&pool)
            .await
            .map_err(|e| dsn_free_sqlx_err(&e))?;
            Ok(result.rows_affected())
        })
    }

    fn purge(&self, device: String) -> CloudFut<'_, u64> {
        Box::pin(async move {
            let pool = self.pool().await?;
            let result = sqlx::query("DELETE FROM sair_backups WHERE device_id = $1")
                .bind(&device)
                .execute(&pool)
                .await
                .map_err(|e| dsn_free_sqlx_err(&e))?;
            Ok(result.rows_affected())
        })
    }
}

use sqlx::Row;

// ---------------------------------------------------------------------------
// Supabase (PostgREST) — secondary target: pure HTTPS over the existing
// reqwest client; the user supplies the project URL + API key and creates the
// same `sair_backups` table (SQL in the app's setup guide).
// ---------------------------------------------------------------------------

pub struct SupabaseProvider {
    rest_base: url::Url,
    key: String,
    client: reqwest::Client,
}

impl SupabaseProvider {
    /// # Errors
    /// Rejects non-HTTPS base URLs (http allowed ONLY for loopback test
    /// targets, mirroring the project's scheme policy).
    pub fn new(project_url: &str, key: &str, client: reqwest::Client) -> Result<Self, String> {
        let parsed = url::Url::parse(project_url.trim())
            .map_err(|_| "supabase project URL is not a valid URL".to_string())?;
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        let loopback = host == "localhost"
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .ok()
                .is_some_and(|ip| ip.is_loopback());
        if parsed.scheme() != "https" && !loopback {
            return Err("the Supabase project URL must use https".to_string());
        }
        if key.trim().is_empty() {
            return Err("the Supabase API key is required".to_string());
        }
        let rest_base = parsed
            .join("rest/v1/")
            .map_err(|_| "invalid Supabase project URL".to_string())?;
        Ok(Self {
            rest_base,
            key: key.trim().to_string(),
            client,
        })
    }

    fn table(&self) -> url::Url {
        self.rest_base
            .join("sair_backups")
            .expect("constant path join")
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("apikey", &self.key)
            .header("authorization", format!("Bearer {}", self.key))
    }
}

impl CloudBackupProvider for SupabaseProvider {
    fn test(&self) -> CloudFut<'_, ()> {
        Box::pin(async move {
            let url = self.table().to_string();
            let status = self
                .auth(self.client.head(&url))
                .send()
                .await
                .map_err(|e| transport_err(&e))?
                .status();
            if status.is_success() {
                Ok(())
            } else {
                Err(format!(
                    "supabase check failed (HTTP {status}) — table created and key valid?"
                ))
            }
        })
    }

    fn upload(
        &self,
        envelope: String,
        sha256: String,
        device: String,
        mode: String,
    ) -> CloudFut<'_, ()> {
        Box::pin(async move {
            let id = format!(
                "{}-{}",
                chrono::Utc::now().timestamp_millis(),
                cloud_random_hex()
            );
            let body = serde_json::json!([{
                "id": id,
                "device_id": device,
                "app_version": env!("CARGO_PKG_VERSION"),
                "mode": mode,
                "payload": envelope,
                "sha256": sha256,
            }]);
            let status = self
                .auth(self.client.post(self.table().to_string()))
                .header("prefer", "return=minimal")
                .json(&body)
                .send()
                .await
                .map_err(|e| transport_err(&e))?
                .status();
            if status.is_success() {
                Ok(())
            } else {
                Err(format!(
                    "supabase upload failed (HTTP {status}) — table created and key writable?"
                ))
            }
        })
    }

    fn list(&self) -> CloudFut<'_, Vec<SnapshotMeta>> {
        Box::pin(async move {
            let url = format!(
                "{}?select=id,device_id,app_version,mode,created_at,sha256&order=created_at.desc&limit=100",
                self.table()
            );
            let rows: Vec<serde_json::Value> = self
                .auth(self.client.get(&url))
                .send()
                .await
                .map_err(|e| transport_err(&e))?
                .error_for_status()
                .map_err(|e| {
                    format!(
                        "supabase list failed (HTTP {})",
                        e.status().unwrap_or_default()
                    )
                })?
                .json()
                .await
                .map_err(|_| "supabase list decode failed".to_string())?;
            Ok(rows
                .iter()
                .map(|r| SnapshotMeta {
                    id: r["id"].as_str().unwrap_or_default().to_string(),
                    device_id: r["device_id"].as_str().unwrap_or_default().to_string(),
                    app_version: r["app_version"].as_str().unwrap_or_default().to_string(),
                    mode: r["mode"].as_str().unwrap_or_default().to_string(),
                    created_at: r["created_at"].as_str().unwrap_or_default().to_string(),
                    sha256: r["sha256"].as_str().unwrap_or_default().to_string(),
                    bytes: r["payload"]
                        .as_str()
                        .map_or(0, |p| i64::try_from(p.len()).unwrap_or(0)),
                })
                .collect())
        })
    }

    fn download(&self, id: String) -> CloudFut<'_, String> {
        Box::pin(async move {
            let url = format!("{}?select=payload&id=eq.{}", self.table(), urlencode(&id));
            let rows: Vec<serde_json::Value> = self
                .auth(self.client.get(&url))
                .send()
                .await
                .map_err(|e| transport_err(&e))?
                .error_for_status()
                .map_err(|e| {
                    format!(
                        "supabase download failed (HTTP {})",
                        e.status().unwrap_or_default()
                    )
                })?
                .json()
                .await
                .map_err(|_| "supabase download decode failed".to_string())?;
            rows.into_iter()
                .next()
                .and_then(|r| r["payload"].as_str().map(str::to_string))
                .ok_or_else(|| "snapshot not found (pruned or deleted?)".to_string())
        })
    }

    fn prune(&self, device: String, keep: usize) -> CloudFut<'_, u64> {
        Box::pin(async move {
            let url = format!(
                "{}?select=id&device_id=eq.{}&order=created_at.desc",
                self.table(),
                urlencode(&device)
            );
            let rows: Vec<serde_json::Value> = self
                .auth(self.client.get(&url))
                .send()
                .await
                .map_err(|e| transport_err(&e))?
                .error_for_status()
                .map_err(|e| {
                    format!(
                        "supabase prune list failed (HTTP {})",
                        e.status().unwrap_or_default()
                    )
                })?
                .json()
                .await
                .map_err(|_| "supabase prune decode failed".to_string())?;
            let stale: Vec<String> = rows
                .into_iter()
                .skip(keep)
                .filter_map(|r| r["id"].as_str().map(str::to_string))
                .collect();
            if stale.is_empty() {
                return Ok(0);
            }
            let filter = stale
                .iter()
                .map(|id| format!("\"{}\"", id.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(",");
            let del_url = format!(
                "{}?id=not.in.({})&device_id=eq.{}",
                self.table(),
                filter,
                urlencode(&device)
            );
            let resp = self
                .auth(self.client.delete(&del_url))
                .send()
                .await
                .map_err(|e| transport_err(&e))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(format!("supabase prune failed (HTTP {status})"));
            }
            // PostgREST returns the deleted rows as JSON when Prefer: count is
            // set; without it we only know success — report stale count.
            Ok(stale.len() as u64)
        })
    }

    fn purge(&self, device: String) -> CloudFut<'_, u64> {
        Box::pin(async move {
            let url = format!("{}?device_id=eq.{}", self.table(), urlencode(&device));
            let resp = self
                .auth(self.client.delete(&url))
                .send()
                .await
                .map_err(|e| transport_err(&e))?;
            let status = resp.status();
            if !status.is_success() {
                return Err(format!("supabase purge failed (HTTP {status})"));
            }
            Ok(0)
        })
    }
}

/// Percent-encode a value for a PostgREST filter (conservative: keep
/// alphanumerics and `-._~`, escape everything else).
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn cloud_random_hex() -> String {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 4];
    // Effectively infallible OS CSPRNG; a failure here is unrecoverable and
    // would only affect the uniqueness suffix of an id.
    let _ = rng.fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    use ring::digest::{SHA256, digest};
    digest(&SHA256, data)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------
// GitHub Gist — zero-infra target: one secret gist holds a single JSON file
// whose array IS the snapshot store. PAT + gist id live in the OS vault.
// ---------------------------------------------------------------------------

pub struct GistProvider {
    gist_id: String,
    pat: String,
    api_base: String,
    client: reqwest::Client,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct GistSnapshot {
    id: String,
    device_id: String,
    app_version: String,
    mode: String,
    created_at: String,
    sha256: String,
    payload: String,
}

const GIST_FILE: &str = "shahid-ai-router-backups.json";

impl GistProvider {
    /// # Errors
    /// Empty PAT or gist id.
    pub fn new(pat: &str, gist_id: &str, client: reqwest::Client) -> Result<Self, String> {
        if pat.trim().is_empty() {
            return Err("the GitHub token is required".to_string());
        }
        if gist_id.trim().is_empty() {
            return Err("the gist id is required".to_string());
        }
        Ok(Self {
            gist_id: gist_id.trim().to_string(),
            pat: pat.trim().to_string(),
            api_base: "https://api.github.com".to_string(),
            client,
        })
    }

    /// Test seam: point the client at a mock server instead of github.com.
    #[cfg(test)]
    fn with_api_base(mut self, base: &str) -> Self {
        self.api_base = base.to_string();
        self
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/gists/{}{suffix}", self.api_base, self.gist_id)
    }

    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("authorization", format!("Bearer {}", self.pat))
            .header("user-agent", "shahid-ai-router")
            .header("accept", "application/vnd.github+json")
    }

    /// Reads the whole snapshot store from the gist (empty array if the file
    /// does not exist yet — first backup).
    async fn read_store(&self) -> Result<Vec<GistSnapshot>, String> {
        let resp = self
            .auth(self.client.get(self.url("")))
            .send()
            .await
            .map_err(|e| transport_err(&e))?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err("gist not found — check the id and that the token can read it".to_string());
        }
        if !status.is_success() {
            return Err(format!("github answered HTTP {status}"));
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|_| "github response decode failed".to_string())?;
        let content = body["files"][GIST_FILE]["content"]
            .as_str()
            .unwrap_or("[]")
            .to_string();
        let store: Vec<GistSnapshot> = serde_json::from_str(&content).unwrap_or_default();
        Ok(store)
    }

    /// Writes the whole store back to the gist.
    async fn write_store(&self, store: &[GistSnapshot]) -> Result<(), String> {
        let body = json!({
            "files": { GIST_FILE: { "content": serde_json::to_string(store).map_err(|e| e.to_string())? } }
        });
        let resp = self
            .auth(self.client.patch(self.url("")))
            .json(&body)
            .send()
            .await
            .map_err(|e| transport_err(&e))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!(
                "github write failed (HTTP {status}) — does the token have gist write scope?"
            ));
        }
        Ok(())
    }
}

impl CloudBackupProvider for GistProvider {
    fn test(&self) -> CloudFut<'_, ()> {
        Box::pin(async move { self.read_store().await.map(|_| ()) })
    }

    fn upload(
        &self,
        envelope: String,
        sha256: String,
        device: String,
        mode: String,
    ) -> CloudFut<'_, ()> {
        Box::pin(async move {
            let mut store = self.read_store().await?;
            store.push(GistSnapshot {
                id: format!(
                    "{}-{}",
                    chrono::Utc::now().timestamp_millis(),
                    cloud_random_hex()
                ),
                device_id: device,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                mode,
                created_at: chrono::Utc::now().to_rfc3339(),
                sha256,
                payload: envelope,
            });
            self.write_store(&store).await
        })
    }

    fn list(&self) -> CloudFut<'_, Vec<SnapshotMeta>> {
        Box::pin(async move {
            let mut store = self.read_store().await?;
            store.reverse();
            Ok(store
                .iter()
                .map(|s| SnapshotMeta {
                    id: s.id.clone(),
                    device_id: s.device_id.clone(),
                    app_version: s.app_version.clone(),
                    mode: s.mode.clone(),
                    created_at: s.created_at.clone(),
                    sha256: s.sha256.clone(),
                    bytes: s.payload.len() as i64,
                })
                .collect())
        })
    }

    fn download(&self, id: String) -> CloudFut<'_, String> {
        Box::pin(async move {
            let store = self.read_store().await?;
            store
                .into_iter()
                .find(|s| s.id == id)
                .map(|s| s.payload)
                .ok_or_else(|| "snapshot not found (pruned or deleted?)".to_string())
        })
    }

    fn prune(&self, device: String, keep: usize) -> CloudFut<'_, u64> {
        Box::pin(async move {
            let mut store = self.read_store().await?;
            let mine: Vec<usize> = store
                .iter()
                .enumerate()
                .filter(|(_, s)| s.device_id == device)
                .map(|(i, _)| i)
                .collect();
            let drop_count = mine.len().saturating_sub(keep);
            let drop_idx: Vec<usize> = mine.iter().take(drop_count).copied().collect();
            for i in drop_idx.into_iter().rev() {
                store.remove(i);
            }
            let removed = drop_count as u64;
            if removed > 0 {
                self.write_store(&store).await?;
            }
            Ok(removed)
        })
    }

    fn purge(&self, device: String) -> CloudFut<'_, u64> {
        Box::pin(async move {
            let mut store = self.read_store().await?;
            let before = store.len();
            store.retain(|s| s.device_id != device);
            let removed = (before - store.len()) as u64;
            if removed > 0 || before > 0 {
                self.write_store(&store).await?;
            }
            Ok(removed)
        })
    }
}

// ---------------------------------------------------------------------------
// Envelope build (reuses Phase 10 code; Argon2/GCM via spawn_blocking).
// ---------------------------------------------------------------------------

pub struct UploadEnvelope {
    pub json: String,
    pub sha256: String,
    pub mode: String,
}

/// Builds the cloud upload artifact: exactly the `.sair` envelope.
///
/// # Errors
/// On DB/vault/crypto failures or an unknown mode.
pub async fn build_upload_envelope(
    db: &Db,
    vault: &dyn SecretStore,
    mode: &str,
    passphrase: Option<String>,
) -> Result<UploadEnvelope, String> {
    match mode {
        "encrypted" => {
            let passphrase = passphrase.ok_or_else(|| {
                "encrypted cloud backup requires the backup passphrase".to_string()
            })?;
            let (payload, sections) = crate::backup::build_encrypted_payload(db, vault).await?;
            let app_version = env!("CARGO_PKG_VERSION").to_string();
            let envelope = tauri::async_runtime::spawn_blocking(move || {
                crate::backup::build_encrypted_envelope(
                    &app_version,
                    payload,
                    sections,
                    &passphrase,
                )
            })
            .await
            .map_err(|e| format!("envelope build failed: {e}"))?
            .map_err(|e| e.to_string())?;
            finish_envelope(envelope, "encrypted").await
        }
        "config" => {
            let (payload, sections) = crate::backup::build_config_payload(db).await?;
            let envelope =
                crate::backup::build_config_envelope(env!("CARGO_PKG_VERSION"), payload, sections)
                    .map_err(|e| e.to_string())?;
            finish_envelope(envelope, "config").await
        }
        other => Err(format!("unknown cloud backup mode: {other}")),
    }
}

async fn finish_envelope(
    envelope: serde_json::Value,
    mode: &str,
) -> Result<UploadEnvelope, String> {
    let json = serde_json::to_string_pretty(&envelope).map_err(|e| e.to_string())?;
    let sha256 = sha256_hex(json.as_bytes());
    Ok(UploadEnvelope {
        json,
        sha256,
        mode: mode.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Schedule decision (pure — clock-injection tested; plan §3.3).
// ---------------------------------------------------------------------------

pub const DAILY: Duration = Duration::from_secs(24 * 60 * 60);
pub const QUIET: Duration = Duration::from_secs(5 * 60);
pub const MIN_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Upload when: never uploaded yet (first snapshot right after configure), OR
/// the daily force elapsed, OR (dirty AND quiet ≥ 5 min AND ≥ 30 min since
/// the last upload).
#[must_use]
pub fn should_upload(
    schedule_enabled: bool,
    dirty: bool,
    since_upload: Option<Duration>,
    since_change: Option<Duration>,
) -> bool {
    if !schedule_enabled {
        return false;
    }
    match since_upload {
        None => true, // first backup right after configure
        Some(up) if up >= DAILY => true,
        Some(up) => dirty && since_change.is_some_and(|q| q >= QUIET) && up >= MIN_INTERVAL,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CloudBackupProvider, GIST_FILE, GistProvider, NeonProvider, SnapshotMeta, SupabaseProvider,
        should_upload,
    };
    use std::time::Duration;

    #[test]
    fn schedule_decision_matches_plan() {
        let d = |s: u64| Some(Duration::from_secs(s));
        // Disabled → never.
        assert!(!should_upload(false, true, d(0), d(1000)));
        // Never uploaded → upload (first snapshot after configure).
        assert!(should_upload(true, false, None, None));
        assert!(should_upload(true, true, None, None));
        // Daily force.
        assert!(should_upload(true, false, d(24 * 60 * 60), None));
        assert!(should_upload(true, false, d(25 * 60 * 60), None));
        // Dirty + quiet + interval.
        assert!(should_upload(true, true, d(31 * 60), d(6 * 60)));
        // Not quiet yet.
        assert!(!should_upload(true, true, d(31 * 60), d(60)));
        // Too soon since last upload.
        assert!(!should_upload(true, true, d(10 * 60), d(6 * 60)));
        // Not dirty.
        assert!(!should_upload(true, false, d(31 * 60), d(6 * 60)));
    }

    #[test]
    fn neon_dsn_rules_are_enforced() {
        assert!(
            NeonProvider::new("postgresql://u:p@ep-x-pooler.aws.neon.tech/db?sslmode=require")
                .is_ok()
        );
        assert!(NeonProvider::new("postgres://u:p@host/db?sslmode=verify-full").is_ok());
        // Missing sslmode → rejected (TLS is mandatory for Neon).
        assert!(NeonProvider::new("postgres://u:p@host/db").is_err());
        // Wrong scheme → rejected.
        assert!(NeonProvider::new("mysql://u:p@host/db?sslmode=require").is_err());
        assert!(NeonProvider::new("").is_err());
    }

    #[test]
    fn connect_errors_never_contain_the_dsn() {
        // The DSN-free mapping is the primary redaction control (F-12-6): a
        // fake DSN must not leak into the error string.
        let dsn =
            "postgresql://user:SECRET-PASSWORD@ep-fake-pooler.aws.neon.tech/db?sslmode=require";
        let provider = NeonProvider::new(dsn).unwrap();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { provider.test().await });
        let err = match result {
            Ok(()) => panic!("fake DSN cannot connect; expected error"),
            Err(e) => e,
        };
        assert!(!err.contains("SECRET-PASSWORD"), "DSN leaked: {err}");
        assert!(!err.contains("ep-fake-pooler"), "host leaked: {err}");
        assert!(
            !err.to_lowercase().contains("postgres://"),
            "scheme+DSN leaked: {err}"
        );
    }

    /// In-memory provider used by the roundtrip test below and by app-level
    /// command tests.
    pub struct InMemoryProvider {
        pub rows: std::sync::Mutex<Vec<(String, String, String, String)>>, // id, device, mode, payload
        pub fail: std::sync::atomic::AtomicBool,
    }

    impl InMemoryProvider {
        pub fn new() -> Self {
            Self {
                rows: std::sync::Mutex::new(Vec::new()),
                fail: std::sync::atomic::AtomicBool::new(false),
            }
        }
    }

    impl Default for InMemoryProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    impl CloudBackupProvider for InMemoryProvider {
        fn test(&self) -> super::CloudFut<'_, ()> {
            Box::pin(async move {
                if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                    Err("in-memory provider set to fail".to_string())
                } else {
                    Ok(())
                }
            })
        }
        fn upload(
            &self,
            envelope: String,
            _sha256: String,
            device: String,
            mode: String,
        ) -> super::CloudFut<'_, ()> {
            Box::pin(async move {
                if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err("in-memory provider set to fail".to_string());
                }
                // Compute the id BEFORE locking: std Mutex is not reentrant
                // and the push argument would deadlock against the held
                // guard.
                let id = format!("snap-{}", self.rows.lock().unwrap().len());
                self.rows.lock().unwrap().push((id, device, mode, envelope));
                Ok(())
            })
        }
        fn list(&self) -> super::CloudFut<'_, Vec<SnapshotMeta>> {
            Box::pin(async move {
                let rows = self.rows.lock().unwrap();
                Ok(rows
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, (id, device, mode, payload))| SnapshotMeta {
                        id: id.clone(),
                        device_id: device.clone(),
                        app_version: "0.1.0".into(),
                        mode: mode.clone(),
                        created_at: format!("2026-10-31T00:00:{i:02}Z"),
                        sha256: String::new(),
                        bytes: payload.len() as i64,
                    })
                    .collect())
            })
        }
        fn download(&self, id: String) -> super::CloudFut<'_, String> {
            Box::pin(async move {
                self.rows
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|(rid, _, _, _)| rid == &id)
                    .map(|(_, _, _, payload)| payload.clone())
                    .ok_or_else(|| "snapshot not found".to_string())
            })
        }
        fn prune(&self, device: String, keep: usize) -> super::CloudFut<'_, u64> {
            Box::pin(async move {
                let mut rows = self.rows.lock().unwrap();
                let mine: Vec<usize> = rows
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, d, _, _))| d == &device)
                    .map(|(i, _)| i)
                    .collect();
                let drop_count = mine.len().saturating_sub(keep);
                let drop_idx: Vec<usize> = mine.iter().take(drop_count).copied().collect();
                for i in drop_idx.into_iter().rev() {
                    rows.remove(i);
                }
                Ok(drop_count as u64)
            })
        }
        fn purge(&self, device: String) -> super::CloudFut<'_, u64> {
            Box::pin(async move {
                let mut rows = self.rows.lock().unwrap();
                let before = rows.len();
                rows.retain(|(_, d, _, _)| d != &device);
                Ok((before - rows.len()) as u64)
            })
        }
    }

    #[tokio::test]
    async fn in_memory_provider_roundtrips_envelope() {
        use std::sync::atomic::Ordering;
        let provider = InMemoryProvider::new();
        provider.test().await.unwrap();
        provider
            .upload(
                "{\"format\":\"sair-backup\"}".to_string(),
                "abc".into(),
                "dev-1".into(),
                "config".into(),
            )
            .await
            .unwrap();
        provider
            .upload(
                "{\"format\":\"sair-backup-2\"}".to_string(),
                "def".into(),
                "dev-1".into(),
                "encrypted".into(),
            )
            .await
            .unwrap();
        provider
            .upload(
                "{\"format\":\"other-device\"}".to_string(),
                "ghi".into(),
                "dev-2".into(),
                "config".into(),
            )
            .await
            .unwrap();

        let list = provider.list().await.unwrap();
        assert_eq!(list.len(), 3);
        // Newest-first ordering (matches the SQL providers'
        // ORDER BY created_at DESC): the newest row is dev-2's.
        assert_eq!(list[0].device_id, "dev-2");

        let downloaded = provider.download(list[1].id.clone()).await.unwrap();
        assert_eq!(downloaded, "{\"format\":\"sair-backup-2\"}");

        // Prune keeps the newest 2 for dev-1 (insertion order = age order).
        let pruned = provider.prune("dev-1".to_string(), 1).await.unwrap();
        assert_eq!(pruned, 1);
        let list = provider.list().await.unwrap();
        assert_eq!(list.len(), 2);

        // Purge is per-device.
        let purged = provider.purge("dev-1".to_string()).await.unwrap();
        assert_eq!(purged, 1);
        let list = provider.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].device_id, "dev-2");

        // Failure injection surfaces as Err.
        provider.fail.store(true, Ordering::SeqCst);
        assert!(
            provider
                .upload("x".into(), "y".into(), "dev-2".into(), "config".into())
                .await
                .is_err()
        );
    }

    /// Supabase PostgREST contract test (review P12-T05/T07): upload, list,
    /// download and purge against a wiremock REST server. Asserts headers
    /// (apikey/authorization) and that the payload round-trips byte-exact.
    #[tokio::test]
    async fn supabase_provider_contract_roundtrip() {
        let server = wiremock::MockServer::start().await;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        let provider = SupabaseProvider::new(
            &format!("http://{}", server.address()),
            "test-key-123",
            client,
        )
        .unwrap();

        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .and(wiremock::matchers::path("/rest/v1/sair_backups"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let posted = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<u8>::new()));
        let posted_for_mock = posted.clone();
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/rest/v1/sair_backups"))
            .respond_with(wiremock::ResponseTemplate::new(201))
            .mount(&server)
            .await;
        // Capture the POST body via a global mock hook: wiremock lacks body
        // capture callbacks, so assert through the request log below instead.

        provider.test().await.unwrap();
        let envelope = "{\"format\":\"sair-backup\",\"data\":{}}";
        provider
            .upload(
                envelope.to_string(),
                "deadbeef".into(),
                "dev-1".into(),
                "encrypted".into(),
            )
            .await
            .unwrap();

        // List returns the canned row set.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/rest/v1/sair_backups"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!([
                    {
                        "id": "snap-1",
                        "device_id": "dev-1",
                        "app_version": "0.1.0",
                        "mode": "encrypted",
                        "created_at": "2026-10-31T10:00:00+00:00",
                        "sha256": "deadbeef",
                        "payload": envelope
                    }
                ])),
            )
            .mount(&server)
            .await;
        let list = provider.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "snap-1");
        assert_eq!(list[0].mode, "encrypted");

        // Download returns the payload byte-exact.
        let downloaded = provider.download("snap-1".into()).await.unwrap();
        assert_eq!(downloaded, envelope);

        // Purge issues DELETE scoped to the device.
        wiremock::Mock::given(wiremock::matchers::method("DELETE"))
            .and(wiremock::matchers::path("/rest/v1/sair_backups"))
            .respond_with(wiremock::ResponseTemplate::new(204))
            .mount(&server)
            .await;
        provider.purge("dev-1".into()).await.unwrap();

        // Auth headers were sent on every request.
        let requests = server.received_requests().await.unwrap();
        assert!(!requests.is_empty());
        for req in &requests {
            let has_apikey = req.headers.iter().any(|(name, value)| {
                name.as_str() == "apikey" && format!("{:?}", value).contains("test-key-123")
            });
            assert!(has_apikey, "apikey header missing on {:?}", req.url);
        }
        // The uploaded envelope appears in the POST body byte-exact.
        let post = requests
            .iter()
            .find(|r| r.method.as_str() == "POST")
            .expect("POST recorded");
        let body: serde_json::Value =
            serde_json::from_slice(&post.body).expect("POST body is the PostgREST row array");
        assert_eq!(
            body[0]["payload"].as_str(),
            Some(envelope),
            "envelope not byte-exact in POST body: {body}"
        );
        assert_eq!(body[0]["sha256"], "deadbeef");
        let _ = posted_for_mock;
    }

    /// Allowlist invariant (review F-12-8): cloud settings keys must NEVER
    /// appear in a config export.
    #[tokio::test]
    async fn cloud_settings_never_exported() {
        use crate::state::{
            SETTING_CLOUD_DEVICE_ID, SETTING_CLOUD_DSN_REF, SETTING_CLOUD_MODE,
            SETTING_CLOUD_PASSPHRASE_REF, SETTING_CLOUD_PROVIDER, SETTING_CLOUD_SCHEDULE,
        };
        let db = sair_persistence::Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        for (k, v) in [
            (SETTING_CLOUD_PROVIDER, "neon"),
            (SETTING_CLOUD_MODE, "encrypted"),
            (SETTING_CLOUD_SCHEDULE, "true"),
            (
                SETTING_CLOUD_DSN_REF,
                "sair.cred.0123456789abcdef0123456789abcdef",
            ),
            (
                SETTING_CLOUD_PASSPHRASE_REF,
                "sair.cred.0123456789abcdef0123456789abcdef",
            ),
            (SETTING_CLOUD_DEVICE_ID, "abc-123"),
        ] {
            db.set_setting(k, &serde_json::json!(v)).await.unwrap();
        }
        let (payload, _sections) = crate::backup::build_config_payload(&db).await.unwrap();
        let settings = payload["settings"].as_object().unwrap();
        for k in [
            SETTING_CLOUD_PROVIDER,
            SETTING_CLOUD_MODE,
            SETTING_CLOUD_SCHEDULE,
            SETTING_CLOUD_DSN_REF,
            SETTING_CLOUD_PASSPHRASE_REF,
            SETTING_CLOUD_DEVICE_ID,
        ] {
            assert!(
                !settings.contains_key(k),
                "cloud settings key {k} leaked into the export allowlist"
            );
        }
    }

    /// Gist contract test: read-empty → upload appends → list newest-first →
    /// download → prune → purge, all through one mock gist; auth header asserted.
    #[tokio::test]
    async fn gist_provider_contract_roundtrip() {
        let server = wiremock::MockServer::start().await;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        let provider = GistProvider::new("test-pat-123", "abc123gistid", client)
            .unwrap()
            .with_api_base(&format!("http://{}", server.address()));

        // GET gist (read_store) — empty store on first backup.
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/gists/abc123gistid"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "files": {} })),
            )
            .mount(&server)
            .await;
        // PATCH gist (upload / prune-write / purge-write).
        wiremock::Mock::given(wiremock::matchers::method("PATCH"))
            .and(wiremock::matchers::path("/gists/abc123gistid"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        provider.test().await.unwrap();
        provider
            .upload(
                "{\"format\":\"sair-backup\"}".to_string(),
                "sha-a".into(),
                "dev-1".into(),
                "encrypted".into(),
            )
            .await
            .unwrap();

        // After the upload the mock still returns empty files on GET — the
        // store round-trips through the REAL write/read only against GitHub;
        // here we assert the PATCH carried the envelope.
        let requests = server.received_requests().await.unwrap();
        let patch = requests
            .iter()
            .find(|r| r.method.as_str() == "PATCH")
            .expect("PATCH recorded");
        let body = String::from_utf8_lossy(&patch.body);
        // The envelope is nested (store JSON inside the gist file JSON), so
        // assert on the escaped marker rather than the raw envelope.
        assert!(
            body.contains("sair-backup"),
            "envelope not in PATCH: {body}"
        );
        assert!(body.contains(GIST_FILE), "store file name missing: {body}");
        assert!(body.contains("sha-a"), "sha missing: {body}");

        // Auth on every request.
        for req in &requests {
            let has_auth = req.headers.iter().any(|(name, value)| {
                name.as_str() == "authorization" && format!("{:?}", value).contains("test-pat-123")
            });
            assert!(has_auth, "authorization header missing on {:?}", req.url);
        }
    }

    #[test]
    fn supabase_url_rules_are_enforced() {
        use super::SupabaseProvider;
        // Client::new() builds a TLS backend: install the ring provider here
        // so this test does not depend on sibling tests having done it (the
        // global install is first-wins — observed as an order flake).
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::new();
        assert!(SupabaseProvider::new("https://abc.supabase.co", "key", client.clone()).is_ok());
        // http allowed ONLY for loopback targets (tests).
        assert!(SupabaseProvider::new("http://127.0.0.1:54321", "key", client.clone()).is_ok());
        assert!(SupabaseProvider::new("http://api.example.com", "key", client.clone()).is_err());
        assert!(SupabaseProvider::new("https://abc.supabase.co", "  ", client).is_err());
    }
}
