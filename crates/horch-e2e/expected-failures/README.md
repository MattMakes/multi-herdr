# The list of expected failures

Phase 0 adds red tests. Each red test stays red until a later phase makes it
green. This folder is the list of those tests (design amendment A22).

## The run

```sh
cargo test --workspace --locked
```

The run passes when each test that is not on the list passes, and each test
that is on the list fails for its listed reason.

## The files

`<stem>.txt` holds the entries of `crates/horch-e2e/tests/<stem>.rs`. Each test
file has its own list file, so that tickets can change their lists at the same
time.

1 line is 1 entry:

```text
<test name> | <phase> | <reason>
```

- `<test name>` is the name of a `#[test]` function in the test file. A test
  is listed 1 time only.
- `<phase>` is the phase that makes the test green: 1 of `P`, `1`, `2`, `3`,
  `4`, `5`, `6`, `7`.
- `<reason>` is a text that the panic message of the test must contain. It is
  the rest of the line, so it can contain `|`.

Blank lines and lines that start with `#` are comments.

## The test

A listed test runs its body through the wrapper:

```rust
#[test]
fn my_test() {
    horch_e2e::expected::check(file!(), "my_test", || {
        // the body of the test
    });
}
```

A listed test has 3 outcomes:

1. The body panics with a message that contains the reason: the test passes.
2. The body panics with another message: the test fails and shows the 2
   messages.
3. The body passes: the test fails and names the phase and the list file.

A test that is not on the list runs as a normal test.

## The rule

A phase removes its tests from the list. When a phase makes a test green, the
test fails until its line is removed.

The test `expected_failures_list_gates_the_run` in
`crates/horch-e2e/tests/expected_failures.rs` fails the run when a list file
has no test file, when an entry names no `#[test]` function, when a listed test
does not call `check(file!(), "<test>", ...)`, or when a list file does not
parse.
