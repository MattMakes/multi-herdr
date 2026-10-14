# Prove the test can fail

A test that passes before the fix proves nothing about the fix. Prove each new regression test,
guard test or check in one of these ways, and write both results down.

1. **Red first.** Run the new test before the change with a name filter. Read each red message.
   It must be an assertion that fails for the expected reason. A parse error or a missing file is
   not proof.
2. **Patch out, patch in.** If the change exists already, save it as a patch:
   `git diff <files> > <tmp>/change.patch`. Revert it with `git apply -R <tmp>/change.patch`. Run
   the test and record the fail count. Apply the patch again and record the pass count. Use a
   patch, not a stash, so the worktree state stays visible.
3. **Guard tests.** Disable 1 key line of the fix, or plant 1 bad file, for 1 run. Check that the
   FAIL line names the fault. Restore, and run `git diff` to show nothing is left.
4. **Mutants.** Change 1 constant or 1 offset in the code under test. The test must fail. If the
   mutant also fails tests you did not expect, run the real code 3 times to rule out a flake.
5. **Passed on the first run?** Add a temporary print of the measured values to confirm the test
   measures real numbers. Check that the inputs reach the rule at all (a loop that hits the rule
   3 times in 400 proves little). Remove the print.
6. **Rewrites.** Keep a copy of the old code and compare outputs on many random inputs.
7. **Per-frame code over many entities.** Add a cost test with N entities and a budget in ms.

Tests that pass before the change describe current behaviour. Keep them, but do not count them as
proof. Sort each failure into a test fault or a code fault before you change anything. Report
both counts, for example "all 5 fail without the fix, all 5 pass with it".
