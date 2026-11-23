# CI0: run the CI of the run branch on macOS, Linux and Windows now, and record each failure

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/ci-baseline-2026-11.md && grep -q '^## Windows' docs/specs/fleetd/gates/ci-baseline-2026-11.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/ci-baseline-2026-11.md`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (item L12).

## Goal

No Linux or Windows result exists for fleetd. Get the first one now, before the merge of `main`, so the fixes can run in parallel with the landing chain.

## Facts

- `.github/workflows/ci.yml` has the trigger `workflow_dispatch`. The trigger `pull_request` does not run for a branch that has conflicts with `main`.
- The run branch is `fleet/fleetd-finish` on `origin`.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Read `.github/workflows/ci.yml` in your worktree. List its jobs and the OS of each.
2. Confirm the run branch is on the remote: `git ls-remote origin refs/heads/fleet/fleetd-finish`. Record the tip.
3. Start the workflow: `gh workflow run ci.yml --ref fleet/fleetd-finish`. Find the run id: `gh run list --workflow ci.yml --branch fleet/fleetd-finish --limit 1 --json databaseId,headSha,status`.
   - Check: `headSha` equals the tip of step 2.
4. Wait for the run: `gh run watch <id> --exit-status` (it can take more than 30 min; do not start a second run).
5. For each job, record the result. For each failed job, read the log: `gh run view <id> --log-failed`. List each failed test with its first error line.
6. Sort each failure into 1 class: (a) a test that is on `crates/horch-e2e/expected-failures/`; (b) a compile error on that OS; (c) a test that is unix-only by its code but runs on Windows; (d) a real fault of the product on that OS; (e) a fault of the CI setup.
7. Write the report and commit it with the subject `CI0: the first CI result of fleetd on 3 OSes`.

## Report

`docs/specs/fleetd/gates/ci-baseline-2026-11.md`: the run id and URL, the head commit, then `## macOS`, `## Linux`, `## Windows`. Each section: the job result, the count of passed and failed tests, and 1 table row for each failure: test or step, class of step 6, first error line, the file:line of the cause when you can find it. End with `## Fix units`: group the failures into fix units that 1 worker can do, with the files each unit touches.

## Out of scope

- Do not fix a failure. Do not edit `ci.yml`.
- Do not open a pull request. Do not push a branch other than through `horch ledger finish`.

## Done when

The Check passes, and each failed job of the run has its failures in the report.
