import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";

interface Alias {
  id: string;
  alias: string;
  model_ids: string[];
}

export function AliasesScreen() {
  const [aliases, setAliases] = useState<Alias[]>([]);
  const [aliasName, setAliasName] = useState("");
  const [bindTarget, setBindTarget] = useState<{ aliasId: string; deploymentId: string; priority: number }>({
    aliasId: "",
    deploymentId: "",
    priority: 10,
  });
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await invokeOr<Alias[]>("list_aliases", {});
      if (list) setAliases(list);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">Routing Policies &amp; Aliases</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Aliases are the stable model names local clients call. The default
        policy is priority order (strategy editable in a later phase).
      </p>
      {error && (
        <p role="alert" className="mt-2 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {error}
        </p>
      )}

      <ul className="mt-4 space-y-2">
        {aliases.map((a) => (
          <li key={a.id} className="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
            <div className="flex items-center justify-between">
              <span className="font-mono text-sm">{a.alias}</span>
              <button
                type="button"
                className="rounded border border-red-300 px-2 py-1 text-xs text-red-700 dark:border-red-800 dark:text-red-400"
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
                ? `→ ${a.model_ids.join(", ")}`
                : "no deployments bound"}
            </p>
            <form
              className="mt-2 flex flex-wrap gap-2 text-xs"
              onSubmit={(e) => {
                e.preventDefault();
                void run(async () => {
                  await invokeOr("bind_alias_deployment", {
                    // camelCase keys required by the Tauri bridge (audit P1 fix)
                    aliasId: a.id,
                    deploymentId: bindTarget.deploymentId,
                    priority: bindTarget.priority,
                  });
                  setBindTarget({ aliasId: "", deploymentId: "", priority: 10 });
                });
              }}
            >
              <input
                required
                placeholder="Deployment id"
                aria-label={`Deployment id for alias ${a.alias}`}
                value={bindTarget.aliasId === a.id ? bindTarget.deploymentId : ""}
                onChange={(e) => setBindTarget({ ...bindTarget, aliasId: a.id, deploymentId: e.target.value })}
                className="w-64 rounded border border-zinc-300 px-2 py-1 font-mono dark:border-zinc-700 dark:bg-zinc-900"
              />
              <input
                type="number"
                aria-label="Priority"
                value={bindTarget.aliasId === a.id ? bindTarget.priority : 10}
                onChange={(e) => setBindTarget({ ...bindTarget, aliasId: a.id, priority: Number(e.target.value) })}
                className="w-20 rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900"
              />
              <button type="submit" className="rounded bg-[#006eb9] px-2 py-1 font-medium text-white">
                Bind deployment
              </button>
            </form>
          </li>
        ))}
      </ul>

      <form
        className="mt-6 flex max-w-md gap-2 text-sm"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            await invokeOr("add_alias", { alias: aliasName, description: null });
            setAliasName("");
          });
        }}
      >
        <input
          required
          placeholder="New alias (e.g. fast-model)"
          value={aliasName}
          onChange={(e) => setAliasName(e.target.value)}
          className="flex-1 rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
        />
        <button type="submit" className="rounded-md bg-[#006eb9] px-3 py-1.5 font-medium text-white">
          Add alias
        </button>
      </form>
    </section>
  );
}
