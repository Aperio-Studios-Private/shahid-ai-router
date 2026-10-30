//! Capability metadata and routing-decision types (FR-004/007/012, ADR-009).

use serde::Serialize;

use crate::protocol::ProtocolFamily;

/// What a deployment can do, as declared by its adapter and model row.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CapabilityMetadata {
    pub protocol_families: Vec<ProtocolFamily>,
    pub streaming: bool,
    pub tool_calling: bool,
    pub vision: bool,
    pub structured_output: bool,
    pub context_window: Option<i64>,
}

impl CapabilityMetadata {
    #[must_use]
    pub fn supports_protocol(&self, family: ProtocolFamily) -> bool {
        self.protocol_families.contains(&family)
    }
}

/// Routing strategy (docs/05). `lru`/`manual_pin` are declared for schema
/// compatibility but rejected at runtime until implemented (ADR-018).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Priority,
    RoundRobin,
    Weighted,
    Lru,
    ManualPin,
}

impl Strategy {
    /// # Errors
    /// Returns the strategy name when it is declared but not implemented.
    pub fn ensure_implemented(self) -> Result<Self, UnimplementedStrategy> {
        match self {
            Strategy::Priority | Strategy::RoundRobin | Strategy::Weighted => Ok(self),
            Strategy::Lru => Err(UnimplementedStrategy("lru".to_string())),
            Strategy::ManualPin => Err(UnimplementedStrategy("manual_pin".to_string())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("routing strategy is declared but not implemented: {0}")]
pub struct UnimplementedStrategy(pub String);

/// One routable (deployment, credential) pair (ADR-018 composition).
#[derive(Debug, Clone, Serialize)]
pub struct RouteCandidate {
    pub deployment_id: String,
    pub provider_id: String,
    pub provider_label: String,
    pub model_id: String,
    pub base_url: String,
    pub allow_loopback: bool,
    pub endpoint_override: Option<String>,
    pub protocol_family: ProtocolFamily,
    pub capabilities: CapabilityMetadata,
    pub credential_id: String,
    pub credential_secret_ref: String,
    /// Ordering: alias-deployment priority (higher first).
    pub alias_priority: i64,
    /// Ordering: credential priority (higher first, within a deployment).
    pub credential_priority: i64,
    pub credential_weight: i64,
}

impl RouteCandidate {
    /// Lexicographic ordering key per ADR-018.
    #[must_use]
    pub fn order_key(&self) -> (i64, i64) {
        (self.alias_priority, self.credential_priority)
    }
}

/// Why a candidate was excluded (FR-012: explain unavailable deployments).
#[derive(Debug, Clone, Serialize)]
pub struct Exclusion {
    pub deployment_id: String,
    pub credential_id: String,
    pub reason: String,
}

/// The routing outcome, persisted with the request record (FR-012).
#[derive(Debug, Clone, Serialize)]
pub struct RoutingDecision {
    pub alias: String,
    pub strategy: Strategy,
    pub selected: Option<RouteCandidate>,
    /// Ordered candidate list (post-strategy); the handler iterates this for
    /// rotation within the attempt budget.
    pub ordered: Vec<RouteCandidate>,
    pub exclusions: Vec<Exclusion>,
}
