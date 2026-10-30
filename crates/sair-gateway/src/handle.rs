//! Gateway lifecycle handle (FR-015/FR-017/FR-018, review finding F-9).
//!
//! `start` binds `127.0.0.1:<port>` only and surfaces bind failures as an
//! explicit error (no fallback port). `stop(grace)` drains in-flight
//! requests via axum's graceful shutdown; if the grace period elapses the
//! task is aborted, which is the bounded-termination guarantee.

use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::GatewayError;
use crate::router;

/// Runtime configuration for one gateway instance.
#[derive(Clone)]
pub struct GatewayConfig {
    /// The expected gateway token (from the OS vault).
    pub token: std::sync::Arc<Vec<u8>>,
    /// Loopback port (FR-005).
    pub port: u16,
    /// Request body size budget in bytes (default 32 MiB).
    pub body_limit_bytes: usize,
    /// Maximum concurrently executing requests; excess → 503 (F-1).
    pub concurrency: usize,
    /// Per-request timeout budget in ms (F-1); exceeded → 408.
    pub timeout_ms: u64,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            token: std::sync::Arc::new(Vec::new()),
            port: 8787,
            body_limit_bytes: 32 * 1024 * 1024,
            concurrency: 16,
            timeout_ms: 120_000,
        }
    }
}

/// Owns the running server task.
#[derive(Debug)]
pub struct GatewayHandle {
    addr: std::net::SocketAddr,
    task: JoinHandle<()>,
    shutdown: watch::Sender<bool>,
}

impl GatewayHandle {
    /// Binds the loopback listener and spawns the server task.
    ///
    /// # Errors
    /// [`GatewayError::Bind`] when the port cannot be bound (FR-018: no
    /// fallback port is attempted).
    pub async fn start(config: GatewayConfig) -> Result<Self, GatewayError> {
        let bind_addr = std::net::SocketAddr::from(([127, 0, 0, 1], config.port));
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|source| GatewayError::Bind {
                addr: bind_addr,
                source,
            })?;
        // The bound address is authoritative (supports ephemeral ports in
        // tests; equals the configured port in normal operation).
        let addr = listener.local_addr().map_err(|source| GatewayError::Bind {
            addr: bind_addr,
            source,
        })?;
        let app = router(&config);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = shutdown_rx.wait_for(|stop| *stop).await;
            });
            if let Err(err) = server.await {
                // An accept-loop failure after a successful bind is
                // contained and observable (docs/02 failure isolation).
                tracing::error!(error = %err, "gateway server task failed");
            }
        });
        Ok(Self {
            addr,
            task,
            shutdown,
        })
    }

    #[must_use]
    pub fn addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    /// Signals shutdown and waits up to `grace` for in-flight requests to
    /// finish (FR-017). Aborts the task when the grace period elapses.
    ///
    /// # Errors
    /// [`GatewayError::DrainTimedOut`] when the grace period elapses; the
    /// task is aborted in that case (bounded termination).
    pub async fn stop(self, grace: Duration) -> Result<(), GatewayError> {
        let _ = self.shutdown.send(true);
        let mut task = self.task;
        match tokio::time::timeout(grace, &mut task).await {
            Ok(Ok(())) | Ok(Err(_)) => Ok(()),
            Err(_elapsed) => {
                task.abort();
                Err(GatewayError::DrainTimedOut)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{GatewayConfig, GatewayError, GatewayHandle};

    fn config(port: u16, timeout_ms: u64, concurrency: usize, body_limit: usize) -> GatewayConfig {
        GatewayConfig {
            token: Arc::new(b"0123456789abcdefghijklmnopqrstuvwxyz0".to_vec()),
            port,
            body_limit_bytes: body_limit,
            concurrency,
            timeout_ms,
        }
    }

    const AUTH: &str = "Bearer 0123456789abcdefghijklmnopqrstuvwxyz0";

    fn client() -> reqwest::Client {
        // reqwest 0.13 requires an installed rustls provider at client build
        // even for plain-HTTP test traffic (ADR-006: the app installs ring
        // before any client exists; tests do the same, idempotently).
        let _ = rustls::crypto::ring::default_provider().install_default();
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    fn url(addr: std::net::SocketAddr, path: &str) -> String {
        format!("http://{addr}{path}")
    }

    #[tokio::test]
    async fn start_stop_and_healthz_roundtrip() {
        let handle = GatewayHandle::start(config(0, 5_000, 8, 1 << 20))
            .await
            .unwrap();
        let addr = handle.addr();
        let response = client()
            .get(url(addr, "/healthz"))
            .header("authorization", AUTH)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        handle.stop(Duration::from_secs(2)).await.unwrap();

        // After stop, the listener is gone: connection refused.
        let result = client()
            .get(url(addr, "/healthz"))
            .header("authorization", AUTH)
            .send()
            .await;
        assert!(result.is_err(), "gateway must not accept after stop");
    }

    #[tokio::test]
    async fn occupied_port_surfaces_bind_error_and_no_fallback() {
        // Hold a real listener, then try to start on the same port (FR-018).
        let holder = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let occupied = holder.local_addr().unwrap().port();
        let error = GatewayHandle::start(config(occupied, 5_000, 8, 1 << 20))
            .await
            .unwrap_err();
        assert!(matches!(error, GatewayError::Bind { .. }));
        drop(holder);
    }

    #[tokio::test]
    async fn in_flight_request_completes_during_graceful_drain() {
        let handle = GatewayHandle::start(config(0, 5_000, 8, 1 << 20))
            .await
            .unwrap();
        let client = client();
        let url = url(handle.addr(), "/test/slow");
        let inflight = tokio::spawn(async move {
            client
                .get(url)
                .header("authorization", AUTH)
                .send()
                .await
                .unwrap()
        });
        // Let the request reach the slow handler while the server runs.
        tokio::time::sleep(Duration::from_millis(100)).await;
        // FR-017: drain lets the in-flight request finish within the grace.
        handle.stop(Duration::from_secs(2)).await.unwrap();
        let response = inflight.await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "slow-done");
    }

    #[tokio::test]
    async fn slow_request_hits_timeout_budget() {
        let handle = GatewayHandle::start(config(0, 50, 8, 1 << 20))
            .await
            .unwrap();
        let response = client()
            .get(url(handle.addr(), "/test/slow"))
            .header("authorization", AUTH)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::REQUEST_TIMEOUT);
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn saturated_concurrency_returns_503_immediately() {
        // One slot; the slow request holds it; the second must get 503 at
        // once (no queueing, F-1).
        let handle = GatewayHandle::start(config(0, 5_000, 1, 1 << 20))
            .await
            .unwrap();
        let addr = handle.addr();
        let first_client = client();
        let first_url = url(addr, "/test/slow");
        let first = tokio::spawn(async move {
            first_client
                .get(first_url)
                .header("authorization", AUTH)
                .send()
                .await
                .unwrap()
        });
        // Let the spawned request acquire the only slot.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = std::time::Instant::now();
        let second = client()
            .get(url(addr, "/healthz"))
            .header("authorization", AUTH)
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        assert!(started.elapsed() < Duration::from_millis(500));
        handle.stop(Duration::from_secs(3)).await.unwrap();
        // The slow request still completes (slot was its own).
        assert_eq!(first.await.unwrap().status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn oversized_body_is_rejected_with_413() {
        let handle = GatewayHandle::start(config(0, 5_000, 8, 1_024))
            .await
            .unwrap();
        let body = vec![b'x'; 4_096];
        let response = client()
            .post(url(handle.addr(), "/test/echo"))
            .header("authorization", AUTH)
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    #[tokio::test]
    async fn end_to_end_token_via_x_api_key() {
        let handle = GatewayHandle::start(config(0, 5_000, 8, 1 << 20))
            .await
            .unwrap();
        let response = client()
            .get(url(handle.addr(), "/healthz"))
            .header("x-api-key", "0123456789abcdefghijklmnopqrstuvwxyz0")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let denied = client()
            .get(url(handle.addr(), "/healthz"))
            .header("x-api-key", "wrong-wrong-wrong-wrong-wrong-wrong-")
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), reqwest::StatusCode::UNAUTHORIZED);
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }
}
