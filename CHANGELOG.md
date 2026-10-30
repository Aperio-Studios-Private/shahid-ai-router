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
