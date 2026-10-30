# Phase 5 Plan — Independent Review

Reviewer: independent plan reviewer. Date: 2026-10-30.
Subject: audits/planning/phase5-plan.md (OpenRouter + alias/capability matrix), repo head ~ebd3974.
Evidence inspected: docs/01_PRD.md, docs/04_PROVIDER_PROTOCOLS.md, docs/05_ROUTING_FAILOVER.md, docs/DECISION_LOG.md (ADR-009/010/015/017/018), .agent/SECURITY_RULES.md, .agent/QUALITY_GATES.md, crates/sair-providers/src/openai.rs, crates/sair-providers/src/http.rs, crates/sair-providers/src/adapter.rs, crates/sair-gateway/src/api.rs, crates/sair-gateway/src/routing.rs, crates/sair-domain/src/routing.rs, crates/sair-persistence/src/repos.rs, crates/sair-persistence/src/lib.rs, apps/desktop/src-tauri/src/commands_management.rs, OpenRouter attribution docs (re-verified 2026-10-30).

## Verified positives (checked, no finding)

- `send_chat_with_headers` generalization is genuinely additive: keeping `send_chat` as a wrapper that passes empty extra headers preserves existing request shape (bearer auth, model rewrite, timeout, classification) by construction; existing wiremock contract tests already pin the request shape (openai.rs `request_shape_bearer_and_model_mapping`).
- `tools: [non-empty]` requirement correctly treats an empty array as "no requirement" — the plan states this explicitly.
- Streaming is rejected at two layers already (api.rs handler and openai.rs send_chat), so the plan's claim "stream: true → already rejected" is accurate; no streaming requirement is needed and none is claimed (Phase 7).
- Vision deferral is honest: nothing in the plan claims vision support; the deferral is recorded per the PRD "visibly incomplete" acceptance rule.
- Proposed exclusion reason names (`capability_tool_calling`, `capability_structured_output`) are consistent with the existing snake_case reason family (`provider_disabled`, `deployment_disabled`, `credential_{status}`, `credential_cooling_down`, `protocol_mismatch` — repos.rs `load_route_snapshot_detailed`, routing.rs `plan_route`).
- Rollback story (additive; absent setting = no headers) is sound.

## Findings

### F-1 — High — Design gap: no provider-dispatch path exists for attribution headers
- Category: Plan completeness / security (cross-provider data leak risk).
- Evidence: `crates/sair-gateway/src/api.rs` `chat_completions` calls `send_chat` unconditionally for every candidate; there is no per-provider adapter dispatch. `crates/sair-persistence/src/repos.rs` `raw_snapshot_rows` does not select `p.provider_type`, and `sair_domain::routing::RouteCandidate` carries no provider-kind field (only provider_id/label/base_url). The plan says "OpenRouter calls it with attribution; other providers with none" but never specifies how an OpenRouter candidate is identified at the call site.
- Consequence if implemented as written: the implementer must invent the dispatch. The path of least resistance is keying headers off the global `app_settings` value alone — which sends `HTTP-Referer`/`X-Title` to every provider on every request (exactly the reviewer question 4 leak scenario: configured app URL/name leaked to non-OpenRouter upstreams), or adds a brittle base_url host check.
- Recommendation: extend the plan: (a) add `p.provider_type` to `raw_snapshot_rows` and a `provider_kind`/`provider_type` field to `RouteCandidate`; (b) dispatch per candidate in the handler (attribution extras only when `provider_type == "openrouter"` and setting present); (c) add the negative test "non-OpenRouter provider receives no attribution headers even when configured" — the plan currently lacks it; (d) provider_type values are schema-constrained (0001_init.sql CHECK includes `'openrouter'`), so no migration is needed.

### F-2 — Medium — `response_format` detection over-triggers, and `structured_output` can never be true in the product
- Category: Correctness (false exclusions) / plan accuracy.
- Evidence: (1) The plan requires `structured_output` whenever "body contains `response_format`". OpenAI-compatible `response_format: {"type": "text"}` is plain-text mode and must NOT require structured output; requiring it wrongly excludes deployments that would serve the request fine. Only `{"type":"json_object"}` / `{"type":"json_schema"}` (and an absent/unknown type must fail open, not closed) should require the flag. (2) The plan claims "`models.capabilities` JSON already carries the flags (P3 UI writes them)" — but the only model-write path, `apps/desktop/src-tauri/src/commands_management.rs` (`insert_model` command, ~line 299–305), hardcodes `"structured_output": false` (not a UI parameter), and no `update_model` exists anywhere (repos.rs has only `insert_model`). So in the real product every `response_format` (json) request would 503 against every model, while the Phase 5 gate tests (which insert rows via repos directly) would still pass — a test/product divergence.
- Recommendation: (a) detect on `response_format.type ∈ {json_object, json_schema}`; unknown types fail open; (b) either add a `structured_output` input to the model-insert command (small UI delta, contradicting "frontend unchanged-checks" — the plan must then say so) or explicitly record the limitation in the plan/report so the gate test is not silently unrepresentative.

### F-3 — Medium — Attribution header name is wrong; contradicts the plan's "verified docs" claim
- Category: Factual error / feature silently broken.
- Evidence: The plan says it sends "`HTTP-Referer`, `X-OpenRouter-Title` (per docs; `X-Title` also accepted — we send the documented pair)". OpenRouter's documented attribution headers are `HTTP-Referer` and `X-Title` (re-verified against openrouter.ai docs 2026-10-30; `X-OpenRouter-Title` appears nowhere in the documentation). With `X-OpenRouter-Title`, title attribution would silently never register — no error, no test failure against a mock that only checks presence.
- Recommendation: send `HTTP-Referer` + `X-Title`; pin both exact names in the E2E mock assertions.

### F-4 — Low — "503 with reasons" is underspecified: the handler does not currently return exclusion reasons
- Category: Spec gap (FR-012 visibility).
- Evidence: The plan's gate says "No capable candidate → 503 `no_eligible_deployment` with reasons". Today api.rs returns that error with only `{ "alias": ... }` in the body; `decision.exclusions` (reasons) is persisted in `decision_json` but never surfaced to the client (`upstream_failure_response` carries only a count).
- Recommendation: decide explicitly — either include sanitized reasons (deployment_id/credential_id + reason strings only; no labels containing user notes) in the 503 body, or reword the gate to assert reasons on the persisted decision record. Add the corresponding assertion to the matrix test.

### F-5 — Medium — Header-injection hardening and negative tests are asserted but not specified (SECURITY_RULES #8, QUALITY_GATES)
- Category: Security gate compliance.
- Evidence: The plan says "referer must be http(s) URL; title bounded (≤128 chars, header-safe)" but does not define: (a) implementation via fallible `HeaderValue::from_str` — `reqwest::RequestBuilder::header` panics on invalid name/value, so validation must precede builder use; (b) a concrete "header-safe" definition (reject CTL/CR/LF and non-ASCII-printable; `app_settings` is user-editable config and untrusted per SECURITY_RULES #7); (c) behavior when configured values fail validation (recommended: fail-open — omit the headers, `tracing::warn!`, request proceeds — attribution is cosmetic; a fail-closed choice would let a stale setting break all OpenRouter traffic); (d) negative tests. QUALITY_GATES requires negative tests for security-sensitive changes, and header injection is security-sensitive; the plan's gate section lists none.
- Recommendation: add to the plan: unit tests for CRLF/CTL injection, >128-char title, non-http(s) referer (ftp://, javascript:), and the fail-open path; state that the setting is read once per request (SQLite point query; acceptable — do not cache in v1 to avoid staleness) and reused across rotation attempts, not re-read per attempt.

### F-6 — Low — Filter ordering and reason precedence are unspecified
- Category: Spec gap (FR-012 accuracy).
- Evidence: `plan_route` currently applies the protocol-family filter to `candidates` (reason `protocol_mismatch`); the plan adds a capability requirement to the same function without stating order or precedence. A candidate that is both cross-protocol and capability-less must report `protocol_mismatch` (the first failing check), and capability checks apply only to candidates that already passed protocol + DB eligibility — never to DB-`excluded` rows (their reasons come from the SQL layer).
- Recommendation: one sentence in the plan: capability filter runs after the protocol filter; first-failing-check reason wins; capability exclusions are appended to the same `exclusions` vec.

### F-7 — Low — Two honesty/recording gaps in the plan's completeness story
- Category: Documentation honesty (PRD acceptance rule).
- Evidence: (1) `context_window` is a live capability flag in `CapabilityMetadata`/`capabilities_from_json`, but the plan is silent that no request-derived requirement exists for it (v1 has no tokenizer/token counting) — it should be recorded as deferred, like vision, so FR-007's "deepened" scope is exact. (2) QUALITY_GATES "Provider" gate lists "opt-in live test" (docs/04 onboarding likewise); the plan's E2E is mock-only and does not state the live-test status for the OpenRouter adapter.
- Recommendation: record the `context_window` deferral alongside the vision deferral; state explicitly that live OpenRouter testing is opt-in/deferred and that all Phase 5 evidence is mock-based.

## Verdict

VERDICT: APPROVED WITH REQUIRED REVISIONS — F-1, F-2, F-3 must be incorporated into the plan before implementation begins; F-4–F-7 are recommendations to fold in during the same revision. The plan's core approach (thin OpenRouter adapter over the generalized `send_chat`, explicit-signal capability requirements, honest vision/streaming deferral, additive rollback) is sound and consistent with ADR-009/010/015/017/018 and the PRD.
