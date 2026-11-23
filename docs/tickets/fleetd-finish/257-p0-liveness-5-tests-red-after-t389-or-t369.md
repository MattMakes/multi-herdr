# Regression on the run branch: 5 tests of p0_liveness wait 120 s for "the fleetd row to name the new fleetd"

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p0_liveness && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p0_liveness.rs`

## Fault

On base 3acb0212 of `fleet/fleetd-finish` these 5 tests of `crates/horch-e2e/tests/p0_liveness.rs` fail, each after 120 s with "waiting for the fleetd row to name the new fleetd" (proved by task T-384 in a scratch worktree with fresh binaries): `a_reaped_worker_whose_pane_has_no_process_keeps_its_pane_with_1_item`, `a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed`, `an_unreadable_old_record_of_the_polite_stop_is_removed_and_the_pane_is_kept`, `a_pane_that_names_another_process_after_the_polite_stop_is_kept_with_1_item`, `a_fleetd_that_dies_after_the_polite_stop_leaves_the_pane_close_to_the_next_fleetd`. The file passed 9 of 9 before the rebase onto 3d3e9a8a. Suspects: commit 513b73e1 (task T-389: the start of fleetd refuses a git older than 2.41, checked before the fleetd row) and the commits of task T-369 (edf19799 and the 9 before it: the stop of a job and of a task attempt signals only a group whose leader runs with the recorded start; a git of fleetd has a time limit; the hold of the termination handler). The Check of T-389 and of T-369 did not run this file.

## Steps

1. Reproduce 1 of the 5 tests and keep its world. Read the output of the second fleetd of the test (its stderr and log): does it refuse to start (the git check: which git does the world of these tests give, and what does `git version` print there; these tests may start fleetd with a PATH or a context that has no git or a fake git), or does it start and not write its row, or did the first fleetd not end?
2. Prove the cause with `git bisect` between 25660806 and 3acb0212 on 1 test (a bisect step is 1 build and 1 test: at most 4 test processes at the same time), or by a direct proof from step 1.
3. Fix at the cause. If the test world gives fleetd no usable git by design of the test, the fix is in the world (the same way as the other fleetd worlds of the harness get their git), not a weaker check in the product. If the product is wrong (for example the check reads the git of the wrong PATH, SR-4: the git of the context of fleetd; or a stop no longer ends a process that it must end, so the old fleetd or a pane process stays), fix the product: send `QUESTION:` with the file list before you edit a file outside your Own.
4. Run `p0_liveness` 3 times. Then run the other e2e files that start a second fleetd or stop a fleetd (`git grep -ln 'serve_fleetd\|new fleetd' -- crates/horch-e2e/tests`) 1 time each and list each red test with its message; task T-378 runs the whole workspace now: read `docs/specs/fleetd/gates/full-gate-macos-early.md` if it landed.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`. Never install the build. No test touches the launchd label `horch-fleetd`.
3. No flaky test, no `#[ignore]`, no weaker assert, no longer wait, no expected-failures entry.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`.
6. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes 3 times and the commit message names the commit that made the regression.
