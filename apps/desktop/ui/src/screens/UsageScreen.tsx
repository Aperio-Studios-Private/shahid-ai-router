import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";

interface Usage {
  requests_24h: number;
  errors_24h: number;
  avg_latency_ms_24h: number | null;
  tokens_in_24h: number | null;
  tokens_out_24h: number | null;
  requests_7d: number;
  errors_7d: number;
}

function Card({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
      <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</dt>
      <dd className="mt-1 text-lg font-semibold">{value}</dd>
    </div>
  );
}

export function UsageScreen() {
  const [usage, setUsage] = useState<Usage | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const data = await invokeOr<Usage>("usage_summary", {});
      if (data) setUsage(data);
      else setError("Usage data unavailable.");
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const fmt = (n: number | null | undefined) => (n === null || n === undefined ? "—" : String(n));

  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">Usage</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Aggregated from metadata-only records. "Error" = requests classified as
        failures. Token counts appear when the upstream supplied them.
      </p>
      {error && (
        <p role="alert" className="mt-3 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {error}
        </p>
      )}
      <h2 className="mt-4 text-sm font-semibold">Last 24 hours</h2>
      <dl className="mt-2 grid grid-cols-2 gap-3 sm:grid-cols-3">
        <Card label="Requests" value={fmt(usage?.requests_24h)} />
        <Card label="Errors" value={fmt(usage?.errors_24h)} />
        <Card label="Avg latency" value={usage?.avg_latency_ms_24h != null ? `${Math.round(usage.avg_latency_ms_24h)} ms` : "—"} />
        <Card label="Tokens in" value={fmt(usage?.tokens_in_24h)} />
        <Card label="Tokens out" value={fmt(usage?.tokens_out_24h)} />
      </dl>
      <h2 className="mt-6 text-sm font-semibold">Last 7 days</h2>
      <dl className="mt-2 grid grid-cols-2 gap-3 sm:grid-cols-3">
        <Card label="Requests" value={fmt(usage?.requests_7d)} />
        <Card label="Errors" value={fmt(usage?.errors_7d)} />
      </dl>
      {usage === null && !error && (
        <p className="mt-4 text-sm text-zinc-500 dark:text-zinc-400">Loading…</p>
      )}
    </section>
  );
}
