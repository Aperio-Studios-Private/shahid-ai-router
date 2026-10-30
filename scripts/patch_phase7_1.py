import io

# ---- plan amendment ----
p = 'audits/planning/phase7-plan.md'
src = io.open(p, encoding='utf-8').read()
add = '''

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

Status: READY FOR IMPLEMENTATION.'''
io.open(p, 'w', encoding='utf-8', newline='\n').write(src.rstrip() + '\n' + add)
print('plan amended')

# ---- http.rs: streaming clients (F-1) ----
p = 'crates/sair-providers/src/http.rs'
src = io.open(p, encoding='utf-8').read()
add = '''
/// As [`build_client_with_timeout`], but for STREAMING: no total deadline
/// (Phase 7 review F-1 — a total timeout would truncate a long-lived SSE
/// stream after commitment). Bounded by `connect_timeout` (establishment)
/// and `read_timeout` (idle between reads, client-level in reqwest 0.13).
#[must_use]
pub fn build_streaming_client(policy: SsrfPolicy) -> SsrfCheckedClient {
    let redirect_policy = policy.clone();
    let inner = reqwest::Client::builder()
        .dns_resolver(Arc::new(ValidatingResolver::new(policy.clone())))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error("too many redirects");
            }
            let Some(prev) = attempt.previous().last() else {
                return attempt.follow();
            };
            if redirect_policy.redirect_allowed(
                prev.host_str().unwrap_or_default(),
                prev.scheme(),
                attempt.url().host_str().unwrap_or_default(),
                attempt.url().scheme(),
            ) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(300))
        .build()
        .expect("streaming client must build; static policy and resolver only");
    SsrfCheckedClient {
        client: inner,
        policy,
    }
}
'''
src = src.rstrip() + '\n' + add
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('streaming client added')
