# U07 e2e-fakes: fake-opencode, fake-prime, fake-herdr failures, git harness

Unit slug: `e2e-fakes`. Branch: `ard/e2e-fakes`. Phases: A6/B2 test infrastructure.

## GOAL

The hermetic e2e harness can stand in for all 5 harnesses and for herdr pane
failures, and can build a real temp git repo. Later phases use this to write
`arc_16_e2e_fail_split`, `arc_26_e2e_lifecycle_matrix_{claude,codex,opencode,pi,prime}`
and the dataset e2e tests. This unit adds infrastructure plus smoke tests
only; it does not write those requirement tests.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §3 "A6" (the paragraph on fake-herdr and new
  fakes, and the test list), §3 "B2" (`Harness::with_git()`), §3 "B3" item 6
  (candidate mode; NOT in this unit), §5 "Per-phase e2e".
- Read the existing fakes and harness:
  - `crates/horch-e2e/src/lib.rs` (`Call`, `say`, `scenario`)
  - `crates/horch-e2e/src/harness.rs` (`Harness::new`, `seal`, `set`, `calls`,
    `violations`)
  - `crates/horch-e2e/src/bin/fake-herdr.rs` (state in
    `$HORCH_FAKE_LOG.state.json`; mutating calls recorded as violations;
    scenarios `default`, `fail_create`, `exec`)
  - `crates/horch-e2e/src/bin/fake-claude.rs`, `fake-codex.rs`, `fake-pi.rs`
  - `crates/horch-e2e/tests/e2e.rs` and `tests/scenario.rs` for how tests use them.
- Read how horch discovers sessions for the two harnesses that you fake new:
  - OpenCode: `crates/horch-core/src/opencode.rs` (it runs
    `opencode session list` and picks the session by directory). Also read
    the opencode launch builder in `crates/horch-core/src/launch.rs` (about
    line 120) and the OpenCode transcript reader in
    `crates/horch-core/src/telemetry/readers.rs` (it reads SQLite through the
    `sqlite3` CLI).
  - Prime: `crates/horch-core/src/prime.rs` (daemon socket, `find_session` in
    a sessions dir) and the prime/pi builder in `launch.rs` (about line 199),
    and the Prime reader in `telemetry/readers.rs`.
  - How horch picks a binary path: `crates/horch-core/src/agent.rs`
    (`HORCH_*_BIN` overrides). The harness `seal()` sets these for the
    existing fakes; do the same for the new ones.
- Phase A6 (later) adds `fail_split`/`fail_run` handling to horch; you add the
  fake side now.

## FILES

own:
- `crates/horch-e2e/Cargo.toml` (new `[[bin]]` entries)
- `crates/horch-e2e/src/bin/fake-opencode.rs` (new)
- `crates/horch-e2e/src/bin/fake-prime.rs` (new)
- `crates/horch-e2e/src/bin/fake-herdr.rs`
- `crates/horch-e2e/src/harness.rs`
- `crates/horch-e2e/src/lib.rs`
- `crates/horch-e2e/tests/fakes.rs` (new, smoke tests for the fakes)
- `ai_docs/reports/arch-refactor-dataset/e2e-fakes.md`

do not touch: `crates/horch-core/**`, `crates/horch/**`,
`crates/horch-e2e/tests/e2e.rs`, `crates/horch-e2e/tests/scenario.rs`.
If an existing e2e test breaks because of your fake-herdr change, fix the
fake, not the test.

## STEPS

1. Create the worktree (conventions §2).
2. fake-herdr:
   - `pane split` adds a new pane to the state (in the source pane's tab and
     workspace) and returns its id in the same JSON shape that real herdr and
     the current fake return. Keep recording it as a violation exactly as
     today, so existing assertions keep their meaning.
   - `pane close` removes the pane from the state. If the pane was started
     with the `exec` scenario, kill its process (store the pid in the state).
   - New scenarios `fail_split` (`pane split` exits 1 with an error message on
     stderr, state unchanged) and `fail_run` (`pane run` exits 1, state
     unchanged). Allow combining scenarios: `HORCH_FAKE_SCENARIO` may hold a
     comma-separated list; update `scenario()` in `lib.rs` to support
     `scenario_has("fail_split")` while keeping `scenario()` working.
   - Document every scenario in the file header.
3. fake-opencode: answers the commands horch runs:
   - The launch argv (whatever `launch.rs` builds for OpenCode): record the
     call, then behave by scenario (`default`: exit 0 at once; `stay`: sleep
     until killed).
   - `opencode session list` (with the flags horch passes): print a session
     list in the format `opencode.rs` parses, with one session whose directory
     equals the process cwd. Use a deterministic session id derived from the
     cwd (`ses_` + 16 hex of a simple hash), so a test can predict it.
   - `--version`: print a fixed version.
   - If `HORCH_FAKE_TRANSCRIPTS=1`, also write the minimal SQLite rows that
     the telemetry reader needs, through the `sqlite3` CLI if present; skip
     silently if `sqlite3` is absent. If this needs more than 1 hour, skip it
     and record a NOTE in the report.
4. fake-prime: answers the Prime launch argv and the daemon interaction that
   `prime.rs` and `launch.rs` expect. At minimum: record the call; create the
   session file in the sessions dir that `find_session` scans, with a
   deterministic session id; honor the socket path argument by creating the
   socket path as a plain file or a listening unix socket, whichever
   `prime.rs` requires; exit by scenario as fake-opencode does.
5. `Harness` additions in `harness.rs`:
   - Seal the new fakes the same way as the others (bin overrides on PATH and
     in the `HORCH_*_BIN` variables).
   - `Harness::with_git(self) -> Self`: find the real `git` by absolute path
     (skip when absent unless `HORCH_REQUIRE_GIT=1`), create an empty
     `GIT_CONFIG_GLOBAL` file in the harness temp dir, set
     `GIT_CONFIG_NOSYSTEM=1`, `GIT_AUTHOR_NAME/EMAIL`,
     `GIT_COMMITTER_NAME/EMAIL`, fixed `GIT_AUTHOR_DATE`/`GIT_COMMITTER_DATE`,
     and init a tiny repo in the harness project dir with 2 files and 1
     commit on branch `main`. Expose `git_bin()` and `head_sha()`.
   - Keep the NFR-01 rule: every spawned program path is inside the fakes dir,
     except the real `git` when `with_git()` is used. Update the check that
     enforces NFR-01 so that it allows exactly that one path.
6. Smoke tests in `crates/horch-e2e/tests/fakes.rs` (not requirement tests):
   - `fake_herdr_split_adds_pane_and_close_removes_it`
   - `fake_herdr_fail_split_and_fail_run`
   - `fake_opencode_session_list_matches_cwd`
   - `fake_prime_creates_session_file`
   - `harness_with_git_makes_repo`
7. Run the gate. Commit per step: `A6: fake-herdr pane state and failure scenarios`,
   `A6: Add fake-opencode`, `A6: Add fake-prime`, `B2: Add Harness::with_git`.
8. Write and commit the report: the scenarios, the deterministic ids, and how
   a test enables each fake. Follow conventions §6 to finish.

## CONSTRAINTS

- The fakes use only `std` and `serde_json`. No new crate.
- Existing e2e tests in `tests/e2e.rs` and `tests/scenario.rs` stay green and
  unchanged.

## DONE WHEN

- The 5 smoke tests pass with `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1`.
- `cargo test -p horch-e2e` passes. The full gate is green.

## REPORT

- `horch note` after each fake.
- `horch done` summary: scenarios, deterministic ids, gotchas for A6.
