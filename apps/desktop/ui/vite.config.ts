/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The UI is served by Vite in dev and built to dist/ for the Tauri shell
// (see apps/desktop/src-tauri/tauri.conf.json build section).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  // WKWebView on older macOS (Monterey = Safari 15/16) cannot parse Vite 8's
  // default 'baseline-widely-available' output — the bundle fails to parse and
  // the window renders white (observed 2026-11-01). es2020 is safely within
  // every supported macOS (12+).
  build: {
    target: "es2020",
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: "./src/test/setup.ts",
    css: false,
  },
});
