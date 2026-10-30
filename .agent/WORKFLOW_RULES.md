# Autonomous Feature Workflow

State machine:
QUEUED -> DISCOVERY -> PLAN_DRAFT -> PLAN_REVIEW -> TASK_BREAKDOWN -> IMPLEMENTATION -> INTEGRATION -> TESTING -> CODE_AUDIT -> FIXES -> SECURITY_REVIEW -> REGRESSION -> FINAL_REVIEW -> SHIPPED.

Failure returns to the appropriate stage. External blocker => BLOCKED. Never mark shipped while blocked.

1. Discovery: inspect code, tests, git state, docs, current official references and risks.
2. Plan: scope/non-scope, interfaces, files, migrations, dependencies, acceptance criteria, tests and rollback.
3. Independent review: challenge completeness, feasibility, security, compatibility, migration and testability.
4. Task breakdown: small coherent tasks, each with ID, owner, dependencies, files, acceptance criteria and tests.
5. Implementation: one task at a time; parallelize only with stable contracts and non-overlapping ownership.
6. Verification: targeted tests after each task; full feature suite at end.
7. Independent audits: correctness, maintainability, security, performance, accessibility and docs.
8. Fix loop: root-cause fixes, retest and regression. Never hide findings.
9. Ship gate: all criteria pass, no Critical/High findings, docs and traceability updated, report complete.

Use a bounded loop. After three failed attempts at the same root cause, reassess and mark BLOCKED if unresolved.
