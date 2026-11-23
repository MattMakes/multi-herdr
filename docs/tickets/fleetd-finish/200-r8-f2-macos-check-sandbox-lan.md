# R8-F2 (Important): SR-3/SR-4: on macOS a task check can connect to the own LAN address of the host

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-platform --locked sandbox && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p4_verifier && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/mac/sandbox.rs`, `crates/horch-e2e/tests/p4_verifier.rs`, `crates/horch-platform/tests/sandbox.rs`

Source: the end review `docs/specs/fleetd/reviews/end-review-a.md` (task T-297, reviewed tip 08c6d4ff). Read its finding first; it has the file and line of each fact.

## Fault

The check contract says "loopback and Unix sockets only" (`check_sandbox.rs:45`). On macOS, `crates/horch-platform/src/mac/sandbox.rs:73` denies remote IP and allows `localhost:*`; Seatbelt also treats the own LAN address of the host as local. Task T-246 changed `sr3_check_that_connects_off_host_is_refused` to use 192.0.2.1 (the orchestrator ruled it a test-shape fault; the review does not agree, and the review decides). So a check can reach a service that listens on the LAN address of the machine. On Linux (`--unshare-net`) it cannot.

## Goal

On macOS a task check reaches loopback (127.0.0.1, ::1) and Unix sockets only, as on Linux. A test with a live listener on the own LAN address of the host proves the refusal, next to the TEST-NET case.

## Steps

1. Find a Seatbelt profile form that permits only loopback: try each of these and record the result with `sandbox-exec` and a listener on 127.0.0.1, on ::1 and on the LAN address: a rule by `(local ip ...)`, by `(remote ip "localhost:*")` with a deny for the interface, by `(remote unix-socket)` only plus a loopback proxy, and what Apple's profiles for loopback-only tools use. Write the table of results in the commit message.
2. If a form works: write the failing test first (a listener on the LAN address of the host; the connect of the check is refused; loopback still connects), fix `mac/sandbox.rs`, and keep the TEST-NET assert.
3. If no form of Seatbelt can do it: do NOT weaken the text of the rule in code and do NOT mark macOS as unsupported by yourself (that stops each macOS user). Send `QUESTION:` with the table of step 1 and the 3 options for the operator: (a) macOS check sandbox with no network at all (deny each IP; only Unix sockets), with the list of tests of this repo that then fail; (b) keep the present behavior and change the contract text and the design to "this host only"; (c) mark the check sandbox of macOS unsupported. Measure option (a) before you ask.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build or a tool. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`. Each `claude` command of a test or of the product removes it from the environment.
5. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
6. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never run `cargo test -p horch --bin horch` with a filter that matches `watch_once` before task T-287 has landed (it starts a process chain).
7. The host has a high load from another fleet of the operator: never touch its processes. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Other tasks edit the same large files (`pass.rs`, `handlers_worker.rs`, `cmd/fleetd.rs`): change only the functions of your ticket there; `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish` and again when the reviews are in; resolve a conflict in favor of both changes.
9. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the LAN listener test is green 3 times, or the QUESTION of step 3 is sent with its evidence.
