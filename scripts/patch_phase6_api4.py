import io

p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

# ---------- Piece C: upstream dispatch per surface ----------
old_c = '''        // F-1: attribution headers are OpenRouter-only, per candidate
        // dispatch. Never leaked to other providers.
        let send_result = if candidate.provider_type == "openrouter" {
            let extra = api.openrouter_attribution.headers();
            send_chat_with_headers(
                client,
                &base,
                &candidate.model_id,
                &secret,
                parsed.clone(),
                Duration::from_millis(policy.timeout_ms.max(1) as u64),
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
                Duration::from_millis(policy.timeout_ms.max(1) as u64),
            )
            .await
        };
        match send_result {'''
new_c = '''        let timeout = Duration::from_millis(policy.timeout_ms.max(1) as u64);
        // Phase 6: per-surface adapter dispatch. OpenRouter attribution
        // (F-1, Phase 5) applies only on the OpenAI surface to openrouter
        // providers — never leaked elsewhere.
        let send_result = match (surface, operation) {
            (Surface::OpenAiChat, Operation::Chat) => {
                if candidate.provider_type == "openrouter" {
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
                }
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
            }
            (Surface::GeminiGenerateContent, _) => {
                sair_providers::gemini::generate_content(
                    client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    parsed.clone(),
                    timeout,
                )
                .await
            }
        };
        match send_result {'''
assert old_c in src
src = src.replace(old_c, new_c)

# ---------- Piece D: terminal returns use family envelopes ----------
src = src.replace('''                        return upstream_failure_response(
                            &failure,
                            alias,
                            decision.exclusions.len(),
                        );''', '''                        return family_error_response(
                            surface,
                            &failure,
                            alias,
                            decision.exclusions.len(),
                        );''')
src = src.replace('''    // Rotation budget exhausted or rotation set exhausted.
    if let Some(failure) = last_failure {
        record_failure_minimal(&api, &correlation, alias, &failure, &decision_json).await;
        return upstream_failure_response(&failure, alias, decision.exclusions.len());
    }
    // R4-06: distinguish terminal states (actual upstream attempts, not
    // reserved credentials).
    if busy_skips > 0 && actual_attempts == 0 {
        return openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "deployment_busy",
            "every eligible deployment is at its concurrency bound",
            json!({ "alias": alias, "busy_deployments": busy_skips }),
        );
    }
    openai_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "no_attempt_made",
        "routing produced no executable candidate",
        json!({
            "alias": alias,
            "exclusions": decision.exclusions.len(),
            "skipped_duplicates": skipped_duplicates,
            "busy_deployments": busy_skips,
        }),
    )
}''', '''    // Rotation budget exhausted or rotation set exhausted.
    if let Some(failure) = last_failure {
        record_failure_minimal(&api, &correlation, alias, &failure, &decision_json).await;
        return family_error_response(surface, &failure, alias, decision.exclusions.len());
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
    )
}''')

# ---------- Piece E: record signatures carry family + operation ----------
src = src.replace('''async fn record(
    api: &ApiDeps,
    correlation: &str,
    alias: &str,
    candidate: &sair_domain::routing::RouteCandidate,
    status: Option<u16>,
    error_category: Option<&str>,''', '''async fn record(
    api: &ApiDeps,
    surface: Surface,
    operation: Operation,
    correlation: &str,
    alias: &str,
    candidate: &sair_domain::routing::RouteCandidate,
    status: Option<u16>,
    error_category: Option<&str>,''')
src = src.replace('''    let record = RequestRecordInsert {
        id: correlation.to_string(),
        ts: sair_persistence::time::now_iso(),
        correlation_id: correlation.to_string(),
        protocol_family: Some("openai_chat".to_string()),
        operation: Some("chat_completions".to_string()),''', '''    let record = RequestRecordInsert {
        id: correlation.to_string(),
        ts: sair_persistence::time::now_iso(),
        correlation_id: correlation.to_string(),
        protocol_family: Some(surface.label().to_string()),
        operation: Some(match operation {
            Operation::Chat => "chat",
            Operation::CountTokens => "count_tokens",
        }
        .to_string()),''')
src = src.replace('''async fn record_failure_minimal(
    api: &ApiDeps,
    correlation: &str,
    alias: &str,
    failure: &UpstreamFailure,
    decision_json: &Option<String>,
) {
    let record = RequestRecordInsert {
        id: correlation.to_string(),
        ts: sair_persistence::time::now_iso(),
        correlation_id: correlation.to_string(),
        protocol_family: Some("openai_chat".to_string()),
        operation: Some("chat_completions".to_string()),''', '''async fn record_failure_minimal(
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
        operation: Some(match operation {
            Operation::Chat => "chat",
            Operation::CountTokens => "count_tokens",
        }
        .to_string()),''')
# update record() call sites inside process_chat (3 sites)
src = src.replace('''                record(
                    &api,
                    &correlation,
                    alias,
                    candidate,
                    Some(status),''', '''                record(
                    &api,
                    surface,
                    operation,
                    &correlation,
                    alias,
                    candidate,
                    Some(status),''')
src = src.replace('''                    FailureAction::Stop => {
                        record(
                            &api,
                            &correlation,
                            alias,
                            candidate,''', '''                    FailureAction::Stop => {
                        record(
                            &api,
                            surface,
                            operation,
                            &correlation,
                            alias,
                            candidate,''')
src = src.replace('''    if let Some(failure) = last_failure {
        record_failure_minimal(&api, &correlation, alias, &failure, &decision_json).await;''', '''    if let Some(failure) = last_failure {
        record_failure_minimal(&api, surface, operation, &correlation, alias, &failure, &decision_json)
            .await;''')

# ---------- routes ----------
src = src.replace('''    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(list_models))
        .with_state(api)''', '''    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/messages", post(anthropic_messages))
        .route("/v1/messages/count_tokens", post(anthropic_count_tokens))
        .route("/v1beta/models/{model_and_method}", post(gemini_generate))
        .route("/v1/models", get(list_models))
        .with_state(api)''')

# ---------- imports ----------
src = src.replace('''use sair_providers::openai::{ChatOutcome, send_chat, send_chat_with_headers};''', '''use sair_providers::openai::{ChatOutcome, send_chat, send_chat_with_headers};''')

io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('pieces C/D/E + routes done')
