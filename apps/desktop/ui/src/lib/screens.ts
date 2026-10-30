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
  | "settings"
  | "about";

export interface Screen {
  id: ScreenId;
  label: string;
}

/** Navigation structure mandated by docs/08_UX_SPEC.md. */
export const SCREENS: Screen[] = [
  { id: "overview", label: "Overview" },
  { id: "providers", label: "Providers" },
  { id: "api-keys", label: "API Keys" },
  { id: "models", label: "Models" },
  { id: "deployments", label: "Deployments" },
  { id: "routing", label: "Routing Policies" },
  { id: "agent-integrations", label: "Agent Integrations" },
  { id: "gateway", label: "Local Gateway" },
  { id: "logs", label: "Request Logs" },
  { id: "usage", label: "Usage" },
  { id: "health", label: "Health Monitor" },
  { id: "backup", label: "Backup & Restore" },
  { id: "settings", label: "Settings" },
  { id: "about", label: "About" },
];
