# The operator learns that git is too old only when the first rescue fails: check the git version in horch doctor and at the start of fleetd

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked rescue_git && scripts/dev-sandbox.sh cargo test -p horch --locked doctor && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/doctor.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/rescue_git.rs`

Source: `docs/specs/fleetd/gates/linux-git-241-proof.md` on the run branch, section "Where the product checks the git version", gaps G1 to G4. Read it first.

## Fault

The rescue job of fleetd needs git 2.41 or newer (`--attr-source`; `rescue_git.rs` `has_attr_source`, about line 318). On a host with an older git (Debian bookworm has 2.39.5), `horch doctor` says nothing, the start of fleetd says nothing, and the first message is a failed rescue job with a needs-you item, at the moment when a tree with work of a worker must be saved.

## Goal

1. `horch doctor` has 1 check line for git: the version that it found, and a failure with the fix text (the needed version and why: the rescue of a worktree) when the version is too old or git is not found. It uses the same rule as the rescue (`has_attr_source`: 1 rule, 1 place; make it reachable through the public API of its context, not a copy).
2. The start of fleetd (the daemon) checks the same rule 1 time. Read the design (`docs/specs/fleetd/design.md`: the start of fleetd, the doctor, the health guards) and choose by the design: a refusal to start with the fix text, or a start with dispatch held and 1 needs-you item. Send `QUESTION:` with the 2 options and the design lines before you write this part; the file of the start (`crates/horch/src/cmd/fleetd.rs`) is a shared file that task T-369 edits.
3. The git that is checked is the git that fleetd and its jobs use (the PATH of the context of fleetd, SR-4), not the git of the PATH of the shell if they differ: say which it is with the code line.

## Tests

A fake git on the PATH of the test that prints `git version 2.39.5`: doctor fails with the fix text; with `2.41.0` and with a form like `git version 2.51.2.windows.1` or `2.39.5 (Apple Git-154)` it passes or fails by the numbers. Read the test file names first (`ls crates/horch/tests | grep -i doctor`; the Check runs the test filter `doctor` of the crate `horch`: confirm that it runs your tests and say the count).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `/fleet`. Never install the build; a manual run uses the built binary by its path with HOME, HORCH_STATE_DIR, HORCH_LEDGER_DB and FLEET_ROOT inside your sandbox folder.
3. No OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`.
4. Never run pkill or killall. Never touch a process of another fleet.
5. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, restore each comment that gives a rule or a design id.
6. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes and part 2 is done or has an open `QUESTION:`.
