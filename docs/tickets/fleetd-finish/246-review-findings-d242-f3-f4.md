# Review findings D242-F3 (Important) and D242-F4 (Minor): the Windows detached child fails closed, and 1 invariant comment of RR-13 comes back

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc && scripts/dev-sandbox.sh cargo test -p horch-platform --locked && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/windows/group.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_custody.rs`

Source: `docs/specs/fleetd/reviews/delta-review-jobs-and-signals.md` on the run branch, findings D242-F3 and D242-F4. Read them first.

## D242-F3

`spawn_without_inherited_stdio` in `crates/horch-platform/src/windows/group.rs` (about lines 74 to 84, task T-342) reads a failed `GetHandleInformation` as "does not inherit", ignores a failed `SetHandleInformation` at the clear, and ignores each failure at the restore. So on a failure the detached child still inherits the pipe and the caller of `horch tell` waits until that child ends (the hang of CI run 4), or the parent keeps a changed flag.

Fix: fail closed. Before the spawn, a failed query or clear of a valid standard handle gives an error and restores each handle that was already changed; no spawn. After the spawn, each handle is restored also when 1 restore fails, and a restore failure is reported (the child is already running: return the child and log the error through the way that the crate uses, or return an error type that carries the child; choose and say why). A handle that is not valid (no console, `NULL` or `INVALID_HANDLE_VALUE`) is not a failure. Tests: put the 3 system calls behind a small trait or function pointers so that a test can make the query, the clear and the restore fail; the tests compile for Windows and run in CI there (this host cannot run them: say so). Tasks T-369 and T-382 are open: T-369 adds 1 function to this file (a TERM of a child group); rebase on it when it lands and keep both.

## D242-F4

The comment sweep 0069a171 of task T-254 removed the 6-line comment above `refuse_moved_head` in `handlers_custody.rs` (about line 311) that gives the RR-13 invariant: a commit made after the HEAD check in the transaction is not lost, because the rescue reads HEAD again and the removal accepts only the parent of the rescue commit. Restore it (text from `git show 0069a171`); check that it is still true against the code of today (task T-289 edits this file: `git fetch` and rebase first), and correct the words if the code moved.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install a tool (no Windows SDK, no cargo-xwin).
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`. Never install the build.
3. Never run pkill or killall. Never touch a process of another fleet.
4. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, restore each SAFETY comment and each comment that gives an invariant.
5. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes and the commit message names D242-F3 and D242-F4.
