import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";
import { EmptyState, Hint, Spinner, Tooltip } from "../components/ui";

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

interface Deployment {
  id: string;
  model_id: string;
  protocol_family: string;
  enabled: boolean;
  endpoint_override: string | null;
}

type Busy = "idle" | "loading" | "error";

export function ProvidersScreen() {
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [busy, setBusy] = useState<Busy>("idle");
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [form, setForm] = useState({ name: "", base_url: "", provider_type: "openai_compatible", allow_loopback: false });
  const [credentialForm, setCredentialForm] = useState({ label: "", secret: "", priority: 0, weight: 1 });
  const [modelForm, setModelForm] = useState({ model_id: "", protocol_family: "openai_chat" });

  const refresh = useCallback(async () => {
    try {
      const list = await invokeOr<Provider[]>("list_providers", {});
      setProviders(list ?? []);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy("loading");
    setError(null);
    try {
      await fn();
      setBusy("idle");
      await refresh();
    } catch (err) {
      setBusy("error");
      setError(String(err));
    }
  };

  const testProvider = async (id: string) => {
    setBusy("loading");
    setError(null);
    try {
      const result = await invokeOr<{ ok: boolean; status?: number; detail?: string }>("test_provider", { id });
      setBusy("idle");
      setError(result && !result.ok ? `test failed: ${result.detail ?? `HTTP ${result.status}`}` : null);
    } catch (err) {
      setBusy("error");
      setError(String(err));
    }
  };

  return (
    <section className="max-w-3xl">
      <h1 className="text-lg font-semibold">Providers</h1>
      {error && (
        <p role="alert" className="mt-2 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {error}
        </p>
      )}

      {providers === null ? (
        <div className="mt-4 flex items-center gap-2 text-sm text-zinc-500 dark:text-zinc-400">
          <Spinner /> Loading providers…
        </div>
      ) : providers.length === 0 ? (
        <EmptyState
          title="No providers yet"
          body="A provider is an AI service this router can call (OpenAI, Anthropic, Gemini, NVIDIA NIM, a local Ollama…). Add your first one with the form below — you'll need its API base URL and an API key."
        />
      ) : (
      <ul className="mt-4 space-y-2">
        {providers.map((p) => (
          <li key={p.id} className="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
            <div className="flex items-center justify-between gap-2">
              <button
                type="button"
                aria-expanded={expanded === p.id}
                className="flex items-center gap-1.5 text-left"
                onClick={() => setExpanded(expanded === p.id ? null : p.id)}
              >
                <span
                  aria-hidden="true"
                  className={
                    "inline-block text-xs text-zinc-400 transition-transform " +
                    (expanded === p.id ? "rotate-90" : "")
                  }
                >
                  ▶
                </span>
                <span className="text-sm font-medium">{p.name}</span>
                <span className="ml-1 text-xs text-zinc-500 dark:text-zinc-400">{p.provider_type}</span>
              </button>
              <button
                type="button"
                onClick={() => setExpanded(expanded === p.id ? null : p.id)}
                className="rounded border border-zinc-300 px-2 py-1 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
              >
                {expanded === p.id ? "Close" : "Add keys & models"}
              </button>
              <span
                className="rounded px-2 py-0.5 text-xs font-medium"
                aria-label={p.enabled ? "enabled" : "disabled"}
              >
                {p.enabled ? "● enabled" : "○ disabled"}
              </span>
            </div>
            <p className="mt-1 font-mono text-xs text-zinc-500 dark:text-zinc-400">{p.base_url}</p>
            <div className="mt-2 flex flex-wrap gap-2 text-xs">
              <button type="button" className="rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700" onClick={() => void run(() => invokeOr("set_provider_enabled", { id: p.id, enabled: !p.enabled }))}>
                {p.enabled ? "Disable" : "Enable"}
              </button>
              <button type="button" disabled={busy === "loading"} className="rounded border border-zinc-300 px-2 py-1 disabled:opacity-40 dark:border-zinc-700" onClick={() => void testProvider(p.id)}>
                {busy === "loading" ? "Testing…" : "Test"}
              </button>
              <button
                type="button"
                className="rounded border border-red-300 px-2 py-1 text-red-700 dark:border-red-800 dark:text-red-400"
                onClick={() => {
                  if (window.confirm(`Delete provider "${p.name}" and all its credentials/deployments?`)) {
                    void run(() => invokeOr("delete_provider", { id: p.id }));
                  }
                }}
              >
                Delete
              </button>
            </div>

            {expanded === p.id && (
              <div className="mt-3 space-y-4 border-t border-zinc-200 pt-3 dark:border-zinc-800">
                <CredentialsPanel providerId={p.id} form={credentialForm} setForm={setCredentialForm} run={run} />
                <DeploymentsPanel providerId={p.id} modelForm={modelForm} setModelForm={setModelForm} run={run} />
              </div>
            )}
          </li>
        ))}
      </ul>
      )}

      <h2 className="mt-6 text-sm font-semibold">
        Add provider
        <Tooltip label="The provider is the upstream AI service. Everything you add here stays on this Mac: the router only talks to it when one of your AI tools makes a request." />
      </h2>
      <form
        className="mt-2 grid max-w-xl grid-cols-1 gap-2 text-sm"
        onSubmit={(e) => {
          e.preventDefault();
          // Tauri v2 maps JS camelCase keys to Rust snake_case params; the
          // form state is snake_case, so map explicitly (audit P1 fix).
          void run(async () => {
            // run() refreshes afterwards; capture the new id first so we can
            // open the freshly added provider (its API-key form lives inside).
            const newId = await invokeOr<string>("add_provider", {
              name: form.name,
              providerType: form.provider_type,
              baseUrl: form.base_url,
              allowLoopback: form.allow_loopback,
              notes: null,
            });
            if (newId) setExpanded(newId);
          });
        }}
      >
        <input required placeholder="Name (shown in the app only)" aria-label="Provider name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900" />
        <input required placeholder="Base URL (https://…, include /v1 for OpenAI-style APIs)" aria-label="Base URL" value={form.base_url} onChange={(e) => setForm({ ...form, base_url: e.target.value })} className="rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900" />
        <span className="inline-flex items-center text-xs text-zinc-500 dark:text-zinc-400">
          Base URL
          <Tooltip label="Where the provider's API lives. OpenAI-style APIs need the /v1 suffix (https://api.openai.com/v1, https://integrate.api.nvidia.com/v1). Anthropic/Gemini endpoints are added WITHOUT /v1 — the app adds the fixed path itself." />
          <Tooltip label="Always https unless the provider runs on this same Mac (then enable loopback below)." />
        </span>
        <select aria-label="Provider type" value={form.provider_type} onChange={(e) => setForm({ ...form, provider_type: e.target.value })} className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900">
          <option value="openai_compatible">OpenAI-compatible (OpenAI, NVIDIA NIM, local servers…)</option>
          <option value="openai">OpenAI</option>
          <option value="openrouter">OpenRouter</option>
          <option value="anthropic">Anthropic</option>
          <option value="gemini">Gemini</option>
        </select>
        <label className="flex items-center gap-2 text-xs">
          <input type="checkbox" checked={form.allow_loopback} onChange={(e) => setForm({ ...form, allow_loopback: e.target.checked })} />
          Allow loopback HTTP (local dev providers like Ollama/LM Studio)
          <Tooltip label="Security gate: plain-HTTP (unencrypted) requests are only allowed to 127.0.0.1/localhost when this is checked. Internet providers always use https and don't need this." />
        </label>
        <button type="submit" disabled={busy === "loading"} className="rounded-md bg-[#006eb9] px-3 py-1.5 font-medium text-white hover:bg-[#005c9b] disabled:opacity-40">
          Add provider
        </button>
      </form>
    </section>
  );
}

function CredentialsPanel({
  providerId,
  form,
  setForm,
  run,
}: {
  providerId: string;
  form: { label: string; secret: string; priority: number; weight: number };
  setForm: (f: { label: string; secret: string; priority: number; weight: number }) => void;
  run: (fn: () => Promise<unknown>) => Promise<void>;
}) {
  const [credentials, setCredentials] = useState<Credential[]>([]);
  const [panelError, setPanelError] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      const list = await invokeOr<Credential[]>("list_credentials", { providerId });
      if (list) setCredentials(list);
    } catch (err) {
      setPanelError(String(err));
    }
  }, [providerId]);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div>
      <h3 className="text-xs font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
        API keys
        <Tooltip label="Multiple keys per provider = automatic failover. If one key hits a rate limit or dies, the router rotates to the next. Keys go straight into the macOS Keychain — the app database stores only an opaque reference." />
      </h3>
      {credentials.length === 0 && (
        <Hint>No keys yet — add one below. The secret is stored in the OS Keychain, never in the app database.</Hint>
      )}
      {panelError && (
        <p role="alert" className="mt-1 rounded border border-red-300 bg-red-50 px-2 py-1 text-xs text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {panelError}
        </p>
      )}
      <ul className="mt-1 space-y-1">
        {credentials.map((c) => (
          <li key={c.id} className="flex items-center justify-between rounded border border-zinc-200 px-2 py-1 text-xs dark:border-zinc-800">
            <span>
              {c.label} <span className="ml-1 text-zinc-500">{c.status}</span>
              {c.cooldown_until && <span className="ml-1 text-amber-600">cooling</span>}
            </span>
            <span className="flex gap-1">
              <button type="button" className="rounded border border-zinc-300 px-1.5 dark:border-zinc-700" onClick={() => void run(() => invokeOr("set_credential_status", { id: c.id, status: c.status === "active" ? "quarantined" : "active" }))}>
                {c.status === "active" ? "Quarantine" : "Reactivate"}
              </button>
              <button
                type="button"
                className="rounded border border-red-300 px-1.5 text-red-700 dark:border-red-800 dark:text-red-400"
                onClick={() => {
                  if (window.confirm(`Delete credential "${c.label}" (vault item removed)?`)) {
                    void run(() => invokeOr("delete_credential", { providerId, id: c.id }));
                  }
                }}
              >
                Delete
              </button>
            </span>
          </li>
        ))}
      </ul>
      <form
        className="mt-2 flex flex-wrap gap-2 text-xs"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            await invokeOr("add_credential", { providerId, ...form });
            setForm({ label: "", secret: "", priority: 0, weight: 1 });
          });
        }}
      >
        <input required placeholder="Label" aria-label="Credential label" value={form.label} onChange={(e) => setForm({ ...form, label: e.target.value })} className="w-32 rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900" />
        <input required type="password" placeholder="Secret (never stored in the app DB)" aria-label="API secret" value={form.secret} onChange={(e) => setForm({ ...form, secret: e.target.value })} className="w-56 rounded border border-zinc-300 px-2 py-1 font-mono dark:border-zinc-700 dark:bg-zinc-900" />
        <Tooltip label="Lower number = tried first when several keys exist." />
        <input type="number" placeholder="Priority" aria-label="Priority" value={form.priority} onChange={(e) => setForm({ ...form, priority: Number(e.target.value) })} className="w-20 rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900" />
        <Tooltip label="Weight only matters with the weighted strategy: higher weight = picked more often among equal-priority keys." />
        <input type="number" min={1} placeholder="Weight" aria-label="Weight" value={form.weight} onChange={(e) => setForm({ ...form, weight: Number(e.target.value) })} className="w-16 rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900" />
        <button type="submit" className="rounded bg-[#006eb9] px-2 py-1 font-medium text-white">Add key</button>
      </form>
    </div>
  );
}

function DeploymentsPanel({
  providerId,
  modelForm,
  setModelForm,
  run,
}: {
  providerId: string;
  modelForm: { model_id: string; protocol_family: string };
  setModelForm: (f: { model_id: string; protocol_family: string }) => void;
  run: (fn: () => Promise<unknown>) => Promise<void>;
}) {
  const [deployments, setDeployments] = useState<Deployment[]>([]);
  const [panelError, setPanelError] = useState<string | null>(null);
  const refresh = useCallback(async () => {
    try {
      const list = await invokeOr<Deployment[]>("list_deployments", { providerId });
      if (list) setDeployments(list);
    } catch (err) {
      setPanelError(String(err));
    }
  }, [providerId]);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div>
      <h3 className="text-xs font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
        Deployments
        <Tooltip label="A deployment = one upstream model this provider can serve (e.g. gpt-4o-mini). Aliases on the Routing screen bind to deployments to build fallback chains." />
      </h3>
      {deployments.length === 0 && (
        <Hint>No deployments yet — add the model id exactly as the provider names it (e.g. gpt-4o-mini, meta/llama-3.1-8b-instruct).</Hint>
      )}
      {panelError && (
        <p role="alert" className="mt-1 rounded border border-red-300 bg-red-50 px-2 py-1 text-xs text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {panelError}
        </p>
      )}
      <ul className="mt-1 space-y-1">
        {deployments.map((d) => (
          <li key={d.id} className="flex items-center justify-between rounded border border-zinc-200 px-2 py-1 text-xs dark:border-zinc-800">
            <span className="font-mono">{d.model_id}</span>
            <span className="flex gap-1">
              <button type="button" className="rounded border border-zinc-300 px-1.5 dark:border-zinc-700" onClick={() => void run(() => invokeOr("set_deployment_enabled", { id: d.id, enabled: !d.enabled }))}>
                {d.enabled ? "Disable" : "Enable"}
              </button>
              <button type="button" className="rounded border border-red-300 px-1.5 text-red-700 dark:border-red-800 dark:text-red-400" onClick={() => void run(() => invokeOr("delete_deployment", { id: d.id }))}>
                Delete
              </button>
            </span>
          </li>
        ))}
      </ul>
      <form
        className="mt-2 flex flex-wrap gap-2 text-xs"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => {
            const modelPk = await invokeOr<string>("add_model", {
              providerId,
              // camelCase keys required by the Tauri bridge (audit P1 fix);
              // capabilities default off — edit later via update_model_capabilities.
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
        <input required placeholder="Upstream model id" aria-label="Upstream model id" value={modelForm.model_id} onChange={(e) => setModelForm({ ...modelForm, model_id: e.target.value })} className="w-44 rounded border border-zinc-300 px-2 py-1 font-mono dark:border-zinc-700 dark:bg-zinc-900" />
        <Tooltip label="Which wire protocol the model speaks: openai_chat for OpenAI-compatible APIs (incl. NVIDIA NIM, OpenRouter, Ollama), anthropic_messages for Claude, gemini for Google Gemini. Must match the provider's real API." />
        <select aria-label="Protocol family" value={modelForm.protocol_family} onChange={(e) => setModelForm({ ...modelForm, protocol_family: e.target.value })} className="rounded border border-zinc-300 px-2 py-1 dark:border-zinc-700 dark:bg-zinc-900">
          <option value="openai_chat">openai_chat</option>
          <option value="anthropic_messages">anthropic_messages</option>
          <option value="gemini_generate_content">gemini</option>
        </select>
        <button type="submit" className="rounded bg-[#006eb9] px-2 py-1 font-medium text-white">Add deployment</button>
      </form>
    </div>
  );
}
