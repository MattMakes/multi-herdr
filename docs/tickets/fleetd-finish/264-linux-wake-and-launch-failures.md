# Linux core: a ledger commit does not wake a pass in 15 s, and an attempt is not live in 120 s (2 tests of CI run 5)

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p1_pass --test lifecycle && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p1_pass.rs`, `crates/horch-e2e/tests/lifecycle.rs`

## Fault

CI run 5 (`docs/specs/fleetd/gates/ci-run-5.md` on the run branch, section Linux), red on Linux and green on macOS: (A) `rr4_file_request_and_job_events_wake_pass_before_period` (`crates/horch-e2e/tests/p1_pass.rs:106`): a ledger commit did not wake a pass of fleetd in 15 s. (B) `arc_26_e2e_lifecycle_matrix_claude` (`crates/horch-e2e/tests/lifecycle.rs:209`): the attempt is not live in 120 s. A is the wake path of fleetd (RR-4): on Linux the watch of the ledger file can miss a commit (inotify on a WAL file, a watch on a path that a rename replaced, a watch that is set after the first write).

## Steps

1. Reproduce each in a container: image `t-git241-linux:local` exists (`docker run --rm --name t413-lx-<n> --privileged --init`, the repo mounted read only and copied inside, its own `target` in the container). 10 runs each. Docker rules: never stop, restart, remove or prune a container, image or volume that you did not make; never `docker ... prune`; remove by exact name only what you made.
2. Find the cause in the product (the Linux file watch of `horch-platform`, the wake of the pass, the launch path of a claude attempt). A longer wait in the test is not a fix.
3. Fix the product. Files outside your Own: send `QUESTION:` with the list and continue with the analysis. No OS `cfg` outside `horch-platform`.
4. Proof: 10 of 10 green runs of each test in the container, and green on macOS.

## Rules

1. Work only in the worktree that `horch ledger claim` gives; it keeps its own `target/`. Run each `cargo test` on macOS through `scripts/dev-sandbox.sh`. Never install a tool or the build on the host.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never run pkill or killall. Never touch a process of another fleet.
4. Write 1 `horch ledger progress` line each 5 minutes inside each wait loop. Commit after each step.
5. `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish`. After the comment sweep, run finish again at once; restore no comment.
6. Before DONE: no process and no container of the unit is alive; the sandbox folder is removed.

## Done when

The Check passes, and the commit message names each cause with the 10-run proof on Linux.
