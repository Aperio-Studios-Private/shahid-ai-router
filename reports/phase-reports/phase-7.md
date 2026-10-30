# Phase 7 Report — Streaming

Feature ID: PHASE-7. Date: 2026-10-31. Status: **PASS** (local evidence; CI blocked per B-01).

## Requirements delivered
FR-010 (SSE pass-through for all three surfaces; the stream-commit boundary is structural — the rotation loop exits at commitment), docs/05 streaming rules.

## Delivered
1. **Streaming clients (F-1, review-High):** dedicated strict/loopback streaming clients with `connect_timeout(30s)` + `read_timeout(300s)` (idle) and **no total deadline** — a stream may outlive `timeout_ms`. Non-streaming clients keep their total timeout. Verified against reqwest 0.13 docs (timeout covers the whole body read — would have truncated streams post-commitment).
2. **Total commit predicate (F-2, review-High):** `text/event-stream` on 2xx → committed `StreamOutcome`; any other 2xx → bounded buffer → non-streamed passthrough; non-2xx → classify/rotate (pre-commit). No undetectable cases remain.
3. **Adapter variants** for all three families over a shared `stream_request` core with the family classifier injected; Gemini appends `alt=sse` (F-3); anthropic-version forwarded; OpenRouter attribution scoped to its surface.
4. **Handler:** commit point exits the loop by construction; the deployment permit (FR-021) moves INTO the relayed stream via `PermitStream` and is released when the stream ends (F-6 normative); `content-length`/hop-by-hop headers not relayed; records carry `"streamed": true` in decision_json (F-5); streamed token counts are NULL (FR-011 residual, recorded in traceability).
5. **Tests (5 new E2E):** SSE byte-fidelity + single upstream hit; pre-commit 429 rotation to a streaming fallback; streaming error-JSON rotation; Gemini `:streamGenerateContent` + `alt=sse` assertion; FR-021 permit-hold rotation.

## Verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **123 passed, 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; fmt clean; tsc clean; vitest 2/2; frontend build ok

## Honest scope statements (F-4/F-5/F-6 review items)
- wiremock 0.6 cannot stream bodies — the four raw-TCP scenarios (mid-stream upstream drop, stream outliving the timeout at runtime, drain with an open stream, permit release on client disconnect) are specified in the review report and **deferred to Phase 11** with a traceability entry (recorded, not dropped).
- Streamed token counts are NULL (usage arrives mid-stream; frame parsing deferred) — FR-011 residual recorded.
- Unbounded stream relay is a documented accepted exception (SECURITY_RULES #8): streaming is the client's explicit request.
- Live opt-in SSE tests against real providers: mocked-only evidence this phase.
- CI remains blocked (B-01); macOS pending (ADR-016).

## Next eligible
Phase 8 (Agent profiles: Codex CLI `wire_api="chat"`, Claude Code, Gemini CLI; consent-gated external edits) — entry per workflow.
