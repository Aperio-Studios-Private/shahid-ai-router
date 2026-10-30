# Phase 6 Plan — Native Anthropic + Gemini Protocol Surfaces

Feature ID: PHASE-6. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-006 (protocol surfaces: provider-specific passthrough where normalization loses semantics), FR-009/011/012 (rotation/history/health across surfaces), docs/04 (adapter contract + error categories), ADR-009 (same-protocol routing), ADR-010 (surfaces: `/v1/messages` + `/v1/messages/count_tokens`; Gemini `models/{model}:generateContent`), ADR-015/017.

## Repository state
Phase 5 complete (85167d2): OpenAI surface + capability routing + attribution, 100 tests green.

## Design

### 1. Family-parameterized handler (refactor, no behavior change for OpenAI)
The chat handler is generalized: the protocol surface determines `ProtocolFamily` (OpenAiChat from `/v1/chat/completions`, AnthropicMessages from `/v1/messages`, GeminiGenerateContent from `models/{model}:generateContent`). Per family: request validation, adapter call, error envelope, usage extraction, request records (`protocol_family` column already exists). Response passthrough is body-faithful per family; error responses use the CLIENT's protocol shape:
- OpenAI: `{error:{message,type,code,shahid:{...}}}` (existing).
- Anthropic: `{type:"error", error:{type, message}, shahid:{...}}` — `error.type` uses Anthropic's names (`authentication_error`, `rate_limit_error`, …) mapped from our category; our category preserved in `shahid`.
- Gemini: `{error:{code, message, status, shahid:{...}}}` — `status` uses Gemini's names.

### 2. Anthropic adapter (`sair-providers/src/anthropic.rs`) — verified docs 2026-10-30/31
- `POST {base}/v1/messages`, headers: `x-api-key: <secret>`, `anthropic-version: 2023-06-01` (pinned), `content-type: application/json`.
- Body: client's body with `model` rewritten (alias → upstream id). `stream:true` → structured rejection (Phase 7).
- Classification (body `{type:"error", error:{type,message}}`): `authentication_error`→InvalidCredential, `permission_error`→PermissionDenied, `rate_limit_error`→RateLimited (+Retry-After header), `billing`/`credit` hints in type/message→BillingOrQuota, `not_found_error`→ModelUnavailable (model) / InvalidRequest (path), `request_too_large`→InvalidRequest, `invalid_request_error`→InvalidRequest (ContextLimit match set first: "prompt is too long", "max_tokens"), `overloaded_error`/`api_error`/5xx→ProviderUnavailable, transport→TransportFailure/Timeout. Upstream status + error type preserved.
- Response usage: `usage.input_tokens` / `usage.output_tokens`.

### 3. Anthropic surface endpoints
- `POST /v1/messages` — full routing/rotation flow (same engine, family AnthropicMessages).
- `POST /v1/messages/count_tokens` — proxied to `{base}/v1/messages/count_tokens` for anthropic-family deployments; for aliases with no anthropic deployment → 501 structured `count_tokens_not_supported` (ADR-010). Non-streaming only; no rotation loop needed beyond single attempt per candidate (count is idempotent-cheap — but keep the same bounded loop for consistency).

### 4. Gemini adapter (`sair-providers/src/gemini.rs`)
- `POST {base}/v1beta/models/{upstream_model}:generateContent`, header `x-goog-api-key: <secret>`.
- Response usage: `usageMetadata.promptTokenCount` / `candidatesTokenCount`.
- Classification (body `{error:{code,message,status,details}}`): `UNAUTHENTICATED`→InvalidCredential, `PERMISSION_DENIED`→PermissionDenied, `RESOURCE_EXHAUSTED`→BillingOrQuota when details/status hints contain `QuotaFailure`/`billing` (free-tier quota exhaustion), else RateLimited; `NOT_FOUND`→ModelUnavailable; `INVALID_ARGUMENT`→InvalidRequest (ContextLimit set: "input token count", "exceeds the maximum"); `FAILED_PRECONDITION`→InvalidRequest; `UNAVAILABLE`/`INTERNAL`/5xx→ProviderUnavailable.
- Client auth for the GEMINI SURFACE: `x-goog-api-key` header (gateway middleware already accepts it). The `?key=` query-param form is **rejected** (token-in-URL leak vector; documented for agent profiles in Phase 8).

### 5. Gemini surface endpoints
- `POST /v1beta/models/{model_and_method}` where the segment is `{alias}:generateContent` → parse suffix; `:streamGenerateContent` → structured streaming rejection (Phase 7); unknown suffix → 404.
- Query param `key` present → 401 with `query_param_auth_rejected` (explicit, tested).

### 6. Rotation/records
All families flow through the same engine (ADR-009 same-protocol: requests only route to deployments whose `protocol_family` matches the surface — already in the snapshot loader). Request records carry the family; health events unchanged.

## Tests (gate: protocol contract tests per docs/11)
- Anthropic adapter wiremock contract: auth headers (x-api-key + anthropic-version), model rewrite, classification matrix incl. 429+Retry-After, usage extraction, malformed-200 rejection.
- Gemini adapter wiremock contract: x-goog-api-key header, URL shape (`:generateContent`), classification matrix incl. RESOURCE_EXHAUSTED both ways, usage extraction.
- Surface E2E: `/v1/messages` happy path + passthrough fidelity; `/v1/messages` with `stream:true` → structured rejection; `count_tokens` proxied for anthropic upstream, 501 for openai-only alias; Gemini `:generateContent` happy path; `:streamGenerateContent` → rejection; `?key=` auth rejected; cross-family request (OpenAI body to `/v1/messages` shape errors) validated.
- Rotation E2E across an anthropic alias with two providers (fallback on rate_limit_error).

## Affected files
sair-providers (anthropic.rs, gemini.rs, lib exports), sair-gateway (api.rs refactor into family dispatch; new surface routes), app (no changes), tests.

## Acceptance
All listed tests + full suites green; clippy/fmt; frontend unchanged checks; report + traceability + push.

## Rollback
Additive commits; no schema changes; OpenAI surface behavior preserved (existing tests guard).

## Review resolution (2026-10-31) — verdict: APPROVED WITH REQUIRED REVISIONS

Report: `audits/planning/phase6-plan-review.md` (external verifications with sources included there). All incorporated:

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | Same-protocol upstream failures preserve `failure.status` as the response status (clamped to a valid StatusCode; gateway-local errors keep their own codes). No blanket 502 for same-protocol upstreams. |
| F-2 | High | Anthropic `timeout_error` and Gemini `DEADLINE_EXCEEDED` map to **Timeout** before any 5xx fallback — ADR-017 (never rotate ambiguous executions) holds per family; rotation tests cover it. |
| F-3 | Medium | Complete mapping tables both directions: every `ErrorCategory` → Anthropic type (incl. `billing_error` 402 anchor for BillingOrQuota, `conflict_error` fallback) and → Gemini status (incl. ABORTED/UNIMPLEMENTED/etc. fallbacks); unmatched upstream types classify via fallback sets, never silently. |
| F-4 | Medium | Gateway-local errors defined per envelope: count_tokens 501 uses the Anthropic shape with `error.type: "invalid_request_error"` + `shahid.code: "count_tokens_not_supported"`; alias/streaming errors use family-legal names. |
| F-5 | Medium | `anthropic-version`: the client's validated header (visible-ASCII, ≤64 chars) is FORWARDED when present; otherwise pinned default `2023-06-01` (legacy-supported; 2026-10-21 noted as current but removes `temperature` — forwarding avoids semantic surprises). Plan's "pinned 2023-06-01" claim corrected. |
| F-6 | Medium | Per-family pre-validation BEFORE routing (no upstream attempt, no health event): Anthropic requires `max_tokens` + `messages`; Gemini derive_from_body adds `generationConfig.responseMimeType == application/json` or `responseSchema` → structured_output. |
| F-7 | Medium | Gemini `RESOURCE_EXHAUSTED` defaults to **BillingOrQuota** (free-tier exhaustion is quota — docs/05), downgraded to RateLimited only on explicit rate-limit signals (`details[].@type` containing `rate` / message `rate limit`); `google.rpc RetryInfo.retryDelay` ("5s") parsed for cooldown. |
| F-8 | Low | count_tokens: body carries model+messages (verified); model rewritten; record `operation="count_tokens"`, `input_tokens`→tokens_in; single attempt per candidate within the same bounded loop. |
| F-9 | Low | `?key=` test uses a valid token header + `?key=` so the request reaches the handler; handler rejects with the Gemini envelope 401 `query_param_auth_rejected`. |
| F-10 | Low | `:generateContent` parsed from the decoded `Path<String>` extractor; encoded-colon (`%3A`) test added. |
| F-11 | Low | Gate adds: capability mismatch on new families, Gemini malformed-200, timeout classification per family, openai-only alias `/v1/messages` → 503 in the Anthropic envelope; live tests opt-in, mocked-only evidence stated. |

Status: READY FOR IMPLEMENTATION.
