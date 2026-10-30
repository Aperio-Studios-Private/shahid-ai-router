
## Review resolution (2026-10-31) — verdict: APPROVED WITH REQUIRED REVISIONS

Report: `audits/planning/phase8-plan-review.md` (external verifications with sources: Codex `wire_api="chat"` REMOVED ~Feb 2026 — hard config error; Claude Code env block verified; Gemini CLI env-var mechanism verified).

| ID | Sev | Resolution |
|---|---|---|
| F-1 | High | **Codex CLI dropped from v1 generators** — `wire_api="chat"` is a hard config error in current Codex (removed variant), and `responses` is out of scope (FR-023). Codex ships as documented-UNSUPPORTED with limitation text ("requires /v1/responses, planned post-v1"); ADR-010 amended. |
| F-2 | High | apply_agent_profile hardened: closed allowlist (the shipped config paths only, resolved under the OS home dir), canonicalization + symlink/junction rejection on the target and parents, two-step consent with expected-content-hash precondition (TOCTOU), atomic write (temp+rename), size cap; threat-model addendum entry. |
| F-3 | Medium | Secret sweep extended to apply/backup paths (no token in logs/reports); generated files ALWAYS use the placeholder (enforced by generator design — apply rejects content without the placeholder); backups carry the previous user file (may contain user's own keys) — retention note: backups are user-managed, one per apply, path surfaced. |
| F-4 | Medium | Gemini generator corrected: `GOOGLE_GEMINI_BASE_URL` = bare `http://127.0.0.1:{port}` (SDK appends v1beta); config lives in `~/.gemini/.env` env vars (no settings.json base-URL field); limitations = gemini-api-key auth mode + generateContent-only surface (header form verified — ?key= moot). |
| F-5 | Medium | UI language: "contract-tested on <date>" never "supported" (docs/09); generate_agent_profile validates alias existence + same-protocol deployments (structured error otherwise); dated-citation fixture tests for each generator format. |
| F-6 | Low | Claude Code fast alias → `ANTHROPIC_DEFAULT_HAIKU_MODEL`; Windows home/UNC handled via home-dir allowlist join; limitations persisted in agent_profiles.notes. |

Status: READY FOR IMPLEMENTATION.
