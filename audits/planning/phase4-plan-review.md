# Phase 4 Plan Review — Rotation & Health Completion (independent)

Reviewer: independent plan reviewer (did not author the plan). Date: 2026-10-30.
Plan under review: `audits/planning/phase4-plan.md` (DRAFT, Feature ID PHASE-4). Repo head: e314a5d.

Inputs read in full: docs/01 (PRD, FR-002/008/012/021), docs/05 (Rotation/Retry safety/Quota truth), docs/07, docs/11, docs/DECISION_LOG.md (ADR-012/015/017/018), docs/08 (health/API-keys sections), docs/TRACEABILITY_MATRIX.md (FR-002/008/012/021 rows), audits/planning/phase3-plan-review.md, reports/phase-reports/phase-3.md; code: `crates/sair-gateway/src/{api,routing,lib,handle}.rs`, `crates/sair-persistence/src/{repos,time,lib}.rs`, `crates/sair-persistence/migrations/0001_init.sql` + `0002_routing_decisions.sql`, `crates/sair-providers/src/openai.rs` (classification/Retry-After), `crates/sair-domain/src/error.rs`, `apps/desktop/src-tauri/src/{state,commands_management}.rs`. `.agent/SECURITY_RULES.md` and `.agent/QUALITY_GATES.md` used as gates.

Method: each plan claim cross-checked against committed schema constraints, the actual chat-handler control flow, the snapshot loader's cooldown comparison, and the existing command/validation patterns; challenge list from the review charter worked item by item.

---

## Findings

### R4-01 — High — Health events are silently discarded today: every failure insert violates the `health_events.event` CHECK, and Phase 4 builds its FR-012 surface on that empty table

**Category:** FR-012 correctness / schema constraint vs. runtime data.

**Description.** The plan's Health Monitor deliverable (`list_health_events`, `credential_health_summary`, UI table) reads `health_events`. But the only writer (`api.rs`) inserts `event = failure.category_str()`, whose values (`invalid_credential`, `billing_or_quota`, `rate_limited`, `provider_unavailable`, `transport_failure`, `timeout`, …) are **not** in the column's CHECK set `('ok','degraded','cooldown_start','cooldown_end','quarantine','invalid','recovered')` — so every failure insert fails the constraint and the error is swallowed by `let _ =`. The table is empty today and would stay empty (only the new P4 `cooldown_start` rows would appear), making the Phase 4 health surface vacuous while looking complete. The phase-3 report claims "request records + health events" implemented without any test asserting a row persisted — this defect slipped through precisely because no test reads the table back. The plan does not mention fixing the writer, and its tests verify cooldowns "via DB" but never health-event rows.

**Evidence.** `crates/sair-persistence/migrations/0001_init.sql:159-161` (CHECK set); `crates/sair-gateway/src/api.rs:325-337` (`insert_health_event(..., failure.category_str(), ...)` behind `let _ =`); `crates/sair-domain/src/error.rs:125-139` (category_str values); `reports/phase-reports/phase-3.md:17` (claim, no read-back test).

**Recommendation.** Add to the plan: (a) a category→event mapping (e.g., InvalidCredential/PermissionDenied→`quarantine`, RateLimited-with-cooldown→`cooldown_start`, others→`degraded`; or migrate the CHECK to accept category strings — pick one and record it), applied at the single insert site; (b) a `cooldown_start` event for the existing RateLimited cooldown path too (currently no event at all); (c) regression tests that **read `health_events` back** after a 401 (quarantine event), a 429-with-Retry-After (cooldown_start), and the new group-cooldown path (one event per affected credential, shared correlation id); (d) `credential_health_summary` tests against persisted data.

---

### R4-02 — High — Rotation can re-attempt the same credential within one request via a second deployment of the same provider; the plan's "no storm" invariant is not enforced by the engine

**Category:** Rotation correctness (docs/05 retry safety, quota truth) / planned test assertion is false in general.

**Description.** The snapshot loader joins **all** active credentials of a provider onto **every** alias deployment of that provider (`JOIN credentials c ON c.provider_id = d.provider_id`), so one credential appears once per deployment in `decision.ordered`. The rotation loop consults neither cooldown nor an attempted-set between candidates: after credential C fails RateLimited on deployment D1 and its cooldown is written, the very next candidate in the same request can be C-on-D2 (snapshot was loaded pre-failure), re-attempting a just-cooled key immediately. Consequences: duplicate burn of a rate-limited key within one request, doubled latency, and health records contradicting the cooldown just written. This also breaks the plan's own scenario-4 assertion ("each credential attempted at most once per request window") whenever the test fixture has ≥2 deployments per provider — with 1 deployment per credential the test passes vacuously and the hole ships. (The 20-parallel aspect is handled in R4-05; this finding is about the per-request invariant.)

**Evidence.** `crates/sair-persistence/src/repos.rs:703-718` (join, `WHERE ad.alias_id = ?`); `crates/sair-gateway/src/api.rs:221-385` (rotation loop: no attempted-set, cooldown/status not re-checked in-loop); plan §5 scenario 4 (line 35).

**Recommendation.** Add to the plan: per-request `HashSet<credential_id>` of attempted credentials; candidates whose credential was already attempted are skipped with exclusion reason `credential_already_attempted` (surfaced in the decision record per FR-012). Test both shapes: 1 credential × 2 deployments (same provider) where the first attempt fails rotate-class → second deployment must be skipped/next-credential used, and the standard multi-credential fixture.

---

### R4-03 — Medium — Cooldown writes are unconditional overwrites (no monotonic guard): concurrent failures and shorter cooldowns can shorten a longer standing cooldown, incl. across the account group

**Category:** Cooldown semantics / retry-storm adjacence (docs/05).

**Description.** Both the existing `set_credential_cooldown` and the plan's group UPDATE write `cooldown_until = ?` unconditionally. SQLite (WAL) serializes the two simultaneous billing failures correctly (single statement, last-write-wins — the plan's concurrency claim is otherwise sound), but last-write-wins is not monotonic: (a) a group member cooling until T+300 from a Retry-After is reset to now+policy(60s) by the billing write, becoming eligible early to hit the same exhausted quota; (b) two near-simultaneous failures with different Retry-After values let the shorter one win if it commits last; (c) within one request, the RateLimited write and a subsequent group write can interact the same way. Early re-eligibility directly contradicts docs/05's avoid-storms/quota-truth intent and silently erodes R4-04's cap.

**Evidence.** `crates/sair-persistence/src/repos.rs:256-269` (unconditional set); plan §1 line 16 (group UPDATE unconditional); `crates/sair-gateway/src/api.rs:338-347` (RateLimited write); docs/05:21-23.

**Recommendation.** Make both writes monotonic in one statement, e.g. `SET cooldown_until = CASE WHEN cooldown_until IS NULL OR cooldown_until < :until THEN :until ELSE cooldown_until END, updated_at = :now WHERE (account_id = :account OR id = :cred) AND status = 'active'` (string `<` is valid under the R4-08 format pin). Add a test: a credential with a longer standing cooldown does not get shortened by a newer shorter failure, group-wide.

---

### R4-04 — Medium — Retry-After: no cap, `u64→i64` wrap and a chrono overflow panic on huge values; the standard HTTP header is not parsed at all; billing-path interaction unpinned

**Category:** Untrusted-input hardening (SECURITY_RULES #7/#8) / FR-009 fidelity.

**Description.** (1) `retry_after_secs` comes only from the JSON body (`error.retry_after`, parsed as `u64`) — the HTTP `Retry-After` header is never read, yet that is where OpenAI and most providers actually send it; with real upstreams `cooldown_for` will almost always fall back to the policy value, so "Respect Retry-After" is only body-supported. (2) There is no cap: `cooldown_for` does `s as i64` (values > i64::MAX wrap negative → 0), and `Utc::now() + chrono::Duration::seconds(s)` **panics** for s ≳ 9.2e15 — a malformed or hostile provider response (docs/05: provider responses are untrusted) can abort the request handler. (3) The plan's group path (BillingOrQuota) is silent on which cooldown value it uses and whether a Retry-After arriving on a quota-class failure is honored or ignored.

**Evidence.** `crates/sair-providers/src/openai.rs:168-175` (body-only, as_u64); `crates/sair-gateway/src/routing.rs:157-162` (`s as i64`); `crates/sair-gateway/src/api.rs:341` (chrono addition); plan §1/§5; docs/05:21.

**Recommendation.** Plan amendment: (a) clamp to a bounded maximum — saturating conversion plus a configurable cap (default e.g. 3600s, floor policy cooldown); unit tests for cap and overflow values; (b) parse the HTTP `Retry-After` header (seconds form; HTTP-date explicitly out of scope v0 or rejected to Unknown) in the adapter; (c) state the billing-path rule (recommended: group cooldown uses `max(policy.cooldown_seconds, capped Retry-After)` only for the failed credential; group members get policy cooldown — or ignore Retry-After for quota class; either is fine, pin it).

---

### R4-05 — Medium — The concurrent-rotation E2E is nondeterministic as specified; reshape into a deterministic two-phase design

**Category:** Test adequacy / flakiness (docs/11, ADR-012 "deterministic").

**Description.** Scenario 4 fires 20 parallel requests with the first credential always 429-limited and asserts "each credential attempted at most once per request window". The number of requests that load their snapshot before the first cooldown write commits is nondeterministic (1..20; pool of 8 connections), so credential-1's hit count is a race outcome — the assertion as written is flaky, and combined with R4-02 the invariant it names is not engine-enforced. Scenario 1's "Retry-After sets cooldown (verified via DB)" invites an exact-value assert computed from `Utc::now()`, also a timing hazard. Scenarios 2, 3, 5, 6 are deterministic as specified (scenario 5's "rotates or 503s" disjunction is honest but weak — see R4-06 for pinning it).

**Evidence.** Plan §5 lines 30-37; `crates/sair-persistence/src/lib.rs` (pool max 8); `repos.rs:609-622` (snapshot loaded once per request).

**Recommendation.** Two-phase deterministic design: Phase A — one request hits cred1 (429) → cooldown set, served by cred2 (proves the write path); Phase B — 20 parallel requests → assert all 200 from cred2, wiremock verifies cred1 total hits == 1 and cred2 hits == 21. Keep the P3 engine-level 20-task round-robin test as the distribution-under-concurrency coverage. Assert cooldowns as ranges (`started ≤ cooldown_until ≤ started + capped_retry_after + slack`), never exact strings.

---

### R4-06 — Medium — FR-021 semaphore design leaves four semantics unpinned (attempt-budget consumption, terminal all-busy envelope, permit scoping, live-apply)

**Category:** FR-021 design completeness.

**Description.** (1) Busy-skip vs. `max_attempts`: the natural loop implementation increments `attempt_index` on busy-skips, so under load busy candidates consume real attempt slots and the number of actual upstream attempts shrinks (with max_attempts=3 and two busy primaries, only one upstream call happens). Since iteration is bounded by the finite candidate list, busy-skips do not need to count. (2) When every candidate is busy, the plan text resolves to the existing `no_attempt_made`/`no_eligible_deployment` fallback path — the client envelope code and `shahid` fields for "all busy" must be pinned (recommended: 503 `deployment_busy` + exclusion reasons) so scenario 5 asserts something exact. (3) Permit scope: must be acquired inside the candidate iteration and dropped when `send_chat` resolves (success or failure) — never held across rotations; plan says "released when the upstream call completes" but the rotation-loop interaction should be explicit. (4) `deployment_concurrency` is read at gateway start like the other `app_settings` values (consistent with port/limits); state that changes apply on gateway restart, that the map is keyed by deployment_id with entries living for the gateway lifetime (bounded by deployment count; restart clears), and record the policy consequence that a locally-saturated primary rotates bursts onto fallback deployments (fallback quota burn — acceptable, but record it). Also name the mutex discipline for the map (short critical sections, no awaits — ADR-012 analog). Interaction with the global limit is sound as designed (global `try_acquire` 503 at middleware, non-blocking per-deployment try_acquire inside, one permit per request — no nesting deadlock).

**Evidence.** Plan §2; `crates/sair-gateway/src/api.rs:221-228` (attempt budget loop); `crates/sair-gateway/src/lib.rs:140-152` (global guard, `gateway_busy`); `apps/desktop/src-tauri/src/state.rs:241-245` (settings apply at start).

**Recommendation.** Pin (1)-(4) in the plan; make scenario 5 two deterministic sub-cases: second candidate configured → 200 from fallback; single candidate → 503 `deployment_busy` envelope.

---

### R4-07 — Medium — `assign_credential_account` lacks same-provider validation; command-level validation for priority/weight and list limits is unspecified

**Category:** FR-002 data integrity / input validation (SECURITY_RULES #8).

**Description.** `credentials.account_id` FK only checks the account exists, not that it belongs to the credential's provider — a cross-provider assignment would put a credential into a foreign group, and the plan's group UPDATE (`WHERE account_id = ? OR id = ?`) has no provider guard, so a future billing cooldown could cool credentials across providers. Additionally: schema `CHECK (weight > 0)` means weight 0 reaches a raw SQL constraint error surfaced as an opaque string to the UI (priority is unconstrained — negative values are legal and work with the DESC ordering; document that); `list_health_events(limit)` needs a clamp (e.g. 1..=1000, default 100); `add_provider_account` label/group_key need length bounds and the UNIQUE(provider_id, label) collision deserves a friendly error. Secret-leak check of the new surface: none found — existing views exclude `secret_ref` and health rows carry only bounded non-secret detail; keep `account_id`/health views under the same rule.

**Evidence.** `migrations/0001_init.sql:32,38` (FK, weight CHECK); `apps/desktop/src-tauri/src/commands_management.rs:159-166` (CredentialView pattern — no secret_ref), `:194-216` (no weight validation precedent); plan §3.

**Recommendation.** Plan amendment: same-provider check in `assign_credential_account` (fetch both rows, compare `provider_id`, structured error); validate `weight ≥ 1` (and cap, e.g. 1_000_000) in `set_credential_priority_weight`; clamp `limit`; state "negative priority allowed by design".

---

### R4-08 — Low — RFC-3339 string comparison for cooldown eligibility depends on identical timestamp construction; pin one helper

**Category:** Persistence correctness (edge).

**Description.** The loader compares `cooldown_until > now_iso` as raw strings — valid only while every writer uses `chrono::Utc::now().to_rfc3339()` (`+00:00` offset, AutoSi sub-seconds). Today both writers do, so timezone-format consistency holds. Residual edges: mixed sub-second digit counts (3/6/9) make a value-equal prefix sort before the longer string, so a credential can stay "cooling" up to ~1s past `cooldown_until`; and any future writer using `to_rfc3339_opts(..., true)` (Z) or Secs precision silently corrupts eligibility ordering. Equality at exact `cooldown_until == now` is correctly eligible (`>` strict).

**Evidence.** `crates/sair-persistence/src/repos.rs:619-622`; `crates/sair-persistence/src/time.rs` (now_iso); `crates/sair-gateway/src/api.rs:341`; `migrations/0001_init.sql:2` ("lexicographically sortable" intent).

**Recommendation.** One shared helper (e.g. `sair_persistence::time::rfc3339_fixed(dt)` with a fixed `SecondsFormat`) used by `now_iso` and both cooldown writers, or parse-compare in the loader; add the equality test (`cooldown_until == now` → eligible).

---

### R4-09 — Low — Acceptance gate omits required frontend checks; dependency-audit note missing

**Category:** QUALITY_GATES compliance (carry-over of R3-08).

**Description.** The gate line is "All new tests + full suites green; clippy/fmt clean; frontend builds" — QUALITY_GATES requires frontend format/lint, typecheck, component tests, **accessibility checks**, and production build, and the plan edits/adds two screens (API Keys priority/weight + account selector, Health Monitor). No new dependencies are planned, so the dependency-audit step should be stated as satisfied rather than omitted.

**Evidence.** Plan §Acceptance; `.agent/QUALITY_GATES.md` (Frontend gates); phase3-plan-review R3-08.

**Recommendation.** Add the five frontend gates and the "no new dependencies — audit not applicable" statement to the acceptance section.

---

## Sound areas (verified, no action)

- **Account-group UPDATE under concurrency**: single statement, WAL-serialized, no transaction across awaits (ADR-004); `account_id = NULL` naturally reduces to the individual-cooldown case; `status = 'active'` preserves concurrent quarantines; credentials assigned to a group after a failure stay eligible until the next failure (self-correcting re-cooldown) — acceptable, worth one sentence in the plan.
- **Per-affected-credential health events with `cooldown_start` + correlation id** fit the CHECK set and the existing insert signature (but see R4-01 for the writer fix and R4-03 for the value).
- **Semaphore safety**: permit held across the upstream call is right; no deadlock with the global limit; try_acquire-only keeps latency bounded; map growth is bounded by deployment count and cleared on restart.
- **Secret handling**: new commands/views can follow the existing pattern (secret_ref never serialized; secrets one-way UI→vault; health detail is bounded non-secret upstream messages) — no leak path found in the plan.
- **"No migration required"** is accurate (`provider_accounts`, `credentials.account_id` exist; `deployment_concurrency` is an `app_settings` row); rollback story (additive commits, settings default) is accurate.
- **Repository-state claims** (e314a5d, 88 tests, Phase 3 gate complete) match `reports/phase-reports/phase-3.md`.
- **ADR-012/017/018 compliance** in intent: the rotation mutex, quota-truth stop, and (deployment, credential) candidate model are correctly carried forward; ADR-017's BillingOrQuota-stops-rotation means concurrent billing failures each burn at most one key before group cooldown lands — sound.

---

## Verdict

The plan's direction is right and most of its mechanics survive scrutiny, but it builds the FR-012 surface on a health-events path that silently persists nothing (R4-01), and its own "no storm" gate test asserts an invariant the engine does not enforce (R4-02). Cooldown monotonicity, Retry-After bounds/header support, FR-021 semantics, and command validation need pinning before code.

**VERDICT: APPROVE WITH CHANGES** — resolve R4-01 and R4-02 (mandatory; they reshape the health-event and rotation-loop tasks) and incorporate R4-03, R4-04, R4-05, R4-06, R4-07 in the plan amendment before implementation; R4-08, R4-09 in the same amendment.
