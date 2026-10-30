import io

# ---- openai.rs: generic streaming request + StreamStart (F-2 total predicate) ----
p = 'crates/sair-providers/src/openai.rs'
src = io.open(p, encoding='utf-8').read()

# extend StreamStart: NonStreamed carries raw body for per-family mapping
src = src.replace('''pub enum StreamStart {
    Committed(StreamOutcome),
    NonStreamed(ChatOutcome),
}''', '''pub enum StreamStart {
    Committed(StreamOutcome),
    /// 2xx without SSE content-type (F-2): bounded body, mapped per family.
    NonStreamed { status: u16, body: Value },
}''')

# generic streaming request helper, appended before #[cfg(test)]
addition = '''
/// Shared streaming request core (Phase 7): sends `body` verbatim, applies
/// the F-2 total commit predicate, and classifies non-2xx with the
/// family-provided classifier. Streaming clients carry NO total timeout
/// (F-1) — callers must pass a `build_streaming_client` client.
///
/// # Errors
/// Classified [`UpstreamFailure`] for non-2xx and gateway-detected
/// malformed/oversized bodies.
pub async fn stream_request(
    client: &SsrfCheckedClient,
    endpoint: url::Url,
    auth_header: (&str, &str),
    extra_headers: &[(String, String)],
    body: Value,
    classify_fn: fn(u16, &Value) -> UpstreamFailure,
) -> Result<StreamStart, UpstreamFailure> {
    let request = client
        .request(reqwest::Method::POST, endpoint)
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("SSRF policy rejected upstream: {e}"),
            retry_after_secs: None,
        })?
        .header(auth_header.0, auth_header.1)
        .json(&body);
    let request = extra_headers.iter().fold(request, |req, (name, value)| {
        match (
            reqwest::header::HeaderName::try_from(name.as_str()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            (Ok(n), Ok(v)) => req.header(n, v),
            _ => {
                tracing::warn!(header = %name, "skipping invalid attribution header");
                req
            }
        }
    });
    let response = request.send().await.map_err(|e| classify_transport(&e))?;
    let status = response.status().as_u16();

    // F-2 total commit predicate: SSE content-type on 2xx commits; anything
    // else is buffered (bounded) and resolved as passthrough or error.
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if (200..300).contains(&status)
        && content_type
            .as_deref()
            .is_some_and(|ct| ct.starts_with("text/event-stream"))
    {
        return Ok(StreamStart::Committed(StreamOutcome {
            status,
            content_type,
            byte_stream: Box::pin(response.bytes_stream()),
        }));
    }

    let bytes = response.bytes().await.map_err(|e| classify_transport(&e))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(UpstreamFailure {
            category: ErrorCategory::TransportFailure,
            status: Some(status),
            code: Some("response_too_large".to_string()),
            detail: format!("upstream body exceeded {MAX_RESPONSE_BYTES} bytes"),
            retry_after_secs: None,
        });
    }
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|e| UpstreamFailure {
        category: ErrorCategory::Unknown,
        status: Some(status),
        code: Some("malformed_upstream_response".to_string()),
        detail: format!("upstream returned non-JSON body: {e}"),
        retry_after_secs: None,
    })?;
    if (200..300).contains(&status) {
        Ok(StreamStart::NonStreamed { status, body: parsed })
    } else {
        Err(classify_fn(status, &parsed))
    }
}
'''
src = src.rstrip() + '\n' + addition

# exports
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
p = 'crates/sair-providers/src/lib.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('pub use openai::{ChatOutcome, MAX_RESPONSE_BYTES, is_header_safe, send_chat, send_chat_with_headers};',
                  'pub use openai::{ChatOutcome, MAX_RESPONSE_BYTES, StreamOutcome, StreamStart, is_header_safe, send_chat, send_chat_with_headers, stream_request};')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('stream core added')

# ---- Cargo: futures-core + bytes ----
p = 'crates/sair-providers/Cargo.toml'
src = io.open(p, encoding='utf-8').read()
src = src.replace('url = "2"', 'url = "2"\nfutures-core = "0.3"\nbytes = "1"')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('deps added')
