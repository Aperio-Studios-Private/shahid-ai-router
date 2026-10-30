# Phase 7 Plan — Streaming (SSE pass-through + Commit Boundary)

Feature ID: PHASE-7. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-010 (streaming: preserve protocol event semantics; no transparent switch after stream commitment unless resumability is explicitly supported), docs/05 (Streaming section), ADR-010/015/017.

## Repository state
Phase 6 complete (e4fac89): three native surfaces, 118 tests green.

## Design

### 1. Streaming adapter variants (sair-providers)
`StreamOutcome { status: u16, byte_stream: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>> }` — no axum dependency in the provider crate (boundary preserved).
- `send_chat_streaming_with_headers(...)` (openai), `send_messages_streaming(...)` (anthropic), `generate_content_streaming(...)` (gemini): identical request construction to non-streaming (body keeps `stream:true` / `:streamGenerateContent` method).
- **Commit semantics (docs/05):** classification happens on the response HEADERS only. 2xx + SSE-ish response → `StreamOutcome` (COMMITTED at return). Non-2xx (or 2xx with a JSON error content-type) → full body read + classify → `UpstreamFailure` — rotation is still legal BEFORE commitment.
- Error `commitment: bool` NOT added to UpstreamFailure: pre-commit failures are the only ones the router sees by construction.

### 2. Handler integration
- `stream:true` requests (and `:streamGenerateContent`) route through the SAME rotation loop; a streaming Ok → axum `Body::from_stream(outcome.byte_stream)` response → **committed**: the loop returns immediately; no failover after bytes reach the client. Pre-commit failures rotate per ADR-017 exactly like non-streaming.
- Streaming responses: Content-Type passthrough from the upstream (`text/event-stream`).
- Records: streaming rows recorded at commit time (status, latency, decision_json); token counts are NULL for streamed responses (usage arrives mid-stream; parsing frames would inspect content — deferred, recorded honestly).
- Concurrency permit (FR-021) is held for the stream's lifetime (drop guard moved into the response body via `map`... v0: held until the response completes — implement via a permit-carrying stream wrapper; tested).

### 3. Tests
- SSE frame fidelity: upstream SSE body relayed byte-faithful (data: frames + [DONE]).
- Pre-commit rotation: first candidate RateLimited → fallback streams (rotation before commit).
- No-retry-after-commit: happy-path streaming hit-count == 1 per attempt; the loop is structurally exited (assert single upstream hit).
- Streaming 4xx classifies and rotates (error JSON body, not SSE).
- Gemini `:streamGenerateContent` accepted via streaming path; Anthropic `stream:true` via streaming path.
- FR-021: streaming holds the deployment permit (concurrent streaming request to a concurrency-1 deployment rotates past).

## Acceptance
All tests + full suites green; clippy/fmt; frontend checks; report + traceability (FR-010) + push.

## Rollback
Additive; non-streaming paths untouched (existing tests guard).


## Review resolution (2026-10-31) — verdict: REVISE BEFORE IMPLEMENTATION (resolved)

Report: `audits/planning/phase7-plan-review.md` (external verifications incl. reqwest 0.13.5 timeout semantics, wiremock 0.6.5 capabilities, tower-http TimeoutLayer source). All incorporated:

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | Dedicated **streaming clients** (strict + loopback): `connect_timeout(30s)` + `read_timeout(300s)` (idle, client-level) and NO total deadline — a stream may outlive `timeout_ms`; tested by a stream that outlives the policy budget. Non-streaming clients keep `.timeout()`. |
| F-2 | High | Total commit predicate: `Content-Type: text/event-stream` on 2xx → COMMIT; any other 2xx → bounded buffer (MAX_RESPONSE_BYTES) → if it parses as a normal response body, non-streamed success passthrough; classified error only when non-JSON/oversized. Non-2xx → classify/rotate (unchanged). No header-undetectable cases remain. |
| F-3 | Medium | Gemini streaming URL: `{base}/v1beta/models/{model}:streamGenerateContent?alt=sse` — `alt=sse` appended by the gateway (never `key`); query forwarded from the surface call. |
| F-4 | Medium | Evidence: wiremock 0.6.5 cannot stream bodies — hit-count assertions prove no-retry-after-commit; the four raw-TCP scenarios (mid-stream drop, stream outliving timeout, drain with open stream, permit release on client disconnect) are specified in the review report and DEFERRED to Phase 11 hardening with an explicit traceability entry (recorded, not silently dropped). |
| F-5 | Medium | Records: `decision_json` gains `"streamed": true`; latency/status semantics documented (commit-time); FR-011 residual (stream token counts NULL) recorded in traceability. |
| F-6 | Low | Phase-6 rejection points enumerated and removed (2 validate_request checks, gemini method branch, 3 adapter body checks, 2 inverted tests); pre-commit error-body reads bounded by MAX_RESPONSE_BYTES; `content-length` and hop-by-hop headers never relayed; unbounded stream relay documented as an explicit accepted exception (SECURITY_RULES #8); permit normatively moves into the response stream (never dropped at handler return); live-test status: opt-in, mocked-only this phase. |

Status: READY FOR IMPLEMENTATION.