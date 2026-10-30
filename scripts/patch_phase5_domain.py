import io

# ---- domain routing.rs: CapabilityRequirements + provider_type on candidate ----
p = 'crates/sair-domain/src/routing.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''use serde::Serialize;

use crate::protocol::ProtocolFamily;''', '''use serde::Serialize;
use serde_json::Value;

use crate::protocol::ProtocolFamily;''')
src = src.replace('''/// One routable (deployment, credential) pair (ADR-018 composition).''', '''/// Capability requirements derived from the request body (FR-007; Phase 5).
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
        if let Some(brand) = body.get("response_format").and_then(|rf| rf.get("type")) {
            if let Some(brand_str) = brand.as_str() {
                // F-2: only structured brands require the capability; "text"
                // and unknown brands fail open.
                if matches!(brand_str, "json_object" | "json_schema") {
                    requirements.structured_output = true;
                }
            }
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

/// One routable (deployment, credential) pair (ADR-018 composition).''')
src = src.replace('''    pub credential_account_id: Option<String>,''', '''    pub credential_account_id: Option<String>,
    /// Provider kind (Phase 5 dispatch: attribution headers etc.).
    pub provider_type: String,''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('domain patched')

# ---- repos: select p.provider_type, wire candidate ----
p = 'crates/sair-persistence/src/repos.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''                d.endpoint_override, d.enabled AS deployment_enabled, p.enabled AS provider_enabled,
                p.name AS provider_label,''', '''                d.endpoint_override, d.enabled AS deployment_enabled, p.enabled AS provider_enabled,
                p.name AS provider_label, p.provider_type,''')
src = src.replace('''struct RawRow {
    deployment_id: String,
    provider_id: String,''', '''struct RawRow {
    deployment_id: String,
    provider_id: String,
    provider_type: String,''')
src = src.replace('''                provider_id: row.get("provider_id"),
                provider_label: row.get("provider_label"),''', '''                provider_id: row.get("provider_id"),
                provider_label: row.get("provider_label"),
                provider_type: row.get("provider_type"),''')
src = src.replace('''            credential_account_id: row.account_id.clone(),
            alias_priority: row.alias_priority,''', '''            credential_account_id: row.account_id.clone(),
            provider_type: row.provider_type.clone(),
            alias_priority: row.alias_priority,''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('repos patched')
