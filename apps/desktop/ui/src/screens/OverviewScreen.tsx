import { useCallback, useEffect, useState } from "react";
import { invokeOr, type AppInfo } from "../lib/tauri";
import type { ScreenId } from "../lib/screens";
import { Spinner, Hint } from "../components/ui";

interface ProviderLite {
  id: string;
  name: string;
  enabled: boolean;
}
interface CredentialLite {
  id: string;
  label: string;
  status: string;
}
interface DeploymentLite {
  id: string;
  enabled: boolean;
}
interface AliasLite {
  id: string;
  alias: string;
  model_ids: string[];
}
interface CloudStatusLite {
  configured: boolean;
  provider: string | null;
}

interface Step {
  n: number;
  title: string;
  why: string;
  screen: ScreenId;
  done: boolean;
  doneLabel: string;
  optional?: boolean;
}

/**
 * Home (Phase 13): a live "Get started" checklist instead of static text.
 * Progress is derived from real app data, so the list updates as steps are
 * completed (research: setup checklists maintain momentum; empty states must
 * teach and offer one clear next action).
 */
export function OverviewScreen({
  appInfo,
  onNavigate,
}: {
  appInfo: AppInfo | null;
  onNavigate: (id: ScreenId) => void;
}) {
  const [providers, setProviders] = useState<ProviderLite[] | null>(null);
  const [credentialCount, setCredentialCount] = useState<number | null>(null);
  const [deployments, setDeployments] = useState<DeploymentLite[] | null>(null);
  const [aliases, setAliases] = useState<AliasLite[] | null>(null);
  const [hasRequests, setHasRequests] = useState<boolean | null>(null);
  const [cloud, setCloud] = useState<CloudStatusLite | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const provs = (await invokeOr<ProviderLite[]>("list_providers", {})) ?? [];
      setProviders(provs);
      let credCount = 0;
      const allDeployments: DeploymentLite[] = [];
      for (const p of provs) {
        const creds =
          (await invokeOr<CredentialLite[]>("list_credentials", {
            providerId: p.id,
          })) ?? [];
        credCount += creds.length;
        const deps =
          (await invokeOr<DeploymentLite[]>("list_deployments", {
            providerId: p.id,
          })) ?? [];
        allDeployments.push(...deps);
      }
      setCredentialCount(credCount);
      setDeployments(allDeployments);
      const als = (await invokeOr<AliasLite[]>("list_aliases", {})) ?? [];
      setAliases(als);
      const records = await invokeOr<unknown[]>("list_request_records", {
        limit: 1,
        aliasFilter: null,
        errorsOnly: false,
      });
      setHasRequests((records?.length ?? 0) > 0);
      const cs = await invokeOr<CloudStatusLite>("cloud_status", {});
      if (cs) setCloud(cs);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const providerDone = (providers?.length ?? 0) > 0;
  const keyDone = (credentialCount ?? 0) > 0;
  const deploymentDone = (deployments?.length ?? 0) > 0;
  const aliasDone = (aliases ?? []).some((a) => a.model_ids.length > 0);
  const requestDone = hasRequests === true;
  const cloudDone = cloud?.configured === true;

  const steps: Step[] = [
    {
      n: 1,
      title: "Add a provider",
      why: "Where should the router send AI requests? Add OpenAI, Anthropic, Gemini, NVIDIA NIM, OpenRouter, or any OpenAI-compatible server (Ollama, LM Studio, vLLM…).",
      screen: "providers",
      done: providerDone,
      doneLabel:
        providers && providers.length > 0
          ? `${providers.length} provider${providers.length > 1 ? "s" : ""} added`
          : "",
    },
    {
      n: 2,
      title: "Add an API key",
      why: "Keys are stored in your Mac's Keychain — never in the app database, never logged, never sent anywhere except the provider you chose.",
      screen: "providers",
      done: keyDone,
      doneLabel: keyDone ? `${credentialCount} key(s) in the vault` : "",
    },
    {
      n: 3,
      title: "Deploy a model",
      why: "A deployment connects one upstream model (e.g. gpt-4o, meta/llama-3.1-8b-instruct) to its provider, so the router knows what it can call.",
      screen: "providers",
      done: deploymentDone,
      doneLabel: deploymentDone ? `${deployments?.length} model(s) deployed` : "",
    },
    {
      n: 4,
      title: "Create an alias and bind the deployment",
      why: "The alias is the stable model name your AI tools use (e.g. \"fast-model\"). Bindings decide the fallback order when a provider fails.",
      screen: "routing",
      done: aliasDone,
      doneLabel: aliasDone ? `${aliases?.length} alias(es) routed` : "",
    },
    {
      n: 5,
      title: "Send a test request",
      why: "AI coding agents (Claude Code, Gemini CLI, Codex…) point at this machine's gateway: http://127.0.0.1:8787 — get the token on the Local Gateway screen.",
      screen: "gateway",
      done: requestDone,
      doneLabel: requestDone ? "Requests are flowing — see Activity" : "",
    },
    {
      n: 6,
      title: "Generate an agent profile",
      why: "One click writes the correct environment/config for Claude Code, Gemini CLI or any OpenAI-compatible client — consent-gated, with an automatic backup of the file.",
      screen: "agent-integrations",
      done: false,
      doneLabel: "",
      optional: true,
    },
    {
      n: 7,
      title: "Turn on cloud backup",
      why: "Snapshots your whole setup to your own free Neon/Supabase project daily (and after changes) — restore on a new Mac in two clicks.",
      screen: "backup",
      done: cloudDone,
      doneLabel: cloudDone ? `Synced to ${cloud?.provider}` : "",
      optional: true,
    },
  ];

  const required = steps.filter((s) => !s.optional);
  const requiredDone = required.filter((s) => s.done).length;
  const allDone = requiredDone === required.length;

  return (
    <section className="max-w-3xl">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h1 className="text-lg font-semibold">Welcome to Shahid AI Router</h1>
          <p className="mt-1 max-w-2xl text-sm text-zinc-500 dark:text-zinc-400">
            One local, authenticated endpoint for all your AI providers. Your AI
            coding agents talk to this app; the app picks a working provider,
            rotates keys on failures, and keeps your keys in the macOS Keychain.
            {appInfo ? ` (v${appInfo.version})` : ""}
          </p>
        </div>
        {allDone && (
          <span className="rounded-full border border-emerald-300 bg-emerald-50 px-3 py-1 text-xs font-medium text-emerald-800 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-200">
            ✓ Setup complete
          </span>
        )}
      </div>

      {error && (
        <p
          role="alert"
          className="mt-3 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
        >
          {error}
        </p>
      )}

      <div className="mt-4 flex items-center justify-between">
        <h2 className="text-sm font-semibold">Get started</h2>
        <span className="text-xs text-zinc-500 dark:text-zinc-400">
          {requiredDone}/{required.length} required steps
        </span>
      </div>

      {providers === null ? (
        <div className="mt-3 flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
          <Spinner /> Checking your setup…
        </div>
      ) : (
        <ol className="mt-2 space-y-2">
          {steps.map((s) => (
            <li
              key={s.n}
              className={
                "rounded-lg border p-3 " +
                (s.done
                  ? "border-emerald-200 bg-emerald-50/50 dark:border-emerald-800 dark:bg-emerald-950/30"
                  : "border-zinc-200 dark:border-zinc-800")
              }
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="text-sm font-medium">
                    <span
                      aria-hidden="true"
                      className={
                        "mr-2 inline-flex h-5 w-5 items-center justify-center rounded-full text-xs font-semibold " +
                        (s.done
                          ? "bg-emerald-600 text-white"
                          : "bg-zinc-200 text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300")
                      }
                    >
                      {s.done ? "✓" : s.n}
                    </span>
                    {s.title}
                    {s.optional && (
                      <span className="ml-2 rounded border border-zinc-300 px-1.5 py-0.5 text-[10px] font-normal uppercase tracking-wide text-zinc-500 dark:border-zinc-700">
                        optional
                      </span>
                    )}
                  </p>
                  <Hint>{s.why}</Hint>
                  {s.done && s.doneLabel && (
                    <p className="mt-1 text-xs font-medium text-emerald-700 dark:text-emerald-400">
                      ✓ {s.doneLabel}
                    </p>
                  )}
                </div>
                {!s.done && (
                  <button
                    type="button"
                    onClick={() => onNavigate(s.screen)}
                    className="shrink-0 rounded-md bg-[#006eb9] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#005c9b]"
                  >
                    {s.n === 5 ? "Get the token" : "Do it"}
                  </button>
                )}
              </div>
            </li>
          ))}
        </ol>
      )}

      <div className="mt-6 rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
        <h2 className="text-sm font-semibold">How it works, in one picture</h2>
        <pre className="mt-2 overflow-x-auto rounded-md bg-zinc-100 p-3 text-xs leading-relaxed dark:bg-zinc-900">
{`Claude Code / Gemini CLI / curl
        │  http://127.0.0.1:8787  (token-authenticated, Mac-local only)
        ▼
┌─────────────────────────────┐
│ Shahid AI Router gateway    │  picks an eligible deployment
│  • alias → fallback order   │  rotates keys on failures
│  • capability-aware routing │  cools the whole account on quota
└─────────────────────────────┘
   │            │            │
   ▼            ▼            ▼
 Provider A   Provider B   Provider C
 (keys stay in the macOS Keychain — never in the app database)`}
        </pre>
      </div>
    </section>
  );
}
