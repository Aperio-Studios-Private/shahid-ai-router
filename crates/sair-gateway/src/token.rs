//! Gateway token lifecycle (FR-019, ADR-011, review findings F-5/F-10).
//!
//! The token is 32 OS-CSPRNG bytes, base64url-encoded (43 chars). It lives in
//! the OS vault; the database stores the opaque ref only. Verification is
//! length-checked then constant-time.

use base64::Engine;
use ring::rand::SecureRandom;
use sair_vault::{SecretRef, SecretStore, VaultError};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::GatewayError;

/// Fixed base64url length of a 32-byte token (public information).
pub const TOKEN_B64_LEN: usize = 43;

pub fn generate_token_bytes() -> Result<Vec<u8>, GatewayError> {
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .map_err(|_| GatewayError::Token("OS CSPRNG failure".to_string()))?;
    Ok(bytes.to_vec())
}

pub fn encode_token(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Constant-time verification. `expected` holds the encoded (base64url)
/// token bytes — the exact form clients send. Length is checked first
/// (length is public); the comparison itself is constant time (subtle).
#[must_use]
pub fn verify_token(provided: Option<&str>, expected: &[u8]) -> bool {
    let Some(provided) = provided else {
        return false;
    };
    if provided.len() != expected.len() {
        return false;
    }
    bool::from(provided.as_bytes().ct_eq(expected))
}

/// Creates and persists tokens in the OS vault. Regeneration uses a fresh
/// vault item each time; the app layer then swaps the persisted ref and
/// deletes the replaced item — the crash-safe order is create → persist ref
/// → delete old (review finding F-10).
pub struct TokenManager {
    store: std::sync::Arc<dyn SecretStore>,
}

pub struct TokenMaterial {
    pub secret_ref: SecretRef,
    /// The encoded token bytes (the exact form clients send). Zeroize-on-drop
    /// (audit P3 hardening): every in-memory copy is wiped when dropped.
    pub value: Zeroizing<Vec<u8>>,
}

impl TokenManager {
    #[must_use]
    pub fn new(store: std::sync::Arc<dyn SecretStore>) -> Self {
        Self { store }
    }

    /// Generates a fresh token, stores its **encoded** form (the exact bytes
    /// clients will send) under a new vault item, and returns its material.
    ///
    /// # Errors
    /// See [`GatewayError::Token`].
    pub fn create(&self) -> Result<TokenMaterial, GatewayError> {
        let raw = generate_token_bytes()?;
        let encoded = encode_token(&raw);
        let secret_ref = self
            .store
            .store(encoded.as_bytes())
            .map_err(|e| GatewayError::Token(e.to_string()))?;
        Ok(TokenMaterial {
            secret_ref,
            value: Zeroizing::new(encoded.into_bytes()),
        })
    }

    /// Loads the token value for the given ref (fail-closed on error —
    /// review finding F-9: the gateway must refuse to start without it).
    ///
    /// # Errors
    /// See [`GatewayError::Token`].
    pub fn load(&self, secret_ref: &SecretRef) -> Result<Zeroizing<Vec<u8>>, GatewayError> {
        self.store
            .retrieve(secret_ref)
            .map(Zeroizing::new)
            .map_err(|e| GatewayError::Token(e.to_string()))
    }

    /// Deletes the vault item for a replaced token.
    ///
    /// # Errors
    /// See [`GatewayError::Token`].
    pub fn delete(&self, secret_ref: &SecretRef) -> Result<(), GatewayError> {
        match self.store.delete(secret_ref) {
            Ok(()) | Err(VaultError::NotFound) => Ok(()),
            Err(e) => Err(GatewayError::Token(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TOKEN_B64_LEN, encode_token, generate_token_bytes, verify_token};
    use crate::GatewayError;

    #[test]
    fn generated_tokens_are_32_bytes_43_b64url_chars() {
        let bytes = generate_token_bytes().unwrap();
        assert_eq!(bytes.len(), 32);
        let encoded = encode_token(&bytes);
        assert_eq!(encoded.len(), TOKEN_B64_LEN);
        assert!(!encoded.contains('+') && !encoded.contains('/') && !encoded.contains('='));
    }

    #[test]
    fn verification_is_exact() {
        let raw = generate_token_bytes().unwrap();
        let encoded = encode_token(&raw);
        let expected = encoded.as_bytes();
        assert!(verify_token(Some(&encoded), expected));
        assert!(!verify_token(None, expected));
        assert!(!verify_token(Some(""), expected));
        assert!(!verify_token(Some(&"x".repeat(TOKEN_B64_LEN)), expected));
        // Wrong length early-out (length is public).
        assert!(!verify_token(Some("short"), expected));
        // Right length, wrong content.
        let wrong = "A".repeat(TOKEN_B64_LEN);
        assert!(!verify_token(Some(&wrong), expected));
    }

    #[test]
    fn create_persists_and_loads_roundtrip() {
        use sair_vault::InMemorySecretStore;
        let manager = super::TokenManager::new(std::sync::Arc::new(InMemorySecretStore::new()));
        let material = manager.create().unwrap();
        assert_eq!(material.value.len(), TOKEN_B64_LEN);
        let loaded = manager.load(&material.secret_ref).unwrap();
        assert_eq!(loaded, material.value);
        manager.delete(&material.secret_ref).unwrap();
        assert!(matches!(
            manager.load(&material.secret_ref),
            Err(GatewayError::Token(_))
        ));
    }
}
