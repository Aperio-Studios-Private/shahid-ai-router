# Desktop UX Specification

## Direction
Professional developer utility: clean, compact, readable and information-dense. Light/dark themes. Avoid gratuitous 3D and decorative animation.

## Navigation
Overview; Providers (includes API Keys, Models and Deployments — grouped per the Screens section below); Routing Policies & Aliases; Agent Integrations; Local Gateway; Request Logs; Usage; Health Monitor; Backup & Restore; Settings; About. (Amendment 2026-10-31, Phase 9: the originally separate Models/Deployments entries fold into Providers per the Screens grouping; Models/Deployments entries were internally inconsistent with that grouping.)

## Screens
Dashboard: gateway state, endpoint, provider/credential health, aliases, request/error/latency summaries.
Providers: search, add/edit/test/enable/disable.
API Keys: masked values, labels, health, cooldown, account grouping.
Models/deployments: provider IDs, aliases, capabilities, verification date.
Routing: strategy, priorities, weights, fallback, retries and route preview.
Agent integrations: exact settings, copy controls, test and optional consent-based config edit.
Logs: filters, correlation IDs and redaction.
Backup: export mode, import preview, conflict handling and validation.

## Interaction
Every async action has loading/success/failure states. Destructive actions confirm. Validate on frontend and backend. Keyboard navigation and visible focus. Do not use color alone for status. Explain why a deployment is unavailable. Never expose secrets in tooltips or logs.
