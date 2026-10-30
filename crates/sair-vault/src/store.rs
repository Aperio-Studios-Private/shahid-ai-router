//! Secret store trait and reference type. Implementations live in `mock`
//! (test-only) and `os` (keyring-backed).

use thiserror::Error;

/// Opaque reference to a secret held in the OS vault. Safe to persist in the
/// database; it conveys nothing about the secret value itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretRef(String);

impl SecretRef {
    /// Creates a reference from a string previously produced by
    /// [`SecretStore::store`]. Validation of existence happens on retrieve.
    /// Canonical formats are built via `crate::refs` (review finding F-4).
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
/// by the mock store.
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

#[cfg(test)]
pub(crate) mod contract {
    //! Shared contract tests. Every [`SecretStore`](super::SecretStore)
    //! implementation must pass all of them (phase2-plan P2-T02; ADR-016).

    use super::{SecretRef, SecretStore, VaultError};

    /// Runs the full contract against `store`. `make_ref` turns a fresh
    /// account name into a ref the store accepts.
    pub fn run_contract<S: SecretStore>(store: &S) {
        // Roundtrip (ASCII plus non-ASCII bytes through UTF-8).
        let secret = "sk-test-0123456789-unicode-✓".as_bytes();
        let secret_ref = store.store(secret).expect("store succeeds");
        assert_ne!(secret_ref.as_str(), "");
        let retrieved = store.retrieve(&secret_ref).expect("retrieve succeeds");
        assert_eq!(retrieved, secret);

        // Overwrite semantics: storing again under a new ref does not affect
        // the first item; each ref is independent.
        let second = store.store(b"second-value").expect("second store succeeds");
        assert_ne!(second, secret_ref);
        assert_eq!(store.retrieve(&secret_ref).unwrap(), secret);
        assert_eq!(store.retrieve(&second).unwrap(), b"second-value");

        // Delete removes the item; retrieving after delete is NotFound.
        store.delete(&second).expect("delete succeeds");
        assert!(matches!(store.retrieve(&second), Err(VaultError::NotFound)));

        // Deleting an unknown ref is NotFound (idempotent cleanup).
        let unknown = SecretRef::parse("sair.cred.deadbeefdeadbeef").unwrap();
        assert!(matches!(store.delete(&unknown), Err(VaultError::NotFound)));

        // Empty secrets are permitted (e.g. placeholder rotation states).
        let empty_ref = store.store(b"").expect("empty secret stores");
        assert_eq!(store.retrieve(&empty_ref).unwrap(), Vec::<u8>::new());
        store.delete(&empty_ref).expect("empty secret deletes");

        // Cleanup: remove the first item so tests leave nothing behind.
        store.delete(&secret_ref).expect("cleanup delete succeeds");
    }
}
