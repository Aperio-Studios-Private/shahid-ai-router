import type { AppInfo } from "../lib/tauri";

interface Props {
  appInfo: AppInfo | null;
}

export function OverviewScreen({ appInfo }: Props) {
  return (
    <section>
      <h1 className="text-lg font-semibold">Overview</h1>
      <dl className="mt-4 grid max-w-xl grid-cols-2 gap-4 text-sm">
        <div className="rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">Application</dt>
          <dd className="mt-1 font-medium">{appInfo ? `${appInfo.name} v${appInfo.version}` : "UI preview (no Tauri bridge)"}</dd>
        </div>
        <div className="rounded-lg border border-zinc-200 p-4 dark:border-zinc-800">
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">Local gateway</dt>
          <dd className="mt-1 font-medium">
            {appInfo
              ? appInfo.gateway_running
                ? "Running"
                : "Stopped"
              : "Unknown outside the desktop shell"}
          </dd>
        </div>
      </dl>
      <p className="mt-6 max-w-xl text-sm text-zinc-500 dark:text-zinc-400">
        Providers, credentials, aliases and routing arrive in Phases 2&ndash;5. The gateway endpoint
        and its state will appear here once the security layer lands (Phase 2).
      </p>
    </section>
  );
}
