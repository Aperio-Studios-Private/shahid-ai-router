//! Canonical secret-ref formats (review finding F-4). Vault and persistence
//! consumers must only create refs through these helpers; the pinned unit
//! tests in this module prevent drift.

use crate::store::{SecretRef, VaultError};

/// Prefix for credential secret refs in the database.
pub const CREDENTIAL_REF_PREFIX: &str = "sair.cred.";

/// Builds the canonical secret ref for a credential with the given random id
/// (16 random bytes from the OS CSPRNG, hex-encoded).
///
/// # Errors
/// Returns [`VaultError::InvalidRef`] when `id_hex` is empty or not hex.
pub fn credential_ref(id_hex: &str) -> Result<SecretRef, VaultError> {
    if id_hex.is_empty() || !id_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(VaultError::InvalidRef);
    }
    SecretRef::parse(&format!("{CREDENTIAL_REF_PREFIX}{id_hex}"))
}

/// Builds the canonical secret ref for the local gateway token (legacy
/// fixed-ref format; live token refs use fresh uuid items per ADR/F-10).
///
/// # Errors
/// Returns [`VaultError::InvalidRef`] if the composed ref is malformed
/// (cannot happen; the check pins the format).
pub fn gateway_token_ref() -> Result<SecretRef, VaultError> {
    SecretRef::parse("sair.gateway-token")
}

#[cfg(test)]
mod tests {
    use super::{CREDENTIAL_REF_PREFIX, credential_ref, gateway_token_ref};

    #[test]
    fn refs_match_validator_charset() {
        let r = credential_ref("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(r.as_str(), "sair.cred.0123456789abcdef0123456789abcdef");
        assert!(r.as_str().starts_with(CREDENTIAL_REF_PREFIX));

        let t = gateway_token_ref().unwrap();
        assert_eq!(t.as_str(), "sair.gateway-token");
    }

    #[test]
    fn refs_reject_bad_ids() {
        // The id portion must also satisfy the global validator charset;
        // colons/slashes from older drafts are rejected (F-4).
        assert!(credential_ref("bad:id").is_err());
        assert!(credential_ref("").is_err());
    }
}
