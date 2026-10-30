// Phase 7 streaming tests: SSE fidelity, pre-commit rotation, no-retry-after-
// commit (hit counts), Gemini alt=sse, FR-021 permit hold. Per review F-4:
// mid-stream drop and drain-with-open-stream scenarios need a raw-TCP mock
// and are deferred to Phase 11 with an explicit traceability entry.

#[cfg(test)]
mod phase7_tests {
    use crate::api::ApiDeps;
    use crate::handle::{GatewayConfig, GatewayHandle};
    use sair_persistence::Db;
    use sair_vault::{InMemorySecretStore, SecretStore};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{MockServer, ResponseTemplate};

    const TOKEN: &str = "0123456789abcdefghijklmnopqrstuvwxyz0";

    async fn setup(upstream: &MockServer) -> GatewayHandle {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();

        let secret_ref = vault.store(b"sk-key-0".to_vec().as_slice()).unwrap();
        let provider_id = db
            .repos()
            .insert_provider("Primary", "openai_compatible", &upstream.uri(), true, None)
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key-0", secret_ref.as_str(), 0, 1)
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
        db.repos()
            .bind_deployment(&alias_id, &deployment_id, 10)
            .await
            .unwrap();

        let secret_ref = vault.store(b"sk-key-1".to_vec().as_slice()).unwrap();
        let provider_id = db
            .repos()
            .insert_provider("Fallback", "openai_compatible", &upstream.uri(), true, None)
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key-1", secret_ref.as_str(), 0, 1)
            .await
            .unwrap();
        let model_pk = db
            .repos()
            .insert_model(&provider_id, "model-1", "openai_chat", &json!({}), None)
            .await
            .unwrap();
        let deployment_id = db
            .repos()
            .insert_deployment(&provider_id, &model_pk, None)
            .await
            .unwrap();
        db.repos()
            .bind_deployment(&alias_id, &deployment_id, 9)
            .await
            .unwrap();

        let api = Arc::new(ApiDeps::new(db, vault, 4, Default::default()));
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

    async fn chat_stream(handle: &GatewayHandle, body: Value) -> reqwest::Response {
        client()
            .post(format!("http://{}/v1/chat/completions", handle.addr()))
            .header("authorization", format!("Bearer {TOKEN}"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    /// SSE frame fidelity: the relayed body matches the upstream bytes
    /// exactly (data frames + [DONE]); exactly one upstream hit (no
    /// failover after commitment).
    #[tokio::test]
    async fn sse_frames_are_relayed_byte_faithful() {
        let upstream = MockServer::start().await;
        let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\ndata: [DONE]\n\n";
        wiremock::Mock::given(method("POST"))
            .and(body_partial_json(json!({"model": "model-0"})))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(sse_body.as_bytes(), "text/event-stream"),
            )
            .expect(1)
            .mount(&upstream)
            .await;
        let handle = setup(&upstream).await;

        let response = chat_stream(
            &handle,
            json!({"model": "test-model", "stream": true, "messages": []}),
        )
        .await;
        let status = response.status();
        let ct = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response.text().await.unwrap_or_default();
        assert_eq!(status, 200, "body: {body}");
        assert!(
            ct.starts_with("text/event-stream"),
            "SSE content type must pass through, got: {ct}"
        );
        assert_eq!(body, sse_body, "frames must be byte-faithful");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Pre-commit rotation: primary 429s (error JSON, not SSE) → the
    /// fallback streams; the client receives the fallback's SSE.
    #[tokio::test]
    async fn pre_commit_failure_rotates_to_streaming_fallback() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(method("POST"))
            .and(body_partial_json(json!({"model": "model-0"})))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": {"message": "slow down"}
            })))
            .mount(&upstream)
            .await;
        wiremock::Mock::given(method("POST"))
            .and(body_partial_json(json!({"model": "model-1"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("data: [DONE]\n\n".as_bytes(), "text/event-stream"),
            )
            .mount(&upstream)
            .await;
        let handle = setup(&upstream).await;

        let response = chat_stream(
            &handle,
            json!({"model": "test-model", "stream": true, "messages": []}),
        )
        .await;
        assert_eq!(response.status(), 200);
        assert!(response.text().await.unwrap().contains("[DONE]"));
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// A streaming 4xx (error JSON, not SSE) classifies and rotates —
    /// commitment never happened.
    #[tokio::test]
    async fn streaming_error_json_rotates() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(method("POST"))
            .and(body_partial_json(json!({"model": "model-0"})))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "error": {"message": "slow down"}
            })))
            .mount(&upstream)
            .await;
        wiremock::Mock::given(method("POST"))
            .and(body_partial_json(json!({"model": "model-1"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("data: [DONE]\n\n".as_bytes(), "text/event-stream"),
            )
            .mount(&upstream)
            .await;
        let handle = setup(&upstream).await;
        let response = chat_stream(
            &handle,
            json!({"model": "test-model", "stream": true, "messages": []}),
        )
        .await;
        assert_eq!(response.status(), 200);
        // Consume the response so the pooled connection closes before drain.
        let _ = response.text().await.unwrap();
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Gemini `:streamGenerateContent` with `alt=sse` appended (F-3) and the
    /// SSE relayed.
    #[tokio::test]
    async fn gemini_stream_generate_relays_sse() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(method("POST"))
            .and(path("/v1beta/models/gemini-model:streamGenerateContent"))
            .and(query_param("alt", "sse"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "data: {\"candidates\":[]}\n\n".as_bytes(),
                "text/event-stream",
            ))
            .mount(&upstream)
            .await;

        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let alias_id = db.repos().insert_alias("gemini-pro", None).await.unwrap();
        let secret_ref = vault.store(b"gemini-key".to_vec().as_slice()).unwrap();
        let provider_id = db
            .repos()
            .insert_provider("Gemini", "gemini", &upstream.uri(), true, None)
            .await
            .unwrap();
        db.repos()
            .insert_credential(&provider_id, "key", secret_ref.as_str(), 0, 1)
            .await
            .unwrap();
        let model_pk = db
            .repos()
            .insert_model(
                &provider_id,
                "gemini-model",
                "gemini_generate_content",
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
        db.repos()
            .bind_deployment(&alias_id, &deployment_id, 10)
            .await
            .unwrap();
        let api = Arc::new(ApiDeps::new(db, vault, 4, Default::default()));
        let config = GatewayConfig {
            token: Arc::new(TOKEN.as_bytes().to_vec()),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();

        let response = client()
            .post(format!(
                "http://{}/v1beta/models/gemini-pro:streamGenerateContent",
                handle.addr()
            ))
            .header("authorization", format!("Bearer {TOKEN}"))
            .json(&json!({"contents": []}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(response.text().await.unwrap().contains("candidates"));
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// FR-021: a streaming request rotates past a deployment whose permit is
    /// held (concurrency 1; the test holds the permit).
    #[tokio::test]
    async fn streaming_rotates_past_busy_deployment() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(method("POST"))
            .and(body_partial_json(json!({"model": "model-0"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("data: [DONE]\n\n".as_bytes(), "text/event-stream"),
            )
            .mount(&upstream)
            .await;
        wiremock::Mock::given(method("POST"))
            .and(body_partial_json(json!({"model": "model-1"})))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                "data: {\"fallback\":true}\n\n".as_bytes(),
                "text/event-stream",
            ))
            .mount(&upstream)
            .await;

        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();
        let mut deployment_ids = Vec::new();
        for i in 0..2 {
            let secret_ref = vault.store(format!("sk-{i}").as_bytes()).unwrap();
            let provider_id = db
                .repos()
                .insert_provider(
                    &format!("P{i}"),
                    "openai_compatible",
                    &upstream.uri(),
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
                .insert_model(
                    &provider_id,
                    &format!("model-{i}"),
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
            db.repos()
                .bind_deployment(&alias_id, &deployment_id, 10 - i as i64)
                .await
                .unwrap();
            deployment_ids.push(deployment_id);
        }
        let api = Arc::new(ApiDeps::new(db, vault, 1, Default::default()));
        let config = GatewayConfig {
            token: Arc::new(TOKEN.as_bytes().to_vec()),
            port: 0,
            api: Some(api.clone()),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();

        // Hold the primary deployment's only permit from the test.
        let permit = api
            .semaphore_for(&deployment_ids[0])
            .try_acquire_owned()
            .unwrap();

        let response = chat_stream(
            &handle,
            json!({"model": "test-model", "stream": true, "messages": []}),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body = response.text().await.unwrap();
        assert!(
            body.contains("\"fallback\":true"),
            "streaming must rotate past the busy deployment: {body}"
        );
        drop(permit);
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }
}
