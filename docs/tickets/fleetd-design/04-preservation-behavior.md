# 04: Preservation: behavior of the control plane that fleetd must keep

<!-- horch-ticket: 1 -->
**Persona:** primary `researcher`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/preservation-04-behavior.md`
**Own:** `docs/specs/fleetd/preservation-04-behavior.md`

## GOAL

The file `docs/specs/fleetd/preservation-04-behavior.md` lists each behavioral invariant and each failure-handling path of the current control plane, with its location, its reason, the risk if it is lost, and what the design does with it.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/preservation-context.md` first. It gives the situation and the rules for the findings file.
- Ticket 05 lists the contract surface and the fixed values. Do not list CLI output formats or thresholds here, only name them when a behavior depends on them.
- Ticket 06 lists the wiring of the agent CLIs and the platform code. Do not list them here.
- The control plane code is in `crates/horch-core/src/taskledger/`, `crates/horch-core/src/runtime/`, `crates/horch-core/src/vcs/`, `crates/horch-core/src/messaging/`, `crates/horch-core/src/compaction/` and `crates/horch/src/cmd/`.
- The ADRs are in `docs/adr/`. The specs are in `docs/specs/`. `docs/architecture.md` and `docs/command-flow.md` describe the flows.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file, the decisions file and the design.
3. List the invariants of the task lifecycle: queue, claim, attempts, hold after the attempt limit, requeue, park, cancel, fallback persona, handoff, reopen and dismiss.
4. List the invariants of the finish steps: check, code-sweep, complexity-sweep, recheck, land, pr, cleanup. Include the order, what each step refuses, and the land lock and retry.
5. List the invariants of worktrees: creation, the states (cleaned, archived, trashed, scratch, parked), triage, the trash refs, and what stops a run from closing.
6. List the invariants of supervision: the watchdog, checkins, stall and death detection, stop by recorded PID only, the orchestrator respawn limits, and the rule that a worker with the usage-limit message is never reaped.
7. List the invariants of messaging and questions: delivery, acknowledgement, redelivery, open questions, waits, and what closes a question.
8. List the invariants of runs: 1 active non-solo run per workspace, solo runs, the run gate, run close and its refusals.
9. List the invariants of context control: thresholds, the compaction request cycle and the handoff route.
10. For each row write the table columns: `#`, `Behavior`, `Location`, `Reason`, `Risk if lost`, `Design`.
11. Add a table `Failure modes`: `Risk`, `Likelihood`, `Impact`, `Mitigation today`, `Design`. Use the failures that the current code handles on purpose.
12. Write the `## Summary` section as the context file says.
13. Run `test -s docs/specs/fleetd/preservation-04-behavior.md`. It must exit 0.

## CONSTRAINTS

- Read only. Do not edit code. Do not change the ledger, a run, a task or a worktree.
- Do not run `cargo test` for the whole workspace. The machine runs other agents.
- Do not edit a file other than the one in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of steps 3 to 9 has at least 3 rows, or the sentence "I could not determine <X>".
- Each row has a `file:line` and a `Design` value.
- The file starts with `## Summary`.

## REPORT

Record 1 progress note after each of steps 3 to 9. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the count of rows for each `Design` value and the 3 most important `NOT MENTIONED` rows.
