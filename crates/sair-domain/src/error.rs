//! Provider error taxonomy (docs/04 "Error categories").
//!
//! Classification drives routing decisions (docs/05 "Retry safety"): which
//! failures may be retried, which quarantine a credential, and which must be
//! surfaced to the client instead of blindly retried.

use serde::Serialize;

/// Upstream failure categories. Preserve upstream status/code alongside the
/// category in the adapter layer; this enum is the routing-relevant view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// The credential is rejected (bad key, revoked).
    InvalidCredential,
    /// Authenticated but not permitted for this resource.
    PermissionDenied,
    /// Billing or account/project quota exhaustion (not transient rate limiting).
    BillingOrQuota,
    /// Transient rate limiting; may carry Retry-After.
    RateLimited,
    /// The requested model is unavailable or unknown on this deployment.
    ModelUnavailable,
    /// The request itself is invalid; retrying unchanged cannot succeed.
    InvalidRequest,
    /// The request exceeds the model's context limit.
    ContextLimit,
    /// Bounded timeout elapsed.
    Timeout,
    /// Provider is down or unreachable (5xx, connection failures).
    ProviderUnavailable,
    /// Transport-level failure before a classified response (DNS, TLS, reset).
    TransportFailure,
    /// Anything unclassifiable; treated conservatively.
    Unknown,
}

impl ErrorCategory {
    /// Whether a failed request may be retried as-is without risk of harmful
    /// duplicate execution or guaranteed repeat failure (docs/05).
    #[must_use]
    pub fn is_safely_retryable(self) -> bool {
        matches!(
            self,
            ErrorCategory::RateLimited
                | ErrorCategory::Timeout
                | ErrorCategory::ProviderUnavailable
                | ErrorCategory::TransportFailure
        )
    }

    /// Whether this category implicates the credential itself such that the
    /// credential should be quarantined pending user review (docs/05: an
    /// authentication failure may quarantine one credential).
    #[must_use]
    pub fn should_quarantine_credential(self) -> bool {
        matches!(
            self,
            ErrorCategory::InvalidCredential | ErrorCategory::PermissionDenied
        )
    }

    /// Whether this is an account-level condition: rotating to another
    /// credential from the same account/project must not be attempted blindly
    /// (docs/05 "Quota truth").
    #[must_use]
    pub fn is_account_level(self) -> bool {
        matches!(self, ErrorCategory::BillingOrQuota)
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorCategory;

    #[test]
    fn transient_categories_are_safely_retryable() {
        assert!(ErrorCategory::RateLimited.is_safely_retryable());
        assert!(ErrorCategory::Timeout.is_safely_retryable());
        assert!(ErrorCategory::ProviderUnavailable.is_safely_retryable());
        assert!(ErrorCategory::TransportFailure.is_safely_retryable());
    }

    #[test]
    fn invalid_request_is_never_retried_blindly() {
        assert!(!ErrorCategory::InvalidRequest.is_safely_retryable());
        assert!(!ErrorCategory::ContextLimit.is_safely_retryable());
        assert!(!ErrorCategory::ModelUnavailable.is_safely_retryable());
    }

    #[test]
    fn auth_failures_quarantine_the_credential() {
        assert!(ErrorCategory::InvalidCredential.should_quarantine_credential());
        assert!(ErrorCategory::PermissionDenied.should_quarantine_credential());
        assert!(!ErrorCategory::RateLimited.should_quarantine_credential());
    }

    #[test]
    fn billing_is_account_level_but_not_a_credential_fault() {
        assert!(ErrorCategory::BillingOrQuota.is_account_level());
        assert!(!ErrorCategory::BillingOrQuota.should_quarantine_credential());
    }
}
