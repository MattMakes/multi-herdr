# Windows core: a verbatim path still reaches git, and the worktree_add job never ends (157 tests of CI run 5)

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc && scripts/dev-sandbox.sh cargo test -p horch-platform --locked && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test worktree_create --test worktree_jobs --test arch_scan && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/path.rs`, `crates/horch-platform/src/start.rs`

## Fault

The largest Windows group of CI run 5 (157 tests): `worktree \?\C:\...1-T1-after is not made yet: the worktree_add job 1 is running` (76 tests); `git ["worktree", "add", ..., "\\?\C:\...\gate-F-0001"]: fatal: could not create leading directories` (9); `git ["read-tree", ...]: fatal: Unable to create '\?\...` and more. Tasks T-340/T-348 (`horch_platform::path::for_child` in `start::Command`) and T-355 (test helpers) did not end it. So git children are started by a path that does not go through that choke point, or a path is given inside an argument or an environment variable (GIT_DIR, GIT_WORK_TREE, GIT_INDEX_FILE, a `-C` value, a config value), or the job host itself gets a verbatim path and its job never ends.

This host cannot run Windows tests and cannot compile `horch-core` for Windows. Evidence: the log of CI run 44890393381, job `test-windows` (`gh run view 44890393381 --log --job <id>`; find the id with `gh run view 44890393381 --json jobs`), and `docs/specs/fleetd/gates/ci-run-5.md` on the run branch, section Windows. Compile check that works here: `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc`. Reproduce each cause on macOS with a unit test that feeds the Windows form of the input (a path string, bytes) where that is possible. Say in the commit message which tests of the log each change should turn green; the next CI run proves it.

## Steps

1. From the log, list each distinct failing git command line and each job that stays `running`, with the code line that builds it (`git grep -n 'canonicalize()' -- crates/*/src`; each `Command::new` that is not `horch_platform::start::Command`: `git grep -n 'process::Command::new' -- crates/*/src`; the git runners of `horch-marketplace/src/git.rs` and `worktree_custody/adapters/git.rs`).
2. Fix at the choke points: each path that goes to a child as an argument, as the working folder or in an environment variable is plain (`for_child`). Prefer 1 place (the runner) over each call. Files outside your Own: send `QUESTION:` with the list first; go on with the analysis meanwhile.
3. Why does the `worktree_add` job never end on Windows: read how the job host is started and how its end is seen there (the process identity, the result file path, the wait). Name the cause with the code line; fix it or give the facts if it needs a Windows run.
4. Unit tests on macOS for each fixed call with a Windows form path string.

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
