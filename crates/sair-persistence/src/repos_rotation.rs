//! Phase 4 repository functions: rotation & health completion (FR-002/008/012/021).
//! Split from `repos.rs` for reviewability; same `Repos` type.

use serde_json::Value;
use sqlx::Row;

use crate::DbError;
use crate::repos::Repos;

impl Repos<'_> {
    /// Monotonic cooldown write (R4-03): extends — never shortens — the
    /// standing cooldown. With an account group, every active credential of
    /// that group is cooled (docs/05 Quota truth); without, only the
    /// credential itself.
    ///
    /// # Errors
    /// DB failures.
    pub async fn apply_group_cooldown(
        &self,
        credential_id: &str,
        account_id: Option<&str>,
        until: &str,
    ) -> Result<u64, DbError> {
        let result = match account_id {
            Some(account) => {
                sqlx::query(
                    "UPDATE credentials SET cooldown_until = ?2, updated_at = ?3
                     WHERE (account_id = ?4 OR id = ?1)
                       AND status = 'active'
                       AND (cooldown_until IS NULL OR cooldown_until < ?2)",
                )
                .bind(credential_id)
                .bind(until)
                .bind(crate::time::now_iso())
                .bind(account)
                .execute(self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "UPDATE credentials SET cooldown_until = ?2, updated_at = ?3
                     WHERE id = ?1 AND status = 'active'
                       AND (cooldown_until IS NULL OR cooldown_until < ?2)",
                )
                .bind(credential_id)
                .bind(until)
                .bind(crate::time::now_iso())
                .execute(self.pool)
                .await?
            }
        };
        Ok(result.rows_affected())
    }

    /// Recent health events, newest first (FR-012). Limit clamped 1..=500
    /// (R4-07).
    ///
    /// # Errors
    /// DB failures.
    pub async fn list_health_events(&self, limit: i64) -> Result<Vec<serde_json::Value>, DbError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query(
            "SELECT ts, subject_type, subject_id, event, category, detail, correlation_id
             FROM health_events ORDER BY ts DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "ts": r.get::<String, _>("ts"),
                    "subject_type": r.get::<String, _>("subject_type"),
                    "subject_id": r.get::<String, _>("subject_id"),
                    "event": r.get::<String, _>("event"),
                    "category": r.get::<Option<String>, _>("category"),
                    "detail": r.get::<Option<String>, _>("detail"),
                    "correlation_id": r.get::<Option<String>, _>("correlation_id"),
                })
            })
            .collect())
    }

    /// Per-credential health summary for the UI (FR-012).
    ///
    /// # Errors
    /// DB failures.
    pub async fn credential_health_summary(&self) -> Result<Vec<serde_json::Value>, DbError> {
        let rows = sqlx::query(
            "SELECT c.id, p.name AS provider, c.label, c.status, c.priority, c.weight,
                    c.cooldown_until, c.last_used_at, c.last_error_category, c.last_error_code
             FROM credentials c JOIN providers p ON p.id = c.provider_id
             ORDER BY p.name, c.priority DESC",
        )
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "provider": r.get::<String, _>("provider"),
                    "label": r.get::<String, _>("label"),
                    "status": r.get::<String, _>("status"),
                    "priority": r.get::<i64, _>("priority"),
                    "weight": r.get::<i64, _>("weight"),
                    "cooldown_until": r.get::<Option<String>, _>("cooldown_until"),
                    "last_used_at": r.get::<Option<String>, _>("last_used_at"),
                    "last_error_category": r.get::<Option<String>, _>("last_error_category"),
                    "last_error_code": r.get::<Option<String>, _>("last_error_code"),
                })
            })
            .collect())
    }

    /// Lists account groups of a provider (docs/05 quota grouping).
    ///
    /// # Errors
    /// DB failures.
    pub async fn list_provider_accounts(
        &self,
        provider_id: &str,
    ) -> Result<Vec<serde_json::Value>, DbError> {
        let rows = sqlx::query(
            "SELECT id, label, group_key FROM provider_accounts WHERE provider_id = ?1 ORDER BY label",
        )
        .bind(provider_id)
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "label": r.get::<String, _>("label"),
                    "group_key": r.get::<Option<String>, _>("group_key"),
                })
            })
            .collect())
    }

    /// # Errors
    /// DB failures.
    pub async fn insert_provider_account(
        &self,
        provider_id: &str,
        label: &str,
        group_key: Option<&str>,
    ) -> Result<String, DbError> {
        let id = crate::repos::new_id();
        sqlx::query(
            "INSERT INTO provider_accounts (id, provider_id, label, group_key, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind(&id)
        .bind(provider_id)
        .bind(label)
        .bind(group_key)
        .bind(crate::time::now_iso())
        .execute(self.pool)
        .await?;
        Ok(id)
    }

    /// Assigns a credential to an account group of the SAME provider
    /// (R4-07: cross-provider assignment is rejected).
    ///
    /// # Errors
    /// Validation message for cross-provider attempts or unknown ids; DB
    /// failures.
    pub async fn assign_credential_account(
        &self,
        credential_id: &str,
        account_id: &str,
    ) -> Result<(), String> {
        let check = sqlx::query(
            "SELECT (SELECT provider_id FROM credentials WHERE id = ?1) AS cred_provider,
                    (SELECT provider_id FROM provider_accounts WHERE id = ?2) AS account_provider",
        )
        .bind(credential_id)
        .bind(account_id)
        .fetch_one(self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let cred_provider: Option<String> = check.get("cred_provider");
        let account_provider: Option<String> = check.get("account_provider");
        match (cred_provider, account_provider) {
            (Some(cp), Some(ap)) if cp == ap => {
                sqlx::query(
                    "UPDATE credentials SET account_id = ?2, updated_at = ?3 WHERE id = ?1",
                )
                .bind(credential_id)
                .bind(account_id)
                .bind(crate::time::now_iso())
                .execute(self.pool)
                .await
                .map_err(|e| e.to_string())?;
                Ok(())
            }
            _ => Err("credential and account group must belong to the same provider".to_string()),
        }
    }

    /// Updates ordering metadata for a credential (FR-002). Weight floor 1
    /// (R4-07); negative priority is legal (lower precedence).
    ///
    /// # Errors
    /// Validation message for weight < 1; DB failures.
    pub async fn set_credential_priority_weight(
        &self,
        id: &str,
        priority: i64,
        weight: i64,
    ) -> Result<(), String> {
        if weight < 1 {
            return Err("weight must be >= 1".to_string());
        }
        sqlx::query(
            "UPDATE credentials SET priority = ?2, weight = ?3, updated_at = ?4 WHERE id = ?1",
        )
        .bind(id)
        .bind(priority)
        .bind(weight)
        .bind(crate::time::now_iso())
        .execute(self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl Repos<'_> {
    /// Phase 5 (F-2): updates a model row's capability flags.
    ///
    /// # Errors
    /// DB failures.
    pub async fn update_model_capabilities(
        &self,
        model_pk: &str,
        capabilities: &serde_json::Value,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE models SET capabilities = ?2, verified_at = ?3 WHERE id = ?1")
            .bind(model_pk)
            .bind(capabilities.to_string())
            .bind(crate::time::now_iso())
            .execute(self.pool)
            .await?;
        Ok(())
    }
}

impl Repos<'_> {
    /// Phase 8 (F-5): validates that an alias exists and has enabled
    /// deployments of the given protocol family.
    ///
    /// # Errors
    /// DB failures.
    pub async fn alias_has_family_deployments(
        &self,
        alias: &str,
        protocol_family: &str,
    ) -> Result<bool, DbError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM alias_deployments ad
             JOIN deployments d ON d.id = ad.deployment_id
             JOIN models m ON m.id = d.model_id
             JOIN providers p ON p.id = d.provider_id
             WHERE ad.alias_id = (SELECT id FROM model_aliases WHERE alias = ?1)
               AND m.protocol_family = ?2 AND d.enabled = 1 AND p.enabled = 1",
        )
        .bind(alias)
        .bind(protocol_family)
        .fetch_one(self.pool)
        .await?;
        Ok(count > 0)
    }

    /// Phase 8: alias existence check.
    ///
    /// # Errors
    /// DB failures.
    pub async fn alias_exists(&self, alias: &str) -> Result<bool, DbError> {
        let row: Option<i64> = sqlx::query_scalar("SELECT 1 FROM model_aliases WHERE alias = ?1")
            .bind(alias)
            .fetch_optional(self.pool)
            .await?;
        Ok(row.is_some())
    }
}

impl Repos<'_> {
    /// Phase 9: recent request records projected for the UI — decision
    /// exclusions parsed server-side with credential refs replaced by LABELS
    /// (review F-2: secret_refs never leave the backend).
    ///
    /// # Errors
    /// DB failures.
    #[allow(clippy::too_many_lines)]
    pub async fn list_request_records(
        &self,
        limit: i64,
        alias_filter: Option<&str>,
        errors_only: bool,
    ) -> Result<Vec<serde_json::Value>, DbError> {
        let limit = limit.clamp(1, 500);
        let rows = sqlx::query(
            "SELECT r.ts, r.correlation_id, r.protocol_family, r.operation, r.alias,
                    r.provider_label, r.model_label, r.deployment_label,
                    r.status, r.error_category, r.latency_ms, r.tokens_in, r.tokens_out,
                    c.label AS credential_label,
                    r.decision_json
             FROM request_records r
             LEFT JOIN credentials c ON c.id = r.credential_id
             WHERE (?1 IS NULL OR r.alias = ?1)
               AND (?2 = 0 OR r.error_category IS NOT NULL)
             ORDER BY r.ts DESC LIMIT ?3",
        )
        .bind(alias_filter)
        .bind(i64::from(errors_only))
        .bind(limit)
        .fetch_all(self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                // F-2: project exclusions server-side; replace credential
                // ids with the joined label.
                let decision: Option<Value> = r
                    .try_get::<Option<String>, _>("decision_json")
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok());
                let credential_label: Option<String> = r.get("credential_label");
                let exclusions: Vec<Value> = decision
                    .as_ref()
                    .and_then(|d| d.get("decision"))
                    .and_then(|d| d.get("exclusions"))
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|e| {
                                serde_json::json!({
                                    "deployment": e.get("deployment_id").cloned().unwrap_or(Value::Null),
                                    "credential": credential_label.clone().unwrap_or_else(|| {
                                        e.get("credential_id").and_then(Value::as_str).unwrap_or("?").to_string()
                                    }),
                                    "reason": e.get("reason").cloned().unwrap_or(Value::Null),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                serde_json::json!({
                    "ts": r.get::<String, _>("ts"),
                    "correlation_id": r.get::<String, _>("correlation_id"),
                    "protocol_family": r.get::<Option<String>, _>("protocol_family"),
                    "operation": r.get::<Option<String>, _>("operation"),
                    "alias": r.get::<Option<String>, _>("alias"),
                    "provider": r.get::<Option<String>, _>("provider_label"),
                    "model": r.get::<Option<String>, _>("model_label"),
                    "deployment": r.get::<Option<String>, _>("deployment_label"),
                    "credential": credential_label,
                    "status": r.get::<Option<i64>, _>("status"),
                    "error_category": r.get::<Option<String>, _>("error_category"),
                    "latency_ms": r.get::<Option<i64>, _>("latency_ms"),
                    "tokens_in": r.get::<Option<i64>, _>("tokens_in"),
                    "tokens_out": r.get::<Option<i64>, _>("tokens_out"),
                    "streamed": decision
                        .as_ref()
                        .and_then(|d| d.get("streamed"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    "exclusions": exclusions,
                })
            })
            .collect())
    }

    /// Phase 9 (F-3): usage aggregates. Cutoffs computed in Rust chrono
    /// (RFC-3339 `+00:00` comparisons verified lexicographically correct).
    /// "error" = error_category IS NOT NULL.
    ///
    /// # Errors
    /// DB failures.
    pub async fn usage_summary(&self) -> Result<serde_json::Value, DbError> {
        let now = chrono::Utc::now();
        let cutoff_24h = (now - chrono::Duration::hours(24)).to_rfc3339();
        let cutoff_7d = (now - chrono::Duration::days(7)).to_rfc3339();
        let agg = |cutoff: &str| {
            let cutoff = cutoff.to_string();
            async move {
                let row = sqlx::query(
                    "SELECT COUNT(*) AS requests,
                            SUM(CASE WHEN error_category IS NOT NULL THEN 1 ELSE 0 END) AS errors,
                            AVG(latency_ms) AS avg_latency,
                            SUM(tokens_in) AS tokens_in,
                            SUM(tokens_out) AS tokens_out
                     FROM request_records WHERE ts >= ?1",
                )
                .bind(&cutoff)
                .fetch_one(self.pool)
                .await?;
                Ok::<serde_json::Value, DbError>(serde_json::json!({
                    "requests": row.get::<i64, _>("requests"),
                    "errors": row.get::<Option<i64>, _>("errors").unwrap_or(0),
                    "avg_latency_ms": row.get::<Option<f64>, _>("avg_latency"),
                    "tokens_in": row.get::<Option<i64>, _>("tokens_in"),
                    "tokens_out": row.get::<Option<i64>, _>("tokens_out"),
                }))
            }
        };
        let day = agg(&cutoff_24h).await?;
        let week = agg(&cutoff_7d).await?;
        Ok(serde_json::json!({
            "requests_24h": day["requests"],
            "errors_24h": day["errors"],
            "avg_latency_ms_24h": day["avg_latency"],
            "tokens_in_24h": day["tokens_in"],
            "tokens_out_24h": day["tokens_out"],
            "requests_7d": week["requests"],
            "errors_7d": week["errors"],
        }))
    }
}
