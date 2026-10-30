# Phase 5 Report — OpenRouter + Alias/Capability Matrix

Feature ID: PHASE-5. Date: 2026-10-31. Status: **PASS** (local evidence; CI blocked per B-01).

## Requirements delivered
FR-007 deepened (request-derived capability requirements), FR-004 (aliases across providers with capability matching), docs/04 OpenRouter onboarding step 1 (attribution headers), plus the F-2 UI gap fix (model capability editing).

## Delivered
1. **OpenRouter attribution** (openrouter.rs): documented pair `HTTP-Referer` + `X-Title` (F-3; reviewer re-verified against openrouter.ai docs — my earlier draft had an undocumented variant), opt-in via `app_settings.openrouter_attribution`, header-safe validation with fail-open on invalid config (F-5: fallible `HeaderValue` construction, no panic path), dispatched **only** to providers of type `openrouter` (F-1) — negative E2E test proves configured attribution does not leak to other providers.
2. **Capability-aware routing (FR-007):** requirements derived from the request body (non-empty `tools` → tool_calling; `response_format.type` ∈ {json_object, json_schema} → structured_output; `text`/unknown fail open — F-2); applied after the protocol filter with single-reason exclusions (F-6); surfaced in 503 `no_eligible_deployment` reasons (F-4).
3. **Capability editing (F-2):** `add_model` takes `structured_output`; new `update_model_capabilities` command (repo-layer `update_model_capabilities`); removes the hardcoded-false gap from P3.
4. **Matrix tests:** tools request routes to the capable provider (incapable provider never called); no-capable candidate → 503 + capability reasons; `text` fails open; attribution E2E (leak negative test). Deferrals recorded (F-7): vision heuristic, context_window enforcement, live OpenRouter test opt-in.

## Verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **100 passed, 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; fmt clean
- Frontend: tsc clean (TS 7), vitest 2/2, production build ok

## Honest scope statements
- Vision requirement detection is NOT implemented (heuristic; deferred, recorded).
- context_window enforcement NOT implemented (metadata stored only).
- OpenRouter evidence is mocked-only (wiremock); a live opt-in test against the real API is deferred (needs a real key — external credential, not available to the agent).
- Streaming comment-payload/usage-chunk handling remains Phase 7.
- CI remains blocked (B-01); macOS pending (ADR-016).

## Next eligible
Phase 6 (native Anthropic + Gemini protocol surfaces) — entry per workflow: plan + independent review.
