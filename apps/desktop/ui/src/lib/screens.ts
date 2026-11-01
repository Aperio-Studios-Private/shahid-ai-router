export type ScreenId =
  | "overview"
  | "providers"
  | "api-keys"
  | "models"
  | "deployments"
  | "routing"
  | "agent-integrations"
  | "gateway"
  | "logs"
  | "usage"
  | "health"
  | "backup"
  | "activity"
  | "settings"
  | "about";

export interface Screen {
  id: ScreenId;
  label: string;
}

/** Primary navigation (Phase 13 redesign, docs/08 amendment): 7 real
 * destinations. API Keys/Models/Deployments fold into Providers; Logs/Usage/
 * Health fold into Activity. The ScreenId union keeps the old ids so deep
 * routes and the docs/08 amendments stay valid. */
export const SCREENS: Screen[] = [
  { id: "overview", label: "Home" },
  { id: "providers", label: "Providers" },
  { id: "routing", label: "Routing" },
  { id: "agent-integrations", label: "Agent Integrations" },
  { id: "gateway", label: "Local Gateway" },
  { id: "activity", label: "Activity" },
  { id: "backup", label: "Backup & Restore" },
];

/** Footer links (low-traffic destinations). */
export const FOOTER_SCREENS: Screen[] = [
  { id: "settings", label: "Settings" },
  { id: "about", label: "About" },
];
