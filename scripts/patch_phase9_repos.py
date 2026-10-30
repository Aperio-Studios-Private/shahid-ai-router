import io

# ---- repos: usage summary + record view projection with label join ----
p = 'crates/sair-persistence/src/repos_rotation.rs'
src = io.open(p, encoding='utf-8').read()
src += '''
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
'''
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('repos extended')
