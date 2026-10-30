//! Local loopback HTTP gateway (FR-005, ADR-011 as amended by the Phase 2
//! review findings F-1/F-2).
//!
//! Middleware stack, outermost first (request order):
//! 1. Concurrency guard — immediate 503 at capacity (no unbounded queueing).
//! 2. Timeout budget (tower-http) — 408 when the budget elapses.
//! 3. Host allowlist — exact `127.0.0.1[:port]` / `localhost[:port]` only.
//! 4. Token auth — `Authorization: Bearer`, `x-api-key` or `x-goog-api-key`;
//!    constant-time comparison. `/healthz` sits behind auth too: the
//!    invariant is that no unauthenticated route exists (including 404s).
//! 5. Body size limit — 413 over budget.
//!
//! No CORS headers are emitted. The gateway binds `127.0.0.1` only.

pub mod api;
#[cfg(test)]
mod api_rotation_tests;
pub mod handle;
pub mod routing;
pub mod token;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use tower_http::timeout::TimeoutLayer;

pub use handle::{GatewayConfig, GatewayHandle};
pub use token::TokenManager;

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("gateway is already running")]
    AlreadyRunning,
    #[error("gateway is not running")]
    NotRunning,
    #[error("failed to bind {addr}: {source}")]
    Bind {
        addr: std::net::SocketAddr,
        source: std::io::Error,
    },
    #[error("drain did not finish within the grace period")]
    DrainTimedOut,
    #[error("gateway token failure: {0}")]
    Token(String),
}

/// Per-router runtime state shared with middleware. The expected token is
/// behind a lock so `GatewayHandle::update_token` hot-swaps it without a
/// restart (security review finding SEC2-02: rotation must revoke the old
/// token immediately).
#[derive(Clone)]
pub struct GatewayRuntimeState {
    expected_token: Arc<tokio::sync::RwLock<Arc<Vec<u8>>>>,
    permits: Arc<tokio::sync::Semaphore>,
}

/// Builds the public router with the full middleware stack.
pub fn router(config: &GatewayConfig) -> Router {
    let state = GatewayRuntimeState {
        expected_token: Arc::new(tokio::sync::RwLock::new(config.token.clone())),
        permits: Arc::new(tokio::sync::Semaphore::new(config.concurrency)),
    };
    router_with_state(config, state)
}

/// Builds the router around an externally owned token lock so
/// [`GatewayHandle::update_token`] hot-swaps the live router's expectation
/// (SEC2-02).
pub(crate) fn router_with_state(config: &GatewayConfig, state: GatewayRuntimeState) -> Router {
    // axum applies layers outermost-last: the first `.layer` added is the
    // innermost. Request order = reverse of the order below.
    let mut router = Router::new()
        .route("/healthz", get(health))
        .fallback(fallback_404);
    // Protocol surfaces merge BEFORE the layers so auth/Host/limits apply.
    if let Some(api) = &config.api {
        router = router.merge(crate::api::router(api.clone()));
    }
    #[cfg(test)]
    let router = {
        use axum::body::Bytes;
        router
            .route(
                "/test/slow",
                get(|| async {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    "slow-done"
                }),
            )
            .route(
                "/test/echo",
                axum::routing::post(|body: Bytes| async move { body.len().to_string() }),
            )
    };
    router
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            token_auth,
        ))
        .layer(axum::middleware::from_fn(host_allowlist))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_millis(config.timeout_ms),
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            concurrency_guard,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(
            config.body_limit_bytes,
        ))
    // Note: the router's state type is `()` — middleware state is supplied
    // via `from_fn_with_state`, and the API surface carries its own
    // `Arc<ApiDeps>` state (merged above).
}

async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "service": "shahid-ai-router-gateway",
        })),
    )
}

async fn fallback_404() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": { "code": "not_found" } })),
    )
}

/// Concurrency guard: try-acquire (no waiting); 503 when saturated (F-1).
async fn concurrency_guard(
    State(state): State<GatewayRuntimeState>,
    req: Request,
    next: Next,
) -> Response {
    match state.permits.clone().try_acquire_owned() {
        Ok(permit) => {
            let response = next.run(req).await;
            drop(permit);
            response
        }
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": { "code": "gateway_busy" } })),
        )
            .into_response(),
    }
}

/// Host allowlist (F-2/R-5): exact host match, case-insensitive, optional
/// port; missing Host is rejected. DNS-rebinding defense.
async fn host_allowlist(req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());
    if host.is_some_and(is_allowed_host) {
        next.run(req).await
    } else {
        (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": { "code": "host_not_allowed" } })),
        )
            .into_response()
    }
}

fn is_allowed_host(host: &str) -> bool {
    let (name, _port) = match host.rsplit_once(':') {
        Some((n, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (n, p),
        // No port, or a colon that is part of an (unsupported) IPv6 literal.
        _ => (host, ""),
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "localhost"
    )
}

/// Token auth: Bearer / x-api-key / x-goog-api-key, constant-time compare
/// against the current token (hot-swappable without restart, SEC2-02).
async fn token_auth(
    State(state): State<GatewayRuntimeState>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    let provided = extract_token(&headers);
    let expected = state.expected_token.read().await.clone();
    if token::verify_token(provided, &expected) {
        next.run(req).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": { "code": "unauthorized" } })),
        )
            .into_response()
    }
}

fn extract_token(headers: &HeaderMap) -> Option<&str> {
    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        });
    if let Some(token) = bearer {
        return Some(token.trim());
    }
    ["x-api-key", "x-goog-api-key"].iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|trimmed| !trimmed.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::GatewayConfig;
    use super::router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_config() -> GatewayConfig {
        GatewayConfig {
            token: std::sync::Arc::new(b"0123456789abcdefghijklmnopqrstuvwxyz0".to_vec()),
            ..GatewayConfig::default()
        }
    }

    fn authorized_request(uri: &str, method: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:8787")
            .header(
                "authorization",
                "Bearer 0123456789abcdefghijklmnopqrstuvwxyz0",
            )
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn healthz_ok_when_authorized() {
        let response = router(&test_config())
            .oneshot(authorized_request("/healthz", "GET"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn healthz_requires_auth() {
        // No unauthenticated route exists (F-2): healthz included.
        let response = router(&test_config())
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .header("host", "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn fallback_is_behind_auth() {
        let response = router(&test_config())
            .oneshot(
                Request::builder()
                    .uri("/definitely-not-here")
                    .header("host", "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rejects_bad_token() {
        let response = router(&test_config())
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .header("host", "127.0.0.1:8787")
                    .header("authorization", "Bearer wrong-token-wrong-token-wrong-toke")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn accepts_x_api_key_form() {
        let response = router(&test_config())
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .header("host", "127.0.0.1:8787")
                    .header("x-api-key", "0123456789abcdefghijklmnopqrstuvwxyz0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn host_allowlist_rejects_spoofs_and_missing_host() {
        let router = router(&test_config());
        for host in [
            "127.0.0.1.evil.com:8787",
            "evil.com",
            "LOCALHOST:9999", // case-insensitive exact match is allowed...
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/healthz")
                        .header("host", host)
                        .header(
                            "authorization",
                            "Bearer 0123456789abcdefghijklmnopqrstuvwxyz0",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            if host == "LOCALHOST:9999" {
                assert_eq!(response.status(), StatusCode::OK, "case-insensitive OK");
            } else {
                assert_eq!(response.status(), StatusCode::FORBIDDEN, "spoof {host}");
            }
        }
        // Missing Host (HTTP/1.0 style) → 403.
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .header(
                        "authorization",
                        "Bearer 0123456789abcdefghijklmnopqrstuvwxyz0",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn head_on_protected_route_still_goes_through_middleware() {
        let router = router(&test_config());
        let with_auth = router
            .clone()
            .oneshot(authorized_request("/healthz", "HEAD"))
            .await
            .unwrap();
        assert_eq!(with_auth.status(), StatusCode::OK);
        let without_auth = router
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri("/healthz")
                    .header("host", "127.0.0.1:8787")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(without_auth.status(), StatusCode::UNAUTHORIZED);
    }
}
