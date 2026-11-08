# 25: Integrate hark end to end and run every acceptance test

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus-domain` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `PYTHONPATH=skills/hark/engine /Users/mascott/.local/state/hark/venv/bin/python -m pytest skills/hark/engine/tests -q -m 'not ml and not live'`
**Own:** `skills/hark/engine/`, `skills/hark/scripts/`

## Goal

All hark areas work together: every CLI command runs end to end, the bank is rebuilt, and the whole suite passes.

**Side: clean.**

## Context

- The operator wants a sound engineer for video SFX and game SFX. The fleet rebuilds an open-source sound tool for coding agents, clean-room, as our own engine `hark`, plus 2 skills and new teammates.
- Read the clean-room rules first: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/CLEANROOM.md`. They are absolute. Your side is named below.
- Read the decisions: `/Users/mascott/projects/multi-herdr/ai_docs/plans/hark/decisions.md`.
- The package is `hark`. Its engine lives at `skills/hark/engine/hark/`. Never use the source's names for our things.

## Steps

1. Run the whole suite. Fix every failure at its cause, in any area.
2. Run every command of the ARCHITECTURE command table on synthetic files: --help, a normal run, --json, --out. Fix crashes and contract gaps.
3. Run the bank build command. Run `hark sfx check` and `hark kit check` on the result.
4. Install the ML extras into the hark venv (CPU torch). Run the `ml`-marked tests and `describe`, `match`, `rank`, `realness`, `timeline` on synthetic files. Record load times.
5. Test the launcher from a fresh shell, and the install prompt in a TTY and in a non-TTY.
6. Write `skills/hark/engine/README.md`: layout, how to run tests, how to rebuild data.
7. Run the check. Commit exact files. Run `horch ledger finish`, then `horch ledger done`.

## Done when

- The check passes, and the ml tests pass with the extras installed. The report lists each command and its result.

