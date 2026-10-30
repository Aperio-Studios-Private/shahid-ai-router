//! Local loopback HTTP gateway (FR-005, ADR-011).
//!
//! Skeleton: a router with `/healthz`. Phase 2 adds the middleware stack
//! (token auth, Host allowlist, body size, concurrency limits) and Phase 3
//! the protocol surfaces. The gateway binds loopback only; no CORS headers
//! are emitted.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

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
}

/// Builds the public router. Kept pure (no side effects) for testability;
/// the server task wrapper arrives with lifecycle work (FR-015/FR-017).
pub fn router() -> Router {
    Router::new().route("/healthz", get(health))
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::router;

    #[tokio::test]
    async fn healthz_reports_ok() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["status"], "ok");
    }

    #[tokio::test]
    async fn unknown_routes_are_404() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/definitely-not-here")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
