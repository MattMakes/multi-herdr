---
name: rust-testing
description: Use when writing, fixing or reviewing Rust tests - unit tests in `#[cfg(test)]` modules, integration tests in `tests/`, doc tests, test fixtures that touch files or the environment, or a flaky or slow test run. Covers where each kind of test goes, hermetic temp-dir fixtures, `cargo test --workspace --no-fail-fast` and `cargo nextest`, and asserting error variants instead of calling `.unwrap()` on the happy path only.
---

# Rust testing

A good Rust test fails for one reason, runs the same way on every machine, and says what broke. Put each test where it can see what it needs and no more: private logic in a unit test, the public API in an integration test, usage examples in doc tests. Keep each test sealed from the machine it runs on. Test the failure paths as hard as the success path, because that is where bugs hide.

Follow the test style that the crate already uses: its helpers, its fixture crates, its naming. Check the APIs of test crates (`tempfile`, `insta`, `rstest`, `assert_cmd`, `cargo-nextest`) with the docs tool you have, for the versions in `Cargo.lock`. Do not add a new test crate inside an unrelated task.

## When to use

- A new function, module or bug fix that needs a test.
- A test that fails only on CI, only in parallel, or only sometimes.
- A test that writes files, reads environment variables, changes the current directory or starts a process.
- A review where tests call `.unwrap()` on every result and never check a failure.

## Where each test goes

- **Unit tests** go in a `#[cfg(test)] mod tests { use super::*; ... }` block at the bottom of the source file. They can reach private items. Use them for logic inside a module.
- **Integration tests** go in `tests/*.rs`. Each file is a separate crate that sees only the public API. Use them for behaviour that a caller depends on. Shared helpers go in `tests/common/mod.rs` (not `tests/common.rs`, which Cargo would treat as its own test crate).
- **Doc tests** are code blocks in `///` comments. They run with `cargo test --doc` and prove that the example in the docs compiles and works. Use `?` in examples with a hidden `# fn main() -> Result<(), Box<dyn std::error::Error>> {` wrapper so readers copy the right pattern. Mark a block `no_run` when it must compile but cannot run in a test, and `ignore` only with a reason.
- **Binary crates** cannot be called from integration tests. Move the logic into a `lib.rs` and keep `main.rs` thin, or test the binary as a process with `env!("CARGO_BIN_EXE_<name>")`.

## Workflow

1. **Read the existing tests.** Find 2 or 3 tests near the code you change. Note the helpers, fixtures and naming they use. Check: you know where the new test goes and which helpers to reuse.

2. **Write the test first when you fix a bug.** Reproduce the bug in a test, run it, and watch it fail for the right reason. Check: the failure message names the bug.

3. **Make the test hermetic.** Use the fixture checklist below. Check: the test passes when you run it alone, with the full suite, and twice in a row.

4. **Assert the result, including errors.** Use the assertion checklist below. Check: each new error path has a test that triggers it.

5. **Run the suite.**
   - Run `cargo test --workspace --no-fail-fast` so one failing crate does not hide failures in others. Add `--all-features` or the feature sets that CI uses when the crate has features.
   - When the repository uses nextest (a `.config/nextest.toml` or a `cargo nextest` line in CI or the justfile), run `cargo nextest run --workspace --no-fail-fast` as well. Nextest runs each test in its own process, which finds tests that share state by accident. Nextest does not run doc tests, so also run `cargo test --doc`.
   - Run one test with `cargo test -p <crate> <name_filter> -- --exact` while you work. Add `--nocapture` to see its output.
   Check: the full suite is green, and you have its output.

## Checklist: hermetic fixtures

- **Files.** Create a fresh temp dir per test with `tempfile::tempdir()` (or the helper the repository uses). Keep the guard alive until the end of the test, because dropping it deletes the dir. Never write into the source tree, the home directory or a fixed path in `/tmp`.
- **Fixture data.** Read checked-in fixture files through `env!("CARGO_MANIFEST_DIR")`, not a relative path. Tests do not always run from the crate root.
- **Current directory.** Do not call `std::env::set_current_dir` in a test. It changes the directory for every test thread in the process. Pass the path to the code under test instead.
- **Environment variables.** `std::env::set_var` is process-wide and is `unsafe` from the 2024 edition. Prefer code that takes its config as an argument. When a test must set a variable for a child process, set it on the `Command` with `.env()` and `.env_remove()`.
- **Processes.** Start child processes with an explicit environment and working directory. Wait for them and check the exit status. Kill them on failure so they do not outlive the test.
- **Time and randomness.** Do not depend on the wall clock or an unseeded RNG. Pass a clock or a seed in. A test that sleeps to wait for something is slow and flaky. Wait on the actual event with a timeout.
- **Network.** Bind to `127.0.0.1:0` and read the port the OS gave you. Never call a real external service from a test.
- **Shared state.** A `static` or a global registry carries between tests in the same process. Reset it, or give each test its own instance.

## Checklist: assertions

- Assert the specific value: `assert_eq!(got, expected)` prints both sides on failure. `assert!(got == expected)` does not.
- For an error, assert the variant: `assert!(matches!(err, ParseError::UnexpectedEof { offset: 12 }), "{err:?}")`. Get the error with `.unwrap_err()` or `let Err(err) = result else { panic!(...) };`.
- Assert on the error message only when the message is the contract (a CLI's user-facing text). Otherwise match the type, because message text can change.
- A test that calls `.unwrap()` on every result checks only the happy path. For each `Result` the change adds, write one test where it is `Err`.
- Use `#[should_panic(expected = "...")]` only for code whose contract is to panic. Always give `expected`, or the test passes on any panic.
- A test can return `Result<(), E>` and use `?`. A returned `Err` fails the test with its `Debug` output, which is clearer than an `unwrap` panic.
- For large or structured output, use the snapshot crate that the repository already has (for example `insta`). Review each snapshot change as a diff. Do not accept snapshots in bulk.

## Gotchas

- `cargo test` runs tests in parallel threads by default. A test that passes alone and fails in the suite shares state with another test. Fix the sharing. Do not add `--test-threads=1` to hide it.
- `#[ignore]` without a reason in the attribute (`#[ignore = "needs a GPU"]`) hides a test forever. Give the reason, or delete the test.
- Doc tests compile as separate crates and are slow. Keep them as examples. Put edge cases in unit tests.
- A test that passes before your fix proves nothing about the fix. Watch it fail first.
- `cfg(test)` code is not compiled into integration tests or other crates. A helper that integration tests need goes in `tests/common/` or behind a `test-util` feature.
