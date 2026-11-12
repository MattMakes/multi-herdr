# 38: Run the gate of the run

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet`, fallback `opus` · **Parallel:** no
**Kind:** report
**Review:** none
**Check:** none
**Own:** nothing
**Do not touch:** each file of the repository

## GOAL

`horch ledger run gate` records a passing gate on the tip of the run branch `fleet/fleetd-design`.

## CONTEXT

- This run wrote documents only: a design in `docs/specs/fleetd/`, a DDD model in `ddd/` and tickets in `docs/tickets/fleetd-design/`. It changed no source code.
- Each other task of the run is done and landed. This task is the last one.
- The gate runs 1 check command on the tip of the run branch. The operator can then open the pull request, and the orchestrator can close the run.
- This task changes no file. A commit would move the tip, and the gate would then be stale.

## STEPS

1. Run `horch ledger status --run fleetd-design`. Each other task must be done or cancelled. If a task is running, landing or queued, send `BLOCKED:` with the line and end your turn.
2. Run this command from your worktree:
   `horch ledger run gate --run fleetd-design --check "node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs validate ddd && node /Users/MASCOTT/.claude/plugins/cache/skill-marketplace/software/1.0.0/shared/bin/ddd.mjs contracts check ddd && test -s ddd/COVERAGE.md && test -s ddd/review.html && test -s ddd/diagrams/final.html && test -s docs/specs/fleetd/design.md && test -s docs/specs/fleetd/amendments.md && grep -q PASS docs/specs/fleetd/design-review-4.md && test -s docs/tickets/fleetd-design/38-run-gate.md"`
3. Record the complete output of the command in a progress note.
4. If the gate fails, do not repair a file. Send `BLOCKED:` with the output and end your turn.

## CONSTRAINTS

- Do not edit, add or delete a file. Do not commit.
- Do not close the run. Do not open a pull request. Do not push.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The gate command of step 2 reports a pass on the tip of `fleet/fleetd-design`.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the result of the gate, the commit of the tip, and each line of the output that is a warning.
