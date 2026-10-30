# Planning Audit — Shahid AI Router (Phase 0)

Auditor: orchestrator agent. Date: 2026-10-30. Inputs: all `docs/`, all `.agent/`, `tasks/`, `PACK_MANIFEST.json`, plus `audits/planning/phase0-research.md`. Scope: audit PRD, architecture, tech stack, security posture, roadmap; identify inconsistencies, gaps, risks; finalize the implementation plan. This document is the pre-implementation plan baseline for independent review.

---

## 1. Repository state

- Documentation-only pack; **no application source exists**. 33 files: 15 docs, 8 agent protocols, 4 empty task boards, manifest/README/LICENSE/CHANGELOG.
- The enclosing git repository is the user's **home directory** (`C:/Users/nacht`), containing unrelated commits (a separate Letzer project). The pack README says "Extract it into the root of a new repository."
  - **Decision:** the pack folder `...\Shahid-AI-Router-Documentation-Pack\Shahid-AI-Router-Documentation-Pack\` becomes the new project repo root; a fresh `git init` will be created there (nested repos are ignored by the outer repo). The home-directory repo will never be committed to.

## 2. PRD audit (docs/01)

Requirements FR-001…FR-016 are coherent and testable in principle. Gaps and ambiguities found:

| ID | Gap | Resolution (proposed amendment) |
|---|---|---|
| G-01 | No graceful-shutdown requirement: behavior of in-flight requests when the app/gateway stops is unspecified. | Add FR-017: gateway stop/app exit drains in-flight requests within a bounded grace period; streaming responses are terminated cleanly. |
| G-02 | Port conflict behavior unspecified. | Add FR-018: bind failure on the configured port is an explicit, surfaced error state; no silent port fallback. |
| G-03 | Gateway token lifecycle incomplete: rotation action not required anywhere except backup import. | Add FR-019: user-facing token regeneration with clear client-impact warning. |
| G-04 | Request-history retention: docs/07 says "configurable retention" but no default or cleanup mechanism. | Add FR-020: retention default 30 days, background cleanup on startup and daily. |
| G-05 | No bound on concurrent upstream requests per deployment. | Add FR-021: per-deployment concurrency semaphore, bounded and configurable. |
| G-06 | App lifecycle vs gateway lifetime ambiguity (docs/03 admits Tauri has no automatic background service). | Add FR-022: v1 gateway runs only while the app process runs; UI must state this explicitly. Tray/resident mode is a post-v1 option. |
| G-07 | FR-006 "where verified" is subjective for scope control. | Operationalized: v1 protocol surfaces = OpenAI-compatible `/v1/chat/completions` (+`/v1/models`), Anthropic `/v1/messages`, Gemini `models/{model}:generateContent` (+`streamGenerateContent`). Non-streaming first, streaming in Phase 7. |

Non-functional requirements are qualitative; quantified targets are added in §7.

## 3. Architecture audit (docs/02, docs/14)

- Modular monolith mandate vs 10-crate diagram in docs/14 conflict superficially; docs/14 itself resolves it ("do not create empty crates"). **Final crate plan (6 crates, each with a real boundary and independent test value):**
  - `sair-domain` — pure types: provider/credential/deployment/alias models, error taxonomy (docs/04 categories), routing policy types, capability metadata. No IO.
  - `sair-persistence` — SQLite via SQLx 0.9, embedded migrations, repositories, all schema from docs/07 entities.
  - `sair-vault` — OS secret storage abstraction (keyring 4.x), in-memory mock for tests, secret-ref validation.
  - `sair-providers` — adapter trait + openai-compatible, openrouter, openai, anthropic, gemini adapters + deterministic mock provider for tests.
  - `sair-gateway` — Axum server: auth, protocol surfaces, alias resolution, routing engine, rotation/failover, health updates, redacted telemetry.
  - `app` (Tauri shell) — commands, state wiring, UI process integration.
  - Crates may be split later when boundaries earn it (record in DECISION_LOG at that time).
- Request flow in docs/02 is sound. Two additions: (a) an explicit **routing decision record** object returned by the router so UI can explain "why this route" (FR-012/UX spec) without re-deriving; (b) the gateway must hold **no DB transaction across an upstream await** (already mandated — enforced by repo-layer design: load state snapshot → decide → fetch secret → call upstream → persist result).
- Concurrency: Tokio multi-thread runtime owned by Tauri's async runtime; rotation state (cooldown, health) guarded by a single-writer `Arc<Mutex>`-free design using atomics/tokio watch where possible; final mechanism chosen in implementation with race-condition tests (Phase 4 gate).

## 4. Tech stack audit (docs/03 + research)

All recommendations validated against current stable versions (see research notes): Tauri 2.12.x, Axum 0.8.x, SQLx 0.9.x, keyring 4.x (+ platform store crates), reqwest 0.13 (rustls default), tokio 1.x, argon2 0.6. Frontend: React 19, Vite, TypeScript, Tailwind 4, vitest, Playwright.

Deviations/decisions requiring ADRs:
1. **SQLx 0.9 (not 0.8):** current stable; MSRV 1.94 ≤ our 1.99.1. Requires `AssertSqlSafe` discipline for any dynamic SQL; we use static SQL + type-safe query builders only.
2. **keyring 4.x facade** with `windows-native` / `apple-native` store features; our own `SecretStore` trait isolates churn; in-memory store for tests only (never in release builds).
3. **reqwest 0.13 rustls** backend; redirect handling manual with per-hop SSRF re-validation and auth-header stripping on host change (defense-in-depth beyond reqwest 0.13.4's built-in stripping).
4. **Encrypted backup crypto (Phase 10):** Argon2id (m=64 MiB, t=3, p=1 — finalize at implementation) + AES-256-GCM via RustCrypto `aes-gcm`; format versioned; algorithms recorded in the `.sair` manifest. No custom primitives.
5. **TypeScript 7.0.2 vs 5.9.x:** decide at scaffold; if any toolchain component (ESLint parser, plugins) fails with TS7, fall back and record.
6. **CI:** GitHub Actions, matrix windows-latest + macos-latest (compile/tests cross-platform evidence, since macOS runtime verification is impossible on this Windows machine).

## 5. Security audit (docs/06) — design commitments

- Loopback bind `127.0.0.1` only; token = 32 random bytes (OS CSPRNG) base64url, stored in OS vault, `Authorization: Bearer` + `x-api-key` + `x-goog-api-key` accepted for client compatibility.
- No CORS headers emitted (local non-browser clients; browsers are not a v1 client target). Request body limit 32 MiB default; concurrency limit; per-request timeout budget.
- SSRF: custom base URLs must be https; `http` allowed only for explicit loopback literals. For non-loopback hosts: resolve + reject private/link-local/metadata ranges before connect, re-validate per redirect hop, strip auth on host change, reject non-HTTP schemes and CRLF in headers (reqwest rejects invalid header values; still validated at adapter boundary).
- Secrets: only in OS vault; DB holds `secret_ref`; tracing layer redacts by construction (secret values never enter spans; guard test asserts no key material in logs/snapshots); UI masks by default.
- Import (Phase 10): size caps, strict schema validation, transactional staging, passphrase KDF, never execute imported content, new gateway token on restore.
- Tauri: minimal capabilities file, CSP locked down (no remote content), no shell/plugin permissions beyond `opener` for docs if needed.

## 6. Testing strategy (docs/11)

- Unit: domain logic, error classification, redaction, SSRF validator.
- Adapter contract tests: mock upstream (deterministic HTTP server, e.g. `wiremock`) asserting request shape, error mapping, streaming semantics per protocol.
- Integration: gateway end-to-end over loopback with mock provider: auth, routing, rotation, cooldown, retry budget, stream-commit boundary, oversized/malformed requests.
- Migration tests: from every historical schema version forward + interrupted migration recovery.
- E2E: Phase 1+ smoke via Tauri WebDriver (tauri-driver + msedgedriver on Windows) scoped to critical flows; if tooling proves unstable on Windows, downgrade to HTTP-level E2E + component tests and disclose the limitation (recorded in decision log at that time).
- Security tests: secret-redaction sweep, loopback-only bind assertion, auth negative tests, SSRF matrix (local metadata IPs, redirect hops, header injection).
- Performance: gateway overhead budget p99 < 25 ms added latency for proxied non-streaming calls on loopback with mock upstream (v1 target; measure in Phase 11).

## 7. Finalized roadmap (maps to docs/13; adds acceptance gates)

| Phase | Scope (summary) | Exit gate |
|---|---|---|
| 0 Discovery | Research (done), audits, plan review, ADRs | This document + review + decision log, approved |
| 1 Skeleton | Cargo workspace, Tauri shell, React UI, CI, fmt/clippy/tsc/vitest wiring | Clean `cargo build` + `npm run build` + CI green |
| 2 Security/persistence | SQLite schema+migrations, vault, gateway token, tracing redaction | Secret lifecycle tests: store→ref→retrieve→delete, no plaintext paths |
| 3 First vertical slice | OpenAI-compatible adapter (mock upstream), one credential, alias resolution, non-streaming gateway call, request record | E2E: curl → gateway → mock upstream → logged record |
| 4 Rotation | Credential pool, health, error classification, cooldown, account grouping | Simulated failure suite incl. race tests |
| 5 OpenRouter + aliases | Second adapter via same protocol, alias/capability resolution | Alias/capability matrix tests |
| 6 Anthropic + Gemini native | Protocol surfaces + adapters | Protocol contract tests incl. header/body fidelity |
| 7 Streaming | SSE pass-through for all three surfaces, stream-commit failover boundary | Streaming contract + no-failover-after-commit tests |
| 8 Agent profiles | Profile generation, copyable config, consent-gated external edits | Verified against ≥2 real agent config formats (documented versions) |
| 9 UI completion | All screens per docs/08, logs, usage, diagnostics | Component + a11y checks + manual checklist |
| 10 Backup/restore | `.sair` config-only + encrypted full, import validation pipeline | Encrypted roundtrip + tamper rejection + cross-platform restore (CI) |
| 11 Hardening/release | Perf budget, security review pass, clean-install checklist, SBOM | Release checklist executed; signing requires human-provided certs (expected BLOCKED item) |

Execution order constraint (docs/13): one feature at a time through the full workflow state machine; no parallel features sharing files.

## 8. Risks

| ID | Risk | Likelihood | Mitigation |
|---|---|---|---|
| R-01 | TypeScript 7 tooling lag | Medium | Fall back to 5.9.x; ADR at scaffold |
| R-02 | MSVC/WebView2 build friction on this machine | Low–Med | Early Phase 1 build as proof; BuildTools verified present |
| R-03 | macOS cannot be runtime-verified here | Certain | CI (macos-latest) compile+test evidence; explicit platform evidence matrix in reports; no macOS claims without it |
| R-04 | keyring v4 API churn (recent major) | Medium | `SecretStore` trait isolation; contract tests; pin exact versions |
| R-05 | Provider API drift (e.g., Gemini Interactions API) | Medium | Re-verify per phase; adapters behind traits; mock contract tests |
| R-06 | Total scope exceeds any single session | Certain | Phase gates + honest task boards; visibly incomplete = correct state per PRD acceptance rule |
| R-07 | Signing/notarization needs external certs/accounts | Certain | Documented BLOCKED dependency for release; all other work proceeds |

## 9. Documentation updates required by this audit

- docs/01: append FR-017…FR-022 amendments (§2).
- docs/DECISION_LOG.md: record ADR-001…ADR-012.
- docs/TRACEABILITY_MATRIX.md: map FRs → phases/tasks (maintained continuously).
- tasks/: populate boards from §7 breakdown.

## 10. Plan status

READY FOR INDEPENDENT REVIEW. Implementation starts only after reviewer findings are resolved per the workflow state machine.
