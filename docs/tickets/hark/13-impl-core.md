# 13: Build hark core: audio I/O, features, CLI, models, doctor and launcher

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests/core -q`
**Own:** `skills/hark/engine/hark/core/`, `skills/hark/engine/hark/cli.py`, `skills/hark/engine/hark/__main__.py`, `skills/hark/engine/hark/doctor.py`, `skills/hark/engine/hark/models/`, `skills/hark/engine/tests/core/`, `skills/hark/scripts/hark`

## Goal

The `core` part of hark works as `docs/specs/hark/core-io.md` and `docs/specs/hark/ARCHITECTURE.md` say, with a pytest suite in `skills/hark/engine/tests/core/` that covers every acceptance test of the spec.

**Side: clean (implementer).** Never open the source. Read only the specs, the clean-room rules and the repo.

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Steps

1. Read `docs/specs/hark/ARCHITECTURE.md` completely, then `docs/specs/hark/core-io.md` and its parts.
2. Build the modules of your Own paths with the exact signatures of ARCHITECTURE.md. Do not edit another unit's paths. If you need a change in a shared contract, send a QUESTION to the orchestrator and end your turn.
3. The launcher `skills/hark/scripts/hark` finds its own engine folder from its own path at run time, uses the hark venv python, and offers to create the venv when it is absent (TTY prompt; non-TTY: exit 3 with the command). The CLI registers every command of the ARCHITECTURE command table and imports each command module lazily, so a missing module of another unit gives a clear 'not built yet' error, not a crash.
4. Write pytest cases for every acceptance test of the spec. Use `tmp_path`, never a fixed `/tmp` path.
5. Run the check with the hark venv: `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests/core -q`.
6. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Constraints

- Python 3.11+. Type hints on public functions. Small functions: the finish step runs a complexity sweep.
- Base deps only, except code behind `hark.models.require_ml`.
- Never set or use ANTHROPIC_API_KEY.

## Done when

- The check passes. Every acceptance test of the spec has a test, or the report says why not.
- The report lists any spec gap you filled with your own judgement.

