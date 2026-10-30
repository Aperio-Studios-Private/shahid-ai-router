
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
