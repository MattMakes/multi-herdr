# 13: A controller pane spawns its report workers with a brief

<!-- horch-ticket: 1 -->
**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch -p horch-core && python3 -m unittest discover -s scripts/skill-intake/tests`
**Own:** `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch-e2e/tests/`, `skills/skill-intake/scripts/intakelib/cmd_spawn.py`, `skills/skill-intake/scripts/intakelib/roles.py`

## Goal

`intake spawn auditor` starts an `intake-auditor` worker from the `skill-importer` pane, in a ledger run, on the hc backend and on the herdr backend.

## Context

- Bug, seen on 2026-11-25 in run external-imports (horch 0.5.3, backend hc). `skill-importer-1` ran `SKILL_INTAKE_MODE=fleet intake spawn auditor --task <T>`. intake exited 11. horch printed "horch spawn takes no task text: a worker claims its task from the task ledger".
- intake runs `horch spawn intake-auditor --exact "Read and follow <brief> exactly."` (`skills/skill-intake/scripts/intakelib/cmd_spawn.py`, `roles.py`).
- `crates/horch/src/cmd/spawn.rs` function `check_task_text` allows task text only when `controller.is_some()` and the target has `horch_access: report`. `teammates/intake-auditor.md` has `horch_access: report`. `teammates/skill-importer.md` has `spawn_only: ["intake-*"]`.
- So `spawn_only_check` returned no controller: horch did not resolve the calling pane as `skill-importer-1`. Line 119 of `spawn.rs` builds `caller`. An hc pane has `HC_TERMINAL_ID`, `HC_SCOPE` and `HC_TERMINAL_SOCKET`, and no `HERDR_PANE_ID`. The caller was a ledger worker (claimed task T-5), and its Bash ran in a Claude sandbox.
- Ticket 08 of this run already fixed the same class of defect in `intakelib/task.py`.

## Steps

1. Find why `caller` is empty for a ledger worker in an hc pane. Record the root cause with `horch ledger progress`.
2. Write a failing test that resolves the caller from the environment of an hc ledger worker pane. Check: it fails.
3. Fix the caller resolution. Check: the test passes, and the tests `spawn_only_*` in `spawn.rs` pass.
4. Add 1 end-to-end test in `crates/horch-e2e/tests/`: a controller with `spawn_only` spawns a report worker with task text inside a ledger run on the hc backend. Follow `crates/horch-e2e/tests/hc_fleet_start.rs`.
5. When horch refuses the spawn, `intake spawn` must print the whole horch error. Remove the cut of the message in `cmd_spawn.py` or `roles.py`. Add a unit test.
6. Run the Check line.

## Constraints

- Do not weaken `spawn_only`, `--exact` or the task-text refusal for a caller that is not a controller.
- Do not reinstall horch and do not restart a pane.
- Add no code comments in lines you add.
- Never read, print or use `ANTHROPIC_API_KEY`.
- If the root cause is in a file outside Own, send `horch tell orchestrator "QUESTION: ..."` with the path and end your turn.

## Done when

The Check passes, and the DONE summary states the root cause in 1 sentence.

## Report

Run `horch ledger finish`, then `horch ledger done`.
