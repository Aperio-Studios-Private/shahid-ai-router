//! Secret store trait, reference format, and implementations.

use std::collections::HashMap;
use std::sync::Mutex;

use thiserror::Error;

/// Opaque reference to a secret held in the OS vault. Safe to persist in the
/// database; it conveys nothing about the secret value itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef(String);

impl SecretRef {
    /// Creates a reference from a string previously produced by
    /// [`SecretStore::store`]. Validation of existence happens on retrieve.
    ///
    /// # Errors
    /// Returns [`VaultError::InvalidRef`] for empty or malformed input.
    pub fn parse(raw: &str) -> Result<Self, VaultError> {
        if raw.is_empty()
            || raw.len() > 256
            || !raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        {
            return Err(VaultError::InvalidRef);
        }
        Ok(Self(raw.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Intentionally non-revealing: the ref is opaque and short.
        write!(f, "secret_ref({})", self.0)
    }
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("secret reference is malformed")]
    InvalidRef,
    #[error("secret not found in vault")]
    NotFound,
    #[error("OS credential store is unavailable")]
    PlatformUnavailable,
    #[error("vault backend failure: {0}")]
    Backend(String),
}

/// Abstraction over the OS secure store. Implemented by the keyring-backed
/// store (Windows Credential Manager / macOS Keychain) and, for tests only,
/// by [`InMemorySecretStore`].
pub trait SecretStore: Send + Sync {
    /// Stores the secret and returns the opaque reference.
    ///
    /// # Errors
    /// See [`VaultError`].
    fn store(&self, secret: &[u8]) -> Result<SecretRef, VaultError>;

    /// Retrieves the secret for an outbound request.
    ///
    /// # Errors
    /// See [`VaultError`].
    fn retrieve(&self, secret_ref: &SecretRef) -> Result<Vec<u8>, VaultError>;

    /// Deletes the vault item (credential deletion / rotation cleanup).
    ///
    /// # Errors
    /// See [`VaultError`].
    fn delete(&self, secret_ref: &SecretRef) -> Result<(), VaultError>;
}

/// Test-only store. Never wire this into a release build (see crate docs).
#[derive(Default)]
pub struct InMemorySecretStore {
    items: Mutex<HashMap<SecretRef, Vec<u8>>>,
    counter: Mutex<u64>,
}

impl InMemorySecretStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for InMemorySecretStore {
    fn store(&self, secret: &[u8]) -> Result<SecretRef, VaultError> {
        let mut counter = self
            .counter
            .lock()
            .map_err(|_| VaultError::Backend("poisoned test store".into()))?;
        *counter += 1;
        let id = format!("mock-{counter:016}");
        let secret_ref = SecretRef::parse(&id)?;
        self.items
            .lock()
            .map_err(|_| VaultError::Backend("poisoned test store".into()))?
            .insert(secret_ref.clone(), secret.to_vec());
        Ok(secret_ref)
    }

    fn retrieve(&self, secret_ref: &SecretRef) -> Result<Vec<u8>, VaultError> {
        self.items
            .lock()
            .map_err(|_| VaultError::Backend("poisoned test store".into()))?
            .get(secret_ref)
            .cloned()
            .ok_or(VaultError::NotFound)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), VaultError> {
        self.items
            .lock()
            .map_err(|_| VaultError::Backend("poisoned test store".into()))?
            .remove(secret_ref)
            .map(|_| ())
            .ok_or(VaultError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemorySecretStore, SecretRef, SecretStore, VaultError};

    #[test]
    fn store_retrieve_delete_roundtrip() {
        let store = InMemorySecretStore::new();
        let secret = b"sk-test-0123456789";
        let secret_ref = store.store(secret).unwrap();
        assert_ne!(secret_ref.as_str(), "");

        let retrieved = store.retrieve(&secret_ref).unwrap();
        assert_eq!(retrieved, secret);

        store.delete(&secret_ref).unwrap();
        assert!(matches!(
            store.retrieve(&secret_ref),
            Err(VaultError::NotFound)
        ));
    }

    #[test]
    fn refs_are_opaque_and_validated() {
        assert!(SecretRef::parse("").is_err());
        assert!(SecretRef::parse("has spaces").is_err());
        assert!(SecretRef::parse("mock-0000000000000001").is_ok());
    }

    #[test]
    fn deleting_unknown_ref_is_not_found() {
        let store = InMemorySecretStore::new();
        let unknown = SecretRef::parse("mock-doesnotexist").unwrap();
        assert!(matches!(store.delete(&unknown), Err(VaultError::NotFound)));
    }
}
