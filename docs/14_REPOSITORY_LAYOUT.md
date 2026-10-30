# Proposed Repository Layout

```text
shahid-ai-router/
├── .agent/
│   ├── AGENT_INSTRUCTIONS.md
│   ├── WORKFLOW_RULES.md
│   ├── TASK_EXECUTION_PROTOCOL.md
│   ├── SUBAGENT_ROLES.md
│   ├── QUALITY_GATES.md
│   ├── SECURITY_RULES.md
│   ├── RESEARCH_PROTOCOL.md
│   └── REPORT_TEMPLATES.md
├── apps/desktop/                 # Tauri + React application
├── crates/
│   ├── app-core/ gateway/ routing/
│   ├── provider-abstractions/ credentials/
│   ├── persistence/ security/ telemetry/ common/
├── providers/
│   ├── openai-compatible/ openrouter/
│   ├── openai/ anthropic/ gemini/
├── docs/
├── tasks/                         # backlog and task state
├── audits/{planning,code,security,quality}/
├── reports/{feature-reports,phase-reports}/
├── tests/{integration,e2e,security,performance}/
├── scripts/
├── Cargo.toml
├── package.json
├── README.md
└── CHANGELOG.md
```

Do not create empty crates just to match the diagram. Introduce modules when a real boundary and test value exist. Keep one consistent provider packaging strategy.
