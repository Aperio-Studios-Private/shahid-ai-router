//! Test-only in-memory store. Never wire this into a release build — the
//! `compile_error!` guard in `lib.rs` enforces that (review finding F-10).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::refs::credential_ref;
use crate::store::{SecretRef, SecretStore, VaultError};

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

    fn next_ref(&self) -> Result<SecretRef, VaultError> {
        let mut counter = self
            .counter
            .lock()
            .map_err(|_| VaultError::Backend("poisoned test store".into()))?;
        *counter += 1;
        credential_ref(&format!("{:032x}", *counter))
    }

    fn lock_items(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<SecretRef, Vec<u8>>>, VaultError> {
        self.items
            .lock()
            .map_err(|_| VaultError::Backend("poisoned test store".into()))
    }
}

impl SecretStore for InMemorySecretStore {
    fn store(&self, secret: &[u8]) -> Result<SecretRef, VaultError> {
        let secret_ref = self.next_ref()?;
        self.lock_items()?
            .insert(secret_ref.clone(), secret.to_vec());
        Ok(secret_ref)
    }

    fn retrieve(&self, secret_ref: &SecretRef) -> Result<Vec<u8>, VaultError> {
        self.lock_items()?
            .get(secret_ref)
            .cloned()
            .ok_or(VaultError::NotFound)
    }

    fn delete(&self, secret_ref: &SecretRef) -> Result<(), VaultError> {
        self.lock_items()?
            .remove(secret_ref)
            .map(|_| ())
            .ok_or(VaultError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use crate::store::contract;
    use crate::store::{SecretStore, VaultError};
    use crate::{InMemorySecretStore, refs::gateway_token_ref};

    #[test]
    fn in_memory_store_passes_contract() {
        contract::run_contract(&InMemorySecretStore::new());
    }

    #[test]
    fn unknown_ref_retrieve_is_not_found() {
        let store = InMemorySecretStore::new();
        let unknown = gateway_token_ref().unwrap();
        assert!(matches!(
            store.retrieve(&unknown),
            Err(VaultError::NotFound)
        ));
    }
}
