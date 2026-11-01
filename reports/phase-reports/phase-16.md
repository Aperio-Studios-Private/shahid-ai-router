# Phase 16 Report — Automation & ease pass

**Status: PASS** (19/19 Rust tests, clippy -D warnings, fmt, tsc/vitest/build; CI on push)

## Shipped
- **Auto-detected capabilities** (automation #3): `fetch_provider_models` now returns
  `ModelInfo { id, streaming, tool_calling, vision, structured_output, context_window }` —
  OpenRouter's `architecture.input_modalities` → vision, `supported_parameters` →
  tools/structured_output, `context_length` → context window; other providers get safe
  chat defaults. The model picker shows vision/tools/context badges.
- **One-click deploy** (#2): clicking a fetched model deploys it immediately
  (idempotent `add_model`/`add_deployment` from the user-reported UNIQUE crash fix);
  "deployed ✓" state per row.
- **Connection block** (#5): Home card with the live gateway URL and one-click
  "Copy URL + key" (token via the existing reveal IPC, clipboard only, never logged).
- **Automatic-setup wizard** (#1): one dialog — preset → API key → alias name — chains
  add_provider → add_credential → fetch → model pick (prefers nano/mini/8b/flash/lite)
  → add_model+deployment → alias+bind, with per-step progress and readable failures.
- **Real Settings screen** (#8): gateway port/body-limit/deployment-concurrency/
  timeout/header-timeout/upstream-idle + retention days, validated and saved through
  new allowlisted `get_public_settings`/`save_settings` IPC, with a restart-gateway
  button (network changes apply on restart).

## Honest residuals
- Capability auto-detect only has real signal from OpenRouter-style catalogues; NIM and
  OpenAI list bare ids (safe defaults used; editable later via update_model_capabilities).
- The wizard's "prefers small/fast" heuristic is a regex, not a benchmark.
- Cloud schedule editing stays on the Cloud card (no duplicate control in Settings).

## Gate
PASS — exact commands recorded above in this session's gate table (fmt/clippy/test/frontend all green before push).
