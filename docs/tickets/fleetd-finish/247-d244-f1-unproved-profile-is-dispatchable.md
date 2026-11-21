# Review finding D244-F1 (Critical): a profile whose first conformance is "not proved" gets dispatch

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test p7_profile_health --test p7_conformance --test p7_operator_boundary --test arch_scan && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p7_conformance_binary --test p0_liveness --test p5_operator_boundary && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/application/health.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`, `crates/horch-core/tests/p7_profile_health.rs`

Source: `docs/specs/fleetd/reviews/delta-review-custody-and-conformance.md` on the run branch. Read the finding first.

## Fault

Task T-347 added the verdict "not proved" for a conformance check that fleetd cannot prove when no channel server exists. `health()` (`worker_profiles/application/health.rs:83`) reads a profile with no health event as healthy, and `mark_after_conformance()` writes nothing for a report with "not proved" and no failed check. So a new CLI version or adapter that no conformance run ever proved gets dispatch: the boundary of SR-8 and FR-8 is not fail-closed.

## Step 1: facts first (send them as a `NOTE:` before you change code)

1. The design: read `docs/specs/fleetd/design.md` for conformance (FR-8, SR-8, FM-19, A37, the docs job and the conformance job). Which line says when a profile may get dispatch: after a passing run for its current CLI version and adapter hash, or until a run fails? Quote it.
2. The order at a start of fleetd: when does the channel server start, when does the first conformance job of each CLI run, and how long does 1 run take (task T-333 measured about 60 s in an e2e world)? Can the job be queued only when the channel server is up, so that "not proved" does not occur in a normal start at all?
3. The size of the change in tests: with a hold of dispatch until a passing run, how many e2e worlds wait for a conformance run before their first dispatch, and how long? Does each fake CLI of the harness (`fake-claude`, `fake-codex`, the others; agy is unsupported by design A37) pass conformance when a daemon with a channel runs? Measure with 1 world.

## Step 2: fix

Goal: a profile gets dispatch only when the last complete conformance report for its current CLI version and adapter hash passed (the proof). Missing proof and "not proved" hold dispatch for that profile and show the cause in the holds of a task. A failed check marks it unhealthy as today. The proof survives a restart of fleetd (it is a row; the row before the effect). The pass reads the proofs of all profiles in a set read. A profile with no proof must not stop the fleet silently: the first conformance job is queued when the channel server is up, and the hold text names the run that is awaited.

If step 1 shows that the hold makes most e2e tests wait about 60 s more or go red, do not weaken the rule and do not add a test-only switch that the product can also read: send `QUESTION:` with the numbers and options (for example: a faster conformance for a fake CLI through the fixture of the harness; a proof that the harness writes as a fixture row through a test API of the crate, never through an environment variable of the product).

Tests: a first report with "not proved" holds the profile, and a later passing report releases it; a new CLI version or adapter hash holds again until its proof; a restart keeps the proof; the hold text. Each fails before the fix.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`, or the ledger of another fleet. Never install the build.
3. A pass never waits for a job and runs no SQL statement for each row; a row is written before its effect; no OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert, no expected-failures entry.
4. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet. At most 4 test processes at the same time. 1 `horch ledger progress` line each 5 minutes while a long test runs.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each comment that gives a security rule, an invariant, a bound or a design id.
6. Before DONE: no process of the unit is alive (`ps`), and the sandbox folder is removed.

## Done when

The Check passes, the facts of step 1 are in the commit message, and each new test fails before the fix.
