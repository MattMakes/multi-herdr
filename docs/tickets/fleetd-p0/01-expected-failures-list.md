# 01: The list of expected failures and the test rule that reads it

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --lib expected && cargo test -p horch-e2e --test expected_failures`
**Own:** `crates/horch-e2e/src/expected.rs`, `crates/horch-e2e/src/lib.rs`, `crates/horch-e2e/tests/expected_failures.rs`, `crates/horch-e2e/expected-failures/README.md`

Spec: `docs/specs/fleetd/design.md`, section "Migration plan", item 1 (Phase 0), and amendment A22 in `docs/specs/fleetd/amendments.md`.

## Goal

Phase 0 adds red tests. Each red test stays red until a later phase makes it green. The command
`cargo test --workspace --locked` must pass when each test that is not on the list passes, and each
test that is on the list fails for its listed reason. This ticket makes that rule. Each later ticket
of Phase 0 uses it.

## Requirements

No requirement of the design becomes green in Phase 0. This ticket gives the mechanism "the list
of expected failures (0)" of the row Phase 0 in the table "What each gate needs", and the A22 rules:

- Each red test of Phase 0 is on the list, with the phase that makes it green.
- A test run passes when each test that is not on the list passes, and each test that is on the list fails.
- A phase removes its tests from the list.

The orchestrator added 3 rules (answer to Q1 of the plan):

- Each list entry names the phase that makes it green.
- 1 test fails when a list entry names no test that exists (no stale entry).
- A listed test whose body passes fails the run.

## Context

- Rust has no "expected failure" mark. The rule is a wrapper: a listed test runs its body under
  `std::panic::catch_unwind` and inverts the result.
- The list is 1 file for each test file, so that the scenario tickets 02 to 07 run at the same time
  and each owns its own list file: `crates/horch-e2e/expected-failures/<stem>.txt` holds the entries of
  `crates/horch-e2e/tests/<stem>.rs`. Together these files are the list of expected failures.
- `crates/horch-e2e/src/lib.rs` declares the modules `harness` and `process` today. Add `pub mod expected;`.
- `horch_e2e::harness::repo_root()` (in `crates/horch-e2e/src/harness.rs`) gives the repository root.
- The format of 1 line of a list file: `<test name> | <phase> | <reason>`. `<phase>` is 1 of `P`, `1`,
  `2`, `3`, `4`, `5`, `6`, `7`. `<reason>` is a text that the panic message of the test must contain.
  Lines that are blank or start with `#` are comments.

## Steps

1. Write the unit tests first, in `crates/horch-e2e/src/expected.rs` (`#[cfg(test)] mod tests`).
   Each one fails before the code because the functions do not exist. Name them:
   - `expected_parse_reads_test_phase_and_reason`
   - `expected_parse_refuses_a_line_without_a_phase_or_a_reason`
   - `expected_parse_refuses_an_unknown_phase`
   - `expected_parse_refuses_a_test_listed_twice`
   - `expected_listed_body_that_panics_with_the_reason_passes`
   - `expected_listed_body_that_panics_for_another_reason_fails`
   - `expected_listed_body_that_passes_fails_and_names_the_phase`
   - `expected_unlisted_body_runs_as_a_normal_test`
   - `expected_audit_reports_an_entry_with_no_test` (stale entry)
   - `expected_audit_reports_a_list_file_with_no_test_file`
   - `expected_audit_reports_a_listed_test_that_does_not_call_the_wrapper`
2. Write `crates/horch-e2e/src/expected.rs`:
   - `pub struct Entry { pub test: String, pub phase: String, pub reason: String }`.
   - `pub fn parse(text: &str) -> Result<Vec<Entry>, String>`: the rules above.
   - `pub fn check(test_file: &str, test: &str, body: impl FnOnce() + std::panic::UnwindSafe)`.
     The caller passes `file!()` and its own function name. It reads the list file of the stem of
     `test_file`. A missing list file means an empty list.
     - The test is not listed: run `body()` with no wrapper.
     - The test is listed and the body panics with a message that contains the reason: return.
     - The test is listed and the body panics with another message: panic with both messages.
     - The test is listed and the body passes: panic with "`<test>` passed but is on the list of
       expected failures. Phase `<phase>` makes it green: remove its line from `<list file>`."
   - `pub fn audit(lists: &Path, tests: &Path) -> Vec<String>`: 1 line for each fault. A list file
     with no test file of the same stem. An entry whose test file has no `fn <test>(` after a
     `#[test]`. An entry whose test does not call `check(file!(), "<test>"`. A list file that does
     not parse.
3. Write `crates/horch-e2e/tests/expected_failures.rs` with 1 test,
   `expected_failures_list_gates_the_run`. It calls `audit` on `crates/horch-e2e/expected-failures/`
   and `crates/horch-e2e/tests/`, and asserts that the result is empty. Before the list files exist,
   it passes.
4. Write `crates/horch-e2e/expected-failures/README.md`: the line format, the 3 outcomes of a listed
   test, and the rule "a phase removes its tests from the list". Give the command of the run:
   `cargo test --workspace --locked`.
5. Run the Check. Run `cargo clippy -p horch-e2e --tests -- -D warnings`.

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

## Done when

- The Check passes. Each unit test of step 1 was seen to fail before the code.
- `cargo clippy -p horch-e2e --tests -- -D warnings` passes.
- The README names the format, the phases and the command of the run.
