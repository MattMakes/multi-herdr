# Integrated worker skills

All but one of these repo-owned bundles are curated adaptations of the local public-skills collection, upstream [MattMakes/skill-marketplace](https://github.com/MattMakes/skill-marketplace) at revision `d47670328c59a3311a9b4149bc5f8f33f0a92754`. They are deliberately maintained copies, not links to a developer machine and not byte-for-byte upstream mirrors. [provenance.json](provenance.json) records the exact source path and SHA-256 of each original SKILL.md, and a null source for the one bundle that has no upstream.

Each folder contains a self-contained Agent Skills entrypoint with a name, a targeted discovery description, and the complete adapted workflow. Only the selected phase's fleet-owned catalog is materialized; harnesses may also discover ambient skills. Workers load relevant bodies on demand. None requires runtime downloads, another skill, an upstream script, a particular model, or a particular harness tool API.

## Deliberate adaptations

- Preserve research evidence, behavioral preservation, dependency/wiring checks, verified implementation, regression testing, and evidence-based review.
- Replace repeated human approval gates and UI questions with focused orchestrator messages. Existing task authorization remains authoritative; only actions dependent on an unanswered decision are blocked.
- Remove mandatory nested agent fan-out, provider-specific commands, machine paths, bundled-script assumptions, repetitive persuasion, and fixed response rituals. Workers use the tools actually available in their harness.
- `execute` adapts `execute-plan`; `check` adapts `check-implementation`; `code-review` adapts `review-git-changes`; `handoff` adapts `whats-next`; `document` adapts `document-project`.
- `code-analysis` adapts `complexity-sweep` by using project-native analyzers. The upstream JavaScript engine, its 14-metric implementation, config generator, plugins, and dependency installation are intentionally not bundled. Available measurements are reported honestly; an unavailable analyzer gets an explicitly qualitative review, not fabricated scores.
- `security-review` adapts `security-sweep` into a direct audit workflow with trust-boundary mapping, deduplication, false-positive verification, and evidence reports. It does not claim to ship the upstream scanner scripts, language catalogs, report assembler, or hierarchical agent pipeline.
- `orchestrate` has no upstream. It is original to this repository, reconciled from the retired external `herdr-orchestrator` and `herdr-worker` skills against the real `horch` CLI: every command, flag and lifecycle claim in those two files was checked against `horch --help` and either carried, corrected or dropped. The reconciliation table is in [ai_docs/reports/bake-in-orchestration-inventory.md](../ai_docs/reports/bake-in-orchestration-inventory.md). It is attached to `orchestrator` and `orchestrator-codex` by name and belongs to no phase.
- `tdd` retains red/green/refactor for meaningful behavior changes and removes unrelated skill-authoring machinery and destructive instructions to delete existing work. Structural checks are appropriate for low-impact prose/config changes.

## Source mapping

| Bundle | Upstream source |
| --- | --- |
| [brainstorm](brainstorm/SKILL.md) | `plugins/dev/skills/brainstorm/SKILL.md` |
| [research-codebase](research-codebase/SKILL.md) | `plugins/dev/skills/research-codebase/SKILL.md` |
| [trace](trace/SKILL.md) | `plugins/dev/skills/trace/SKILL.md` |
| [create-plan](create-plan/SKILL.md) | `plugins/dev/skills/create-plan/SKILL.md` |
| [pre-flight](pre-flight/SKILL.md) | `plugins/tools/skills/pre-flight/SKILL.md` |
| [execute](execute/SKILL.md) | `plugins/dev/skills/execute-plan/SKILL.md` |
| [tdd](tdd/SKILL.md) | `plugins/dev/skills/tdd/SKILL.md` |
| [debug](debug/SKILL.md) | `plugins/dev/skills/debug/SKILL.md` |
| [check](check/SKILL.md) | `plugins/dev/skills/check-implementation/SKILL.md` |
| [code-analysis](code-analysis/SKILL.md) | `plugins/code/skills/complexity-sweep/SKILL.md` |
| [code-review](code-review/SKILL.md) | `plugins/dev/skills/review-git-changes/SKILL.md` |
| [security-review](security-review/SKILL.md) | `plugins/code/skills/security-sweep/SKILL.md` |
| [handoff](handoff/SKILL.md) | `plugins/dev/skills/whats-next/SKILL.md` |
| [document](document/SKILL.md) | `plugins/dev/skills/document-project/SKILL.md` |
| [orchestrate](orchestrate/SKILL.md) | *(none - repo-original)* |
