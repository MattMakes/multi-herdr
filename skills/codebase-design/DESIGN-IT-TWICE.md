# Design It Twice

When the task is to explore alternative interfaces for a chosen deepening candidate, use this parallel workers pattern. Based on "Design It Twice" (Ousterhout): your first idea is unlikely to be the best.

Uses the vocabulary in [SKILL.md](SKILL.md): **module**, **interface**, **seam**, **adapter**, **leverage**.

## Process

### 1. Frame the problem space

Before asking for workers, write an explanation of the problem space for the chosen candidate:

- The constraints any new interface would need to satisfy
- The dependencies it would rely on, and which category they fall into (see [DEEPENING.md](DEEPENING.md))
- A rough illustrative code sketch to ground the constraints, not a proposal, just a way to make the constraints concrete

Put this in the `QUESTION:` of Step 2.

### 2. Ask for workers

Send 1 `QUESTION:` to the orchestrator (`horch tell orchestrator`) that asks for 2 or more workers, in parallel. Each must produce a **radically different** interface for the deepened module.

Give each worker a separate technical brief (file paths, coupling details, dependency category from [DEEPENING.md](DEEPENING.md), what sits behind the seam). The brief is independent of the problem-space explanation in Step 1. Give each worker a different design constraint:

- Worker 1: "Minimize the interface: aim for 1–3 entry points max. Maximise leverage per entry point."
- Worker 2: "Maximise flexibility: support many use cases and extension."
- Worker 3: "Optimise for the most common caller: make the default case trivial."
- Worker 4 (if applicable): "Design around ports & adapters for cross-seam dependencies."

Include both [SKILL.md](SKILL.md) vocabulary and GLOSSARY.md vocabulary in the brief so each worker names things consistently with the architecture language and the project's domain language.

Each worker outputs:

1. Interface (types, methods, params, plus invariants, ordering, error modes)
2. Usage example showing how callers use it
3. What the implementation hides behind the seam
4. Dependency strategy and adapters (see [DEEPENING.md](DEEPENING.md))
5. Trade-offs: where leverage is high, where it's thin

If the orchestrator says no, write the designs yourself, 1 after the other, before you compare them.

### 3. Present and compare

Present designs sequentially, then compare them in prose. Contrast by **depth** (leverage at the interface), **locality** (where change concentrates), and **seam placement**.

After comparing, give your own recommendation: which design you think is strongest and why. If elements from different designs would combine well, propose a hybrid. Be opinionated: give a strong read, not a menu.
