---
name: research-codebase
description: Use when answering how an existing codebase works or locating its components and relationships.
---

# Research Codebase

Document the system as it exists. Keep proposed improvements separate and include them only when the assignment requests them.

1. Restate the research question and scope from the brief. Read directly referenced files and relevant repository instructions before broad searching.
2. Locate entry points, configuration, tests, and similar implementations using symbol navigation when available, otherwise targeted file and text search. Narrow searches as evidence identifies the owning components.
3. Follow actual definitions and registrations to explain behavior. Check callers, downstream dependencies, configuration precedence, and error paths. Do not infer implementation from names or rely solely on older research notes.
4. Capture each substantial claim with a current `file:line` reference. Distinguish observed runtime behavior, code-derived behavior, and unanswered questions. If external documentation is needed, use authoritative sources and preserve the supporting URL.
5. Synthesize connections across components rather than listing search hits. Include the trigger, relevant data flow, state changes, and where responsibilities cross module boundaries.
6. Write to the assigned artifact path or `ai_docs/research/YYYY-MM-DD-topic.md`. Record scope, revision examined, concise answer, component map, detailed findings, evidence, and remaining unknowns. Use commit permalinks only when the referenced content exists at that commit; otherwise retain local file references.

Return the direct answer and artifact location. Append follow-up findings to the same document when they extend the question. This is a read-only code investigation: documentation output does not authorize production changes.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
