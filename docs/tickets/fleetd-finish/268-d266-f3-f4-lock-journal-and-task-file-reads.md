# Review findings D266-F3 and D266-F4 (Important): the git-lock notice is lost at a crash, and the task-file step reads the store for each attempt

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan --test fleetd_jobs && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked task_execution && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`

Read both findings in full: `docs/specs/fleetd/reviews/delta-review-3-windows-facts-and-landings.md` on the run branch (sections D266-F3 and D266-F4).

## F3

A fleetd git that passes its time limit is killed and can leave a `.lock`; the path goes only into the process-local set `LOCKS_LEFT`; a crash before `Pass::record_locks_left` loses it, so no needs-you item opens after the restart. Fix: write the lock path to a small journal file of fleetd (owner-only, atomic write, under the state folder of the ledger) before the timed git returns its error; at the start and in each pass ingest the journal into needs-you rows (idempotent) and drop an entry only when its lock file is gone. Test: the restart between the kill and the next pass transaction (the test that delta review 2 asked for). `pass.rs` is outside Own: it is allowed for the call of the ingest only.

## F4

`write_with_facts` calls `tasks::get` and `runs::get` for each attempt with no task files (600 statements for 300 attempts), though `facts` read both rows just before. Fix: read the task, run and project-root facts of all missing attempts in set reads on 1 snapshot and render from memory. A statement-count test with 3 and 300 missing attempts gives the same count (`taskledger::testing::count_statements`).

## Rules

1. Work only in the worktree that `horch ledger claim` gives; it keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install a tool or the build.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. No OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`. A pass runs no SQL statement for each row and never waits for a job. Row before effect.
4. Never run pkill or killall. Never touch a process of another fleet.
5. Write 1 `horch ledger progress` line each 5 minutes inside each wait loop. Commit after each step. The host is loaded: run 1 test binary at a time.
6. `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish`. After the comment sweep run finish again at once; restore no comment.
7. A file outside Own that the fix needs: send `QUESTION:` with the list and continue.

## Done when

The Check passes; the commit message names each finding id with its proof.

