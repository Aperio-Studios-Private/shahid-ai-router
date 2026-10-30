//! Pure domain types for Shahid AI Router.
//!
//! This crate must stay free of IO (network, filesystem, database). It holds
//! the shared vocabulary used by every other crate: the provider error
//! taxonomy (`error`), protocol families (`protocol`), and — in later phases —
//! routing policy and capability types.

pub mod error;
pub mod protocol;
