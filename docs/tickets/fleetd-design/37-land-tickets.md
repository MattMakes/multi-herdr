# 37: Put the tickets of this run on the run branch

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Review:** none
**Check:** `test "$(ls docs/tickets/fleetd-design/*.md | wc -l | tr -d ' ')" = "38" && test -z "$(git status --porcelain docs/tickets/fleetd-design)"`
**Own:** `docs/tickets/fleetd-design/`
**Do not touch:** `ddd/`, `docs/specs/`, `crates/`

## GOAL

The 38 ticket files of this run are committed on the run branch in `docs/tickets/fleetd-design/`, each one equal to its source file.

## CONTEXT

- The orchestrator wrote the tickets of this run in the main checkout: `/Users/MASCOTT/projects/mh/multi-herdr/docs/tickets/fleetd-design/`. They are not in git.
- The pull request of this run must contain them.
- Your worktree is on a branch from the run branch. The directory `docs/tickets/fleetd-design/` does not exist there.

## STEPS

1. Run `git remote -v` and `git config user.email`. The remote must be `git@git-alt:MattMakes/multi-herdr.git`. The email must be `m4ttplu5@gmail.com`. If 1 is different, send `BLOCKED:` and end your turn.
2. Run `ls /Users/MASCOTT/projects/mh/multi-herdr/docs/tickets/fleetd-design/*.md | wc -l`. The result must be 38. If it is different, send `BLOCKED:` with the list and end your turn.
3. Run `mkdir -p docs/tickets/fleetd-design`. Copy each of the 38 files into it with `cp`. Do not copy a file that is not a `.md` file.
4. Run `diff -r /Users/MASCOTT/projects/mh/multi-herdr/docs/tickets/fleetd-design docs/tickets/fleetd-design`. It must print nothing.
5. Commit the 38 files with the message "Add the tickets of the fleetd design run".
6. Run the **Check** command of this ticket. It must exit 0.

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
