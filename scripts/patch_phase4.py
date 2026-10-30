import io

# ---- openai.rs: Retry-After from HTTP header + cap (R4-04) ----
p = 'crates/sair-providers/src/openai.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''    let response = request
        .send()
        .await
        .map_err(|e| classify_transport(&e))?;

    let status = response.status().as_u16();''', '''    let response = request
        .send()
        .await
        .map_err(|e| classify_transport(&e))?;

    let status = response.status().as_u16();
    // R4-04: standard HTTP Retry-After header (seconds form only; HTTP-date
    // values are ignored), capped at 3600 s so a hostile provider cannot
    // freeze credentials long-term.
    let header_retry_after: Option<u64> = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| s.min(3600));''')
src = src.replace('''    } else {
        Err(classify(status, &parsed))
    }
}''', '''    } else {
        let mut failure = classify(status, &parsed);
        failure.retry_after_secs = header_retry_after.or(failure.retry_after_secs);
        failure.retry_after_secs = failure.retry_after_secs.map(|s| s.min(3600));
        Err(failure)
    }
}''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('openai.rs patched')

# ---- routing.rs tests: account_id field in constructors ----
p = 'crates/sair-gateway/src/routing.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''            credential_id: cred.to_string(),
            credential_secret_ref: format!("sair.cred.{cred}"),''', '''            credential_id: cred.to_string(),
            credential_secret_ref: format!("sair.cred.{cred}"),
            credential_account_id: None,''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('routing.rs patched')

# ---- api.rs: health mapping, attempted set, semaphores, group cooldown ----
p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

src = src.replace('''/// Maximum attempts across candidates (from the policy row at runtime).
pub struct ApiDeps {''', '''/// Maps a classification to a schema-legal `health_events.event` value
/// (R4-01: the CHECK constraint allows only the listed events).
#[must_use]
pub fn health_event_for_category(category: &ErrorCategory) -> &'static str {
    match category {
        ErrorCategory::RateLimited | ErrorCategory::BillingOrQuota => "cooldown_start",
        ErrorCategory::InvalidCredential => "invalid",
        ErrorCategory::PermissionDenied => "quarantine",
        _ => "degraded",
    }
}

/// Maximum attempts across candidates (from the policy row at runtime).
pub struct ApiDeps {''')

src = src.replace('''    /// Loopback-opt-in client, used only for providers that explicitly
    /// allow it (`providers.allow_loopback`).
    pub client_loopback: SsrfCheckedClient,
}''', '''    /// Loopback-opt-in client, used only for providers that explicitly
    /// allow it (`providers.allow_loopback`).
    pub client_loopback: SsrfCheckedClient,
    /// FR-021: per-deployment concurrency semaphores (R4-06: setting applies
    /// at gateway (re)start).
    pub deployment_concurrency: usize,
    semaphores: std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>,
}''')

src = src.replace('''    /// Builds the two policy-fixed clients (strict + loopback opt-in).
    ///
    /// # Errors
    /// Never in practice; clients are built from static policy.
    pub fn new(db: Db, vault: Arc<dyn SecretStore>) -> Self {
        Self {
            db,
            vault,
            rotation: Arc::new(RotationState::new()),
            client_strict: build_client(SsrfPolicy::default()),
            client_loopback: build_client(SsrfPolicy::allow_loopback()),
        }
    }''', '''    /// Builds the two policy-fixed clients (strict + loopback opt-in).
    #[must_use]
    pub fn new(db: Db, vault: Arc<dyn SecretStore>, deployment_concurrency: usize) -> Self {
        Self {
            db,
            vault,
            rotation: Arc::new(RotationState::new()),
            client_strict: build_client(SsrfPolicy::default()),
            client_loopback: build_client(SsrfPolicy::allow_loopback()),
            deployment_concurrency: deployment_concurrency.max(1),
            semaphores: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Returns (creating once) the per-deployment semaphore (FR-021). The
    /// critical section never spans an await (ADR-012).
    fn semaphore_for(&self, deployment_id: &str) -> Arc<tokio::sync::Semaphore> {
        let mut map = self
            .semaphores
            .lock()
            .expect("deployment semaphores poisoned; unrecoverable in-process state");
        map.entry(deployment_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(self.deployment_concurrency)))
            .clone()
    }''')

src = src.replace('''    let decision_json = serde_json::to_string(&decision).ok();
    let mut last_failure: Option<UpstreamFailure> = None;
''', '''    let decision_json = serde_json::to_string(&decision).ok();
    let mut last_failure: Option<UpstreamFailure> = None;
    // R4-02: never attempt the same credential twice within one request —
    // a just-cooled credential must not be re-tried via a second deployment.
    let mut attempted_credentials: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut skipped_duplicates = 0usize;
    let mut busy_skips = 0usize;
''')

src = src.replace('''    for (attempt_index, candidate) in decision.ordered.iter().enumerate() {
        if attempt_index >= policy.max_attempts.max(1) as usize {
            break;
        }
        if started.elapsed() > Duration::from_millis(policy.retry_budget_ms.max(1) as u64) {
            tracing::warn!(alias, "retry budget exhausted before rotation");
            break;
        }
''', '''    for candidate in &decision.ordered {
        if attempted_credentials.len() >= policy.max_attempts.max(1) as usize {
            break;
        }
        if started.elapsed() > Duration::from_millis(policy.retry_budget_ms.max(1) as u64) {
            tracing::warn!(alias, "retry budget exhausted before rotation");
            break;
        }
        // R4-02: dedupe credentials across deployments of the same provider.
        if !attempted_credentials.insert(candidate.credential_id.clone()) {
            skipped_duplicates += 1;
            continue;
        }
        // FR-021: per-deployment bound. A busy deployment rotates past
        // WITHOUT consuming an attempt slot (R4-06) — it is not an upstream
        // attempt.
        let deployment_permit = match api
            .semaphore_for(&candidate.deployment_id)
            .try_acquire_owned()
        {
            Ok(permit) => permit,
            Err(_) => {
                busy_skips += 1;
                continue;
            }
        };
''')

src = src.replace('''                let _ = api.db.repos().touch_credential_used(&candidate.credential_id).await;
                record(&api, &correlation, alias, candidate, Some(status), None, latency, tokens_in, tokens_out, &decision_json).await;
                return (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body))
                    .into_response();''', '''                let _ = api.db.repos().touch_credential_used(&candidate.credential_id).await;
                record(&api, &correlation, alias, candidate, Some(status), None, latency, tokens_in, tokens_out, &decision_json).await;
                drop(deployment_permit);
                return (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body))
                    .into_response();''')

src = src.replace('''                let _ = api
                    .db
                    .repos()
                    .insert_health_event(
                        "credential",
                        &candidate.credential_id,
                        Some(&candidate.credential_id),
                        failure.category_str(),
                        failure.code.as_deref(),
                        Some(&failure.detail),
                        Some(&correlation),
                    )
                    .await;
                if matches!(failure.category, ErrorCategory::RateLimited) {
                    let seconds = cooldown_for(&failure, policy.cooldown_seconds);
                    let until = (chrono::Utc::now() + chrono::Duration::seconds(seconds))
                        .to_rfc3339();
                    let _ = api
                        .db
                        .repos()
                        .set_credential_cooldown(&candidate.credential_id, Some(until))
                        .await;
                }''', '''                // R4-01: schema-legal event + no silent swallow.
                let event = health_event_for_category(&failure.category);
                if let Err(e) = api
                    .db
                    .repos()
                    .insert_health_event(
                        "credential",
                        &candidate.credential_id,
                        Some(&candidate.credential_id),
                        event,
                        Some(failure.category_str()),
                        Some(&failure.detail),
                        Some(&correlation),
                    )
                    .await
                {
                    tracing::warn!(error = %e, "failed to persist health event");
                }
                // ADR-017: rate limits cool the credential (Retry-After or
                // policy); billing/quota cools the ACCOUNT GROUP (docs/05);
                // other categories leave cooldown to the user.
                if matches!(
                    failure.category,
                    ErrorCategory::RateLimited | ErrorCategory::BillingOrQuota
                ) {
                    let seconds = cooldown_for(&failure, policy.cooldown_seconds);
                    // R4-08: shared timestamp helper; R4-04: capped seconds.
                    let until = (chrono::Utc::now()
                        + chrono::Duration::seconds(seconds.min(3600).max(0)))
                    .to_rfc3339();
                    match api
                        .db
                        .repos()
                        .apply_group_cooldown(
                            &candidate.credential_id,
                            candidate.credential_account_id.as_deref(),
                            &until,
                        )
                        .await
                    {
                        Ok(n) if n > 1 => {
                            tracing::warn!(alias, cooled = n, "account group cooled (quota truth)");
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!(error = %e, "cooldown write failed"),
                    }
                }''')

src = src.replace('''                drop(deployment_permit);
                return (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body))
                    .into_response();''', '''                drop(deployment_permit);
                return (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(body))
                    .into_response();''')

src = src.replace('''    // Rotation budget exhausted or rotation set exhausted.
    if let Some(failure) = last_failure {
        record_failure_minimal(&api, &correlation, alias, &failure, &decision_json).await;
        return upstream_failure_response(&failure, alias, decision.exclusions.len());
    }
    openai_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "no_attempt_made",
        "routing produced no executable candidate",
        json!({ "alias": alias, "exclusions": decision.exclusions.len() }),
    )
}''', '''    // Rotation budget exhausted or rotation set exhausted.
    if let Some(failure) = last_failure {
        record_failure_minimal(&api, &correlation, alias, &failure, &decision_json).await;
        return upstream_failure_response(&failure, alias, decision.exclusions.len());
    }
    // R4-06: distinguish terminal states.
    if busy_skips > 0 && attempted_credentials.is_empty() {
        return openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "deployment_busy",
            "every eligible deployment is at its concurrency bound",
            json!({ "alias": alias, "busy_deployments": busy_skips }),
        );
    }
    openai_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "no_attempt_made",
        "routing produced no executable candidate",
        json!({
            "alias": alias,
            "exclusions": decision.exclusions.len(),
            "skipped_duplicates": skipped_duplicates,
            "busy_deployments": busy_skips,
        }),
    )
}''')

io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('api.rs patched')
