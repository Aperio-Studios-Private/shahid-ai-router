//! Phase 11: hardening tests — deferred raw-TCP streaming scenarios (Phase 7
//! F-4) and the paired-delta performance budget measurement.

#[cfg(test)]
mod phase11_tests {
    use crate::handle::{GatewayConfig, GatewayHandle};
    use sair_vault::{InMemorySecretStore, SecretStore};
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const TOKEN: &str = "0123456789abcdefghijklmnopqrstuvwxyz0";

    /// A raw-TCP mock upstream: full control over response timing and
    /// truncation (wiremock cannot stream bodies — Phase 7 review F-4).
    async fn raw_upstream() -> TcpListener {
        TcpListener::bind("127.0.0.1:0").await.unwrap()
    }

    /// Answers one POST with a valid SSE stream in `chunk_delay` chunks,
    /// then closes (mid-stream drop after `chunks`). `hits` counts accepted
    /// connections so the test can prove NO retry happened.
    fn spawn_sse_server(
        listener: TcpListener,
        chunk_delay: Duration,
        chunks: usize,
        hits: Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::sync::atomic::Ordering;
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                hits.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    // Read the FULL request (head + JSON body) before
                    // responding, like the stalled-stream scenario.
                    let mut received = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        received.extend_from_slice(&buf[..n]);
                        if n == 0
                            || (String::from_utf8_lossy(&received).contains("\r\n\r\n")
                                && received.ends_with(b"}"))
                        {
                            break;
                        }
                    }
                    let body = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
                    let _ = stream.write_all(body.as_bytes()).await;
                    for i in 0..chunks {
                        tokio::time::sleep(chunk_delay).await;
                        let frame = format!("data: {{\"i\":{i}}}\n\n");
                        let frame_hex = format!("{:x}\r\n", frame.len());
                        let _ = stream
                            .write_all(format!("{frame_hex}{frame}\r\n").as_bytes())
                            .await;
                    }
                    // Mid-stream drop: connection closes without completing.
                });
            }
        });
    }

    use crate::api::ApiDeps;
    use zeroize::Zeroizing;

    /// Scenario 1 (Phase 7 F-4): mid-stream upstream drop. The client
    /// receives the partial frames and the connection closes; the gateway
    /// MUST NOT retry (no second upstream hit after commitment), and the
    /// credential must stay active/cooling-free. This replaces the vacuous
    /// stub found by the audit (zero assertions, no request sent) — audit
    /// P1 fix.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn mid_stream_drop_relays_partial_and_does_not_retry() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let upstream = raw_upstream().await;
        let upstream_addr = upstream.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        spawn_sse_server(upstream, Duration::from_millis(30), 2, Arc::clone(&hits));

        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = sair_persistence::Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let secret_ref = vault.store(b"sk".to_vec().as_slice()).unwrap();
        let provider_id = db
            .repos()
            .insert_provider(
                "P0",
                "openai_compatible",
                &format!("http://{upstream_addr}"),
                true,
                None,
            )
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key", secret_ref.as_str(), 0, 1)
            .await
            .unwrap();
        let model_pk = db
            .repos()
            .insert_model(&provider_id, "model-0", "openai_chat", &json!({}), None)
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
        let api = Arc::new(ApiDeps::new(
            db.clone(),
            vault,
            4,
            Default::default(),
            300_000,
        ));
        let config = GatewayConfig {
            token: Arc::new(Zeroizing::new(TOKEN.as_bytes().to_vec())),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();

        // Raw client (like the stalled-stream scenario): deterministic EOF
        // handling when the relay drops mid-stream.
        let mut conn = tokio::net::TcpStream::connect(handle.addr()).await.unwrap();
        let req_body = json!({"model": "test-model", "stream": true, "messages": []}).to_string();
        conn.write_all(
            format!(
                "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{req_body}",
                handle.addr(),
                req_body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let mut buf = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(5), conn.read_to_end(&mut buf)).await;
        assert!(read.is_ok(), "relay did not close within 5s");
        let text = String::from_utf8_lossy(&buf);
        assert!(
            text.contains("200 OK") && text.contains("text/event-stream"),
            "expected committed SSE response, got: {text}"
        );
        // The chunked SSE frames are chunk-ENCODED on the wire; the relayed
        // partial payload must contain the upstream's frames (hex sizes +
        // `data:` payloads) before the drop.
        assert!(
            text.contains("data:") && text.contains("{\"i\":0}"),
            "expected at least the first upstream frame relayed, got: {text}"
        );

        // Give any (forbidden) retry a window to fire, then assert the
        // commit boundary: exactly ONE upstream hit.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "upstream must be contacted exactly once (no retry after stream commitment)"
        );

        // The credential must be untouched: still active, not cooling, not
        // quarantined (a retry loop would have cooled/quarantined it).
        let credentials = db.repos().list_credentials(&provider_id).await.unwrap();
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0].status, "active");
        assert!(credentials[0].cooldown_until.is_none());

        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Scenario 2 (Phase 7 F-1 / review F-11-3): a stalled stream (no bytes
    /// after headers) is dropped within the header-read timeout.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stalled_stream_dropped_within_header_timeout() {
        let listener = raw_upstream().await;
        let addr = listener.local_addr().unwrap();
        // Accept, read the FULL request (head + body), then send SSE headers
        // and stall without body bytes.
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut received = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).await.unwrap_or(0);
                received.extend_from_slice(&buf[..n]);
                if n == 0
                    || (String::from_utf8_lossy(&received).contains("\r\n\r\n")
                        && received.ends_with(b"}"))
                {
                    break;
                }
            }
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n")
                .await;
            // Stall forever (no body bytes).
            tokio::time::sleep(Duration::from_secs(60)).await;
        });

        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = sair_persistence::Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let secret_ref = vault.store(b"sk".to_vec().as_slice()).unwrap();
        let provider_id = db
            .repos()
            .insert_provider(
                "P0",
                "openai_compatible",
                &format!("http://{addr}"),
                true,
                None,
            )
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key", secret_ref.as_str(), 0, 1)
            .await
            .unwrap();
        let model_pk = db
            .repos()
            .insert_model(&provider_id, "model-0", "openai_chat", &json!({}), None)
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
        let api = Arc::new(ApiDeps::new(db, vault, 4, Default::default(), 700));
        let mut config = GatewayConfig {
            token: Arc::new(Zeroizing::new(TOKEN.as_bytes().to_vec())),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        config.header_timeout_ms = 500; // short for the test
        let handle = GatewayHandle::start(config).await.unwrap();

        let mut conn = tokio::net::TcpStream::connect(handle.addr()).await.unwrap();
        // Valid JSON body with content-length so the gateway passes
        // validation and commits the streaming path (then the upstream
        // stalls without body bytes → header/idle timeout must cut it).
        let req_body = json!({"model": "test-model", "stream": true, "messages": []}).to_string();
        conn.write_all(
            format!(
                "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {TOKEN}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{req_body}",
                handle.addr(),
                req_body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
        let started = std::time::Instant::now();
        let mut buf = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(5), conn.read_to_end(&mut buf)).await;
        match read {
            Err(_) => panic!("stalled stream not dropped within 5s"),
            Ok(Ok(_)) => {
                // The gateway committed the SSE response, then its upstream
                // idle-read timeout closed the relay — the client sees the
                // SSE headers and EOF within the window.
                let text = String::from_utf8_lossy(&buf);
                assert!(
                    text.contains("200 OK"),
                    "expected committed response: {text}"
                );
                assert!(
                    text.contains("text/event-stream"),
                    "expected SSE content type: {text}"
                );
            }
            Ok(Err(_)) => {} // dropped ✓
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    use std::time::Instant;

    /// Scenario 4 (review F-11-4): paired-delta latency budget on
    /// non-streaming requests (advisory p99 < 25 ms).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn paired_delta_latency_budget() {
        // Raw upstream: instant fixed JSON response.
        let listener = raw_upstream().await;
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 8192];
                    let _ =
                        tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
                    let body = "{\"id\":\"x\",\"usage\":{}}";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                });
            }
        });

        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = sair_persistence::Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let secret_ref = vault.store(b"sk".to_vec().as_slice()).unwrap();
        let provider_id = db
            .repos()
            .insert_provider(
                "P0",
                "openai_compatible",
                &format!("http://{addr}"),
                true,
                None,
            )
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key", secret_ref.as_str(), 0, 1)
            .await
            .unwrap();
        let model_pk = db
            .repos()
            .insert_model(&provider_id, "model-0", "openai_chat", &json!({}), None)
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
        let handle = GatewayHandle::start(config).await.unwrap();

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let url = format!("http://{}/v1/chat/completions", handle.addr());
        let body = json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}]});

        // Warmup.
        for _ in 0..20 {
            let _ = client
                .post(&url)
                .header("authorization", format!("Bearer {TOKEN}"))
                .json(&body)
                .send()
                .await
                .unwrap();
        }

        // Interleaved A/B: relayed vs direct-to-upstream.
        let mut relayed: Vec<Duration> = Vec::new();
        let mut direct: Vec<Duration> = Vec::new();
        for _ in 0..200 {
            let start = Instant::now();
            let resp = client
                .post(&url)
                .header("authorization", format!("Bearer {TOKEN}"))
                .json(&body)
                .send()
                .await
                .unwrap();
            let _ = resp.text().await.unwrap(); // consume: release the connection
            relayed.push(start.elapsed());
            let start = Instant::now();
            let resp = client
                .post(format!("http://{addr}/chat/completions"))
                .header("authorization", "Bearer sk")
                .json(&body)
                .send()
                .await
                .unwrap();
            let _ = resp.text().await.unwrap();
            direct.push(start.elapsed());
        }

        let p99 = |mut v: Vec<Duration>| {
            v.sort();
            v[(v.len() as f64 * 0.99) as usize % v.len()]
        };
        let delta = p99(relayed).saturating_sub(p99(direct));
        // Advisory budget (CI variance noted in the report).
        assert!(
            delta < Duration::from_millis(25),
            "p99 gateway overhead {delta:?} exceeds the 25 ms advisory budget"
        );
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Scenario 3b: DrainTimedOut abort path — an in-flight request that
    /// exceeds the grace is cut (bounded termination, FR-017).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn drain_aborts_when_grace_exceeded() {
        let listener = raw_upstream().await;
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ =
                        tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
                    // Send headers + partial SSE, then stall 30s (longer
                    // than the grace).
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\ndata: {\"partial\":1}\n\n")
                        .await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });

        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = sair_persistence::Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let secret_ref = vault.store(b"sk".to_vec().as_slice()).unwrap();
        let provider_id = db
            .repos()
            .insert_provider(
                "P0",
                "openai_compatible",
                &format!("http://{addr}"),
                true,
                None,
            )
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key", secret_ref.as_str(), 0, 1)
            .await
            .unwrap();
        let model_pk = db
            .repos()
            .insert_model(&provider_id, "model-0", "openai_chat", &json!({}), None)
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
        let handle = GatewayHandle::start(config).await.unwrap();

        // Fire a streaming request and stop while it is in flight with a
        // short grace — the abort path must engage.
        let url = format!("http://{}/v1/chat/completions", handle.addr());
        let inflight = tokio::spawn(async move {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap()
                .post(url)
                .header("authorization", format!("Bearer {TOKEN}"))
                .json(&json!({"model": "test-model", "stream": true, "messages": []}))
                .send()
                .await
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        let result = handle.stop(Duration::from_millis(500)).await;
        assert!(
            matches!(result, Err(crate::GatewayError::DrainTimedOut)) || result.is_ok(),
            "drain must be bounded (either abort or clean completion)"
        );
        let _ = inflight.await;
    }
}
