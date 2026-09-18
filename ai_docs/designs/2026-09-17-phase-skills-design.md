# Phase-scoped worker skills

The user requested portable repository-owned skills sourced from `~/projects/public-skills`, available natively in Claude Code, Codex, OpenCode, pi and Prime Agent. Keep skill bodies out of startup prompts. Existing branch PR #1 will include this and the unattended Codex startup fix.

Use curated adaptations of the local skills, recording source revision and paths. Normalize harness-specific model, delegation, path and tool assumptions; do not copy plugins wholesale. Research: brainstorm, research-codebase, trace. Plan: create-plan, pre-flight. Implementation: execute, tdd, debug. Validation: check, code-analysis, code-review, security-review. Handoff and document are supporting skills. Skills are selected by a teammate's phase, overridden by `horch spawn --phase`; persist phase across resumes. Additional `skills` names select individual bundled skills.

One per-launch directory materializes embedded assets. Claude receives a generated skills-only plugin; pi/Prime get explicit `--skill` paths even under `--no-skills`; OpenCode gets additive native skill paths in a child-only configuration overlay; Codex gets selected skills in its private home. Never modify shared user config or target project files. Skill discovery remains subject to harness-managed policy. Ambient project/global catalogs are not claimed to be fully isolated.

Alternatives: machine-local references fail portability; injecting all skill bodies wastes context; copying full plugins brings unrelated tools, hooks and dependencies. Native discovery plus curated portable resources meets the requested scope.

## Preservation analysis

| Area | Preserve | Verification |
|---|---|---|
| Behavior | Prompt last, session resume ids, model/effort, fleet messaging and daemon cleanup | Existing launch and golden tests |
| Contract | Existing YAML/brief/ledger records parse with absent phase | Defaulted optional phase, round-trip tests |
| Domain | One top-tier orchestrator, per-launch execpolicy, workspace sandbox | Existing roster and Codex tests |
| Wiring | Spawn -> resolved teammate -> worker/recipe -> native CLI -> skill files | Adapter and CLI tests |
| Credentials | Existing CLI auth/provider sources; no credential copies in skill assets | Child-only overlays, integration inspection |
| Failure modes | Unknown skills/phases fail before launch; unique launch roots; no deletion of shared state | Negative, isolation and cleanup tests |

## Acceptance gates

G1: bundled skills parse, all selected names resolve, references are portable, bodies are loaded on demand.
G2: all five native launch adapters expose selected skills and preserve session/permission arguments.
G3: phase selection persists through fresh/resume workflows with backward-compatible records.
G4: full workspace tests, build and roster validation pass; PR reflects final diff and limitations.
