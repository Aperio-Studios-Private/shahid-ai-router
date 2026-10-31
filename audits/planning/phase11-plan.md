# Phase 11 Plan — Hardening / Release Readiness

Feature ID: PHASE-11. Date: 2026-10-31. Author: orchestrator. Status: DRAFT for independent review.

## PRD refs
docs/12 (pipeline: format/lint; Rust tests; frontend; adapter contracts; integration/E2E; dependency/security audit; platform builds; clean-machine install; gateway lifecycle + vault verification; signing/notarization; checksums; release notes; SBOM), docs/13 Phase 11 gate, docs/11 performance + platform tests.

## Repository state
Phase 10 complete (c8c7d0e): 135 tests green; all functional phases delivered.

## Design

### 1. Performance budget (docs/11, plan §6: p99 < 25 ms gateway overhead)
- Measurement: integration test measuring loopback relay latency (gateway → mock upstream) vs direct client→mock, 200 iterations, report p50/p95/p99; budget check p99 < 25 ms (advisory — CI variance noted).
- Deferred raw-TCP scenarios (Phase 7 F-4): mid-stream drop, stream-outliving-timeout, drain-with-open-stream, permit-release-on-disconnect — implemented now with a raw TCP mock upstream (no wiremock), closing the last deferred traceability items.

### 2. Final security review (independent subagent)
Scope: full diff since Phase 2 security review; focus on the new surfaces (streaming commit boundary, backup crypto, agent-profile file application, usage/logs data exposure). Threat-model addendum refreshed (Phase 9/10 additions). cargo audit re-run.

### 3. Clean-install checklist
- `cargo build --release` + `tauri build --no-bundle` (release) evidence.
- First-run behavior on a clean data dir (temp HOME override): migrations, token mint, gateway autostart, then UI smoke.
- Uninstall safety: user data under app-data dir is NOT removed by default (documented).
- Signing/notarization: requires owner certificates (BLOCKED, owner action) — documented.

### 4. SBOM + checksums
- `cargo audit` report archived; SBOM via `cargo tree` JSON (CycloneDX needs extra tooling — record tree-based inventory; full SBOM tooling post-v1, documented).
- Release archive checksums (SHA-256) for built artifacts.

### 5. Release notes + version
- Version 0.1.0 final; CHANGELOG refresh; traceability finalized (all FRs to final states with residuals).

## Acceptance
p99 budget measured; deferred scenarios implemented + passing; security review verdict (no open Critical/High); clean-install checklist executed; SBOM/checksums archived; report + push. Remaining known gaps: CI execution (B-01), macOS runtime verification (owner), signing (owner certs).

## Review resolution (2026-10-31) — verdict: REVISE BEFORE IMPLEMENTATION (resolved)

Report: `audits/planning/phase11-plan-review.md`. All incorporated:

| ID | Sev | Resolution |
|---|---|---|
| F-11-1 | High | Clean-install test does NOT use env override. App-level data-dir override is NOT added in v1 (new attack surface). Instead: (a) the app binary's own data dir is exercised on THIS machine only for the existing smoke evidence; (b) a NEW integration test creates its own `Db::open(tempfile)` — the gateway/migration/vault lifecycle is already temp-isolated by construction in tests; (c) the true clean-machine install verification is recorded as BLOCKED pending a fresh VM/second user (owner action), consistent with docs/12 honesty. |
| F-11-2 | High | `tauri build --no-bundle` documented as installer-BLOCKED (bundle requires NSIS tooling/signing — owner action). Release claim text gains: no auto-updater in v1 (explicit), upgrade-from-previous N/A (first release), `npm audit` added to the pipeline alongside cargo audit. |
| F-11-3 | Medium | Scenario specs pinned: (2) stalled-stream uses a `header_timeout_ms` knob made configurable in GatewayConfig (default 10s, floor 1s — same floors as settings) and asserts the client connection drops within budget; (3) drain asserts BOTH the graceful path (in-flight completes) and `DrainTimedOut` abort (in-flight exceeds grace), documenting the inner 30s cap; (4) two credentials seeded, asserting the cooled one receives ZERO upstream hits after cooldown starts. |
| F-11-4 | Medium | Perf = PAIRED DELTA on non-streaming requests (gateway-relayed vs direct), 200 iterations with 20 warmup, interleaved A/B, multi-thread runtime, REAL OS vault (Windows Credential Manager — production cost included). Budget stays advisory (p99 delta < 25ms). |
| F-11-5 | Medium | `cargo-cyclonedx` adopted for a real CycloneDX SBOM; cargo-tree inventory kept as secondary. |
| F-11-6 | Medium | Traceability: FR-023 updated (count_tokens implemented); docs/06 threat-model addendum gains the SSRF-race + DNS-rebinding revalidation outcomes (both re-reviewed, residual documented); release claim fixed to "code-complete with residuals" — BLOCKED items (CI, macOS, signing) require the owner's authorized waiver per docs/11 before any release tag. |

Status: READY FOR IMPLEMENTATION.
