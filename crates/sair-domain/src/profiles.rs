//! Agent profile generators (FR-013, docs/09; Phase 8).
//!
//! Pure functions: no IO, no token material. Generated configs ALWAYS carry
//! the `<<GATEWAY_TOKEN>>` placeholder (Phase 8 review F-3/F-5 — the token
//! only ever exists in memory when the user explicitly reveals/copies).
//!
//! Supported v1: Claude Code, Gemini CLI, generic OpenAI-compatible clients.
//! **OpenAI Codex CLI is documented-UNSUPPORTED in v1** (ADR-010 amendment:
//! current Codex removed `wire_api="chat"`; it requires `/v1/responses`,
//! which is out of scope v1) — a limitation profile is generated instead
//! so the UI can state exactly why.

use serde::Serialize;

/// Placeholder substituted by the UI when the user explicitly reveals the
/// token for pasting.
pub const TOKEN_PLACEHOLDER: &str = "<<GATEWAY_TOKEN>>";

/// Language for the UI (docs/09 F-5): never claim "supported".
pub const VERIFICATION_PREFIX: &str = "contract-tested";

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedProfile {
    pub agent_id: &'static str,
    /// The copyable config (placeholder, never a real token).
    pub config_text: String,
    /// Absolute paths this profile would touch when applied.
    pub files: Vec<String>,
    /// Rollback instructions (docs/09).
    pub rollback: String,
    /// Per-agent limitations, verbatim for the UI.
    pub limitations: String,
    /// Whether a config file application is available at all.
    pub applicable: bool,
}

fn claude_code_profile(port: u16, alias: String, fast_alias: Option<String>) -> GeneratedProfile {
    // JSON-escape user-supplied aliases so a quote/backslash/newline cannot
    // produce an invalid settings.json (audit P2 fix).
    let alias_json = serde_json::to_string(&alias).unwrap_or_else(|_| "\"\"".to_string());
    let fast = fast_alias
        .map(|a| {
            let a_json = serde_json::to_string(&a).unwrap_or_else(|_| "\"\"".to_string());
            format!(",\n    \"ANTHROPIC_DEFAULT_HAIKU_MODEL\": {a_json}")
        })
        .unwrap_or_default();
    let config = format!(
        "{{\n    \"env\": {{\n    \"ANTHROPIC_BASE_URL\": \"http://127.0.0.1:{port}\",\n    \"ANTHROPIC_AUTH_TOKEN\": \"{TOKEN_PLACEHOLDER}\",\n    \"ANTHROPIC_MODEL\": {alias_json}{fast}\n    }}\n}}\n"
    );
    GeneratedProfile {
        agent_id: "claude-code",
        config_text: config,
        files: vec!["~/.claude/settings.json".to_string()],
        rollback: "Restore the backup file written next to settings.json before the edit \
                   (shahid-backup-<timestamp>), or remove the `env` block entries \
                   ANTHROPIC_BASE_URL / ANTHROPIC_AUTH_TOKEN / ANTHROPIC_MODEL manually."
            .to_string(),
        limitations: "Contract-tested against this gateway's /v1/messages + count_tokens \
                      (non-streaming and streaming). The real Claude Code CLI was not executed \
                      in this environment; verify interactively before production use."
            .to_string(),
        applicable: true,
    }
}

fn gemini_cli_profile(port: u16, _alias: String) -> GeneratedProfile {
    // The alias is bound client-side via the model selector; the env file
    // carries endpoint + key only.
    let config = format!(
        "GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:{port}\nGEMINI_API_KEY={TOKEN_PLACEHOLDER}\n"
    );
    GeneratedProfile {
        agent_id: "gemini-cli",
        config_text: config,
        // F-4: the CLI reads env vars from ~/.gemini/.env (no settings.json
        // base-URL field exists).
        files: vec!["~/.gemini/.env".to_string()],
        rollback: "Restore the backup of ~/.gemini/.env written next to it before the edit \
                   (shahid-backup-<timestamp>), or remove the GOOGLE_GEMINI_BASE_URL / \
                   GEMINI_API_KEY lines manually."
            .to_string(),
        limitations: "Contract-tested against /v1beta generateContent (+streamGenerateContent). \
                      The CLI sends x-goog-api-key (header form) — accepted; the ?key= query \
                      form is rejected by the gateway. gemini-api-key auth mode only; the real \
                      Gemini CLI was not executed in this environment."
            .to_string(),
        applicable: true,
    }
}

fn generic_openai_profile(port: u16, alias: String) -> GeneratedProfile {
    // Escape for both the snippet's quoted strings and the shell example.
    let alias_safe = alias.replace('\\', "\\\\").replace('"', "\\\"");
    let config = format!(
        "# Generic OpenAI-compatible client\nbase_url = \"http://127.0.0.1:{port}/v1\"\napi_key = \"{TOKEN_PLACEHOLDER}\" # sent as Authorization: Bearer\nmodel = \"{alias_safe}\"\n\n# curl:\n# curl http://127.0.0.1:{port}/v1/chat/completions \\\n#   -H \"Authorization: Bearer <<GATEWAY_TOKEN>>\" \\\n#   -d '{{\"model\":\"{alias_safe}\",\"messages\":[{{\"role\":\"user\",\"content\":\"hi\"}}]}}'\n"
    );
    GeneratedProfile {
        agent_id: "generic-openai",
        config_text: config,
        files: Vec::new(), // no external file for the generic profile
        rollback: "No file is modified by this profile; discard the copied snippet.".to_string(),
        limitations: "Contract-tested against /v1/chat/completions + /v1/models (non-streaming \
                      and streaming). Any OpenAI-compatible client that honors a custom \
                      base_url works; specific third-party tools are not executed in this \
                      environment."
            .to_string(),
        applicable: false,
    }
}

fn codex_unsupported_profile() -> GeneratedProfile {
    GeneratedProfile {
        agent_id: "codex-cli",
        config_text: String::new(),
        files: Vec::new(),
        rollback: "Nothing to roll back: this gateway does not generate Codex configuration."
            .to_string(),
        limitations: "NOT SUPPORTED in v1: current Codex CLI requires the /v1/responses wire \
                      API (the chat wire API was removed from Codex and /v1/responses is out \
                      of scope for this gateway in v1). Revisit post-v1."
            .to_string(),
        applicable: false,
    }
}

/// Generates the profile for an agent. `supported_agents` documents what v1
/// ships; unknown ids are rejected.
#[must_use]
pub fn generate_profile(
    agent_id: &str,
    port: u16,
    alias: String,
    fast_alias: Option<String>,
) -> Option<GeneratedProfile> {
    match agent_id {
        "claude-code" => Some(claude_code_profile(port, alias, fast_alias)),
        "gemini-cli" => Some(gemini_cli_profile(port, alias)),
        "generic-openai" => Some(generic_openai_profile(port, alias)),
        "codex-cli" => Some(codex_unsupported_profile()),
        _ => None,
    }
}

/// The v1 agent catalogue for the UI (docs/09: select agent first).
pub const SUPPORTED_AGENTS: &[&str] = &["claude-code", "gemini-cli", "generic-openai", "codex-cli"];

#[cfg(test)]
mod tests {
    use super::{SUPPORTED_AGENTS, TOKEN_PLACEHOLDER, generate_profile};

    #[test]
    fn claude_code_config_is_valid_json_with_placeholder() {
        let profile =
            generate_profile("claude-code", 8787, "fast-model".to_string(), None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&profile.config_text).unwrap();
        assert_eq!(value["env"]["ANTHROPIC_BASE_URL"], "http://127.0.0.1:8787");
        assert_eq!(value["env"]["ANTHROPIC_AUTH_TOKEN"], TOKEN_PLACEHOLDER);
        assert_eq!(value["env"]["ANTHROPIC_MODEL"], "fast-model");
        assert!(!profile.config_text.contains("sk-"));
    }

    #[test]
    fn claude_code_fast_alias_uses_current_env_name() {
        let profile = generate_profile(
            "claude-code",
            8787,
            "main".to_string(),
            Some("quick".to_string()),
        )
        .unwrap();
        // F-6: ANTHROPIC_DEFAULT_HAIKU_MODEL (SMALL_FAST_MODEL deprecated).
        assert!(
            profile
                .config_text
                .contains("ANTHROPIC_DEFAULT_HAIKU_MODEL"),
            "{}",
            profile.config_text
        );
        assert!(!profile.config_text.contains("SMALL_FAST_MODEL"));
    }

    #[test]
    fn gemini_base_url_is_bare_and_env_file_only() {
        let profile =
            generate_profile("gemini-cli", 8787, "gemini-alias".to_string(), None).unwrap();
        // F-4: bare base URL (the CLI appends v1beta itself).
        assert!(
            profile
                .config_text
                .contains("GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:8787\n")
        );
        assert!(!profile.config_text.contains("8787/v1beta"));
        assert_eq!(profile.files, vec!["~/.gemini/.env".to_string()]);
    }

    #[test]
    fn codex_is_documented_unsupported() {
        let profile = generate_profile("codex-cli", 8787, "x".to_string(), None).unwrap();
        assert!(!profile.applicable);
        assert!(profile.limitations.contains("NOT SUPPORTED"));
        assert!(profile.config_text.is_empty());
    }

    #[test]
    fn no_token_material_in_any_profile() {
        for agent in SUPPORTED_AGENTS {
            let profile = generate_profile(agent, 8787, "alias".to_string(), None).unwrap();
            assert!(!profile.config_text.contains("sk-live"));
            assert!(
                profile.config_text.contains(TOKEN_PLACEHOLDER) || profile.config_text.is_empty()
            );
            assert!(!profile.rollback.is_empty());
        }
    }

    #[test]
    fn unknown_agent_rejected() {
        assert!(generate_profile("unknown-agent", 8787, "a".to_string(), None).is_none());
    }
}
