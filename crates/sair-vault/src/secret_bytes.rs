//! Owned secret buffer wiped on drop (docs/06 "clear buffers where
//! practical"; Phase 3 review finding R3-09).

use zeroize::Zeroize;

/// Wrap secret material so it is zeroized when it leaves scope. Access via
/// [`expose`](Self::expose) only at the last moment before use.
#[derive(Clone)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    #[must_use]
    pub fn new(value: Vec<u8>) -> Self {
        Self(value)
    }

    /// Borrows the secret. Keep the borrow short; never copy into logs.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// UTF-8 view for tokens stored as encoded text.
    ///
    /// # Errors
    /// Returns the length when the bytes are not valid UTF-8.
    pub fn expose_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never reveal length or content in Debug output.
        write!(f, "SecretBytes([REDACTED])")
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::SecretBytes;

    #[test]
    fn debug_never_reveals_content() {
        let secret = SecretBytes::new(b"sk-super-secret-value".to_vec());
        let rendered = format!("{secret:?}");
        assert!(!rendered.contains("sk-super-secret-value"));
        assert!(rendered.contains("REDACTED"));
    }

    #[test]
    fn expose_gives_short_lived_access() {
        let secret = SecretBytes::new(b"abc".to_vec());
        assert_eq!(secret.expose(), b"abc");
        assert_eq!(secret.len(), 3);
        assert!(!secret.is_empty());
    }
}
