# 05: Preservation: contract surface and fixed values that fleetd must keep

<!-- horch-ticket: 1 -->
**Persona:** primary `researcher`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/preservation-05-contracts-and-values.md`
**Own:** `docs/specs/fleetd/preservation-05-contracts-and-values.md`

## GOAL

The file `docs/specs/fleetd/preservation-05-contracts-and-values.md` lists each external contract of `horch` that a consumer depends on, and each fixed value in the code that has a meaning, with what the design does with it.

## CONTEXT

- Read `/Users/MASCOTT/projects/mh/multi-herdr/ai_docs/plans/fleetd-design/preservation-context.md` first. It gives the situation and the rules for the findings file.
- Ticket 04 lists behavior. Ticket 06 lists the wiring of the agent CLIs and the platform code. Do not list them here.
- The consumers of `horch` are: the agents (they read prompts and type `horch` commands), the teammate files in `teammates/`, the skills in `skills/`, the operator, the release pipeline in `.github/workflows/`, and other users who install `horch` with `horch self-update`.
- The prompts are built in `crates/horch-core/src/prompts.rs`. `crates/horch-core/tests/golden_prompts.rs` and `crates/horch-core/tests/oracles/` pin them.
- The ledger schema is in `crates/horch-core/src/taskledger/schema.rs`.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Read the context file, the decisions file and the design.
3. List the CLI contract. Run `horch --help` and the `--help` of each subcommand group. For each command that a prompt, a teammate file, a skill or a doc tells an agent or the operator to run, give: the command, who runs it, where it is named, and the exit codes that a caller reads.
4. List the message contract: the form of a mail line, the keywords (`ready`, `DONE:`, `BLOCKED:`, `NOTE:`, `QUESTION:`), the mail id, the redelivery suffix, the channel event form, and the `UNLANDED:` and `UNREGISTERED:` parts of a DONE line.
5. List the file contracts: the ticket format, the map format, the teammate frontmatter fields, the skill bundle layout, the handoff file paths, the evidence log paths, and each file under `~/.horch/` and `~/.local/state/horch/` with its writer and its reader.
6. List the ledger schema contract: the current version, how a migration runs, each table, and what reads the database outside `horch` (search the repo and `scripts/`).
7. List the environment contract: each environment variable that `horch` reads or sets (`HORCH_*` and others). Give the name and the meaning only. Never print a value.
8. List the fixed values: each number or string in the control plane code that sets a time limit, a count limit, a threshold or a ratio. Search for durations, `const`, and the numbers named in the orchestrator briefing (for example 600 s, 20 min, 7 days, 1.5x pace, 3 attempts, 10 min respawn window, context thresholds). Give: the value, the location, why this value (from a comment, a spec or a commit), and the impact if it changes.
9. For each list write a table with a last column `Design`, as the context file says.
10. Write the `## Summary` section as the context file says.
11. Run `test -s docs/specs/fleetd/preservation-05-contracts-and-values.md`. It must exit 0.

## CONSTRAINTS

- Never read, print or use `ANTHROPIC_API_KEY`. Never print the value of an environment variable.
- Read only. Do not edit code. Do not change the ledger, a run, a task or a worktree.
- Do not edit a file other than the one in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- Each of steps 3 to 8 has a table with at least 3 rows, or the sentence "I could not determine <X>".
- Each row has a location and a `Design` value.
- The file starts with `## Summary`.

## REPORT

Record 1 progress note after each of steps 3 to 8. Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the count of rows for each `Design` value and the 3 most important `NOT MENTIONED` rows.
