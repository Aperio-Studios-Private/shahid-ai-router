//! Adapter trait and capability declarations.

use sair_domain::protocol::ProtocolFamily;
use serde::Serialize;

/// Stable identifier of a provider integration (e.g. `openai`, `openrouter`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ProviderId(String);

impl ProviderId {
    /// # Errors
    /// Returns an error when the id is empty, longer than 64 chars, or not
    /// lowercase alphanumeric/dash (stable identifiers for config + logs).
    pub fn new(raw: &str) -> Result<Self, ProviderError> {
        let ok = !raw.is_empty()
            && raw.len() <= 64
            && raw
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !ok {
            return Err(ProviderError::InvalidProviderId(raw.to_string()));
        }
        Ok(Self(raw.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("invalid provider id: {0:?}")]
    InvalidProviderId(String),
}

/// What an adapter (and thereby its deployments) can do. Declared statically
/// per provider, verified per request by the router.
#[derive(Debug, Clone, Serialize)]
pub struct AdapterCapabilities {
    /// Protocol families this adapter can speak natively (ADR-009).
    pub protocol_families: Vec<ProtocolFamily>,
    /// Server-sent-event streaming support per family.
    pub streaming: bool,
    /// Tool/function calling support.
    pub tool_calling: bool,
    /// Vision/multimodal input support.
    pub vision: bool,
    /// JSON/structured output support.
    pub structured_output: bool,
}

impl AdapterCapabilities {
    #[must_use]
    pub fn supports_protocol(&self, family: ProtocolFamily) -> bool {
        self.protocol_families.contains(&family)
    }
}

/// Contract implemented once per provider integration.
pub trait ProviderAdapter: Send + Sync {
    fn provider_id(&self) -> &ProviderId;

    fn capabilities(&self) -> &AdapterCapabilities;

    /// Human-readable summary for the UI and diagnostics; must never include
    /// credential material.
    fn describe(&self) -> String {
        format!("provider {} [{}]", self.provider_id().as_str(), "adapter")
    }
}

/// Deterministic mock used by router/gateway tests. Never listed in the
/// production provider registry.
pub struct MockProvider {
    id: ProviderId,
    capabilities: AdapterCapabilities,
}

impl MockProvider {
    #[must_use]
    pub fn new(id: ProviderId, capabilities: AdapterCapabilities) -> Self {
        Self { id, capabilities }
    }
}

impl ProviderAdapter for MockProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }

    fn capabilities(&self) -> &AdapterCapabilities {
        &self.capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::{AdapterCapabilities, MockProvider, ProviderAdapter, ProviderId};
    use sair_domain::protocol::ProtocolFamily;

    fn caps(families: Vec<ProtocolFamily>) -> AdapterCapabilities {
        AdapterCapabilities {
            protocol_families: families,
            streaming: false,
            tool_calling: false,
            vision: false,
            structured_output: false,
        }
    }

    #[test]
    fn provider_ids_are_constrained() {
        assert!(ProviderId::new("openai").is_ok());
        assert!(ProviderId::new("OpenAI").is_err());
        assert!(ProviderId::new("").is_err());
        assert!(ProviderId::new("has space").is_err());
    }

    #[test]
    fn capabilities_declare_protocol_support() {
        let provider = MockProvider::new(
            ProviderId::new("mock").unwrap(),
            caps(vec![ProtocolFamily::OpenAiChat]),
        );
        assert!(
            provider
                .capabilities()
                .supports_protocol(ProtocolFamily::OpenAiChat)
        );
        assert!(
            !provider
                .capabilities()
                .supports_protocol(ProtocolFamily::AnthropicMessages)
        );
        assert_eq!(provider.provider_id().as_str(), "mock");
    }
}
