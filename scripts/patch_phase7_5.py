import io

p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

# ---- imports + streaming clients ----
src = src.replace('''use sair_providers::openai::{ChatOutcome, send_chat, send_chat_with_headers};''',
'''use sair_providers::gemini::generate_content_streaming;
use sair_providers::anthropic::{count_tokens as anthropic_count_tokens_upstream, send_messages, send_messages_streaming};
use sair_providers::openai::{
    ChatOutcome, StreamStart, send_chat, send_chat_streaming_with_headers, send_chat_with_headers,
};''')
src = src.replace('''use sair_providers::{SsrfCheckedClient, SsrfPolicy, build_client};''',
'''use sair_providers::{SsrfCheckedClient, SsrfPolicy, build_client, build_streaming_client};''')
src = src.replace('''    /// Loopback-opt-in client, used only for providers that explicitly
    /// allow it (`providers.allow_loopback`).
    pub client_loopback: SsrfCheckedClient,''', '''    /// Loopback-opt-in client, used only for providers that explicitly
    /// allow it (`providers.allow_loopback`).
    pub client_loopback: SsrfCheckedClient,
    /// Phase 7 (F-1): streaming clients carry NO total deadline — only
    /// connect + idle read bounds — so streams may outlive `timeout_ms`.
    pub client_strict_streaming: SsrfCheckedClient,
    pub client_loopback_streaming: SsrfCheckedClient,''')
src = src.replace('''            client_strict: build_client(SsrfPolicy::default()),
            client_loopback: build_client(SsrfPolicy::allow_loopback()),
            deployment_concurrency: deployment_concurrency.max(1),''', '''            client_strict: build_client(SsrfPolicy::default()),
            client_loopback: build_client(SsrfPolicy::allow_loopback()),
            client_strict_streaming: build_streaming_client(SsrfPolicy::default()),
            client_loopback_streaming: build_streaming_client(SsrfPolicy::allow_loopback()),
            deployment_concurrency: deployment_concurrency.max(1),''')

# ---- validate_request: remove stream rejections (streaming now supported) ----
src = src.replace('''        Surface::OpenAiChat => {
            if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "streaming_not_supported",
                    "streaming is not implemented yet (planned Phase 7)".to_string(),
                ));
            }
            if parsed.get("model").and_then(Value::as_str).is_none() {''', '''        Surface::OpenAiChat => {
            if parsed.get("model").and_then(Value::as_str).is_none() {''')
src = src.replace('''        Surface::AnthropicMessages => {
            if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "streaming_not_supported",
                    "streaming is not implemented yet (planned Phase 7)".to_string(),
                ));
            }
            // F-8: count_tokens requires model + messages but NOT max_tokens.''', '''        Surface::AnthropicMessages => {
            // F-8: count_tokens requires model + messages but NOT max_tokens.''')

# ---- gemini wrapper: streamGenerateContent routes to streaming (remove rejection) ----
src = src.replace('''    if method != "generateContent" {
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
}''', '''    if method != "generateContent" && method != "streamGenerateContent" {
        return family_local_error(
            Surface::GeminiGenerateContent,
            StatusCode::NOT_FOUND,
            "unknown_method",
            &format!("unknown Gemini method `{method}`"),
            json!({}),
        );
    }
    let stream_requested = method == "streamGenerateContent";
    process_chat(
        api,
        Surface::GeminiGenerateContent,
        Some(alias.to_string()),
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}''')

# ---- process_chat signature + stream detection ----
src = src.replace('''async fn process_chat(
    api: Arc<ApiDeps>,
    surface: Surface,
    path_model: Option<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
    operation: Operation,
) -> Response {''', '''#[allow(clippy::too_many_arguments)]
async fn process_chat(
    api: Arc<ApiDeps>,
    surface: Surface,
    path_model: Option<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
    operation: Operation,
    stream_requested: bool,
) -> Response {''')
src = src.replace('''    process_chat(api, Surface::OpenAiChat, None, headers, body, Operation::Chat)
        .await
}''', '''    let stream_requested = parsed_stream_flag(&body);
    process_chat(
        api,
        Surface::OpenAiChat,
        None,
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}

/// Detects `stream: true` in the raw body (pre-parse optimization kept
/// honest: the parsed body is the authority inside process_chat).
fn parsed_stream_flag(body: &Bytes) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("stream").and_then(Value::as_bool))
        .unwrap_or(false)
}''')
src = src.replace('''    process_chat(
        api,
        Surface::AnthropicMessages,
        None,
        headers,
        body,
        Operation::Chat,
    )
    .await
}''', '''    let stream_requested = parsed_stream_flag(&body);
    process_chat(
        api,
        Surface::AnthropicMessages,
        None,
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}''')
src = src.replace('''    process_chat(
        api,
        Surface::GeminiGenerateContent,
        Some(alias.to_string()),
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}''', '''    process_chat(
        api,
        Surface::GeminiGenerateContent,
        Some(alias.to_string()),
        headers,
        body,
        Operation::Chat,
        stream_requested,
    )
    .await
}''')

# ---- decision_json carries streamed flag (F-5) ----
src = src.replace('''    let decision_json = serde_json::to_string(&decision).ok();
    let mut last_failure: Option<UpstreamFailure> = None;''', '''    let decision_json = Some(
        serde_json::json!({
            "decision": decision,
            "streamed": stream_requested,
        })
        .to_string(),
    );
    let mut last_failure: Option<UpstreamFailure> = None;''')

io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('api.rs streaming scaffolding done')
