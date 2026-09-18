---
name: debug
description: Use when a test, build, runtime behavior, or integration fails unexpectedly and the cause is unknown.
---

# Debug

Find the cause before changing behavior. Keep a short record of observations and hypotheses so failed experiments are not repeated.

1. Read the complete error and relevant stack/log context. Reproduce with exact inputs, commands, environment, and expected versus actual behavior. If intermittent, gather occurrences and distinguish failing from successful runs.
2. Inspect recent changes and compare a working path or configuration. Follow the bad value backward through callers to its origin. At each component boundary inspect inputs, outputs, state, and configuration propagation; redact secrets rather than dumping the environment.
3. State one falsifiable hypothesis with supporting evidence. Use the smallest controlled experiment that changes one variable. Observe the result before adding another change; remove temporary instrumentation when no longer useful.
4. Once confirmed, capture the failure with a focused regression test or reproducible check. Fix the source of the invalid state, preserving the intended contract. Add validation at additional boundaries only when they enforce distinct invariants, not as speculative masking.
5. Rerun the original reproduction, relevant tests, and necessary integration checks. For timing failures, prefer event/condition synchronization to inflated sleeps. Confirm cleanup, retry bounds, and error visibility where relevant.

After repeated unsuccessful fixes, stop stacking patches. Reassess the hypothesis, shared state, and architecture; send the orchestrator the evidence, failed attempts, and the specific decision or access required. Continue investigations that do not depend on that answer.

If the evidence points to an external or environmental issue, state what is known and what remains unproven. Add scoped handling only when authorized and supported by the observed failure. Report root cause, fix, regression evidence, and remaining uncertainty; do not claim a fix merely because the code changed.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
