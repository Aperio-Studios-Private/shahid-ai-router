# Requirements Traceability Matrix

Maintain requirement -> task -> source files -> tests/evidence -> status. Do not mark implemented without evidence.

Statuses: Planned | In progress | Implemented (evidence ref) | Verified (independent evidence).

| Requirement | Description | Phase | Task | Evidence | Status |
|---|---|---:|---|---|---|
| FR-001 | Provider management | 3+ | P3-T05 (UI), P3-T04 (backend) | TBD | Planned |
| FR-002 | Credential pool | 4 | Phase 4 epic | TBD | Planned |
| FR-003 | Model catalogue | 3 | P3-T01/T03 | TBD | Planned |
| FR-004 | Virtual aliases (same-protocol, ADR-009) | 3-5 | P3-T03, Phase 5 | TBD | Planned |
| FR-005 | Local gateway (loopback + auth) | 3 | P3-T04 (middleware: P2-T03) | reports/phase-reports/phase-2.md | Implemented (middleware + lifecycle; protocol surfaces Phase 3) |
| FR-006 | Protocol surfaces (ADR-010 scope) | 3,6 | P3-T04, Phase 6 | TBD | Planned |
| FR-007 | Capability-aware routing | 3-5 | P3-T03, Phase 5 | TBD | Planned |
| FR-008 | Rotation | 4 | Phase 4 epic | TBD | Planned |
| FR-009 | Retry/failover | 4 | Phase 4 epic | TBD | Planned |
| FR-010 | Streaming | 7 | Phase 7 epic | TBD | Planned |
| FR-011 | Request history | 3 | P3-T04 | TBD | Planned |
| FR-012 | Health diagnostics | 4+ | Phase 4 epic | TBD | Planned |
| FR-013 | Agent profiles | 8 | Phase 8 epic | TBD | Planned |
| FR-014 | Backup/restore | 10 | Phase 10 epic | TBD | Planned |
| FR-015 | Lifecycle | 1+ | P1-T04 (skeleton), P2-T03 | reports/phase-reports/phase-2.md | Implemented (start/stop/drain/exit hook) |
| FR-016 | Redacted diagnostics | 2+ | P2-T04 | reports/phase-reports/phase-2.md | Implemented (redaction + sweep tests) |
| FR-017 | Graceful drain | 3 | P2-T03 | reports/phase-reports/phase-2.md | Implemented (drain + exit hook, tested) |
| FR-018 | Port conflict surfaced | 1 | P2-T03 | reports/phase-reports/phase-2.md | Implemented (bind error, tested) |
| FR-019 | Token rotation | 2 | P2-T03 | reports/phase-reports/phase-2.md | Implemented (regenerate + hot-swap, tested) |
| FR-020 | Retention 30d | 3 | P2-T01 + P2-T03 | reports/phase-reports/phase-2.md | Implemented (purge + startup/daily task, tested) |
| FR-021 | Upstream concurrency bound | 4 | Phase 4 epic | TBD | Planned |
| FR-022 | Lifecycle transparency | 1 | P2-T03 (UI gateway state) | reports/phase-reports/phase-2.md | Implemented (state + errors surfaced) |
| FR-023 | Routing scope/agent constraints | 3+ | P3-T03, Phase 8 | TBD | Planned |

NFR evidence: security (P2 gates), performance budget (Phase 11), a11y (Phase 9), cross-platform (ADR-013 CI matrix).
