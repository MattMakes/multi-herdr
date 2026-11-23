# Review findings D266-F1 and D266-F2 (Important): Windows state of an older horch defeats the new result-folder ACL and the new project-root form

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc && scripts/dev-sandbox.sh cargo test -p horch-platform --locked && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked taskledger::runs && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/windows/fs.rs`, `crates/horch-core/src/taskledger/runs.rs`, `crates/horch-core/src/taskledger/schema.rs`

Read both findings in full: `docs/specs/fleetd/reviews/delta-review-3-windows-facts-and-landings.md` on the run branch (sections D266-F1 and D266-F2), with the order of events and the smallest fix of each.

## F1

`create_dir_owner_only` returns at once for a folder that exists, so a `run/jobs` folder of an older horch keeps its owner and inherited DACL, `read_protected` rejects each result, and each job stays running. Fix: for an existing folder verify the owner and the owner-only DACL; repair them when this user may (it is the owner or holds the admin token that made it), else fail with a typed error that names the folder. Never weaken a check of `read_protected`.

## F2

`root_key` now gives the plain form, but an existing ledger holds `projects.root` (and `runs.project_root`) in the verbatim form, so lookups miss the old project and a second project row is made. Fix: 1 migration step at ledger open that rewrites each stored root through the same function as `root_key` (a pure string function for the stored text: make it testable on macOS with a Windows form string), and merges 2 rows of the same root onto the older id (each table that refers to a project id). Idempotent. A migration test with a seeded verbatim root proves 1 project id and that the old active run is found.

This host cannot run Windows tests: prove with unit tests that feed Windows form strings, and the Windows compile check of horch-platform in the Check. Say in the commit message what the next CI run must show.

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

