# 64: Put the new tickets of this run on the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `test "$(ls docs/tickets/fleetd-design/*.md | wc -l | tr -d ' ')" = "65" && test -z "$(git status --porcelain docs/tickets/fleetd-design)"`
**Own:** `docs/tickets/fleetd-design/`
**Do not touch:** `ddd/`, `docs/specs/`, `crates/`

## GOAL

The 65 ticket files of this run are committed on the run branch in `docs/tickets/fleetd-design/`, each one equal to its source file.

## CONTEXT

- The orchestrator wrote the tickets of this run in the main checkout: `/Users/MASCOTT/projects/mh/multi-herdr/docs/tickets/fleetd-design/`.
- The run branch has the first 38 of them. Tickets 39 to 65 are new and are not in git.
- The pull request of this run must contain all 65.
- The connection to GitHub is not always stable. If the push in `horch ledger finish` fails with a network error, run `horch ledger finish` again, up to 3 times.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Run `ls /Users/MASCOTT/projects/mh/multi-herdr/docs/tickets/fleetd-design/*.md | wc -l`. The result must be 65. If it is different, send `BLOCKED:` with the list and end your turn.
3. Copy each of the 65 files into `docs/tickets/fleetd-design/` with `cp`. Do not copy a file that is not a `.md` file.
4. Run `diff -r /Users/MASCOTT/projects/mh/multi-herdr/docs/tickets/fleetd-design docs/tickets/fleetd-design`. It must print nothing.
5. Run `git status --porcelain docs/tickets/fleetd-design`. It must show 27 new files and no changed file. If a file of the first 38 shows as changed, send `BLOCKED:` with its name and end your turn.
6. Commit the new files with the message "Add the tickets 39 to 65 of the fleetd design run".
7. Run the **Check** command of this ticket. It must exit 0.

## CONSTRAINTS

- Do not change the text of a ticket.
- Do not edit a file in the main checkout.
- Do not edit a file other than those in **Own**.
- Do not start a subagent or a nested agent CLI.

## DONE WHEN

- The check passes.
- The `diff -r` of step 4 prints nothing.

## REPORT

Finish with `horch ledger finish`, then `horch ledger done`. In the DONE line, give the count of files and the commit.
