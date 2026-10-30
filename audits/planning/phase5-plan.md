# Phase 5 Plan — OpenRouter + Alias/Capability Matrix

Feature ID: PHASE-5. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-003 (capability metadata honored), FR-004 (aliases across providers), FR-007 (capability-aware routing deepened), docs/04 (OpenRouter attribution headers), ADR-009/010/015.

## Repository state
Phase 4 complete (ebd3974): routing engine + rotation/health, 93 tests green.

## Design

### 1. OpenRouter adapter (thin — OpenAI-compatible, verified docs 2026-10-30)
`openrouter.rs`: reuses `send_chat` with provider-specific header injection:
- Optional attribution headers sent only when configured (app_settings `openrouter_attribution`: `{referer, title}`): `HTTP-Referer`, `X-OpenRouter-Title` (per docs; `X-Title` also accepted — we send the documented pair).
- Validation: referer must be http(s) URL; title bounded (≤128 chars, header-safe).
- Implementation: `send_chat_with_headers(client, ..., extra_headers)` generalization in `openai.rs` (OpenRouter calls it with attribution; other providers with none). No behavior change for existing providers (tested).
- SSE comment-payload and usage-chunk handling are Phase 7 (streaming) concerns — noted.

### 2. Capability-aware routing deepened (FR-007)
- Request-derived capability requirements (explicit signals only):
  - body contains `tools: [non-empty]` → require `tool_calling` capability.
  - body contains `response_format` → require `structured_output` capability.
  - `stream: true` → already rejected (Phase 7).
  - Vision: content-block detection is heuristic — deferred, recorded (docs/01 acceptance rule: visibly incomplete).
- Router: requirements flow into `plan_route` as a `CapabilityRequirements` struct; mismatching candidates get exclusion reason `capability_tool_calling` / `capability_structured_output` (FR-012 visibility).
- `models.capabilities` JSON already carries the flags (P3 UI writes them).

### 3. Alias matrix tests (gate)
- Alias spanning two providers where only one supports tools → tool request routes to the capable one (decision record shows the other excluded with `capability_tool_calling`).
- Same for `response_format` → structured_output.
- No capable candidate → 503 `no_eligible_deployment` with reasons.
- OpenRouter E2E: attribution headers present when configured, absent when not; passthrough fidelity (model mapping to an OpenRouter-style model id).

## Acceptance / gate
New tests + full suites green; clippy/fmt; frontend unchanged-checks; report + traceability (FR-007 update) + push.

## Rollback
Additive; settings default absent = no attribution headers.

## Review resolution (2026-10-31) — verdict: APPROVED WITH REQUIRED REVISIONS

Report: `audits/planning/phase5-plan-review.md`. All incorporated before implementation:

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | Dispatch path added: `p.provider_type` selected in the snapshot, `RouteCandidate.provider_type` carries it, per-candidate dispatch in the handler; attribution headers ONLY for `provider_type == 'openrouter'`; negative test: configured attribution must NOT appear on non-OpenRouter providers. |
| F-2 | Medium | Requirements: `response_format.type` in {json_object, json_schema} → structured_output (type 'text' and unknown types fail open); empty/nonexistent tools → no requirement. UI gap fixed: `add_model` accepts structured_output; new `update_model_capabilities` command (no more hardcoded false). |
| F-3 | Medium | Attribution pair corrected to `HTTP-Referer` + `X-Title` (docs re-verified by reviewer 2026-10-31); test asserts the exact pair. |
| F-4 | Low | 503 `no_eligible_deployment` envelope gains `reasons: [...]` (decision exclusions) in the shahid extension. |
| F-5 | Medium | Headers built via fallible `HeaderValue::from_str` (never panics); header-safe definition = HTTP visible ASCII, no CR/LF; invalid configured values → fail-open (header skipped) + warn; negative tests for invalid config. |
| F-6 | Low | Capability filter runs after protocol filter; one reason per candidate (first failing check). |
| F-7 | Low | Deferrals recorded: vision (heuristic) and context_window enforcement not implemented; live OpenRouter test is opt-in, mocked-only evidence this phase. |

Status: READY FOR IMPLEMENTATION.
