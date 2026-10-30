# Testing & Definition of Done

## Layers
Unit; adapter contract; integration; opt-in live provider; end-to-end; security; performance; macOS/Windows platform tests.

## Mandatory scenarios
Valid/invalid credentials; rate limits and Retry-After; account-level quota; provider outage; capability mismatch; streaming; no retry after stream commitment; ambiguous timeout; concurrent rotation; gateway auth; loopback-only default; secret redaction; malformed/oversized/tampered import; cross-platform restore; schema migration.

## Definition of Done
Requirements implemented; acceptance criteria pass; unit/integration/E2E tests pass as applicable; formatting/lint/type checks pass; independent code review; security review for sensitive changes; regression tests pass; docs/changelog/traceability updated; no unresolved Critical/High issues; Medium/Low residual issues documented and triaged; evidence attached.

Compile success alone is not completion. Do not claim bug-free software.

Statuses: PASS, FAIL, BLOCKED. Only an authorized human may approve a waiver; an agent cannot waive its own failed gate.
