---
name: tdd
description: Use when adding or changing behavior that needs a regression test, before implementing the change.
---

# Tdd

Prove that a test detects the required behavior before relying on it to validate implementation. Use the project's existing test framework and conventions.

1. Identify one externally observable behavior or invariant from the requirement. For a bug, capture the smallest reliable reproduction; for a refactor, preserve the existing contract.
2. Write a focused test with realistic inputs and a specific outcome. Exercise real application code. Substitute only genuinely external or nondeterministic boundaries; avoid assertions that merely restate the mock setup.
3. Run the focused test and inspect its failure. A syntax/import/setup error is not the intended red result. Fix the fixture or setup until the failure demonstrates the missing behavior. If it already passes, establish whether the requirement is already satisfied or the test fails to exercise it.
4. Implement the smallest complete behavior and rerun the test. Keep the assertion stable unless evidence shows it encoded the wrong requirement; do not weaken tests to accommodate a defect.
5. Refactor only after green, preserving observable behavior. Run the related regression suite and required checks after substantive changes.

Use boundary cases and failure paths that protect the contract, not one test per private method. For asynchronous behavior, wait on observable conditions/events with bounded timeouts instead of arbitrary sleeps. Keep fixtures isolated and verify cleanup where resources or shared state are involved.

If implementation already exists, do not delete another worker's changes to manufacture test-first history. Add a meaningful regression test and, where safe in an isolated copy, show it catches the pre-fix behavior. Report the actual sequence honestly. Low-impact prose or configuration edits may need structural validation instead of new tests; follow the task's acceptance requirements.

Return the test command, intended red failure, green result, and coverage limits. Escalate missing contract decisions without blocking unrelated work.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
