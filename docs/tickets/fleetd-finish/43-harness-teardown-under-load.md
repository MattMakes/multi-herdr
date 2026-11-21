# e2e harness teardown: processes of fleetd are alive 5 s after SIGTERM under load

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --lib --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p3_triage --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/src/harness.rs`

## Goal

3 of 30 runs of `crates/horch-e2e/tests/p6_fr10_approval.rs` fail in teardown (tests `fix_a_*`, `fr10_a_*`, `fr10_b_*`) at a host load average of 18 to 28. The harness check near `crates/horch-e2e/src/harness.rs:672` finds the processes `horch job run <n>` and `horch worker idea-lead-3` alive 5 s after SIGTERM (`REAP_GRACE`). A flaky test is a bug. Find the cause and fix it.

## Read first

- `docs/specs/fleetd/gates/t4-teardown-evidence.md` in your worktree (evidence of task T-4), if it exists.
- `crates/horch-e2e/src/harness.rs`: the teardown, `REAP_GRACE`, and the check for left processes.
- The stop path of a job process and of the worker wrapper: how `horch job run` and `horch worker` handle SIGTERM, and the stop ladder of fleetd in the design (`docs/specs/fleetd/design.md`, search "stop ladder" and "grace").

## Steps

1. Reproduce without host-wide load (other fleets use this host; never start busy loops or extra builds to make load). Make the stop slow in a deterministic way: in a test, give the fake agent or the boot job (`conformance`, `docs_sync`) an injected delay at exit, or stop fleetd while a boot job runs. Task T-55 saw the same leak in `crates/horch-e2e/tests/p3_triage.rs`: the left process was a boot job (conformance or docs_sync) of fleetd that still ran at teardown. Record each failure with the process list.
2. Find the cause. The 2 possible classes: (a) the product: a job process or wrapper does not end on SIGTERM in its design time (it blocks the signal, waits for a child without a limit, or the parent does not send the signal to the group); fix the product and add a unit test of the stop path that does not depend on time (use a condition or a fake clock). (b) the harness: the product ends in its design time, but the harness waits a fixed 5 s that is shorter than the design time of the stop ladder; then make the harness wait for the exit of each process with the design limit of the stop ladder plus a margin, and fail only after that limit. The harness must still fail when a process stays alive, and it must still kill and report each left process.
3. Do not make the test pass by a longer sleep without the reason from the design. Write the design time and the measured times in the commit message.
4. Run the deterministic slow-stop test and the test file 10 times: 0 failures. If the cause is in product code (for example `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`), that file is not in your Own: send `QUESTION:` to the orchestrator with the evidence and the fix you want.

## Out of scope

`fleet_process` and the FR-10 tests themselves (task T-4).

## Done when

The slow-stop test passes, 0 of 10 runs fail, the cause is in the commit message, and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

