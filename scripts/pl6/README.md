# The test of PL-6

PL-6 (`docs/specs/fleetd/design.md`) says that the move of Phase P changes no behavior:

> On each of macOS, Linux and Windows, each test has the same result before the move and after
> it. A test that passes before the move passes after it. A test on the list of expected failures
> still fails.

This folder gives the tool for that test and the macOS record of `BASE`, the commit of
`fleet/fleetd-build` before the first move ticket.

## Files

- `pl6.py` reads the output of `cargo test --workspace --no-fail-fast` and compares 2 records.
- `check.sh` runs the tests on this host and compares the result with `baseline-macos.txt`.
- `baseline-macos.txt` is the macOS record of `BASE`.
- `BASE` holds the commit of that record.
- `test_pl6.py` tests `pl6.py`: `python3 -I scripts/pl6/test_pl6.py`.

## The 3 commands

```sh
python3 -I scripts/pl6/pl6.py record <log> <out>
python3 -I scripts/pl6/pl6.py compare <before> <after> [--gaps <file>]
scripts/pl6/check.sh
```

`record` writes 1 line `<key>\t<result>` for each test, sorted. The result is `ok`, `failed` or
`ignored`. The key is `<source path>|<binary stem>::<test name>`, for example
`src/main.rs|horch::parse::reads_flags`, and `doc|<crate>::<name>` for a doc test. The key of a
doc test has no `(line N)`, because the line changes when code moves. `record` refuses a log with
no `test result:` line, and a log in which a test binary starts and has no result line.

`compare` prints each difference and exits 1 when 1 exists:

- A test with another result is an error.
- A test of `<before>` that is absent from `<after>` is an error ("removed or renamed").
- A new test must pass.
- A test that fails in both records is accepted. This is how an expected failure of Phase 0
  stays a failure: the tool does not read the list of expected failures.

`check.sh` runs the tests with temporary folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and
`HORCH_LEDGER_DB`, prints the path of the log, and exits with the exit code of `compare`. It does
not use the exit code of `cargo test`, because an expected failure fails the run. Each move ticket
(10 to 15) and the gate ticket 17 run it as their proof.

## Linux and Windows (ticket 17)

The record of each OS comes from the CI run of `BASE` and the CI run of the tip. In
`.github/workflows/ci.yml`, the job `test (ubuntu-latest)` and the job `test-windows` run the
same `cargo test` command.

```sh
gh run view <id of the run of BASE> --repo MattMakes/multi-herdr --log > base.log
gh run view <id of the run of the tip> --repo MattMakes/multi-herdr --log > tip.log
```

A log of `gh run view --log` holds the lines of all jobs. Keep the lines of 1 OS job before you
record them, for example `grep '^test (ubuntu-latest)' base.log > base-linux.log`. Use the job
name that the log prints. `record` accepts the `<job>\t<step>\t<time> ` prefix, the time prefix
alone, and a plain log. Do the same with `^test-windows` for Windows.

```sh
python3 -I scripts/pl6/pl6.py record base-linux.log base-linux.txt
python3 -I scripts/pl6/pl6.py record tip-linux.log tip-linux.txt
python3 -I scripts/pl6/pl6.py compare base-linux.txt tip-linux.txt
```

On Windows, PL-6 allows 1 exception: "A test that fails on Windows for a gap that exists today is
fixed, or it is skipped on Windows by name with the row of section 6 that states the gap."
Ticket 17 owns the gaps file. It has 1 line for each such test:

```text
<key>\t<row of section 6>\t<fixed|skipped>
```

With `--gaps <file>`, the key of a line may go from `failed` to `ok` (`fixed`) or to `ignored`
(`skipped`). Any other change of that key is still an error. A key that is not in the file gets
no exception. The tool has no other list of exceptions.

## A flaky test

A test is flaky when it gives another result on a second run with no change. Do not add it to a
list, and do not change the tool to hide it. Report each flaky test by name to the orchestrator
with `horch tell`. A test that changes its result after a move is a fault of the move until a run
with no change shows that it is flaky.
