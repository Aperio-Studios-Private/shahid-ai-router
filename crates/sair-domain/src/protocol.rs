//! Protocol families (ADR-009): v1 routes same-protocol only, so a request
//! received on a surface may only be routed to deployments whose upstream
//! natively speaks that protocol family.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFamily {
    /// OpenAI Chat Completions wire format (`/v1/chat/completions`).
    OpenAiChat,
    /// Anthropic Messages wire format (`/v1/messages`).
    AnthropicMessages,
    /// Gemini generateContent wire format.
    GeminiGenerateContent,
}

impl ProtocolFamily {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ProtocolFamily::OpenAiChat => "openai_chat",
            ProtocolFamily::AnthropicMessages => "anthropic_messages",
            ProtocolFamily::GeminiGenerateContent => "gemini_generate_content",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProtocolFamily;

    #[test]
    fn names_are_stable_identifiers() {
        assert_eq!(ProtocolFamily::OpenAiChat.as_str(), "openai_chat");
        assert_eq!(
            ProtocolFamily::AnthropicMessages.as_str(),
            "anthropic_messages"
        );
        assert_eq!(
            ProtocolFamily::GeminiGenerateContent.as_str(),
            "gemini_generate_content"
        );
    }
}
