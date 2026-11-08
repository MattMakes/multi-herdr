# 10: Spec: the 2 agent skills and their workflows

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `test -s docs/specs/hark/skills-and-docs.md`
**Own:** `docs/specs/hark/skills-and-docs.md`

## Goal

`docs/specs/hark/skills-and-docs.md` is a complete behaviour spec of this part of the source. A clean implementer who never sees the source can rebuild it from your spec alone.

**Side: contaminated (spec writer).**

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Source files of this unit

skills/earshot/SKILL.md, skills/real-sound/ (SKILL.md, references/, scripts/, templates/, templates/prompts/)

## Steps

1. Clone the source as `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md` says, into `~/.local/state/hark-cleanroom/src-<your role>`. Check out the pinned commit.
2. Read every file of this unit completely. Read other source files only to understand calls into them.
3. Write the spec. Cover for the tool skill: when an agent uses it and the workflows it teaches. For the sourcing skill: the whole workflow (brief, sourcing real recordings, the rights audit, arrange, mix, master, QA, variants, credits, mux), every script's behaviour and interface, every template's fields, and every role prompt's job and checks (described, not copied). Our names: tool skill `hark`, sourcing skill `sound-sourcing`. Treat the source's licence-audit feature as a usage-rights audit of downloaded sounds.
4. Add these sections: `Interface` (exact), `Behaviour`, `Algorithms` (steps and math), `Constants` (tables of numbers), `Data assets` (format, origin, and REGENERATE / DERIVE / NEEDS-DATA with a fallback), `Errors and edge cases`, `Acceptance tests` (at least 10 concrete cases: input, and the properties of the output), `Dependencies on other units` (by unit name: core-io, listen, timeline, forge-core, recipes-impacts, recipes-motion, bank-kit-sfx, fit, surfaces, skills-and-docs), `Open questions`.
5. Re-read your spec against the rules. Remove every copied sentence, code line and docstring.
6. Commit only `docs/specs/hark/skills-and-docs.md`. Run `horch ledger finish`, then `horch ledger done`.

## Constraints

- Never run code from the source. Never commit anything from the source.
- Do not shorten for length. Meticulous and complete is the goal. If the spec passes 60 KB, split it into `docs/specs/hark/skills-and-docs.md` (index) and `docs/specs/hark/skills-and-docs-<part>.md`, and list the parts in the index. Own covers only the index; name extra parts in your report.
- Do not delete your clone. The orchestrator may send you a spec question later.

## Done when

- The spec has every section of step 4 and covers every file of this unit.
- Your report lists each NEEDS-DATA asset and each open question.

