# The operator-boundary probe counts "file not found" as "refused": on Linux it passes rules that allow the token read

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test p7_operator_boundary --test p7_conformance && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked conformance && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/p7_operator_boundary.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`, `crates/horch-e2e/src/probe.rs`

Source: the note of task T-309 (worker opus-hardening-18, 2026-11-21T05:22Z), measured in a Linux container on the base commit 2c6ef0b4 and on its own tree: 3 tests of `crates/horch-core/tests/p7_operator_boundary.rs` fail 3 of 3 on Linux and pass on macOS.

## Facts

1. `operator_boundary_fails_when_the_rules_allow_the_token_read` (line 376) and `operator_boundary_fails_when_the_rules_allow_the_channel` (line 385): the assert wants the verdict `Fail`; Linux gives `Pass`. The evidence says "refused" for: read the operator token, connect to the command channel, write into the request folder, and the 3 orphan acts.
2. `codex_operator_boundary_conformance_refuses_unproven_version_and_probes_sandbox_acts` (line 518): `read_token`, `connect_channel` and `write_requests` are "refused" with `No such file or directory (os error 2)`, and `write_outside` is allowed.
3. Reading of the worker (not proved by a run): the fixture is a tempdir under `/tmp`. The Linux sandbox mounts a new empty tmpfs on `/tmp`. Inside the sandbox the token, the socket and the request folder do not exist, so each probe gets ENOENT and counts it as refused; a write "outside" goes into the tmpfs, so it is allowed. CI FIX-18 (commit 8bfd800c) moved the e2e harness root to `/var/tmp` for the same cause.

## Why this is a product fault and not only a test fault

The conformance probe is the proof of FR-10 and SR-8 for a real profile: a worker cannot read the operator token, connect to the command channel as the operator, or write into the request folder. A probe that reads "the file is not there" as "the sandbox refused it" gives the verdict `Pass` without a test of the rules. On a host where the fleet root is under a path that the sandbox hides or replaces (for example `/tmp` on Linux), a profile whose rules ALLOW the token read passes conformance. The 2 tests of fact 1 prove exactly that.

## Goal

1. A probe act counts as refused only when the sandbox refused it (permission denied, operation not permitted, or the refusal error of the platform). "Not found" for a path that exists outside the sandbox is not a refusal: decide by the design and the code which verdict it gives (`Fail`, or a third "not proved" result that marks the profile as not conformant) and say why. The verdict must never be `Pass` when the probe could not see the thing that it tests.
2. The probe proves that it can see its targets: before the acts that must be refused, it checks from inside the sandbox that each target path is visible (for example a stat of the folder that holds it, or a control file next to the token that the rules allow to read). If the target is not visible, the verdict says so in plain words with the path.
3. `write_outside` must test a path that is outside each allowed path AND is real on the host (not a private tmpfs of the sandbox): verify from outside the sandbox that no file appeared.
4. The 3 tests pass on Linux and on macOS with no weaker assert. If the fixture must move out of `/tmp` on Linux (as FIX-18 did), do that too, but only after goal 1: with the old fixture the fixed probe must give a clear "not visible" verdict, and 1 new test pins that.

## Steps

1. Read `conformance.rs` (application and domain), `crates/horch-e2e/src/probe.rs`, the 3 tests, and the design rules FR-10, SR-8 and the conformance section of `docs/specs/fleetd/design.md`.
2. Reproduce on Linux in a container (task T-309 gives the commands in its commit message and DONE note: build the image, `docker run --rm`, exact names; never `docker ... prune`; never touch the containers `litellm-postgres`, `phoenix`, `buzz-prod-*`). Prove or refute fact 3 with 1 run: print the errno of each act and `ls` of the fixture path inside the sandbox.
3. Write the failing tests first: (a) the 3 tests as they are (red on Linux); (b) a new test, on both OS: a probe whose target is not visible in the sandbox does not give `Pass`.
4. Fix the probe, then the fixture. Check the real path too: where is the fleet root of a real install, and is it visible in the Linux worker sandbox? If a real install can have its `run/` folder hidden from the worker by the sandbox, say what the worker then can and cannot do, and send `NOTE:`.
5. macOS: run the Check. Linux: run the 2 test files 3 times in the container.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`. The probe and its tests use fake tokens only: never the operator token of the real fleet.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet.
6. Write 1 `horch ledger progress` line each 5 minutes while a container run is in progress. A container run with no output for 10 minutes hangs: read `docker top <name>`, record it, stop the container by its exact name.
7. Before DONE: no process and no container that the unit started is alive, the image that the unit built is removed, and the sandbox folder is removed.

## Done when

The Check passes on macOS; the 3 tests and test (b) are green 3 times on Linux; the commit message states the verdict rule for "not visible".
