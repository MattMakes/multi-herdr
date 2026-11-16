# 02: Record each test result before the move, and compare after it (the test of PL-6)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `python3 -I scripts/pl6/test_pl6.py && test -s scripts/pl6/baseline-macos.txt && test -s scripts/pl6/BASE`
**Own:** `scripts/pl6/`

Spec: `docs/specs/fleetd/design.md`, requirement PL-6 and the table "What a release does to a run
in flight" (row Phase P).

## Goal

1 command records the result of each test of the workspace, and 1 command compares 2 records. The
record of macOS before the first move is committed. Each move ticket (10 to 15) and the gate ticket
use `scripts/pl6/check.sh` as their proof that the move changed no test result.

## Requirements

- **PL-6**: "On each of macOS, Linux and Windows, each test has the same result before the move and
  after it. A test that passes before the move passes after it. A test on the list of expected
  failures still fails." This ticket gives the tool and the macOS baseline. The gate ticket 17
  runs it for the 3 OSes.

## Context

- The test runner is libtest through `cargo test --workspace --no-fail-fast`. Its output has a
  line `Running unittests src/lib.rs (target/debug/deps/horch_core-<hash>)` or
  `Running tests/arch_scan.rs (target/debug/deps/arch_scan-<hash>)` before each test binary,
  `Doc-tests <crate>` before the doc tests, and 1 line `test <name> ... ok`, `... FAILED` or
  `... ignored` (maybe `, <reason>`) for each test.
- The crate `horch` has both `src/lib.rs` and `src/main.rs`, so the binary stem alone is not a
  unique key. A doc test name has `(line N)`, which changes when code moves.
- Phase 0 keeps the list of expected failures. A test on it fails before the move and must fail
  after it. A raw compare of results gives this rule with no extra code. Do not read the file of
  the list here.
- CI log lines have a time prefix (`2026-11-16T21:27:57.1234567Z `). `gh run view <id> --log`
  prints `<job>\t<step>\t<time> <line>`. The parser must accept both forms and a plain log.
- `scripts/dev/overlap_check.py` is an example of a Python tool of this repo (standard library
  only).
- Build rule 1 applies: the test run uses temporary folders for `HORCH_DATA_DIR`,
  `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`.

## Steps

1. Write the tests first, in `scripts/pl6/test_pl6.py` (`unittest`, standard library only). Each
   fails before `scripts/pl6/pl6.py` exists:
   - `test_parses_lib_bin_integration_and_doc_tests`: a fixed log with a lib, a bin of the same
     stem, an integration test and a doc test gives 4 distinct keys.
   - `test_doc_test_key_has_no_line_number`.
   - `test_parses_ci_log_prefixes`: the same log with the time prefix and with the
     `gh run view --log` prefix gives the same records.
   - `test_same_results_pass`.
   - `test_changed_result_fails_and_names_the_test`.
   - `test_missing_test_fails`: a test of the baseline that is absent after is an error ("removed
     or renamed").
   - `test_new_test_must_pass`: a new test that passes is accepted; a new test that fails is an
     error.
   - `test_expected_failure_that_still_fails_passes`: a test that fails in both records is
     accepted.
   - `test_gap_file_allows_a_named_windows_fix_or_skip`: with `--gaps <file>`, a key of the file
     may go from `failed` to the result that the file names (`ok` for `fixed`, `ignored` for
     `skipped`); any other change of that key is still an error, and a key that is not in the
     file gets no exception.
2. Write `scripts/pl6/pl6.py` with 2 subcommands:
   - `record <log> <out>`: write 1 line `<key>\t<result>` for each test, sorted. The key is
     `<running source path>|<binary stem without hash>::<test name>`, and
     `doc|<crate>::<doc test name without (line N)>` for a doc test. The result is `ok`, `failed`
     or `ignored`. Refuse a log that has no `test result:` line, or a binary that started and has
     no result line (an incomplete run).
   - `compare <before> <after> [--gaps <file>]`: print each difference and exit 1 when 1 exists.
     The rules are those of step 1. The gaps file has 1 line `<key>\t<row of section 6>\t<fixed|skipped>`
     for each Windows test of a gap of today that ticket 17 fixes or skips by name. PL-6 allows
     this: "A test that fails on Windows for a gap that exists today is fixed, or it is skipped on
     Windows by name with the row of section 6 that states the gap." Do not create the gaps file:
     ticket 17 owns it.
3. Write `scripts/pl6/check.sh` (POSIX `sh`, executable): make a temporary folder, run
   `env -u ANTHROPIC_API_KEY HORCH_DATA_DIR=<tmp>/data HORCH_STATE_DIR=<tmp>/state HORCH_LEDGER_DB=<tmp>/ledger.sqlite3 cargo test --workspace --no-fail-fast`
   into `<tmp>/test.log`, record it, and compare it with `scripts/pl6/baseline-macos.txt`. It
   prints the path of the log. Its exit code is the exit code of the compare. It does not use the
   exit code of `cargo test`, because an expected failure fails the run.
4. Record the baseline. Run `git rev-parse origin/fleet/fleetd-build` and write the hash to
   `scripts/pl6/BASE`. Make a scratch worktree with
   `horch ledger worktree scratch pl6-base`, and run `git checkout --detach <BASE>` in it. Run the same `cargo test` command there, record it to
   `scripts/pl6/baseline-macos.txt`, and commit the file. Do not record in your own worktree,
   because its tree has the tool and may differ from `BASE`.
5. Write `scripts/pl6/README.md`: what PL-6 asks, the 3 commands, how ticket 17 records Linux and
   Windows from the CI logs of `BASE` and of the tip
   (`gh run view <id> --repo MattMakes/multi-herdr --log`), and the rule for a flaky test (step 6).
6. Run `scripts/pl6/check.sh` once in your worktree. It must pass. If a test gives another result
   on a second run with no change, it is flaky. Report each flaky test by name to the orchestrator
   with `horch tell`. Do not hide it in the tool.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

Rules of this phase:

- The tool has no list of exceptions other than the gaps file of ticket 17. A test that changes
  its result is a fault of the move.
- Use only the Python standard library. Run Python with `python3 -I`.

## Done when

- The Check passes.
- `scripts/pl6/BASE` holds the commit of `origin/fleet/fleetd-build` before any move ticket lands.
- `scripts/pl6/check.sh` passes in this worktree, and its run time is in the summary.
