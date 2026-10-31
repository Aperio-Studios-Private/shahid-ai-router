
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
