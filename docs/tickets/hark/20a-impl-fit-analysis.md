# 20a: Build hark fit analysis models

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests/fit_analysis -q`
**Own:** `skills/hark/engine/hark/fit/analysis/`, `skills/hark/engine/tests/fit_analysis/`

## Goal

The `fit` part of hark works as `docs/specs/hark/fit.md` and `docs/specs/hark/ARCHITECTURE.md` say, with a pytest suite in `skills/hark/engine/tests/fit_analysis/` that covers every acceptance test of the spec.

**Side: clean (implementer).** Never open the source. Read only the specs, the clean-room rules and the repo.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Your part

Build only the analysis stages of the fit spec in `hark/fit/analysis/`: sinusoidal, modal, spectral, space (room) and gesture analysis, each with the interface ARCHITECTURE.md fixes. Another worker builds the pipeline (model choice, quality measure, code generation) in `hark/fit/` outside `analysis/`, in parallel, against the same interfaces.

## Steps

1. Read `docs/specs/hark/ARCHITECTURE.md` completely, then `docs/specs/hark/fit.md` and its parts.
2. Build the modules of your Own paths with the exact signatures of ARCHITECTURE.md. Do not edit another unit's paths. If you need a change in a shared contract, send a QUESTION to the orchestrator and end your turn.
3. The generated module must import and render through the forge engine contract. Test on synthetic recordings (a decaying modal strike, a noise whoosh, a tonal chirp) with the quality targets of the spec.
4. Write pytest cases for every acceptance test of the spec. Use `tmp_path`, never a fixed `/tmp` path.
5. Run the check with the hark venv: `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests/fit_analysis -q`.
6. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Constraints

- Python 3.11+. Type hints on public functions. Small functions: the finish step runs a complexity sweep.
- Base deps only, except code behind `hark.models.require_ml`.
- Never set or use ANTHROPIC_API_KEY.

## Done when

- The check passes. Every acceptance test of the spec has a test, or the report says why not.
- The report lists any spec gap you filled with your own judgement.

