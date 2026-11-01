import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";
import { EmptyState, Hint, Spinner, Tooltip } from "../components/ui";
import {
  CircleSlash,
  Copy,
  FlaskConical,
  KeyRound,
  ListPlus,
  Plus,
  Search,
  Server,
  Trash2,
} from "lucide-react";

interface Provider {
  id: string;
  name: string;
  provider_type: string;
  base_url: string;
  enabled: boolean;
  allow_loopback: boolean;
  notes: string | null;
}

interface Credential {
  id: string;
  label: string;
  status: string;
  priority: number;
  weight: number;
  cooldown_until: string | null;
}

interface ModelInfo {
  id: string;
  streaming: boolean;
  tool_calling: boolean;
  vision: boolean;
  structured_output: boolean;
  context_window: number | null;
}

interface Deployment {
  id: string;
  model_id: string;
  protocol_family: string;
  enabled: boolean;
  endpoint_override: string | null;
}

type Busy = "idle" | "loading" | "error";

const BTN =
  "inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 bg-white px-2.5 py-1.5 text-xs font-medium text-zinc-700 shadow-sm hover:bg-zinc-50 disabled:opacity-40 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-200 dark:hover:bg-zinc-700";
const BTN_DANGER =
  "inline-flex items-center gap-1.5 rounded-lg border border-red-200 bg-white px-2.5 py-1.5 text-xs font-medium text-red-600 shadow-sm hover:bg-red-50 disabled:opacity-40 dark:border-red-900/60 dark:bg-zinc-800 dark:text-red-400 dark:hover:bg-red-950/40";
const INPUT =
  "rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm shadow-sm placeholder:text-zinc-400 dark:border-zinc-700 dark:bg-zinc-800";

/** Well-known provider presets (Phase 15): base URL + type prefilled. */
interface Preset {
  name: string;
  provider_type: string;
  base_url: string;
  allow_loopback: boolean;
}
export const PRESETS: Preset[] = [
  { name: "NVIDIA NIM (hosted)", provider_type: "openai_compatible", base_url: "https://integrate.api.nvidia.com/v1", allow_loopback: false },
  { name: "OpenAI", provider_type: "openai", base_url: "https://api.openai.com/v1", allow_loopback: false },
  { name: "Anthropic", provider_type: "anthropic", base_url: "https://api.anthropic.com", allow_loopback: false },
  { name: "Google Gemini", provider_type: "gemini", base_url: "https://generativelanguage.googleapis.com", allow_loopback: false },
  { name: "OpenRouter", provider_type: "openrouter", base_url: "https://openrouter.ai/api/v1", allow_loopback: false },
  { name: "Groq", provider_type: "openai_compatible", base_url: "https://api.groq.com/openai/v1", allow_loopback: false },
  { name: "Together AI", provider_type: "openai_compatible", base_url: "https://api.together.xyz/v1", allow_loopback: false },
  { name: "DeepSeek", provider_type: "openai_compatible", base_url: "https://api.deepseek.com/v1", allow_loopback: false },
  { name: "Mistral", provider_type: "openai_compatible", base_url: "https://api.mistral.ai/v1", allow_loopback: false },
  { name: "xAI Grok", provider_type: "openai_compatible", base_url: "https://api.x.ai/v1", allow_loopback: false },
  { name: "Ollama (this Mac)", provider_type: "openai_compatible", base_url: "http://127.0.0.1:11434/v1", allow_loopback: true },
  { name: "LM Studio (this Mac)", provider_type: "openai_compatible", base_url: "http://127.0.0.1:1234/v1", allow_loopback: true },
  { name: "vLLM (this Mac)", provider_type: "openai_compatible", base_url: "http://127.0.0.1:8000/v1", allow_loopback: true },
];

/** Provider type → the deployment protocol family its models speak. */
export function familyFor(providerType: string): string {
  if (providerType === "anthropic") return "anthropic_messages";
  if (providerType === "gemini") return "gemini_generate_content";
  return "openai_chat";
}

/**
 * Providers (Phase 15): master-detail. The provider list lives on the left;
 * the selected provider's credentials and deployments live in fixed panels on
 * the right — the accordion-hunting problem is gone for good.
 */
export function ProvidersScreen() {
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState<Busy>("idle");
  const [error, setError] = useState<string | null>(null);
  const [showAddForm, setShowAddForm] = useState(false);
  const [form, setForm] = useState({
    name: "",
    base_url: "",
    provider_type: "openai_compatible",
    allow_loopback: false,
  });

  const refresh = useCallback(async (): Promise<Provider[]> => {
    const list = (await invokeOr<Provider[]>("list_providers", {})) ?? [];
    setProviders(list);
    return list;
  }, []);

  useEffect(() => {
    refresh()
      .then((list) => {
        if (list.length > 0) setSelectedId((cur) => cur ?? list[0].id);
      })
      .catch((err) => setError(String(err)));
  }, [refresh]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy("loading");
    setError(null);
    try {
      await fn();
      setBusy("idle");
      const list = await refresh();
      if (selectedId && !list.some((p) => p.id === selectedId)) {
        setSelectedId(list[0]?.id ?? null);
      }
    } catch (err) {
      setBusy("error");
      setError(String(err));
    }
  };

  const testProvider = async (id: string) => {
    setBusy("loading");
    setError(null);
    try {
      const result = await invokeOr<{ ok: boolean; status?: number; detail?: string }>(
        "test_provider",
        { id },
      );
      setBusy("idle");
      setError(
        result && !result.ok
          ? `Connection test failed: ${result.detail ?? `HTTP ${result.status}`}`
          : null,
      );
    } catch (err) {
      setBusy("error");
      setError(String(err));
    }
  };

  const selected = providers?.find((p) => p.id === selectedId) ?? null;

  return (
    <section className="flex h-full max-w-6xl flex-col">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold">Providers</h1>
          <Hint>
            Each provider is an upstream AI service. Select one to manage its API
            keys (stored in the macOS Keychain) and model deployments.
          </Hint>
        </div>
        <button
          type="button"
          onClick={() => setShowAddForm((v) => !v)}
          className="inline-flex items-center gap-1.5 rounded-lg bg-[#006eb9] px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-[#005c9b]"
        >
          <Plus size={15} aria-hidden="true" /> Add provider
        </button>
      </div>

      {error && (
        <p
          role="alert"
          className="mt-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-200"
        >
          {error}
        </p>
      )}

      {showAddForm && (
        <form
          className="rise mt-4 grid grid-cols-1 gap-2 rounded-xl border border-zinc-200 bg-white p-4 shadow-sm sm:grid-cols-2 dark:border-zinc-800 dark:bg-zinc-900"
          onSubmit={(e) => {
            e.preventDefault();
            void run(async () => {
              const newId = await invokeOr<string>("add_provider", {
                name: form.name,
                providerType: form.provider_type,
                baseUrl: form.base_url,
                allowLoopback: form.allow_loopback,
                notes: null,
              });
              if (newId) setSelectedId(newId);
              setShowAddForm(false);
              setForm({ name: "", base_url: "", provider_type: "openai_compatible", allow_loopback: false });
            });
          }}
        >
          <label className="text-xs font-medium text-zinc-500 dark:text-zinc-400">
            Name
            <input
              required
              placeholder="e.g. NVIDIA NIM"
              aria-label="Provider name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              className={INPUT + " mt-1 w-full"}
            />
          </label>
          <label className="text-xs font-medium text-zinc-500 dark:text-zinc-400">
            <span className="inline-flex items-center">
              Base URL
              <Tooltip label="OpenAI-style APIs need the /v1 suffix (https://api.openai.com/v1, https://integrate.api.nvidia.com/v1). Anthropic/Gemini endpoints are added WITHOUT /v1. Always https unless it runs on this Mac." />
            </span>
            <input
              required
              placeholder="https://integrate.api.nvidia.com/v1"
              aria-label="Base URL"
              value={form.base_url}
              onChange={(e) => setForm({ ...form, base_url: e.target.value })}
              className={INPUT + " mt-1 w-full font-mono text-xs"}
            />
          </label>
          <label className="text-xs font-medium text-zinc-500 dark:text-zinc-400">
            <span className="inline-flex items-center">
              Type
              <Tooltip label="openai_compatible covers OpenAI-style APIs including NVIDIA NIM, OpenRouter, Ollama and vLLM." />
            </span>
            <select
              aria-label="Provider type"
              value={form.provider_type}
              onChange={(e) => setForm({ ...form, provider_type: e.target.value })}
              className={INPUT + " mt-1 w-full"}
            >
              <option value="openai_compatible">OpenAI-compatible (NVIDIA NIM, Ollama…)</option>
              <option value="openai">OpenAI</option>
              <option value="openrouter">OpenRouter</option>
              <option value="anthropic">Anthropic</option>
              <option value="gemini">Gemini</option>
            </select>
          </label>
          <label className="mt-5 flex items-center gap-2 text-xs text-zinc-600 dark:text-zinc-300">
            <input
              type="checkbox"
              checked={form.allow_loopback}
              onChange={(e) => setForm({ ...form, allow_loopback: e.target.checked })}
            />
            Allow loopback HTTP (Ollama/LM Studio on this Mac)
            <Tooltip label="Security gate: plain-HTTP is only allowed to 127.0.0.1/localhost when this is checked. Internet providers always use https." />
          </label>
          <label className="text-xs font-medium text-zinc-500 sm:col-span-2 dark:text-zinc-400">
            Start from a well-known provider
            <select
              aria-label="Provider preset"
              defaultValue=""
              onChange={(e) => {
                const preset = PRESETS[Number(e.target.value)];
                if (preset) {
                  setForm({
                    name: preset.name,
                    base_url: preset.base_url,
                    provider_type: preset.provider_type,
                    allow_loopback: preset.allow_loopback,
                  });
                }
              }}
              className={INPUT + " mt-1 w-full"}
            >
              <option value="">— choose a preset (fills everything below) —</option>
              {PRESETS.map((preset, i) => (
                <option key={preset.name} value={i}>
                  {preset.name}
                </option>
              ))}
            </select>
          </label>
          <div className="sm:col-span-2">
            <button
              type="submit"
              disabled={busy === "loading"}
              className="rounded-lg bg-[#006eb9] px-4 py-2 text-sm font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
            >
              Add provider
            </button>
          </div>
        </form>
      )}

      <div className="mt-4 flex min-h-0 flex-1 gap-4">
        {/* Master list */}
        <div className="w-64 shrink-0 overflow-y-auto">
          {providers === null ? (
            <div className="flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
              <Spinner /> Loading…
            </div>
          ) : providers.length === 0 ? (
            <EmptyState
              title="No providers yet"
              body="Add your first AI service with the button above."
            />
          ) : (
            <ul className="space-y-1.5">
              {providers.map((p) => (
                <li key={p.id}>
                  <button
                    type="button"
                    aria-current={selectedId === p.id ? "true" : undefined}
                    onClick={() => setSelectedId(p.id)}
                    className={
                      "w-full rounded-xl border p-3 text-left transition-shadow " +
                      (selectedId === p.id
                        ? "border-[#006eb9]/40 bg-white shadow-md dark:border-sky-500/40 dark:bg-zinc-800"
                        : "border-zinc-200 bg-white shadow-sm hover:shadow dark:border-zinc-800 dark:bg-zinc-900")
                    }
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="flex min-w-0 items-center gap-2">
                        <Server
                          size={14}
                          aria-hidden="true"
                          className={p.enabled ? "text-emerald-600 dark:text-emerald-400" : "text-zinc-400"}
                        />
                        <span className="truncate text-sm font-medium">{p.name}</span>
                      </span>
                      {!p.enabled && (
                        <CircleSlash size={13} aria-hidden="true" className="shrink-0 text-zinc-400" />
                      )}
                    </div>
                    <p className="mt-1 truncate font-mono text-[11px] text-zinc-400">{p.base_url}</p>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        {/* Detail */}
        <div className="min-w-0 flex-1 overflow-y-auto">
          {selected ? (
            <div className="rise space-y-4">
              <div className="rounded-xl border border-zinc-200 bg-white p-4 shadow-sm dark:border-zinc-800 dark:bg-zinc-900">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="min-w-0">
                    <h2 className="text-sm font-semibold">{selected.name}</h2>
                    <p className="truncate font-mono text-[11px] text-zinc-400">{selected.base_url}</p>
                  </div>
                  <div className="flex gap-1.5">
                    <button type="button" disabled={busy === "loading"} className={BTN} onClick={() => void testProvider(selected.id)}>
                      <FlaskConical size={13} aria-hidden="true" /> Test connection
                    </button>
                    <button
                      type="button"
                      disabled={busy === "loading"}
                      className={BTN}
                      onClick={() => void run(() => invokeOr("set_provider_enabled", { id: selected.id, enabled: !selected.enabled }))}
                    >
                      {selected.enabled ? "Disable" : "Enable"}
                    </button>
                    <button
                      type="button"
                      className={BTN_DANGER}
                      onClick={() => {
                        if (window.confirm(`Delete provider "${selected.name}" and all its credentials/deployments?`)) {
                          void run(() => invokeOr("delete_provider", { id: selected.id }));
                        }
                      }}
                    >
                      <Trash2 size={13} aria-hidden="true" /> Delete
                    </button>
                  </div>
                </div>
              </div>
              <CredentialsPanel providerId={selected.id} run={run} busy={busy === "loading"} />
              <DeploymentsPanel
                providerId={selected.id}
                defaultFamily={familyFor(selected.provider_type)}
                run={run}
                busy={busy === "loading"}
              />
            </div>
          ) : (
            providers !== null &&
            providers.length > 0 && (
              <EmptyState title="Select a provider" body="Choose one on the left to manage its keys and deployments." />
            )
          )}
        </div>
      </div>
    </section>
  );
}

function PanelHeader({ icon: Icon, title, tooltip }: { icon: typeof KeyRound; title: string; tooltip: string }) {
  return (
    <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
      <Icon size={12} aria-hidden="true" />
      {title}
      <Tooltip label={tooltip} />
    </h3>
  );
}

function CredentialsPanel({
  providerId,
  run,
  busy,
}: {
  providerId: string;
  run: (fn: () => Promise<unknown>) => Promise<void>;
  busy: boolean;
}) {
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [panelError, setPanelError] = useState<string | null>(null);
  const [form, setForm] = useState({ label: "", secret: "", priority: 0, weight: 1 });

  const refresh = useCallback(async () => {
    try {
      const list = (await invokeOr<Credential[]>("list_credentials", { providerId })) ?? [];
      setCredentials(list);
    } catch (err) {
      setPanelError(String(err));
    }
  }, [providerId]);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="rounded-xl border border-zinc-200 bg-white p-4 shadow-sm dark:border-zinc-800 dark:bg-zinc-900">
      <PanelHeader
        icon={KeyRound}
        title="API keys"
        tooltip="Multiple keys per provider = automatic failover: if one hits a rate limit, the router rotates to the next. Secrets go straight into the macOS Keychain — the database stores only an opaque reference."
      />
      {panelError && (
        <p role="alert" className="mt-2 rounded border border-red-200 bg-red-50 px-2 py-1 text-xs text-red-800 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-200">
          {panelError}
        </p>
      )}
      {credentials.length === 0 ? (
        <Hint>Add your first key below — it is stored in the OS Keychain, never in the app database.</Hint>
      ) : (
        <ul className="mt-2 space-y-1">
          {credentials.map((c) => (
            <li
              key={c.id}
              className="flex items-center justify-between rounded-lg border border-zinc-100 px-2.5 py-1.5 text-xs dark:border-zinc-800"
            >
              <span className="flex min-w-0 items-center gap-2">
                <span
                  aria-hidden="true"
                  className={
                    "h-1.5 w-1.5 shrink-0 rounded-full " +
                    (c.cooldown_until
                      ? "bg-amber-400"
                      : c.status === "active"
                        ? "bg-emerald-500"
                        : "bg-zinc-300 dark:bg-zinc-600")
                  }
                />
                <span className="truncate font-medium">{c.label}</span>
                <span className="text-zinc-400">
                  {c.status}
                  {c.cooldown_until ? " · cooling" : ""} · p{c.priority} · w{c.weight}
                </span>
              </span>
              <span className="flex shrink-0 gap-1">
                <button
                  type="button"
                  disabled={busy}
                  className={BTN}
                  onClick={() =>
                    void run(() =>
                      invokeOr("set_credential_status", {
                        id: c.id,
                        status: c.status === "active" ? "quarantined" : "active",
                      }),
                    )
                  }
                >
                  <CircleSlash size={12} aria-hidden="true" />
                  {c.status === "active" ? "Quarantine" : "Reactivate"}
                </button>
                <button
                  type="button"
                  className={BTN_DANGER}
                  onClick={() => {
                    if (window.confirm(`Delete credential "${c.label}"? The vault item is removed too.`)) {
                      void run(() => invokeOr("delete_credential", { providerId, id: c.id }));
                    }
                  }}
                >
                  <Trash2 size={12} aria-hidden="true" />
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
      <form
        className="mt-3 flex flex-wrap items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            await invokeOr("add_credential", { providerId, ...form });
            setForm({ label: "", secret: "", priority: 0, weight: 1 });
          });
        }}
      >
        <input
          required
          placeholder="Label (e.g. work-key)"
          aria-label="Credential label"
          value={form.label}
          onChange={(e) => setForm({ ...form, label: e.target.value })}
          className={INPUT + " w-40 py-1.5 text-xs"}
        />
        <input
          required
          type="password"
          placeholder="API secret"
          aria-label="API secret"
          value={form.secret}
          onChange={(e) => setForm({ ...form, secret: e.target.value })}
          className={INPUT + " w-56 py-1.5 font-mono text-xs"}
        />
        <Tooltip label="Lower number = tried first when several keys exist." />
        <input
          type="number"
          placeholder="Priority"
          aria-label="Priority"
          value={form.priority}
          onChange={(e) => setForm({ ...form, priority: Number(e.target.value) })}
          className={INPUT + " w-20 py-1.5 text-xs"}
        />
        <Tooltip label="Weight only matters with the weighted strategy: higher = picked more often among equal-priority keys." />
        <input
          type="number"
          min={1}
          placeholder="Weight"
          aria-label="Weight"
          value={form.weight}
          onChange={(e) => setForm({ ...form, weight: Number(e.target.value) })}
          className={INPUT + " w-16 py-1.5 text-xs"}
        />
        <button type="submit" disabled={busy} className={BTN + " border-[#006eb9] bg-[#006eb9] text-white hover:bg-[#005c9b]"}>
          <Plus size={12} aria-hidden="true" /> Add key
        </button>
      </form>
    </div>
  );
}

function DeploymentsPanel({
  providerId,
  defaultFamily,
  run,
  busy,
}: {
  providerId: string;
  defaultFamily: string;
  run: (fn: () => Promise<unknown>) => Promise<void>;
  busy: boolean;
}) {
  const [deployments, setDeployments] = useState<Deployment[]>([]);
  const [panelError, setPanelError] = useState<string | null>(null);
  const [modelForm, setModelForm] = useState({ model_id: "", protocol_family: "openai_chat" });
  const [fetched, setFetched] = useState<ModelInfo[] | null>(null);
  const [addedIds, setAddedIds] = useState<Set<string>>(new Set());
  const [fetching, setFetching] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [modelQuery, setModelQuery] = useState("");

  const fetchModels = async () => {
    setFetching(true);
    setFetchError(null);
    try {
      // Rust command param is `id` — Tauri matches invoke keys by name.
      const list = await invokeOr<ModelInfo[]>("fetch_provider_models", { id: providerId });
      setFetched(list ?? []);
      setModelQuery("");
      setAddedIds(new Set());
    } catch (err) {
      setFetchError(String(err));
    } finally {
      setFetching(false);
    }
  };

  const filteredModels = (fetched ?? []).filter((m) =>
    m.id.toLowerCase().includes(modelQuery.trim().toLowerCase()),
  );

  const refresh = useCallback(async () => {
    try {
      const list = (await invokeOr<Deployment[]>("list_deployments", { providerId })) ?? [];
      setDeployments(list);
    } catch (err) {
      setPanelError(String(err));
    }
  }, [providerId]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  // Protocol family follows the provider's type (preset-aware).
  useEffect(() => {
    setModelForm((f) => ({ ...f, protocol_family: defaultFamily }));
  }, [defaultFamily, providerId]);

  return (
    <div className="rounded-xl border border-zinc-200 bg-white p-4 shadow-sm dark:border-zinc-800 dark:bg-zinc-900">
      <PanelHeader
        icon={Copy}
        title="Deployments"
        tooltip="A deployment = one upstream model this provider can serve. Aliases on the Routing screen bind to deployments to build fallback chains."
      />
      {panelError && (
        <p role="alert" className="mt-2 rounded border border-red-200 bg-red-50 px-2 py-1 text-xs text-red-800 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-200">
          {panelError}
        </p>
      )}
      {deployments.length === 0 ? (
        <Hint>
          No deployments yet — add the model id exactly as the provider names it (e.g. gpt-4o-mini,
          meta/llama-3.1-8b-instruct).
        </Hint>
      ) : (
        <ul className="mt-2 space-y-1">
          {deployments.map((d) => (
            <li
              key={d.id}
              className="flex items-center justify-between rounded-lg border border-zinc-100 px-2.5 py-1.5 text-xs dark:border-zinc-800"
            >
              <span className="flex min-w-0 items-center gap-2">
                <span
                  aria-hidden="true"
                  className={
                    "h-1.5 w-1.5 shrink-0 rounded-full " + (d.enabled ? "bg-emerald-500" : "bg-zinc-300 dark:bg-zinc-600")
                  }
                />
                <span className="truncate font-mono font-medium">{d.model_id}</span>
                <span className="text-zinc-400">{d.protocol_family}</span>
              </span>
              <span className="flex shrink-0 gap-1">
                <button
                  type="button"
                  disabled={busy}
                  className={BTN}
                  onClick={() => void run(() => invokeOr("set_deployment_enabled", { id: d.id, enabled: !d.enabled }))}
                >
                  {d.enabled ? "Disable" : "Enable"}
                </button>
                <button
                  type="button"
                  className={BTN_DANGER}
                  onClick={() => void run(() => invokeOr("delete_deployment", { id: d.id }))}
                >
                  <Trash2 size={12} aria-hidden="true" />
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
      <div className="mt-3 rounded-lg border border-zinc-100 bg-zinc-50/60 p-2.5 dark:border-zinc-800 dark:bg-zinc-800/40">
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled={fetching}
            onClick={() => void fetchModels()}
            className={BTN + " border-[#006eb9]/40 text-[#006eb9] dark:border-sky-500/40 dark:text-sky-300"}
          >
            <ListPlus size={13} aria-hidden="true" />
            {fetching ? "Fetching…" : fetched ? "Re-fetch model list" : "Fetch model list"}
          </button>
          <Hint>
            Pulls the live model catalogue from the provider so you never type ids
            blindly.
          </Hint>
        </div>
        {fetchError && (
          <p role="alert" className="mt-2 rounded border border-red-200 bg-red-50 px-2 py-1 text-xs text-red-700 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-300">
            {fetchError}
          </p>
        )}
        {fetched !== null && (
          <div className="mt-2">
            <div className="flex items-center gap-1.5 rounded-lg border border-zinc-200 bg-white px-2 py-1.5 dark:border-zinc-700 dark:bg-zinc-800">
              <Search size={13} className="text-zinc-400" aria-hidden="true" />
              <input
                placeholder={`Search ${fetched.length} models — click one to deploy it…`}
                aria-label="Search models"
                value={modelQuery}
                onChange={(e) => setModelQuery(e.target.value)}
                className="w-full bg-transparent text-xs outline-none placeholder:text-zinc-400"
              />
            </div>
            <ul className="mt-1.5 max-h-44 space-y-0.5 overflow-y-auto">
              {filteredModels.length === 0 && (
                <li className="px-2 py-2 text-xs text-zinc-400">No models match “{modelQuery}”</li>
              )}
              {filteredModels.map((m) => (
                <li key={m.id}>
                  <button
                    type="button"
                    disabled={busy}
                    title="Click to deploy this model (one click — capabilities auto-filled)"
                    onClick={() =>
                      void run(async () => {
                        const modelPk = await invokeOr<string>("add_model", {
                          providerId,
                          modelId: m.id,
                          protocolFamily: modelForm.protocol_family,
                          streaming: m.streaming,
                          toolCalling: m.tool_calling,
                          vision: m.vision,
                          structuredOutput: m.structured_output,
                          contextWindow: m.context_window,
                        });
                        await invokeOr("add_deployment", {
                          providerId,
                          modelPk,
                          endpointOverride: null,
                        });
                        setAddedIds((prev) => new Set(prev).add(m.id));
                        setModelForm((f) => ({ ...f, model_id: m.id }));
                      })
                    }
                    className={
                      "flex w-full items-center justify-between rounded-md px-2 py-1 text-left font-mono text-xs disabled:opacity-50 " +
                      (addedIds.has(m.id)
                        ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-300"
                        : "text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800")
                    }
                  >
                    <span className="truncate">{m.id}</span>
                    <span className="flex shrink-0 items-center gap-1 text-[10px] font-sans">
                      {m.vision && <span className="rounded bg-violet-100 px-1 text-violet-700 dark:bg-violet-500/15 dark:text-violet-300">vision</span>}
                      {m.tool_calling && <span className="rounded bg-sky-100 px-1 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300">tools</span>}
                      {m.context_window !== null && (
                        <span className="text-zinc-400">{Math.round(m.context_window / 1024)}k</span>
                      )}
                      {addedIds.has(m.id && m.id) || addedIds.has(m.id) ? (
                        <span className="font-medium text-emerald-600 dark:text-emerald-400">deployed ✓</span>
                      ) : null}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
      <form
        className="mt-3 flex flex-wrap items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            const modelPk = await invokeOr<string>("add_model", {
              providerId,
              modelId: modelForm.model_id,
              protocolFamily: modelForm.protocol_family,
              streaming: false,
              toolCalling: false,
              vision: false,
              structuredOutput: false,
              contextWindow: null,
            });
            if (modelPk) {
              await invokeOr("add_deployment", { providerId, modelPk, endpointOverride: null });
            }
            setModelForm({ model_id: "", protocol_family: "openai_chat" });
          });
        }}
      >
        <input
          required
          placeholder="Upstream model id (or pick above)"
          aria-label="Upstream model id"
          value={modelForm.model_id}
          onChange={(e) => setModelForm({ ...modelForm, model_id: e.target.value })}
          className={INPUT + " w-56 py-1.5 font-mono text-xs"}
        />
        <Tooltip label="Which wire protocol the model speaks: openai_chat for OpenAI-compatible APIs (NVIDIA NIM, OpenRouter, Ollama), anthropic_messages for Claude, gemini for Google Gemini. Must match the provider's real API." />
        <select
          aria-label="Protocol family"
          value={modelForm.protocol_family}
          onChange={(e) => setModelForm({ ...modelForm, protocol_family: e.target.value })}
          className={INPUT + " py-1.5 text-xs"}
        >
          <option value="openai_chat">openai_chat</option>
          <option value="anthropic_messages">anthropic_messages</option>
          <option value="gemini_generate_content">gemini</option>
        </select>
        <button type="submit" disabled={busy} className={BTN + " border-[#006eb9] bg-[#006eb9] text-white hover:bg-[#005c9b]"}>
          <Plus size={12} aria-hidden="true" /> Add deployment
        </button>
      </form>
    </div>
  );
}
