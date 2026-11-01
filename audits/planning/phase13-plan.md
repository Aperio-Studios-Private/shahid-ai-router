# Phase 13 Plan — UX overhaul: guided, state-complete UI (FR-025 draft)

**Date:** 2026-11-01 · **Status:** DRAFT → implemented in same session (UI-only phase; no backend/crypto surface changes).

## Research grounding
Setup checklists with progress, empty states that teach (one CTA per blank screen),
and progressive disclosure beat front-loaded config screens for dev tools
(sources: Medium "Designing for Nothing", "Empty States: UX Patterns That Teach
and Convert", "10 UX design principles for great onboarding").

## Problems in the current UI
1. No guidance: Home is static text; a new user cannot tell what to do first.
2. Navigation shows 14 items, 5 of which are placeholders (docs/08 fold made
   Models/Deployments/API-Keys aliases of Providers, but the sidebar still lists them).
3. Binding an alias requires pasting a raw hex deployment id — worst flow in the app.
4. Zero tooltips / helper text on non-obvious concepts (alias, deployment, priority
   vs weight, `/v1` base URL, allow-loopback, backup modes).
5. State coverage gaps: several screens lack loading spinners, empty CTAs, or
   busy-disabled buttons (docs/08 rule: every async action has loading/success/failure).

## Changes
- **Home = onboarding checklist**: live progress over real data (provider added /
  key added / deployment / alias bound / request sent / cloud configured), each
  step one-line explained with a "Do it" jump; gateway status chip + copyable
  curl example. (screens: OverviewScreen rewritten; App passes `onNavigate`.)
- **Navigation 14 → 7**: Home · Providers · Routing · Agent Integrations ·
  Local Gateway · Activity · Backup & Restore (+ About/Settings links in a footer
  section). New **Activity** screen with tabs reusing Logs/Usage/Health.
- **Tooltip component** (accessible: hover + focus + click toggle, aria-describedby)
  added to every non-obvious control; docs/08 rule respected (no secrets in tooltips).
- **Deployment dropdown** in the alias bind form: fetches all providers' deployments
  and offers "Provider · model" options — no more hex pasting.
- **State matrix** (checkpoint — all states): see phase report table; adds spinners,
  empty CTAs, busy-disabled buttons where missing.
- Plain-language copy throughout (what the gateway IS, why a key, etc.).

## Tasks
| ID | Task |
|---|---|
| P13-T01 | Tooltip + Spinner components |
| P13-T02 | screens.ts nav consolidation + App routing + ActivityScreen tabs |
| P13-T03 | OverviewScreen → guided Home (checklist, live data, jump actions) |
| P13-T04 | AliasesScreen: deployment dropdown, empty state, tooltips |
| P13-T05 | ProvidersScreen: tooltips, hints, loading/empty/busy states |
| P13-T06 | Gateway/Backup/Agent screens: tooltips + copy touch-ups |
| P13-T07 | docs/08 redesign section + state matrix; phase-13 report; boards |

Acceptance: tsc clean, vitest green, vite build ok; every screen renders
loading/empty/error/success states; no screen requires pasting raw ids.
