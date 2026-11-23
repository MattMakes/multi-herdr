# 81: Fix R6-F2: probe outside-sandbox write access to the request folder in operator_reach (SR-8)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test p7_operator_boundary && scripts/dev-sandbox.sh cargo test -p horch-core --test p7_conformance && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`, `crates/horch-core/tests/p7_operator_boundary.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p7.md` (finding R6-F2) and `docs/specs/fleetd/design.md` (rule SR-8).

## Goal

Before launching the `OperatorBoundary` conformance check, `operator_reach` in `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs` proves that the runner process itself can create and remove an exclusive probe file in `operator.requests` outside the worker sandbox. An existing but unwritable `run/requests` directory fails the precondition with `"proves nothing"` instead of allowing an inside-sandbox write failure to masquerade as a sandbox refusal.

## Requirements

- **R6-F2 (Important — SR-8)**:
  - **Finding text and file:line**:
    SR-8 requires the suite to prove that each protected Operator act works outside the sandbox and is refused inside it. `operator_reach` opens the token and connects to the channel, but it only checks `operator.requests.is_dir()` for `WriteRequests` at `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs:357`. It never proves that the runner can write the folder.
    Concrete case: host permissions make `run/requests` unwritable while a rendered sandbox accidentally permits the path. The fake agent's write fails, and the check reports a sandbox refusal. An Operator later repairs the folder permissions, and the healthy worker can write Operator requests. The smallest fix is an outside-sandbox exclusive create and removal of a unique harmless probe file before launch, plus a test with an existing but unwritable request folder.
  - **Design rule**:
    - **SR-8** (`docs/specs/fleetd/design.md:182`): "fleetd shall authorize each request at the server by the class of its kind. An `operator` request shall carry the operator token from `/fleet/run/operator.token` (mode 0600), and fleetd shall refuse one without a valid token. The sandbox of each worker shall deny reading the token file, connecting to the command channel and writing into the request folder, and profile validation shall refuse a profile whose sandbox allows 1 of them."

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

1. Write the failing test first in `crates/horch-core/tests/p7_operator_boundary.rs`:
   - Add test function `operator_boundary_fails_when_the_request_folder_is_not_writable`:
     - Create a `Machine::new(Codex::WithNetworkProxy)`.
     - Change the permissions of `m.operator_paths().requests` to read-only (`0o500`) while keeping it an existing directory.
     - Run `let r = m.check(&profile(HarnessKind::Claude));` (and restore `0o700` on `m.operator_paths().requests` afterwards so `TempDir` cleanup succeeds).
     - Assert `assert_verdict(&r, Verdict::Fail);`, `assert!(r.evidence.contains(&Act::WriteRequests.to_string()), "{r:?}");`, and `assert!(r.evidence.contains("proves nothing"), "{r:?}");`, plus `m.assert_clean();`.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test p7_operator_boundary -- operator_boundary_fails_when_the_request_folder_is_not_writable` and see it fail (`Verdict::Pass` instead of `Verdict::Fail`) because `operator_reach` currently checks only `operator.requests.is_dir()`.
2. Update `operator_reach` in `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs:344-368`:
   - For `Act::WriteRequests`, verify that `operator.requests.is_dir()`, and then perform an outside-sandbox exclusive create (`std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)`) and immediate removal (`std::fs::remove_file(&probe)`) of a unique harmless probe file inside `operator.requests` (for example `.conformance-reach-<uuid>` or `.reach-<pid>-<uuid>`, which is a dotfile and not a `.json` request file, and is removed before `operator_reach` returns).
   - Map any error creating or removing the probe file to `Err(e.to_string())` for `Act::WriteRequests`, so `operator_reach` reports the unwritable request folder and skips the launch with `"proves nothing"`.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Changing request file ingestion in `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_files.rs`.

## Done when

`operator_boundary_fails_when_the_request_folder_is_not_writable` in `crates/horch-core/tests/p7_operator_boundary.rs` passes, `operator_reach` leaves no probe file behind in `operator.requests`, and the ticket Check passes.
