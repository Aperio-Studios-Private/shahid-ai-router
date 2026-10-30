# Build & Release

Define supported macOS/Windows minimum versions and architectures. Verify Tauri/Rust dependencies, signing, notarization, installer and update behavior.

Pipeline: format/lint; Rust tests; frontend type/tests; adapter contracts; integration/E2E; dependency/security audit; platform builds; clean-machine install; gateway lifecycle and credential vault verification; signing/notarization; checksums; release notes; archive evidence and SBOM where practical.

Test upgrades from previous release, preserve user data, back up before migrations and provide recovery guidance. Uninstall must not delete user data unless explicitly requested.
