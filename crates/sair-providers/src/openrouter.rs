//! OpenRouter attribution headers (docs/04; docs re-verified 2026-10-31 —
//! the documented pair is `HTTP-Referer` + `X-Title`, review finding F-3).
//!
//! Attribution is opt-in configuration, sent ONLY to providers dispatched as
//! `openrouter` (F-1: never leaked to other upstreams). Values are validated
//! header-safe; invalid configured values fail open — the header is skipped
//! and a warning logged (F-5).

use serde::Deserialize;

use crate::openai::is_header_safe;

/// Optional attribution configuration (from app_settings).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AttributionConfig {
    pub referer: Option<String>,
    pub title: Option<String>,
}

impl AttributionConfig {
    /// Parses the configuration from an arbitrary JSON value (app_settings).
    #[must_use]
    pub fn from_value(value: Option<&serde_json::Value>) -> Option<Self> {
        let value = value?;
        serde_json::from_value(value.clone()).ok()
    }

    /// Builds the header pair for OpenRouter. Returns only header-safe
    /// values; invalid entries are dropped (fail-open, R4/F-5).
    #[must_use]
    pub fn headers(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        if let Some(referer) = self.referer.as_deref().filter(|r| {
            is_header_safe(r) && (r.starts_with("https://") || r.starts_with("http://"))
        }) {
            headers.push(("HTTP-Referer".to_string(), referer.to_string()));
        }
        if let Some(title) = self
            .title
            .as_deref()
            .filter(|t| is_header_safe(t) && t.chars().count() <= 128)
        {
            headers.push(("X-Title".to_string(), title.to_string()));
        }
        headers
    }
}

#[cfg(test)]
mod tests {
    use super::AttributionConfig;
    use serde_json::json;

    #[test]
    fn headers_use_the_documented_pair() {
        let config = AttributionConfig {
            referer: Some("https://myapp.example".to_string()),
            title: Some("My App".to_string()),
        };
        let headers = config.headers();
        assert_eq!(
            headers,
            vec![
                (
                    "HTTP-Referer".to_string(),
                    "https://myapp.example".to_string()
                ),
                ("X-Title".to_string(), "My App".to_string()),
            ]
        );
    }

    #[test]
    fn invalid_values_fail_open() {
        let config = AttributionConfig {
            referer: Some("ftp://not-http".to_string()),
            title: Some("bad\r\nheader-injection".to_string()),
        };
        assert!(
            config.headers().is_empty(),
            "invalid values must be dropped"
        );
    }

    #[test]
    fn from_value_is_tolerant() {
        assert!(AttributionConfig::from_value(None).is_none());
        assert!(AttributionConfig::from_value(Some(&json!("not an object"))).is_none());
        let config = AttributionConfig::from_value(Some(&json!({"referer": "https://x.example"})));
        assert!(config.is_some());
    }
}
