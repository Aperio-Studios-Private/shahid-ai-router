//! OS credential vault abstraction (ADR-005).
//!
//! Contract (docs/06 "Secrets"):
//! - Secrets are written to the OS secure store and never persisted elsewhere.
//! - `store` returns an opaque [`SecretRef`] for database storage.
//! - Secrets are retrieved only when an outbound request needs them.
//! - `delete` removes the vault item when a credential is deleted.
//! - There is deliberately **no** filesystem/plaintext fallback. Release
//!   builds use the `os` implementation only; the `mock` store exists for
//!   tests and is blocked from release compilation by a `compile_error!` guard.
//!
//! Reference format (pinned by tests, review finding F-4): `sair.cred.<hex>`
//! for credentials and `sair.gateway-token` for the local gateway token.

pub mod refs;
pub mod secret_bytes;
pub mod store;

#[cfg(feature = "mock")]
pub mod mock;

#[cfg(feature = "os")]
pub mod os;

pub use refs::{credential_ref, gateway_token_ref};
pub use secret_bytes::SecretBytes;
pub use store::{SecretRef, SecretStore, VaultError};

#[cfg(feature = "mock")]
pub use mock::InMemorySecretStore;

#[cfg(feature = "os")]
pub use os::OsSecretStore;

/// Release-build guard (review finding F-10): a build with the test-only
/// mock store enabled must fail to compile instead of shipping.
#[cfg(all(feature = "mock", not(debug_assertions)))]
compile_error!("the mock secret store is test-only and must not be compiled into release builds");
