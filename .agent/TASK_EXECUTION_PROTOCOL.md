# Task Execution Protocol

Task template:
- ID / parent feature / title
- objective, scope, out-of-scope
- dependencies and owner
- files/components and interface contract
- acceptance criteria and tests
- risks and status (QUEUED/IN_PROGRESS/PASS/FAIL/BLOCKED)
- evidence

A task must produce one coherent reviewable result. Split tasks spanning unrelated layers or too many subsystems. Verify dependencies, assign file ownership, implement minimally, run tests, inspect diff, update docs, then mark PASS only with evidence.

Never let agents concurrently edit the same files without explicit coordination. Use branches/worktrees if available. Main orchestrator integrates and verifies all work.
