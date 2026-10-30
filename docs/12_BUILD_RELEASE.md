# Build & Release

Supported minimums (ADR-014, 2026-10-30): Windows 10+ x64 (WebView2 Evergreen); macOS 12+ x86_64/aarch64. Exact Tauri 2 minimums verified at Phase 1 build.

Define supported macOS/Windows minimum versions and architectures. Verify Tauri/Rust dependencies, signing, notarization, installer and update behavior.

Pipeline: format/lint; Rust tests; frontend type/tests; adapter contracts; integration/E2E; dependency/security audit; platform builds; clean-machine install; gateway lifecycle and credential vault verification; signing/notarization; checksums; release notes; archive evidence and SBOM where practical.

Test upgrades from previous release, preserve user data, back up before migrations and provide recovery guidance. Uninstall must not delete user data unless explicitly requested.
