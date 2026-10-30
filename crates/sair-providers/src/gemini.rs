//! Native Gemini generateContent adapter (FR-006, ADR-009/010; docs verified
//! 2026-10-31 by the Phase 6 review — ai.google.dev; the Interactions API is
//! complementary, generateContent remains supported).
//!
//! Same-protocol passthrough with `x-goog-api-key` auth. Classification is
//! body-aware; `RESOURCE_EXHAUSTED` defaults to BillingOrQuota (free-tier
//! exhaustion IS quota — docs/05) and downgrades to RateLimited only on
//! explicit rate-limit signals (F-7).

use std::time::Duration;

use sair_domain::error::{ErrorCategory, UpstreamFailure};
use sair_vault::SecretBytes;
use serde_json::Value;

use crate::http::SsrfCheckedClient;
use crate::openai::{ChatOutcome, MAX_RESPONSE_BYTES};

/// Sends one non-streaming `models/{model}:generateContent` request.
///
/// # Errors
/// [`UpstreamFailure`] with the docs/04 category.
pub async fn generate_content(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    mut body: Value,
    timeout: Duration,
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

    let base = base_url.trim_end_matches('/');
    let endpoint = url::Url::parse(&format!("{base}/v1beta/models/{model_id}:generateContent"))
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("invalid upstream base URL: {e}"),
            retry_after_secs: None,
        })?;

    // F-2: streaming method rejected at the surface; generationConfig-level
    // streaming does not exist in generateContent.
    let endpoint = if body.get("stream").and_then(Value::as_bool) == Some(true) {
        return Err(UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: Some("streaming_not_supported".to_string()),
            detail: "streaming is not implemented yet (Phase 7)".to_string(),
            retry_after_secs: None,
        });
    } else {
        endpoint
    };
    let _ = &mut body; // passthrough: no body mutation beyond the URL model

    let request = client
        .request(reqwest::Method::POST, endpoint)
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("SSRF policy rejected upstream: {e}"),
            retry_after_secs: None,
        })?
        .header("x-goog-api-key", &key)
        .timeout(timeout)
        .json(&body);

    let response = request.send().await.map_err(|e| classify_transport(&e))?;
    let status = response.status().as_u16();

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
        let usage = parsed.get("usageMetadata");
        Ok(ChatOutcome {
            status,
            tokens_in: usage
                .and_then(|u| u.get("promptTokenCount"))
                .and_then(Value::as_i64),
            tokens_out: usage
                .and_then(|u| u.get("candidatesTokenCount"))
                .and_then(Value::as_i64),
            body: parsed,
        })
    } else {
        Err(classify(status, &parsed))
    }
}

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

/// Body-aware classification of a non-2xx Gemini response (F-2/F-3/F-7).
#[must_use]
pub fn classify(status: u16, body: &Value) -> UpstreamFailure {
    let err = body.get("error");
    let status_name = err
        .and_then(|e| e.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let message = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("upstream error")
        .to_string();
    let details: Vec<String> = err
        .and_then(|e| e.get("details"))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|d| d.to_string().to_lowercase()).collect())
        .unwrap_or_default();
    // google.rpc RetryInfo.retryDelay ("5s") for cooldown (F-7).
    let retry_after_secs: Option<u64> = details
        .iter()
        .find(|d| d.contains("retryinfo"))
        .and_then(|d| {
            // Details were lowercased for matching; splitting on the full
            // serialized key `"retrydelay":"` puts the value first.
            let rest = d.split("retrydelay\":\"").nth(1)?;
            let end = rest.find('"')?;
            rest[..end].trim_end_matches('s').parse::<u64>().ok()
        })
        .map(|s| s.min(3600));

    let is_rate_limit_signal =
        details.iter().any(|d| d.contains("rate")) || message.to_lowercase().contains("rate limit");

    let category = match status_name.as_str() {
        // F-2 before any 5xx fallback.
        "DEADLINE_EXCEEDED" => ErrorCategory::Timeout,
        "UNAUTHENTICATED" => ErrorCategory::InvalidCredential,
        "PERMISSION_DENIED" => ErrorCategory::PermissionDenied,
        // F-7: free-tier exhaustion is quota; only explicit rate-limit
        // signals downgrade to RateLimited.
        "RESOURCE_EXHAUSTED" if is_rate_limit_signal => ErrorCategory::RateLimited,
        "RESOURCE_EXHAUSTED" => ErrorCategory::BillingOrQuota,
        "NOT_FOUND" => ErrorCategory::ModelUnavailable,
        "INVALID_ARGUMENT" if is_context_limit(&message) => ErrorCategory::ContextLimit,
        "INVALID_ARGUMENT" | "FAILED_PRECONDITION" | "OUT_OF_RANGE" => {
            ErrorCategory::InvalidRequest
        }
        "UNAVAILABLE" | "INTERNAL" => ErrorCategory::ProviderUnavailable,
        // F-3: unmatched statuses fall back conservatively by HTTP status.
        _ => match status {
            401 => ErrorCategory::InvalidCredential,
            403 => ErrorCategory::PermissionDenied,
            429 => ErrorCategory::RateLimited,
            404 => ErrorCategory::InvalidRequest,
            400..=499 if is_context_limit(&message) => ErrorCategory::ContextLimit,
            400..=499 => ErrorCategory::InvalidRequest,
            500..=599 => ErrorCategory::ProviderUnavailable,
            _ => ErrorCategory::Unknown,
        },
    };

    UpstreamFailure {
        category,
        status: Some(status),
        code: Some(status_name),
        detail: truncate_detail(&message),
        retry_after_secs,
    }
}

fn is_context_limit(message: &str) -> bool {
    let m = message.to_lowercase();
    [
        "input token count",
        "exceeds the maximum",
        "context length",
        "token limit",
    ]
    .iter()
    .any(|n| m.contains(n))
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
    use super::{classify, generate_content};
    use crate::http::{SsrfPolicy, build_client};
    use sair_domain::error::{ErrorCategory, FailureAction};
    use sair_vault::SecretBytes;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn classification_matrix_matches_gemini_statuses() {
        let f = |s: &str| {
            classify(
                400,
                &json!({"error": {"code": 400, "status": s, "message": "m"}}),
            )
        };
        assert_eq!(
            f("UNAUTHENTICATED").category,
            ErrorCategory::InvalidCredential
        );
        assert_eq!(
            f("PERMISSION_DENIED").category,
            ErrorCategory::PermissionDenied
        );
        assert_eq!(f("DEADLINE_EXCEEDED").category, ErrorCategory::Timeout);
        assert_eq!(f("NOT_FOUND").category, ErrorCategory::ModelUnavailable);
        assert_eq!(
            f("INVALID_ARGUMENT").category,
            ErrorCategory::InvalidRequest
        );
        assert_eq!(
            f("FAILED_PRECONDITION").category,
            ErrorCategory::InvalidRequest
        );
        assert_eq!(
            classify(
                500,
                &json!({"error": {"status": "UNAVAILABLE", "message": ""}})
            )
            .category,
            ErrorCategory::ProviderUnavailable
        );
        assert_eq!(
            classify(400, &json!({"error": {"status": "INVALID_ARGUMENT", "message": "input token count exceeds the maximum"}})).category,
            ErrorCategory::ContextLimit
        );
    }

    #[test]
    fn resource_exhausted_defaults_to_billing_with_rate_limit_downgrade() {
        // Free-tier quota exhaustion: no rate-limit signal → BillingOrQuota
        // (Stop — no key burning).
        let quota = classify(
            429,
            &json!({"error": {"status": "RESOURCE_EXHAUSTED", "message": "Resource has been exhausted"}}),
        );
        assert_eq!(quota.category, ErrorCategory::BillingOrQuota);
        assert_eq!(quota.action(), FailureAction::Stop);
        // Explicit rate-limit signal → RateLimited (Rotate).
        let rate = classify(
            429,
            &json!({"error": {"status": "RESOURCE_EXHAUSTED", "message": "rate limit exceeded"}}),
        );
        assert_eq!(rate.category, ErrorCategory::RateLimited);
        assert_eq!(rate.action(), FailureAction::Rotate);
    }

    #[test]
    fn retry_info_delay_is_parsed_and_capped() {
        let f = classify(
            429,
            &json!({"error": {
                "status": "RESOURCE_EXHAUSTED",
                "message": "rate limit exceeded",
                "details": [{"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "7s"}]
            }}),
        );
        assert_eq!(f.retry_after_secs, Some(7));
        let huge = classify(
            429,
            &json!({"error": {
                "status": "RESOURCE_EXHAUSTED",
                "message": "rate limit",
                "details": [{"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "999999s"}]
            }}),
        );
        assert_eq!(huge.retry_after_secs, Some(3600));
    }

    #[tokio::test]
    async fn request_shape_key_header_and_url() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/models/upstream-model:generateContent"))
            .and(header("x-goog-api-key", "gemini-key-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "candidates": [],
                "usageMetadata": {"promptTokenCount": 4, "candidatesTokenCount": 2}
            })))
            .mount(&server)
            .await;

        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = build_client(SsrfPolicy::allow_loopback());
        let outcome = generate_content(
            &client,
            &server.uri(),
            "upstream-model",
            &SecretBytes::new(b"gemini-key-123".to_vec()),
            json!({"contents": [{"parts": [{"text": "hi"}]}]}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(outcome.tokens_in, Some(4));
        assert_eq!(outcome.tokens_out, Some(2));
    }
}
