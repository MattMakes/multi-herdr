---
name: create-plan
description: Use when a design or requirements are settled and need executable implementation tasks with verification.
---

# Create Plan

Translate the accepted design into a plan another worker can execute without reconstructing the architecture.

1. Read the design, constraints, current code, and existing tests. Resolve stale paths and identify the actual build/test commands before assigning work.
2. State the goal, architecture, technologies, scope, and behavioral invariants. Map public interfaces through implementation and registration/call sites; identify regression hotspots with source references.
3. Split work into cohesive deliverable tasks. For each specify ownership paths, dependencies, purpose, inputs/outputs, failure cases, invariants, implementation approach, and exact verification commands with expected results. Provide code sketches only when needed to disambiguate a contract; do not freeze speculative implementations.
4. Include meaningful regression tests before behavior changes, wiring checks for new integrations, and parity checks for migrations. Keep independent work separable and order shared contract changes before their consumers.
5. For services, include local startup and a minimal representative smoke check. Inventory external dependencies, configuration, credential-loading sources, and required certificates/proxies without secret values. Distinguish checks possible locally from checks needing an unavailable service; block only tasks that actually depend on them.
6. Give every acceptance requirement an identifier and a reproducible check plus expected observation. Connect each task to its acceptance requirements so omissions are visible. Include documentation and cleanup needed for the complete result.
7. Save to the assigned path or `ai_docs/plans/YYYY-MM-DD-topic-plan.md`. Finish with verification coverage, dependencies, open decisions, and explicit completion criteria.

Review the plan against the source before implementation. Continue into authorized execution without inserting a new human approval checkpoint; escalate only consequential unresolved scope or contract decisions.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
