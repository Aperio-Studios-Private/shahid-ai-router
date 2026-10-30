# Independent Plan Review — Shahid AI Router (Phase 0)

- **Reviewer:** independent plan reviewer (did not author the plan; orchestrator-authored `audits/planning/planning-audit.md` + `audits/planning/phase0-research.md` under review)
- **Date:** 2026-10-30
- **Plan version reviewed:** planning-audit.md and phase0-research.md as committed in baseline commit `69e7e92` (pack version 0.1.1), together with all `docs/` and `.agent/` files
- **Method:** full read of docs/00–14, `.agent/` protocols, task boards, decision log, traceability matrix; independent re-verification of version/API claims against crates.io, npm registry, docs.rs/changelog sources, and this machine's toolchain; adversarial review of scope, architecture, security, failure handling, testing and cross-platform claims.

---

## A. Independent verification of the plan's factual claims

Per the review mandate I spot-checked the load-bearing claims myself rather than trusting the research notes.

**Verified correct (evidence: crates.io API, npm registry, sqlx CHANGELOG on `main`, local machine — all accessed 2026-10-30):**

- Crate versions: `tauri 2.12.5`, `axum 0.8.9`, `sqlx 0.9.0` (MSRV `rust-version = 1.94.0`, not yanked), `keyring 4.2.0`, `reqwest 0.13.5`, `argon2 0.6.0`, `tokio 1.54.1`, `keyring-core 1.0.1`. All match the research table exactly.
- sqlx 0.9 breaking-change claims all verified against the changelog: "all `query*()` functions now take `impl SqlSafeStr`" (only `&'static str` and `AssertSqlSafe` implement it), "`SqliteValue` is now `!Sync` and `SqliteValueRef` is `!Send`", SQLite `no_tx` migrations, `Migrator::with_migrations()`. The plan's persistence discipline (static SQL only, snapshot-load → decide → call upstream → persist, no transaction across awaits) is the correct response.
- keyring 4.x structure: facade over `keyring-core` with platform stores as **separate crates** — `windows-native-keyring-store 1.1.0`, `apple-native-keyring-store 1.0.2`, `linux-keyutils-keyring-store 1.0.0` all exist on crates.io. The research is right (but see finding R-4: the planning audit contradicts it).
- npm versions: `typescript 7.0.2`, `vite 8.3.5` (engines `^20.19.0 || >=22.12.0`), `react`/`react-dom 19.3.1`, `@tauri-apps/cli`/`api 2.12.4`, `tailwindcss 4.3.3`, `vitest 5.0.5` (engines include `>=26.0.0`), `@playwright/test 1.64.1`. All match.
- Tauri 2 + Vite 8: Tauri is bundler-agnostic (dev server URL + `frontendDist`); no known incompatibility with Vite 8 found. Sound.
- Local toolchain: rustc/cargo `1.99.1` (stable-x86_64-pc-windows-msvc, present in `%USERPROFILE%\.cargo\bin`), Node `v26.3.1`, npm `11.16.0` — matches the research. Note: cargo is **not on this shell's PATH**; Phase 1 must confirm the environment used by CI and the agent is the same one the research audited.
- Agent protocol facts: Anthropic `/v1/messages` with `x-api-key` + pinned `anthropic-version`, Gemini `x-goog-api-key` + `v1beta`, OpenRouter OpenAI-compatible with attribution headers — consistent with current official docs. OpenAI's own docs remain unverified (403 to automated fetch) and are honestly flagged as such in the research.

The research quality is unusually good: **every claim I checked independently was accurate**, with facts/assumptions properly separated. The findings below are therefore mostly about scope, consistency, and environment gaps the research did not cover.

---

## B. Findings

Severity scale per `.agent/QUALITY_GATES.md` (Critical / High / Medium / Low / Informational).

---

### R-1 — High — Architecture/Scope: cross-protocol translation is implied by the docs but never scoped, scheduled, or excluded

**Category:** architectural problem / unstated assumption.

**Description:** G-07 operationalizes FR-006 into three v1 protocol surfaces (OpenAI-compatible `/v1/chat/completions` + `/v1/models`, Anthropic `/v1/messages`, Gemini `:generateContent`/`:streamGenerateContent`). It never states whether an inbound request on one surface may be routed to a deployment whose upstream speaks a **different** protocol. Yet:

- docs/01 FR-004 defines aliases over "eligible deployments" with no protocol constraint; docs/02's flow (`protocol parser → alias resolution → capability filtering → routing → provider adapter`) and docs/04 ("provider-specific passthrough **where normalization loses semantics**") both presuppose a normalization/translation layer exists for the cases where semantics *are* preserved.
- No phase in the finalized roadmap (§7) delivers a translator. Phase 5 delivers aliases, Phase 6 delivers native adapters, Phase 7 delivers streaming — but OpenAI↔Anthropic↔Gemini request/response translation (tool calling, system prompts, content blocks, error mapping, usage semantics, streaming event mapping) is a project-sized feature on its own, and per-protocol streaming event semantics (FR-010) make translated streaming much harder than passthrough.

If this stays unresolved, Phase 5/6 will discover the ambiguity mid-implementation: either the alias model silently constrains routing (undeclared behavior change) or the plan silently absorbs a large unpriced scope.

**Evidence:** docs/01 FR-004/FR-006; docs/02 "Protocol design"; docs/04 §Adapter contract; planning-audit §2 G-07 and §7 phases 5–7 — no mention of protocol translation or a protocol-compatibility constraint on alias eligibility.

**Recommended resolution:** Amend G-07 with an explicit rule: **v1 routes same-protocol only** — a deployment is eligible for an inbound request only if its adapter supports the inbound surface's protocol; the routing decision record must report "incompatible protocol" as an explicit exclusion reason (this also strengthens FR-012). Cross-protocol translation becomes a named post-v1 epic in the roadmap. Update FR-004 and docs/02 wording accordingly. Cost: one paragraph now; otherwise days of rework or silent scope creep later.

---

### R-2 — High — Provider protocols: the v1 surface list omits auxiliary endpoints and config constraints that the plan's own target agents require

**Category:** missing requirement / provider protocol compatibility (mandate item: protocol-surface scope in G-07).

**Description:** docs/09 lists OpenAI Codex CLI and Claude Code as candidate clients, and the Phase 8 gate requires profiles "verified against ≥2 real agent config formats". Verified facts the plan does not account for:

1. **Codex CLI defaults to `wire_api = "responses"`** (the OpenAI Responses API, `/v1/responses`). It works against a chat-completions-only gateway only if the generated profile sets `wire_api = "chat"` in the custom provider block. G-07 excludes `/v1/responses` from v1 — legitimate — but the plan nowhere records that Codex profiles are chat-only, nor that `/v1/responses` is a deliberate exclusion.
2. **Claude Code calls `POST /v1/messages/count_tokens`** in addition to `/v1/messages` for context management; a minimal compatible proxy implements both (some flows fall back to a `max_tokens: 1` probe, but behavior is version-dependent and undocumented). G-07 lists only `/v1/messages`.
3. **Base-URL conventions differ per client** (e.g., Claude Code's `ANTHROPIC_BASE_URL` must be the root without `/v1`; Codex provider blocks carry `base_url` + `env_key` + `wire_api`). The profile generator (FR-013) needs these rules or generated configs will 404.

None of this breaks the architecture, but the Phase 8 gate cannot pass with the current G-07 surface list unless these decisions are made explicit; discovering them at Phase 8 invites unplanned surface work.

**Evidence:** planning-audit §2 G-07 (surface list); docs/09 candidate clients; Codex `config.toml` `wire_api` semantics (openai/codex docs and issues #3848/#46470); community/official guidance for Claude Code custom proxies requiring `/v1/messages/count_tokens` and root base URLs.

**Recommended resolution:** Append a "surface scope appendix" to G-07: (a) `/v1/responses` is out of v1 scope; Codex profiles must set `wire_api = "chat"` — record as ADR; (b) decide `/v1/messages/count_tokens` v1 behavior (implement an approximate counter, or return a documented 404/501 and record the degraded Claude Code behavior as a profile limitation); (c) profile generator rules per client (root base URL for Anthropic clients, `wire_api`, `env_key`); (d) note Gemini `:streamGenerateContent` requires `alt=sse` handling in Phase 7. Re-verify against pinned agent versions at Phase 8 per docs/09.

---

### R-3 — Medium — Build/toolchain: reqwest 0.13's default TLS backend (aws-lc-rs) needs CMake/NASM on Windows; neither is installed and the toolchain audit missed them

**Category:** technical incompatibility / environment gap.

**Description:** The plan adopts reqwest 0.13 with its rustls default. reqwest 0.13's default feature set resolves to the **aws-lc-rs** crypto provider (`default-tls => ["rustls"]` → `__rustls-aws-lc-rs`), and `aws-lc-sys` on Windows MSVC requires CMake and NASM (and possibly clang) to build — the well-known "Windows compilation hell" (reqwest issue #2937). I verified this machine: **CMake NOT FOUND, NASM NOT FOUND**. The Phase 0 toolchain audit checked MSVC Build Tools but not these. Two aggravating details:

- GitHub Actions `windows-latest` runners have CMake preinstalled, so **CI would pass while the primary dev machine fails** — an expensive divergence to debug in Phase 3.
- The audit's ADR-3 ("reqwest 0.13 rustls backend") records no provider decision, so the failure will surface as an opaque `aws-lc-sys` build error.

**Evidence:** reqwest 0.13.5 feature tree on crates.io (`default-tls => ['rustls']`, `rustls => ['__rustls-aws-lc-rs', …]`); reqwest#2937; aws-lc-sys Windows build docs; local check `where cmake` / `where nasm` both negative (2026-10-30).

**Recommended resolution:** Resolve before Phase 1 by ADR, one of: (a) install CMake + NASM as documented dev prerequisites (and note CI has them); (b) use `reqwest` with `default-features = false` + `rustls-no-provider` and install the **ring** provider explicitly (avoids aws-lc-sys entirely; confirm feature support at pin time); or (c) keep aws-lc-rs and rely on `AWS_LC_SYS_PREBUILT_NASM` / prebuilt bindings, verified by a Phase 1 spike. Whichever is chosen, add it to the Phase 1 gate ("clean cargo build") on this machine, not only in CI.

---

### R-4 — Medium — Internal inconsistency: planning audit's keyring ADR names features that do not exist in keyring 4.x

**Category:** technical incompatibility / ADR correctness (mandate item: keyring 4.x feature names).

**Description:** planning-audit §4 decision 2 says: "keyring 4.x facade with `windows-native` / `apple-native` **store features**". The research notes correctly state the opposite structure: keyring 4 is a facade over `keyring-core` and platform stores are **separate crates**. I verified against the crates.io metadata for keyring 4.2.0: its only features are `cli`, `default`, `v1` — **no `windows-native` or `apple-native` features exist**. If this ADR is copied into `docs/DECISION_LOG.md` as written (§9 of the audit commits to recording ADR-001…ADR-012), Phase 2 will stall on nonexistent feature names, or worse, someone will "fix" it by pinning keyring 3.x, contradicting the researched decision.

**Evidence:** crates.io `keyring/4.2.0` feature list (`['cli', 'default', 'v1']`); existence of `windows-native-keyring-store 1.1.0` and `apple-native-keyring-store 1.0.2`; phase0-research §2 keyring 4.x vs planning-audit §4 item 2.

**Recommended resolution:** Correct ADR-2 before recording: depend on `keyring` 4.2 (facade) **plus** `windows-native-keyring-store` and `apple-native-keyring-store` as direct dependencies — or, per the maintainers' own recommendation for fine-grained control (which the research cites), `keyring-core` + the two store crates directly behind the project's `SecretStore` trait. Either way, pin exact versions and note the facade contributes no store by itself.

---

### R-5 — Medium — Security: no Host/Origin validation; DNS-rebinding and browser-origin hardening is absent from the gateway middleware list

**Category:** security weakness (mandate item: loopback gateway auth).

**Description:** The gateway security list (§5: loopback bind, bearer token in three header forms, no CORS, body/concurrency/timeout limits) lacks **`Host` header validation** (allowlist `127.0.0.1:<port>` / `localhost:<port>`) and any `Origin`/cross-origin request policy. docs/06's threat model includes "malicious local clients" but the plan's mitigations address none of the classic browser-to-localhost attack class: a webpage (or DNS-rebound hostname resolving to 127.0.0.1) can send *simple* cross-origin POSTs to loopback without preflight regardless of CORS response headers, and a rebound origin reads responses as same-origin. Mandatory token auth keeps the practical risk bounded (an attacker who cannot read the token cannot authenticate), which is why this is Medium and not High — but the hardening is one middleware and two negative tests, and it closes the class rather than leaning on token entropy.

**Evidence:** planning-audit §5 (middleware enumeration); docs/06 §Gateway/§Threat model (no browser-origin/rebinding item).

**Recommended resolution:** Add to the Phase 2/3 gateway gate: `Host` allowlist middleware (reject others with 403), reject requests bearing a foreign `Origin` header, and negative tests ("rebound host", "cross-origin POST") in the security test list §6.

---

### R-6 — Medium — Security/SSRF: resolve-then-connect has a TOCTOU gap, and the IP denylist is under-specified for IPv6 and edge ranges

**Category:** security weakness (mandate item: SSRF policy for custom provider URLs).

**Description:** §5's SSRF design validates by resolving the host and rejecting private/link-local/metadata ranges *before* connecting, then "re-validates per redirect hop". Two gaps:

1. **TOCTOU/DNS rebinding:** the DNS answer used for validation and the address the connection actually dials are separate lookups; an attacker-controlled custom-endpoint host can rebind between them. The plan already requires DNS revalidation for redirects but has no mechanism pinning the validated address for the first connection.
2. **Denylist enumeration:** "private, link-local and metadata destinations" does not state IPv6 coverage (ULA `fc00::/7`, link-local `fe80::/10`, IPv4-mapped IPv6 `::ffff:0:0/96` — the classic bypass), nor `0.0.0.0`, `100.64.0.0/10` (CGNAT), or the IPv6 metadata equivalents. The SSRF test matrix (§6) lists "local metadata IPs" but not these.

**Evidence:** planning-audit §5 SSRF bullet and §6 security tests; docs/06 §SSRF.

**Recommended resolution:** (a) Pin the validated IP for the actual connection — e.g., resolve, validate, then pass the chosen address via reqwest's per-request `resolve()`/custom connector so check and dial are the same address; re-validate on each redirect hop with the same mechanism. (b) Write the denylist as explicit CIDR enumeration including IPv6 ULA/link-local/mapped forms and `0.0.0.0`/`100.64.0.0/10`; unit-test the validator against that table and add a rebinding-style mock (DNS answer changes between lookup and connect) to the SSRF matrix.

---

### R-7 — Medium — Concurrency design: rotation/cooldown state mechanism is deferred to implementation ("atomics/tokio watch where possible")

**Category:** architectural problem (mandate item: cooldown races, concurrency design).

**Description:** §3 says rotation state (cooldown, health) will use "atomics/tokio watch where possible; final mechanism chosen in implementation with race-condition tests (Phase 4 gate)". This defers the single most race-prone core decision to the middle of the project, and the "lock-free where possible" aspiration invites premature cleverness: cooldown expiry racing in-flight failure classification, LRU bookkeeping vs concurrent selections, and priority reordering are all state-machine problems, not throughput problems — at desktop scale (a handful of deployments, single user) contention is irrelevant. Lock-free atomics here make the mandatory "concurrent rotation" test (docs/11) non-deterministic and the code harder to review.

**Evidence:** planning-audit §3 "Concurrency"; docs/02 §Concurrency; docs/11 mandatory scenario "concurrent rotation".

**Recommended resolution:** Commit now in the plan: rotation/cooldown/health state lives behind a single `tokio::sync::Mutex<RotationState>` (or `RwLock`), with the invariant already mandated elsewhere — no DB access and no network await while holding the lock (snapshot in, decision out, event log appended). Health changes are recorded as ordered events (`health_events` table already exists in docs/07), which makes the Phase 4 race tests deterministic and the state reconstructable for the UI. Record as ADR; revisit only with measured evidence.

---

### R-8 — Low — Data model/backup: config-only export semantics for dangling `secret_ref`s are undefined

**Category:** missing requirement.

**Description:** Config-only export excludes secrets but necessarily includes credential rows carrying machine-local `secret_ref` values (opaque OS-vault references). docs/10's import flow validates "schema/reference" and "reports skipped entries" but never defines what happens to credentials whose refs point at vault items that do not exist on the destination machine. The obvious behavior (import credential as disabled/`missing_secret`, surface in health UI, prompt re-entry) is unstated, and the wrong behavior (reusing or guessing destination vault items) would be a correctness/privacy bug.

**Evidence:** docs/10 export mode 1 + import flow; docs/07 secret lifecycle (secret_ref is machine-local by design).

**Recommended resolution:** Add one sentence to FR-014/Phase 10 acceptance: config-only import marks credentials `missing_secret` (visible, excluded from routing), never rewrites or guesses vault references; encrypted-full import re-stores into the destination vault per docs/07.

---

### R-9 — Low — Release: supported minimum OS versions/architectures are required by docs/12 but undecided

**Category:** missing requirement.

**Description:** docs/12 mandates "Define supported macOS/Windows minimum versions and architectures" as a build/release input, but neither the audit nor the roadmap states them (e.g., Windows 10+ x64 only, macOS 12+ on aarch64/x86_64, aligned with the Tauri 2 baseline and WebView2 Evergreen). This also affects CI matrix value (macos-13 vs macos-14 runners differ in architecture) and the macOS-evidence matrix (R-03).

**Evidence:** docs/12; planning-audit §4 item 6 (CI matrix) and §7 Phase 11.

**Recommended resolution:** Decide and record (ADR) at Phase 1 so the CI matrix and the platform evidence matrix use the same baseline; re-check against Tauri 2's minimum supported versions at that time.

---

### R-10 — Informational — Process: §1 repo-state description is already stale; git identity and `.gitignore` still missing

**Category:** repository hygiene.

**Description:** planning-audit §1 describes the git decision as future work ("a fresh `git init` will be created there"), but the pack directory already **is** a dedicated git repo with a baseline commit (`69e7e92`, containing the pack + the plan itself). The decision is effectively executed — the audit text should not mislead the next reader. Separately, no `.gitignore` and no committed git identity exist; both are Phase 1 prerequisites (and the DECISION_LOG required by §9 is still the empty template).

**Evidence:** `git rev-parse --show-toplevel` in the pack directory resolves to the pack root; `git log` shows `69e7e92 docs: baseline …`; no `.gitignore` present.

**Recommended resolution:** No rework. When writing DECISION_LOG entries (§9), record the repo-bootstrap state as done, and add `.gitignore` + identity as the first Phase 1 tasks.

---

## C. Areas reviewed and found sound (one line each)

- **Crate boundaries:** the 6-crate reduction from docs/14's 10-crate diagram is a genuine improvement; `sair-domain` purity (no IO) and provider adapters under one trait are the right seams; "no empty crates" discipline retained.
- **Tauri/Axum single-process integration:** in-process Axum on Tauri's Tokio runtime, gateway lifetime bound to app process (FR-022) with explicit UI disclosure, bind-failure surfaced (FR-018), drain-on-stop (FR-017) — coherent and honestly scoped.
- **Database/migrations:** SQLx 0.9 with static-SQL discipline, embedded migrations, migrate-from-every-version tests, no transactions across awaits — the correct posture given the verified 0.9 breaking changes; no risks beyond the research already documented.
- **Backup crypto plan:** Argon2id + AES-256-GCM via RustCrypto, versioned format, algorithms finalized only at implementation with security review, no custom primitives — sound; parameters (m=64 MiB) appropriately deferred.
- **Stream-commit and retry rules:** no failover after stream commitment, Retry-After respect, no blind retries on InvalidRequest/ContextLimit, ambiguity surfaced instead of blind retry on timeouts, quota-truth/account-grouping rules — all match docs/05 and are testable as staged.
- **Testing coverage:** every docs/11 mandatory scenario maps to a phase gate in §7 (concurrent rotation → Phase 4, stream commitment → Phase 7, tampered import + cross-platform restore → Phase 10 CI, migration → Phase 2/10); E2E tooling risk on Windows is honestly flagged with a disclosed fallback.
- **Log redaction:** secrets-by-construction (never entering spans) plus a guard test is stronger than scrubbing, and matches docs/06/07.
- **Cross-platform honesty (R-03):** no macOS runtime claims without CI evidence; signing/notarization correctly pre-declared as a human-provided-certificate BLOCKED item rather than assumed.
- **TS 7 / toolchain churn risk (R-01):** fallback to 5.9.x with ADR is the right hedge; vitest 5 and Vite 8 engines both accept the installed Node 26.

---

## D. Verdict

No Critical findings. Two High findings (R-1, R-2) are scope ambiguities with concrete, low-cost amendments that must be folded into G-07/FR-004 **before Phase 5–8 work begins** (R-2's profile rules before Phase 8; both are documentation-level changes now, rework-level if discovered later). R-3 must be resolved by ADR before the Phase 1 gate. The remaining Medium findings are plan amendments foldable into the DECISION_LOG and phase gates without restructuring.

**VERDICT: APPROVE WITH CHANGES**

Required before the plan is finalized (all are plan-text/ADR changes, not redesign):
1. R-1: declare v1 same-protocol-only routing in G-07/FR-004.
2. R-2: add the agent surface-scope appendix (Codex `wire_api="chat"`, `/v1/messages/count_tokens` decision, per-client base-URL rules) to G-07/Phase 8.
3. R-3: TLS-provider ADR (CMake/NASM prerequisites or ring provider) before the Phase 1 gate.
4. R-4: correct the keyring ADR wording (separate store crates, not features) before recording ADR-001…ADR-012.
5. R-5/R-6/R-7: fold Host/Origin validation, SSRF IP-pinning + full denylist enumeration, and the concrete rotation-state mechanism into the DECISION_LOG and the Phase 2–4 gates.

Recommended (non-blocking): R-8 (dangling secret_ref semantics), R-9 (minimum OS baselines), R-10 (repo bootstrap note in DECISION_LOG).
