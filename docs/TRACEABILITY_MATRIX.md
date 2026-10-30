# Requirements Traceability Matrix

Maintain requirement -> task -> source files -> tests/evidence -> status. Do not mark implemented without evidence.

Statuses: Planned | In progress | Implemented (evidence ref) | Verified (independent evidence).

| Requirement | Description | Phase | Task | Evidence | Status |
|---|---|---:|---|---|---|
| FR-001 | Provider management | 3+ | P3-T05 (UI/IPC), P3-T04 | reports/phase-reports/phase-3.md | Implemented |
| FR-002 | Credential pool | 4 | P3-T05 + P4 (groups, priority/weight) | reports/phase-reports/phase-4.md | Implemented |
| FR-003 | Model catalogue | 3 | P3-T02 (repos) + P3-T05 (UI) | reports/phase-reports/phase-3.md | Implemented |
| FR-004 | Virtual aliases (same-protocol, ADR-009) | 3-5 | P3-T03/T05 + P5 (capability matrix) | reports/phase-reports/phase-5.md | Implemented (priority/rr/weighted; lru/pin 501) |
| FR-005 | Local gateway (loopback + auth) | 3 | P3-T04 (middleware: P2-T03) | reports/phase-reports/phase-2.md | Implemented (middleware + lifecycle; protocol surfaces Phase 3) |
| FR-006 | Protocol surfaces (ADR-010 scope) | 3,6 | P3-T04 + P6 (anthropic/gemini) | reports/phase-reports/phase-6.md | Implemented (streaming Phase 7) |
| FR-007 | Capability-aware routing | 3-5 | P3-T03 + P5 (request-derived requirements) | reports/phase-reports/phase-5.md | Implemented (vision heuristic deferred) |
| FR-008 | Rotation | 4 | P3-T03/T04 + P4 (account grouping) | reports/phase-reports/phase-4.md | Implemented |
| FR-009 | Retry/failover | 4 | P3-T04 (ADR-017 semantics) | reports/phase-reports/phase-3.md | Implemented (v0) |
| FR-010 | Streaming | 7 | P7 (SSE relay + commit boundary) | reports/phase-reports/phase-7.md | Implemented (stream token counts NULL — FR-011 residual; raw-TCP scenarios Phase 11) |
| FR-011 | Request history | 3 | P3-T04 | reports/phase-reports/phase-3.md | Implemented (metadata + decision JSON) |
| FR-012 | Health diagnostics | 4+ | P3 + P4 (events/summary) | reports/phase-reports/phase-4.md | Implemented (UI Phase 9) |
| FR-013 | Agent profiles | 8 | P8 (generators + consent-gated apply) | reports/phase-reports/phase-8.md | Implemented (Codex unsupported by upstream — documented; live CLI verification pending) |
| FR-014 | Backup/restore | 10 | Phase 10 epic | TBD | Planned |
| FR-015 | Lifecycle | 1+ | P1-T04 (skeleton), P2-T03 | reports/phase-reports/phase-2.md | Implemented (start/stop/drain/exit hook) |
| FR-016 | Redacted diagnostics | 2+ | P2-T04 | reports/phase-reports/phase-2.md | Implemented (redaction + sweep tests) |
| FR-017 | Graceful drain | 3 | P2-T03 | reports/phase-reports/phase-2.md | Implemented (drain + exit hook, tested) |
| FR-018 | Port conflict surfaced | 1 | P2-T03 | reports/phase-reports/phase-2.md | Implemented (bind error, tested) |
| FR-019 | Token rotation | 2 | P2-T03 | reports/phase-reports/phase-2.md | Implemented (regenerate + hot-swap, tested) |
| FR-020 | Retention 30d | 3 | P2-T01 + P2-T03 | reports/phase-reports/phase-2.md | Implemented (purge + startup/daily task, tested) |
| FR-021 | Upstream concurrency bound | 4 | P4 (deployment semaphores) | reports/phase-reports/phase-4.md | Implemented |
| FR-022 | Lifecycle transparency | 1 | P2-T03 (UI gateway state) | reports/phase-reports/phase-2.md | Implemented (state + errors surfaced) |
| FR-023 | Routing scope/agent constraints | 3+ | P3-T03, Phase 8 | TBD | Planned |

NFR evidence: security (P2 gates), performance budget (Phase 11), a11y (Phase 9), cross-platform (ADR-013 CI matrix).
