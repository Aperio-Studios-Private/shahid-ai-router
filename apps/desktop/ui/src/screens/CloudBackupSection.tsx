import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";

interface CloudStatus {
  configured: boolean;
  provider: string | null;
  mode: string | null;
  schedule_enabled: boolean;
  device_id: string;
  has_connection: boolean;
  has_passphrase: boolean;
  last_upload_at: string | null;
  last_error: string | null;
  dirty: boolean;
}

interface Snapshot {
  id: string;
  device_id: string;
  app_version: string;
  mode: string;
  created_at: string;
  sha256: string;
  bytes: number;
}

interface CloudPreview {
  preview: {
    mode: string;
    created_at: string;
    app_version: string;
    counts: Record<string, unknown>;
    lost_passphrase_warning: boolean;
  };
  mode: string;
}

/**
 * Cloud auto-backup (Phase 12, ADR-019): opt-in sync of the `.sair` envelope
 * to a user-owned free-tier backend (Neon / Supabase). Secrets never leave
 * the device unencrypted; destination credentials live in the OS vault.
 * Configure is consent-gated (dialog before the command runs).
 */
export function CloudBackupSection() {
  const [status, setStatus] = useState<CloudStatus | null>(null);
  const [provider, setProvider] = useState<"neon" | "supabase" | "github-gist">("neon");
  const [mode, setMode] = useState<"encrypted" | "config">("encrypted");
  const [dsn, setDsn] = useState("");
  const [key, setKey] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [schedule, setSchedule] = useState(true);
  const [snapshots, setSnapshots] = useState<Snapshot[] | null>(null);
  const [staged, setStaged] = useState<CloudPreview | null>(null);
  const [restorePassphrase, setRestorePassphrase] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await invokeOr<CloudStatus>("cloud_status", {});
      if (s) setStatus(s);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const run = useCallback(
    async (fn: () => Promise<string>) => {
      setBusy(true);
      setError(null);
      setMessage(null);
      try {
        setMessage(await fn());
      } catch (err) {
        setError(String(err));
      } finally {
        setBusy(false);
        await refreshStatus();
      }
    },
    [refreshStatus],
  );

  const configure = () =>
    run(async () => {
      if (!dsn.trim()) throw "connection string / project URL is required";
      const ok = window.confirm(
        `Enable cloud backup to ${provider === "neon" ? "your Neon project" : "your Supabase project"}?\n\n` +
          `• Mode: ${mode === "encrypted" ? "ENCRYPTED (secrets included, AES-256-GCM with your passphrase)" : "CONFIG-ONLY (no secrets — credentials restore incomplete)"}\n` +
          `• Your connection string/key and the backup passphrase are stored ONLY in this machine's OS vault.\n` +
          `• Encrypted backups are recoverable only with the passphrase — keep it in your password manager; it is never uploaded.\n` +
          `• Uploads happen daily and after config changes (metadata only, never request logs).\n\nContinue?`,
      );
      if (!ok) return "Cloud setup cancelled.";
      const result = await invokeOr<{ configured: boolean }>("cloud_configure", {
        provider,
        mode,
        dsn: dsn.trim(),
        key: provider === "supabase" ? key.trim() : null,
        passphrase: mode === "encrypted" ? passphrase : null,
        scheduleEnabled: schedule,
      });
      if (!result) throw "cloud bridge unavailable (browser preview?)";
      setPassphrase("");
      setKey("");
      // First snapshot right away.
      await invokeOr("cloud_backup_now", {});
      return "Cloud backup configured — first snapshot uploaded.";
    });

  const testConnection = () =>
    run(async () => {
      await invokeOr("cloud_test", {});
      return "Cloud destination reachable.";
    });

  const backupNow = () =>
    run(async () => {
      const result = await invokeOr<{ bytes: number; mode: string }>(
        "cloud_backup_now",
        {},
      );
      if (!result) throw "cloud bridge unavailable (browser preview?)";
      return `Snapshot uploaded (${result.mode}, ${result.bytes} bytes).`;
    });

  const listSnapshots = () =>
    run(async () => {
      const list = await invokeOr<Snapshot[]>("cloud_list", {});
      setSnapshots(list ?? []);
      return `Found ${list?.length ?? 0} snapshot(s).`;
    });

  const restoreSnapshot = (id: string, snapshotMode: string) =>
    run(async () => {
      const needsPass = snapshotMode === "encrypted" && restorePassphrase.trim() === "";
      const result = await invokeOr<CloudPreview>("cloud_restore", {
        id,
        passphrase: needsPass ? null : restorePassphrase.trim() || null,
      });
      if (!result) throw "cloud bridge unavailable (browser preview?)";
      setStaged(result);
      return "Snapshot staged — review the preview, then commit.";
    });

  const commitRestore = () =>
    run(async () => {
      if (
        !window.confirm(
          "Commit this cloud restore? Rows are staged transactionally; secrets are re-stored to this machine's OS vault only in encrypted mode.",
        )
      ) {
        return "Restore cancelled.";
      }
      const result = await invokeOr<Record<string, unknown>>("cloud_commit", {});
      setStaged(null);
      setRestorePassphrase("");
      return `Restore committed: ${JSON.stringify(result)}`;
    });

  const disable = (purge: boolean) =>
    run(async () => {
      const ok = window.confirm(
        purge
          ? "Disable cloud backup AND delete this device's snapshots from the cloud?"
          : "Disable cloud backup? (Cloud snapshots are kept.)",
      );
      if (!ok) return "Cancelled.";
      const result = await invokeOr<{ purged_rows: number }>("cloud_disable", { purge });
      setSnapshots(null);
      return `Cloud backup disabled.${purge ? ` Deleted ${result?.purged_rows ?? 0} cloud row(s).` : ""}`;
    });

  const configured = status?.configured ?? false;

  return (
    <div className="mt-10 border-t border-zinc-200 pt-6 dark:border-zinc-800">
      <h2 className="text-sm font-semibold">Cloud backup (beta)</h2>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Automatic snapshots of your configuration to your own free-tier Neon or
        Supabase project. Only the backup envelope is uploaded — encrypted with
        your passphrase (keys included) or config-only (no secrets). Request
        logs never leave this machine. Destination credentials live in the OS
        vault.
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

      {configured && status && (
        <div className="mt-3 max-w-xl rounded-md border border-zinc-200 p-3 text-sm dark:border-zinc-800">
          <p className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
            Status
          </p>
          <ul className="mt-1 text-xs text-zinc-600 dark:text-zinc-300">
            <li>Provider: {status.provider}</li>
            <li>Mode: {status.mode}</li>
            <li>Schedule: {status.schedule_enabled ? "daily + on change" : "manual only"}</li>
            <li>Device: {status.device_id}</li>
            <li>Last upload: {status.last_upload_at ?? "never"}</li>
            {status.dirty && <li>Pending changes: yes</li>}
            {status.last_error && (
              <li className="text-red-700 dark:text-red-400">Last error: {status.last_error}</li>
            )}
          </ul>
          <div className="mt-2 flex flex-wrap gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={() => void backupNow()}
              className="rounded-md bg-[#006eb9] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
            >
              Back up now
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void listSnapshots()}
              className="rounded-md border border-zinc-300 px-3 py-1.5 text-xs hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
            >
              Refresh snapshots
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void disable(false)}
              className="rounded-md border border-zinc-300 px-3 py-1.5 text-xs hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
            >
              Disable
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void disable(true)}
              className="rounded-md border border-red-300 px-3 py-1.5 text-xs text-red-700 hover:bg-red-50 disabled:opacity-40 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
            >
              Disable &amp; delete cloud data…
            </button>
          </div>
        </div>
      )}

      {snapshots !== null && (
        <div className="mt-3 max-w-xl">
          <p className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
            Cloud snapshots ({snapshots.length})
          </p>
          {snapshots.length === 0 && (
            <p className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">
              No snapshots yet — press “Back up now”.
            </p>
          )}
          <ul className="mt-1 space-y-1">
            {snapshots.map((s) => (
              <li
                key={s.id}
                className="flex items-center justify-between rounded border border-zinc-200 px-2 py-1 text-xs dark:border-zinc-800"
              >
                <span className="font-mono">
                  {s.created_at} · {s.mode} · {s.device_id} · {s.app_version} ·{" "}
                  {s.bytes} B
                </span>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void restoreSnapshot(s.id, s.mode)}
                  className="rounded border border-zinc-300 px-2 py-0.5 hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
                >
                  Restore…
                </button>
              </li>
            ))}
          </ul>
          {snapshots.some((s) => s.mode === "encrypted") && (
            <input
              type="password"
              placeholder="Passphrase for encrypted snapshots"
              aria-label="Snapshot passphrase"
              value={restorePassphrase}
              onChange={(e) => setRestorePassphrase(e.target.value)}
              className="mt-2 w-full rounded-md border border-zinc-300 px-3 py-1.5 text-xs dark:border-zinc-700 dark:bg-zinc-900"
            />
          )}
        </div>
      )}

      {staged && (
        <div className="mt-3 max-w-xl rounded-md border border-zinc-200 p-3 text-sm dark:border-zinc-800">
          <p className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
            Cloud restore preview ({staged.preview.mode}, created{" "}
            {staged.preview.created_at})
          </p>
          <ul className="mt-1 text-xs text-zinc-600 dark:text-zinc-300">
            {Object.entries(staged.preview.counts).map(([k, v]) => (
              <li key={k}>
                {k}: {String(v)}
              </li>
            ))}
          </ul>
          {staged.preview.mode === "config" && (
            <p className="mt-1 text-xs text-amber-700 dark:text-amber-400">
              Config mode: credentials are imported as incomplete — re-enter
              their keys afterwards.
            </p>
          )}
          <button
            type="button"
            disabled={busy}
            onClick={() => void commitRestore()}
            className="mt-2 rounded-md bg-[#006eb9] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
          >
            Commit cloud restore…
          </button>
        </div>
      )}

      {!configured && (
        <form
          className="mt-3 grid max-w-xl grid-cols-1 gap-2 text-sm"
          onSubmit={(e) => {
            e.preventDefault();
            void configure();
          }}
        >
          <select
            aria-label="Cloud provider"
            value={provider}
            onChange={(e) => setProvider(e.target.value as "neon" | "supabase")}
            className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900"
          >
            <option value="neon">Neon (Postgres)</option>
            <option value="supabase">Supabase</option>
            <option value="github-gist">GitHub secret Gist (zero infra)</option>
          </select>
          <select
            aria-label="Backup mode"
            value={mode}
            onChange={(e) => setMode(e.target.value as "encrypted" | "config")}
            className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900"
          >
            <option value="encrypted">Encrypted (includes keys; passphrase-protected)</option>
            <option value="config">Config-only (no secrets)</option>
          </select>
          <input
            required
            placeholder={
              provider === "neon"
                ? "postgresql://…?sslmode=require (pooled host)"
                : provider === "supabase"
                  ? "https://<project>.supabase.co"
                  : "Gist id (from the gist URL)"
            }
            aria-label="Connection string or project URL"
            value={dsn}
            onChange={(e) => setDsn(e.target.value)}
            className="rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
          />
          {(provider === "supabase" || provider === "github-gist") && (
            <input
              required
              type="password"
              placeholder={provider === "supabase" ? "Supabase API key (service_role or anon with table access)" : "GitHub token with gist scope"}
              aria-label="Cloud API key"
              value={key}
              onChange={(e) => setKey(e.target.value)}
              className="rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
            />
          )}
          {mode === "encrypted" && (
            <input
              required
              type="password"
              placeholder="Backup passphrase (min 12 characters — kept only in the OS vault)"
              aria-label="Backup passphrase"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900"
            />
          )}
          <label className="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={schedule}
              onChange={(e) => setSchedule(e.target.checked)}
            />
            Auto-backup: daily + after config changes
          </label>
          <div className="flex gap-2">
            <button
              type="submit"
              disabled={busy}
              className="w-fit rounded-md bg-[#006eb9] px-3 py-1.5 font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
            >
              Configure &amp; test…
            </button>
            <button
              type="button"
              disabled={busy || !dsn.trim()}
              onClick={() => void testConnection()}
              className="w-fit rounded-md border border-zinc-300 px-3 py-1.5 font-medium hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
            >
              Test connection
            </button>
          </div>
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            Create the backup table once in your project&apos;s SQL editor (the
            app links the exact SQL in the docs). Your credentials never leave
            this machine unencrypted, and request logs are never uploaded.
          </p>
        </form>
      )}
    </div>
  );
}
