# 18: Whole-run gate on fleet/external-imports

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet`, fallback `codex-terra` · **Parallel:** no
**Kind:** report
**Review:** none
**Check:** none

## Goal

A recorded gate result exists for the tip of `fleet/external-imports`.

## Steps

1. Run exactly: `horch ledger run gate --run external-imports --check "./scripts/phase-gate.sh && python3 -I skills/design-system/scripts/test_palette.py"`. Give it a timeout of 10 minutes or more.
2. If the gate is green, go to Report.
3. If the gate is red, read the log path that the command prints. Do not fix a failure. List each failing test or step with its error line.

## Constraints

- Change no file. Commit nothing.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

`horch ledger run gate` printed a result for the current tip.

## Report

Run `horch ledger done`. The summary gives green or red, the tip sha, the log path, and each failure.
