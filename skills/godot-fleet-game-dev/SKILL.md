---
name: godot-fleet-game-dev
description: >-
  Habits that let many agent sessions build one Godot game at the same time without
  breaking each other: one git worktree per plan unit, a fresh import, a baseline, small
  committed steps, merges of a moving base, a finish gate, and a clean land. It also covers
  the shared-machine rules (a slot runner for headless Godot, mkdir locks for the window,
  the timing machine and the main checkout), scope questions, dependency waits, compaction
  handoffs, long runs, the steward who lands branches, the balance lead, and the owner who
  decides gameplay changes. Use it whenever a fleet, an orchestrator, a worker, a steward
  or several parallel agents work on a game repo, or someone asks how to split a feature
  into parallel units, why a worktree fails its tests after a merge, how to land a branch
  safely, how to stop agents from overloading one machine, or how a worker should wait or
  ask, even if nobody says "fleet".
---

# Fleet Game Dev

Target engine: **Godot 4.7**. SKILL.md holds no code. The reference shell blocks are POSIX `sh`,
and the Python block uses the standard library only.

A game built by many agent sessions breaks in ways a solo project never sees. Two workers
edit the same row, a fresh worktree fails 40 tests because its import cache is empty, 20
headless runs load the machine while the human plays, a question gets lost and a worker
idles for hours, and a DONE claim hides a red suite. The method gives each plan unit its
own worktree and a fixed loop: import, baseline, small steps, merge, finish gate, land. It
puts every shared resource behind one lock shape and every decision behind one message
shape. It works because each failure above has one owner and one check, so a fresh session
can follow the loop and land clean work without the history.

This skill is the Godot and game side of the fleet. The herdr fleet tool `horch`, your
worker briefing and the `orchestrate` skill give the generic worker and orchestrator
mechanics. Read those for the `horch` commands. Read this skill for what a game fleet adds.


## When to use

- You are a worker that gets 1 plan unit of a Godot game and must land it on a moving base.
- You orchestrate or lead a wave of parallel units on 1 game repo.
- You land other workers' branches as a steward.
- Several agents share 1 machine, 1 display or 1 checkout, and runs or locks collide.
- A worker must wait for a land, a GO, a lock or an answer, or must hand off before compaction.

## When not to use

- A single agent works alone on a branch. Use `godot-test-loop` for the test loop instead.
- The question is how to write the test, the bot or the seam. This skill only says when to run it.
- The fleet tool itself fails to install or start. Use the herdr setup docs.

## Words used here

[references/words.md](references/words.md) defines unit, base branch, orchestrator, steward,
lead, owner, parse check, visual gate and the message keywords.

## Method: the unit lifecycle

1. **Read the plan, the wave common file, the fleet rules and the contract in full.** Read
   a named skill only when its step comes. Check: you can name every file in your FILES
   list and every do-not-touch file.
2. **Make the worktree and branch from the base.** Use the fleet's worktree command. Never
   switch branches in the main checkout, because other sessions work there. Check:
   `git -C <worktree> branch --show-current` names your unit branch.
3. **Import before you read code or run tests.** A fresh worktree has no `.godot` cache, so
   class names do not resolve. Copy the cache from a working worktree if the rules allow,
   then run 1 headless import with a time limit. Check: the import log has no `ERROR` line.
4. **Record a baseline.** Run the full suite, or the folders you will touch, and every
   balance model or data generator. Save the outputs as "before". Check: you list every
   failure that exists before your first edit, so nobody blames you for it later.
5. **Build each plan step with its tests.** For a fix, write the test first and see it fail
   for the expected reason. Edit with an asserted script or the editor tool (see
   [Asserted scripted edits](references/merges-edits-and-claims.md#asserted-scripted-edits)). Check: the filtered tests pass.
6. **Commit each step after its filtered tests pass.** Commit exact files, not folders.
   Commit the `.uid` files of your own new scripts with the scripts. Leave other units'
   `.uid` files out. Check: `git status --short` shows no stray file of yours.
7. **Send a note at each milestone with counts.** A note is for a future session: files,
   decisions, dead ends. For a gap or a file outside FILES, see
   [Scope questions and blocks](references/waits-and-handoffs.md#scope-questions-and-blocks).
8. **Merge the base when a relevant peer lands and before the final checks.** Read the
   incoming diff first for files that change your measures. Import again after the merge,
   then run the suite. See [Merges in a worktree](references/merges-edits-and-claims.md#merges-in-a-worktree). Check: no
   "Identifier not declared" or "Compilation failed" line.
9. **Run the finish gate.** Import, the project parse check, the full suite, the visual gate
   if you touched anything on screen, every generator with a clean `git status`, and a diff
   that lists only your files. The `godot-test-loop` skill owns the gate. Check: the gate
   output and the counts go into your notes.
10. **List the files of your own commits, not the diff against the base.** A branch that
    landed first shows up in a plain diff. Check: every file is in FILES or approved.
11. **Check that no Godot process of yours runs.** Match by binary and worktree path, not by
    name, because a name match finds your own shell. Stop only your own processes.
12. **Write the notes file.** Changes, checks with counts, gotchas for the next unit, and
    every claim checked against a log (see [Verify claims](references/merges-edits-and-claims.md#verify-claims-before-you-report)).
13. **Report and land.** Send READY or DONE with the commit hash, the test line and the
    changed files. Land only if the plan allows it. If the plan says "never merge" and the
    fleet rules say "land your work", ask which rule applies. Correct a wrong hash at once.

When a failure appears only after a merge, test the base alone in a detached worktree.
That proves whether the merge or the base causes it.

## Waits, shared machine, merges and owner decisions

These sections moved to reference files.

- **Scope questions and blocks**, **Dependency waits**, **Phase-gated delivery**,
  **Compaction handoff**, **Long command waits**: [references/waits-and-handoffs.md](references/waits-and-handoffs.md).
- **Shared machine: slot runner and load**, **Resource locks**, **macOS shell**,
  **Sandboxed workers**: [references/shared-machine.md](references/shared-machine.md).
- **Merges in a worktree**, **Asserted scripted edits**, **Verify claims before you
  report**, **Orchestration CLI drift**: [references/merges-edits-and-claims.md](references/merges-edits-and-claims.md).
- **Gameplay changes go to the owner**: [references/gameplay-changes.md](references/gameplay-changes.md).
- **Lead, steward and orchestrator duties**: [references/fleet-roles.md](references/fleet-roles.md).

## Pitfalls

The most common faults are below. [references/pitfalls.md](references/pitfalls.md) holds
the full table.

| Symptom | Cause | Fix |
| --- | --- | --- |
| 30 or more UI tests fail with "Identifier not declared" or "Compilation failed". | Fresh worktree or a merge brought new class names; the cache is stale. | Import after every merge and in every fresh worktree. |
| `godot --check-only` says "Identifier not found" for an autoload. | `--check-only` does not load autoloads. | Use the project parse check or the suite. |
| Another worker's lock disappeared; 2 timing runs overlapped. | `mkdir L && run; rm L` ran the `rm` after a failed `mkdir`. | Release only inside the success branch, after an owner check. |
| Load average 50+ while the owner plays. | Unbounded runs; hung windows. | Slot runner, `nice`, 1 process per worker, runs that end by themselves. |
| 1 call waited about 10 hours. | The slot wait sat inside or outside the alarm. | Alarm on the Godot command only; check the runner after a long gap. |
| A worker idled for hours. | The question went only to the ledger, or the message failed. | Tell plus a notes-file copy. |
| A land brought 2 later commits with red tests. | The steward merged a branch name. | Merge the exact hash only. |
| A `sed` for 1 value changed another unit's row. | Pattern matched a shared value. | Asserted edit on the unique row id. |

## Artifacts to build

- **Fleet rules file**: 1 rule set per worker type (commit or not, land or not, locks, slot
  runner, message keywords). Plans link it; they do not restate it.
- **Wave common file**: the owner's request verbatim, shared rules, the contract, the file
  ownership map, shared files with their 2 owners and lines, the steward conflict rules.
- **Unit plan template**: GOAL, CONTEXT, FILES own and do-not-touch, STEPS each with a check,
  CONSTRAINTS, DONE WHEN, REPORT.
- **Owner check script**: reads the plans and fails when a file has 0 or 2 owners. Keep it
  in the repo, not in `/tmp`.
- **Slot runner** ([script-slot-run.md](references/script-slot-run.md); copy the block into
  `slot-run.sh`, run with `sh`): `mkdir` slot pool, owner file, stale-pid cleanup, `nice`.
- **Lock helper** ([script-with-lock.md](references/script-with-lock.md); copy the block
  into `with-lock.sh`, run with `sh`): take-wait-run-release in the if-mkdir shape.
- **Asserted edit helper** ([script-rep.md](references/script-rep.md); copy the block into
  `rep.py`, run with `python3 -I`): pairs of old and new text, each must match once.
- **Unit notes file**: changes, checks with counts, QUESTION and answer blocks with dates,
  gotchas for the next unit.
- **Handoff file**: the shape in [Compaction handoff](references/waits-and-handoffs.md#compaction-handoff).
- **Steward land log**: 1 entry per land with hash, merge commit, conflicts, counts, gate,
  copies and strays.
- **Decision file**: numbered gameplay items with evidence, options, recommendation and the
  owner's exact words.
- **Message shapes**: role tag, keyword, 1 outcome sentence, details. `COMMIT REQUEST`,
  `MERGE NOTICE n`, `GATE OPEN`, `READY-2A`, `INT-READY` for the special flows.

## Related skills

- `godot-test-loop`: the test loop and the unit finish gate that step 9 runs.
- `godot-headless-runs`: Godot flags, import and parse hygiene, tool save isolation; this
  skill only gives the slot and load rules.
- `godot-visual-verification`: the windowed gate and the capture queue inside a window lock.
- `godot-bot-balance-loop`: the tables a balance wave measures and proposes from.
- `godot-upgrade-tree-design`: the design side of the gameplay-change rule.
- `godot-feature-seams-and-saves`: what a seam looks like when a scope question asks for one.
- `godot-safe-refactor`: review units and fix waves that a steward lands.
- `orchestrate`: the generic herdr orchestrator loop; `horch` gives the fleet CLI.

## References

- [references/words.md](references/words.md): the fleet words.
- [references/waits-and-handoffs.md](references/waits-and-handoffs.md): before a question, a wait or a compaction.
- [references/shared-machine.md](references/shared-machine.md): before a headless run, a lock or a sandbox.
- [references/merges-edits-and-claims.md](references/merges-edits-and-claims.md): before a merge, a scripted edit or a report.
- [references/gameplay-changes.md](references/gameplay-changes.md): when a change touches a mechanic or a cue.
- [references/fleet-roles.md](references/fleet-roles.md): to plan a wave, lead, land, push or run 2 fleets.
- [references/pitfalls.md](references/pitfalls.md): when a fault is not in the short table.
- [references/script-slot-run.md](references/script-slot-run.md): the slot runner script.
- [references/script-with-lock.md](references/script-with-lock.md): the lock helper script.
- [references/script-rep.md](references/script-rep.md): the asserted edit script.
