# Flakes and test isolation

A test that passes alone and fails in the suite almost always reads shared state. Find the state;
do not add waits.

1. Run the file alone and in the full suite. Then run it 20 times with 1 log per run
   (`repeat_run.sh`: copy the block in [script-repeat-run.md](script-repeat-run.md) into a file)
   and count the failed runs.
2. List the shared state: fixed `user://` paths, static vars, autoloads, live signal handlers and
   lambdas, node meta, timing budgets, and global input state.
3. Give every file that a test writes its own path under a per-process folder (`scratch_path()` in
   `test_case.gd`, see [script-test-case.md](script-test-case.md)). Give each tool run its own save. Turn telemetry and analytics off in
   test runs. Why: 2 suites from 2 worktrees share 1 user folder.
4. Give each stateful autoload a reset function. Call it from the runner before each test file
   (`reset_tests.gd`), and in the setup of any test that opens a screen that reads it. Save
   global state in `before_each` and restore it in `after_each`. Clear lambdas in `after_each`.
5. In signal handlers, check an `active` flag. Do not call a reset that fires signals with side
   effects. Load an empty save, then the target save, instead.
6. For a flake, form 1 hypothesis and add 1 diagnostic print of expected against actual. If the
   cause is a selection rule (for example "the first match in dictionary order"), fix the rule
   and add a test for that case.
7. For timing tests, wait on timers, not frame counts; headless frames run faster than real time.
   Wait for tweens to end before you assert. Scale a time budget by a measured load factor, and
   prove the test still fails with a deliberate busy loop.
8. To prove isolation, run 2 full suites at the same time from 2 copies of the project. A FAIL
   in only 1 run shows a shared file.
9. Keep a short, named list of allowed flaky tests. Allow no other failure. A flake that repeats
   gets its own fix, confirmed by 20 green runs in a row plus 3 green full suites.
10. A failure with a state cause is not a flake. Compare with the base commit first.
