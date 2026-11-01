import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";
import { EmptyState, Hint, Tooltip } from "../components/ui";

interface Alias {
  id: string;
  alias: string;
  model_ids: string[];
}
interface ProviderLite {
  id: string;
  name: string;
}
interface DeploymentOption {
  id: string;
  label: string;
  enabled: boolean;
}

/**
 * Routing (Phase 13): create aliases and bind deployments via a DROPDOWN of
 * "Provider · model" options — previously this form required pasting a raw
 * deployment id, the worst flow in the app (phase-13 plan §Problems.3).
 */
export function AliasesScreen() {
  const [aliases, setAliases] = useState<Alias[] | null>(null);
  const [deployments, setDeployments] = useState<DeploymentOption[] | null>(null);
  const [aliasName, setAliasName] = useState("");
  const [bindTarget, setBindTarget] = useState<{ aliasId: string; deploymentId: string; priority: number }>({
    aliasId: "",
    deploymentId: "",
    priority: 10,
  });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const list = await invokeOr<Alias[]>("list_aliases", {});
      setAliases(list ?? []);
      const provs = (await invokeOr<ProviderLite[]>("list_providers", {})) ?? [];
      const options: DeploymentOption[] = [];
      for (const p of provs) {
        const deps =
          (await invokeOr<{ id: string; model_id: string; enabled: boolean }[]>(
            "list_deployments",
            { providerId: p.id },
          )) ?? [];
        for (const d of deps) {
          options.push({
            id: d.id,
            label: `${p.name} · ${d.model_id}`,
            enabled: d.enabled === true,
          });
        }
      }
      setDeployments(options);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">Routing</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Aliases are the stable model names your AI tools call
        (<code className="font-mono text-xs">"model": "fast-model"</code>). If a
        provider fails or runs out of quota, the router automatically falls back
        to the next deployment you bind here — in priority order.
      </p>
      {error && (
        <p
          role="alert"
          className="mt-2 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
        >
          {error}
        </p>
      )}

      <h2 className="mt-5 text-sm font-semibold">Your aliases</h2>
      {aliases === null ? (
        <p className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">Loading…</p>
      ) : aliases.length === 0 ? (
        <EmptyState
          title="No aliases yet"
          body={"An alias is what your AI tools type as the model name. Create one below (for example \"fast-model\"), then bind a deployment to it."}
        />
      ) : (
        <ul className="mt-2 space-y-2">
          {aliases.map((a) => (
            <li key={a.id} className="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
              <div className="flex items-center justify-between">
                <span className="font-mono text-sm">{a.alias}</span>
                <button
                  type="button"
                  disabled={busy}
                  className="rounded border border-red-300 px-2 py-1 text-xs text-red-700 dark:border-red-800 dark:text-red-400 disabled:opacity-40"
                  onClick={() => {
                    if (window.confirm(`Delete alias "${a.alias}"?`)) {
                      void run(() => invokeOr("delete_alias", { id: a.id }));
                    }
                  }}
                >
                  Delete
                </button>
              </div>
              <p className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">
                {a.model_ids.length > 0
                  ? `→ ${a.model_ids.length} deployment(s) bound (fallback order by priority)`
                  : "No deployments bound yet — this alias can't serve requests until you bind one below."}
              </p>

              <form
                className="mt-2 flex flex-wrap items-center gap-2 text-xs"
                onSubmit={(e) => {
                  e.preventDefault();
                  void run(async () => {
                    await invokeOr("bind_alias_deployment", {
                      aliasId: a.id,
                      deploymentId: bindTarget.deploymentId,
                      priority: bindTarget.priority,
                    });
                    setBindTarget({ aliasId: "", deploymentId: "", priority: 10 });
                  });
                }}
              >
                <label className="sr-only" htmlFor={`dep-${a.id}`}>
                  Deployment
                </label>
                <select
                  id={`dep-${a.id}`}
                  required
                  value={bindTarget.aliasId === a.id ? bindTarget.deploymentId : ""}
                  onChange={(e) =>
                    setBindTarget({ ...bindTarget, aliasId: a.id, deploymentId: e.target.value })
                  }
                  className="w-64 rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
                >
                  <option value="">
                    {deployments && deployments.length > 0
                      ? "Choose a deployment…"
                      : "No deployments available yet"}
                  </option>
                  {(deployments ?? []).map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.label}
                      {d.enabled ? "" : " (disabled)"}
                    </option>
                  ))}
                </select>
                <label className="flex items-center gap-1">
                  Priority
                  <Tooltip label="Lower number = tried first. The router works down the list when a deployment fails, is cooling down, or is busy." />
                  <input
                    type="number"
                    aria-label="Priority"
                    value={bindTarget.aliasId === a.id ? bindTarget.priority : 10}
                    onChange={(e) =>
                      setBindTarget({ ...bindTarget, aliasId: a.id, priority: Number(e.target.value) })
                    }
                    className="w-16 rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
                  />
                </label>
                <button
                  type="submit"
                  disabled={busy || (deployments?.length ?? 0) === 0}
                  className="rounded bg-[#006eb9] px-2 py-1 font-medium text-white disabled:opacity-40"
                >
                  Bind deployment
                </button>
              </form>
              {(deployments?.length ?? 0) === 0 && (
                <Hint>
                  Deployments are added on the Providers screen (expand a provider
                  → Deployments).
                </Hint>
              )}
            </li>
          ))}
        </ul>
      )}

      <h2 className="mt-6 text-sm font-semibold">Add an alias</h2>
      <form
        className="mt-2 flex max-w-md gap-2 text-sm"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            await invokeOr("add_alias", { alias: aliasName, description: null });
            setAliasName("");
          });
        }}
      >
        <label className="sr-only" htmlFor="new-alias">
          New alias name
        </label>
        <input
          id="new-alias"
          required
          placeholder="e.g. fast-model"
          value={aliasName}
          onChange={(e) => setAliasName(e.target.value)}
          className="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
        />
        <Tooltip label="Use a name that describes the job, not the provider — e.g. fast-model, coder, vision. Your tools only ever see this name, so you can swap providers later without touching them." />
        <button
          type="submit"
          disabled={busy}
          className="rounded-md bg-[#006eb9] px-3 py-1.5 font-medium text-white disabled:opacity-40"
        >
          Add alias
        </button>
      </form>
    </section>
  );
}
