//! SQLite persistence (ADR-004).
//!
//! Rules enforced by design here:
//! - Connections use WAL and `foreign_keys = ON`.
//! - No method holds a transaction across an upstream network await.
//! - Migrations are embedded and must run at startup before the gateway
//!   accepts traffic; a migration failure aborts startup (docs/02 failure
//!   isolation).
//! - `request_records` are metadata-only; no bodies are ever stored
//!   (docs/07, ADR-008).

pub mod time;

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use thiserror::Error;

/// Embedded migrations (`crates/sair-persistence/migrations`).
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database open or connect failed: {0}")]
    Connect(#[from] sqlx::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("stored setting is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Handle over the application SQLite pool.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    /// Opens (creating if needed) the database at `path` with the mandated
    /// connection settings.
    ///
    /// # Errors
    /// Returns [`DbError::Connect`] when the database cannot be opened.
    pub async fn open(path: &str) -> Result<Self, DbError> {
        let options = SqliteConnectOptions::from_str(path)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    /// Applies embedded migrations. Safe to call on every startup; applying
    /// an already-applied set is a no-op.
    ///
    /// # Errors
    /// Returns [`DbError::Migration`] when any migration fails; startup must
    /// stop before the gateway binds (docs/02).
    pub async fn run_migrations(&self) -> Result<(), DbError> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    /// Cheap liveness check used at startup and by diagnostics.
    ///
    /// # Errors
    /// Returns [`DbError::Connect`] when the probe query fails.
    pub async fn ping(&self) -> Result<(), DbError> {
        sqlx::query_scalar::<_, i64>("SELECT 1")
            .fetch_one(&self.pool)
            .await?;
        Ok(())
    }

    /// Deletes metadata-only request records older than the cutoff
    /// (FR-020). Returns the number of rows removed.
    ///
    /// # Errors
    /// Returns [`DbError::Connect`] when the delete fails.
    pub async fn purge_request_records_before(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, DbError> {
        let result = sqlx::query("DELETE FROM request_records WHERE ts < ?1")
            .bind(cutoff.to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Reads a JSON app setting by key, deserialized to `serde_json::Value`.
    ///
    /// # Errors
    /// Returns [`DbError::Connect`] when the query fails, or
    /// [`DbError::Json`] when the stored value is not valid JSON.
    pub async fn get_setting(&self, key: &str) -> Result<Option<serde_json::Value>, DbError> {
        let raw: Option<String> =
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        match raw {
            Some(s) => serde_json::from_str(&s).map(Some).map_err(DbError::Json),
            None => Ok(None),
        }
    }

    /// Upserts a JSON app setting.
    ///
    /// # Errors
    /// Returns [`DbError::Connect`] when the write fails.
    pub async fn set_setting(&self, key: &str, value: &serde_json::Value) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value.to_string())
        .bind(time::now_iso())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::{Db, MIGRATOR};

    async fn migrated_db() -> Db {
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        db
    }

    #[tokio::test]
    async fn opens_memory_db_and_pings() {
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.ping().await.unwrap();
    }

    #[tokio::test]
    async fn foreign_keys_are_enabled() {
        let db = Db::open("sqlite::memory:").await.unwrap();
        let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(enabled, 1);
    }

    #[tokio::test]
    async fn wal_mode_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let db = Db::open(path.to_str().unwrap()).await.unwrap();
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[tokio::test]
    async fn migrations_apply_fresh_and_are_idempotent() {
        let db = migrated_db().await;
        // Second application is a no-op.
        db.run_migrations().await.unwrap();
        let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(applied, MIGRATOR.migrations.len() as i64);
        let tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'
             AND name IN ('providers','provider_accounts','credentials','models',
                          'deployments','routing_policies','model_aliases','alias_deployments',
                          'agent_profiles','request_records','health_events','app_settings')",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(tables, 12);
    }

    /// Inserts the full fixture chain: provider → account → credential →
    /// model → deployment, policy → alias → alias_deployment, plus agent
    /// profile, one request record and one health event.
    async fn seed_fixture(db: &Db) {
        sqlx::query(
            "INSERT INTO providers (id, name, provider_type, base_url, enabled, created_at, updated_at)
             VALUES ('p1', 'Mock', 'openai_compatible', 'https://mock.local', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO provider_accounts (id, provider_id, label, group_key, created_at, updated_at)
             VALUES ('a1', 'p1', 'main', 'org-1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO credentials (id, provider_id, account_id, label, secret_ref, status, created_at, updated_at)
             VALUES ('c1', 'p1', 'a1', 'key-1', 'sair.cred.1111', 'active', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO models (id, provider_id, model_id, protocol_family, capabilities, created_at)
             VALUES ('m1', 'p1', 'mock-large', 'openai_chat', '{}', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO deployments (id, provider_id, model_id, enabled, created_at, updated_at)
             VALUES ('d1', 'p1', 'm1', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO routing_policies (id, name, strategy, created_at, updated_at)
             VALUES ('r1', 'default', 'priority', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO model_aliases (id, alias, policy_id, created_at, updated_at)
             VALUES ('x1', 'fast-model', 'r1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query("INSERT INTO alias_deployments (id, alias_id, deployment_id) VALUES ('ad1', 'x1', 'd1')")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO agent_profiles (id, agent_id, protocol_family, endpoint, auth_method, model_alias, created_at, updated_at)
             VALUES ('g1', 'codex-cli', 'openai_chat', 'http://127.0.0.1:8787/v1', 'bearer', 'x1', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO request_records (id, ts, correlation_id, alias, deployment_id, credential_id, status)
             VALUES ('q1', '2026-01-01T00:00:00Z', 'corr-1', 'fast-model', 'd1', 'c1', 200)",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO health_events (id, ts, subject_type, subject_id, credential_id, event)
             VALUES ('h1', '2026-01-01T00:00:00Z', 'credential', 'c1', 'c1', 'ok')",
        )
        .execute(db.pool())
        .await
        .unwrap();
    }

    async fn count(db: &Db, sql: &'static str) -> i64 {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(db.pool())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn fk_delete_cascade_provider_takes_credentials_accounts_models_deployments() {
        let db = migrated_db().await;
        seed_fixture(&db).await;
        sqlx::query("DELETE FROM providers WHERE id = 'p1'")
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(count(&db, "SELECT COUNT(*) FROM credentials").await, 0);
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM provider_accounts").await,
            0
        );
        assert_eq!(count(&db, "SELECT COUNT(*) FROM models").await, 0);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM deployments").await, 0);
    }

    #[tokio::test]
    async fn fk_delete_account_nulls_credential_account_link() {
        let db = migrated_db().await;
        seed_fixture(&db).await;
        sqlx::query("DELETE FROM provider_accounts WHERE id = 'a1'")
            .execute(db.pool())
            .await
            .unwrap();
        let account: Option<String> =
            sqlx::query_scalar("SELECT account_id FROM credentials WHERE id = 'c1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(account, None);
    }

    #[tokio::test]
    async fn fk_delete_deployment_cascades_link_and_nulls_history() {
        let db = migrated_db().await;
        seed_fixture(&db).await;
        sqlx::query("DELETE FROM deployments WHERE id = 'd1'")
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM alias_deployments").await,
            0
        );
        let dep: Option<String> =
            sqlx::query_scalar("SELECT deployment_id FROM request_records WHERE id = 'q1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(dep, None);
    }

    #[tokio::test]
    async fn fk_delete_alias_cascades_links_and_nulls_agent_profile() {
        let db = migrated_db().await;
        seed_fixture(&db).await;
        sqlx::query("DELETE FROM model_aliases WHERE id = 'x1'")
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM alias_deployments").await,
            0
        );
        let alias: Option<String> =
            sqlx::query_scalar("SELECT model_alias FROM agent_profiles WHERE id = 'g1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(alias, None);
    }

    #[tokio::test]
    async fn fk_delete_credential_nulls_history_and_health() {
        let db = migrated_db().await;
        seed_fixture(&db).await;
        sqlx::query("DELETE FROM credentials WHERE id = 'c1'")
            .execute(db.pool())
            .await
            .unwrap();
        let cred: Option<String> =
            sqlx::query_scalar("SELECT credential_id FROM request_records WHERE id = 'q1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(cred, None);
        let health_cred: Option<String> =
            sqlx::query_scalar("SELECT credential_id FROM health_events WHERE id = 'h1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(health_cred, None);
    }

    #[tokio::test]
    async fn fk_delete_policy_nulls_alias_policy_link() {
        let db = migrated_db().await;
        seed_fixture(&db).await;
        sqlx::query("DELETE FROM routing_policies WHERE id = 'r1'")
            .execute(db.pool())
            .await
            .unwrap();
        let policy: Option<String> =
            sqlx::query_scalar("SELECT policy_id FROM model_aliases WHERE id = 'x1'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(policy, None);
    }

    #[tokio::test]
    async fn retention_purge_removes_only_old_records() {
        let db = migrated_db().await;
        sqlx::query(
            "INSERT INTO request_records (id, ts, correlation_id, status) VALUES
             ('old', '2025-01-01T00:00:00Z', 'corr-old', 200),
             ('new', '2026-10-31T00:00:00Z', 'corr-new', 200)",
        )
        .execute(db.pool())
        .await
        .unwrap();
        let cutoff = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let removed = db.purge_request_records_before(cutoff).await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM request_records").await, 1);
    }

    #[tokio::test]
    async fn settings_roundtrip_json() {
        let db = migrated_db().await;
        assert!(db.get_setting("gateway").await.unwrap().is_none());
        db.set_setting("gateway", &serde_json::json!({"port": 8787}))
            .await
            .unwrap();
        assert_eq!(
            db.get_setting("gateway").await.unwrap(),
            Some(serde_json::json!({"port": 8787}))
        );
        db.set_setting("gateway", &serde_json::json!({"port": 8788}))
            .await
            .unwrap();
        assert_eq!(
            db.get_setting("gateway").await.unwrap(),
            Some(serde_json::json!({"port": 8788}))
        );
    }
}
