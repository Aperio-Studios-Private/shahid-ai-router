# Phase 6 Report — Native Anthropic + Gemini Protocol Surfaces

Feature ID: PHASE-6. Date: 2026-10-31. Status: **PASS** (local evidence; CI blocked per B-01).

## Requirements delivered
FR-006 completion for v1 scope (all three protocol surfaces per ADR-010), same-protocol passthrough fidelity (ADR-009), records/health per family.

## Delivered
1. **Anthropic surface**: `POST /v1/messages` + `POST /v1/messages/count_tokens` (ADR-010). Upstream: `x-api-key` + `anthropic-version` (client's validated header forwarded, pinned `2023-06-01` default — F-5; the review verified `2026-10-21` is now current but removes `temperature`, so forwarding avoids semantic surprises). Classification covers Anthropic's error taxonomy incl. `timeout_error` → Timeout (never rotates — F-2) and the `billing_error` anchor (F-3). Pre-route validation (F-6): `model`/`messages` (+`max_tokens` for chat) — malformed requests never burn an upstream attempt.
2. **Gemini surface**: `POST /v1beta/models/{alias}:generateContent` with `%3A`-decoded path parsing (F-10); upstream `x-goog-api-key`; `RESOURCE_EXHAUSTED` defaults to BillingOrQuota (free-tier exhaustion is quota — docs/05) with explicit rate-limit-signal downgrade and `google.rpc RetryInfo.retryDelay` cooldown parsing capped 3600 s (F-7); `DEADLINE_EXCEEDED` → Timeout (F-2); `?key=` query auth rejected (F-9 — tokens never in URLs).
3. **Family-parameterized handler**: one rotation/records engine, three wire shapes. Error envelopes in the CLIENT's protocol (OpenAI/Anthropic `{type:"error",...}`/Gemini `{error:{code,message,status}}`), every category mapped (F-3), gateway-local codes family-legal (F-4). **F-1 (review): same-protocol upstream status preserved for 4xx/5xx; gateway-detected 2xx errors never echo success.** Capability requirements extended with Gemini signals (F-6); 503s carry exclusion reasons.
4. **Records**: `protocol_family` + `operation` (chat/count_tokens) per request; count_tokens records `input_tokens`.

## Verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **118 passed, 0 failed** (providers 30 incl. both new classification matrices + wiremock contract tests; gateway 43 incl. 9 new surface E2E tests)
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; fmt clean; tsc clean; vitest 2/2; frontend build ok
- Live smoke (previous phase binary) re-validated the shared engine paths.

## Honest scope statements
- Streaming remains Phase 7 (`stream:true` / `:streamGenerateContent` → structured rejection per family).
- Live opt-in tests against real Anthropic/Gemini APIs are mocked-only this phase (external credentials unavailable to the agent).
- count_tokens is proxied only for anthropic deployments (ADR-010); 501 otherwise — tested.
- Vision/capability heuristics unchanged (Phase 5 deferrals stand).
- CI remains blocked (B-01); macOS pending (ADR-016).

## Next eligible
Phase 7 (Streaming: SSE pass-through for all three surfaces + stream-commit failover boundary) — entry per workflow: plan + independent review.
