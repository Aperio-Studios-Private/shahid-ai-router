import { useCallback, useEffect, useState } from "react";
import {
  Bot,
  Check,
  CloudUpload,
  Copy,
  Globe,
  KeyRound,
  MessageSquare,
  Plus,
  Rocket,
  Route,
  Server,
  X,
} from "lucide-react";
import { invokeOr, type AppInfo } from "../lib/tauri";
import type { ScreenId } from "../lib/screens";
import { Spinner, Hint } from "../components/ui";
import { PRESETS, familyFor } from "./ProvidersScreen";
import type { LucideIcon } from "lucide-react";

interface ModelInfoLite {
  id: string;
  streaming: boolean;
  tool_calling: boolean;
  vision: boolean;
  structured_output: boolean;
  context_window: number | null;
}

interface ProviderLite {
  id: string;
  name: string;
  enabled: boolean;
}
interface CredentialLite {
  id: string;
}
interface DeploymentLite {
  id: string;
}
interface AliasLite {
  id: string;
  model_ids: string[];
}
interface CloudStatusLite {
  configured: boolean;
  provider: string | null;
}
interface GatewayLite {
  running: boolean;
  addr: string | null;
}

/** Visual icon chip: tinted rounded square + Lucide icon. */
function IconChip({ icon: Icon, tone }: { icon: LucideIcon; tone: string }) {
  return (
    <span
      aria-hidden="true"
      className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-xl shadow-sm ${tone}`}
    >
      <Icon size={18} strokeWidth={1.8} />
    </span>
  );
}

const TONES = {
  blue: "bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300",
  green: "bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300",
  violet: "bg-violet-100 text-violet-700 dark:bg-violet-500/15 dark:text-violet-300",
  amber: "bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300",
  rose: "bg-rose-100 text-rose-700 dark:bg-rose-500/15 dark:text-rose-300",
  zinc: "bg-zinc-100 text-zinc-600 dark:bg-zinc-500/15 dark:text-zinc-300",
};

const CARD =
  "rounded-2xl border border-zinc-200 bg-white p-4 shadow-sm transition hover:shadow-md dark:border-zinc-800 dark:bg-zinc-900";

/**
 * Home (Phase 14): card-based redesign — a status card row, the guided setup
 * as visual step cards, and quick-action cards (research: setup checklists +
 * teaching empty states; cards give each item one clear job and CTA).
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
  const [requestCount, setRequestCount] = useState<number | null>(null);
  const [cloud, setCloud] = useState<CloudStatusLite | null>(null);
  const [gateway, setGateway] = useState<GatewayLite | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [wizardPreset, setWizardPreset] = useState(0);
  const [wizardKey, setWizardKey] = useState("");
  const [wizardAlias, setWizardAlias] = useState("fast-model");
  const [wizardBusy, setWizardBusy] = useState(false);
  const [wizardLog, setWizardLog] = useState<string[]>([]);
  const [wizardError, setWizardError] = useState<string | null>(null);

  const runWizard = async () => {
    const preset = PRESETS[wizardPreset];
    setWizardBusy(true);
    setWizardError(null);
    setWizardLog([]);
    const step = (line: string) => setWizardLog((l) => [...l, line]);
    try {
      if (!preset.allow_loopback && !wizardKey.trim()) {
        throw "this provider needs an API key";
      }
      step(`Adding provider: ${preset.name}…`);
      const providerId = await invokeOr<string>("add_provider", {
        name: preset.name,
        providerType: preset.provider_type,
        baseUrl: preset.base_url,
        allowLoopback: preset.allow_loopback,
        notes: "Created by automatic setup",
      });
      if (!providerId) throw "bridge unavailable";
      step("Storing API key in the OS vault…");
      await invokeOr("add_credential", {
        providerId,
        label: "default",
        secret: wizardKey.trim() || "local-no-key",
        priority: 0,
        weight: 1,
      });
      step("Fetching the live model catalogue…");
      const models =
        (await invokeOr<ModelInfoLite[]>("fetch_provider_models", { id: providerId })) ?? [];
      if (models.length === 0) throw "the provider returned no models";
      const pick =
        models.find((m) =>
          /nano|mini|small|8b|flash|instant|turbo|lite/i.test(m.id),
        ) ?? models[0];
      step(`Deploying ${pick.id}…`);
      const modelPk = await invokeOr<string>("add_model", {
        providerId,
        modelId: pick.id,
        protocolFamily: familyFor(preset.provider_type),
        streaming: pick.streaming,
        toolCalling: pick.tool_calling,
        vision: pick.vision,
        structuredOutput: pick.structured_output,
        contextWindow: pick.context_window,
      });
      const depId = await invokeOr<string>("add_deployment", {
        providerId,
        modelPk,
        endpointOverride: null,
      });
      step(`Creating alias "${wizardAlias}"…`);
      const existing = (await invokeOr<{ id: string; alias: string }[]>("list_aliases", {})) ?? [];
      const existingAlias = existing.find((a) => a.alias === wizardAlias);
      const aliasId =
        existingAlias ??
        (await invokeOr<string>("add_alias", {
          alias: wizardAlias,
          description: "Created by automatic setup",
        }));
      if (!aliasId) throw "alias creation failed";
      await invokeOr("bind_alias_deployment", {
        aliasId,
        deploymentId: depId,
        priority: 10,
      });
      step("Done — the alias is live on the gateway ✓");
      setWizardKey("");
      await refresh();
    } catch (err) {
      setWizardError(String(err));
    } finally {
      setWizardBusy(false);
    }
  };

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const provs = (await invokeOr<ProviderLite[]>("list_providers", {})) ?? [];
      setProviders(provs);
      let credCount = 0;
      const allDeployments: DeploymentLite[] = [];
      for (const p of provs) {
        const creds =
          (await invokeOr<CredentialLite[]>("list_credentials", { providerId: p.id })) ?? [];
        credCount += creds.length;
        const deps =
          (await invokeOr<DeploymentLite[]>("list_deployments", { providerId: p.id })) ?? [];
        allDeployments.push(...deps);
      }
      setCredentialCount(credCount);
      setDeployments(allDeployments);
      const als = (await invokeOr<AliasLite[]>("list_aliases", {})) ?? [];
      setAliases(als);
      const records = (await invokeOr<unknown[]>("list_request_records", {
        limit: 100,
        aliasFilter: null,
        errorsOnly: false,
      })) ?? [];
      setRequestCount(records.length);
      const cs = await invokeOr<CloudStatusLite>("cloud_status", {});
      if (cs) setCloud(cs);
      const gw = await invokeOr<GatewayLite>("get_gateway_state", {});
      if (gw) setGateway(gw);
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
  const requestDone = (requestCount ?? 0) > 0;
  const cloudDone = cloud?.configured === true;

  const steps = [
    {
      icon: Server,
      tone: TONES.blue,
      title: "Add a provider",
      why: "Connect OpenAI, Anthropic, Gemini, NVIDIA NIM, OpenRouter or a local server (Ollama, LM Studio…).",
      screen: "providers" as ScreenId,
      done: providerDone,
      doneLabel: `${providers?.length ?? 0} added`,
    },
    {
      icon: KeyRound,
      tone: TONES.green,
      title: "Add an API key",
      why: "Keys go straight into your Mac's Keychain — never in the database, never logged.",
      screen: "providers" as ScreenId,
      done: keyDone,
      doneLabel: `${credentialCount ?? 0} in the vault`,
    },
    {
      icon: Rocket,
      tone: TONES.violet,
      title: "Deploy a model",
      why: "Point the router at an upstream model (gpt-4o, meta/llama-3.1-8b-instruct…) so it knows what to call.",
      screen: "providers" as ScreenId,
      done: deploymentDone,
      doneLabel: `${deployments?.length ?? 0} deployed`,
    },
    {
      icon: Route,
      tone: TONES.amber,
      title: "Route an alias",
      why: "Give your tools one stable name (\"fast-model\") and a fallback order for when a provider fails.",
      screen: "routing" as ScreenId,
      done: aliasDone,
      doneLabel: `${aliases?.length ?? 0} routed`,
    },
    {
      icon: MessageSquare,
      tone: TONES.rose,
      title: "Send a test request",
      why: "Point Claude Code / Gemini CLI / curl at http://127.0.0.1:8787 with the token from the Gateway screen.",
      screen: "gateway" as ScreenId,
      done: requestDone,
      doneLabel: `${requestCount ?? 0} request(s) so far`,
    },
    {
      icon: CloudUpload,
      tone: TONES.zinc,
      title: "Cloud backup",
      why: "Snapshots your setup to your own free Neon/Supabase project — restore on any Mac in two clicks.",
      screen: "backup" as ScreenId,
      done: cloudDone,
      doneLabel: cloudDone ? `Synced to ${cloud?.provider}` : "Optional",
      optional: true,
    },
    {
      icon: Bot,
      tone: TONES.blue,
      title: "Connect an AI agent",
      why: "Generate a ready-made config for Claude Code, Gemini CLI or any OpenAI-compatible client.",
      screen: "agent-integrations" as ScreenId,
      done: false,
      doneLabel: "Optional",
      optional: true,
    },
  ];

  const required = steps.filter((s) => !s.optional);
  const requiredDone = required.filter((s) => s.done).length;
  const allDone = requiredDone === required.length;

  const stats = [
    {
      icon: Globe,
      tone: gateway?.running ? TONES.green : TONES.zinc,
      label: "Gateway",
      value: gateway === null ? "…" : gateway.running ? "Running" : "Stopped",
      sub: gateway?.addr ?? "127.0.0.1:8787",
      screen: "gateway" as ScreenId,
    },
    { icon: Server, tone: TONES.blue, label: "Providers", value: String(providers?.length ?? 0), sub: `${providers?.filter((p) => p.enabled).length ?? 0} enabled`, screen: "providers" as ScreenId },
    { icon: KeyRound, tone: TONES.green, label: "Keys", value: String(credentialCount ?? 0), sub: "in OS vault", screen: "providers" as ScreenId },
    { icon: MessageSquare, tone: TONES.violet, label: "Requests", value: requestCount === null ? "…" : String(requestCount), sub: "recent (of all history)", screen: "activity" as ScreenId },
  ];

  return (
    <section className="max-w-4xl">
      {/* Header card */}
      <div className="rounded-2xl border border-zinc-200 bg-gradient-to-br from-[#006eb9] to-sky-500 p-5 text-white shadow-md dark:border-zinc-800">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 className="text-lg font-semibold">
              Shahid AI Router {appInfo ? <span className="text-sm font-normal opacity-80">v{appInfo.version}</span> : null}
            </h1>
            <p className="mt-1 max-w-2xl text-sm text-white/85">
              One local, authenticated endpoint for all your AI providers. Your
              AI coding agents talk to this app — it picks a working provider,
              rotates keys on failures, and keeps secrets in the macOS Keychain.
            </p>
          </div>
          <span
            className={
              "flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium shadow-sm " +
              (gateway?.running ? "bg-emerald-400/90 text-emerald-950" : "bg-white/25 text-white")
            }
          >
            <span
              aria-hidden="true"
              className={
                "h-1.5 w-1.5 rounded-full " +
                (gateway === null ? "bg-white/50" : gateway.running ? "bg-emerald-700" : "bg-white/60")
              }
            />
            {gateway === null
              ? "…"
              : gateway.running
                ? "Gateway running"
                : "Gateway stopped"}
          </span>
        </div>
      </div>

      {error && (
        <p
          role="alert"
          className="mt-3 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200"
        >
          {error}
        </p>
      )}

      {/* Stat cards */}
      <div className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
        {stats.map((s) => (
          <button
            key={s.label}
            type="button"
            onClick={() => onNavigate(s.screen)}
            className={CARD + " text-left hover:border-[#006eb9]/40"}
          >
            <div className="flex items-center gap-2">
              <IconChip icon={s.icon} tone={s.tone} />
              <div className="min-w-0">
                <p className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{s.label}</p>
                <p className="text-lg font-semibold leading-tight">{s.value}</p>
              </div>
            </div>
            <p className="mt-2 truncate text-xs text-zinc-500 dark:text-zinc-400">{s.sub}</p>
          </button>
        ))}
      </div>

      {/* Setup checklist */}
      <div className="mt-6 flex items-center justify-between">
        <h2 className="text-sm font-semibold">Get started</h2>
        <span className="text-xs text-zinc-500 dark:text-zinc-400">
          {requiredDone}/{required.length} required ·{" "}
          {allDone ? (
            <span className="font-medium text-emerald-600 dark:text-emerald-400">complete ✓</span>
          ) : (
            "finish the required cards"
          )}
        </span>
      </div>
      {providers === null ? (
        <div className="mt-3 flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
          <Spinner /> Checking your setup…
        </div>
      ) : (
        <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
          {steps.map((s) => (
            <div
              key={s.title}
              className={
                CARD +
                (s.done
                  ? " border-emerald-300/70 bg-emerald-50/40 dark:border-emerald-800/60 dark:bg-emerald-950/20"
                  : "")
              }
            >
              <div className="flex items-start gap-3">
                <IconChip icon={s.icon} tone={s.tone} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center justify-between gap-2">
                    <p className="text-sm font-semibold">
                      {s.title}
                      {s.optional && (
                        <span className="ml-2 rounded border border-zinc-300 px-1.5 py-0.5 text-[10px] font-normal uppercase tracking-wide text-zinc-500 dark:border-zinc-700">
                          optional
                        </span>
                      )}
                    </p>
                    {s.done && (
                      <span className="flex shrink-0 items-center gap-1 rounded-full bg-emerald-100 px-2 py-0.5 text-[10px] font-medium text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300">
                        <Check size={10} strokeWidth={3} /> {s.doneLabel}
                      </span>
                    )}
                  </div>
                  <Hint>{s.why}</Hint>
                  {!s.done && (
                    <button
                      type="button"
                      onClick={() => onNavigate(s.screen)}
                      className="mt-2 rounded-md bg-[#006eb9] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#005c9b]"
                    >
                      {s.title.startsWith("Send") ? "Get the token" : "Do it"}
                    </button>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Connection block (#5): one-click copy of everything a tool needs */}
      <div className={CARD + " mt-6"}>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold">Connect any AI tool</h2>
            <p className="mt-0.5 truncate font-mono text-xs text-zinc-500 dark:text-zinc-400">
              {gateway?.addr ? `http://${gateway.addr}/v1` : "gateway not running"}
            </p>
          </div>
          <div className="flex gap-1.5">
            <button
              type="button"
              disabled={!gateway?.running}
              className="rounded-lg border border-zinc-200 bg-white px-2.5 py-1.5 text-xs font-medium shadow-sm hover:bg-zinc-50 disabled:opacity-40 dark:border-zinc-700 dark:bg-zinc-800 dark:hover:bg-zinc-700"
              onClick={() => {
                void invokeOr<string>("reveal_gateway_token", {}).then((token) => {
                  const url = `http://${gateway?.addr ?? "127.0.0.1:8787"}/v1`;
                  void navigator.clipboard
                    .writeText(`Base URL: ${url}\nAPI key: ${token ?? "<reveal failed>"}`)
                    .then(() => setCopied(true))
                    .catch(() => setCopied(false));
                  window.setTimeout(() => setCopied(false), 2000);
                });
              }}
            >
              <Copy size={12} aria-hidden="true" className="inline" />{" "}
              {copied ? "Copied ✓" : "Copy URL + key"}
            </button>
            <button
              type="button"
              onClick={() => onNavigate("gateway")}
              className="rounded-lg border border-zinc-200 bg-white px-2.5 py-1.5 text-xs font-medium shadow-sm hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-800 dark:hover:bg-zinc-700"
            >
              Gateway screen
            </button>
          </div>
        </div>
        <Hint>
          “Copy URL + key” puts the base URL and gateway token on your clipboard —
          paste into any AI tool's provider settings. The token never leaves this
          Mac.
        </Hint>
      </div>

      {/* Quick setup wizard (#1) */}
      {wizardOpen && (
        <div
          className="palette-backdrop fixed inset-0 z-50 flex items-start justify-center pt-24"
          onClick={() => setWizardOpen(false)}
          role="presentation"
        >
          <div
            role="dialog"
            aria-label="Quick setup"
            className="rise w-[520px] max-w-[92vw] rounded-xl border border-zinc-200 bg-white p-5 shadow-2xl dark:border-zinc-800 dark:bg-zinc-900"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between">
              <h2 className="text-sm font-semibold">Automatic setup</h2>
              <button type="button" aria-label="Close" onClick={() => setWizardOpen(false)}>
                <X size={15} className="text-zinc-400" />
              </button>
            </div>
            <p className="mt-1 text-xs text-zinc-500 dark:text-zinc-400">
              One flow: provider + key + model + alias, fully wired. Model chosen
              automatically (prefers small/fast variants).
            </p>
            <label className="mt-3 block text-xs font-medium text-zinc-500 dark:text-zinc-400">
              Provider preset
              <select
                aria-label="Wizard provider preset"
                value={wizardPreset}
                onChange={(e) => setWizardPreset(Number(e.target.value))}
                className="mt-1 w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm shadow-sm dark:border-zinc-700 dark:bg-zinc-800"
              >
                {PRESETS.map((preset, i) => (
                  <option key={preset.name} value={i}>
                    {preset.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="mt-2 block text-xs font-medium text-zinc-500 dark:text-zinc-400">
              API key
              <input
                type="password"
                aria-label="Wizard API key"
                placeholder={PRESETS[wizardPreset]?.allow_loopback ? "optional for local servers" : "required"}
                value={wizardKey}
                onChange={(e) => setWizardKey(e.target.value)}
                className="mt-1 w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 font-mono text-xs shadow-sm dark:border-zinc-700 dark:bg-zinc-800"
              />
            </label>
            <label className="mt-2 block text-xs font-medium text-zinc-500 dark:text-zinc-400">
              Alias name
              <input
                aria-label="Wizard alias name"
                value={wizardAlias}
                onChange={(e) => setWizardAlias(e.target.value)}
                className="mt-1 w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 font-mono text-xs shadow-sm dark:border-zinc-700 dark:bg-zinc-800"
              />
            </label>
            {wizardLog.length > 0 && (
              <ul className="mt-3 space-y-1 rounded-lg border border-zinc-100 bg-zinc-50 p-2 text-xs dark:border-zinc-800 dark:bg-zinc-800/60">
                {wizardLog.map((line, i) => (
                  <li key={i} className="text-zinc-600 dark:text-zinc-300">
                    {line}
                  </li>
                ))}
              </ul>
            )}
            {wizardError && (
              <p role="alert" className="mt-2 text-xs text-red-600 dark:text-red-400">
                {wizardError}
              </p>
            )}
            <button
              type="button"
              disabled={wizardBusy}
              onClick={() => void runWizard()}
              className="mt-3 w-full rounded-lg bg-[#006eb9] px-4 py-2 text-sm font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
            >
              {wizardBusy ? "Setting up…" : "Set up automatically"}
            </button>
          </div>
        </div>
      )}
      <button
        type="button"
        onClick={() => setWizardOpen(true)}
        className="mt-3 flex w-full items-center justify-center gap-2 rounded-xl border border-dashed border-[#006eb9]/50 bg-[#006eb9]/5 px-4 py-3 text-sm font-medium text-[#006eb9] hover:bg-[#006eb9]/10 dark:border-sky-500/40 dark:bg-sky-500/5 dark:text-sky-300"
      >
        <Plus size={15} aria-hidden="true" />
        Automatic setup: provider + key + model + alias in one flow
      </button>

      {/* Architecture card */}
      <div className={CARD + " mt-6"}>
        <h2 className="text-sm font-semibold">How it works, in one picture</h2>
        <pre className="mt-2 overflow-x-auto rounded-lg bg-zinc-100 p-3 text-xs leading-relaxed dark:bg-zinc-900">
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
