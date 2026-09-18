---
name: code-review
description: Use when reviewing a diff, branch, or completed feature for correctness, maintainability, and regression risk.
---

# Code Review

Review the assigned change in context and lead with actionable findings.

1. Establish the requested base and head, staged diff, or working-tree scope. Inspect repository status and the actual diff. Use a verified default branch or merge base when needed; do not silently review an arbitrary branch.
2. Read requirements and affected code beyond changed lines: callers, consumers, tests, configuration, and existing patterns. Summarize what behavior the diff changes and where it enters the running system.
3. Assess correctness, code quality, user experience, performance, robustness, and documentation in proportion to the change. Look for missing edge/error paths, incompatible contracts, unintended state changes, resource leaks, duplicate logic, and misleading tests. Distinguish measurable defects from stylistic preference.
4. Reproduce suspected problems with targeted checks when feasible. Verify that an alleged issue is introduced or exposed by this change and identify the concrete trigger and consequence. Do not report a hypothetical issue without a plausible path through the code.
5. Present findings by severity, each with a concise title, current `file:line`, trigger, impact, and suggested correction. Separate confirmed findings from questions and checks that could not run. Include strengths or grades only if useful or requested; a grade is not a substitute for evidence.
6. Save the assigned review artifact or `ai_docs/reviews/feature-review.md`, recording revision/scope and commands/results. If no material findings remain, say so explicitly while naming validation limits.

Keep review-only assignments read-only apart from the report. Send findings to the orchestrator rather than expanding scope into unsolicited refactoring. When revisions are requested, verify the final fix against the original finding and repeat only affected checks.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
