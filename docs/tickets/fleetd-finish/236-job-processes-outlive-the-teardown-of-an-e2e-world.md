# Job processes of fleetd outlive the teardown of an e2e world by 25 s ("the test leaked N process(es)")

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p0_liveness --test fakes && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan --test gate_jobs && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/src/harness.rs`

## Fault

The third finish Check of task T-336 (the e2e file `taskledger`, 1029 s, host under load) failed 2 tests at `crates/horch-e2e/src/harness.rs:700`: `a_command_means_the_run_it_names_before_the_run_of_its_worker` ("the test leaked 2 process(es) that outlived teardown by 25s", 1 of them `horch job run 2`) and `a_spawn_is_refused_as_retired_and_adds_no_worker` ("the test leaked 1 process(es)", `horch job run 7`). These are jobs that fleetd starts at boot or in a pass (for example the conformance job of each CLI, about 60 s; the docs job; a worktree list). Done criterion 6 of this run: no leftover process. No flaky test is accepted.

## Questions to answer with evidence, then fix

1. Which job kinds are `horch job run 2` and `horch job run 7` in these 2 tests (read the jobs table of a world of each test)? How long do they run alone and at load?
2. What does the teardown of the harness do now: does it stop fleetd, and how (TERM, the stop command, a kill of the group)? What does fleetd do with its running jobs when it stops: the design (`docs/specs/fleetd/design.md`: search for the stop of fleetd, the stop grace of a job, and the adoption of a job at the next start) says if a job must end with fleetd or may live and be adopted by the next fleetd. Name the lines.
3. Decide by the design:
   - If a stopped fleetd must end its jobs (TERM, grace, KILL of each job group): the product has a fault. Fix it in the stop path of fleetd (send `QUESTION:` for the files; task T-319 changes the stop rules of a job in `fleet_runtime/application/jobs.rs` now: read its commits on the run branch or wait for its landing, and use its TERM-first rule and its group identity check).
   - If a job may outlive fleetd by design (the next fleetd adopts it): then a test world must end what it started: the teardown of the harness stops each job of the world by its recorded group (pid and start time from the jobs table of the world, never by name, never a process of another world or fleet), waits for the end within a bound, and only then checks for leaks. The 25 s leak check stays as it is: do not make it longer and do not skip `horch job run`.
4. Proof: a test that starts a world, lets a long job start (a fixture job that sleeps), tears the world down and asserts that no process of the world is alive within the bound. It fails before the fix. Run the 2 named tests 10 times each (at most 4 test processes at the same time).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`, or the ledger of another fleet. Never install the build.
3. A dev fleetd and a test never stop a process that they did not start. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Other fleets run on this host: never touch their processes.
4. No flaky test, no `#[ignore]`, no weaker assert, no longer leak bound, no expected-failures entry.
5. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, restore each comment that gives a safety rule, a bound or a design id.
6. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed.

## Done when

The Check passes, the new test fails before the fix and passes after it, and the 20 runs have 0 leak failures.
