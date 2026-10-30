import io

# ---- openai.rs: send_chat gains validated extra headers (F-5) ----
p = 'crates/sair-providers/src/openai.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''pub async fn send_chat(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    mut body: Value,
    timeout: Duration,
) -> Result<ChatOutcome, UpstreamFailure> {''', '''/// Header-safe attribution/validation contract (Phase 5, F-5): names and
/// values must be HTTP visible ASCII without CR/LF; anything else is
/// rejected before a header is ever constructed.
#[must_use]
pub fn is_header_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value
            .bytes()
            .all(|b| (0x21..=0x7e).contains(&b) && b != b':' && b != b',' )
}

pub async fn send_chat(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    body: Value,
    timeout: Duration,
) -> Result<ChatOutcome, UpstreamFailure> {
    send_chat_with_headers(client, base_url, model_id, credential, body, timeout, &[])
}

/// As [`send_chat`], attaching pre-validated extra headers (provider-specific
/// attribution, Phase 5). Invalid entries are skipped by the CALLER; this
/// function only receives header-safe pairs.
///
/// # Errors
/// See [`send_chat`].
pub async fn send_chat_with_headers(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    mut body: Value,
    timeout: Duration,
    extra_headers: &[(String, String)],
) -> Result<ChatOutcome, UpstreamFailure> {''')
src = src.replace('''        .bearer_auth(&key)
        .timeout(timeout)
        .json(&body);''', '''        .bearer_auth(&key)
        .timeout(timeout)
        .json(&body);
    let request = extra_headers.iter().fold(request, |req, (name, value)| {
        // Callers pre-validate via is_header_safe; from_str is fallible so a
        // bad value can never panic (F-5).
        match (reqwest::header::HeaderName::try_from(name.as_str()), reqwest::header::HeaderValue::from_str(value)) {
            (Ok(n), Ok(v)) => req.header(n, v),
            _ => {
                tracing::warn!(header = %name, "skipping invalid attribution header");
                req
            }
        }
    });''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('openai.rs generalized')

# ---- lib.rs export ----
p = 'crates/sair-providers/src/lib.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('pub mod openai;', 'pub mod openai;\npub mod openrouter;')
src = src.replace('pub use openai::{ChatOutcome, MAX_RESPONSE_BYTES, send_chat};', 'pub use openai::{ChatOutcome, MAX_RESPONSE_BYTES, is_header_safe, send_chat, send_chat_with_headers};\npub use openrouter::{AttributionConfig, attribution_headers};')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('lib.rs exported')
