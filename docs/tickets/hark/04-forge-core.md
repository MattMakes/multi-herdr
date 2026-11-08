# 04: Spec: the forge engine (render, DSP, FX, tone, polish, knobs, schema, search, critic, refsets, viz)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `test -s docs/specs/hark/forge-core.md`
**Own:** `docs/specs/hark/forge-core.md`

## Goal

`docs/specs/hark/forge-core.md` is a complete behaviour spec of this part of the source. A clean implementer who never sees the source can rebuild it from your spec alone.

**Side: contaminated (spec writer).**

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Source files of this unit

earshot/forge/__init__.py, dsp.py, fx.py, tone.py, polish.py, knobs.py, schema.py, search.py, critic.py, refsets.py, viz.py, earshot/forge/recipes/__init__.py and _tonal.py, docs/FORGE_V2.md, docs/DESIGN.md, docs/history/FORGE_V2_LOG.md, eval/forge/ (bench.py, blind_v2.py, build_refsets.py, frozen_tone.py, RESULTS.md)

## Steps

1. Clone the source as `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md` says, into `~/.local/state/hark-cleanroom/src-<your role>`. Check out the pinned commit.
2. Read every file of this unit completely. Read other source files only to understand calls into them.
3. Write the spec. Cover the recipe contract (how a recipe declares parameters and renders), every DSP and FX building block with its math, tone helpers, the polish chain, knobs and morphs, the parameter schema, the listen-and-improve search loop and its critic scoring, reference sets, and the visualisation. Give the benchmark method and its target numbers as acceptance tests.
4. Add these sections: `Interface` (exact), `Behaviour`, `Algorithms` (steps and math), `Constants` (tables of numbers), `Data assets` (format, origin, and REGENERATE / DERIVE / NEEDS-DATA with a fallback), `Errors and edge cases`, `Acceptance tests` (at least 10 concrete cases: input, and the properties of the output), `Dependencies on other units` (by unit name: core-io, listen, timeline, forge-core, recipes-impacts, recipes-motion, bank-kit-sfx, fit, surfaces, skills-and-docs), `Open questions`.
5. Re-read your spec against the rules. Remove every copied sentence, code line and docstring.
6. Commit only `docs/specs/hark/forge-core.md`. Run `horch ledger finish`, then `horch ledger done`.

## Constraints

- Never run code from the source. Never commit anything from the source.
- Do not shorten for length. Meticulous and complete is the goal. If the spec passes 60 KB, split it into `docs/specs/hark/forge-core.md` (index) and `docs/specs/hark/forge-core-<part>.md`, and list the parts in the index. Own covers only the index; name extra parts in your report.
- Do not delete your clone. The orchestrator may send you a spec question later.

## Done when

- The spec has every section of step 4 and covers every file of this unit.
- Your report lists each NEEDS-DATA asset and each open question.

