//! OS credential vault abstraction (ADR-005).
//!
//! Contract (docs/06 "Secrets"):
//! - Secrets are written to the OS secure store and never persisted elsewhere.
//! - `store` returns an opaque `secret_ref` for database storage.
//! - Secrets are retrieved only when an outbound request needs them.
//! - `delete` removes the vault item when a credential is deleted.
//! - There is deliberately **no** filesystem/plaintext fallback. Release
//!   builds may only use the OS-backed implementation; [`InMemorySecretStore`]
//!   exists for tests and must never be wired into a release build.

pub mod store;

pub use store::{InMemorySecretStore, SecretRef, SecretStore, VaultError};
