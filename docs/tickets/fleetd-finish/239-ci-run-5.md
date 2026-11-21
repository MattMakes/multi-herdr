# CI5: fifth CI run of the run branch (early signal while the last fix tasks run), and the difference to the fourth run

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/ci-run-5.md && grep -q '^## Windows' docs/specs/fleetd/gates/ci-run-5.md && grep -q '^## Difference to run 4' docs/specs/fleetd/gates/ci-run-5.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/ci-run-5.md`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (item L12).

## Goal

The third CI run (report `docs/specs/fleetd/gates/ci-run-3.md`, run 44784709135, head 001e73c2) had macOS 188 failed, Linux 368 failed, release-lint 2 errors and 1 compile error of the Windows tests. Since then the fix units FIX-11 to FIX-25 and about 20 more fix tasks landed. Some fix tasks still run (see `horch ledger status --run fleetd-finish`): a failure that a running or queued task owns is expected; name the task. Run the CI again on the tip of now. List what is still red, and name each failure that has no ticket.

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
7. Compare with the second report `docs/specs/fleetd/gates/ci-run-3.md`: for each failure group of run 3, write fixed, still red, or new. Name each test that is red now and was not red in run 3.
8. For each failure that is still red: name the task that covers it. Read `horch ledger status --run fleetd-finish` for the queued and running tasks, and the tickets in `/Users/matthewscott/repo/multi-herdr/docs/tickets/fleetd-finish/` (`FIX-INDEX-8.md`, `FIX-INDEX-9.md`, tickets 156 to 203). Write `no ticket` when none covers it. Also list each `no ticket` row of `ci-run-3.md` (for example the Linux `p4_repair` hang) with its state now.
9. Write the report and commit it with the subject `CI3: the fourth CI result of fleetd on 3 OSes`.

## Report

`docs/specs/fleetd/gates/ci-run-5.md`: the run id and URL, the head commit, then `## macOS`, `## Linux`, `## Windows`. Each section: the job result, the count of passed and failed tests, and 1 table row for each failure: test or step, class of step 6, first error line, the file:line of the cause when you can find it. Then `## Difference to run 4` (steps 7 and 8). End with `## Fix units` (only failures with no ticket): group the failures into fix units that 1 worker can do, with the files each unit touches.

## Out of scope

- Do not fix a failure. Do not edit `ci.yml`.
- Do not open a pull request. Do not push a branch other than through `horch ledger finish`.

## Done when

The Check passes, and each failed job of the run has its failures in the report.

## Added rules of 2026-11-21

- If the tip of `origin/fleet/fleetd-finish` is behind the local branch `fleet/fleetd-finish`, do not push it yourself: send `QUESTION:` with both commit ids.
- Do not run a local test suite: this task only reads the CI. Write 1 `horch ledger progress` line each 10 minutes while the run is in progress.
- In `## Fix units`, give for each unit the exact `cargo test` command that reproduces 1 failure of it on macOS when that is possible, so that the Check of its ticket is a command that someone ran.

## Added for run 5 (read this first; it replaces what differs above)

1. The run to compare with is run 4: `docs/specs/fleetd/gates/ci-run-4.md` (head 2c6ef0b4: macOS 23 failed, Linux 52 failed, docs 1 rustdoc error, Windows 2666 passed and 813 failed and then a hang of 3 hours). Each FIX-nn of that file has a task that landed since; `ai_docs/plans/fleetd-finish/decisions.md` in the main repository lists them. For each FIX-nn of run 4 say: green now, or still red with the new message.
2. The workflow jobs have `timeout-minutes` now (task T-342). If a job reaches its limit, report the last test that started and do not start the run again without an answer of the orchestrator. If a run hangs past the limit, send `BLOCKED:`; do not run a cancel command that holds any other command.
3. Start the run with `gh workflow run` on the branch `fleet/fleetd-finish` at its tip (write the head commit in the report). Never push to `main`, never merge, never change a pull request. Wait with an `until` loop in the background and 1 `horch ledger progress` line each 10 minutes.
4. Group each failure by cause, with the count, 1 example message, and a proposed owner ticket (FIX-41 and up). Name each test that is new red against run 4 first: those are regressions of the fix wave.
5. Windows: say how many of the 813 are green now, and list the groups that stay (the sandbox for git on Windows, FIX-37, is an open operator decision: count them, do not fix them).
