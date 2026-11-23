# A fleetd that a stand-in start launches inherits the env of the calling process, so a worker can arm HORCH_FAULT

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --bin horch --locked cmd::fleetd && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p0_fleetd_hang --test p1_pass --locked -- --skip scans_of_10000 && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/fleetd.rs`, `crates/horch-e2e/src/harness.rs`

## Goal

Task T-163 found this. Each horch command runs `check_before_command` in `crates/horch/src/cmd/fleetd.rs`, then `run_check`, then `start_fleetd`. In `Mode::StandIn` (the ledger or the fleet root is not the default of the OS user; `fn mode`, about line 186), `start_fleetd` launches a detached fleetd through the launcher with the env of the calling horch process. `scrub_child_env` and `scrub_pane_identity` do not remove `HORCH_FAULT`. So when fleetd is dead, hung or runs an older binary, a horch command of a worker starts the new fleetd with the env of that worker. A worker can then arm each fault point of fleetd (for example `hang-pass`), and can give fleetd each other env variable that fleetd reads. `Mode::Service` uses only the env of the service spec and does not have this fault. Rules: SR-8 (a worker cannot act as the operator), SR-4.

## Read first

- `crates/horch/src/cmd/fleetd.rs`: `mode`, `check_before_command`, `run_check`, `start_fleetd`, `scrub_child_env`, `scrub_pane_identity`.
- Each env variable that fleetd reads: `git grep -n 'env::var' crates/horch-core/src/fleetd crates/horch/src/cmd/fleetd.rs` and the `HORCH_FAULT` reader.
- How the e2e harness arms a fault: `git grep -n HORCH_FAULT crates/horch-e2e`.
- `docs/specs/fleetd/design.md`: the stand-in start, the operator token, FR-10 (who is a fleet process).

## Steps

1. Write the failing test first: a process that the rule `fleet_process` sees as a fleet process (or a test stand-in for it) runs a horch command while no fleetd runs, with `HORCH_FAULT=hang-pass` in its env; the fleetd that starts must not have the fault armed. Name: `a_stand_in_start_from_a_fleet_process_gives_fleetd_no_fault_and_no_caller_env`.
2. Decide by the design and write the reason in the commit message. Possible parts: (a) a stand-in fleetd starts with an env built from an allow list (the variables that the service spec would give: paths, `HOME`, `PATH`, the ledger and fleet root settings), never with the full env of the caller; (b) a fleet process never starts fleetd: its command fails with a reason, and only an operator process or the service manager starts it; (c) `HORCH_FAULT` is read only from a place that a worker cannot write (for example a file in the protected run folder, or only in a build for tests). The e2e tests arm faults through a stand-in start today: keep a path for the tests that a worker cannot use, and change the harness to it.
3. List in the commit message each env variable that fleetd reads and what a worker could do with it before your fix.
4. Run the Check 3 times before finish (not as a loop in the Check).

## Out of scope

`Mode::Service`. The fault points themselves.

## Done when

The new test passes, each e2e test that arms a fault still passes, and the Check passes.

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

