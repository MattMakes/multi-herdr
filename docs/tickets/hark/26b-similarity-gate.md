# 26b: Clean-room similarity gate against the source

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/hark/SIMILARITY.md`
**Own:** `docs/specs/hark/SIMILARITY.md`

## Goal

A measured similarity reading of every hark file against the source, and every file over the threshold is named for a rewrite.

**Side: contaminated (gate).** You may read the source, but you write no code.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Steps

1. Clone the source as `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md` says, into `~/.local/state/hark-cleanroom/src-gate`.
2. Read `/Users/mascott/.claude/skills/unlicense/SKILL.md`. Run its `scripts/similarity.py` for: `skills/hark/engine/` against the source package folder, `skills/hark/` and `skills/sound-sourcing/` against the source skills folder, and `docs/specs/hark/` against the source docs.
3. Pair files by role (for example our `hark/forge/engine/dsp.py` with the source DSP module). Report line-level and token-level similarity per pair.
4. Threshold: a code file over 30 % token similarity, or any 5 or more identical consecutive non-trivial lines, fails. A prose file over 20 % fails.
5. Write `docs/specs/hark/SIMILARITY.md`: the table, the failures, and for each failure the lines (as our-file line ranges only; never paste source text).
6. Commit only that file. Run `horch ledger finish`, then `horch ledger done` with the count of failures.

## Done when

- The table covers every file. The report gives the failure count.

