import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  Bot,
  Box,
  DatabaseBackup,
  Globe,
  Home,
  Info,
  Keyboard,
  Moon,
  Play,
  Route,
  Search,
  Server,
  Settings,
  Square,
  Sun,
} from "lucide-react";
import { FOOTER_SCREENS, SCREENS, type ScreenId } from "./lib/screens";
import { invokeOr, type AppInfo } from "./lib/tauri";
import { OverviewScreen } from "./screens/OverviewScreen";
import { GatewayScreen } from "./screens/GatewayScreen";
import { ProvidersScreen } from "./screens/ProvidersScreen";
import { AliasesScreen } from "./screens/AliasesScreen";
import { AgentIntegrationsScreen } from "./screens/AgentIntegrationsScreen";
import { ActivityScreen } from "./screens/ActivityScreen";
import { UsageScreen } from "./screens/UsageScreen";
import { HealthScreen } from "./screens/HealthScreen";
import { LogsScreen } from "./screens/LogsScreen";
import { BackupScreen } from "./screens/BackupScreen";
import { AboutScreen } from "./screens/AboutScreen";
import { PlaceholderScreen } from "./screens/PlaceholderScreen";
import { SettingsScreen } from "./screens/SettingsScreen";

function useTheme(): [boolean, () => void] {
  const [dark, setDark] = useState<boolean>(() =>
    window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
  }, [dark]);
  return [dark, () => setDark((d) => !d)];
}

const NAV_ICONS: Record<string, typeof Home> = {
  overview: Home,
  providers: Server,
  routing: Route,
  "agent-integrations": Bot,
  gateway: Globe,
  activity: Activity,
  backup: DatabaseBackup,
  settings: Settings,
  about: Info,
};

const NAV_GROUPS: { label: string; ids: ScreenId[] }[] = [
  { label: "General", ids: ["overview", "activity"] },
  { label: "Configuration", ids: ["providers", "routing", "agent-integrations"] },
  { label: "System", ids: ["gateway", "backup"] },
];

interface PaletteItem {
  id: string;
  label: string;
  hint: string;
  icon: typeof Home;
  run: () => void;
}

/** ⌘K command palette (Phase 15): jump anywhere + core actions. */
function CommandPalette({
  open,
  onClose,
  onNavigate,
  gatewayRunning,
}: {
  open: boolean;
  onClose: () => void;
  onNavigate: (id: ScreenId) => void;
  gatewayRunning: boolean;
}) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [actionNote, setActionNote] = useState<string | null>(null);

  const items: PaletteItem[] = useMemo(() => {
    const nav: PaletteItem[] = [...SCREENS, ...FOOTER_SCREENS].map((s) => ({
      id: `nav-${s.id}`,
      label: s.label,
      hint: "Go to",
      icon: NAV_ICONS[s.id] ?? Box,
      run: () => onNavigate(s.id),
    }));
    const actions: PaletteItem[] = [
      {
        id: "act-gateway",
        label: gatewayRunning ? "Stop gateway" : "Start gateway",
        hint: "Action",
        icon: gatewayRunning ? Square : Play,
        run: () => {
          void invokeOr(gatewayRunning ? "stop_gateway" : "start_gateway", {}).then(
            () => setActionNote(gatewayRunning ? "Gateway stopped" : "Gateway started"),
          );
        },
      },
      {
        id: "act-backup",
        label: "Back up to cloud now",
        hint: "Action",
        icon: DatabaseBackup,
        run: () => {
          void invokeOr("cloud_backup_now", {}).then((r) =>
            setActionNote(r ? "Snapshot uploaded" : "Cloud not configured"),
          );
        },
      },
    ];
    return [...nav, ...actions];
  }, [onNavigate, gatewayRunning]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter((i) => i.label.toLowerCase().includes(q));
  }, [items, query]);

  useEffect(() => setIndex(0), [query, open]);
  useEffect(() => {
    if (open) {
      setQuery("");
      setActionNote(null);
    }
  }, [open]);

  if (!open) return null;

  return (
    <div
      className="palette-backdrop fixed inset-0 z-50 flex items-start justify-center pt-32"
      onClick={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-label="Command palette"
        className="rise w-[540px] max-w-[90vw] overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-2xl dark:border-zinc-800 dark:bg-zinc-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-zinc-200 px-3 dark:border-zinc-800">
          <Search size={15} className="text-zinc-400" aria-hidden="true" />
          <input
            autoFocus
            aria-label="Search commands"
            placeholder="Search screens and actions…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setIndex((i) => Math.min(i + 1, filtered.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIndex((i) => Math.max(i - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                filtered[index]?.run();
                if (filtered[index]?.hint === "Go to") onClose();
              } else if (e.key === "Escape") {
                onClose();
              }
            }}
            className="w-full bg-transparent py-3 text-sm outline-none placeholder:text-zinc-400"
          />
          <kbd className="rounded border border-zinc-200 px-1.5 py-0.5 text-[10px] text-zinc-400 dark:border-zinc-700">
            esc
          </kbd>
        </div>
        {actionNote && (
          <p className="px-3 py-2 text-xs text-emerald-600 dark:text-emerald-400" role="status">
            {actionNote}
          </p>
        )}
        <ul className="max-h-80 overflow-y-auto p-1.5" role="listbox" aria-label="Commands">
          {filtered.length === 0 && (
            <li className="px-3 py-6 text-center text-sm text-zinc-400">No matches</li>
          )}
          {filtered.map((item, i) => {
            const Icon = item.icon;
            return (
              <li key={item.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={i === index}
                  onMouseEnter={() => setIndex(i)}
                  onClick={() => {
                    item.run();
                    if (item.hint === "Go to") onClose();
                  }}
                  className={
                    "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm " +
                    (i === index
                      ? "bg-[#006eb9]/10 text-[#006eb9] dark:bg-sky-500/10 dark:text-sky-300"
                      : "text-zinc-600 dark:text-zinc-300")
                  }
                >
                  <Icon size={15} aria-hidden="true" />
                  <span className="flex-1">{item.label}</span>
                  <span className="text-[10px] uppercase tracking-wide text-zinc-400">
                    {item.hint}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}

export default function App() {
  const [active, setActive] = useState<ScreenId>("overview");
  const [dark, toggleTheme] = useTheme();
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    invokeOr<AppInfo>("get_app_info")
      .then((info) => {
        if (!cancelled) setAppInfo(info);
      })
      .catch(() => {
        // Browser preview has no Tauri bridge; leave the fallback label.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((o) => !o);
      } else if (e.key === "Escape") {
        setPaletteOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const navigate = (id: ScreenId) => {
    setActive(id);
    setPaletteOpen(false);
  };

  const renderScreen = () => {
    if (active === "overview") return <OverviewScreen appInfo={appInfo} onNavigate={navigate} />;
    if (active === "gateway") return <GatewayScreen />;
    if (active === "providers" || active === "api-keys" || active === "models" || active === "deployments")
      return <ProvidersScreen />;
    if (active === "routing") return <AliasesScreen />;
    if (active === "agent-integrations") return <AgentIntegrationsScreen />;
    if (active === "activity") return <ActivityScreen />;
    if (active === "logs") return <LogsScreen />;
    if (active === "usage") return <UsageScreen />;
    if (active === "health") return <HealthScreen />;
    if (active === "backup") return <BackupScreen />;
    if (active === "about") return <AboutScreen appInfo={appInfo} />;
    if (active === "settings") return <SettingsScreen />;
    return (
      <PlaceholderScreen
        title={SCREENS.find((s) => s.id === active)?.label ?? "Settings"}
        note="Settings arrive in a later phase. Unimplemented requirements stay visibly incomplete (docs/01 acceptance rule)."
      />
    );
  };

  const navButton = (id: ScreenId, label: string, small = false) => {
    const Icon = NAV_ICONS[id] ?? Box;
    const isActive = active === id;
    return (
      <button
        type="button"
        aria-current={isActive ? "page" : undefined}
        onClick={() => setActive(id)}
        className={
          "flex w-full items-center gap-2.5 rounded-lg px-2.5 text-left transition-colors " +
          (small ? "py-1 text-xs " : "py-1.5 text-sm ") +
          (isActive
            ? "bg-white font-medium text-[#006eb9] shadow-sm dark:bg-zinc-800 dark:text-sky-300"
            : "text-zinc-600 hover:bg-white/60 dark:text-zinc-400 dark:hover:bg-zinc-800/60")
        }
      >
        <Icon size={small ? 13 : 15} aria-hidden="true" />
        {label}
      </button>
    );
  };

  return (
    <div className="flex h-full">
      <nav
        aria-label="Main navigation"
        className="flex w-52 shrink-0 flex-col border-r border-zinc-200 bg-zinc-200/50 py-3 dark:border-zinc-800/80 dark:bg-zinc-900/60"
      >
        <div className="px-3 pb-3">
          <p className="text-sm font-semibold tracking-tight">Shahid AI Router</p>
          <p className="text-[11px] text-zinc-500 dark:text-zinc-500">
            {appInfo ? `v${appInfo.version}` : "local-first AI gateway"}
          </p>
        </div>

        <button
          type="button"
          onClick={() => setPaletteOpen(true)}
          className="mx-3 mb-2 flex items-center gap-2 rounded-lg border border-zinc-200 bg-white px-2.5 py-1.5 text-xs text-zinc-500 shadow-sm hover:border-zinc-300 dark:border-zinc-700 dark:bg-zinc-800/60 dark:text-zinc-400"
        >
          <Search size={13} aria-hidden="true" />
          <span className="flex-1 text-left">Search…</span>
          <kbd className="flex items-center gap-0.5 rounded border border-zinc-200 px-1 py-0.5 text-[10px] dark:border-zinc-700">
            <Keyboard size={9} aria-hidden="true" />K
          </kbd>
        </button>

        <div className="flex-1 space-y-4 overflow-y-auto px-2">
          {NAV_GROUPS.map((group) => (
            <div key={group.label}>
              <p className="px-2.5 pb-1 text-[10px] font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-500">
                {group.label}
              </p>
              <div className="space-y-0.5">
                {group.ids.map((id) =>
                  navButton(id, SCREENS.find((s) => s.id === id)?.label ?? id),
                )}
              </div>
            </div>
          ))}
        </div>

        <div className="space-y-0.5 border-t border-zinc-200 px-2 pt-2 dark:border-zinc-800/80">
          {FOOTER_SCREENS.map((s) => navButton(s.id, s.label, true))}
          <button
            type="button"
            onClick={toggleTheme}
            className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1 text-xs text-zinc-500 hover:bg-white/60 dark:text-zinc-400 dark:hover:bg-zinc-800/60"
          >
            {dark ? <Sun size={13} aria-hidden="true" /> : <Moon size={13} aria-hidden="true" />}
            {dark ? "Switch to light" : "Switch to dark"}
          </button>
        </div>
      </nav>

      <main className="flex-1 overflow-y-auto p-6">
        <div key={active} className="rise">
          {renderScreen()}
        </div>
      </main>

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onNavigate={navigate}
        gatewayRunning={appInfo?.gateway_running ?? false}
      />
    </div>
  );
}
