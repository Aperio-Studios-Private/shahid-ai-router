//! Provider adapter contract (docs/04 "Adapter contract").
//!
//! Every adapter declares what it supports before any request is made. The
//! router uses these declarations for capability-aware selection (FR-007) and
//! same-protocol routing (ADR-009). Adapters never expose credential values;
//! they receive an opaque handle to fetch the secret from the vault at call
//! time (Phase 3).

pub mod adapter;

pub use adapter::{AdapterCapabilities, MockProvider, ProviderAdapter, ProviderId};
