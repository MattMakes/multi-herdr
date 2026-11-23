# CI FIX-1: the e2e harness must not take the cargo profile from a folder name

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --lib --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test mail --locked && cargo fmt --all --check`
**Own:** `crates/horch-e2e/src/harness.rs`

## Goal

On CI (rustc and cargo 1.100) about 560 tests fail on macOS and on Linux with `the build of horch and the fakes failed: cargo build ... --profile out`. Make the harness build work with cargo 1.100 and with the local toolchain.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md` in your worktree: section "macOS", "Groups", and "Fix units" row FIX-1.
- `crates/horch-e2e/src/harness.rs`: `build_command` (about lines 960 to 1030) and its unit test (about line 1264).
- The CI log: `gh run view 44680906908 --log-failed` (job 136103798948). Read only the first error of 1 e2e test.

## Cause (from the CI report; verify it in the code)

cargo 1.100 puts a test binary in `target/debug/build/<package>/<hash>/out/`. `build_command` takes the name of the last folder of the path of the test binary as the cargo profile. The name is `out`, so it calls cargo with `--profile out`.

## Steps

1. Write the failing unit test first: a test binary path of the new layout (`.../target/debug/build/horch-e2e/<hash>/out/<name>`) and of the old layout (`.../target/debug/deps/<name>`) each give the profile `dev` (and `release` for a `release` path). Name: `profile_comes_from_the_target_layout_of_old_and_new_cargo`.
2. Fix `build_command`: find the profile folder as the path part directly below the target folder (handle a target triple folder, for example `target/<triple>/debug`). Do not use the name of the last folder. Map `debug` to `dev`.
3. Find each other place of the harness that derives a path from the folder of the test binary (the place of the built `horch` binary and of the fakes). Fix it for the new layout too.
4. Do not install a second toolchain. Do not run `rustup update`. The proof for cargo 1.100 is the unit test and a later CI run.

## Out of scope

Other failures of the CI report. `.github/workflows/ci.yml`.

## Done when

The new unit test passes, the Check passes, and the commit message names the cause.

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

