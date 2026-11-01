import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";
import { Hint, Spinner } from "../components/ui";
import { RotateCcw, Save } from "lucide-react";

interface GatewaySettings {
  port?: number;
  body_limit_bytes?: number;
  deployment_concurrency?: number;
  timeout_ms?: number;
  header_timeout_ms?: number;
  upstream_idle_read_ms?: number;
}

const FIELDS: { key: keyof GatewaySettings; label: string; help: string; step?: number }[] = [
  { key: "port", label: "Port", help: "Loopback port the gateway binds (127.0.0.1 only). Needs a gateway restart." },
  { key: "body_limit_bytes", label: "Body limit (bytes)", help: "413 over this. Minimum 1 KiB.", step: 1024 },
  { key: "deployment_concurrency", label: "Per-deployment concurrency", help: "Max simultaneous in-flight requests per deployment; excess gets 503." },
  { key: "timeout_ms", label: "Request timeout (ms)", help: "Whole-request budget; 408 when it elapses." },
  { key: "header_timeout_ms", label: "Header timeout (ms)", help: "Slowloris bound for reading request headers." },
  { key: "upstream_idle_read_ms", label: "Upstream idle read (ms)", help: "Stalled-stream cutoff during relay." },
];

/**
 * Settings (Phase 15): the real screen for the keys the backend already
 * consumes. Gateway changes apply on restart (button included).
 */
export function SettingsScreen() {
  const [gateway, setGateway] = useState<GatewaySettings | null>(null);
  const [retention, setRetention] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const s = await invokeOr<{ gateway: GatewaySettings; retention_days: number }>(
        "get_public_settings",
        {},
      );
      if (s) {
        setGateway(s.gateway ?? {});
        setRetention(s.retention_days ?? 30);
      }
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const save = async () => {
    if (gateway === null || retention === null) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await invokeOr("save_settings", { gateway, retentionDays: retention });
      setMessage("Settings saved. Restart the gateway to apply network changes.");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const restartGateway = async () => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await invokeOr("stop_gateway", {});
      await invokeOr("start_gateway", {});
      setMessage("Gateway restarted with the new settings.");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">Settings</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Tuning for the local gateway and data retention. Secrets and provider
        configuration live on their own screens.
      </p>

      {message && (
        <p
          role="status"
          className="mt-3 rounded-md border border-emerald-300 bg-emerald-50 px-3 py-2 text-sm text-emerald-800 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-200"
        >
          {message}
        </p>
      )}
      {error && (
        <p
          role="alert"
          className="mt-3 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
        >
          {error}
        </p>
      )}

      {gateway === null ? (
        <div className="mt-4 flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
          <Spinner /> Loading settings…
        </div>
      ) : (
        <>
          <h2 className="mt-5 text-sm font-semibold">Gateway</h2>
          <div className="mt-2 grid grid-cols-1 gap-3 sm:grid-cols-2">
            {FIELDS.map((f) => (
              <label key={f.key} className="text-xs font-medium text-zinc-500 dark:text-zinc-400">
                <span className="inline-flex">{f.label}</span>
                <input
                  type="number"
                  aria-label={f.label}
                  step={f.step}
                  value={gateway[f.key] ?? ""}
                  onChange={(e) =>
                    setGateway({ ...gateway, [f.key]: Number(e.target.value) })
                  }
                  className="mt-1 w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm shadow-sm dark:border-zinc-700 dark:bg-zinc-800"
                />
              </label>
            ))}
          </div>

          <h2 className="mt-5 text-sm font-semibold">Privacy</h2>
          <label className="mt-2 block text-xs font-medium text-zinc-500 dark:text-zinc-400">
            Request-record retention (days)
            <input
              type="number"
              aria-label="Retention days"
              min={1}
              value={retention ?? 30}
              onChange={(e) => setRetention(Number(e.target.value))}
              className="mt-1 w-32 rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm shadow-sm dark:border-zinc-700 dark:bg-zinc-800"
            />
          </label>
          <Hint>
            Metadata-only records older than this are purged daily (FR-020).
            Prompts/responses are never stored.
          </Hint>

          <div className="mt-5 flex gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={() => void save()}
              className="inline-flex items-center gap-1.5 rounded-lg bg-[#006eb9] px-4 py-2 text-sm font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
            >
              <Save size={14} aria-hidden="true" /> Save
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void restartGateway()}
              className="inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 bg-white px-4 py-2 text-sm font-medium shadow-sm hover:bg-zinc-50 disabled:opacity-40 dark:border-zinc-700 dark:bg-zinc-800 dark:hover:bg-zinc-700"
            >
              <RotateCcw size={14} aria-hidden="true" /> Restart gateway
            </button>
          </div>
        </>
      )}
    </section>
  );
}
