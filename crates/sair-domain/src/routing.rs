//! Capability metadata and routing-decision types (FR-004/007/012, ADR-009).

use serde::Serialize;
use serde_json::Value;

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

/// Capability requirements derived from the request body (FR-007; Phase 5).
/// Explicit signals only: empty/nonexistent `tools` imposes no requirement;
/// `response_format` type `text`/unknown fails open (review F-2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CapabilityRequirements {
    pub tool_calling: bool,
    pub structured_output: bool,
}

impl CapabilityRequirements {
    /// Derives requirements from a chat-completions request body.
    #[must_use]
    pub fn derive_from_body(body: &Value) -> Self {
        let mut requirements = Self::default();
        let tools_required = body
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| !tools.is_empty());
        requirements.tool_calling = tools_required;
        // F-2: only structured brands require the capability; "text" and
        // unknown brands fail open.
        if let Some("json_object" | "json_schema") = body
            .get("response_format")
            .and_then(|rf| rf.get("type"))
            .and_then(|brand| brand.as_str())
        {
            requirements.structured_output = true;
        }
        // Phase 6 (F-6): Gemini structured-output signals.
        let generation_config = body.get("generationConfig");
        if generation_config
            .and_then(|g| g.get("responseMimeType"))
            .and_then(Value::as_str)
            == Some("application/json")
            || generation_config
                .and_then(|g| g.get("responseSchema"))
                .is_some()
        {
            requirements.structured_output = true;
        }
        requirements
    }

    /// First failing capability check for a candidate, if any (F-6: one
    /// reason per candidate).
    #[must_use]
    pub fn first_unmet(&self, capabilities: &CapabilityMetadata) -> Option<&'static str> {
        if self.tool_calling && !capabilities.tool_calling {
            return Some("capability_tool_calling");
        }
        if self.structured_output && !capabilities.structured_output {
            return Some("capability_structured_output");
        }
        None
    }
}

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
    /// Account group (docs/05 Quota truth): BillingOrQuota cools the group.
    pub credential_account_id: Option<String>,
    /// Provider kind (Phase 5 dispatch: attribution headers etc.).
    pub provider_type: String,
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
