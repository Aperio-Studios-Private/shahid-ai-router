import io

p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

# ---- UpstreamCall enum + PermitStream + extract_usage (before process_chat) ----
addition = '''/// What one upstream candidate produced (Phase 7).
enum UpstreamCall {
    Chat(ChatOutcome),
    /// SSE committed: the byte stream must be relayed; no failover after.
    CommittedStream(sair_providers::openai::StreamOutcome),
    /// 2xx non-SSE body (F-2): normal passthrough.
    NonStreamed { status: u16, body: Value },
}

/// FR-021: a stream wrapper that releases the deployment permit when the
/// relayed stream ends (F-6: the permit lives as long as the stream).
struct PermitStream<S> {
    inner: S,
    permit: tokio::sync::OwnedSemaphorePermit,
}

impl<S> PermitStream<S> {
    fn new(inner: S, permit: tokio::sync::OwnedSemaphorePermit) -> Self {
        Self { inner, permit }
    }
}

impl<S> futures_core::Stream for PermitStream<S>
where
    S: futures_core::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin,
{
    type Item = reqwest::Result<bytes::Bytes>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.inner).poll_next(cx)
    }
}

/// Per-family usage extraction for non-streamed bodies.
fn extract_usage(surface: Surface, body: &Value) -> (Option<i64>, Option<i64>) {
    let usage = match surface {
        Surface::OpenAiChat => body.get("usage").map(|u| (u, u)),
        Surface::AnthropicMessages => body.get("usage").map(|u| (u, u)),
        Surface::GeminiGenerateContent => body.get("usageMetadata").map(|u| (u, u)),
    };
    match surface {
        Surface::OpenAiChat => (
            usage
                .and_then(|(u, _)| u.get("prompt_tokens"))
                .and_then(Value::as_i64),
            usage
                .and_then(|(_, u)| u.get("completion_tokens"))
                .and_then(Value::as_i64),
        ),
        Surface::AnthropicMessages => (
            usage
                .and_then(|(u, _)| u.get("input_tokens"))
                .and_then(Value::as_i64),
            usage
                .and_then(|(_, u)| u.get("output_tokens"))
                .and_then(Value::as_i64),
        ),
        Surface::GeminiGenerateContent => (
            usage
                .and_then(|(u, _)| u.get("promptTokenCount"))
                .and_then(Value::as_i64),
            usage
                .and_then(|(_, u)| u.get("candidatesTokenCount"))
                .and_then(Value::as_i64),
        ),
    }
}

'''
anchor = '''/// Family-parameterized chat/count flow: validation, routing, rotation loop,
/// per-family envelopes (Phase 6 refactor; behavior preserved per family).'''
assert anchor in src
src = src.replace(anchor, addition + anchor)
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('helpers added')
