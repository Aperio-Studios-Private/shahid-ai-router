# Phase 3 Plan — First Vertical Slice

Feature ID: PHASE-3. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
FR-001 (provider CRUD), FR-002 (credential pool metadata), FR-003 (model catalogue), FR-004 (aliases), FR-005 (gateway, done in P2 — extended with protocol surfaces), FR-006 (protocol surfaces: OpenAI-compatible non-streaming), FR-007 (capability-aware routing), FR-009 (bounded retry budget — v0: single attempt per credential, rotation on classified failure), FR-011 (request history), FR-012 (health diagnostics: decision record explains exclusions), FR-021 (per-deployment concurrency — engine-level semaphore reuses gateway limit in P3).

## Repository state
Phase 2 complete (commit aeb04b3): schema, OS vault, gateway auth/middleware/lifecycle, redaction, SSRF wrapper. 61 tests green. CI blocked (B-01).

## Design

### Same-protocol passthrough (FR-006, ADR-009)
The OpenAI-compatible surface forwards the client's JSON body validated-but-unmodified except the `model` field (alias → upstream model id). Responses pass through with usage extracted for records. No lossy translation. Error responses are mapped into the OpenAI error envelope `{error:{message,type,code}}` with the classification category preserved in a `shahid` extension field for diagnostics. `stream:true` in P3 → structured 400 `streaming_not_supported` (honest; lands Phase 7).

### Domain additions (P3-T01, sair-domain)
`CapabilityMetadata` (protocol_family, streaming, tool_calling, vision, structured_output, context_window), `Strategy` (priority|round_robin|weighted|lru|manual_pin), `RouteCandidate`, `RoutingDecision` (selected candidate + credential id + ordered exclusion reasons — FR-012).

### Persistence repos (P3-T02a)
Typed repos over the existing schema: providers CRUD, credentials CRUD (status/priority/weight/cooldown/last_*), models CRUD, deployments CRUD, alias+alias_deployments CRUD, policy get/set, request_records insert + query, health_events insert. No transactions across awaits (already enforced by design). Test fixtures seed the full chain.

### OpenAI-compatible adapter (P3-T02b, sair-providers)
- Receives **only** `SsrfCheckedClient` (ADR-015 rule from security re-verification).
- `send_chat(upstream_base, model_id, credential_secret, body_json) -> Result<ChatOutcome, ClassifiedError>`; ChatOutcome = status + passthrough body + usage (tokens in/out).
- Error classification matrix (docs/04): 401 InvalidCredential, 403 PermissionDenied, 402/insufficient_quota BillingOrQuota, 429 RateLimited (+ Retry-After parsed), 404 model_not_found ModelUnavailable, 400/422 InvalidRequest, context_length_exceeded ContextLimit, 5xx ProviderUnavailable, connect/timeout TransportFailure/Timeout, else Unknown. Upstream status/code preserved.
- Contract tests via **wiremock**: request shape (Bearer header, model mapping, body fidelity), full classification matrix, Retry-After passthrough, usage extraction.

### Routing engine v0 (P3-T03, sair-gateway)
`resolve_alias_and_route(snapshot, alias, required: protocol_family) -> RoutingDecision`:
1. alias lookup → candidate deployments; 2. filter: enabled deployment + enabled provider + protocol_family match (ADR-009) + credential active (not invalid/quarantined/incomplete, cooldown_until expired); 3. strategy: priority (desc) default, round-robin (per-alias counter), weighted random; 4. decision record with exclusion reasons (F: "explain why unavailable" — FR-012).
Rotation on classified failure: RateLimited/Timeout/ProviderUnavailable/TransportFailure → try next candidate within attempt budget (default 3); InvalidCredential/PermissionDenied → quarantine credential + stop (no blind rotation per docs/05); BillingOrQuota → stop with account-level note (account grouping lands Phase 4). Cooldown: set credential cooldown_until on classified failure (base cooldown from policy).
Tests: exclusion reasons, priority/round-robin/weighted, cooldown exclusion, quarantine stop, no-rotation on billing.

### Gateway surfaces (P3-T04)
- `POST /v1/chat/completions` (non-streaming): validate body → alias resolution → route → vault retrieve secret → adapter call → map errors to OpenAI envelope → insert request_record + health_events → response passthrough. Bounded attempts with per-candidate fallback.
- `GET /v1/models`: OpenAI list format of enabled models.
- Tauri commands for providers/credentials/models/deployments/aliases CRUD + `test_provider` (harmless models-list call). Secrets: accepted via IPC, stored to vault, never returned.
- UI (P3-T05): Providers, API Keys, Models & Deployments, Aliases screens — compact forms/lists per docs/08 (masked secrets, explicit states).

### Affected files
sair-domain (new modules), sair-persistence (repos), sair-providers (openai.rs), sair-gateway (routing.rs, surfaces/openai.rs), app (commands, state), ui (4 screens). Dev-dep: wiremock 0.6 (workspace).

## Security/privacy
- Secrets only via vault; request records metadata-only; logs redacted (P2 stack).
- Upstream base URLs: `SsrfPolicy` enforced through the wrapper; remote providers https-only, loopback opt-in only for explicit local providers.
- Upstream responses treated as untrusted: size-bounded read (reuse body limit), JSON-validate before passthrough mapping.

## Acceptance / gate (P3-T06)
E2E: real client (reqwest in test) → gateway (auth, Host) → alias resolution → routed deployment → wiremock upstream → passthrough response + request_record row + health rows on failure. Classification matrix E2E for quota/ratelimit/quarantine paths. Full suites + clippy/fmt + frontend typecheck/test/build. Report + traceability + push. Reviewer findings resolved before implementation.

## Rollback
Per-task commits; schema unchanged (Phase 2 migration is the only one; repos are additive). No user data migration concerns.

## Review resolution (2026-10-31) — verdict: APPROVE WITH CHANGES
Report: `audits/planning/phase3-plan-review.md`. Disposition (all incorporated before implementation):

| ID | Sev | Resolution |
|---|---|---|
| R3-01 | High | Body-aware classification: 429 + `type:"insufficient_quota"` (or code `insufficient_quota`) → **BillingOrQuota** (stops rotation); 429 otherwise → RateLimited; 404 disambiguated (model_not_found → ModelUnavailable, else InvalidRequest); ContextLimit matched on bounded body's type/code/message set before generic InvalidRequest; malformed or oversized 200 bodies → classified error (Unknown / TransportFailure respectively), never passthrough. Upstream status/code always preserved. |
| R3-02 | High | v0 retry semantics recorded (ADR-017): Retry-After (seconds form) → credential cooldown + **immediate rotation** to next candidate (no in-request sleeping → no storms, bounded latency); retry_budget_ms = wall-clock budget across attempts (checked before each attempt), timeout_ms = per-attempt budget (per-request override); no same-credential retry in v0 → backoff/jitter N/A (documented); **Timeout does not rotate** (ambiguous execution, docs/05:28 — surface instead); rotation set = RateLimited/ProviderUnavailable/TransportFailure. Policy rows are always written explicitly by the app (schema defaults not relied upon; default inconsistency documented). |
| R3-03 | High | Composition rule: candidates = (alias_deployments ordered by priority desc, weight) × (provider credentials ordered by priority desc, weight); pair priority is lexicographic (alias_dep, credential); round-robin rotates the flattened pair list per alias; weighted uses credential weight. `lru`/`manual_pin` declared but **not implemented in v0** — engine returns structured 501 `strategy_not_implemented` (tested, honest). `RoutingDecision` persisted: migration **0002** adds `request_records.decision_json` and `health_events.correlation_id` (+index); UI display deferred to Phase 9 (recorded). |
| R3-04 | Medium | ADR-012 mechanism implemented now: `RotationState` behind `tokio::sync::Mutex` (round-robin counters + attempt bookkeeping; critical sections never span awaits); deterministic concurrent test (20 tasks, strict round-robin distribution) brought forward. |
| R3-05 | Medium | FR-021 claim dropped from this phase — global gateway bound only; per-deployment semaphore stays Phase 4 (traceability already says so). |
| R3-06 | Medium | Local error matrix defined (alias_not_found 404, no_eligible_deployment 503 + decision reasons, strategy_not_implemented 501, streaming_not_supported 400). `/v1/models` lists **aliases** as callable ids (what clients use), documented + tested. |
| R3-07 | Medium | Base URL normalization (strip trailing slash) + `Url::join` fixed segments; endpoint_override = alternative base URL (same policy validation); per-attempt timeout via request-builder override; migration **0002** adds `providers.allow_loopback` (per-provider explicit opt-in for loopback HTTP). |
| R3-08 | Medium | Added tests: 429-quota vs rate-limit E2E, ambiguous-timeout no-rotation, malformed-200 classification, /v1/models, concurrent round-robin. Phase gate includes the docs/11 applicable-scenario checklist. |
| R3-09 | Low | `SecretBytes` (zeroize-on-drop) wraps retrieved secrets (docs/06 "clear buffers where practical"); correlation_id = 32 hex chars from OS CSPRNG; last_used/last_error bookkeeping on the rotation path. |

Status: READY FOR IMPLEMENTATION (P3-T01 first; migration 0002 + upgrade tests).
