# Agent Integrations

Candidate clients: OpenAI Codex CLI, Claude Code, Gemini CLI, OpenCode, Continue, Aider and generic OpenAI-compatible clients. Verify current official docs and actual versions before claiming support.

Each profile records agent ID/version, protocol, endpoint, auth method, model alias, optional fast model alias, environment variables, config file rules, verification date and limitations.

Workflow: select agent; show protocol and required settings; generate copyable config; optionally show diff/backup and ask consent before editing external files; test harmless request; provide rollback instructions.

Do not assume all agents accept custom base URLs or share environment variables. Some require native protocol semantics. Support is not complete until end-to-end tested on the documented version.
