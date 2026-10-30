# Quality Gates

Statuses: PASS, FAIL, BLOCKED. WAIVED requires authorized human approval; agents cannot self-waive.

Rust: fmt, clippy, tests, dependency audit, integration tests.
Frontend: format/lint, typecheck, component tests, accessibility checks, production build.
Provider: protocol contracts, streaming where declared, error classification, redaction and opt-in live test.
Database: migration, transaction, integrity and backup/restore tests.
Security-sensitive changes: threat-model update, negative tests, secret inspection, dependency/permission review and independent security review.

Severity: Critical (compromise/data loss), High (serious security/core failure), Medium (meaningful limited defect), Low (minor), Informational.

Critical and High block shipping. Medium affecting correctness/security/reliability must be fixed or escalated for human decision. Never claim bug-free software.
