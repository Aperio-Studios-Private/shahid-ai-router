import type { AppInfo } from "../lib/tauri";

export function AboutScreen({ appInfo }: { appInfo: AppInfo | null }) {
  return (
    <section className="max-w-2xl">
      <h1 className="text-lg font-semibold">About</h1>
      <dl className="mt-4 space-y-3 text-sm">
        <div>
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">Application</dt>
          <dd className="mt-1">Shahid AI Router {appInfo ? `v${appInfo.version}` : ""}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">What it is</dt>
          <dd className="mt-1">
            A local-first gateway that manages multiple AI providers, rotates
            credentials, and exposes one authenticated loopback endpoint
            (OpenAI / Anthropic / Gemini surfaces) for AI coding agents.
          </dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">Platform support</dt>
          <dd className="mt-1">
            Windows 10+ x64 and macOS 12+ (x86_64 / aarch64). Linux is not a
            supported platform for v1.
          </dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">Privacy</dt>
          <dd className="mt-1">
            Secrets live only in the OS credential store; request records are
            metadata-only (no prompts or responses) with 30-day retention.
          </dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wide text-zinc-500 dark:text-zinc-400">License</dt>
          <dd className="mt-1">Proprietary — see LICENSE in the repository.</dd>
        </div>
      </dl>
    </section>
  );
}
