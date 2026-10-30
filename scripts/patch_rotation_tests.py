import io, re

p = 'crates/sair-gateway/src/api_rotation_tests.rs'
src = io.open(p, encoding='utf-8').read()

old_setup = src[src.index('    /// Seeds a provider with'):src.index('    async fn chat(')]

new_setup = '''    /// Seeds `n` providers, each with one credential, one model and one
    /// deployment, all bound to alias `test-model` (pool semantics: a
    /// credential belongs to its provider).
    async fn setup_providers(upstream: &MockServer, n: usize) -> Fixture {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = Db::open("sqlite::memory:").await.unwrap();
        db.run_migrations().await.unwrap();
        let vault = Arc::new(InMemorySecretStore::new());

        let alias_id = db.repos().insert_alias("test-model", None).await.unwrap();
        let mut credential_ids = Vec::new();
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
                .insert_model(&provider_id, &format!("model-{i}"), "openai_chat", &json!({}), None)
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
        }

        let api = Arc::new(ApiDeps::new(db.clone(), vault, 4));
        let config = GatewayConfig {
            token: Arc::new(TOKEN.as_bytes().to_vec()),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();
        Fixture { handle, db, credential_ids }
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

        let api = Arc::new(ApiDeps::new(db.clone(), vault, 4));
        let config = GatewayConfig {
            token: Arc::new(TOKEN.as_bytes().to_vec()),
            port: 0,
            api: Some(api),
            ..GatewayConfig::default()
        };
        let handle = GatewayHandle::start(config).await.unwrap();
        Fixture { handle, db, credential_ids }
    }

'''
src = src.replace(old_setup, new_setup)

# Fix fixture uses
src = src.replace('let fixture = setup_multi(&upstream, 2, false).await;', 'let fixture = setup_providers(&upstream, 2).await;')
src = src.replace('let fixture = setup_multi(&upstream, 1, false).await;', 'let fixture = setup_providers(&upstream, 1).await;')
src = src.replace('let fixture = setup_multi(&upstream, 2, true).await;', 'let fixture = setup_grouped(&upstream, 2).await;')

# cooldown_of: per-provider lookups no longer valid (credentials across providers)
old_cooldown = src[src.index('    async fn cooldown_of'):src.index('    /// Scenario 1')]
new_cooldown = '''    async fn cooldown_of(fixture: &Fixture, credential_index: usize) -> Option<String> {
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

'''
src = src.replace(old_cooldown, new_cooldown)

# Scenario 5 comment fix (deployment busy across providers)
src = src.replace('''        // Slow primary (model-0) + fast fallback: with concurrency 1 on the
        // primary, the second parallel request must skip the busy deployment
        // and be served by the fallback.''', '''        // Slow primary provider (model-0) + fast fallback provider: with the
        // deployment concurrency bound reached on the primary, the second
        // parallel request must skip the busy deployment and be served by
        // the fallback.''')

io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('rotation tests restructured')
