//! SQLite persistence (ADR-004).
//!
//! Rules enforced by design here:
//! - Connections use WAL and `foreign_keys = ON`.
//! - No method holds a transaction across an upstream network await.
//! - Migrations are embedded and run at startup before the gateway accepts
//!   traffic (Phase 2 adds the real schema and migration tests).

use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database open or connect failed: {0}")]
    Connect(#[from] sqlx::Error),
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

    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::Db;

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
}
