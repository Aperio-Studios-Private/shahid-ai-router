import { invoke } from "@tauri-apps/api/core";

/**
 * True when running inside the Tauri webview (vs. a plain browser preview
 * during frontend development/tests).
 */
export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export interface AppInfo {
  name: string;
  version: string;
  gateway_running: boolean;
}

/**
 * Wraps Tauri IPC so screens degrade gracefully in browser preview mode.
 * Returns null when the bridge is unavailable.
 */
export async function invokeOr<T>(command: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isTauriRuntime()) {
    return null;
  }
  return invoke<T>(command, args);
}
