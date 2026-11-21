# E1-prep: trial merge of origin/main into the run branch, and the plan for each conflict

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/merge-main-plan.md && grep -q '^## Conflicts' docs/specs/fleetd/gates/merge-main-plan.md && grep -q '^## Changes of main that fleetd code must follow' docs/specs/fleetd/gates/merge-main-plan.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/merge-main-plan.md`

## Goal

The end task E1 (T-221) merges `origin/main` into the run branch `fleet/fleetd-finish`. On 2026-11-21 `origin/main` is 10 commits ahead, and `git merge-tree --write-tree fleet/fleetd-finish origin/main` reports 26 conflicts. E1 is exclusive: no other task runs during it. Make E1 short and safe: do the merge now as a trial, and write the plan. This task lands only the plan file. It does NOT land the merge.

## Steps

1. In your worktree: `git fetch origin main`, then `git log --oneline HEAD..origin/main`. For each commit of main, read its diff and write 1 line: what it changes and why (its PR text if the message names one: `gh pr view <n>`).
2. Trial merge on a scratch branch of your worktree: `git switch -c trial-merge-main` and `git merge --no-ff --no-commit origin/main`. Record each conflicted path (`git diff --name-only --diff-filter=U`).
3. For each conflict: read both sides and write the resolution rule in 1 to 3 lines: which side wins for which hunk and why. The run branch replaced large parts of the old claim-and-spawn model by fleetd; a change of main to code that the run branch deleted needs a decision: (a) the change is not needed in the new code, with the reason; (b) the same fix must be made again in the new code: name the file and function. Do not guess: read the new code path.
4. Resolve each conflict in the trial and build: `scripts/dev-sandbox.sh cargo check --workspace --all-targets --locked`. Record each compile error and its fix. Do not run the test suites (the host has a high load); you can run 1 test binary to prove 1 resolution.
5. Find silent conflicts (no git conflict, but wrong together): for each commit of main, search the run branch for the functions, flags, file names and CHANGELOG lines that it touches. Examples to check: a new CLI flag of main in a command that the run branch retired; a test of main that uses a helper that the run branch removed; a version or CHANGELOG line (rule of this repo: a merge to main with a CHANGELOG line publishes a release at once, so list each CHANGELOG change with care); a change of `.github/workflows/`.
6. Save the result so that E1 can use it: `git diff fleet/fleetd-finish...HEAD` is not enough for a merge. Write for each conflicted file the final content choice in the plan, and keep the trial as a local branch `keep/fleetd-finish-trial-merge-main` in the main repository (a branch name only; do not push it). Before DONE: `git switch` back to your task branch, so that the task branch holds only the plan file.
7. Write `docs/specs/fleetd/gates/merge-main-plan.md`: the tip of both branches; `## Commits of main` (step 1); `## Conflicts` (1 table row for each file: the rule of step 3 and the state in the trial: resolved and compiles, or open with the question); `## Changes of main that fleetd code must follow` (step 5; write `none found` only with the list of what you searched); `## Compile result of the trial`; `## Open questions for the operator`. Label each claim verified or not verified.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Never push `main`. Never push the trial branch. Never merge a pull request. Do not edit `CHANGELOG.md` on the task branch.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet.
6. Write 1 `horch ledger progress` line each 10 minutes.
7. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and each of the 26 conflicts has its row.
