---
name: execute
description: Use when implementing an existing plan or resuming its unfinished tasks.
---

# Execute

Deliver the assigned plan through verified increments, preserving the orchestrator's task ownership and authorization boundaries.

1. Read the plan, design, acceptance requirements, and relevant source. Check repository status and current branch. Preserve existing edits; use the assigned workspace and do not create competing worktrees or switch branches without a task need.
2. Check prerequisites and wiring against current code. On resume, inspect claimed completed changes and recorded evidence rather than trusting checkmarks alone. Start from the first incomplete dependency.
3. For each owned task, establish the relevant test baseline. Add a failing behavior test when warranted, confirm it fails for the intended reason, and implement the smallest complete change. Follow the repository's existing tools and patterns.
4. Verify the actual code against every task requirement. Run focused tests, required build/lint checks, and representative integration checks. Preserve error paths and public contracts. A passing unit test does not prove native integration or deployment.
5. Review specification compliance, then correctness and maintainability. Resolve material defects and rerun affected checks after changes. Do not mark a task complete from an agent's report without inspecting its artifacts and evidence.
6. Record completed task IDs, changes, decisions, commands/results, and remaining blockers in the assigned plan/checkpoint. Continue successive authorized tasks without waiting for routine batch feedback.
7. At completion, review the integrated diff and execute the final acceptance checks. Report requirement coverage, test results, unresolved limitations, and artifact locations. Commit, publish, or merge only as authorized by the task and orchestrator; a worker should not assume ownership of team-wide commits.

If reality invalidates the plan, document the mismatch and send a focused question to the orchestrator. Continue unaffected tasks. Preserve a handoff before context loss with exact next actions; context pressure does not by itself require the human to restart the session.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
