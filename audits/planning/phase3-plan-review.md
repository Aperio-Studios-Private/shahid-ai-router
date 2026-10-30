# Phase 3 Plan Review — First Vertical Slice (independent)

Reviewer: independent plan reviewer (did not author the plan). Date: 2026-10-30.
Plan under review: `audits/planning/phase3-plan.md` (DRAFT, Feature ID PHASE-3).

Inputs read in full: docs/01 (PRD + amendments), docs/02, docs/04, docs/05, docs/06, docs/07, docs/11, docs/13, docs/DECISION_LOG.md (ADR-009/010/011/012/015), audits/security/phase2-security-review.md (incl. retest + Phase 3 watch items), audits/planning/phase2-plan.md, docs/TRACEABILITY_MATRIX.md; code interfaces: `crates/sair-domain/src/{error,protocol,adapter-free lib}.rs`, `crates/sair-providers/src/{http,adapter}.rs`, `crates/sair-gateway/src/{lib,handle}.rs`, `apps/desktop/src-tauri/src/state.rs`, `crates/sair-persistence/migrations/0001_init.sql`, `crates/sair-persistence/src/lib.rs`, `crates/sair-vault/src/store.rs`. `.agent/SECURITY_RULES.md` and `.agent/QUALITY_GATES.md` used as gates.

Method: every plan claim cross-checked against the committed domain types, schema columns, ADR text, and PRD/FR wording; challenge list from the review charter worked item by item.

---

## Findings

### R3-01 — High — Error classification — 429 is mapped to RateLimited unconditionally; docs/04's quota-truth rule is not operationalized

**Category:** Classification fidelity (docs/04) / routing correctness (docs/05).

**Description.** The plan's matrix maps "429 RateLimited" with no body inspection, and checks `insufficient_quota` only for 402. docs/04 states verbatim: "Do not assume every HTTP 429 means quota exhaustion. Distinguish transient rate limiting from billing/account quota when provider signals allow it." The canonical case is OpenAI's 429 with `error.type = "insufficient_quota"` (billing exhaustion delivered with HTTP 429). Because the plan's rotation rule is "RateLimited → try next candidate" while "BillingOrQuota → stop (account-level)", an unconditional 429→RateLimited mapping produces exactly the behavior docs/05 "Quota truth" forbids: burning the attempt budget rotating keys that share the exhausted account's quota, and reporting RateLimited in health/records. Secondary gaps in the same matrix: (a) 404→ModelUnavailable also catches "wrong base_url path" misconfigurations — distinguish OpenAI-coded `model_not_found` from other 404s (else Unknown/ProviderUnavailable); (b) ContextLimit detection is a bare string — specify the v0 match set (e.g. `context_length_exceeded`, `maximum_context_length`) with fallback to InvalidRequest and per-provider refinement later; (c) a 200 response whose body fails JSON validation or the size bound is unclassified — it must become a classified error (ProviderUnavailable or Unknown), never a passthrough.

**Evidence.** Plan §Same-protocol passthrough / §OpenAI-compatible adapter ("402/insufficient_quota BillingOrQuota, 429 RateLimited (+ Retry-After parsed)"); docs/04:14; docs/05:31; committed taxonomy `crates/sair-domain/src/error.rs:66-69` (`is_account_level` exists precisely for this split).

**Recommendation.** Amend the matrix: classification inspects the bounded upstream body's `error.type`/`code` for 4xx; 429 + insufficient_quota-type → BillingOrQuota; 404 disambiguation; explicit classification for invalid/oversized success-path bodies. Contract tests per branch, including a 429-with-insufficient_quota case asserting stop-without-rotation E2E.

---

### R3-02 — High — Retry-After, backoff and the retry budget are unpinned at the routing level; FR-009 is only half-covered

**Category:** Routing/failover semantics (docs/05), FR-009 fidelity.

**Description.** The adapter parses Retry-After, but nothing in the plan says what the router does with it. Open questions the plan leaves to the implementer: (1) does the next attempt wait out Retry-After (which can exceed the whole budget) or is the wait converted into cooldown with immediate rotation to a *different* candidate; (2) `routing_policies.retry_budget_ms` (schema default 10000) is never mentioned — only "attempt budget (default 3)" — and the schema defaults are mutually inconsistent (`timeout_ms` 120000 > `retry_budget_ms` 10000, so one attempt can outlive the total budget); define what the budget bounds (wall clock across attempts? per attempt?) and reconcile defaults; (3) docs/05 requires "bounded exponential backoff with jitter. Avoid retry storms" — absent from the plan; (4) duplicate execution: the plan rotates on Timeout, and the committed domain code marks Timeout safely retryable (`error.rs:42-50`), but docs/05:27-28 warns that a timeout is ambiguous (the request may have executed and billed) and says to "surface ambiguity instead of blind retry" where idempotency is unavailable. Rotation-on-timeout is a defensible v0 choice but must be an explicit recorded decision (e.g., at most one rotation after an ambiguous timeout, and never for the same credential), not an accident of the rotation list.

**Evidence.** Plan §Routing engine v0 ("RateLimited/Timeout/ProviderUnavailable/TransportFailure → try next candidate within attempt budget (default 3)"); docs/05:21-28; FR-009 ("bounded attempts, respect Retry-After, avoid unsafe duplicate execution"); `migrations/0001_init.sql:76-87` (`retry_budget_ms`, `timeout_ms`, `cooldown_seconds` defaults). docs/11 lists "rate limits and Retry-After" as mandatory; the plan's only Retry-After test is adapter-level parsing.

**Recommendation.** Pin the semantics in the plan before T03: per-request wall-clock budget = `retry_budget_ms`; Retry-After honored by (a) setting/raising the credential's `cooldown_until` to `now + max(policy_cooldown, Retry-After)` and (b) rotating immediately to a different candidate without waiting; jittered backoff only when a wait is genuinely taken; timeout rotation decision recorded with rationale; add routing-level tests (Retry-After → cooldown value; budget exhaustion mid-rotation).

---

### R3-03 — High — The routing candidate model is underspecified: two-level priority/weight conflated, half the strategy enum undefined, and the FR-012 decision record has no defined destination

**Category:** Core design gap (P3-T03).

**Description.** Three linked gaps in the engine's data model:
1. **Which priority/weight?** The schema has priority+weight on *both* `alias_deployments` (the user's explicit fallback order per FR-004) and `credentials` (`0001_init.sql:37-38, 101-104`). The plan says "priority (desc) default" without defining whether a candidate is a (deployment, credential) pair, how the cross-product is enumerated, and how the two priority levels compose. Weighted-random has the same ambiguity. This is the engine's central structure; leaving it unpinned guarantees rework.
2. **Strategies:** `Strategy` declares `lru` and `manual_pin`, the schema's CHECK allows them, but the engine implements only priority/round-robin/weighted. Behavior when an alias's policy selects `lru` or `manual_pin` is undefined (silent fallback to priority would be dishonest; an error must be structured). Either implement `lru` now (it is cheap — `credentials.last_used_at` exists) or define a structured "strategy unsupported in v0" rejection enforced at alias/policy-creation time.
3. **Decision record destination:** `RoutingDecision` (selected + ordered exclusion reasons) is defined, but `request_records` has no decision column and `health_events` has no correlation_id. Specify where it goes in P3: surfaced in the error envelope's `shahid` extension when all candidates are exhausted, plus structured log, plus (optionally) JSON in `health_events.detail`; state explicitly that UI surfacing of FR-012 is Phase 9. Also note that docs/05 step 5's allow/deny rules are deferred (no schema entity exists) so the omission is a recorded decision, not an oversight.

**Evidence.** Plan §Domain additions / §Routing engine v0; docs/05:9-16 (selection algorithm, "apply allow/deny rules and priority"); docs/05:19 (strategies incl. LRU and manual pin); FR-004 ("explicit fallback policy"); FR-012; `migrations/0001_init.sql:76-107, 153-167`.

**Recommendation.** Define `RouteCandidate` as (deployment, credential) pairs; composition rule: `alias_deployments.priority/weight` orders deployments, `credentials.priority/weight` orders credentials within a deployment (both participate in all strategies); pin lru/manual_pin behavior; pin the decision-record destinations; record allow/deny deferral.

---

### R3-04 — Medium — ADR-012's committed concurrency mechanism is absent from the plan although rotation now ships in Phase 3

**Category:** ADR compliance / race conditions.

**Description.** ADR-012 commits to `tokio::sync::Mutex<RotationState>` for cooldown/health/last-used state with short critical sections (never across network/DB awaits) and an ordered health-event path, with concurrent-rotation tests "Phase 4". The plan now pulls rotation, cooldown, quarantine, and a per-alias round-robin counter into P3-T03 but never mentions RotationState or the mutex — the counter and cooldown writes would otherwise race under concurrent requests. Because rotation lands in P3, at least a basic concurrent-rotation test should land with it (docs/11 mandatory scenario "concurrent rotation"), or the plan must explicitly record the deferral of that test to Phase 4.

**Evidence.** docs/DECISION_LOG.md ADR-012; plan §Routing engine v0 (round-robin "per-alias counter", cooldown writes, health_events inserts — no synchronization mechanism named); docs/11:7; phase2-security-review retest (rotation was explicitly Phase-4-scoped when ADR-012 was accepted).

**Recommendation.** Add to T03: `RotationState` behind `tokio::sync::Mutex` holding per-alias counters, cooldown overrides, and last-used bookkeeping; DB writes sequenced through the same ordered path; critical sections hold no DB or network awaits (consistent with ADR-004). Add one deterministic concurrent test (N tasks racing one alias) or write the deferral into the plan and traceability.

---

### R3-05 — Medium — FR-021 is claimed but not implemented: "engine-level semaphore reuses gateway limit" is a global bound, not a per-deployment one

**Category:** Requirement fidelity / PRD acceptance.

**Description.** FR-021 (PRD amendment): "Upstream concurrency bound: **per-deployment** concurrency semaphore, configurable." The plan lists FR-021 in its PRD refs and then implements a global limit shared with the gateway (`GatewayRuntimeState.permits`, `lib.rs:57,64`). A global semaphore satisfies ADR-011 hardening but not FR-021's semantics; claiming it violates the PRD acceptance rule ("Unimplemented requirements must remain visibly incomplete") and QUALITY_GATES ("state exactly what was tested"). The traceability matrix already assigns FR-021 to the Phase 4 epic, so the cheapest correct move is to drop the claim.

**Evidence.** Plan line 6 and §Gateway surfaces ("per-deployment concurrency — engine-level semaphore reuses gateway limit in P3"); docs/01:35; docs/TRACEABILITY_MATRIX.md:29; `crates/sair-gateway/src/lib.rs:55-65`.

**Recommendation.** Either implement actual per-deployment semaphores (a small map of `Arc<Semaphore>` keyed by deployment_id, configurable) — reasonable to include since upstream saturation is visible as soon as real upstreams exist — or remove FR-021 from the plan's refs and leave it "Planned, Phase 4" in traceability. Do not claim partial credit via the global limit.

---

### R3-06 — Medium — The client-facing error surface and `/v1/models` shape are unspecified (statuses, envelope contents, validation minimum, alias vs upstream ids)

**Category:** Protocol surface fidelity (ADR-010, agent compatibility).

**Description.** The plan defines the envelope for *upstream* errors but not the full local error matrix an agent client will actually hit: unknown alias (404 vs 400? envelope `type`/`code`?), no eligible deployment / all candidates exhausted (status? does the preserved upstream status leak when the last failure was upstream?), inbound validation failures (which minimum validation? at minimum: body is a JSON object, `model` present and a string, `stream` absent/false, `messages` present — say so), and the `streaming_not_supported` 400's exact envelope. Related: the `/v1/models` response must be pinned — OpenAI list format requires `object`/`created`/`owned_by` per entry, and the plan must decide whether entries are local aliases (what clients must configure as `model`; recommended) or upstream model ids (which would bypass routing if accepted in chat requests — the plan should state that unknown/unmapped `model` values are rejected, i.e. only aliases route). Non-JSON upstream error bodies (HTML from a proxy) must be wrapped with a bounded safe excerpt, not passed through raw.

**Evidence.** Plan §Same-protocol passthrough, §Gateway surfaces ("GET /v1/models: OpenAI list format of enabled models" — one line); ADR-010; docs/02 "protocol design" (keep protocol-specific fidelity); docs/09-relevant concern: Codex CLI/agents validate configs by listing models and read statuses.

**Recommendation.** Add a short "local error matrix" table (trigger → status → envelope type/code → `shahid` fields incl. decision record when exhausted) and a `/v1/models` schema (aliases as ids, upstream ids excluded or clearly namespaced; static `created`/`owned_by` policy). State the minimum inbound validation set and that unvalidated fields pass through untouched.

---

### R3-07 — Medium — Loopback opt-in mechanism, base-URL joining, `endpoint_override` composition, and policy-timeout wiring are unspecified; the E2E design already depends on them

**Category:** Adapter security detail (docs/06 SSRF), P3-T02b.

**Description.** (1) "Loopback opt-in only for explicit local providers" — `SsrfPolicy` is baked into the client at `build_client` time (`http.rs:243-253`), so the plan must define what makes a provider "explicit local": a per-provider user-set flag with explicit confirmation, never inferred from `base_url` content. This is not deferrable: the plan's own E2E uses wiremock on `127.0.0.1`, which requires the loopback-allowing client path to exist and be scoped. (2) URL handling: user-entered `base_url` will carry trailing slashes and sometimes a `/v1` suffix; pin the join rule (trim trailing `/`, append `/chat/completions` once) and how `deployments.endpoint_override` (in the schema since P2) composes with `upstream_base` — the adapter signature takes only `upstream_base` today. (3) Timeout: `build_client_with_timeout` exists but the plan never wires `routing_policies.timeout_ms` into the per-call budget (the default client is fixed at 120 s).

**Evidence.** Plan §Security/privacy ("remote providers https-only, loopback opt-in only for explicit local providers"), §OpenAI-compatible adapter signature; `crates/sair-providers/src/http.rs:55-77, 243-253, 299-319`; `migrations/0001_init.sql:66-71` (`endpoint_override`); phase2-security-review SEC2-01 retest watch item (adapter client discipline).

**Recommendation.** Specify: per-provider `allow_local` flag (default false) → `SsrfPolicy::allow_loopback()`; base-URL normalization rule + endpoint_override semantics; per-request timeout from the resolved policy. Note the SsrfCheckedClient-only rule in docs/04 when adapters land (the ADR-015 amendment already records it; docs/04 was named in the watch item).

---

### R3-08 — Medium — Test plan gaps against docs/11 scenarios applicable in P3, plus gate omissions (dependency review, frontend accessibility)

**Category:** Test adequacy (docs/11, QUALITY_GATES).

**Description.** The listed tests cover classification, strategies, cooldown/quarantine/billing, and E2E happy path — but missing explicit cases for mandatory/applicable scenarios: (a) provider-outage rotation *success* (5xx → next candidate → 200; only exclusion/cooldown outcomes are listed); (b) protocol-family mismatch exclusion (the P3 proxy for "capability mismatch" — nothing enforces ADR-009's filter in the test list); (c) `stream:true` rejection E2E; (d) malformed inbound JSON / missing model field; (e) oversized or non-JSON upstream response (bound + reject, SECURITY_RULES #8 negative test); (f) 429-with-insufficient_quota stop (per R3-01); (g) Retry-After at routing level (per R3-02). Separately: adding `wiremock 0.6` is a new dependency — QUALITY_GATES requires the dependency audit/provenance step to be named (SECURITY_RULES #3), and the frontend gate lists typecheck/test/build but omits the accessibility checks QUALITY_GATES requires for the four new screens (docs/08).

**Evidence.** Plan §Routing engine v0 tests, §Acceptance/gate; docs/11:7 (mandatory scenarios); docs/13 (P3 gate = E2E); QUALITY_GATES (Provider: error classification; Frontend: accessibility checks; dependency audit).

**Recommendation.** Add the seven test cases above to T02b/T03/T06; add "dependency provenance review for wiremock (dev-only, workspace)" and frontend accessibility checks to the gate checklist.

---

### R3-09 — Low — Secret-buffer handling and post-request bookkeeping details are unstated

**Category:** Docs/06 fidelity, persistence consistency.

**Description.** (1) docs/06 says "Clear buffers where practical"; P2 committed no zeroize and the plan (correctly) claims none — but it should state the actual handling: vault secret retrieved as `Vec<u8>` → converted once to the Authorization header value → no extra clones, no Debug/Display on secret-bearing types (the redaction layer only rewrites *named fields*; a Debug-printed struct bypasses it), prompt drop. Do not claim zeroization without implementing it. Also state explicitly that only the request *body* is forwarded upstream — the local client's `Authorization`/`x-api-key` headers are never relayed. (2) The post-request write path (request_record insert + health_events insert + credential update of `cooldown_until`/`last_used_at`/`last_error_*`) is 3+ statements; specify they run in one short transaction with no network await inside (ADR-004-compatible) and that credential status transitions are conditional updates (`WHERE status='active'`) so a concurrent quarantine is not clobbered — this also pins where ADR-012's ordered path meets the DB.

**Evidence.** docs/06:7; plan §Security/privacy; `migrations/0001_init.sql:29-48`; `crates/sair-persistence/src/lib.rs` (no repo layer yet).

**Recommendation.** Add both bullets to the plan's security/persistence sections; neither requires new dependencies.

---

## Sound areas (verified, no action)

- **SSRF discipline** is correctly carried into the design: adapter receives only `SsrfCheckedClient`, by construction, matching the SEC2-01 retest watch item and amended ADR-015; scheme/IP-literal checks run per request at the wrapper boundary (`http.rs:305-318`).
- **Same-protocol passthrough principle and stream honesty** align with ADR-009, ADR-010, and FR-010 (400 rather than fake streaming; streaming deferred to Phase 7 with no retry-after-commitment risk in P3).
- **Quarantine-vs-rotate split** matches the committed domain semantics (`error.rs` `should_quarantine_credential` / `is_safely_retryable` / `is_account_level`), including billing stop and no blind rotation.
- **Filter order** matches docs/05 steps 1–6 for the rules P3 implements (alias → enabled/protocol/credential-active/cooldown → strategy), and the decision record with ordered exclusion reasons is the right FR-012 core.
- **Schema fit**: all routing-relevant columns exist (cooldown_until, dual priority/weight, max_attempts/retry_budget_ms/timeout_ms/cooldown_seconds, health event taxonomy, partial cooldown index, denormalized record labels); hot-path indexes (alias unique, alias_deployments alias_id, deployments model_id, credentials provider+status) are adequate; "no transactions across awaits" restates ADR-004 correctly.
- **Repository-state claims** (commit aeb04b3, 61 tests, B-01 CI billing block) match reports/phase-reports/phase-2.md.
- **Rollback story** (additive repos, no schema change, per-task commits) is accurate for the planned work.

---

## Verdict

The plan's architecture is right and its security posture is real, but three design gaps sit directly in the implementation path (classification of 429-quota, retry/budget semantics, candidate model + strategy semantics), and two committed decisions are violated by omission (ADR-012 mechanism, FR-021 semantics). All are fixable by plan amendment before any task starts; none requires re-architecting.

**VERDICT: APPROVE WITH CHANGES** — resolve R3-01, R3-02, R3-03 (mandatory, reshape P3-T02b/T03) and R3-04, R3-05 (mandatory, compliance corrections) in the plan text before implementation begins; R3-06…R3-09 should be incorporated in the same amendment.
