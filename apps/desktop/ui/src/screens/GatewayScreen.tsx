import { useCallback, useEffect, useState } from "react";
import { invokeOr } from "../lib/tauri";

interface GatewayState {
  running: boolean;
  addr: string | null;
  error: string | null;
}

type ActionState = "idle" | "loading" | "error";

const TOKEN_MASK = "••••••••••••••••";

export function GatewayScreen() {
  const [state, setState] = useState<GatewayState | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [revealed, setRevealed] = useState(false);
  const [action, setAction] = useState<ActionState>("idle");
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const view = await invokeOr<GatewayState>("get_gateway_state");
    if (view) setState(view);
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 4000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setAction("loading");
    setActionError(null);
    try {
      await fn();
      setAction("idle");
      await refresh();
    } catch (err) {
      setAction("error");
      setActionError(`${label} failed: ${String(err)}`);
    }
  };

  const start = () =>
    run("Start", () => invokeOr("start_gateway", {}) ?? Promise.resolve());

  const stop = () =>
    run("Stop", () => invokeOr("stop_gateway", {}) ?? Promise.resolve());

  const reveal = async () => {
    const value = await invokeOr<string>("reveal_gateway_token", {});
    if (value !== null) {
      setToken(value);
      setRevealed(true);
    }
  };

  const regenerate = () => {
    // Destructive: existing clients will receive 401 until reconfigured.
    if (
      !window.confirm(
        "Regenerate the gateway token? Existing clients configured with the current token will stop working until you update them.",
      )
    ) {
      return;
    }
    void run("Regenerate token", async () => {
      const value = await invokeOr<string>("regenerate_gateway_token", {});
      if (value !== null) {
        setToken(value);
        setRevealed(true);
      }
    });
  };

  const copy = async () => {
    if (token) {
      await navigator.clipboard.writeText(token);
    }
  };

  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">Local Gateway</h1>
      <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Binds 127.0.0.1 only. Requests require the gateway token.
      </p>

      <dl className="mt-4 grid grid-cols-2 gap-4 text-sm">
        <div className="rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
            Status
          </dt>
          <dd className="mt-1 flex items-center gap-2 font-medium">
            <span
              aria-hidden="true"
              className={
                "inline-block h-2 w-2 rounded-full " +
                (state?.running ? "bg-emerald-500" : "bg-zinc-400")
              }
            />
            {state === null ? "Loading…" : state.running ? "Running" : "Stopped"}
          </dd>
        </div>
        <div className="rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">
            Endpoint
          </dt>
          <dd className="mt-1 font-mono text-xs">
            {state?.addr ?? "not bound"}
          </dd>
        </div>
      </dl>

      {state?.error && (
        <p role="alert" className="mt-4 rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200">
          {state.error}
        </p>
      )}
      {actionError && (
        <p role="alert" className="mt-2 rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          {actionError}
        </p>
      )}

      <div className="mt-4 flex gap-2">
        <button
          type="button"
          disabled={action === "loading" || Boolean(state?.running)}
          onClick={() => void start()}
          className="rounded-md bg-[#006eb9] px-3 py-1.5 text-sm font-medium text-white hover:bg-[#005c9b] disabled:opacity-40"
        >
          Start
        </button>
        <button
          type="button"
          disabled={action === "loading" || !state?.running}
          onClick={() => void stop()}
          className="rounded-md border border-zinc-300 px-3 py-1.5 text-sm font-medium hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          Stop
        </button>
      </div>

      <h2 className="mt-8 text-sm font-semibold">Gateway token</h2>
      <div className="mt-2 flex items-center gap-2">
        <code className="flex-1 truncate rounded-md border border-zinc-200 bg-zinc-100 px-3 py-2 font-mono text-xs dark:border-zinc-800 dark:bg-zinc-900">
          {revealed && token ? token : TOKEN_MASK}
        </code>
        <button
          type="button"
          onClick={() => (revealed ? setRevealed(false) : void reveal())}
          className="rounded-md border border-zinc-300 px-3 py-2 text-xs hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          {revealed ? "Hide" : "Reveal"}
        </button>
        <button
          type="button"
          disabled={!revealed || !token}
          onClick={() => void copy()}
          className="rounded-md border border-zinc-300 px-3 py-2 text-xs hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800"
        >
          Copy
        </button>
      </div>
      <button
        type="button"
        onClick={() => void regenerate()}
        className="mt-3 rounded-md border border-red-300 px-3 py-1.5 text-xs font-medium text-red-700 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
      >
        Regenerate token…
      </button>
    </section>
  );
}
