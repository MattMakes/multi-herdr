# U07 e2e-fakes report

Branch `ard/e2e-fakes`. The unit adds test infrastructure and smoke tests. It
adds no requirement tests and changes nothing in `horch-core` or `horch`.

## Scenarios

`HORCH_FAKE_SCENARIO` holds one name or a comma-separated list, for example
`exec,fail_split`. `horch_e2e::scenario_has(name)` tests one name.
`horch_e2e::scenario()` returns the first name, so a single name works as
before. `horch_e2e::scenarios()` returns the whole list.

| Fake | Scenario | Effect |
|---|---|---|
| fake-herdr | `default` | Answers everything. |
| fake-herdr | `fail_create` | `workspace create` exits 1. |
| fake-herdr | `fail_split` | `pane split` exits 1, message on stderr, state unchanged. |
| fake-herdr | `fail_run` | `pane run` exits 1, message on stderr, state unchanged, no command runs. |
| fake-herdr | `exec` | `pane run` runs the command detached. The process group id goes into the state. `pane close` kills the group. |
| fake-opencode | `default` | A launch exits 0 at once. |
| fake-opencode | `stay` | A launch sleeps until killed. |
| fake-prime | `default` | A launch exits 0 at once. No daemon stays registered. |
| fake-prime | `stay` | A launch stays alive. It is the registered daemon for its socket. |

`fake-herdr`: `pane split` adds `<workspace>:p<n>` to the state, in the source
pane's tab and workspace, and answers `{"result":{"type":"pane","pane":{pane_id,
workspace_id,tab_id}}}`. `pane close` removes the pane and exits 1 for an unknown
pane. Both calls are still recorded as violations. A split from a pane that is
not in the state still answers, with id `<from>-split`, as before.

## Deterministic ids

- OpenCode session id: `horch_e2e::opencode_session_id(dir)` = `ses_` + 16 hex
  digits (FNV-1a 64 of the canonical directory). Pass the harness project dir.
  The fake hashes its cwd after canonicalization, so `/private/var` on macOS
  matches.
- Prime session id: `horch_e2e::prime_session_id(session_dir)` = `prime_` + 16 hex
  digits (FNV-1a 64 of the `--session-dir` text as passed). The session file is
  `<session-dir>/<id>.jsonl`. Prime's resume handle is the file path.
- Git commit: with `with_git()` the fixture commit hash is the same in every run.

## How a test enables each fake

All fakes are copied into the harness `bin/` and named like the real programs:
`claude`, `codex`, `herdr`, `opencode`, `prime-agent`, `pi`, `ollama`. `seal()`
sets `HORCH_OPENCODE_BIN` and `HORCH_PRIME_BIN` as it does for the others. No
test needs extra setup. Call `h.set("HORCH_FAKE_SCENARIO", "...")` to choose a
scenario.

- `HORCH_FAKE_TRANSCRIPTS=1` makes fake-opencode write 1 session and 1 completed
  assistant message into `opencode.db` (`$HOME/.local/share/opencode/opencode.db`
  in the harness), and fake-prime add 1 assistant line to its session file.
- fake-opencode answers `--version`, `session list ...` and any other argv as a
  launch. It records `session_id`, `cwd`, `model` and `resumed` in the call.
- fake-prime answers `--version`, `status --json` and any other argv as a launch.
  It records `socket`, `session_dir`, `session_id`, `session_file`, `model` and
  `resumed`. `status --json` lists a launch only while its process is alive.
- `Harness::with_git()` returns the harness with a repo in `h.project`.
  `h.git_bin()` is `Option<&Path>`, `h.head_sha()` is `Option<String>`,
  `h.git_cmd(args)` builds a pinned git command. `HORCH_GIT_BIN` names the real
  git in the sealed environment. Git is not on the sealed `PATH`.
  `h.allows_program(path)` is the NFR-01 check: the fakes dir and exactly that git.

## Decisions

- NFR-01 lives in `tests/e2e.rs` (`nfr_01_no_real_binaries`), which this unit does
  not edit. That test stays valid, because git is never on the sealed PATH. The
  new allowance is `Harness::allows_program`, covered by
  `harness_with_git_makes_repo`. A later unit may call it from the e2e test.
- The fakes use only `std` and `serde_json`. The e2e crate has no dependency on
  `horch-core`, so the smoke tests check the JSON shape and do not call
  `parse_sessions`.
- The SQLite rows are written through the `sqlite3` CLI with the SQL on stdin.

## Gotchas for A6

- The sealed PATH holds only the fakes dir. A command run by `exec` must use
  absolute paths for `sleep`, `touch` and similar programs.
- `fake-prime` does not model a daemon that outlives its process. horch's
  `Daemon::finish` finds a pid only for a `stay` launch.
- `fail_split` and `fail_run` are the fake side only. Phase A6 must make horch
  record `LaunchFailed` and close the pane.
- Baseline: on this Mac `Harness::new` copies `sqlite3` and macOS kills the copy
  (exit 137). `tests/e2e.rs:54` also passes SQL that begins with `--` as an
  argument. Another unit fixes both. Until it merges,
  `fake_opencode_writes_transcript_rows_when_asked` finds a `sqlite3` that does
  not run. It then skips, or fails under `HORCH_REQUIRE_SQLITE=1`.
  `with_git` does not depend on sqlite.

## Tests added: 7 in `crates/horch-e2e/tests/fakes.rs`

- `fake_herdr_split_adds_pane_and_close_removes_it`
- `fake_herdr_fail_split_and_fail_run`
- `fake_herdr_close_kills_the_exec_process` (extra)
- `fake_opencode_session_list_matches_cwd`
- `fake_opencode_writes_transcript_rows_when_asked` (extra)
- `fake_prime_creates_session_file`
- `harness_with_git_makes_repo`
