import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";

interface CredentialHealth {
  id: string;
  provider: string;
  label: string;
  status: string;
  cooldown_until: string | null;
  last_used_at: string | null;
  last_error_category: string | null;
  last_error_code: string | null;
}

interface HealthEvent {
  ts: string;
  subject_type: string;
  subject_id: string;
  event: string;
  category: string | null;
  detail: string | null;
  correlation_id: string | null;
}

/// docs/08: explain WHY a credential is unavailable — human-readable reasons,
/// never color-only (every badge carries text).
function statusExplanation(c: CredentialHealth): string {
  if (c.status === "quarantined") {
    return "Quarantined after an authentication/permission failure — reactivate in API Keys after fixing the key.";
  }
  if (c.status === "invalid") {
    return "Marked invalid (rejected upstream or missing secret) — replace the key.";
  }
  if (c.status === "incomplete") {
    return "Imported without its secret — re-enter the key in API Keys.";
  }
  if (c.cooldown_until && new Date(c.cooldown_until).getTime() > Date.now()) {
    return `Cooling down until ${c.cooldown_until} (rate limit / quota) — excluded from routing until then.`;
  }
  if (c.status === "active") {
    return "Eligible for routing.";
  }
  return "Not eligible for routing.";
}

export function HealthScreen() {
  const [credentials, setCredentials] = useState<CredentialHealth[] | null>(null);
  const [events, setEvents] = useState<HealthEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [creds, evts] = await Promise.all([
        invokeOr<CredentialHealth[]>("credential_health_summary", {}),
        invokeOr<HealthEvent[]>("list_health_events", { limit: 50 }),
      ]);
      if (creds) setCredentials(creds);
      if (evts) setEvents(evts);
      if (!creds && !evts) setError("Health data unavailable.");
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  return (
    <section className="max-w-3xl">
      <h1 className="text-lg font-semibold">Health Monitor</h1>

      {error && (
        <p role="alert" className="mt-3 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {error}
        </p>
      )}

      <h2 className="mt-4 text-sm font-semibold">Credentials</h2>
      <ul className="mt-2 space-y-2">
        {(credentials ?? []).map((c) => (
          <li key={c.id} className="rounded-lg border border-zinc-200 p-3 text-sm dark:border-zinc-800">
            <div className="flex items-center justify-between">
              <span className="font-medium">
                {c.provider} · {c.label}
              </span>
              <span className="rounded px-2 py-0.5 text-xs font-medium" aria-label={`status ${c.status}`}>
                {c.status}
              </span>
            </div>
            <p className="mt-1 text-xs text-zinc-600 dark:text-zinc-300">{statusExplanation(c)}</p>
            {c.last_error_category && (
              <p className="mt-1 text-xs text-red-700 dark:text-red-400">
                Last error: {c.last_error_category}
                {c.last_error_code ? ` (${c.last_error_code})` : ""}
              </p>
            )}
          </li>
        ))}
        {credentials !== null && credentials.length === 0 && (
          <li className="text-sm text-zinc-500 dark:text-zinc-400">No credentials configured.</li>
        )}
      </ul>

      <h2 className="mt-6 text-sm font-semibold">Recent events</h2>
      <ul className="mt-2 space-y-1 text-xs">
        {(events ?? []).map((e, i) => (
          <li key={`${e.ts}-${i}`} className="rounded border border-zinc-200 px-2 py-1 dark:border-zinc-800">
            <span className="font-mono">{e.ts}</span> · {e.event} · {e.subject_type}{" "}
            {e.subject_id}
            {e.category ? ` (${e.category})` : ""}
            {e.detail ? ` — ${e.detail}` : ""}
          </li>
        ))}
        {events !== null && events.length === 0 && (
          <li className="text-zinc-500 dark:text-zinc-400">No events recorded.</li>
        )}
      </ul>
    </section>
  );
}
