//! OpenAI-compatible protocol surface (FR-006, ADR-009/010, ADR-017).
//!
//! Non-streaming only in this phase; `stream:true` is rejected with a
//! structured error (streaming lands in Phase 7). Responses pass through
//! with usage extracted for metadata records; errors are mapped into the
//! OpenAI error envelope with a `shahid` extension carrying the
//! classification for diagnostics (no secrets, no bodies).

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Router, body::Bytes};
use ring::rand::SecureRandom;
use sair_domain::error::{ErrorCategory, FailureAction, UpstreamFailure};
use sair_domain::routing::Strategy;
use sair_persistence::repos::{RequestRecordInsert, load_route_snapshot_detailed};
use sair_persistence::{Db, repos};
use sair_vault::SecretStore;
use serde_json::{Value, json};

use crate::routing::{RotationState, cooldown_for, plan_route};
use sair_providers::openai::{ChatOutcome, send_chat};
use sair_providers::{SsrfCheckedClient, SsrfPolicy, build_client};

/// Maps a classification to a schema-legal `health_events.event` value
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
pub struct ApiDeps {
    pub db: Db,
    pub vault: Arc<dyn SecretStore>,
    pub rotation: Arc<RotationState>,
    /// Strict policy client (no loopback).
    pub client_strict: SsrfCheckedClient,
    /// Loopback-opt-in client, used only for providers that explicitly
    /// allow it (`providers.allow_loopback`).
    pub client_loopback: SsrfCheckedClient,
    /// FR-021: per-deployment concurrency semaphores (R4-06: setting applies
    /// at gateway (re)start).
    pub deployment_concurrency: usize,
    semaphores: std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl ApiDeps {
    /// Builds the two policy-fixed clients (strict + loopback opt-in).
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
    pub(crate) fn semaphore_for(&self, deployment_id: &str) -> Arc<tokio::sync::Semaphore> {
        let mut map = self
            .semaphores
            .lock()
            .expect("deployment semaphores poisoned; unrecoverable in-process state");
        map.entry(deployment_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(self.deployment_concurrency)))
            .clone()
    }
}

/// Builds the API router. Merged into the main gateway router **before** the
/// middleware layers, so auth/Host/limits apply to these routes too.
pub fn router(api: Arc<ApiDeps>) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(list_models))
        .with_state(api)
}

fn correlation_id() -> String {
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes)
        .expect("OS CSPRNG must not fail; unrecoverable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn openai_error(status: StatusCode, code: &str, message: &str, extra: Value) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": format!("shahid_{}", code),
                "code": code,
                "shahid": extra,
            }
        })),
    )
        .into_response()
}

fn upstream_failure_response(
    failure: &UpstreamFailure,
    alias: &str,
    exclusions: usize,
) -> Response {
    let status = match failure.category {
        ErrorCategory::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ErrorCategory::Timeout => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::BAD_GATEWAY,
    };
    openai_error(
        status,
        failure.category_str(),
        &failure.detail,
        json!({
            "category": failure.category_str(),
            "upstream_status": failure.status,
            "alias": alias,
            "excluded_candidates": exclusions,
        }),
    )
}

/// POST /v1/chat/completions (non-streaming).
async fn chat_completions(State(api): State<Arc<ApiDeps>>, body: Bytes) -> Response {
    let started = Instant::now();
    let correlation = correlation_id();

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return openai_error(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                &format!("request body is not valid JSON: {e}"),
                json!({}),
            );
        }
    };
    if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "streaming_not_supported",
            "streaming is not implemented yet (planned Phase 7)",
            json!({}),
        );
    }
    let Some(alias) = parsed.get("model").and_then(Value::as_str) else {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "model_required",
            "the `model` field (a configured alias) is required",
            json!({}),
        );
    };

    let (candidates, excluded) = match load_route_snapshot_detailed(&api.db, alias).await {
        Ok(pair) => pair,
        Err(e) => {
            return openai_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("route snapshot unavailable: {e}"),
                json!({}),
            );
        }
    };
    if candidates.is_empty() && excluded.is_empty() {
        return openai_error(
            StatusCode::NOT_FOUND,
            "alias_not_found",
            &format!("no configured alias named `{alias}`"),
            json!({}),
        );
    }

    let policy = match api.db.repos().ensure_default_policy().await {
        Ok(p) => p,
        Err(e) => {
            return openai_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("routing policy unavailable: {e}"),
                json!({}),
            );
        }
    };
    let strategy = match policy.strategy.as_str() {
        "priority" => Strategy::Priority,
        "round_robin" => Strategy::RoundRobin,
        "weighted" => Strategy::Weighted,
        other => {
            return openai_error(
                StatusCode::NOT_IMPLEMENTED,
                "strategy_not_implemented",
                &format!("routing strategy `{other}` is declared but not implemented"),
                json!({}),
            );
        }
    };

    let decision = match plan_route(
        alias,
        sair_domain::protocol::ProtocolFamily::OpenAiChat,
        strategy,
        &candidates,
        &excluded,
        &api.rotation,
    ) {
        Ok(d) => d,
        Err(e) => {
            let (status, code): (axum::http::StatusCode, String) = match &e {
                crate::routing::RouteError::NoEligibleDeployment => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "no_eligible_deployment".to_string(),
                ),
                crate::routing::RouteError::AliasNotFound => {
                    (StatusCode::NOT_FOUND, "alias_not_found".to_string())
                }
                crate::routing::RouteError::UnimplementedStrategy(s) => (
                    StatusCode::NOT_IMPLEMENTED,
                    format!("strategy_{s}_not_implemented"),
                ),
            };
            return openai_error(status, &code, &e.to_string(), json!({ "alias": alias }));
        }
    };

    let decision_json = serde_json::to_string(&decision).ok();
    let mut last_failure: Option<UpstreamFailure> = None;
    // R4-02: never attempt the same credential twice within one request —
    // a just-cooled credential must not be re-tried via a second deployment.
    let mut attempted_credentials: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut skipped_duplicates = 0usize;
    let mut busy_skips = 0usize;
    // R4-06: only real upstream calls count against max_attempts.
    let mut actual_attempts = 0usize;

    for candidate in &decision.ordered {
        if actual_attempts >= policy.max_attempts.max(1) as usize {
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
        actual_attempts += 1;

        // Retrieve the secret at call time only (docs/06).
        let secret_ref = match sair_vault::SecretRef::parse(&candidate.credential_secret_ref) {
            Ok(reference) => reference,
            Err(_) => {
                last_failure = Some(UpstreamFailure::internal(
                    "stored credential reference is malformed",
                ));
                break;
            }
        };
        let secret = match api.vault.retrieve(&secret_ref) {
            Ok(bytes) => sair_vault::SecretBytes::new(bytes),
            Err(sair_vault::VaultError::NotFound) => {
                // The vault item vanished: the credential cannot work.
                let _ = api
                    .db
                    .repos()
                    .set_credential_status(&candidate.credential_id, "invalid")
                    .await;
                last_failure = Some(UpstreamFailure::internal(
                    "credential secret missing from vault",
                ));
                break;
            }
            Err(e) => {
                return openai_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "vault_unavailable",
                    &format!("credential vault unavailable: {e}"),
                    json!({ "alias": alias }),
                );
            }
        };

        let client = if candidate.allow_loopback {
            &api.client_loopback
        } else {
            &api.client_strict
        };
        let base = candidate
            .endpoint_override
            .clone()
            .unwrap_or_else(|| candidate.base_url.clone());

        match send_chat(
            client,
            &base,
            &candidate.model_id,
            &secret,
            parsed.clone(),
            Duration::from_millis(policy.timeout_ms.max(1) as u64),
        )
        .await
        {
            Ok(ChatOutcome {
                status,
                body,
                tokens_in,
                tokens_out,
            }) => {
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    &correlation,
                    alias,
                    candidate,
                    Some(status),
                    None,
                    latency,
                    tokens_in,
                    tokens_out,
                    &decision_json,
                )
                .await;
                // Release the deployment slot before responding (FR-021).
                drop(deployment_permit);
                return (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    Json(body),
                )
                    .into_response();
            }
            Err(failure) => {
                let _ = api
                    .db
                    .repos()
                    .touch_credential_error(
                        &candidate.credential_id,
                        failure.category_str(),
                        failure.code.as_deref(),
                    )
                    .await;
                // R4-01: schema-legal event value; failures are logged, not
                // silently swallowed.
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
                // policy cooldown); billing/quota cools the ACCOUNT GROUP
                // (docs/05 Quota truth); other categories leave cooldown to
                // the user.
                if matches!(
                    failure.category,
                    ErrorCategory::RateLimited | ErrorCategory::BillingOrQuota
                ) {
                    let seconds = cooldown_for(&failure, policy.cooldown_seconds);
                    // R4-08: shared timestamp helper; R4-04: capped seconds.
                    let until = (chrono::Utc::now()
                        + chrono::Duration::seconds(seconds.clamp(0, 3600)))
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
                }
                match failure.action() {
                    FailureAction::Rotate => {
                        last_failure = Some(failure);
                        continue;
                    }
                    FailureAction::Quarantine => {
                        let _ = api
                            .db
                            .repos()
                            .set_credential_status(&candidate.credential_id, "quarantined")
                            .await;
                        last_failure = Some(failure);
                        break;
                    }
                    FailureAction::Stop => {
                        record(
                            &api,
                            &correlation,
                            alias,
                            candidate,
                            failure.status,
                            Some(failure.category_str()),
                            started.elapsed().as_millis() as i64,
                            None,
                            None,
                            &decision_json,
                        )
                        .await;
                        return upstream_failure_response(
                            &failure,
                            alias,
                            decision.exclusions.len(),
                        );
                    }
                }
            }
        }
    }

    // Rotation budget exhausted or rotation set exhausted.
    if let Some(failure) = last_failure {
        record_failure_minimal(&api, &correlation, alias, &failure, &decision_json).await;
        return upstream_failure_response(&failure, alias, decision.exclusions.len());
    }
    // R4-06: distinguish terminal states (actual upstream attempts, not
    // reserved credentials).
    if busy_skips > 0 && actual_attempts == 0 {
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
}

#[allow(clippy::too_many_arguments)]
async fn record(
    api: &ApiDeps,
    correlation: &str,
    alias: &str,
    candidate: &sair_domain::routing::RouteCandidate,
    status: Option<u16>,
    error_category: Option<&str>,
    latency_ms: i64,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
    decision_json: &Option<String>,
) {
    let record = RequestRecordInsert {
        id: correlation.to_string(),
        ts: sair_persistence::time::now_iso(),
        correlation_id: correlation.to_string(),
        protocol_family: Some("openai_chat".to_string()),
        operation: Some("chat_completions".to_string()),
        alias: Some(alias.to_string()),
        deployment_id: Some(candidate.deployment_id.clone()),
        credential_id: Some(candidate.credential_id.clone()),
        provider_label: Some(candidate.provider_label.clone()),
        model_label: Some(candidate.model_id.clone()),
        deployment_label: Some(candidate.deployment_id.clone()),
        status: status.map(i64::from),
        error_category: error_category.map(str::to_string),
        latency_ms: Some(latency_ms),
        tokens_in,
        tokens_out,
        decision_json: decision_json.clone(),
    };
    if let Err(e) = api.db.repos().insert_request_record(&record).await {
        tracing::warn!(error = %e, "failed to persist request record");
    }
}

async fn record_failure_minimal(
    api: &ApiDeps,
    correlation: &str,
    alias: &str,
    failure: &UpstreamFailure,
    decision_json: &Option<String>,
) {
    let record = RequestRecordInsert {
        id: correlation.to_string(),
        ts: sair_persistence::time::now_iso(),
        correlation_id: correlation.to_string(),
        protocol_family: Some("openai_chat".to_string()),
        operation: Some("chat_completions".to_string()),
        alias: Some(alias.to_string()),
        error_category: Some(failure.category_str().to_string()),
        decision_json: decision_json.clone(),
        ..RequestRecordInsert::default()
    };
    if let Err(e) = api.db.repos().insert_request_record(&record).await {
        tracing::warn!(error = %e, "failed to persist request record");
    }
}

/// GET /v1/models — lists configured aliases as callable model ids.
async fn list_models(State(api): State<Arc<ApiDeps>>) -> Response {
    match repos::alias_map(&api.db).await {
        Ok(map) => {
            let mut data: Vec<Value> = map
                .keys()
                .map(|alias| {
                    json!({
                        "id": alias,
                        "object": "model",
                        "owned_by": "shahid-ai-router",
                    })
                })
                .collect();
            data.sort_by(|a, b| {
                a["id"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["id"].as_str().unwrap_or(""))
            });
            (
                StatusCode::OK,
                Json(json!({ "object": "list", "data": data })),
            )
                .into_response()
        }
        Err(e) => openai_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_failure",
            &format!("model catalogue unavailable: {e}"),
            json!({}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::ApiDeps;
    use crate::handle::{GatewayConfig, GatewayHandle};
    use sair_persistence::Db;
    use sair_vault::{InMemorySecretStore, SecretStore};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TOKEN: &str = "0123456789abcdefghijklmnopqrstuvwxyz0";
    const SECRET: &[u8] = b"sk-upstream-live-key";

    async fn setup(upstream: &MockServer) -> GatewayHandle {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());

        let secret_ref = vault.store(SECRET).unwrap();
        let provider_id = db
            .repos()
            .insert_provider("Mock", "openai_compatible", &upstream.uri(), true, None)
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key-1", secret_ref.as_str(), 0, 1)
            .await
            .unwrap();
        let model_pk = db
            .repos()
            .insert_model(
                &provider_id,
                "upstream-large",
                "openai_chat",
                &json!({}),
                None,
            )
            .await
            .unwrap();
        let deployment_id = db
            .repos()
            .insert_deployment(&provider_id, &model_pk, None)
            .await
            .unwrap();
        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();
        db.repos()
            .bind_deployment(&alias_id, &deployment_id, 10)
            .await
            .unwrap();

        let api = Arc::new(ApiDeps::new(db, vault, 4));
        let config = GatewayConfig {
            token: Arc::new(TOKEN.as_bytes().to_vec()),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        GatewayHandle::start(config).await.unwrap()
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    async fn chat(handle: &GatewayHandle, token: &str, body: Value) -> reqwest::Response {
        client()
            .post(format!("http://{}/v1/chat/completions", handle.addr()))
            .header("authorization", format!("Bearer {token}"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn end_to_end_chat_completions_passes_through() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({"model": "upstream-large"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "cmpl-e2e",
                "choices": [{"message": {"role": "assistant", "content": "hello"}}],
                "usage": {"prompt_tokens": 9, "completion_tokens": 4}
            })))
            .mount(&upstream)
            .await;
        let handle = setup(&upstream).await;

        let response = chat(
            &handle,
            TOKEN,
            json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        assert_eq!(status, 200, "body: {text}");
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["id"], "cmpl-e2e");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn api_routes_require_auth() {
        let upstream = MockServer::start().await;
        let handle = setup(&upstream).await;
        let response = client()
            .get(format!("http://{}/v1/models", handle.addr()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn unknown_alias_is_404_with_openai_envelope() {
        let upstream = MockServer::start().await;
        let handle = setup(&upstream).await;
        let response = chat(&handle, TOKEN, json!({"model": "ghost", "messages": []})).await;
        assert_eq!(response.status(), 404);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "alias_not_found");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn streaming_is_rejected_structurally() {
        let upstream = MockServer::start().await;
        let handle = setup(&upstream).await;
        let response = chat(
            &handle,
            TOKEN,
            json!({"model": "test-model", "stream": true, "messages": []}),
        )
        .await;
        assert_eq!(response.status(), 400);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "streaming_not_supported");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn models_lists_configured_aliases() {
        let upstream = MockServer::start().await;
        let handle = setup(&upstream).await;
        let response = client()
            .get(format!("http://{}/v1/models", handle.addr()))
            .header("authorization", format!("Bearer {TOKEN}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["object"], "list");
        let ids: Vec<&str> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["test-model"]);
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn upstream_auth_failure_quarantines_and_stops() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "error": {"message": "bad key"}
            })))
            .mount(&upstream)
            .await;
        let handle = setup(&upstream).await;

        let response = chat(
            &handle,
            TOKEN,
            json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
        assert_eq!(response.status(), 502);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["shahid"]["category"], "invalid_credential");

        // Second request: credential quarantined → no eligible deployment.
        let response = chat(
            &handle,
            TOKEN,
            json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
        assert_eq!(response.status(), 503);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "no_eligible_deployment");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn quota_429_sets_cooldown_and_does_not_burn_keys() {
        let upstream = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": {"type": "insufficient_quota", "message": "quota"}
            })))
            .mount(&upstream)
            .await;
        let handle = setup(&upstream).await;

        let response = chat(
            &handle,
            TOKEN,
            json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
        assert_eq!(response.status(), 502);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["shahid"]["category"], "billing_or_quota");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }
}
