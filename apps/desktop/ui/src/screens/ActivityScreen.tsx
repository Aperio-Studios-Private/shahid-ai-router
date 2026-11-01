import { useState } from "react";
import { LogsScreen } from "./LogsScreen";
import { UsageScreen } from "./UsageScreen";
import { HealthScreen } from "./HealthScreen";

type Tab = "logs" | "usage" | "health";

const TABS: { id: Tab; label: string }[] = [
  { id: "logs", label: "Request Logs" },
  { id: "usage", label: "Usage" },
  { id: "health", label: "Health" },
];

/**
 * Activity (Phase 13): Request Logs, Usage and Health under one roof —
 * they are three views of the same question ("what happened, and is
 * everything OK?"), so they share one nav entry (docs/08 amendment).
 */
export function ActivityScreen() {
  const [tab, setTab] = useState<Tab>("logs");
  return (
    <section className="max-w-4xl">
      <h1 className="text-lg font-semibold">Activity</h1>
      <div
        role="tablist"
        aria-label="Activity views"
        className="mt-3 flex gap-1 border-b border-zinc-200 dark:border-zinc-800"
      >
        {TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            type="button"
            aria-selected={tab === t.id}
            onClick={() => setTab(t.id)}
            className={
              "-mb-px rounded-t-md border-b-2 px-3 py-1.5 text-sm " +
              (tab === t.id
                ? "border-[#006eb9] font-medium text-[#006eb9] dark:border-sky-400 dark:text-sky-400"
                : "border-transparent text-zinc-600 hover:text-zinc-800 dark:text-zinc-400 dark:hover:text-zinc-200")
            }
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="mt-4" role="tabpanel">
        {tab === "logs" && <LogsScreen />}
        {tab === "usage" && <UsageScreen />}
        {tab === "health" && <HealthScreen />}
      </div>
    </section>
  );
}
