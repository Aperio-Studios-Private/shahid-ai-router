# Changelog

All notable changes to Shahid AI Router. Format: Keep a Changelog; versioning: SemVer.

## [0.1.0] — 2026-10-30

### Added (Phase 0 — Discovery)
- Planning audit with PRD/architecture/stack review and finalized roadmap (`audits/planning/planning-audit.md`).
- Phase 0 research notes with verified stack versions and provider API facts (`audits/planning/phase0-research.md`).
- Independent plan review report and resolution of all findings (`audits/planning/plan-review-report.md`).
- Architecture Decision Records ADR-001…ADR-014 (`docs/DECISION_LOG.md`).
- PRD amendments FR-017…FR-023 (`docs/01_PRD.md`); traceability matrix populated (`docs/TRACEABILITY_MATRIX.md`); task boards seeded (`tasks/`).

### Added (Phase 1 — Skeleton)
- Rust workspace: `sair-domain` (error taxonomy, protocol families), `sair-persistence` (SQLite/WAL/foreign-keys via SQLx 0.9), `sair-vault` (SecretStore abstraction + test mock), `sair-providers` (adapter contract + capabilities + mock), `sair-gateway` (Axum `/healthz` skeleton).
- Tauri 2 desktop shell with least-privilege capabilities, locked-down CSP, `get_app_info` IPC command, ring rustls provider install.
- React 19 + Vite 8 + TypeScript 7 + Tailwind 4 frontend with docs/08 navigation skeleton, light/dark theme, vitest component tests.
- Placeholder icon set and generator script (`scripts/make-placeholder-icons.mjs`).
- GitHub Actions CI matrix (Windows/macOS): fmt, clippy `-D warnings`, cargo test, frontend typecheck/test/build.

### Added (Phase 2 — Security & persistence)
- Full docs/07 schema (migrations 0001/0002) with FK delete-rule matrix and probe tests; OS vault (keyring 4.x; Windows Credential Manager verified live) with test-mock release guard; gateway token lifecycle (32-byte CSPRNG, constant-time verify, crash-safe regeneration with live hot-swap); middleware stack (auth, Host allowlist, 503-at-capacity, 408 timeout, 413 body limit, slowloris header-read timeout); redacted telemetry with sweep tests and rotating file sink; SSRF policy + validating resolver + SsrfCheckedClient boundary (ADR-015).
- Independent security review pass with all High/Medium findings fixed and re-verified (audits/security/).

### Added (Phase 3 — First vertical slice)
- OpenAI-compatible `/v1/chat/completions` (non-streaming passthrough) and `/v1/models`; body-aware error classification (ADR-017: 429+insufficient_quota = billing, not rate-limit); routing engine v0 (priority/round-robin/weighted, cooldown, quarantine; lru/pin → structured 501) with deterministic concurrent-rotation tests; request records with full routing decisions (FR-012); management IPC + Providers/Aliases UI; secrets vault-backed, deleted with credentials.
