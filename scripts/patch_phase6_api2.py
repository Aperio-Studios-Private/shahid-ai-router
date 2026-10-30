import io

p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

# ---------- Piece A: signature + validation ----------
old_a = '''/// POST /v1/chat/completions (non-streaming).
async fn chat_completions(State(api): State<Arc<ApiDeps>>, body: Bytes) -> Response {
    let started = Instant::now();
    let correlation = correlation_id();

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return openai_error(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                &format!("request body is not valid JSON: {e}"),
                json!({}),
            );
        }
    };
    if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "streaming_not_supported",
            "streaming is not implemented yet (planned Phase 7)",
            json!({}),
        );
    }
    let Some(alias) = parsed.get("model").and_then(Value::as_str) else {
        return openai_error(
            StatusCode::BAD_REQUEST,
            "model_required",
            "the `model` field (a configured alias) is required",
            json!({}),
        );
    };
'''
new_a = '''/// POST /v1/chat/completions (non-streaming, Phase 3 surface).
async fn chat_completions(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    process_chat(api, Surface::OpenAiChat, None, headers, body, Operation::Chat)
        .await
}

/// POST /v1/messages (Phase 6, ADR-010): native Anthropic surface.
async fn anthropic_messages(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    process_chat(
        api,
        Surface::AnthropicMessages,
        None,
        headers,
        body,
        Operation::Chat,
    )
    .await
}

/// POST /v1/messages/count_tokens (Phase 6, F-8): proxied for anthropic
/// deployments; 501 for aliases without one (ADR-010).
async fn anthropic_count_tokens(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Response {
    process_chat(
        api,
        Surface::AnthropicMessages,
        None,
        headers,
        body,
        Operation::CountTokens,
    )
    .await
}

/// POST /v1beta/models/{model_and_method} (Phase 6, F-10): the segment is
/// `{alias}:generateContent` (decoded — the Path extractor decodes %3A).
async fn gemini_generate(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(model_and_method): axum::extract::Path<String>,
    body: Bytes,
) -> Response {
    let Some((alias, method)) = model_and_method.rsplit_once(':') else {
        return family_local_error(
            Surface::GeminiGenerateContent,
            StatusCode::NOT_FOUND,
            "unknown_method",
            "expected `{model}:generateContent`",
            json!({}),
        );
    };
    if method != "generateContent" {
        if method == "streamGenerateContent" {
            return family_local_error(
                Surface::GeminiGenerateContent,
                StatusCode::BAD_REQUEST,
                "streaming_not_supported",
                "streaming is not implemented yet (planned Phase 7)",
                json!({}),
            );
        }
        return family_local_error(
            Surface::GeminiGenerateContent,
            StatusCode::NOT_FOUND,
            "unknown_method",
            &format!("unknown Gemini method `{method}`"),
            json!({}),
        );
    }
    process_chat(
        api,
        Surface::GeminiGenerateContent,
        Some(alias.to_string()),
        headers,
        body,
        Operation::Chat,
    )
    .await
}

/// Per-family pre-validation (F-6): fails BEFORE routing so a malformed
/// request never burns an upstream attempt or marks a credential degraded.
fn validate_request(
    surface: Surface,
    parsed: &Value,
) -> Result<(), (StatusCode, &'static str, String)> {
    match surface {
        Surface::OpenAiChat => {
            if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "streaming_not_supported",
                    "streaming is not implemented yet (planned Phase 7)".to_string(),
                ));
            }
            if parsed.get("model").and_then(Value::as_str).is_none() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "model_required",
                    "the `model` field (a configured alias) is required".to_string(),
                ));
            }
        }
        Surface::AnthropicMessages => {
            if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "streaming_not_supported",
                    "streaming is not implemented yet (planned Phase 7)".to_string(),
                ));
            }
            if parsed.get("model").and_then(Value::as_str).is_none()
                || parsed.get("max_tokens").is_none()
                || parsed.get("messages").is_none()
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "anthropic requests require `model`, `max_tokens` and `messages`".to_string(),
                ));
            }
        }
        Surface::GeminiGenerateContent => {
            if parsed.get("contents").is_none() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "contents_required",
                    "gemini requests require `contents`".to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Chat,
    CountTokens,
}

/// Family-parameterized chat/count flow: validation, routing, rotation loop,
/// per-family envelopes (Phase 6 refactor; behavior preserved per family).
#[allow(clippy::too_many_lines)]
async fn process_chat(
    api: Arc<ApiDeps>,
    surface: Surface,
    path_model: Option<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
    operation: Operation,
) -> Response {
    let started = Instant::now();
    let correlation = correlation_id();

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return family_local_error(
                surface,
                StatusCode::BAD_REQUEST,
                "invalid_json",
                &format!("request body is not valid JSON: {e}"),
                json!({}),
            );
        }
    };
    if let Err((status, code, message)) = validate_request(surface, &parsed) {
        return family_local_error(surface, status, code, &message, json!({}));
    }
    // F-9: Gemini clients may send `?key=` — that form is rejected (the
    // token must never appear in URLs); the header form is the only auth.
    let _ = &headers;
'''
assert old_a in src
src = src.replace(old_a, new_a)
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('piece A done')
