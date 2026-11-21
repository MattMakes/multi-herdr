# Windows: product output with backslash paths and CRLF text (about 60 tests of the group 'other' and 'goldens' of CI run 5)

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked taskledger::map && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test golden_prompts --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/map.rs`, `crates/horch-core/tests/golden_prompts.rs`

## Fault

CI run 5, Windows, groups with 1 cause each (counts from `ci-run-5.md`): `taskledger/map.rs` 10 (`tickets\MAP.md:3` where `/` is expected); `cmd/context.rs` 8, `context_cli.rs` 6, `context_roster.rs` 7, `context_prime.rs` 6 (the table of `context`); `telemetry_collect.rs` 7 (`rec-b1: -0`); `cmd/compact.rs` 6; `w3_ledger_cli_pins.rs` 5 (`.\.horch\ledger.sqlite3`); goldens and LF-only expected text 10 (`golden_prompts.rs`, `r_routing_assess.rs`, `r_routing_parsers.rs`, `r_runtime_skills_materialize.rs`, `skills_cli.rs`, `context_launch.rs`).

This host cannot run Windows tests and cannot compile `horch-core` for Windows. Evidence: the log of CI run 44890393381, job `test-windows` (`gh run view 44890393381 --log --job <id>`; find the id with `gh run view 44890393381 --json jobs`), and `docs/specs/fleetd/gates/ci-run-5.md` on the run branch, section Windows. Compile check that works here: `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc`. Reproduce each cause on macOS with a unit test that feeds the Windows form of the input (a path string, bytes) where that is possible. Say in the commit message which tests of the log each change should turn green; the next CI run proves it.

## Steps

1. For each group read 2 failure blocks of the log and decide: (a) the product prints a path or text in an OS form where its contract is 1 form (a ticket map path, a ledger path in a message, a prompt): fix the product so that the text is the same on each OS (forward slashes in what a worker or a file reads; LF in files that horch writes); or (b) the test compares with an OS form by mistake: fix the test helper. 1 shared helper for the form of a path in output, not a change at each call.
2. Files outside your Own: send `QUESTION:` with the list after step 1, and go on with `map.rs` and the goldens meanwhile.
3. Unit tests on macOS that feed a Windows form path string to each fixed function.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install a tool or the build.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. No OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert. A pass runs no SQL statement for each row and never waits for a job.
4. Never run pkill or killall. Never touch a process of another fleet. `gh` is read only: never rerun, cancel or change a run or a pull request.
5. Write 1 `horch ledger progress` line each 5 minutes inside each wait loop (the watchdog stops a silent worker after 30 minutes). Commit after each step.
6. `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish`. After the comment sweep, run finish again at once; restore no comment.
7. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes and the commit message names each cause with its proof.
