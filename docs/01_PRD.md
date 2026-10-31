# Product Requirements Document

## Functional requirements
- FR-001 Provider CRUD: create, edit, test, enable, disable and remove provider definitions.
- FR-002 Credential pool: multiple credentials, labels, priority/weight, health, cooldown and last-used metadata.
- FR-003 Model catalogue: preserve provider-specific model IDs and capability metadata.
- FR-004 Virtual aliases: map stable local names to eligible deployments with explicit fallback policy.
- FR-005 Local gateway: configurable port, loopback bind by default, authentication required by default.
- FR-006 Protocol surfaces: OpenAI-compatible, Anthropic Messages and Gemini-compatible interfaces where verified; provider-specific passthrough where normalization loses semantics.
- FR-007 Capability-aware routing: only select deployments that satisfy the request.
- FR-008 Rotation: select another eligible credential/deployment when policy and error classification permit.
- FR-009 Retry/failover: bounded attempts, respect Retry-After, avoid unsafe duplicate execution.
- FR-010 Streaming: preserve protocol event semantics; no transparent switch after stream commitment unless resumability is explicitly supported.
- FR-011 Request history: status, latency, selected deployment and token usage when supplied.
- FR-012 Health diagnostics: explain unavailable, invalid or cooling-down states.
- FR-013 Agent profiles: generate copyable endpoint/model/auth configuration; external edits require consent and backup.
- FR-014 Backup/restore: versioned export, validation, preview and conflict resolution.
- FR-015 Lifecycle: start/stop/restart gateway and accurately display process behavior.
- FR-016 Redacted diagnostic export.

## Non-functional requirements
Security; reliability; low gateway overhead; concurrency safety; cross-platform support; maintainability; accessibility; structured redacted observability; data integrity; privacy by default.

## User stories
As a user, I can add several providers and keys, test them safely, create a model alias, choose routing behavior, see why a route was selected, connect my coding agent, and move configuration to a fresh computer without exposing keys.

## Acceptance
Every requirement must map to tasks and executable tests or documented manual acceptance evidence. Unimplemented requirements must remain visibly incomplete.

## Phase 0 amendments (2026-10-30, from planning audit + independent review)
- FR-017 Graceful drain: gateway stop/app exit drains in-flight requests within a bounded grace period; streaming responses terminate cleanly.
- FR-018 Port conflict: bind failure on the configured port is an explicit surfaced error; no silent port fallback.
- FR-019 Token rotation: user-facing gateway token regeneration with client-impact warning.
- FR-020 History retention: metadata-only request records default to 30-day retention with automatic cleanup.
- FR-021 Upstream concurrency bound: per-deployment concurrency semaphore, configurable.
- FR-022 Lifecycle transparency: v1 gateway runs only while the app process runs; UI states this.
- FR-023 Routing scope (ADR-009/010): v1 routes same-protocol only; Anthropic surface includes `/v1/messages/count_tokens`; `/v1/responses` out of scope v1. (Amendment 2026-10-31, ADR-010: Codex CLI REMOVED `wire_api="chat"` upstream, so no Codex profile ships — Codex is documented-unsupported in v1; the original `wire_api="chat"` rationale is obsolete.)
