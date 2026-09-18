---
name: brainstorm
description: Use when refining a feature, rewrite, or refactor into a design before implementation planning.
---

# Brainstorm

Turn the assigned objective into a design grounded in the current project. Use existing decisions and authorization; do not restart discovery questions already answered.

1. Read the brief, relevant requirements, project instructions, entry points, and existing design evidence. Identify the outcome, scope, constraints, and observable success criteria.
2. Select the applicable path. For a new feature, map domain objects and data flow. For a refactor, name the external behavior to preserve and the measurable internal improvement. For a migration, trace the old implementation and collect available runtime request/response, startup, and failure evidence; explicitly label behavior inferred only from code.
3. Compare realistic approaches with their costs, failure modes, and integration impact. Recommend the smallest complete approach that satisfies the brief. Avoid adding speculative features.
4. For existing code, record preservation requirements: validation and errors, public contracts, defaults with domain meaning, concurrency/locking, retries, events, serialization, and cache behavior. Map abstractions to implementations and registration sites. Record credential sources and rotation/certificate requirements without copying secret values.
5. Resolve material unknowns using repository evidence or a focused orchestrator question. An unavailable legacy runtime limits confidence; identify which design decisions depend on that evidence instead of inventing parity.
6. Write the design to the assigned artifact path, or `ai_docs/designs/YYYY-MM-DD-topic-design.md`. Include chosen approach, alternatives, components, data flow, failure handling, preservation table, open questions, and acceptance checks with observable expected results.

Completion means another worker can plan implementation from the design without guessing contracts. Report unresolved decisions and continue already-authorized planning when the design is sufficiently supported.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
