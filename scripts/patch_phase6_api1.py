import io

p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

# ---- 1. Surface enum + family envelopes (after openai_error) ----
src = src.replace('''fn upstream_failure_response(
    failure: &UpstreamFailure,
    alias: &str,
    exclusions: usize,
) -> Response {
    let status = match failure.category {
        ErrorCategory::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ErrorCategory::Timeout => StatusCode::GATEWAY_TIMEOUT,
        _ => StatusCode::BAD_GATEWAY,
    };
    openai_error(
        status,
        failure.category_str(),
        &failure.detail,
        json!({
            "category": failure.category_str(),
            "upstream_status": failure.status,
            "alias": alias,
            "excluded_candidates": exclusions,
        }),
    )
}''', '''fn upstream_failure_response(
    failure: &UpstreamFailure,
    alias: &str,
    exclusions: usize,
) -> Response {
    family_error_response(Surface::OpenAiChat, failure, alias, exclusions)
}

/// The protocol surface a request arrived on (Phase 6). Determines the
/// protocol family, wire envelope, and upstream adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    OpenAiChat,
    AnthropicMessages,
    GeminiGenerateContent,
}

impl Surface {
    #[must_use]
    pub const fn family(self) -> sair_domain::protocol::ProtocolFamily {
        match self {
            Surface::OpenAiChat => sair_domain::protocol::ProtocolFamily::OpenAiChat,
            Surface::AnthropicMessages => {
                sair_domain::protocol::ProtocolFamily::AnthropicMessages
            }
            Surface::GeminiGenerateContent => {
                sair_domain::protocol::ProtocolFamily::GeminiGenerateContent
            }
        }
    }

    fn label(self) -> &'static str {
        self.family().as_str()
    }
}

/// F-3: every ErrorCategory maps to a wire-legal Anthropic error type.
#[must_use]
fn anthropic_error_type(category: ErrorCategory) -> &'static str {
    match category {
        ErrorCategory::InvalidCredential => "authentication_error",
        ErrorCategory::PermissionDenied => "permission_error",
        ErrorCategory::BillingOrQuota => "billing_error",
        ErrorCategory::RateLimited => "rate_limit_error",
        ErrorCategory::ModelUnavailable
        | ErrorCategory::InvalidRequest
        | ErrorCategory::ContextLimit => "invalid_request_error",
        ErrorCategory::Timeout => "timeout_error",
        ErrorCategory::ProviderUnavailable | ErrorCategory::TransportFailure | ErrorCategory::Unknown => {
            "api_error"
        }
    }
}

/// F-3: every ErrorCategory maps to a wire-legal Gemini status.
#[must_use]
fn gemini_error_status(category: ErrorCategory) -> &'static str {
    match category {
        ErrorCategory::InvalidCredential => "UNAUTHENTICATED",
        ErrorCategory::PermissionDenied => "PERMISSION_DENIED",
        ErrorCategory::BillingOrQuota | ErrorCategory::RateLimited => "RESOURCE_EXHAUSTED",
        ErrorCategory::ModelUnavailable => "NOT_FOUND",
        ErrorCategory::InvalidRequest | ErrorCategory::ContextLimit => "INVALID_ARGUMENT",
        ErrorCategory::Timeout => "DEADLINE_EXCEEDED",
        ErrorCategory::ProviderUnavailable | ErrorCategory::TransportFailure => "UNAVAILABLE",
        ErrorCategory::Unknown => "INTERNAL",
    }
}

/// Family-faithful error envelope (F-1/F-3/F-4): upstream failures on
/// same-protocol surfaces preserve the upstream status; gateway-local errors
/// use family-legal names with our code in `shahid`.
fn family_error_response(
    surface: Surface,
    failure: &UpstreamFailure,
    alias: &str,
    exclusions: usize,
) -> Response {
    // F-1: same-protocol upstream failures keep the upstream status (any
    // 1xx-5xx value the upstream sent, clamped to a valid StatusCode).
    let status = failure
        .status
        .and_then(|s| StatusCode::from_u16(s).ok())
        .unwrap_or(match failure.category {
            ErrorCategory::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCategory::Timeout => StatusCode::GATEWAY_TIMEOUT,
            _ => StatusCode::BAD_GATEWAY,
        });
    let extra = json!({
        "category": failure.category_str(),
        "upstream_status": failure.status,
        "alias": alias,
        "excluded_candidates": exclusions,
    });
    match surface {
        Surface::OpenAiChat => openai_error(status, failure.category_str(), &failure.detail, extra),
        Surface::AnthropicMessages => (
            status,
            Json(json!({
                "type": "error",
                "error": {
                    "type": anthropic_error_type(failure.category),
                    "message": failure.detail,
                },
                "shahid": { "code": failure.category_str(), "extra": extra },
            })),
        )
            .into_response(),
        Surface::GeminiGenerateContent => (
            status,
            Json(json!({
                "error": {
                    "code": status.as_u16(),
                    "message": failure.detail,
                    "status": gemini_error_status(failure.category),
                    "shahid": { "code": failure.category_str(), "extra": extra },
                }
            })),
        )
            .into_response(),
    }
}

/// Gateway-local error in the family's wire shape (F-4).
fn family_local_error(
    surface: Surface,
    status: StatusCode,
    code: &str,
    message: &str,
    extra: Value,
) -> Response {
    match surface {
        Surface::OpenAiChat => openai_error(status, code, message, extra),
        Surface::AnthropicMessages => (
            status,
            Json(json!({
                "type": "error",
                "error": { "type": "invalid_request_error", "message": message },
                "shahid": { "code": code, "extra": extra },
            })),
        )
            .into_response(),
        Surface::GeminiGenerateContent => (
            status,
            Json(json!({
                "error": {
                    "code": status.as_u16(),
                    "message": message,
                    "status": "INVALID_ARGUMENT",
                    "shahid": { "code": code, "extra": extra },
                }
            })),
        )
            .into_response(),
    }
}''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('surface helpers added')
