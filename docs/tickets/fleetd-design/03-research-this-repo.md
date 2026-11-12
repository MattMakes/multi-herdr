# 03: Research: same-file contention in this repository's own history

<!-- horch-ticket: 1 -->
**Persona:** primary `researcher`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/research-03-this-repo.md`
**Own:** `docs/specs/fleetd/research-03-this-repo.md`

## GOAL

The file `docs/specs/fleetd/research-03-this-repo.md` holds measured facts about how often fleet tasks in this repository changed the same file, what that cost, and how `horch` handles it today.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/research-context.md` first. It gives the system, the operator's position, the options A to E and the rules for the findings file.
- Ticket 01 and ticket 02 research the outside world. Do not use the web here. Measure this repository.
- Today `horch` serializes 2 tasks whose **Own** paths overlap. The ticket format is in `docs/specs/ticket-format.md`. The ledger code is in `crates/horch-core/src/taskledger/`.
- The ledger is a SQLite database. Find its path with `horch ledger --help` and the code. Read it only. Never write to it.
- The orchestrator uses your file to make 1 design decision. A wrong number causes a wrong design.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file named above.
3. Measure question Q1, hot files. For the commits on `main` since 2026-09-01, list the 30 files with the most commits. Give for each: the count of commits, the lines added plus deleted, and the current line count. Give the exact `git log` command.
4. Measure question Q2, how the code handles overlap today. Name the function that decides that 2 tasks overlap on **Own** paths. Name the function that lands a task on the run branch, and say if it merges or rebases. Name what occurs on a conflict at landing. Give `file:line` for each.
5. Measure question Q3, the cost of serialization. From the ledger, for each of the last 10 runs with 3 or more tasks: the count of tasks, the count of tasks that were held because of an **Own** overlap, and the total time those tasks were held. If the ledger does not record this, write "I could not determine" and say which record is absent.
6. Measure question Q4, conflicts that occurred. From the ledger events and `git log --merges`, count landings that failed or needed a conflict resolution. For each one that you find, give the run, the file and how it was resolved.
7. Measure question Q5, test tiers. Name the unit test command and the E2E test command of this repository. Read `docs/testing-and-gates.md`, `justfile` and `.github/workflows/`. Give the recorded run time of each tier from the newest CI run logs that you can read. Do not run the full test suite.
8. Measure question Q6, structure of the hot files. For the 10 hottest Rust files of Q1, give the count of top-level items (`fn`, `struct`, `enum`, `impl`, `mod`) and the count of lines in `#[cfg(test)]` modules. This shows if changes to the file are likely to be in separate items.
9. Write the findings file. Number the findings F1, F2 and so on. Follow the rules in the context file.
10. Run `test -s docs/specs/fleetd/research-03-this-repo.md`. It must exit 0.

## CONSTRAINTS

- Read only. Do not write to the ledger database. Do not change a run, a task or a worktree.
- Do not run `cargo test` for the whole workspace. The machine runs other agents.
- Do not edit a file other than the one in **Own**.
- Do not recommend an option. The orchestrator decides.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of Q1 to Q6 has at least 1 finding with its command and output, or the sentence "I could not determine <X>".
- The file starts with `## Summary` and ends with `## What the facts mean for options A to E`.

## REPORT

Record 1 progress note after each question. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the 3 numbers that bear most on the decision.
