# The conformance job fails in each e2e world after about 60 s: the fake agy is not conformant, and a pass with no daemon fails the Operator boundary of claude

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test p7_profile_health --test p7_conformance --test p7_operator_boundary --test arch_scan && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p7_conformance_binary --test p0_liveness && cargo fmt --all --check`
**Own:** `crates/horch-e2e/src/bin/fake-antigravity.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`, `crates/horch-core/tests/p7_profile_health.rs`

Source: the note of task T-333 (worker opus-75, 2026-11-21T07:07Z), proved with a scratch test: it builds the world of `fm3_idle_claim_is_caught_in_under_10_min` and runs `horch ledger watch --once` until each conformance job ends. After 68 s, agy and claude fail conformance, codex passes, and the ledger has the drift features F-0002 and F-0003.

## Facts (from `fleet/docs/<cli>/<version>/conformance.json` of that world)

1. agy 1.1.3: `Private home honoured` fails ("the launch uses the home of the operator, <harness>/home"); `Exact MCP set` fails ("horch-conformance of the profile is not listed; listed {}"); `Sandbox boundary` fails ("the outside write: allowed"). The fake agy of `crates/horch-e2e` reads no private home, lists no MCP set, and has no sandbox.
2. claude 2.1.284: `Operator boundary` fails: "outside the sandbox the runner cannot do each act, so a refusal proves nothing: connect to the command channel: No such file or directory (os error 2)". This is the rule of task T-320 (commit c6dede2f): a world with `ledger watch --once` and no fleetd daemon has no command channel socket, so the runner cannot prove the act.
3. A failed conformance adds a drift idea (design: "Docs sync and adapter conformance", step 4) and can mark the profile unhealthy (`mark_after_conformance`, `pass.rs` and `worker_profiles/application/health.rs`). An unhealthy profile gets no dispatch. On a fast CI runner the job ends inside the test; on this loaded host it is abandoned or still runs. So CI and local runs differ, and tests where a wait times out only in CI can have this cause (CI run 4: the Linux tests of ticket 223, some tests of `taskledger.rs`).

## Decide and fix (2 parts)

Part A, the claude row: a product question. The runner of T-320 is right that a refusal proves nothing when the act cannot be done outside the sandbox. But a missing command channel is a fact about fleetd (it is not up, or the pass runs with no daemon), not about the profile. Read the design (FR-10, SR-8, the conformance section, and what a conformance result may do to the health of a profile). Then choose and implement: (1) the conformance job of a CLI is not started by a pass that has no command channel (the job needs a live fleetd by its meaning), or (2) the check gives a third result "not proved: <reason>" that does not make a drift idea and does not change the health, and the job runs again on a later pass. With either: a profile is never marked unhealthy, and no drift idea is made, because fleetd itself was not ready; and a profile whose rules really allow the act still fails (the tests of T-320 stay green with no change). Say which callers run a pass with no daemon (`horch ledger watch --once`, tests, the stand-in) and what each does now. `conformance_runner.rs`, `domain/conformance.rs` and `pass.rs` are shared files (not in Own): change only what this part needs.

Part B, the fake agy: the fakes of claude and codex pass conformance; the fake agy does not. Design fact to check first: is Antigravity a supported worker CLI of this release, or held as not supported (search `docs/specs/fleetd/design.md` and the decisions for "Antigravity" and "agy"; the end review `docs/specs/fleetd/reviews/end-review-a.md` says the real agy lacks the sandbox and private-home guarantees)? Then: (1) if the product holds agy as unsupported by design, the conformance job of an unsupported CLI must not make a drift idea in each fleet (1 item for the operator at most, by its key, OR-3), and the harness worlds must not get a new feature from it; (2) if agy is supported, make `fake-antigravity` conformant in the same way as `fake-claude` (private home, the MCP list, the sandbox acts), so that the fake proves the adapter. Do not make a test pass by a switch that turns conformance off for all tests: the tests of conformance must still run it.

## Steps

1. Reproduce: write the scratch test of T-333 as a real test in `crates/horch-e2e/tests/p7_conformance_binary.rs` or `p0_liveness.rs` (shared files): a world with the 3 fake CLIs runs passes until each conformance job ends (use the fleet clock or the job state, no sleep and no wall-clock race); assert: no profile of a fake CLI that is conformant by design is unhealthy, and the ledger has no drift feature. It must fail before the fix.
2. Fix part A, then part B.
3. Run the Check. Then run `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test taskledger` 1 time and report which tests change their result.
4. In the DONE note: list the tests of `docs/specs/fleetd/gates/ci-run-4.md` (macOS and Linux tables) whose first error line can be explained by a profile that was unhealthy or by an extra feature run.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator. No new allow-list entry of `arch_scan`.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. At most 4 test processes at the same time. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
6. Run `cargo clippy --workspace --all-targets --locked -- -D warnings` before each finish (`type_complexity` at `disk.rs:190` is red on the base until task T-327 lands; add no new one).
7. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit: restore each comment that gives a security rule, an invariant, a bound or a design id.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, the test of step 1 is green 3 times, and the commit message states the rule for a conformance check that cannot be proved and the state of agy.

## Added 2026-11-21T07:12Z: 1 more red fitness test in the same file

`fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform` (`crates/horch-core/tests/arch_scan.rs`) fails on 2 lines of `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs` (`:769` and `:776`): task T-320 put an OS `cfg` there for the device and inode of a folder. Rule PL-2: an OS `cfg` of production code lives in `horch-platform`. Move the read of the identity of a folder to a function of `horch-platform` (Unix: device and inode; Windows: the volume serial number and the file index, or `None` with the meaning "no identity", which the verdict already reads as "not visible", never as a pass). The runner calls that function. No new allow-list entry. The test must be green before your finish.

## Added 2026-11-21T07:46Z: remove 1 filter that task T-337 added

Task T-337 gave `crates/horch-e2e/tests/p5_operator_boundary.rs` a state helper that leaves out the rows of the drift ideas (the boot conformance run of agy failed and fleetd added an idea, a feature, a run and a sub-orchestrator while the test ran). A security test must compare the full state. When your fix stops that drift idea, remove the filter (the file is a shared file; change only that helper and its callers; if T-337 did not land yet when you finish, send `NOTE:` and the orchestrator makes a small task for it) and run the test 3 times. Tasks T-333 and T-335 list more tests that your fix turns green with no change: read their commit messages on the run branch and run those tests 1 time each: the `triage_*` tests of `i308_fleet_steward.rs` and `g50_legacy_orchestrator_and_host_load_are_advisory`.
