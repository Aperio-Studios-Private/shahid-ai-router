# Technical Stack

| Layer | Recommendation |
|---|---|
| Desktop | Tauri 2 |
| Backend | Rust |
| Async runtime | Tokio |
| HTTP server | Axum |
| HTTP client | Reqwest |
| Frontend | React + TypeScript |
| Styling | Tailwind CSS |
| UI primitives | Accessible, maintained component primitives |
| Database | SQLite |
| DB access | SQLx + migrations |
| Serialization | Serde |
| Logging | tracing |
| Testing | cargo test, frontend test runner, integration/E2E |

Rust is a strong fit for concurrent gateway/networking and memory-safe backend work; it is not literally fastest for every workload. Tauri does not automatically guarantee always-on background service behavior. Prototype lifecycle on macOS and Windows before committing to it.

Use OS-native secret storage (macOS Keychain; Windows credential protection facility) through a maintained integration. Never silently fall back to plaintext.

For encrypted portable backups, use established cryptographic libraries, authenticated encryption and a reviewed password KDF. Never invent cryptography.

Before implementation, verify current stable versions, maintenance, licenses, advisories, platform support and compatibility. Pin dependencies and record decisions in `docs/DECISION_LOG.md`.
