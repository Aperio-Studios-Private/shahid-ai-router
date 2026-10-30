//! OpenAI-compatible Chat Completions adapter (FR-006, ADR-009/010).
//!
//! Same-protocol passthrough: the client's JSON body is forwarded unchanged
//! except the `model` field (alias → upstream model id). Responses pass
//! through with usage extracted for metadata records. Classification is
//! body-aware on 4xx (ADR-017: 429 + `insufficient_quota` is BillingOrQuota,
//! not RateLimited — blind rotation on shared-account quota exhaustion burns
//! keys, docs/05 "Quota truth").

use std::time::Duration;

use sair_domain::error::{ErrorCategory, UpstreamFailure};
use sair_vault::SecretBytes;
use serde_json::Value;

use crate::http::SsrfCheckedClient;

/// Hard cap on upstream response bodies (metadata never stores bodies; the
/// passthrough must not buffer unbounded data).
pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Successful upstream call: passthrough body + extracted usage.
#[derive(Debug, Clone)]
pub struct ChatOutcome {
    pub status: u16,
    pub body: Value,
    pub tokens_in: Option<i64>,
    pub tokens_out: Option<i64>,
}

/// Header-safe contract (Phase 5, F-5): header VALUES may contain visible
/// ASCII and spaces (no CR/LF or other control characters, no DEL); length
/// bounded. Header NAMES are fixed constants in our call sites.
#[must_use]
pub fn is_header_safe(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && value.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// Sends one non-streaming chat completion. `base_url` is normalized and
/// joined with the fixed `/chat/completions` segment.
///
/// # Errors
/// [`UpstreamFailure`] with the docs/04 category; transport failures map to
/// `TransportFailure`/`Timeout`.
pub async fn send_chat(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    body: Value,
    timeout: Duration,
) -> Result<ChatOutcome, UpstreamFailure> {
    send_chat_with_headers(client, base_url, model_id, credential, body, timeout, &[]).await
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
) -> Result<ChatOutcome, UpstreamFailure> {
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

    // Normalize base URL and join the fixed path.
    let base = base_url.trim_end_matches('/');
    let endpoint =
        url::Url::parse(&format!("{base}/chat/completions")).map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("invalid upstream base URL: {e}"),
            retry_after_secs: None,
        })?;

    // Alias → upstream model id (the only body mutation).
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".to_string(), Value::String(model_id.to_string()));
        // Streaming is Phase 7 (ADR-010); reject early with a clear signal.
        if obj.get("stream").and_then(Value::as_bool) == Some(true) {
            return Err(UpstreamFailure {
                category: ErrorCategory::InvalidRequest,
                status: None,
                code: Some("streaming_not_supported".to_string()),
                detail: "streaming is not implemented yet (Phase 7)".to_string(),
                retry_after_secs: None,
            });
        }
    }

    let request = client
        .request(reqwest::Method::POST, endpoint)
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("SSRF policy rejected upstream: {e}"),
            retry_after_secs: None,
        })?
        .bearer_auth(&key)
        .timeout(timeout)
        .json(&body);
    let request = extra_headers.iter().fold(request, |req, (name, value)| {
        // Callers pre-validate via is_header_safe; from_str is fallible so a
        // bad value can never panic (F-5).
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
    // R4-04: standard HTTP Retry-After header (seconds form only; HTTP-date
    // values ignored), capped at 3600 s so a hostile provider cannot freeze
    // credentials long-term.
    let header_retry_after: Option<u64> = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| s.min(3600));
    let bytes = response.bytes().await.map_err(|e| classify_transport(&e))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(UpstreamFailure {
            category: ErrorCategory::TransportFailure,
            status: Some(status),
            code: Some("response_too_large".to_string()),
            detail: format!("upstream body exceeded {} bytes", MAX_RESPONSE_BYTES),
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
        let usage = parsed.get("usage");
        Ok(ChatOutcome {
            status,
            tokens_in: usage
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(Value::as_i64),
            tokens_out: usage
                .and_then(|u| u.get("completion_tokens"))
                .and_then(Value::as_i64),
            body: parsed,
        })
    } else {
        let mut failure = classify(status, &parsed);
        failure.retry_after_secs = header_retry_after.or(failure.retry_after_secs);
        failure.retry_after_secs = failure.retry_after_secs.map(|s| s.min(3600));
        Err(failure)
    }
}

/// Transport-layer classification (connection, TLS, timeout).
fn classify_transport(err: &reqwest::Error) -> UpstreamFailure {
    let is_timeout = err.is_timeout();
    UpstreamFailure {
        category: if is_timeout {
            ErrorCategory::Timeout
        } else {
            ErrorCategory::TransportFailure
        },
        status: None,
        code: None,
        detail: truncate_detail(&err.to_string()),
        retry_after_secs: None,
    }
}

/// Body-aware classification of a non-2xx upstream response (ADR-017,
/// R3-01). `body` is the bounded, parsed JSON body.
#[must_use]
pub fn classify(status: u16, body: &Value) -> UpstreamFailure {
    let err_obj = body.get("error");
    let err_type = err_obj
        .and_then(|e| e.get("type"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let err_code = err_obj.and_then(|e| e.get("code")).and_then(|c| match c {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    });
    let message = err_obj
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("upstream error")
        .to_string();
    let retry_after = err_obj
        .and_then(|e| e.get("retry_after"))
        .and_then(Value::as_u64)
        .or_else(|| {
            err_obj
                .and_then(|e| e.get("retry-after"))
                .and_then(Value::as_u64)
        });

    let category = match status {
        401 => ErrorCategory::InvalidCredential,
        403 => ErrorCategory::PermissionDenied,
        402 => ErrorCategory::BillingOrQuota,
        // 429 is billing exhaustion when the provider says so (OpenAI sends
        // `insufficient_quota` as 429) — rotating on it burns shared keys.
        429 if err_type.as_deref() == Some("insufficient_quota")
            || err_code.as_deref() == Some("insufficient_quota") =>
        {
            ErrorCategory::BillingOrQuota
        }
        429 => ErrorCategory::RateLimited,
        404 if err_type.as_deref() == Some("model_not_found")
            || err_code.as_deref() == Some("model_not_found") =>
        {
            ErrorCategory::ModelUnavailable
        }
        404 => ErrorCategory::InvalidRequest,
        400 | 404..=422 if is_context_limit(&err_type, &err_code, &message) => {
            ErrorCategory::ContextLimit
        }
        400..=499 => ErrorCategory::InvalidRequest,
        500..=599 => ErrorCategory::ProviderUnavailable,
        _ => ErrorCategory::Unknown,
    };

    UpstreamFailure {
        category,
        status: Some(status),
        code: err_code.or(err_type),
        detail: truncate_detail(&message),
        retry_after_secs: retry_after,
    }
}

fn is_context_limit(err_type: &Option<String>, err_code: &Option<String>, message: &str) -> bool {
    let needles = [
        "context_length_exceeded",
        "context_window",
        "max_prompt_tokens",
        "maximum context length",
        "context length",
    ];
    let haystacks = [
        err_type.as_deref().unwrap_or("").to_lowercase(),
        err_code.as_deref().unwrap_or("").to_lowercase(),
        message.to_lowercase(),
    ];
    needles
        .iter()
        .any(|n| haystacks.iter().any(|h| h.contains(n)))
}

fn truncate_detail(s: &str) -> String {
    const CAP: usize = 500;
    if s.len() <= CAP {
        s.to_string()
    } else {
        let mut cut = CAP;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::{ChatOutcome, MAX_RESPONSE_BYTES, classify, send_chat};
    use sair_domain::error::ErrorCategory;
    use serde_json::Value;
    use serde_json::json;

    fn value(v: serde_json::Value) -> Value {
        v
    }

    #[test]
    fn classification_matrix_matches_docs04() {
        // Auth failures.
        assert_eq!(
            classify(401, &value(json!({"error": {"message": "bad key"}}))).category,
            ErrorCategory::InvalidCredential
        );
        assert_eq!(
            classify(403, &value(json!({"error": {"message": "forbidden"}}))).category,
            ErrorCategory::PermissionDenied
        );
        // Billing: 402 and 429+insufficient_quota (R3-01) — never RateLimited.
        assert_eq!(
            classify(402, &value(json!({"error": {"message": "pay"}}))).category,
            ErrorCategory::BillingOrQuota
        );
        let quota_429 = classify(
            429,
            &value(json!({"error": {"type": "insufficient_quota", "message": "quota exceeded"}})),
        );
        assert_eq!(quota_429.category, ErrorCategory::BillingOrQuota);
        // Plain 429 is transient.
        assert_eq!(
            classify(429, &value(json!({"error": {"message": "slow down"}}))).category,
            ErrorCategory::RateLimited
        );
        // Model availability.
        assert_eq!(
            classify(
                404,
                &value(json!({"error": {"code": "model_not_found", "message": "no such model"}}))
            )
            .category,
            ErrorCategory::ModelUnavailable
        );
        // Context limit before generic invalid-request.
        assert_eq!(
            classify(
                400,
                &value(
                    json!({"error": {"code": "context_length_exceeded", "message": "too long"}})
                )
            )
            .category,
            ErrorCategory::ContextLimit
        );
        // Generic client errors.
        assert_eq!(
            classify(400, &value(json!({"error": {"message": "bad field"}}))).category,
            ErrorCategory::InvalidRequest
        );
        assert_eq!(
            classify(500, &value(json!({"error": {"message": "boom"}}))).category,
            ErrorCategory::ProviderUnavailable
        );
        assert_eq!(
            classify(529, &value(json!({"error": {"message": "overloaded"}}))).category,
            ErrorCategory::ProviderUnavailable
        );
    }

    #[test]
    fn quota_429_stops_rotation_but_rate_limit_rotates() {
        let quota = classify(
            429,
            &value(json!({"error": {"type": "insufficient_quota", "message": ""}})),
        );
        let rate = classify(429, &value(json!({"error": {"message": ""}})));
        assert_eq!(quota.action(), sair_domain::error::FailureAction::Stop);
        assert_eq!(rate.action(), sair_domain::error::FailureAction::Rotate);
    }

    #[test]
    fn upstream_status_and_code_are_preserved() {
        let f = classify(
            429,
            &value(json!({"error": {"type": "rate_limit_error", "code": "rl-1", "message": "m"}})),
        );
        assert_eq!(f.status, Some(429));
        assert_eq!(f.code.as_deref(), Some("rl-1"));
    }

    #[test]
    fn retry_after_is_parsed() {
        let f = classify(
            429,
            &value(json!({"error": {"message": "slow", "retry_after": 12}})),
        );
        assert_eq!(f.retry_after_secs, Some(12));
    }

    #[test]
    fn usage_extraction_and_bounds_documented() {
        // Constant kept explicit for the passthrough contract.
        assert_eq!(MAX_RESPONSE_BYTES, 32 * 1024 * 1024);
        let outcome = ChatOutcome {
            status: 200,
            tokens_in: Some(11),
            tokens_out: Some(7),
            body: value(json!({"usage": {"prompt_tokens": 11, "completion_tokens": 7}})),
        };
        assert_eq!(outcome.tokens_in, Some(11));
        assert_eq!(outcome.tokens_out, Some(7));
    }

    /// Contract tests over real HTTP with a deterministic mock upstream
    /// (docs/11 adapter-contract layer).
    mod wiremock_tests {
        use super::send_chat;
        use crate::http::{SsrfPolicy, build_client};
        use sair_domain::error::ErrorCategory;
        use sair_vault::SecretBytes;
        use serde_json::json;
        use std::time::Duration;
        use wiremock::matchers::{body_partial_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        fn client() -> crate::http::SsrfCheckedClient {
            let _ = rustls::crypto::ring::default_provider().install_default();
            // Loopback opt-in: the mock upstream binds 127.0.0.1.
            build_client(SsrfPolicy::allow_loopback())
        }

        #[tokio::test]
        async fn request_shape_bearer_and_model_mapping() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/v1/chat/completions"))
                .and(header("authorization", "Bearer sk-test-123"))
                .and(body_partial_json(json!({"model": "upstream-large"})))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "id": "cmpl-1",
                    "usage": {"prompt_tokens": 5, "completion_tokens": 3}
                })))
                .mount(&server)
                .await;

            let outcome = send_chat(
                &client(),
                &format!("{}/v1", server.uri()),
                "upstream-large",
                &SecretBytes::new(b"sk-test-123".to_vec()),
                json!({"model": "my-alias", "messages": [{"role": "user", "content": "hi"}]}),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
            assert_eq!(outcome.status, 200);
            assert_eq!(outcome.tokens_in, Some(5));
            assert_eq!(outcome.tokens_out, Some(3));
            assert_eq!(outcome.body["id"], "cmpl-1");
        }

        #[tokio::test]
        async fn streaming_rejected_early() {
            let server = MockServer::start().await;
            let err = send_chat(
                &client(),
                &format!("{}/v1", server.uri()),
                "m",
                &SecretBytes::new(b"k".to_vec()),
                json!({"model": "my-alias", "stream": true}),
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
            assert_eq!(err.category, ErrorCategory::InvalidRequest);
            assert_eq!(err.code.as_deref(), Some("streaming_not_supported"));
        }

        #[tokio::test]
        async fn classification_over_http_quota_vs_rate_limit() {
            let server = MockServer::start().await;
            // 429 + insufficient_quota → BillingOrQuota (R3-01).
            Mock::given(method("POST"))
                .and(body_partial_json(json!({"model": "quota-model"})))
                .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                    "error": {"type": "insufficient_quota", "message": "quota exceeded"}
                })))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(body_partial_json(json!({"model": "rate-model"})))
                .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                    "error": {"type": "rate_limit_error", "message": "slow down", "retry_after": 7}
                })))
                .mount(&server)
                .await;

            let quota = send_chat(
                &client(),
                &format!("{}/v1", server.uri()),
                "quota-model",
                &SecretBytes::new(b"k".to_vec()),
                json!({"model": "a"}),
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
            assert_eq!(quota.category, ErrorCategory::BillingOrQuota);

            let rate = send_chat(
                &client(),
                &format!("{}/v1", server.uri()),
                "rate-model",
                &SecretBytes::new(b"k".to_vec()),
                json!({"model": "a"}),
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
            assert_eq!(rate.category, ErrorCategory::RateLimited);
            assert_eq!(rate.retry_after_secs, Some(7));
        }

        #[tokio::test]
        async fn malformed_200_is_classified_not_passthrough() {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
                .mount(&server)
                .await;
            let err = send_chat(
                &client(),
                &format!("{}/v1", server.uri()),
                "m",
                &SecretBytes::new(b"k".to_vec()),
                json!({"model": "a"}),
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
            assert_eq!(err.code.as_deref(), Some("malformed_upstream_response"));
            assert_eq!(err.category, ErrorCategory::Unknown);
        }
    }
}
