# Development Roadmap

Phase 0 Discovery: verify stack, APIs, agent compatibility, keychain and lifecycle prototypes. Gate: evidence-based architecture review.
Phase 1 Skeleton: workspace, Tauri shell, React UI, CI. Gate: clean builds.
Phase 2 Security/persistence: SQLite migrations, OS vault, gateway token, redacted logs. Gate: secret lifecycle tests.
Phase 3 First vertical slice: one OpenAI-compatible provider, credential, model, local gateway, non-streaming request and request record. Gate: E2E.
Phase 4 Rotation: credential pool, health, error classification, cooldown, account grouping. Gate: simulated failure suite.
Phase 5 OpenRouter and aliases. Gate: alias/capability tests.
Phase 6 Native Anthropic and Gemini protocols. Gate: protocol contract tests.
Phase 7 Streaming and advanced routing. Gate: streaming/concurrency/retry safety.
Phase 8 Agent profiles and verified client integrations. Gate: reproducible client tests.
Phase 9 UI, logs, usage and diagnostics. Gate: E2E/accessibility.
Phase 10 Backup/restore. Gate: encrypted and cross-platform restore tests.
Phase 11 Hardening/release. Gate: security, performance and clean-install checklist.

Status (2026-10-31): Phases 0-11 are COMPLETE — per-phase evidence in `reports/phase-reports/phase-1..11.md` (Phase 0 evidence lives in `audits/planning/planning-audit.md`). Phase 11 closed as "code-complete with residuals"; release tagging awaits the owner's waiver of the BLOCKED items (tasks/BLOCKED.md). An independent audit the same day found and fixed UI/backup/CI defects — see `reports/phase-reports/audit-fixes.md`.

Each feature must independently pass `.agent/WORKFLOW_RULES.md` before the next feature begins.
