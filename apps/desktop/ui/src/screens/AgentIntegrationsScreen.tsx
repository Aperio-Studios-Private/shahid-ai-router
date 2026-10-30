import { useCallback, useState } from "react";
import { invokeOr } from "../lib/tauri";

interface ProfileResult {
  agent_id: string;
  config_text: string;
  files: string[];
  rollback: string;
  limitations: string;
  applicable: boolean;
  verification: string;
}

interface AgentOption {
  id: string;
  label: string;
}

// docs/09: Codex CLI is documented-unsupported in v1 (ADR-010 amendment).
const AGENTS: AgentOption[] = [
  { id: "claude-code", label: "Claude Code" },
  { id: "gemini-cli", label: "Gemini CLI" },
  { id: "generic-openai", label: "Generic OpenAI-compatible" },
  { id: "codex-cli", label: "OpenAI Codex CLI (unsupported)" },
];

export function AgentIntegrationsScreen() {
  const [agentId, setAgentId] = useState("claude-code");
  const [alias, setAlias] = useState("");
  const [profile, setProfile] = useState<ProfileResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const generate = useCallback(async () => {
    setBusy(true);
    setError(null);
    setProfile(null);
    try {
      const result = await invokeOr<ProfileResult>("generate_agent_profile", {
        agentId,
        alias,
        fastAlias: null,
      });
      if (result) setProfile(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }, [agentId, alias]);

  const applyToFile = async (index: number) => {
    if (!profile) return;
    const file = profile.files[index];
    const hash = await invokeOr<string>("hash_profile_target", {
      agentId: profile.agent_id,
      fileIndex: index,
    });
    const confirmed = window.confirm(
      `About to modify ${file}.\n\n` +
        "A backup (shahid-backup-<timestamp>) is written next to it first.\n" +
        "The token appears only as the placeholder — replace it after applying.\n\n" +
        "Continue?",
    );
    if (!confirmed) return;
    setBusy(true);
    setError(null);
    try {
      const applied = await invokeOr<string>("apply_agent_profile", {
        agentId: profile.agent_id,
        fileIndex: index,
        content: profile.config_text,
        expectedExisting: hash ?? "",
      });
      setError(`Applied to ${applied}. Rollback: ${profile.rollback}`);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">Agent Integrations</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Profiles are contract-tested (config shape + gateway contract). The
        real CLIs were not executed here — verify interactively before
        production use. OpenAI Codex CLI is unsupported in v1 (requires the
        /v1/responses wire API).
      </p>

      <div className="mt-4 flex flex-wrap items-end gap-2 text-sm">
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase tracking-wide text-zinc-500">Agent</span>
          <select
            value={agentId}
            onChange={(e) => setAgentId(e.target.value)}
            className="rounded-md border border-zinc-300 px-3 py-1.5 dark:border-zinc-700 dark:bg-zinc-900"
          >
            {AGENTS.map((a) => (
              <option key={a.id} value={a.id}>
                {a.label}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs uppercase tracking-wide text-zinc-500">Alias</span>
          <input
            value={alias}
            onChange={(e) => setAlias(e.target.value)}
            placeholder="e.g. fast-model"
            className="w-40 rounded-md border border-zinc-300 px-3 py-1.5 font-mono text-xs dark:border-zinc-700 dark:bg-zinc-900"
          />
        </label>
        <button
          type="button"
          disabled={busy || !alias}
          onClick={() => void generate()}
          className="rounded-md bg-[#006eb9] px-3 py-1.5 font-medium text-white disabled:opacity-40"
        >
          Generate
        </button>
      </div>

      {error && (
        <p role="alert" className="mt-3 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {error}
        </p>
      )}

      {profile && (
        <div className="mt-4">
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            {profile.verification} — {profile.limitations}
          </p>
          <pre className="mt-2 overflow-x-auto rounded-md border border-zinc-200 bg-zinc-100 p-3 text-xs dark:border-zinc-800 dark:bg-zinc-900">
            {profile.config_text || "(no generated configuration)"}
          </pre>
          <button
            type="button"
            disabled={!profile.config_text}
            onClick={() => void navigator.clipboard.writeText(profile.config_text)}
            className="mt-2 rounded-md border border-zinc-300 px-3 py-1.5 text-xs disabled:opacity-40 dark:border-zinc-700"
          >
            Copy config
          </button>
          {profile.applicable &&
            profile.files.map((file, index) => (
              <div key={file} className="mt-3 text-sm">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void applyToFile(index)}
                  className="rounded-md border border-amber-400 px-3 py-1.5 text-xs font-medium text-amber-700 hover:bg-amber-50 disabled:opacity-40 dark:border-amber-600 dark:text-amber-400"
                >
                  Apply to {file}… (backup written first)
                </button>
              </div>
            ))}
          {profile.rollback && (
            <p className="mt-2 text-xs text-zinc-500 dark:text-zinc-400">
              Rollback: {profile.rollback}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
