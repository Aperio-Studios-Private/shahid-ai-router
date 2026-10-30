//! Gateway lifecycle handle (FR-015/FR-017/FR-018, review findings F-9/F-1,
//! security review findings SEC2-02/SEC2-03).
//!
//! `start` binds `127.0.0.1:<port>` only, rejects an empty token (fail-closed,
//! SEC2-07), and surfaces bind failures as an explicit error (no fallback
//! port). Connections are served by hyper-util with a header-read timeout and
//! timer so slowloris partial-header clients are dropped (SEC2-03).
//! `stop(grace)` drains in-flight requests within the grace; elapsing aborts
//! the server (bounded termination). `update_token` hot-swaps the expected
//! token without a restart so rotation revokes the old token immediately
//! (SEC2-02).

use std::sync::Arc;
use std::time::Duration;

use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::task::JoinSet;

use crate::GatewayError;

/// Runtime configuration for one gateway instance.
#[derive(Clone)]
pub struct GatewayConfig {
    /// The expected gateway token (from the OS vault). Must be non-empty.
    pub token: Arc<Vec<u8>>,
    /// Loopback port (FR-005).
    pub port: u16,
    /// Request body size budget in bytes (default 32 MiB).
    pub body_limit_bytes: usize,
    /// Maximum concurrently executing requests; excess → 503 (F-1).
    pub concurrency: usize,
    /// Per-request timeout budget in ms (F-1); exceeded → 408.
    pub timeout_ms: u64,
    /// Per-connection header-read timeout in ms; slowloris bound (SEC2-03).
    pub header_timeout_ms: u64,
    /// Protocol-surface dependencies (Phase 3+). `None` = gateway serves
    /// health only.
    pub api: Option<Arc<crate::api::ApiDeps>>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            token: Arc::new(Vec::new()),
            port: 8787,
            body_limit_bytes: 32 * 1024 * 1024,
            concurrency: 16,
            timeout_ms: 120_000,
            header_timeout_ms: 10_000,
            api: None,
        }
    }
}

/// Owns the running server task.
#[derive(Debug)]
pub struct GatewayHandle {
    addr: std::net::SocketAddr,
    task: JoinHandle<()>,
    shutdown: watch::Sender<bool>,
    token: Arc<tokio::sync::RwLock<Arc<Vec<u8>>>>,
}

impl GatewayHandle {
    /// Binds the loopback listener and spawns the server task.
    ///
    /// # Errors
    /// [`GatewayError::Bind`] when the port cannot be bound (FR-018: no
    /// fallback port is attempted); [`GatewayError::Token`] when the token
    /// is empty (SEC2-07: an empty token must never authenticate).
    pub async fn start(config: GatewayConfig) -> Result<Self, GatewayError> {
        if config.token.is_empty() {
            return Err(GatewayError::Token(
                "gateway token must be non-empty (fail-closed, SEC2-07)".to_string(),
            ));
        }
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
        let token = Arc::new(tokio::sync::RwLock::new(config.token.clone()));
        let state = crate::GatewayRuntimeState {
            expected_token: token.clone(),
            permits: Arc::new(tokio::sync::Semaphore::new(config.concurrency)),
        };
        let app = crate::router_with_state(&config, state);
        let (shutdown, shutdown_rx) = watch::channel(false);
        let header_timeout = Duration::from_millis(config.header_timeout_ms.max(1));
        let task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            let builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
            let mut connections = JoinSet::new();
            let mut accept_failed = false;
            loop {
                tokio::select! {
                    _ = shutdown_rx.wait_for(|stop| *stop) => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _peer)) => {
                            let app = app.clone();
                            let mut conn = builder.clone();
                            connections.spawn(async move {
                                use tower::ServiceExt as _;
                                let service = hyper::service::service_fn(
                                    move |req: hyper::Request<hyper::body::Incoming>| {
                                        let app = app.clone();
                                        async move {
                                            app.oneshot(req.map(axum::body::Body::new)).await
                                        }
                                    },
                                );
                                let _ = conn
                                    .http1()
                                    .timer(TokioTimer::new())
                                    .header_read_timeout(header_timeout)
                                    .serve_connection_with_upgrades(
                                        TokioIo::new(stream),
                                        service,
                                    )
                                    .await;
                            });
                        }
                        Err(err) => {
                            // Accept-loop failure after a successful bind is
                            // contained and observable (docs/02 failure
                            // isolation).
                            tracing::error!(error = %err, "gateway accept failed");
                            accept_failed = true;
                            break;
                        }
                    },
                }
            }
            // Drain: give in-flight connections a bounded window (FR-017);
            // the outer stop(grace) still bounds total shutdown time.
            drop(listener);
            let _ = tokio::time::timeout(Duration::from_secs(30), async {
                while connections.join_next().await.is_some() {}
            })
            .await;
            if accept_failed {
                tracing::error!("gateway server task ended after accept failure");
            }
        });
        Ok(Self {
            addr,
            task,
            shutdown,
            token,
        })
    }

    #[must_use]
    pub fn addr(&self) -> std::net::SocketAddr {
        self.addr
    }

    /// Hot-swaps the expected token without a restart (SEC2-02): the old
    /// token is revoked immediately for all subsequent requests.
    pub async fn update_token(&self, token: Vec<u8>) {
        assert!(!token.is_empty(), "token must be non-empty (SEC2-07)");
        *self.token.write().await = Arc::new(token);
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
            header_timeout_ms: 5_000,
            api: None,
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

    /// SEC2-02: regenerating the token must revoke the old one immediately,
    /// without a restart.
    #[tokio::test]
    async fn update_token_hot_swaps_without_restart() {
        let handle = GatewayHandle::start(config(0, 5_000, 8, 1 << 20))
            .await
            .unwrap();
        let addr = handle.addr();
        let old_auth = (
            "authorization",
            "Bearer 0123456789abcdefghijklmnopqrstuvwxyz0",
        );
        let ok = client()
            .get(url(addr, "/healthz"))
            .header(old_auth.0, old_auth.1)
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), reqwest::StatusCode::OK);

        handle
            .update_token(b"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz".to_vec())
            .await;

        let old_token = client()
            .get(url(addr, "/healthz"))
            .header(old_auth.0, old_auth.1)
            .send()
            .await
            .unwrap();
        assert_eq!(old_token.status(), reqwest::StatusCode::UNAUTHORIZED);
        let new_token = client()
            .get(url(addr, "/healthz"))
            .header("x-api-key", "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz")
            .send()
            .await
            .unwrap();
        assert_eq!(new_token.status(), reqwest::StatusCode::OK);
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// SEC2-07: an empty token must never start a gateway.
    #[tokio::test]
    async fn empty_token_is_rejected_fail_closed() {
        let error = GatewayHandle::start(GatewayConfig {
            token: Arc::new(Vec::new()),
            ..GatewayConfig::default()
        })
        .await
        .unwrap_err();
        assert!(matches!(error, GatewayError::Token(_)));
    }

    /// SEC2-03: a slowloris client that never finishes its headers is
    /// dropped within the header-read timeout.
    #[tokio::test]
    async fn slowloris_partial_headers_are_dropped() {
        let mut cfg = config(0, 60_000, 8, 1 << 20);
        cfg.header_timeout_ms = 500;
        let handle = GatewayHandle::start(cfg).await.unwrap();
        let addr = handle.addr();

        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        // Send half a request, then stall forever on the header.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        conn.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\n")
            .await
            .unwrap();
        let stall_started = std::time::Instant::now();
        let mut buf = [0u8; 64];
        let read = tokio::time::timeout(Duration::from_secs(5), conn.read(&mut buf)).await;
        match read {
            Err(_timeout) => panic!("slowloris connection was not dropped within 5s"),
            Ok(Ok(0)) => {} // closed by server ✓
            Ok(Ok(n)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                assert!(
                    text.contains("408") || text.contains("409") || n == 0,
                    "unexpected partial response: {text}"
                );
            }
            Ok(Err(_)) => {} // connection error = dropped ✓
        }
        assert!(stall_started.elapsed() < Duration::from_secs(5));
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// SEC2-05: a chunked body over the limit is rejected with 413 even
    /// without a content-length header.
    #[tokio::test]
    async fn chunked_oversized_body_rejected() {
        let handle = GatewayHandle::start(config(0, 5_000, 8, 1_024))
            .await
            .unwrap();
        let addr = handle.addr();
        let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let request = format!(
            "POST /test/echo HTTP/1.1\r\nHost: {addr}\r\nAuthorization: {AUTH}\r\nTransfer-Encoding: chunked\r\n\r\n"
        );
        conn.write_all(request.as_bytes()).await.unwrap();
        // Two chunks totalling far over the 1 KiB limit.
        for _ in 0..2 {
            let chunk = vec![b'x'; 2048];
            conn.write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                .await
                .unwrap();
            conn.write_all(&chunk).await.unwrap();
            conn.write_all(b"\r\n").await.unwrap();
        }
        conn.write_all(b"0\r\n\r\n").await.unwrap();
        let mut response = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), conn.read_to_end(&mut response)).await;
        let text = String::from_utf8_lossy(&response);
        assert!(text.contains("413"), "expected 413, got: {text}");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }
}
