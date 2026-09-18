---
name: document
description: Use when documenting actual architecture, developer workflows, or behavior changed by completed work.
---

# Document

Write documentation that helps a reader navigate and operate the actual project.

1. Infer audience and scope from the brief, requirements, and changed behavior. Read existing documentation and repository conventions. A missing formal requirements document is not a reason to halt an otherwise clear documentation task.
2. Inspect manifests, entry points, configuration, build/CI scripts, tests, and representative implementations. For feature documentation, focus on affected components and their connections rather than inventorying the entire repository.
3. Explain purpose, architecture, data/control flow, important interfaces, runtime dependencies, configuration sources, startup/build/test commands, and operational constraints as relevant. Document observed workarounds and inconsistencies with evidence; distinguish them from proposed improvements.
4. Cite actual source paths and symbols or line references for significant claims. Link models and APIs to their maintained definition instead of duplicating large schemas. Do not describe intended architecture as already implemented.
5. Update the established documentation location when one exists; otherwise write the assigned artifact or `ai_docs/docs/project.md`. Keep a navigable structure, concrete examples, and enough context for a new contributor to find the responsible code.
6. Validate file links and commands against the current project. Execute safe representative commands where practical and report any that could not run. Check that examples match real defaults, flag syntax, errors, and public behavior after the change.

Conclude with the artifact location and any material gaps. Avoid invented business rationale or unsupported performance claims. Keep secret values out of examples. Route consequential uncertainty through the orchestrator while completing independent sections; documentation work does not require a sequence of interactive human approvals.

## Worker coordination

Follow the assigned brief and repository instructions. Load only the skill content relevant to the current task. For a material ambiguity or missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected task. Continue independent work and block only the dependent action. If messaging is unavailable, record the blocker in the worker result. Do not wait on an interactive human prompt or add routine approval checkpoints when the work is already authorized.
