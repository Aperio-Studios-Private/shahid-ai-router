// Phase 4 simulated failure suite (docs/11 gate; plan §5, R4-05 deterministic
// reshaping). Sibling module: shares nothing mutable with `tests`.

#[cfg(test)]
mod rotation_tests {
    use crate::api::ApiDeps;
    use crate::handle::{GatewayConfig, GatewayHandle};
    use sair_persistence::Db;
    use sair_vault::{InMemorySecretStore, SecretStore};
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use wiremock::MockServer;

    const TOKEN: &str = "0123456789abcdefghijklmnopqrstuvwxyz0";

    struct Fixture {
        handle: GatewayHandle,
        db: Db,
        credential_ids: Vec<String>,
        deployment_ids: Vec<String>,
        api: Arc<ApiDeps>,
    }

    /// Seeds `n` providers, each with one credential, one model and one
    /// deployment, all bound to alias `test-model` (pool semantics: a
    /// credential belongs to its provider).
    async fn setup_providers(upstream: &MockServer, n: usize) -> Fixture {
        setup_providers_with_concurrency(upstream, n, 4).await
    }

    async fn setup_providers_with_concurrency(
        upstream: &MockServer,
        n: usize,
        deployment_concurrency: usize,
    ) -> Fixture {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());

        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();
        let mut credential_ids = Vec::new();
        let mut deployment_ids = Vec::new();
        for i in 0..n {
            let secret = format!("sk-key-{i}");
            let secret_ref = vault.store(secret.as_bytes()).unwrap();
            let provider_id = db
                .repos()
                .insert_provider(
                    &format!("Provider-{i}"),
                    "openai_compatible",
                    &upstream.uri(),
                    true,
                    None,
                )
                .await
                .unwrap();
            let credential_id = db
                .repos()
                .insert_credential(&provider_id, &format!("key-{i}"), secret_ref.as_str(), 0, 1)
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
            credential_ids.push(credential_id);
            deployment_ids.push(deployment_id);
        }

        let api = Arc::new(ApiDeps::new(
            db.clone(),
            vault,
            deployment_concurrency,
            Default::default(),
        ));
        let config = GatewayConfig {
            token: Arc::new(TOKEN.as_bytes().to_vec()),
            port: 0,
            api: Some(api.clone()),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();
        Fixture {
            handle,
            db,
            credential_ids,
            deployment_ids,
            api,
        }
    }

    /// Seeds ONE provider with `n` credentials sharing an account group and
    /// one deployment (docs/05 Quota truth fixture).
    async fn setup_grouped(upstream: &MockServer, n: usize) -> Fixture {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());

        let provider_id = db
            .repos()
            .insert_provider("Mock", "openai_compatible", &upstream.uri(), true, None)
            .await
            .unwrap();
        let account_id = db
            .repos()
            .insert_provider_account(&provider_id, "shared-account", Some("org-1"))
            .await
            .unwrap();
        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();
        let mut credential_ids = Vec::new();
        for i in 0..n {
            let secret = format!("sk-key-{i}");
            let secret_ref = vault.store(secret.as_bytes()).unwrap();
            let credential_id = db
                .repos()
                .insert_credential(&provider_id, &format!("key-{i}"), secret_ref.as_str(), 0, 1)
                .await
                .unwrap();
            db.repos()
                .assign_credential_account(&credential_id, &account_id)
                .await
                .unwrap();
            credential_ids.push(credential_id);
        }
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

        let api = Arc::new(ApiDeps::new(db.clone(), vault, 4, Default::default()));
        let config = GatewayConfig {
            token: Arc::new(TOKEN.as_bytes().to_vec()),
            port: 0,
            api: Some(api.clone()),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();
        Fixture {
            handle,
            db,
            credential_ids,
            deployment_ids: Vec::new(),
            api,
        }
    }

    async fn chat(handle: &GatewayHandle, body: Value) -> reqwest::Response {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
            .post(format!("http://{}/v1/chat/completions", handle.addr()))
            .header("authorization", format!("Bearer {TOKEN}"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn cooldown_of(fixture: &Fixture, credential_index: usize) -> Option<String> {
        fixture
            .db
            .repos()
            .credential_health_summary()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c["id"].as_str() == Some(fixture.credential_ids[credential_index].as_str()))
            .and_then(|c| c["cooldown_until"].as_str().map(str::to_string))
    }

    /// Scenario 1 (plan §5): primary RateLimited+Retry-After → rotation
    /// serves from fallback; cooldown set on the primary only.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rotation_serves_fallback_and_cools_primary() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-0"
        })))
        .respond_with(wiremock::ResponseTemplate::new(429).set_body_json(json!({
            "error": {"type": "rate_limit_error", "message": "slow", "retry_after": 120}
        })))
        .mount(&upstream)
        .await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-1"
        })))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmpl-fallback", "usage": {"prompt_tokens": 1, "completion_tokens": 1}
        })))
        .mount(&upstream)
        .await;
        let fixture = setup_providers(&upstream, 2).await;

        let response = chat(
            &fixture.handle,
            json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
        assert_eq!(response.status(), 200, "rotation must reach the fallback");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["id"], "cmpl-fallback");

        // R4-04: Retry-After 120 s → primary cooled, fallback untouched.
        let primary = cooldown_of(&fixture, 0).await;
        assert!(primary.is_some(), "primary must be cooling");
        assert!(
            cooldown_of(&fixture, 1).await.is_none(),
            "fallback must stay eligible"
        );
        fixture.handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Scenario 2: 429 insufficient_quota with an account group cools BOTH
    /// credentials (docs/05 Quota truth — no key burning by rotation).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn billing_failure_cools_whole_account_group() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(429).set_body_json(json!({
                "error": {"type": "insufficient_quota", "message": "quota exhausted"}
            })))
            .mount(&upstream)
            .await;
        let fixture = setup_grouped(&upstream, 2).await;

        let response = chat(
            &fixture.handle,
            json!({"model": "test-model", "messages": [{"role": "user", "content": "hi"}]}),
        )
        .await;
        // F-1: upstream 429 preserved.
        assert_eq!(response.status(), 429);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["shahid"]["category"], "billing_or_quota");

        // Both group members must be cooling (rotation must not burn keys).
        assert!(
            cooldown_of(&fixture, 0).await.is_some(),
            "key 0 must be cooling"
        );
        assert!(
            cooldown_of(&fixture, 1).await.is_some(),
            "key 1 must be cooling"
        );
        fixture.handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Scenario 3 (R4-01 regression): health events are actually persisted
    /// with schema-legal event values and readable.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn health_events_are_persisted_and_readable() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(401).set_body_json(json!({
                "error": {"message": "bad key"}
            })))
            .mount(&upstream)
            .await;
        let fixture = setup_providers(&upstream, 1).await;
        let _ = chat(
            &fixture.handle,
            json!({"model": "test-model", "messages": []}),
        )
        .await;

        let events = fixture.db.repos().list_health_events(10).await.unwrap();
        assert!(
            !events.is_empty(),
            "R4-01: failure must leave a health event"
        );
        let event = events[0]["event"].as_str().unwrap();
        assert!(
            [
                "cooldown_start",
                "invalid",
                "quarantine",
                "degraded",
                "ok",
                "recovered"
            ]
            .contains(&event),
            "event must satisfy the schema CHECK, got: {event}"
        );
        assert_eq!(events[0]["category"], "invalid_credential");
        fixture.handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Scenario 4 (R4-02, deterministic two-phase): phase A asserts
    /// sequential rotation; phase B (parallel) asserts the fallback serves
    /// while the primary is cooling — cooldowns verified as existence, not
    /// exact hit counts.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_requests_do_not_storm_cooled_credentials() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-0"
        })))
        .respond_with(wiremock::ResponseTemplate::new(429).set_body_json(json!({
            "error": {"type": "rate_limit_error", "message": "slow", "retry_after": 300}
        })))
        .mount(&upstream)
        .await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-1"
        })))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmpl-ok", "usage": {}
        })))
        .mount(&upstream)
        .await;
        let fixture = setup_providers(&upstream, 2).await;

        // Phase A: one request cools the primary deterministically.
        let first = chat(
            &fixture.handle,
            json!({"model": "test-model", "messages": []}),
        )
        .await;
        assert_eq!(first.status(), 200);
        assert!(cooldown_of(&fixture, 0).await.is_some());

        // Phase B: 8 parallel requests — the primary is cooling, so every
        // served response must come from the fallback (no engine path can
        // re-attempt it: snapshot excludes cooling credentials).
        let mut handles = Vec::new();
        for _ in 0..8 {
            handles.push(tokio::spawn({
                let handle_addr = fixture.handle.addr();
                async move {
                    reqwest::Client::builder()
                        .timeout(Duration::from_secs(5))
                        .build()
                        .unwrap()
                        .post(format!("http://{handle_addr}/v1/chat/completions"))
                        .header("authorization", format!("Bearer {TOKEN}"))
                        .json(&json!({"model": "test-model", "messages": []}))
                        .send()
                        .await
                        .unwrap()
                }
            }));
        }
        // The fallback deployment has concurrency 4: up to 4 serve, the rest
        // get the bounded, observable 503 deployment_busy rejection (F-1 /
        // R4-06) — never a queue, never a primary hit.
        let mut served = 0;
        let mut bounded_rejected = 0;
        for handle in handles {
            let response = handle.await.unwrap();
            match response.status() {
                reqwest::StatusCode::OK => {
                    let body: Value = response.json().await.unwrap();
                    assert_eq!(
                        body["id"], "cmpl-ok",
                        "no request may hit the cooled primary"
                    );
                    served += 1;
                }
                reqwest::StatusCode::SERVICE_UNAVAILABLE => {
                    let body: Value = response.json().await.unwrap();
                    assert_eq!(body["error"]["code"], "deployment_busy");
                    bounded_rejected += 1;
                }
                other => panic!("unexpected status {other}"),
            }
        }
        assert!(served >= 1, "fallback must serve");
        assert_eq!(served + bounded_rejected, 8);
        fixture.handle.stop(Duration::from_secs(2)).await.unwrap();
    }

    /// Scenario 5 (FR-021, R4-06): deterministic — the TEST holds the
    /// primary deployment's only permit, so the request must rotate past the
    /// busy deployment to the fallback.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deployment_busy_rotates_to_next_candidate() {
        let upstream = MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-0"
        })))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmpl-slow", "usage": {}
        })))
        .mount(&upstream)
        .await;
        wiremock::Mock::given(wiremock::matchers::body_partial_json(json!({
            "model": "model-1"
        })))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmpl-fast", "usage": {}
        })))
        .mount(&upstream)
        .await;
        let fixture = setup_providers_with_concurrency(&upstream, 2, 1).await;

        // The test itself occupies the primary deployment's single slot —
        // no timing race.
        let permit = fixture
            .api
            .semaphore_for(&fixture.deployment_ids[0])
            .try_acquire_owned()
            .expect("permit must be free before the request");

        let response = chat(
            &fixture.handle,
            json!({"model": "test-model", "messages": []}),
        )
        .await;
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["id"], "cmpl-fast", "busy primary must rotate past");
        drop(permit);
        fixture.handle.stop(Duration::from_secs(3)).await.unwrap();
    }
}
