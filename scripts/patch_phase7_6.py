import io

p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

# ---- upstream dispatch: streaming branches ----
old = '''        let timeout = Duration::from_millis(policy.timeout_ms.max(1) as u64);
        // Phase 6: per-surface adapter dispatch. OpenRouter attribution
        // (F-1, Phase 5) applies only on the OpenAI surface to openrouter
        // providers — never leaked elsewhere.
        let send_result = match (surface, operation) {'''
new = '''        let timeout = Duration::from_millis(policy.timeout_ms.max(1) as u64);
        // Phase 6: per-surface adapter dispatch. OpenRouter attribution
        // (F-1, Phase 5) applies only on the OpenAI surface to openrouter
        // providers — never leaked elsewhere.
        // Phase 7 (F-1): streaming requests use the dedicated streaming
        // clients (no total deadline) and the F-2 commit predicate.
        let streaming_client = if candidate.allow_loopback {
            &api.client_loopback_streaming
        } else {
            &api.client_strict_streaming
        };
        let send_result = match (surface, operation) {'''
assert old in src
src = src.replace(old, new)

# add streaming arms to the match (before the defensive count arm)
old_arms = '''            // count_tokens is defined only for the Anthropic surface
            // (ADR-010); the count handler routes only AnthropicMessages,
            // so this arm is defensive.
            (Surface::OpenAiChat, Operation::CountTokens)
            | (Surface::GeminiGenerateContent, Operation::CountTokens) => {
                Err(UpstreamFailure::internal(
                    "count_tokens is only supported on the anthropic surface",
                ))
            }'''
new_arms = '''            (Surface::OpenAiChat, Operation::Chat) if stream_requested => {
                let extra = if candidate.provider_type == "openrouter" {
                    api.openrouter_attribution.headers()
                } else {
                    Vec::new()
                };
                send_chat_streaming_with_headers(
                    streaming_client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    parsed.clone(),
                    &extra,
                )
                .await
            }
            (Surface::AnthropicMessages, Operation::Chat) if stream_requested => {
                let version = headers
                    .get("anthropic-version")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or(sair_providers::anthropic::DEFAULT_ANTHROPIC_VERSION)
                    .to_string();
                sair_providers::anthropic::send_messages_streaming(
                    streaming_client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    &version,
                    parsed.clone(),
                )
                .await
            }
            (Surface::GeminiGenerateContent, Operation::Chat) if stream_requested => {
                generate_content_streaming(
                    streaming_client,
                    &base,
                    &candidate.model_id,
                    &secret,
                    parsed.clone(),
                )
                .await
            }
            // count_tokens is defined only for the Anthropic surface
            // (ADR-010); the count handler routes only AnthropicMessages,
            // so this arm is defensive.
            (Surface::OpenAiChat, Operation::CountTokens)
            | (Surface::GeminiGenerateContent, Operation::CountTokens) => {
                Err(UpstreamFailure::internal(
                    "count_tokens is only supported on the anthropic surface",
                ))
            }'''
assert old_arms in src
src = src.replace(old_arms, new_arms)

# ---- Ok branch: handle UpstreamCall variants ----
old_ok = '''        match send_result {
            Ok(ChatOutcome {
                status,
                body,
                tokens_in,
                tokens_out,
            }) => {
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    surface,
                    operation,
                    &correlation,
                    alias,
                    candidate,
                    Some(status),
                    None,
                    latency,
                    tokens_in,
                    tokens_out,
                    &decision_json,
                )
                .await;
                // Release the deployment slot before responding (FR-021).
                drop(deployment_permit);
                return (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    Json(body),
                )
                    .into_response();
            }'''
new_ok = '''        match send_result {
            Ok(UpstreamCall::Chat(ChatOutcome {
                status,
                body,
                tokens_in,
                tokens_out,
            })) => {
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    surface,
                    operation,
                    &correlation,
                    alias,
                    candidate,
                    Some(status),
                    None,
                    latency,
                    tokens_in,
                    tokens_out,
                    &decision_json,
                )
                .await;
                // Release the deployment slot before responding (FR-021).
                drop(deployment_permit);
                return (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    Json(body),
                )
                    .into_response();
            }
            Ok(UpstreamCall::CommittedStream(outcome)) => {
                // FR-010/docs/05: commitment point. From here the loop is
                // exited by construction — no failover after bytes flow.
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    surface,
                    operation,
                    &correlation,
                    alias,
                    candidate,
                    Some(outcome.status),
                    None,
                    latency,
                    None,
                    None,
                    &decision_json,
                )
                .await;
                let content_type = outcome
                    .content_type
                    .clone()
                    .unwrap_or_else(|| "text/event-stream".to_string());
                // FR-021: the permit moves INTO the response stream and is
                // released when the relayed stream ends (F-6 normative).
                let body = axum::body::Body::from_stream(
                    PermitStream::new(outcome.byte_stream, deployment_permit),
                );
                return (
                    StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::OK),
                    [(axum::http::header::CONTENT_TYPE, content_type)],
                    body,
                )
                    .into_response();
            }
            Ok(UpstreamCall::NonStreamed { status, body }) => {
                // F-2: 2xx non-SSE → mapped through the family usage shape
                // and returned as a normal (non-streamed) passthrough.
                let (tokens_in, tokens_out) = extract_usage(surface, &body);
                let latency = started.elapsed().as_millis() as i64;
                let _ = api
                    .db
                    .repos()
                    .touch_credential_used(&candidate.credential_id)
                    .await;
                record(
                    &api,
                    surface,
                    operation,
                    &correlation,
                    alias,
                    candidate,
                    Some(status),
                    None,
                    latency,
                    tokens_in,
                    tokens_out,
                    &decision_json,
                )
                .await;
                drop(deployment_permit);
                return (
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    Json(body),
                )
                    .into_response();
            }'''
assert old_ok in src
src = src.replace(old_ok, new_ok)
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('loop dispatch patched')
