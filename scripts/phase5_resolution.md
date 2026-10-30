
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
