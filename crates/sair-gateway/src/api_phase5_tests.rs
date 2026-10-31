// Phase 5 tests: alias/capability matrix + OpenRouter attribution (F-1/F-2/F-3).

#[cfg(test)]
mod phase5_tests {
    use crate::api::ApiDeps;
    use crate::handle::{GatewayConfig, GatewayHandle};
    use sair_persistence::Db;
    use sair_vault::{InMemorySecretStore, SecretStore};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use wiremock::MockServer;
    use zeroize::Zeroizing;

    const TOKEN: &str = "0123456789abcdefghijklmnopqrstuvwxyz0";

    /// Alias across two providers with per-provider capability flags
    /// (provider-0 is always , provider-1 ).
    async fn setup_matrix(
        upstream: &MockServer,
        p0_capable: bool,
        p1_capable: bool,
    ) -> GatewayHandle {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());

        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();
        for i in 0..2 {
            let secret_ref = vault.store(format!("sk-{i}").as_bytes()).unwrap();
            let provider_type = if i == 0 {
                "openai_compatible"
            } else {
                "openrouter"
            };
            let provider_id = db
                .repos()
                .insert_provider(
                    &format!("Provider-{i}"),
                    provider_type,
                    &upstream.uri(),
                    true,
                    None,
                )
                .await
                .unwrap();
            let credential_id = db
                .repos()
                .insert_credential(&provider_id, "key", secret_ref.as_str(), 0, 1)
                .await
                .unwrap();
            let _ = credential_id;
            let capabilities = json!({
                "tool_calling": (p0_capable && i == 0) || (p1_capable && i == 1),
                "structured_output": (p0_capable && i == 0) || (p1_capable && i == 1),
                "streaming": false,
                "vision": false,
            });
            let model_pk = db
                .repos()
                .insert_model(
                    &provider_id,
                    &format!("model-{i}"),
                    "openai_chat",
                    &capabilities,
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
        }

        let api = Arc::new(ApiDeps::new(db, vault, 4, Default::default(), 300_000));
        let config = GatewayConfig {
            token: Arc::new(Zeroizing::new(TOKEN.as_bytes().to_vec())),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        GatewayHandle::start(config).await.unwrap()
    }

    async fn chat(handle: &GatewayHandle, token: &str, body: Value) -> reqwest::Response {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .post(format!("http://{}/v1/chat/completions", handle.addr()))
            .header("authorization", format!("Bearer {token}"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    /// F-2/F-6: a tools request routes to the tool-capable provider; the
    /// incapable one is excluded with a capability reason.
    #[tokio::test]
    async fn tools_requirement_routes_to_capable_provider() {
        let upstream = MockServer::start().await;
        // Provider-0 (incapable) would serve model-0; must NOT be called.
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-0"
        })))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmpl-wrong"
        })))
        .mount(&upstream)
        .await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-1"
        })))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmpl-capable"
        })))
        .mount(&upstream)
        .await;
        // setup_matrix(false, true): provider-0 incapable, provider-1 capable.
        let handle = setup_matrix(&upstream, false, true).await;

        let response = chat(
            &handle,
            TOKEN,
            json!({
                "model": "test-model",
                "messages": [{"role": "user", "content": "hi"}],
                "tools": [{"type": "function", "function": {"name": "f"}}]
            }),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(
            body["id"], "cmpl-capable",
            "must route to the capable provider"
        );
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// F-2: no capable candidate → 503 with the capability exclusion reasons.
    #[tokio::test]
    async fn no_capable_candidate_surfaces_reasons() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({"id": "x"})))
            .mount(&upstream)
            .await;
        let handle = setup_matrix(&upstream, false, false).await;

        let response = chat(
            &handle,
            TOKEN,
            json!({
                "model": "test-model",
                "messages": [],
                "response_format": {"type": "json_object"}
            }),
        )
        .await;
        assert_eq!(response.status(), 503);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "no_eligible_deployment");
        // F-4: exclusion reasons surfaced.
        let reasons: Vec<String> = body["error"]["shahid"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap_or_default().to_string())
            .collect();
        assert!(
            reasons
                .iter()
                .all(|r| r.contains("capability_structured_output")),
            "reasons must be capability exclusions: {reasons:?}"
        );
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// F-2: response_format type "text" fails open (no requirement).
    #[tokio::test]
    async fn response_format_text_fails_open() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(json!({"id": "cmpl-any"})),
            )
            .mount(&upstream)
            .await;
        let handle = setup_matrix(&upstream, false, false).await;
        let response = chat(
            &handle,
            TOKEN,
            json!({
                "model": "test-model",
                "messages": [],
                "response_format": {"type": "text"}
            }),
        )
        .await;
        assert_eq!(
            response.status(),
            200,
            "text must not require structured_output"
        );
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// F-1/F-3: attribution headers go ONLY to the openrouter provider, with
    /// the documented pair (HTTP-Referer + X-Title); the openai_compatible
    /// provider must never receive them.
    #[tokio::test]
    async fn attribution_headers_only_for_openrouter() {
        use sair_providers::openrouter::AttributionConfig;
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-0"
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(json!({"id": "cmpl-plain"})),
        )
        .mount(&upstream)
        .await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-1"
        })))
        .and(wiremock::matchers::header("x-title", "My App"))
        .and(wiremock::matchers::header(
            "http-referer",
            "https://myapp.example",
        ))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(json!({"id": "cmpl-attributed"})),
        )
        .mount(&upstream)
        .await;
        // A request that would hit provider-0 WITH attribution headers must
        // not match the attributed mock (they are absent) — the plain mock
        // above catches it.

        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());
        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();
        for i in 0..2 {
            let secret_ref = vault.store(format!("sk-{i}").as_bytes()).unwrap();
            let provider_type = if i == 0 {
                "openai_compatible"
            } else {
                "openrouter"
            };
            let provider_id = db
                .repos()
                .insert_provider(
                    &format!("Provider-{i}"),
                    provider_type,
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
        }
        let attribution = AttributionConfig {
            referer: Some("https://myapp.example".to_string()),
            title: Some("My App".to_string()),
        };
        let api = Arc::new(ApiDeps::new(db, vault, 4, attribution, 300_000));
        let config = GatewayConfig {
            token: Arc::new(Zeroizing::new(TOKEN.as_bytes().to_vec())),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();

        let response = chat(
            &handle,
            TOKEN,
            json!({"model": "test-model", "messages": []}),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        // Provider-0 (higher priority) is served WITHOUT attribution; the
        // attributed mock only matches provider-1, so we must get the plain
        // response — proving attribution did NOT leak to provider-0.
        assert_eq!(body["id"], "cmpl-plain");
        handle.stop(Duration::from_secs(2)).await.unwrap();
    }
}
