# Land the work of task T-89 (CI FIX-8: the stale tests of p2_leases, p2_release, p3_refusals, p3_release, p3_unlanded, p3_worktrees) with a check limit of 3600 s

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p2_leases --test p2_release --test p3_refusals --test p3_release --test p3_unlanded --test p3_worktrees --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/expected-failures/p3_refusals.txt`, `crates/horch-e2e/expected-failures/p3_unlanded.txt`, `crates/horch-e2e/expected-failures/p3_worktrees.txt`, `crates/horch-e2e/tests/p2_leases.rs`, `crates/horch-e2e/tests/p2_release.rs`, `crates/horch-e2e/tests/p3_refusals.rs`, `crates/horch-e2e/tests/p3_release.rs`, `crates/horch-e2e/tests/p3_unlanded.rs`, `crates/horch-e2e/tests/p3_worktrees.rs`

## Goal

Task T-89 migrated 36 stale tests of 6 e2e files. Its Check needs more than its limit of 1200 s (`p3_worktrees` alone takes 570 to 790 s). The local branch `keep/fleetd-finish-T89` (tip 5f65a972) holds the work, rebased on the run branch of 2026-11-20T23:00Z. This task lands it. Many tasks wait for it: do no other work.

## Steps

1. In your worktree: `git merge --ff-only keep/fleetd-finish-T89`. If that fails because the run branch moved: `git reset --hard keep/fleetd-finish-T89`, then `git rebase fleet/fleetd-finish`. Resolve a conflict in favor of both changes.
2. `git diff --name-only fleet/fleetd-finish...HEAD` must show only the 9 Own files. If it shows another file (for example `jobs.rs` or `check_sandbox.rs`), restore that file from `fleet/fleetd-finish` and commit.
3. Run the Check 1 time (it takes 20 to 30 minutes; write 1 `horch ledger progress` line each 5 minutes). The worker of T-89 did not run the 6 files on this exact tip.
4. A red test: read the commit messages of the branch first (they hold the cause of each earlier failure). A test that fails by load or time is not accepted as a flake: find the cause. If the cause is a change that landed after the branch was made (for example T-171: a fleet process starts no fleetd; T-172: the wrapper is recorded before the agent starts), fix the test setup and keep each assert.
5. Run `horch ledger finish`. The comment sweep must not change a file outside Own: after the sweep, do step 2 again before the second finish run.

## Marks that stay (do not remove them; other tasks own them)

4 tests have `#[ignore = "flaky until T-199 ..."]` (owner now: T-208). 4 tests are on expected-failures lists (owners: T-208, T-204, T-207, T-207).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`.
3. Never install the build. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Do not weaken an assert. No new `#[ignore]`. No new expected-failures entry without an answer of the orchestrator.
6. Never run `rm -rf /tmp/horch-e2e-*`. Remove only the exact folder of your own test.
7. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The commits of T-89 are on the run branch and the Check passes.
