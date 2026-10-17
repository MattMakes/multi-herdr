---
name: godot-test-loop
description: >-
  Prove Godot changes with a headless GDScript test loop. Record a baseline before you edit,
  write a test that fails first, run focused filters, then the full suite, a parse check and a
  smoke scene. Separate new failures from old ones by name. Covers red-first and mutation
  proofs, flaky tests and shared state, "leaked at exit" lines, audits of tests that only copy
  data, input injection, per-route regression tests, and the finish gate before a unit reports
  done. Holds a small runner, a test base class and a repeat-run script as blocks to copy. Use it
  whenever someone changes GDScript or scenes and must show that the change works, or asks why a
  test passes alone but fails in the suite, why the suite leaks at exit, whether a failure is
  pre-existing, how to test menus with input, or whether a test can fail at all. Use it even if
  nobody says "test runner" or "regression test".
---

# Godot test loop

Target engine: **Godot 4.7**. The GDScript blocks parse on 4.7.2. The 1 shell block is POSIX
`sh` and needs `perl`.

A Godot project that many people or agents change every day needs one fast, honest proof that a
change works and breaks nothing. The method is a headless test loop: record a baseline, write a
test that fails for the right reason, make it pass, then widen the run from the file to the full
suite and compare by failure name. It works because every claim ("my change caused no new
failure", "this test catches the bug") rests on a run you can repeat, not on a reading of the code.

## When to use

- You change GDScript, scenes or data in a Godot project and must prove the change.
- A test fails and you must decide if your change caused it.
- A test passes alone but fails in the full suite, under load, or only sometimes.
- The suite prints "leaked at exit" or RID leak lines.
- A big suite breaks on every valid tuning change, or holds tests that assert nothing.
- A test must drive menus or the player with input.
- A unit of work is about to report done.

## When not to use

- The check needs real pixels, a real renderer or a frame-time number. Use
  `godot-visual-verification` or `godot-perf-measure`.
- The import, a tool script or a headless probe fails before any test starts. Use
  `godot-headless-runs`.
- You must prove that a refactor keeps behaviour byte for byte. Use `godot-safe-refactor`.
- You want to set up GUT or gdUnit4 and learn their assert API. Use `godot-testing`.

## Method

1. **Have a runner that cannot lie.** Use one small runner script (copy the block in
   [script-run-tests.md](references/script-run-tests.md) into a file).
   It runs every `test_*.gd` file and takes `--filter=<path text>`. It fails a test on a failed
   assert and on any engine error or `push_error`, through a registered `Logger`. It counts skips
   as skips, prints passed, failed and skipped, and exits 1 when 0 tests ran. Check: a run with a
   filter that matches nothing exits 1. Why: a green run with 0 tests looks like a pass.
2. **Start from a clean import and parse check.** This loop assumes the prerequisite that
   `godot-headless-runs` owns: the import after each checkout, merge or new `class_name` file,
   and the project parse check. Follow its "Import and parse hygiene" section. Check: both pass
   before you record the baseline. Why: a stale class cache after a merge can fail hundreds of
   tests that your change never touched.
3. **Record a baseline before any edit.** Run the full suite, or at least the target folders.
   Write down passed, failed and skipped counts and each known failure with its cause. Check: the
   baseline is in your notes before the first edit.
4. **Separate old failures from new ones.** When a check fails after your change, run the same
   check on a clean copy of the base (`git archive <base> | tar -x -C <tmp>` or a temporary
   worktree, then import). Copy every folder the check reads. Check: if it fails there too,
   report it as pre-existing with the exact FAIL line, and do not fix it inside your unit.
5. **Read before you write.** Read the test base class and 1 existing test of the same kind. Use
   the real helper names. Why: a guessed helper such as `assert_equal` fails at parse time and can
   take the whole file down.
6. **Build in numbered steps, 1 test file per step.** Write the test first. Run it with the filter
   and check that it fails for the expected reason (see
   [prove-the-test-can-fail.md](references/prove-the-test-can-fail.md)). Write the
   code, run the filter until it passes, and commit the step.
7. **Inject what is not there yet.** Use a test double for a dependency that has not landed.
   Inject test data. Production code reads the real data only when no data is injected. Replace
   the double with the real node when the dependency lands.
8. **Widen the run.** Run the system's folder, then the full suite, then the project parse check
   from `godot-headless-runs`, then a main or demo scene headless with `--quit-after <frames>`
   and a log file. Run `git diff --check`. Check: only the files you own changed.
9. **Compare by name, not by count.** Diff the FAIL lines with the baseline. Explain every new
   test in the passed count (for example "+21 = the 21 new tests"). Check: no new FAIL name, and
   no "leaked at exit" line (see [leaked-at-exit.md](references/leaked-at-exit.md)).
10. **Split headless and window tests.** Mark a test that needs a draw or a real renderer to skip
    headless. Run those tests in a window run, on each renderer you ship.
11. **Do not call a timing test a regression from 1 run.** Run it alone 2 or 3 times and record
    the machine load each time. See
    [flakes-and-isolation.md](references/flakes-and-isolation.md).
12. **Write the counts down.** Put baseline counts, final counts and known failures in the notes
    file and the report.

## Focused procedures

Each procedure is in its own reference file.

- **Prove the test can fail**: red first, patch out and patch in, guard tests, mutants. A test
  that passes before the fix proves nothing about the fix. See
  [prove-the-test-can-fail.md](references/prove-the-test-can-fail.md).
- **Flakes and test isolation**: a test that passes alone and fails in the suite reads shared
  state. Find the state; do not add waits. See
  [flakes-and-isolation.md](references/flakes-and-isolation.md).
- **Leaked at exit**: a clean exit is part of a pass. See
  [leaked-at-exit.md](references/leaked-at-exit.md).
- **Test audit: data echo**: tests assert rules and relations, not copies of data tables. See
  [test-audit-data-echo.md](references/test-audit-data-echo.md).
- **Input injection and route trace tests**: drive menus through real input actions; 1 case per
  route to a screen. See [input-and-route-tests.md](references/input-and-route-tests.md).
- **Finish gate**: run it before a unit of work reports done or ready to land. See
  [finish-gate.md](references/finish-gate.md).

## Pitfalls

The most frequent rows. The full table of 70 rows is in
[pitfalls.md](references/pitfalls.md).

| Symptom | Cause | Fix |
| --- | --- | --- |
| Hundreds of failures after a merge | Stale import or class cache | Import, then run again |
| Failures from conflict markers | Suite ran on an unresolved merge | Resolve the merge first; discard the result |
| Runner green with 0 tests | No skip state; no 0-test rule | Count skips; exit 1 on 0 tests |
| Wrong helper name | Guessed API (`assert_equal`) or clash with base | Read the base class; unique helper names |
| Async helper did nothing | Called without `await` | `await` it |
| Next test sees old tuning | Static var leaked | Restore it in `after_each` |
| Second tap lost | No frame between release and press | 1 frame after release |
| Bugs reached the build | Lands ran only named folders | Full suite at verify |

## Artifacts to build

- **Test runner** (`run_tests.gd`): a `SceneTree` script; `--filter`, `--list-unchecked`; fails on engine
  errors; counts skips; exits 1 on 0 tests; calls an optional per-file reset hook.
- **Test base class** (`test_case.gd`): counted asserts, `skip()`, `skip_if_headless()`,
  `add_node()` with auto-free, `frames()`, `seconds()`, `tap()`, `scratch_path()`.
- **Reset hook** (`res://tests/reset_tests.gd`): a static `reset(tree)` that resets each stateful
  autoload and points saves and settings at scratch files. Do not prefix it with `test_`.
- **Parse check tool**: use the parse check from `godot-headless-runs`. This skill does not hold a
  second copy.
- **Repeat-run loop** (`repeat_run.sh`): N runs of 1 filter, 1 log per run, a fail count.
- **Test spot helper**: reads test positions from level data, so tests survive layout changes.
- **Baseline report template**: date, base commit, passed/failed/skipped, known failures with
  cause, leak line, and the final counts beside them.
- **Audit notes template**: Baseline, Verdicts (1 row per test), Second-pass rows, Rewrite proofs,
  After.
- **Allowed-flake list**: test name, cause, owner, date added.

## Scripts to copy

None of the scripts is Python. Copy each block into a file, then run it with its own tool.

- `run_tests.gd`: copy the block in
  [script-run-tests.md](references/script-run-tests.md) into `res://tests/run_tests.gd`.
  Run `godot --headless --path <project> -s res://tests/run_tests.gd`. It needs Godot 4.5 or newer.
- `test_case.gd`: copy the block in
  [script-test-case.md](references/script-test-case.md) into `res://tests/test_case.gd`.
  The runner loads it; do not run it alone.
- `repeat_run.sh`: copy the block in
  [script-repeat-run.md](references/script-repeat-run.md) into a file. Run
  `sh repeat_run.sh <project dir> <filter> [runs]`. It needs `perl` and a `godot` binary on PATH,
  or the binary path in `GODOT`.

Smoke test for the GDScript files: see
[script-run-tests.md](references/script-run-tests.md#smoke-test).

## Related skills

- `godot-headless-runs`: owns the import, the project parse check, tool save isolation and
  process cleanup. Run its checks before this loop. This skill owns the test semantics.
- `godot-visual-verification`: the window gate and real captures for tests that skip headless.
- `godot-safe-refactor`: proof that a refactor keeps behaviour, and review fix waves.
- `godot-fleet-game-dev`: the worktree unit flow, landing, and the orchestrator's check of done
  claims. The finish gate here is the last step of that flow.
- `godot-release-and-store`: controller input support and the input map that tests inject into.
- `godot-balance-model-data`: the same negative-test proof for checks of design docs against data.
- `godot-perf-measure`: frame-cost budgets and stress harnesses beyond a single cost test.
- `godot-testing`: GUT and gdUnit4 setup and their assert API.

## References

- [script-run-tests.md](references/script-run-tests.md): the runner script; read it
  when you set up the loop in a project.
- [script-test-case.md](references/script-test-case.md): the test base class; read it
  before you write the first test.
- [script-repeat-run.md](references/script-repeat-run.md): the flake counter; read it
  when you must count failed runs of 1 filter.
- [prove-the-test-can-fail.md](references/prove-the-test-can-fail.md): read it for each
  new regression test, guard test or check.
- [flakes-and-isolation.md](references/flakes-and-isolation.md): read it when a test
  passes alone and fails in the suite, or fails only sometimes.
- [leaked-at-exit.md](references/leaked-at-exit.md): read it when the suite prints a
  "leaked at exit" or RID leak line.
- [test-audit-data-echo.md](references/test-audit-data-echo.md): read it to audit a
  suite that copies data tables, or when a change breaks old tests.
- [input-and-route-tests.md](references/input-and-route-tests.md): read it when a test
  drives menus or the player, or a screen shows the wrong state.
- [finish-gate.md](references/finish-gate.md): read it before a unit reports done.
- [pitfalls.md](references/pitfalls.md): the full symptom, cause and fix table; read
  it when a test acts in a way you cannot explain.
