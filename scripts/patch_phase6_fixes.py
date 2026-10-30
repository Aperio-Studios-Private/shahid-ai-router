import io

# 1. validate_request: operation-aware (F-8)
p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace(
    "fn validate_request(\n    surface: Surface,\n    parsed: &Value,\n) -> Result<(), (StatusCode, &'static str, String)> {",
    "fn validate_request(\n    surface: Surface,\n    operation: Operation,\n    parsed: &Value,\n) -> Result<(), (StatusCode, &'static str, String)> {")

old_anthropic = """        Surface::AnthropicMessages => {
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
        }"""
new_anthropic = """        Surface::AnthropicMessages => {
            if parsed.get("stream").and_then(Value::as_bool) == Some(true) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "streaming_not_supported",
                    "streaming is not implemented yet (planned Phase 7)".to_string(),
                ));
            }
            // F-8: count_tokens requires model + messages but NOT max_tokens.
            let needs_max_tokens = operation == Operation::Chat;
            if parsed.get("model").and_then(Value::as_str).is_none()
                || parsed.get("messages").is_none()
                || (needs_max_tokens && parsed.get("max_tokens").is_none())
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "anthropic requests require `model` and `messages` (chat also requires `max_tokens`)"
                        .to_string(),
                ));
            }
        }"""
assert old_anthropic in src, "anthropic validation block not found"
src = src.replace(old_anthropic, new_anthropic)
src = src.replace(
    "    if let Err((status, code, message)) = validate_request(surface, &parsed) {",
    "    if let Err((status, code, message)) = validate_request(surface, operation, &parsed) {")

# 2. ?key= rejection (F-9)
old_gem = """async fn gemini_generate(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(model_and_method): axum::extract::Path<String>,
    body: Bytes,
) -> Response {
    let Some((alias, method)) = model_and_method.rsplit_once(':') else {"""
new_gem = """async fn gemini_generate(
    State(api): State<Arc<ApiDeps>>,
    headers: axum::http::HeaderMap,
    axum::extract::Path(model_and_method): axum::extract::Path<String>,
    query: axum::extract::RawQuery,
    body: Bytes,
) -> Response {
    // F-9: the `?key=` auth form is rejected (tokens must never appear in
    // URLs); the x-goog-api-key header is the only accepted form.
    if query
        .0
        .as_deref()
        .is_some_and(|q| q.split('&').any(|pair| pair.starts_with("key=")))
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "code": 401,
                    "message": "query-parameter authentication is rejected; use the x-goog-api-key header",
                    "status": "UNAUTHENTICATED",
                    "shahid": { "code": "query_param_auth_rejected" }
                }
            })),
        )
            .into_response();
    }
    let Some((alias, method)) = model_and_method.rsplit_once(':') else {"""
assert old_gem in src, "gemini_generate block not found"
src = src.replace(old_gem, new_gem)
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('api.rs fixed')

# 3. phase6 tests: gemini provider in setup() + alias fixes
p = 'crates/sair-gateway/src/api_phase6_tests.rs'
src = io.open(p, encoding='utf-8').read()

gem_block = """        let secret_ref = vault.store(b"sk-gemini-key".to_vec().as_slice()).unwrap();
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
            .insert_model(&provider_id, "gemini-model", "gemini_generate_content", &json!({}), None)
            .await
            .unwrap();
        let deployment_id = db
            .repos()
            .insert_deployment(&provider_id, &model_pk, None)
            .await
            .unwrap();
        let alias_gemini = db.repos().insert_alias("gemini-pro", None).await.unwrap();
        db.repos()
            .bind_deployment(&alias_gemini, &deployment_id, 10)
            .await
            .unwrap();

        let secret_ref = vault.store(b"sk-openai-key".to_vec().as_slice()).unwrap();"""
assert 'sk-openai-key' in src
src = src.replace(
    '        let secret_ref = vault.store(b"sk-openai-key".to_vec().as_slice()).unwrap();',
    gem_block,
    1,
)

# gemini happy path: drop the bespoke fixture, use shared setup
start = src.index("        // The Gemini fixture needs a gemini-family deployment")
end = src.index("        let response = post(&handle, \"/v1beta/models/gemini-pro:generateContent\")")
src = src[:start] + src[end:]

src = src.replace(
    "/v1beta/models/claude:generateContent?key=leaked-token",
    "/v1beta/models/gemini-pro:generateContent?key=leaked-token",
)
src = src.replace(
    """        // The alias is anthropic-family so routing fails first (503 in the
        // Gemini envelope is not guaranteed; assert only that the request is
        // NOT served with query-param bypass: it must not be 200).
        assert_ne!(response.status(), 200);""",
    """        assert_eq!(response.status(), 401);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"]["shahid"]["code"], "query_param_auth_rejected");""",
)
src = src.replace(
    "/v1beta/models/claude%3AgenerateContent",
    "/v1beta/models/ghost-alias%3AgenerateContent",
)
src = src.replace(
    'post(&handle, "/v1beta/models/claude:generateContent")',
    'post(&handle, "/v1beta/models/gemini-pro:generateContent")',
)
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('tests fixed')
