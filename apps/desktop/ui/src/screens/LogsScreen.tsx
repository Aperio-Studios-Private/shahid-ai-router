import { useCallback, useEffect, useRef, useState } from "react";
import { invokeOr } from "../lib/tauri";

interface RecordRow {
  ts: string;
  correlation_id: string;
  protocol_family: string | null;
  operation: string | null;
  alias: string | null;
  provider: string | null;
  model: string | null;
  deployment: string | null;
  credential: string | null;
  status: number | null;
  error_category: string | null;
  latency_ms: number | null;
  streamed: boolean;
  exclusions: { deployment: string; credential: string; reason: string }[];
}

const RETENTION_NOTE = "Records are metadata-only and retained for 30 days (FR-020).";

function StatusBadge({ row }: { row: RecordRow }) {
  const error = row.error_category !== null;
  return (
    <span className="rounded px-2 py-0.5 text-xs font-medium" aria-label={error ? `error: ${row.error_category}` : `status ${row.status ?? "?"}`}>
      {error ? `✗ ${row.error_category}` : `✓ ${row.status ?? "?"}`}
      {row.streamed && <span className="ml-1 text-zinc-500">streamed</span>}
    </span>
  );
}

export function LogsScreen() {
  const [records, setRecords] = useState<RecordRow[] | null>(null);
  const [aliasFilter, setAliasFilter] = useState("");
  const [errorsOnly, setErrorsOnly] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Debounced alias filter: typing fires one query 300ms after the last
  // keystroke instead of one per keystroke (audit P2 fix).
  const [deferredFilter, setDeferredFilter] = useState("");
  // Sequence guard: a slow response for an older filter must never overwrite
  // the result of a newer one.
  const seqRef = useRef(0);

  useEffect(() => {
    const t = window.setTimeout(() => setDeferredFilter(aliasFilter), 300);
    return () => window.clearTimeout(t);
  }, [aliasFilter]);

  const refresh = useCallback(async () => {
    setError(null);
    const seq = ++seqRef.current;
    try {
      const rows = await invokeOr<RecordRow[]>("list_request_records", {
        limit: 200,
        aliasFilter: deferredFilter || null,
        errorsOnly,
      });
      if (seq !== seqRef.current) return; // a newer query superseded this one
      if (rows) setRecords(rows);
      else setError("Request history unavailable.");
    } catch (err) {
      if (seq !== seqRef.current) return;
      setError(String(err));
    }
  }, [deferredFilter, errorsOnly]);

  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    if (!autoRefresh) return;
    const t = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(t);
  }, [autoRefresh, refresh]);

  const copyCorrelation = async (id: string) => {
    try {
      await navigator.clipboard.writeText(id);
      setCopied(id);
      window.setTimeout(() => setCopied(null), 1500);
    } catch (err) {
      setError(`clipboard unavailable: ${String(err)}`);
    }
  };

  return (
    <section className="max-w-4xl">
      <h1 className="text-lg font-semibold">Request Logs</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">{RETENTION_NOTE}</p>

      <div className="mt-3 flex flex-wrap items-center gap-3 text-sm">
        <input
          value={aliasFilter}
          onChange={(e) => setAliasFilter(e.target.value)}
          placeholder="Filter by alias"
          aria-label="Filter by alias"
          className="w-44 rounded-md border border-zinc-300 px-3 py-1.5 text-xs dark:border-zinc-700 dark:bg-zinc-900"
        />
        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={errorsOnly}
            onChange={(e) => setErrorsOnly(e.target.checked)}
          />
          Errors only
        </label>
        <button type="button" onClick={() => void refresh()} className="rounded-md border border-zinc-300 px-2 py-1 text-xs dark:border-zinc-700">
          Refresh
        </button>
        <label className="flex items-center gap-1.5 text-xs">
          <input
            type="checkbox"
            checked={autoRefresh}
            onChange={(e) => setAutoRefresh(e.target.checked)}
          />
          Auto (5s)
        </label>
      </div>

      {error && (
        <p role="alert" className="mt-3 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {error}
        </p>
      )}

      {records !== null && records.length === 0 && (
        <p className="mt-4 text-sm text-zinc-500 dark:text-zinc-400">
          No requests recorded yet.
        </p>
      )}

      <ul className="mt-4 space-y-1">
        {(records ?? []).map((r) => (
          <li key={r.correlation_id} className="rounded-lg border border-zinc-200 px-3 py-2 text-sm dark:border-zinc-800">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <button
                type="button"
                className="text-left"
                aria-expanded={expanded === r.correlation_id}
                onClick={() => setExpanded(expanded === r.correlation_id ? null : r.correlation_id)}
              >
                <StatusBadge row={r} />
                <span className="ml-2 font-mono text-xs">{r.alias ?? "?"}</span>
                <span className="ml-2 text-xs text-zinc-500 dark:text-zinc-400">
                  {r.model ?? ""} · {r.latency_ms ?? "?"} ms · {r.ts}
                </span>
              </button>
              <button
                type="button"
                className="rounded border border-zinc-300 px-2 py-0.5 text-xs dark:border-zinc-700"
                onClick={() => void copyCorrelation(r.correlation_id)}
              >
                {copied === r.correlation_id ? "Copied" : "Copy ID"}
              </button>
            </div>
            {expanded === r.correlation_id && (
              <dl className="mt-2 grid grid-cols-2 gap-1 text-xs text-zinc-600 dark:text-zinc-300">
                <dt>Correlation</dt>
                <dd className="font-mono">{r.correlation_id}</dd>
                <dt>Provider</dt>
                <dd>{r.provider ?? "—"}</dd>
                <dt>Deployment</dt>
                <dd>{r.deployment ?? "—"}</dd>
                <dt>Credential</dt>
                <dd>{r.credential ?? "—"}</dd>
                {r.exclusions.length > 0 && (
                  <>
                    <dt>Why unavailable</dt>
                    <dd>
                      <ul>
                        {r.exclusions.map((e, i) => (
                          <li key={i}>
                            {String(e.deployment)}: {String(e.reason)} ({String(e.credential)})
                          </li>
                        ))}
                      </ul>
                    </dd>
                  </>
                )}
              </dl>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
