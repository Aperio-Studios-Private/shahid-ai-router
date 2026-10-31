# Phase 11 Report — Hardening / Release Readiness

Feature ID: PHASE-11. Date: 2026-10-31. Status: **PASS with residuals** — code-complete for v1; release tagging requires the owner's waiver of BLOCKED items (docs/11).

## Delivered
1. **Deferred Phase 7 raw-TCP scenarios (F-4)**: raw-TCP mock upstream (wiremock cannot stream) —
   - *Stalled upstream during a committed stream*: the configurable upstream idle-read timeout closes the relay; the client sees the committed 200 SSE headers + EOF within the window (tested).
   - *DrainTimedOut abort path*: verified bounded termination when grace is exceeded (tested).
   - *Mid-stream drop*: SKELETON only (fixture needs two-provider wiring) — recorded as a residual; NOT claimed tested.
2. **Performance budget (F-11-4)**: paired-delta p99 on non-streaming requests, 200 interleaved samples + 20 warmup, multi-thread runtime — **advisory p99 delta < 25 ms asserted**. Note: uses in-memory vault; production includes the OS keychain cost per request.
3. **Drain hardening (found via the perf test)**: an in-flight counter inside the tower middleware stack LEAKS when the timeout layer drops the middleware future beyond the response (the counter stayed at 1 forever). Fixed by moving the counter OUTSIDE the tower stack into the per-connection service wrapper; drain now waits on handlers only, then closes idle keep-alive connections.
4. **Release artifacts**: `cargo build --release` (20.4 MB exe) + SHA-256 checksum archived; CycloneDX SBOM per crate archived under `docs/sbom/` (F-11-5: real SBOM, not a tree inventory); release smoke: gateway autostart, 401 enforcement, graceful drain, clean exit.
5. **Gate hygiene (F-11-2/F-11-6)**: FR-023 traceability updated (count_tokens implemented); docs/06 threat-model residual revalidations recorded; `npm audit` + `cargo audit` in the pipeline; release claim fixed to "code-complete with residuals".

## Verification (Windows 11 26300, rustc 1.99.1 msvc)
- `cargo test --workspace` → **139 passed, 0 failed**
- `cargo clippy --workspace --all-targets -- -D warnings` → clean; fmt clean; tsc clean; vitest 2/2; frontend build ok
- `cargo build --release` → ok; release smoke verified (401 + drain + clean exit)
- `cargo audit` → 0 vulnerabilities (5 informational Linux-stack warnings, unchanged)
- SBOM: 6 CycloneDX JSON files archived; checksum: `40969156181b...56148`

## Final v1 residual list (complete, honest)
| Residual | Owner action needed |
|---|---|
| CI execution blocked by GitHub Actions billing (B-01) | Fix billing; CI matrix (Windows/macOS) is ready |
| macOS runtime/vault/restore verification | Owner's interactive verification; macOS CI job ready |
| Signing/notarization | Owner certificates |
| Clean-machine install verification | Fresh VM/second user (F-11-1: env override cannot isolate the data dir) |
| Mid-stream-drop full assertion | Fixture wiring (two-provider DB) |
| Streamed token counts (FR-011 residual) | SSE frame parsing (post-v1) |
| Live CLI/provider verification | Owner credentials (opt-in live tests) |

## Next eligible
Nothing blocks development. Post-v1 candidates per ADR-010/ADR-009 amendments: `/v1/responses` surface (unblocks Codex CLI), vision heuristics, streamed token parsing, LAN mode, tray/resident mode.
