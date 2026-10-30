import { useEffect, useState } from "react";
import { SCREENS, type ScreenId } from "./lib/screens";
import { invokeOr, type AppInfo } from "./lib/tauri";
import { OverviewScreen } from "./screens/OverviewScreen";
import { GatewayScreen } from "./screens/GatewayScreen";
import { PlaceholderScreen } from "./screens/PlaceholderScreen";

function useTheme(): [boolean, () => void] {
  const [dark, setDark] = useState<boolean>(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);
  return [dark, () => setDark((d) => !d)];
}

export default function App() {
  const [active, setActive] = useState<ScreenId>("overview");
  const [dark, toggleTheme] = useTheme();
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    let cancelled = false;
    void invokeOr<AppInfo>("get_app_info").then((info) => {
      if (!cancelled) setAppInfo(info);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="flex h-full">
      <nav
        aria-label="Main navigation"
        className="flex w-56 shrink-0 flex-col border-r border-zinc-200 bg-white py-3 dark:border-zinc-800 dark:bg-zinc-900"
      >
        <div className="px-4 pb-3">
          <p className="text-sm font-semibold tracking-tight">Shahid AI Router</p>
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            {appInfo ? `v${appInfo.version}` : "local-first AI gateway"}
          </p>
        </div>
        <ul className="flex-1 space-y-0.5 overflow-y-auto px-2">
          {SCREENS.map((screen) => (
            <li key={screen.id}>
              <button
                type="button"
                aria-current={active === screen.id ? "page" : undefined}
                onClick={() => setActive(screen.id)}
                className={
                  "w-full rounded-md px-3 py-1.5 text-left text-sm " +
                  (active === screen.id
                    ? "bg-[#006eb9]/10 font-medium text-[#006eb9] dark:bg-sky-500/10 dark:text-sky-400"
                    : "text-zinc-600 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800")
                }
              >
                {screen.label}
              </button>
            </li>
          ))}
        </ul>
        <button
          type="button"
          onClick={toggleTheme}
          className="mx-4 mt-3 rounded-md border border-zinc-200 px-3 py-1.5 text-xs text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
        >
          {dark ? "Switch to light" : "Switch to dark"}
        </button>
      </nav>
      <main className="flex-1 overflow-y-auto p-6">
        {active === "overview" ? (
          <OverviewScreen appInfo={appInfo} />
        ) : active === "gateway" ? (
          <GatewayScreen />
        ) : (
          <PlaceholderScreen
            title={SCREENS.find((s) => s.id === active)?.label ?? ""}
            note="This screen arrives with its implementation phase. Unimplemented requirements stay visibly incomplete (docs/01 acceptance rule)."
          />
        )}
      </main>
    </div>
  );
}
