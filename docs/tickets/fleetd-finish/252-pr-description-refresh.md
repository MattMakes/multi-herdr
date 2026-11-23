# Refresh the draft of the pull request description for the changes since tip 99bf2a9f

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/pr-description.md && grep -q '^## Changes since the first draft' docs/specs/fleetd/gates/pr-description.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/pr-description.md`

## Why

The run ends with 1 draft pull request to `main` whose description explains each change in detail. The draft `docs/specs/fleetd/gates/pr-description.md` (432 lines) describes tip 99bf2a9f. Since then a fix wave landed (CI run 4 fixes, 3 delta reviews and their fixes). The description must be true for the tip.

## Steps

1. Read the draft. List the commits since it: `git log --reverse --format='%h %s' 99bf2a9f..HEAD` (skip the comment sweep commits; read the body of each fix commit: `git log 99bf2a9f..HEAD --format='%h %s%n%b'`). Write the tip that you describe at the top of the file.
2. Add a section `## Changes since the first draft` grouped by subject, each item with what changed for a user or an operator, why (the fault), the task number and the commit: the stop of a job (TERM first, own groups, the orphan group notice, the git writer wait); signal safety (the hold of the termination handler, a stop only of a group whose leader has the recorded start); the folder lock; the ledger (set readers on 1 snapshot, the read-only record for each connection); conformance (the verdict "not proved", agy makes no drift idea, the hold of a "not proved" profile if it landed); worktree custody (states landing and landed, removal of released and dropped trees if task T-289 landed); the question resume; Windows (verbatim paths, CRLF, the detached child, CI time limits); the test harness (1 folder for each agent shell, the teardown stops the jobs of its world); host needs (git 2.41 and its check if it landed, setsid on Linux); new limits that are documented.
3. Correct each sentence of the older sections that the new commits made false (expected failures that are gone, numbers of tests, schema version numbers, lists of open faults). Do not delete the history of a decision; correct the state.
4. Update the sections of verification: each claim names its evidence file under `docs/specs/fleetd/gates/` or `reviews/` and says "verified" or "not verified". CI run 5 and the final gates are not done: say so; leave a line with the words "to fill at the end" for the final tip, the CI links and the gate results.
5. Keep the facts that a reader of the pull request needs first at the top: what fleetd is, what changes for a user of `horch fleet`, what is removed, what the operator must decide (point to `docs/specs/fleetd/gates/operator-decisions.md`, which another task writes now), how to test.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. This task reads and writes text: `cargo fmt --all --check` is the only cargo command.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `/fleet`.
3. Do not edit code, `docs/specs/fleetd/design.md`, a pull request or its description on GitHub. No push other than through `horch ledger finish`. No name of a person and no email address in the file. No attribution line.
4. Label each claim "verified" with its evidence (a commit, a file and line, a report file) or "not verified". Write in Simplified Technical English: short sentences, 1 instruction or fact in each.

## Done when

The Check passes and no sentence of the file is false for the tip that it names.
