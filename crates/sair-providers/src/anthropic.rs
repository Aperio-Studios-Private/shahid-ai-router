//! Native Anthropic Messages adapter (FR-006, ADR-009/010; docs verified
//! 2026-10-31 by the Phase 6 review — platform.claude.com/docs).
//!
//! Same-protocol passthrough: the client's Anthropic body is forwarded
//! unchanged except the `model` field (alias → upstream id). Classification
//! is body-aware (review F-3) with ADR-017 timeout semantics (F-2).

use std::time::Duration;

use sair_domain::error::{ErrorCategory, UpstreamFailure};
use sair_vault::SecretBytes;
use serde_json::Value;

use crate::http::SsrfCheckedClient;
use crate::openai::{ChatOutcome, MAX_RESPONSE_BYTES, is_header_safe};

/// Pinned default `anthropic-version` (legacy-supported; F-5: a client's
/// validated header is forwarded instead when present).
pub const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

/// Sends one non-streaming Anthropic Messages request.
///
/// # Errors
/// [`UpstreamFailure`] with the docs/04 category.
pub async fn send_messages(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    anthropic_version: &str,
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
    let endpoint =
        url::Url::parse(&format!("{base}/v1/messages")).map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("invalid upstream base URL: {e}"),
            retry_after_secs: None,
        })?;

    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".to_string(), Value::String(model_id.to_string()));
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

    let version = if is_header_safe(anthropic_version) {
        anthropic_version
    } else {
        DEFAULT_ANTHROPIC_VERSION
    };

    let request = client
        .request(reqwest::Method::POST, endpoint)
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("SSRF policy rejected upstream: {e}"),
            retry_after_secs: None,
        })?
        .header("x-api-key", &key)
        .header("anthropic-version", version)
        .timeout(timeout)
        .json(&body);

    let response = request.send().await.map_err(|e| classify_transport(&e))?;
    let status = response.status().as_u16();
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
        let usage = parsed.get("usage");
        Ok(ChatOutcome {
            status,
            tokens_in: usage
                .and_then(|u| u.get("input_tokens"))
                .and_then(Value::as_i64),
            tokens_out: usage
                .and_then(|u| u.get("output_tokens"))
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

/// Body-aware classification of a non-2xx Anthropic response (F-2/F-3).
#[must_use]
pub fn classify(status: u16, body: &Value) -> UpstreamFailure {
    let err = body
        .get("error")
        .and_then(|e| e.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let message = body
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("upstream error")
        .to_string();

    let category = match err.as_str() {
        // F-2 before any 5xx fallback: ambiguous execution never rotates.
        "timeout_error" => ErrorCategory::Timeout,
        "authentication_error" => ErrorCategory::InvalidCredential,
        "permission_error" => ErrorCategory::PermissionDenied,
        // F-3: first-class billing anchor (402 billing_error, verified).
        "billing_error" => ErrorCategory::BillingOrQuota,
        "rate_limit_error" => ErrorCategory::RateLimited,
        "not_found_error" => {
            if message.to_lowercase().contains("model") {
                ErrorCategory::ModelUnavailable
            } else {
                ErrorCategory::InvalidRequest
            }
        }
        "request_too_large" => ErrorCategory::InvalidRequest,
        "invalid_request_error" if is_context_limit(&message) => ErrorCategory::ContextLimit,
        "invalid_request_error" => ErrorCategory::InvalidRequest,
        "overloaded_error" | "api_error" => ErrorCategory::ProviderUnavailable,
        // F-3: unmatched types fall back conservatively by status.
        _ => match status {
            401 => ErrorCategory::InvalidCredential,
            403 => ErrorCategory::PermissionDenied,
            402 => ErrorCategory::BillingOrQuota,
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
        code: Some(err),
        detail: truncate_detail(&message),
        retry_after_secs: None,
    }
}

fn is_context_limit(message: &str) -> bool {
    let m = message.to_lowercase();
    [
        "prompt is too long",
        "max_tokens",
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

/// Sends one non-streaming `/v1/messages/count_tokens` request (F-8). The
/// response's `input_tokens` maps to `tokens_in`; the body passes through.
///
/// # Errors
/// See [`send_messages`].
pub async fn count_tokens(
    client: &SsrfCheckedClient,
    base_url: &str,
    model_id: &str,
    credential: &SecretBytes,
    anthropic_version: &str,
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
    let endpoint = url::Url::parse(&format!("{base}/v1/messages/count_tokens")).map_err(|e| {
        UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("invalid upstream base URL: {e}"),
            retry_after_secs: None,
        }
    })?;

    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".to_string(), Value::String(model_id.to_string()));
        obj.remove("stream"); // not a valid count_tokens field
    }

    let version = if is_header_safe(anthropic_version) {
        anthropic_version
    } else {
        DEFAULT_ANTHROPIC_VERSION
    };

    let request = client
        .request(reqwest::Method::POST, endpoint)
        .map_err(|e| UpstreamFailure {
            category: ErrorCategory::InvalidRequest,
            status: None,
            code: None,
            detail: format!("SSRF policy rejected upstream: {e}"),
            retry_after_secs: None,
        })?
        .header("x-api-key", &key)
        .header("anthropic-version", version)
        .timeout(timeout)
        .json(&body);

    let response = request.send().await.map_err(|e| classify_transport(&e))?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await.map_err(|e| classify_transport(&e))?;
    let parsed: Value = serde_json::from_slice(&bytes).map_err(|e| UpstreamFailure {
        category: ErrorCategory::Unknown,
        status: Some(status),
        code: Some("malformed_upstream_response".to_string()),
        detail: format!("upstream returned non-JSON body: {e}"),
        retry_after_secs: None,
    })?;

    if (200..300).contains(&status) {
        Ok(ChatOutcome {
            status,
            tokens_in: parsed.get("input_tokens").and_then(Value::as_i64),
            tokens_out: None,
            body: parsed,
        })
    } else {
        Err(classify(status, &parsed))
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_ANTHROPIC_VERSION, classify, is_header_safe};
    use sair_domain::error::{ErrorCategory, FailureAction};
    use serde_json::json;

    #[test]
    fn classification_matrix_matches_anthropic_errors() {
        let f = |t: &str| {
            classify(
                400,
                &json!({"type": "error", "error": {"type": t, "message": "m"}}),
            )
        };
        assert_eq!(
            f("authentication_error").category,
            ErrorCategory::InvalidCredential
        );
        assert_eq!(
            f("permission_error").category,
            ErrorCategory::PermissionDenied
        );
        assert_eq!(f("billing_error").category, ErrorCategory::BillingOrQuota);
        assert_eq!(f("rate_limit_error").category, ErrorCategory::RateLimited);
        assert_eq!(f("timeout_error").category, ErrorCategory::Timeout);
        assert_eq!(
            f("overloaded_error").category,
            ErrorCategory::ProviderUnavailable
        );
        assert_eq!(
            classify(404, &json!({"type": "error", "error": {"type": "not_found_error", "message": "model not found"}})).category,
            ErrorCategory::ModelUnavailable
        );
        assert_eq!(
            classify(400, &json!({"type": "error", "error": {"type": "invalid_request_error", "message": "prompt is too long: 300000 tokens > 200000"}})).category,
            ErrorCategory::ContextLimit
        );
        assert_eq!(
            f("invalid_request_error").category,
            ErrorCategory::InvalidRequest
        );
    }

    #[test]
    fn timeout_never_rotates_and_rate_limit_rotates() {
        let timeout = classify(
            504,
            &json!({"type": "error", "error": {"type": "timeout_error", "message": ""}}),
        );
        assert_eq!(timeout.action(), FailureAction::Stop);
        let rate = classify(
            429,
            &json!({"type": "error", "error": {"type": "rate_limit_error", "message": ""}}),
        );
        assert_eq!(rate.action(), FailureAction::Rotate);
    }

    #[test]
    fn unmatched_types_fall_back_by_status() {
        let f = classify(
            409,
            &json!({"type": "error", "error": {"type": "conflict_error", "message": "conflict"}}),
        );
        assert_eq!(f.category, ErrorCategory::InvalidRequest);
        let f = classify(
            529,
            &json!({"type": "error", "error": {"type": "unknown_future_type", "message": ""}}),
        );
        assert_eq!(f.category, ErrorCategory::ProviderUnavailable);
    }

    #[test]
    fn default_version_is_the_pinned_legacy() {
        assert_eq!(DEFAULT_ANTHROPIC_VERSION, "2023-06-01");
        // F-5: forwarded client versions must be header-safe.
        assert!(is_header_safe("2023-06-01"));
        assert!(!is_header_safe("bad\r\nversion"));
    }
}
