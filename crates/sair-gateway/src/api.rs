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
use sair_providers::gemini::generate_content_streaming;
use sair_providers::openai::{
    ChatOutcome, StreamStart, send_chat, send_chat_streaming_with_headers, send_chat_with_headers,
};
use sair_providers::{
    SsrfCheckedClient, SsrfPolicy, build_client, build_streaming_client_with_idle_timeout,
};

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
    /// Phase 7 (F-1): streaming clients carry NO total deadline — only
    /// connect + idle read bounds — so streams may outlive `timeout_ms`.
    pub client_strict_streaming: SsrfCheckedClient,
    pub client_loopback_streaming: SsrfCheckedClient,
    /// FR-021: per-deployment concurrency semaphores (R4-06: setting applies
    /// at gateway (re)start).
    pub deployment_concurrency: usize,
    /// Phase 5: OpenRouter attribution config (applies at gateway restart).
    pub openrouter_attribution: sair_providers::openrouter::AttributionConfig,
    semaphores: std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>,
}

impl ApiDeps {
    /// Builds the two policy-fixed clients (strict + loopback opt-in).
    #[must_use]
    pub fn new(
        db: Db,
        vault: Arc<dyn SecretStore>,
        deployment_concurrency: usize,
        openrouter_attribution: sair_providers::openrouter::AttributionConfig,
        upstream_idle_read_ms: u64,
    ) -> Self {
        let idle = std::time::Duration::from_millis(upstream_idle_read_ms.max(1));
        Self {
            db,
            vault,
            rotation: Arc::new(RotationState::new()),
            client_strict: build_client(SsrfPolicy::default()),
            client_loopback: build_client(SsrfPolicy::allow_loopback()),
            client_strict_streaming: build_streaming_client_with_idle_timeout(
                SsrfPolicy::default(),
                idle,
            ),
            client_loopback_streaming: build_streaming_client_with_idle_timeout(
                SsrfPolicy::allow_loopback(),
                idle,
            ),
            deployment_concurrency: deployment_concurrency.max(1),
            openrouter_attribution,
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
        .route("/v1/messages", post(anthropic_messages))
        .route("/v1/messages/count_tokens", post(anthropic_count_tokens))
        .route("/v1beta/models/{model_and_method}", post(gemini_generate))
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

/// Detects  in the raw body (fast pre-parse; the parsed body
/// remains the authority inside process_chat).
fn parsed_stream_flag(body: &Bytes) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("stream").and_then(Value::as_bool))
        .unwrap_or(false)
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

/// The protocol surface a request arrived on (Phase 6). Determines the
/// protocol family, wire envelope, and upstream adapter.
/// Phase 9 (F-1a): projects route exclusions into a decision-shaped JSON for
/// no-route records (no candidates were selected, so no secret refs exist).
/// `stream_requested` is recorded faithfully (audit P3 fix: it was
/// hardcoded to false even for streaming requests).
fn decision_json_placeholder(
    excluded: &[sair_persistence::repos::ExcludedCandidate],
    stream_requested: bool,
) -> Option<String> {
    let exclusions: Vec<Value> = excluded
        .iter()
        .map(|x| {
            serde_json::json!({
                "deployment_id": x.deployment_id,
                "credential_id": x.credential_id,
                "reason": x.reason,
            })
        })
        .collect();
    Some(
        serde_json::json!({
            "decision": { "exclusions": exclusions },
            "streamed": stream_requested,
        })
        .to_string(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    OpenAiChat,
    AnthropicMessages,
    GeminiGenerateContent,
}

impl Surface {
    #[must_use]
    pub const fn family(self) -> sair_domain::protocol::ProtocolFamily {
        match self {
            Surface::OpenAiChat => sair_domain::protocol::ProtocolFamily::OpenAiChat,
            Surface::AnthropicMessages => sair_domain::protocol::ProtocolFamily::AnthropicMessages,
            Surface::GeminiGenerateContent => {
                sair_domain::protocol::ProtocolFamily::GeminiGenerateContent
            }
        }
    }

    fn label(self) -> &'static str {
        self.family().as_str()
    }
}

/// F-3: every ErrorCategory maps to a wire-legal Anthropic error type.
#[must_use]
fn anthropic_error_type(category: ErrorCategory) -> &'static str {
    match category {
        ErrorCategory::InvalidCredential => "authentication_error",
        ErrorCategory::PermissionDenied => "permission_error",
        ErrorCategory::BillingOrQuota => "billing_error",
        ErrorCategory::RateLimited => "rate_limit_error",
        ErrorCategory::ModelUnavailable
        | ErrorCategory::InvalidRequest
        | ErrorCategory::ContextLimit => "invalid_request_error",
        ErrorCategory::Timeout => "timeout_error",
        ErrorCategory::ProviderUnavailable
        | ErrorCategory::TransportFailure
        | ErrorCategory::Unknown => "api_error",
    }
}

/// F-3: every ErrorCategory maps to a wire-legal Gemini status.
#[must_use]
fn gemini_error_status(category: ErrorCategory) -> &'static str {
    match category {
        ErrorCategory::InvalidCredential => "UNAUTHENTICATED",
        ErrorCategory::PermissionDenied => "PERMISSION_DENIED",
        ErrorCategory::BillingOrQuota | ErrorCategory::RateLimited => "RESOURCE_EXHAUSTED",
        ErrorCategory::ModelUnavailable => "NOT_FOUND",
        ErrorCategory::InvalidRequest | ErrorCategory::ContextLimit => "INVALID_ARGUMENT",
        ErrorCategory::Timeout => "DEADLINE_EXCEEDED",
        ErrorCategory::ProviderUnavailable | ErrorCategory::TransportFailure => "UNAVAILABLE",
        ErrorCategory::Unknown => "INTERNAL",
    }
}

/// Adds the correlation id to the family `shahid` block so clients can
/// correlate an error response with the request record (audit P3 fix: the id
/// was recorded but never surfaced). Empty id (pre-correlation local errors)
/// is omitted.
fn with_request_id(mut extra: Value, request_id: &str) -> Value {
    if !request_id.is_empty()
        && let Some(obj) = extra.as_object_mut()
    {
        obj.insert(
            "request_id".to_string(),
            Value::String(request_id.to_string()),
        );
    }
    extra
}

/// Family-faithful error envelope (F-1/F-3/F-4): upstream failures on
/// same-protocol surfaces preserve the upstream status; gateway-local errors
/// use family-legal names with our code in `shahid`.
fn family_error_response(
    surface: Surface,
    failure: &UpstreamFailure,
    alias: &str,
    exclusions: usize,
    request_id: &str,
) -> Response {
    // F-1: same-protocol upstream failures keep the upstream status — for
    // 4xx/5xx only. Gateway-detected errors on a 2xx upstream response
    // (malformed/oversized bodies) must NOT echo a success status.
    let status = failure
        .status
        .filter(|s| !(100..=399).contains(s))
        .and_then(|s| StatusCode::from_u16(s).ok())
        .unwrap_or(match failure.category {
            ErrorCategory::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCategory::Timeout => StatusCode::GATEWAY_TIMEOUT,
            _ => StatusCode::BAD_GATEWAY,
        });
    let extra = with_request_id(
        json!({
            "category": failure.category_str(),
            "upstream_status": failure.status,
            "alias": alias,
            "excluded_candidates": exclusions,
        }),
        request_id,
    );
    match surface {
        Surface::OpenAiChat => openai_error(status, failure.category_str(), &failure.detail, extra),
        Surface::AnthropicMessages => (
            status,
            Json(json!({
                "type": "error",
                "error": {
                    "type": anthropic_error_type(failure.category),
                    "message": failure.detail,
                },
                "shahid": {
                    "code": failure.code.as_deref().unwrap_or(failure.category_str()),
                    "extra": extra,
                },
            })),
        )
            .into_response(),
        Surface::GeminiGenerateContent => (
            status,
            Json(json!({
                "error": {
                    "code": status.as_u16(),
                    "message": failure.detail,
                    "status": gemini_error_status(failure.category),
                    "shahid": {
                        "code": failure.code.as_deref().unwrap_or(failure.category_str()),
                        "extra": extra,
                    },
                }
            })),
        )
            .into_response(),
    }
}

/// Gateway-local error in the family's wire shape (F-4).
fn family_local_error(
    surface: Surface,
    status: StatusCode,
    code: &str,
    message: &str,
    extra: Value,
    request_id: &str,
) -> Response {
    let extra = with_request_id(extra, request_id);
    match surface {
        Surface::OpenAiChat => openai_error(status, code, message, extra),
        Surface::AnthropicMessages => (
            status,
            Json(json!({
                "type": "error",
                "error": { "type": "invalid_request_error", "message": message },
                "shahid": { "code": code, "extra": extra },
            })),
        )
            .into_response(),
        Surface::GeminiGenerateContent => (
            status,
            Json(json!({
                "error": {
                    "code": status.as_u16(),
                    "message": message,
                    "status": "INVALID_ARGUMENT",
                    "shahid": { "code": code, "extra": extra },
                }
            })),
        )
            .into_response(),
    }
}

/// POST /v1/chat/completions (Phase 3 surface; streaming since Phase 7).
async fn chat_completions(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let stream_requested = parsed_stream_flag(&body);
    process_chat(
        api,
        Surface::OpenAiChat,
        None,
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}

/// POST /v1/messages (Phase 6, ADR-010): native Anthropic surface.
async fn anthropic_messages(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    let stream_requested = parsed_stream_flag(&body);
    process_chat(
        api,
        Surface::AnthropicMessages,
        None,
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}

/// POST /v1/messages/count_tokens (Phase 6, F-8): proxied for anthropic
/// deployments; 501 for aliases without one (ADR-010).
async fn anthropic_count_tokens(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    process_chat(
        api,
        Surface::AnthropicMessages,
        None,
        headers,
        body,
        Operation::CountTokens,
        false,
    )
    .await
}

/// POST /v1beta/models/{model_and_method} (Phase 6, F-10): the segment is
/// `{alias}:generateContent` (decoded — the Path extractor decodes %3A).
async fn gemini_generate(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(model_and_method): axum::extract::Path<String>,
    query: axum::extract::RawQuery,
    body: Bytes,
) -> Response {
    // F-9: the `?key=` auth form is rejected (tokens must never appear in
    // URLs); the x-goog-api-key header is the only accepted form.
    if query
        .0
        .as_deref()
        .is_some_and(|q| q.split('&').any(|pair| pair.starts_with("key=")))
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "code": 401,
                    "message": "query-parameter authentication is rejected; use the x-goog-api-key header",
                    "status": "UNAUTHENTICATED",
                    "shahid": { "code": "query_param_auth_rejected" }
                }
            })),
        )
            .into_response();
    }
    let Some((alias, method)) = model_and_method.rsplit_once(':') else {
        return family_local_error(
            Surface::GeminiGenerateContent,
            StatusCode::NOT_FOUND,
            "unknown_method",
            "expected `{model}:generateContent`",
            json!({}),
            "", // pre-correlation local error
        );
    };
    if method != "generateContent" && method != "streamGenerateContent" {
        return family_local_error(
            Surface::GeminiGenerateContent,
            StatusCode::NOT_FOUND,
            "unknown_method",
            &format!("unknown Gemini method `{method}`"),
            json!({}),
            "", // pre-correlation local error
        );
    }
    let stream_requested = method == "streamGenerateContent";
    process_chat(
        api,
        Surface::GeminiGenerateContent,
        Some(alias.to_string()),
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}

/// Per-family pre-validation (F-6): fails BEFORE routing so a malformed
/// request never burns an upstream attempt or marks a credential degraded.
fn validate_request(
    surface: Surface,
    operation: Operation,
    parsed: &Value,
) -> Result<(), (StatusCode, &'static str, String)> {
    match surface {
        Surface::OpenAiChat => {
            if parsed.get("model").and_then(Value::as_str).is_none() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "model_required",
                    "the `model` field (a configured alias) is required".to_string(),
                ));
            }
        }
        Surface::AnthropicMessages => {
            // F-8: count_tokens requires model + messages but NOT max_tokens.
            let needs_max_tokens = operation == Operation::Chat;
            if parsed.get("model").and_then(Value::as_str).is_none()
                || parsed.get("messages").is_none()
                || (needs_max_tokens && parsed.get("max_tokens").is_none())
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "anthropic requests require `model` and `messages` (chat also requires `max_tokens`)"
                        .to_string(),
                ));
            }
        }
        Surface::GeminiGenerateContent => {
            if parsed.get("contents").is_none() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "contents_required",
                    "gemini requests require `contents`".to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Chat,
    CountTokens,
}

/// What one upstream candidate produced (Phase 7).
enum UpstreamCall {
    Chat(ChatOutcome),
    /// SSE committed: the byte stream must be relayed; no failover after.
    CommittedStream(sair_providers::openai::StreamOutcome),
    /// 2xx non-SSE body (F-2): normal passthrough.
    NonStreamed {
        status: u16,
        body: Value,
    },
}

/// FR-021: a stream wrapper that releases the deployment permit when the
/// relayed stream ends (F-6: the permit lives as long as the stream).
struct PermitStream<S> {
    inner: S,
    /// Held (not read) so the deployment slot stays occupied until the
    /// relayed stream ends (FR-021).
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl<S> PermitStream<S> {
    fn new(inner: S, permit: tokio::sync::OwnedSemaphorePermit) -> Self {
        Self {
            inner,
            _permit: permit,
        }
    }
}

impl<S> futures_core::Stream for PermitStream<S>
where
    S: futures_core::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    type Item = std::result::Result<bytes::Bytes, reqwest::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.inner).poll_next(cx)
    }
}

/// Maps a family streaming start into the unified upstream-call shape.
fn upstream_call_from_start(start: StreamStart) -> UpstreamCall {
    match start {
        StreamStart::Committed(outcome) => UpstreamCall::CommittedStream(outcome),
        StreamStart::NonStreamed { status, body } => UpstreamCall::NonStreamed { status, body },
    }
}

/// Per-family usage extraction for non-streamed bodies.
fn extract_usage(surface: Surface, body: &Value) -> (Option<i64>, Option<i64>) {
    let usage = match surface {
        Surface::OpenAiChat => body.get("usage").map(|u| (u, u)),
        Surface::AnthropicMessages => body.get("usage").map(|u| (u, u)),
        Surface::GeminiGenerateContent => body.get("usageMetadata").map(|u| (u, u)),
    };
    match surface {
        Surface::OpenAiChat => (
            usage
                .and_then(|(u, _)| u.get("prompt_tokens"))
                .and_then(Value::as_i64),
            usage
                .and_then(|(_, u)| u.get("completion_tokens"))
                .and_then(Value::as_i64),
        ),
        Surface::AnthropicMessages => (
            usage
                .and_then(|(u, _)| u.get("input_tokens"))
                .and_then(Value::as_i64),
            usage
                .and_then(|(_, u)| u.get("output_tokens"))
                .and_then(Value::as_i64),
        ),
        Surface::GeminiGenerateContent => (
            usage
                .and_then(|(u, _)| u.get("promptTokenCount"))
                .and_then(Value::as_i64),
            usage
                .and_then(|(_, u)| u.get("candidatesTokenCount"))
                .and_then(Value::as_i64),
        ),
    }
}

/// Family-parameterized chat/count flow: validation, routing, rotation loop,
/// per-family envelopes (Phase 6 refactor; behavior preserved per family).
#[allow(clippy::too_many_lines)]
#[allow(clippy::too_many_arguments)]
async fn process_chat(
    api: Arc<ApiDeps>,
    surface: Surface,
    path_model: Option<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
    operation: Operation,
    stream_requested: bool,
) -> Response {
    let started = Instant::now();
    let correlation = correlation_id();

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return family_local_error(
                surface,
                StatusCode::BAD_REQUEST,
                "invalid_json",
                &format!("request body is not valid JSON: {e}"),
                json!({}),
                &correlation,
            );
        }
    };
    if let Err((status, code, message)) = validate_request(surface, operation, &parsed) {
        return family_local_error(surface, status, code, &message, json!({}), &correlation);
    }
    // F-9: Gemini clients may send `?key=` — that form is rejected (the
    // token must never appear in URLs); the header form is the only auth.
    let _ = &headers;
    // Alias source: body `model` for OpenAI/Anthropic, path for Gemini.
    let alias_owned: String = match (&surface, path_model) {
        (Surface::GeminiGenerateContent, Some(m)) => m,
        (Surface::OpenAiChat, None) | (Surface::AnthropicMessages, None) => parsed
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        (Surface::GeminiGenerateContent, None) => String::new(),
        (_, Some(m)) => m,
    };
    let alias: &str = &alias_owned;

    let (candidates, excluded) = match load_route_snapshot_detailed(&api.db, alias).await {
        Ok(pair) => pair,
        Err(e) => {
            return family_local_error(
                surface,
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("route snapshot unavailable: {e}"),
                json!({}),
                &correlation,
            );
        }
    };
    if candidates.is_empty() && excluded.is_empty() {
        return family_local_error(
            surface,
            StatusCode::NOT_FOUND,
            "alias_not_found",
            &format!("no configured alias named `{alias}`"),
            json!({}),
            &correlation,
        );
    }

    let policy = match api.db.repos().ensure_default_policy().await {
        Ok(p) => p,
        Err(e) => {
            return family_local_error(
                surface,
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("routing policy unavailable: {e}"),
                json!({}),
                &correlation,
            );
        }
    };
    let strategy = match policy.strategy.as_str() {
        "priority" => Strategy::Priority,
        "round_robin" => Strategy::RoundRobin,
        "weighted" => Strategy::Weighted,
        other => {
            return family_local_error(
                surface,
                StatusCode::NOT_IMPLEMENTED,
                "strategy_not_implemented",
                &format!("routing strategy `{other}` is declared but not implemented"),
                json!({}),
                &correlation,
            );
        }
    };

    let requirements = sair_domain::routing::CapabilityRequirements::derive_from_body(&parsed);
    let decision = match plan_route(
        alias,
        surface.family(),
        strategy,
        requirements,
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
            // Phase 9 (F-1a): no-route requests are recorded too — the Logs
            // screen must show the "everything unavailable" case.
            if matches!(e, crate::routing::RouteError::NoEligibleDeployment) {
                let record = RequestRecordInsert {
                    id: correlation.clone(),
                    ts: sair_persistence::time::now_iso(),
                    correlation_id: correlation.clone(),
                    protocol_family: Some(surface.label().to_string()),
                    operation: Some(
                        match operation {
                            Operation::Chat => "chat",
                            Operation::CountTokens => "count_tokens",
                        }
                        .to_string(),
                    ),
                    alias: Some(alias.to_string()),
                    error_category: Some("no_eligible_deployment".to_string()),
                    decision_json: decision_json_placeholder(&excluded, stream_requested),
                    ..RequestRecordInsert::default()
                };
                let _ = api.db.repos().insert_request_record(&record).await;
            }
            // F-4/F-8: count_tokens on an alias without an anthropic
            // deployment is a structured 501 (ADR-010).
            if operation == Operation::CountTokens
                && matches!(e, crate::routing::RouteError::NoEligibleDeployment)
            {
                return family_local_error(
                    surface,
                    StatusCode::NOT_IMPLEMENTED,
                    "count_tokens_not_supported",
                    "count_tokens is only available for aliases with anthropic deployments",
                    json!({ "alias": alias }),
                    &correlation,
                );
            }
            // F-4: surface why every candidate was excluded (FR-012).
            let reasons: Vec<String> = excluded
                .iter()
                .map(|x| format!("{}: {}", x.deployment_id, x.reason))
                .collect();
            return family_local_error(
                surface,
                status,
                &code,
                &e.to_string(),
                json!({ "alias": alias, "reasons": reasons }),
                &correlation,
            );
        }
    };

    let decision_json = Some(
        serde_json::json!({
            "decision": decision,
            "streamed": stream_requested,
        })
        .to_string(),
    );
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
        // Floor at 1 s: a zero budget previously collapsed to 1 ms, which
        // aborted the loop after two DB round-trips with a bare
        // no_attempt_made 503 and no explanation (audit P3 fix).
        if started.elapsed() > Duration::from_millis(policy.retry_budget_ms.max(1_000) as u64) {
            tracing::warn!(alias, "retry budget exhausted before rotation");
            break;
        }
        // R4-02: dedupe credentials across deployments of the same provider —
        // membership checked here, but inserted only AFTER a permit is
        // acquired. A busy-skip must not poison the credential for a later
        // deployment of the same provider that has a free slot (audit P2 fix).
        if attempted_credentials.contains(&candidate.credential_id) {
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
        attempted_credentials.insert(candidate.credential_id.clone());
        actual_attempts += 1;

        // Retrieve the secret at call time only (docs/06). A broken
        // credential (malformed ref / missing vault item) rotates to the next
        // candidate instead of aborting the whole alias (audit P3 fix).
        let secret_ref = match sair_vault::SecretRef::parse(&candidate.credential_secret_ref) {
            Ok(reference) => reference,
            Err(_) => {
                last_failure = Some(UpstreamFailure::internal(
                    "stored credential reference is malformed",
                ));
                continue;
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
                continue;
            }
            Err(e) => {
                // Vault-wide failure (not per-credential): surface in the
                // family's own envelope (audit P2 fix — this previously sent
                // an OpenAI-shaped error on the Anthropic/Gemini surfaces).
                return family_local_error(
                    surface,
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "vault_unavailable",
                    &format!("credential vault unavailable: {e}"),
                    json!({ "alias": alias }),
                    &correlation,
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

        let timeout = Duration::from_millis(policy.timeout_ms.max(1) as u64);
        // Phase 6: per-surface adapter dispatch. OpenRouter attribution
        // (F-1, Phase 5) applies only on the OpenAI surface to openrouter
        // providers — never leaked elsewhere.
        // Phase 7 (F-1): streaming requests use the dedicated streaming
        // clients (no total deadline) and the F-2 commit predicate.
        let streaming_client = if candidate.allow_loopback {
            &api.client_loopback_streaming
        } else {
            &api.client_strict_streaming
        };
        let send_result = match (surface, operation) {
            (Surface::OpenAiChat, Operation::Chat) if stream_requested => {
                let extra = if candidate.provider_type == "openrouter" {
                    api.openrouter_attribution.headers()
                } else {
                    Vec::new()
                };
                send_chat_streaming_with_headers(
                    streaming_client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    parsed.clone(),
                    &extra,
                )
                .await
                .map(upstream_call_from_start)
            }
            (Surface::AnthropicMessages, Operation::Chat) if stream_requested => {
                let version = headers
                    .get("anthropic-version")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(sair_providers::anthropic::DEFAULT_ANTHROPIC_VERSION)
                    .to_string();
                sair_providers::anthropic::send_messages_streaming(
                    streaming_client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    &version,
                    parsed.clone(),
                )
                .await
                .map(upstream_call_from_start)
            }
            (Surface::GeminiGenerateContent, Operation::Chat) if stream_requested => {
                generate_content_streaming(
                    streaming_client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    parsed.clone(),
                )
                .await
                .map(upstream_call_from_start)
            }

            (Surface::OpenAiChat, Operation::Chat) => {
                let outcome = if candidate.provider_type == "openrouter" {
                    let extra = api.openrouter_attribution.headers();
                    send_chat_with_headers(
                        client,
                        &base,
                        &candidate.model_id,
                        &secret,
                        parsed.clone(),
                        timeout,
                        &extra,
                    )
                    .await
                } else {
                    send_chat(
                        client,
                        &base,
                        &candidate.model_id,
                        &secret,
                        parsed.clone(),
                        timeout,
                    )
                    .await
                };
                outcome.map(UpstreamCall::Chat)
            }
            (Surface::AnthropicMessages, Operation::Chat) => {
                // F-5: forward a validated client anthropic-version, else the
                // pinned legacy default.
                let version = headers
                    .get("anthropic-version")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(sair_providers::anthropic::DEFAULT_ANTHROPIC_VERSION)
                    .to_string();
                sair_providers::anthropic::send_messages(
                    client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    &version,
                    parsed.clone(),
                    timeout,
                )
                .await
                .map(UpstreamCall::Chat)
            }
            (Surface::AnthropicMessages, Operation::CountTokens) => {
                let version = headers
                    .get("anthropic-version")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(sair_providers::anthropic::DEFAULT_ANTHROPIC_VERSION)
                    .to_string();
                sair_providers::anthropic::count_tokens(
                    client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    &version,
                    parsed.clone(),
                    timeout,
                )
                .await
                .map(UpstreamCall::Chat)
            }
            // count_tokens is defined only for the Anthropic surface
            // (ADR-010); the count handler routes only AnthropicMessages,
            // so this arm is defensive.
            (Surface::OpenAiChat, Operation::CountTokens)
            | (Surface::GeminiGenerateContent, Operation::CountTokens) => {
                Err(UpstreamFailure::internal(
                    "count_tokens is only supported on the anthropic surface",
                ))
            }
            (Surface::GeminiGenerateContent, _) => sair_providers::gemini::generate_content(
                client,
                &base,
                &candidate.model_id,
                &secret,
                parsed.clone(),
                timeout,
            )
            .await
            .map(UpstreamCall::Chat),
        };
        match send_result {
            Ok(UpstreamCall::Chat(ChatOutcome {
                status,
                body,
                tokens_in,
                tokens_out,
            })) => {
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    surface,
                    operation,
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
            Ok(UpstreamCall::CommittedStream(outcome)) => {
                // FR-010/docs/05: commitment point. From here the loop is
                // exited by construction — no failover after bytes flow.
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    surface,
                    operation,
                    &correlation,
                    alias,
                    candidate,
                    Some(outcome.status),
                    None,
                    latency,
                    None,
                    None,
                    &decision_json,
                )
                .await;
                let content_type = outcome
                    .content_type
                    .clone()
                    .unwrap_or_else(|| "text/event-stream".to_string());
                // FR-021: the permit moves INTO the response stream and is
                // released when the relayed stream ends (F-6 normative).
                let body = axum::body::Body::from_stream(PermitStream::new(
                    outcome.byte_stream,
                    deployment_permit,
                ));
                return (
                    StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::OK),
                    [(axum::http::header::CONTENT_TYPE, content_type)],
                    body,
                )
                    .into_response();
            }
            Ok(UpstreamCall::NonStreamed { status, body }) => {
                // F-2: 2xx non-SSE → mapped through the family usage shape
                // and returned as a normal (non-streamed) passthrough.
                let (tokens_in, tokens_out) = extract_usage(surface, &body);
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    surface,
                    operation,
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
                            surface,
                            operation,
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
                        return family_error_response(
                            surface,
                            &failure,
                            alias,
                            decision.exclusions.len(),
                            &correlation,
                        );
                    }
                }
            }
        }
    }

    // Rotation budget exhausted or rotation set exhausted.
    if let Some(failure) = last_failure {
        record_failure_minimal(
            &api,
            surface,
            operation,
            &correlation,
            alias,
            &failure,
            &decision_json,
        )
        .await;
        return family_error_response(
            surface,
            &failure,
            alias,
            decision.exclusions.len(),
            &correlation,
        );
    }
    // R4-06: distinguish terminal states (actual upstream attempts, not
    // reserved credentials).
    if busy_skips > 0 && actual_attempts == 0 {
        return family_local_error(
            surface,
            StatusCode::SERVICE_UNAVAILABLE,
            "deployment_busy",
            "every eligible deployment is at its concurrency bound",
            json!({ "alias": alias, "busy_deployments": busy_skips }),
            &correlation,
        );
    }
    family_local_error(
        surface,
        StatusCode::SERVICE_UNAVAILABLE,
        "no_attempt_made",
        "routing produced no executable candidate",
        json!({
            "alias": alias,
            "exclusions": decision.exclusions.len(),
            "skipped_duplicates": skipped_duplicates,
            "busy_deployments": busy_skips,
        }),
        &correlation,
    )
}

#[allow(clippy::too_many_arguments)]
async fn record(
    api: &ApiDeps,
    surface: Surface,
    operation: Operation,
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
        protocol_family: Some(surface.label().to_string()),
        operation: Some(
            match operation {
                Operation::Chat => "chat",
                Operation::CountTokens => "count_tokens",
            }
            .to_string(),
        ),
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
    surface: Surface,
    operation: Operation,
    correlation: &str,
    alias: &str,
    failure: &UpstreamFailure,
    decision_json: &Option<String>,
) {
    let record = RequestRecordInsert {
        id: correlation.to_string(),
        ts: sair_persistence::time::now_iso(),
        correlation_id: correlation.to_string(),
        protocol_family: Some(surface.label().to_string()),
        operation: Some(
            match operation {
                Operation::Chat => "chat",
                Operation::CountTokens => "count_tokens",
            }
            .to_string(),
        ),
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
    use zeroize::Zeroizing;

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

        let api = Arc::new(ApiDeps::new(db, vault, 4, Default::default(), 300_000));
        let config = GatewayConfig {
            token: Arc::new(Zeroizing::new(TOKEN.as_bytes().to_vec())),
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

    /// Phase 7: streaming is supported. A 2xx non-SSE upstream response
    /// resolves through the F-2 predicate as non-streamed passthrough.
    #[tokio::test]
    async fn streaming_request_with_plain_json_upstream_passes_through() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "cmpl-plain"})))
            .mount(&upstream)
            .await;
        let handle = setup(&upstream).await;
        let response = chat(
            &handle,
            TOKEN,
            json!({"model": "test-model", "stream": true, "messages": []}),
        )
        .await;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        assert_eq!(status, 200, "body: {text}");
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["id"], "cmpl-plain");
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
        // F-1: same-protocol upstream status is preserved (upstream 401).
        assert_eq!(response.status(), 401);
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
        // F-1: upstream 429 preserved; quota-truth classification in shahid.
        assert_eq!(response.status(), 429);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["shahid"]["category"], "billing_or_quota");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }
}
