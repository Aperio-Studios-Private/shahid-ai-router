#!/bin/sh
# One-command launcher (Phase 13): always builds the frontend first so the
# embedded UI can never go stale, then builds and runs the app.
# Usage: scripts/run-app.sh          (release, recommended)
#        DEV=1 scripts/run-app.sh    (debug build; slower, logs to stderr)
set -e
cd "$(dirname "$0")/.."
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
echo "==> Building frontend…"
(cd apps/desktop/ui && npm run build)
echo "==> Building app…"
# tauri-build does NOT re-embed changed frontendDist on incremental cargo
# builds (observed 2026-11-01: binary kept serving a week-old UI). Touch the
# build script to force the asset re-embed every time.
touch apps/desktop/src-tauri/build.rs
if [ "$DEV" = "1" ]; then
  cargo build -p shahid-ai-router
  BIN=target/debug/shahid-ai-router
else
  cargo build --release -p shahid-ai-router
  BIN=target/release/shahid-ai-router
fi
echo "==> Launching $BIN"
exec "$BIN"
