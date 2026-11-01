import { useCallback, useState } from "react";
import { invokeOr } from "../lib/tauri";
import { Tooltip } from "../components/ui";
import { CloudBackupSection } from "./CloudBackupSection";

interface ExportResult {
  path: string;
  mode: string;
  warning: string;
}

interface PreviewResult {
  mode: string;
  created_at: string;
  app_version: string;
  included_sections: string[];
  counts: Record<string, unknown>;
  conflicts: Record<string, unknown>;
  lost_passphrase_warning: boolean;
}

type Report = Record<string, unknown>;

/**
 * Backup & Restore screen (docs/10). Export is one-step; import is the
 * consent-gated two-step preview → commit flow (10-minute preview TTL).
 * Secrets never leave the backend: the UI only sees counts and warnings.
 */
export function BackupScreen() {
  const [path, setPath] = useState("");
  const [mode, setMode] = useState<"config" | "encrypted">("config");
  const [passphrase, setPassphrase] = useState("");
  const [importPath, setImportPath] = useState("");
  const [importPassphrase, setImportPassphrase] = useState("");
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = useCallback(async (fn: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      setMessage(await fn());
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  const exportBackup = () =>
    run(async () => {
      if (!path.trim()) throw "export path is required";
      const result = await invokeOr<ExportResult>("export_backup", {
        path: path.trim(),
        mode,
        passphrase: mode === "encrypted" ? passphrase : null,
      });
      if (!result) throw "backup bridge unavailable (browser preview?)";
      return `Exported ${result.mode} backup to ${result.path}. ${result.warning}`;
    });

  const previewBackup = () =>
    run(async () => {
      if (!importPath.trim()) throw "import path is required";
      const result = await invokeOr<PreviewResult>("preview_backup", {
        path: importPath.trim(),
        passphrase:
          importPassphrase.trim() !== "" ? importPassphrase : null,
      });
      if (!result) throw "backup bridge unavailable (browser preview?)";
      setPreview(result);
      const counts = Object.entries(result.counts)
        .map(([k, v]) => `${k}: ${String(v)}`)
        .join(", ");
      return `Preview OK (${result.mode}, app ${result.app_version}). Contents — ${counts || "empty"}. Review, then commit.`;
    });

  const commitBackup = () =>
    run(async () => {
      if (!preview) throw "preview first";
      if (
        !window.confirm(
          "Commit this import? Rows are staged transactionally; credentials imported in config mode are marked incomplete until you re-enter their keys.",
        )
      ) {
        return "Import cancelled.";
      }
      const result = await invokeOr<Report>("commit_backup", {
        path: importPath.trim(),
        reStoreSecrets: preview.mode === "encrypted",
      });
      return `Import committed: ${JSON.stringify(result)}`;
    });

  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">Backup &amp; Restore</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        <code>.sair</code> containers are versioned data files, never
        executable. Encrypted backups use Argon2id + AES-256-GCM; losing the
        passphrase means the backup is unrecoverable (docs/10).
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

      <h2 className="mt-6 text-sm font-semibold">Export</h2>
      <form
        className="mt-2 grid max-w-xl grid-cols-1 gap-2 text-sm"
        onSubmit={(e) => {
          e.preventDefault();
          void exportBackup();
        }}
      >
        <input
          required
          placeholder="Destination path (…/backup.sair)"
          aria-label="Export destination path"
          value={path}
          onChange={(e) => setPath(e.target.value)}
          className="rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
        />
        <select
          aria-label="Backup mode"
          value={mode}
          onChange={(e) => setMode(e.target.value as "config" | "encrypted")}
          className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900"
        >
          <option value="config">Config-only (no API keys)</option>
          <option value="encrypted">Encrypted (includes keys; needs passphrase)</option>
        </select>
        <span className="inline-flex items-center text-xs text-zinc-500 dark:text-zinc-400">
          Mode
          <Tooltip label="Config-only restores your setup but marks credentials incomplete (re-enter keys). Encrypted includes your API keys as AES-256-GCM ciphertext — unusable without your passphrase. Either way nothing unencrypted ever leaves this Mac." />
        </span>
        {mode === "encrypted" && (
          <input
            required
            type="password"
            placeholder="Passphrase (min 12 characters)"
            aria-label="Backup passphrase"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
            className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900"
          />
        )}
        <button
          type="submit"
          disabled={busy}
          className="w-fit rounded-md bg-[#006eb9] px-3 py-1.5 font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
        >
          Export backup
        </button>
      </form>

      <h2 className="mt-8 text-sm font-semibold">Restore (preview → commit)</h2>
      <form
        className="mt-2 grid max-w-xl grid-cols-1 gap-2 text-sm"
        onSubmit={(e) => {
          e.preventDefault();
          void previewBackup();
        }}
      >
        <input
          required
          placeholder="Backup file path (…/backup.sair)"
          aria-label="Backup file path"
          value={importPath}
          onChange={(e) => {
            setImportPath(e.target.value);
            setPreview(null);
          }}
          className="rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
        />
        <input
          type="password"
          placeholder="Passphrase (encrypted backups only)"
          aria-label="Import passphrase"
          value={importPassphrase}
          onChange={(e) => setImportPassphrase(e.target.value)}
          className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900"
        />
        <button
          type="submit"
          disabled={busy}
          className="w-fit rounded-md border border-zinc-300 px-3 py-1.5 font-medium hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          Preview import
        </button>
      </form>

      {preview && (
        <div className="mt-3 max-w-xl rounded-md border border-zinc-200 p-3 text-sm dark:border-zinc-800">
          <p className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
            Preview ({preview.mode}, created {preview.created_at})
          </p>
          <ul className="mt-1 text-xs text-zinc-600 dark:text-zinc-300">
            {Object.entries(preview.counts).map(([k, v]) => (
              <li key={k}>
                {k}: {String(v)}
              </li>
            ))}
          </ul>
          {preview.mode === "config" && (
            <p className="mt-1 text-xs text-amber-700 dark:text-amber-400">
              Config mode: credentials are imported as incomplete — re-enter
              their keys afterwards.
            </p>
          )}
          {preview.lost_passphrase_warning && (
            <p className="mt-1 text-xs text-amber-700 dark:text-amber-400">
              Encrypted mode: secrets are re-stored into this machine&apos;s OS
              vault on commit.
            </p>
          )}
          <button
            type="button"
            disabled={busy}
            onClick={() => void commitBackup()}
            className="mt-2 rounded-md bg-[#006eb9] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
          >
            Commit import…
          </button>
        </div>
      )}

      <CloudBackupSection />
    </section>
  );
}
