# 08: skill-intake detects fleet mode on the hc backend

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-bugfix`, fallback `codex-terra` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `python3 -m unittest discover -s scripts/skill-intake/tests`
**Own:** `skills/skill-intake/`, `scripts/skill-intake/`

## Goal

`intake mode` prints `fleet` in a worker pane of the hc backend, with no override variable.

## Context

- Bug, seen on 2026-11-24 in run external-imports: `skill-importer-1` ran in an hc pane. `intake mode` printed `plain`. Plain mode needs subagents, which the fleet forbids.
- Cause: `skills/skill-intake/scripts/intakelib/task.py` line 97 has `FLEET_VARS = ("HORCH_ROLE", "HORCH_RECORD_ID", "HERDR_PANE_ID")`. An hc pane has no `HERDR_PANE_ID`. It has `HC_TERMINAL_ID`, `HC_SCOPE` and `HC_TERMINAL_SOCKET`. The backend name is in `HORCH_BACKEND` (`herdr` or `hc`).
- Other files read `HERDR_PANE_ID` too. Run `grep -rn HERDR_PANE_ID skills/skill-intake scripts/skill-intake`.

## Steps

1. Write a failing test in `scripts/skill-intake/tests/`: with `HORCH_ROLE`, `HORCH_RECORD_ID` and `HC_TERMINAL_ID` set, `HERDR_PANE_ID` unset, and the `SKILL_INTAKE_HORCH` test seam, `mode()` returns `fleet`. Check: the test fails.
2. Change the rule: the pane variable is `HERDR_PANE_ID` or `HC_TERMINAL_ID`. Keep the 2 other variables and the `horch --version` check. Check: the new test passes.
3. Fix each other `HERDR_PANE_ID` use the grep shows, when it has the same defect. Add a test for each.
4. Update the mode rule text in `skills/skill-intake/SKILL.md`, `skills/skill-intake/references/process.md` and the report line in `intakelib/report.py`.
5. Run the Check line.

## Constraints

- Do not change a gate, a severity rule or the override `SKILL_INTAKE_MODE`.
- Add no code comments in lines you add.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

The Check passes, and a test covers the hc case and the herdr case.

## Report

Run `horch ledger finish`, then `horch ledger done`. The summary lists each file changed.
