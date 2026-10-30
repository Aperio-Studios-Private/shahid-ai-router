# Sub-Agent Roles

Use only roles needed; four or five agents are not mandatory for every task.

- Researcher: official docs, versions, advisories and compatibility evidence.
- Plan reviewer: independent challenge of assumptions, requirements, security and test coverage.
- Backend engineer: assigned Rust/API/persistence work plus tests.
- Frontend engineer: assigned React/UI/accessibility work; never persist secrets in browser storage.
- QA engineer: scenarios, reproduction, acceptance and regression.
- Security reviewer: threat model, secrets, SSRF, imports, Tauri permissions, dependencies/logging.
- Documentation engineer: synchronized specs, matrix, changelog and reports.
- Orchestrator: ownership, integration, final verification and task state.

Reviewer should not be sole author. If true sub-agents are unavailable, perform a distinct fresh reviewer pass and disclose that it was not an independent agent.
