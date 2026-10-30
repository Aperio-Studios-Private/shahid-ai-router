//! Timestamp helpers. All persisted timestamps are ISO-8601 UTC text.

/// Current UTC time as ISO-8601.
#[must_use]
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::now_iso;

    #[test]
    fn now_is_rfc3339() {
        let value = now_iso();
        chrono::DateTime::parse_from_rfc3339(&value).unwrap();
    }
}
