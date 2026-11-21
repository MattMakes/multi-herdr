# Tests run every new executable once before timing it

The unit, integration and e2e tests run each fake executable they write or
link once, untimed, before any timed call uses it. macOS scans a new
executable at its first exec (syspolicyd `GK performScan` and an XProtect
pass). The scans run 1 at a time, about 0.2 s each, so with 30 new
executables at once the last waits 5.5 s (T-24). Under load one scan takes
14 to 35 s, and tens of seconds at worst. Inside a timed wait that scan
turns into a spurious timeout. The unit and integration helper is
`write_warmed` in `crates/horch-core/src/test_support.rs`,
`crates/horch/src/test_support.rs` and
`crates/horch-core/tests/common/warm.rs`: it writes the script as an
executable and runs it once with the given arguments, with no limit.

The e2e harness in `crates/horch-e2e` links the built fakes into `bin/`, so
the first call of a fake inside a test would carry the scan into a timed
probe. `warm_built_fakes` (`crates/horch-e2e/src/harness.rs`) runs every
built fake once, untimed, before the first test uses it. The first harness
of a test process pays the scan, and the others wait on the same
`std::sync::Once`.

## Consequences

- On Linux, a test thread that forks while another thread writes a script
  can hold the script open for writing. Exec then fails with "Text file
  busy" (ETXTBSY) until that child execs, so `write_warmed` retries until
  it succeeds. The e2e `harness::warm`, which runs a new executable once,
  untimed, and returns its output, retries only that error.
- The exit status of the e2e warm run of a built fake does not matter. A
  fake answers an unknown argument at once and writes no log without
  `HORCH_FAKE_LOG`.
- Fake scripts take a `warm` argument that ends the script before its
  body, so a slow body (`sleep 30`) costs nothing in the warm run. Callers
  never pass `warm` (`crates/horch-core/src/harness/codex.rs`). The fake
  herdr of `script_in` (`crates/horch-core/src/workspace/herdr.rs`,
  `crates/horch-core/src/workspace/exec.rs`) is given back already run
  once, and its warm run exits before `body`. A script with no warm case,
  such as the scripted herdr of `tests/r_workspace_arrange.rs`, is run once
  and its exit is ignored.
- The `prime-agent` fakes (`crates/horch-core/src/harness/prime.rs`,
  `crates/horch-core/tests/context_prime.rs`) run once before they are
  given back. The fakes of git, gh, herdr and the updater
  (`harness::update` `FakeUpdater`) are warmed the same way.
- The `routing::quota_probe` tests give their fakes a 600 s deadline
  (`UNTIL_IT_EXITS`, `UNTIL_THE_FAKE_ANSWERS`) because the fakes always
  answer or exit. With the real 15 s `VERSION_TIMEOUT`, a fake starved by a
  loaded test run gives `NoAnswer` and the test fails. Each fake is warmed
  before the timed section.
- Where a test can avoid a new executable, it does: the complexity
  analyser in `crates/horch-core/tests/w3_finish_steps.rs` runs as `node
  <script>` with node = `/bin/sh`.
- In the e2e tests, `Harness::write_warmed` runs each sealed fake once with
  a `warm` argument before any timed step or deadline. The fake that is
  deaf to SIGTERM, in its own process group, must be warm because the scan
  can last past the 10 s that `pid_in` waits for the pid
  (`tests/fakes.rs`). Another fake there moves to a group of its own at
  the first SIGTERM, as `setsid` before that SIGTERM would, and the next
  SIGTERM ends it. It is warmed too.
- A freshly linked fake can take longer than the relay's real 10 s queue
  budget (`tests/fleet18_messaging.rs`). The offline path of `inbox` is
  measured after one warm `h.run(&["inbox"])`
  (`tests/fleet18_observability.rs`).
- The fake codex is started once with `codex debug models`, which records
  no call, because its scan can outlast the 2 s check timeout
  (`tests/taskledger.rs`).
- The harness that lets the fleet update warms every fake
  (`tests/update.rs`). Every npm package's newest release there is long
  past the cooldown.
