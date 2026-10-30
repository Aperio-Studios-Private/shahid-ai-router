import io

# ---- openai.rs: streaming variant (F-2 commit predicate) ----
p = 'crates/sair-providers/src/openai.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''pub async fn send_chat(
    client: &SsrfCheckedClient,''', '''/// A committed SSE stream (Phase 7): the upstream accepted the streaming
/// request; relaying must begin. `content_type` is passed through verbatim.
pub struct StreamOutcome {
    pub status: u16,
    pub content_type: Option<String>,
    pub byte_stream: std::pin::Pin<
        Box<dyn futures_core::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>,
    >,
}

/// What a streaming request produced (Phase 7, F-2 total commit predicate):
/// SSE on 2xx → committed [`StreamOutcome`]; any other 2xx → bounded
/// non-streamed success passthrough; non-2xx → classified error (rotation
/// still legal pre-commit).
pub enum StreamStart {
    Committed(StreamOutcome),
    NonStreamed(ChatOutcome),
}

pub async fn send_chat(
    client: &SsrfCheckedClient,''')
src = src.replace('''    let request = extra_headers.iter().fold(request, |req, (name, value)| {''', '''    let request = extra_headers.iter().fold(request, |req, (name, value)| {''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('openai stream types added')
