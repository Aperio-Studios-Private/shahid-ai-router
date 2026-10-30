import io

# ---- anthropic.rs: streaming variant ----
p = 'crates/sair-providers/src/anthropic.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''use crate::http::SsrfCheckedClient;
use crate::openai::{ChatOutcome, MAX_RESPONSE_BYTES, is_header_safe};''', '''use crate::http::SsrfCheckedClient;
use crate::openai::{
    ChatOutcome, MAX_RESPONSE_BYTES, StreamStart, is_header_safe, stream_request,
};''')
addition = '''
/// Streaming variant (Phase 7): `stream:true` stays in the body; SSE on 2xx
/// commits, other 2xx is mapped through the non-streamed shape.
///
/// # Errors
/// See [`send_messages`].
#[allow(clippy::too_many_arguments)]
pub async fn send_messages_streaming(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    anthropic_version: &str,
    mut body: Value,
) -> Result<StreamStart, UpstreamFailure> {
    let key = credential
        .expose_str()
        .map_err(|_| UpstreamFailure {
            category: ErrorCategory::Unknown,
            status: None,
            code: None,
            detail: "credential is not valid UTF-8".to_string(),
            retry_after_secs: None,
        })?
        .to_string();
    let base = base_url.trim_end_matches('/');
    let endpoint = url::Url::parse(&format!("{base}/v1/messages"))
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("invalid upstream base URL: {e}"),
            retry_after_secs: None,
        })?;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".to_string(), Value::String(model_id.to_string()));
    }
    let version = if is_header_safe(anthropic_version) {
        anthropic_version
    } else {
        DEFAULT_ANTHROPIC_VERSION
    };
    match stream_request(
        client,
        endpoint,
        ("x-api-key", &key),
        &[("anthropic-version".to_string(), version.to_string())],
        body,
        classify,
    )
    .await?
    {
        StreamStart::Committed(outcome) => Ok(StreamStart::Committed(outcome)),
        StreamStart::NonStreamed { status, body } => {
            let usage = body.get("usage");
            Ok(StreamStart::NonStreamed {
                status,
                body: body,
            })
        }
    }
}
'''
# NonStreamed mapping needs ChatOutcome for handler symmetry: reuse tuple; simpler: return body raw.
src = src.replace('''        StreamStart::NonStreamed { status, body } => {
            let usage = body.get("usage");
            Ok(StreamStart::NonStreamed {
                status,
                body: body,
            })
        }
    }
}''', '''        StreamStart::NonStreamed { status, body } => Ok(StreamStart::NonStreamed { status, body }),
    }
}
''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('anthropic streaming added')

# ---- gemini.rs: streaming variant with alt=sse ----
p = 'crates/sair-providers/src/gemini.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''use crate::http::SsrfCheckedClient;
use crate::openai::{ChatOutcome, MAX_RESPONSE_BYTES};''', '''use crate::http::SsrfCheckedClient;
use crate::openai::{ChatOutcome, MAX_RESPONSE_BYTES, StreamStart, stream_request};''')
addition = '''
/// Streaming variant (Phase 7, F-3): posts to
/// `{base}/v1beta/models/{model}:streamGenerateContent?alt=sse` — the query
/// is appended by the gateway (never `key`).
///
/// # Errors
/// See [`generate_content`].
pub async fn generate_content_streaming(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    body: Value,
) -> Result<StreamStart, UpstreamFailure> {
    let key = credential
        .expose_str()
        .map_err(|_| UpstreamFailure {
            category: ErrorCategory::Unknown,
            status: None,
            code: None,
            detail: "credential is not valid UTF-8".to_string(),
            retry_after_secs: None,
        })?
        .to_string();
    let base = base_url.trim_end_matches('/');
    let endpoint = url::Url::parse(&format!(
        "{base}/v1beta/models/{model_id}:streamGenerateContent?alt=sse"
    ))
    .map_err(|e| UpstreamFailure {
        category: ErrorCategory::InvalidRequest,
        status: None,
        code: None,
        detail: format!("invalid upstream base URL: {e}"),
        retry_after_secs: None,
    })?;
    stream_request(
        client,
        endpoint,
        ("x-goog-api-key", &key),
        &[],
        body,
        classify,
    )
    .await
}
'''
src = src.rstrip() + '\n' + addition
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('gemini streaming added')

# ---- openai.rs: chat streaming variant with attribution support ----
p = 'crates/sair-providers/src/openai.rs'
src = io.open(p, encoding='utf-8').read()
addition = '''
/// Streaming variant (Phase 7): `stream:true` stays in the body; SSE commits,
/// other 2xx maps through the non-streamed shape. Attribution headers only
/// reach openrouter providers via the caller's extra_headers (F-1).
///
/// # Errors
/// See [`send_chat`].
#[allow(clippy::too_many_arguments)]
pub async fn send_chat_streaming_with_headers(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    mut body: Value,
    extra_headers: &[(String, String)],
) -> Result<StreamStart, UpstreamFailure> {
    let key = credential
        .expose_str()
        .map_err(|_| UpstreamFailure {
            category: ErrorCategory::Unknown,
            status: None,
            code: None,
            detail: "credential is not valid UTF-8".to_string(),
            retry_after_secs: None,
        })?
        .to_string();
    let base = base_url.trim_end_matches('/');
    let endpoint = url::Url::parse(&format!("{base}/chat/completions"))
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("invalid upstream base URL: {e}"),
            retry_after_secs: None,
        })?;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".to_string(), Value::String(model_id.to_string()));
    }
    stream_request(
        client,
        endpoint,
        ("Authorization", &format!("Bearer {key}")),
        extra_headers,
        body,
        classify,
    )
    .await
}
'''
src = src.rstrip() + '\n' + addition
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
p = 'crates/sair-providers/src/lib.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('pub use openai::{ChatOutcome, MAX_RESPONSE_BYTES, StreamOutcome, StreamStart, is_header_safe, send_chat, send_chat_with_headers, stream_request};',
                  'pub use openai::{ChatOutcome, MAX_RESPONSE_BYTES, StreamOutcome, StreamStart, is_header_safe, send_chat, send_chat_streaming_with_headers, send_chat_with_headers, stream_request};')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('openai streaming added')
