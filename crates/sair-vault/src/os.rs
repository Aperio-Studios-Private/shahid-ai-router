//! Keyring-backed OS secret store (Windows Credential Manager / macOS
//! Keychain) via the keyring 4.x v1 API (ADR-005; API verified by the Phase 2
//! plan review against docs.rs). Enabled by the `os` feature.

use keyring::v1::Entry;
use ring::rand::SecureRandom;

use crate::refs::credential_ref;
use crate::store::{SecretRef, SecretStore, VaultError};

const REF_ACCOUNT_PREFIX: &str = "sair.";

/// OS-backed store. The service name namespaces all entries; the default is
/// the application service. Test builds may use an isolated service name.
pub struct OsSecretStore {
    service: String,
}

impl OsSecretStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            service: "shahid-ai-router".to_string(),
        }
    }

    #[must_use]
    pub fn with_service(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }

    /// Probes OS credential store availability for degraded-start decisions
    /// (`Entry::store_status` reflects the platform store initialization).
    ///
    /// # Errors
    /// Returns [`VaultError::PlatformUnavailable`] when no default platform
    /// store can be initialized.
    pub fn availability() -> Result<(), VaultError> {
        match Entry::store_status().as_ref() {
            Ok(()) => Ok(()),
            Err(err) => Err(map_keyring_error(err)),
        }
    }

    fn entry_for_ref(&self, secret_ref: &SecretRef) -> Result<Entry, VaultError> {
        let account = secret_ref
            .as_str()
            .strip_prefix(REF_ACCOUNT_PREFIX)
            .ok_or(VaultError::InvalidRef)?;
        Entry::new(&self.service, account).map_err(|e| map_keyring_error(&e))
    }
}

impl Default for OsSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

fn map_keyring_error(err: &keyring::Error) -> VaultError {
    match err {
        keyring::Error::NoEntry => VaultError::NotFound,
        keyring::Error::NoDefaultStore | keyring::Error::PlatformFailure(_) => {
            VaultError::PlatformUnavailable
        }
        other => VaultError::Backend(other.to_string()),
    }
}

fn random_hex() -> String {
    let rng = ring::rand::SystemRandom::new();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes)
        .expect("OS CSPRNG must not fail; failure here is unrecoverable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl SecretStore for OsSecretStore {
    fn store(&self, secret: &[u8]) -> Result<SecretRef, VaultError> {
        let id_hex = random_hex();
        let entry = Entry::new(&self.service, &format!("cred.{id_hex}"))
            .map_err(|e| map_keyring_error(&e))?;
        entry
            .set_secret(secret)
            .map_err(|e| map_keyring_error(&e))?;
        credential_ref(&id_hex)
    }

    fn retrieve(&self, secret_ref: &SecretRef) -> Result<Vec<u8>, VaultError> {
        // entry_for_ref enforces the canonical `sair.` prefix (InvalidRef).
        self.entry_for_ref(secret_ref)?
            .get_secret()
            .map_err(|e| map_keyring_error(&e))
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), VaultError> {
        self.entry_for_ref(secret_ref)?
            .delete_credential()
            .map_err(|e| map_keyring_error(&e))
    }
}

#[cfg(any(windows, target_os = "macos"))]
#[cfg(test)]
mod tests {
    use super::OsSecretStore;
    use crate::refs::{CREDENTIAL_REF_PREFIX, credential_ref};
    use crate::store::contract;
    use crate::store::{SecretRef, SecretStore, VaultError};

    fn test_store() -> OsSecretStore {
        OsSecretStore::with_service("shahid-ai-router-tests")
    }

    #[test]
    fn availability_probe_returns() {
        // Either available or an explicit PlatformUnavailable; both are
        // contract-valid outcomes for the probe itself.
        match OsSecretStore::availability() {
            Ok(()) | Err(VaultError::PlatformUnavailable) => {}
            Err(other) => panic!("unexpected availability error: {other}"),
        }
    }

    #[test]
    fn os_store_passes_contract() {
        // Executes against the real OS credential store (Windows Credential
        // Manager on Windows, Keychain on macOS). Entries are created under
        // the dedicated test service and deleted by the contract cleanup.
        contract::run_contract(&test_store());
    }

    #[test]
    fn store_delete_credential_ref_roundtrip() {
        let store = test_store();
        let secret = b"sk-live-0000000000000000";
        let secret_ref = store.store(secret).unwrap();
        assert!(secret_ref.as_str().starts_with(CREDENTIAL_REF_PREFIX));
        assert_eq!(store.retrieve(&secret_ref).unwrap(), secret);
        store.delete(&secret_ref).unwrap();
        assert!(matches!(
            store.retrieve(&secret_ref),
            Err(VaultError::NotFound)
        ));
    }

    #[test]
    fn malformed_refs_are_rejected_before_touching_the_store() {
        let store = test_store();
        let malformed = SecretRef::parse("not-canonical").unwrap();
        // "not-canonical" parses but has no `sair.` prefix -> InvalidRef.
        assert!(matches!(
            store.retrieve(&malformed),
            Err(VaultError::InvalidRef)
        ));
        assert!(credential_ref("bad:id").is_err());
    }
}
