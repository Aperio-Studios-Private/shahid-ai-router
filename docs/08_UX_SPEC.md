# Desktop UX Specification

## Direction
Professional developer utility: clean, compact, readable and information-dense. Light/dark themes. Avoid gratuitous 3D and decorative animation.

## Navigation
Overview; Providers (includes API Keys, Models and Deployments — grouped per the Screens section below); Routing Policies & Aliases; Agent Integrations; Local Gateway; Request Logs; Usage; Health Monitor; Backup & Restore; Settings; About. (Amendment 2026-10-31, Phase 9: the originally separate Models/Deployments entries fold into Providers per the Screens grouping; Models/Deployments entries were internally inconsistent with that grouping.)

## Screens
Dashboard: gateway state, endpoint, provider/credential health, aliases, request/error/latency summaries.
Providers: search, add/edit/test/enable/disable.
API Keys (within Providers): masked values, labels, health, cooldown, account grouping.
Models/deployments (within Providers): provider IDs, aliases, capabilities, verification date. (Amendment 2026-10-31, audit fix: the Screens section previously still listed these as separate screens, contradicting the Navigation fold; the UI routes api-keys/models/deployments to the Providers screen accordingly.)
Routing: strategy, priorities, weights, fallback, retries and route preview.
Agent integrations: exact settings, copy controls, test and optional consent-based config edit.
Logs: filters, correlation IDs and redaction.
Backup: export mode, import preview, conflict handling and validation.

## Interaction
Every async action has loading/success/failure states. Destructive actions confirm. Validate on frontend and backend. Keyboard navigation and visible focus. Do not use color alone for status. Explain why a deployment is unavailable. Never expose secrets in tooltips or logs.

## UX redesign (Phase 13, 2026-11-01)

Guided-first redesign (research: setup checklists, teaching empty states,
progressive disclosure):

- **Navigation**: 7 primary destinations — Home, Providers, Routing, Agent
  Integrations, Local Gateway, Activity (Logs/Usage/Health tabs), Backup &
  Restore — plus About/Settings footer links. API Keys/Models/Deployments and
  Logs/Usage/Health folds are now reflected in the sidebar itself.
- **Home** = live "Get started" checklist (7 steps; progress derived from real
  data: provider/key/deployment/alias-bound/first-request/cloud-configured),
  each step one-line explained with a jump action; plain-language architecture
  diagram.
- **Tooltips**: accessible inline help (hover/focus/click, aria-describedby) on
  every non-obvious control; secrets are never shown in tooltips (rule above).
- **No raw ids**: alias binding uses a "Provider · model" dropdown.
- **State matrix (the checkpoint — every async surface × state)**:

| Surface | Loading | Empty | Error | Success | Busy-guard |
|---|---|---|---|---|---|
| Home checklist | spinner | n/a (steps show) | alert | ✓ badges per step | n/a |
| Providers list | spinner | EmptyState + CTA | alert | enable/disable inline | buttons disabled while busy |
| Credentials / Deployments panels | n/a (instant) | Hint | alert | list refresh | buttons disabled |
| Routing aliases | text | EmptyState + CTA | alert | refreshed list | bind/add disabled while busy |
| Gateway status | text | n/a | action alert + server error banner | status chip | start/stop disabled |
| Token reveal/copy | instant | n/a | action alert | masked→revealed | copy guarded |
| Logs | text | "No requests recorded yet" | alert | n/a | refresh disabled? (cheap read) |
| Usage / Health | "Loading…" | counts/badges | alert | n/a | poll-safe |
| Backup export/import | busy flag | n/a | alert | role=status message | buttons disabled |
| Cloud backup | busy flag | "No snapshots yet" | alert | status + messages | all buttons disabled |
| Agent profiles | busy flag | n/a | alert | role=status message | generate/apply disabled |

Rule (docs/08 interaction): every async action shows loading, ends in success or
a readable error, and disables its trigger while in flight.
