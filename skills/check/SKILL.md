---
name: check
description: Use when verifying completed implementation against its plan, requirements, tests, and build.
---

# Check

Establish whether the delivered behavior satisfies the assignment using direct evidence.

1. Read the complete plan and original requirements, including accepted changes. Inventory the promised components, contracts, error cases, integrations, and acceptance checks.
2. Locate and read each implementation and its relevant callers/tests. Map every requirement to source and evidence; classify it as matching, intentional deviation, missing, failed, or unverified. Check actual registration and runtime use rather than the existence of a definition alone.
3. Run the prescribed test and build commands in the correct workspace. Capture commands, exit codes, pass/fail counts where available, and relevant output. Include runtime smoke checks when acceptance depends on real integration. Distinguish a mock test from a live boundary check.
4. Inspect boundary inputs, failure/cancellation paths, backward compatibility, configuration preservation, and resource cleanup relevant to the change. Check that tests demonstrate these behaviors rather than just mirroring implementation details.
5. Compare final changes with the scope. Note unexpected changes and unfulfilled documentation or migration work. Do not silently redefine acceptance criteria to match the implementation.
6. Write the assigned report or `ai_docs/reports/YYYY-MM-DD-implementation-check.md` with a requirement/evidence/status table, deviations, missing pieces, test/build results, and remaining risks. Conclude verified complete or list exact outstanding requirements.

Run checks after the final relevant change; prior successful output remains evidence only for the state it tested. A blocked or unavailable check must remain unverified, never be counted as passing. In review-only work report findings without edits. If fixes are already authorized, correct defects within ownership and rerun affected checks; otherwise route findings to the orchestrator.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
