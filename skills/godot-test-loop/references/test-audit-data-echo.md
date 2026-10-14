# Test audit: data echo

Tests should assert rules and relations, not copies of data tables or tuned numbers. A test that
recomputes the expected value from the same table proves nothing and breaks on every valid tuning
change. Audit a suite in 4 phases.

1. **Baseline.** Import, then run each folder with `--list-unchecked`. Record passed, failed,
   skipped and unchecked per folder. Count tests per file so the verdict table covers every
   test. An unchecked hit is a candidate, not a verdict; a test can check through `fail()`.
2. **Verdicts.** Ask 4 questions of each test. Can it fail when the game is wrong? Does it check a
   rule and not a copy? Does it survive a valid tuning change? Is it a duplicate? Write 1 verdict
   (keep, rewrite, remove, merge) and 1 reason per test before any edit.
3. **Second pass.** Grep for equality and near-equality asserts whose expected side reads a data
   table, a rules dictionary, or a literal next to a data key. Give each hit its own row. Keep a
   hit when it is test-owned fixture data, a behaviour end point, or a numbered design rule (cite
   the rule). The hit count must equal the row count.
4. **Rewrite.** Replace a pinned value with a relation, an order, a monotonic property, a bound
   read from data, or a player-visible effect. Loop over the data table, not a pinned id list.
   Assert "non-empty and unique" instead of an exact row count, so new content does not break it.
5. **Proof.** For each rewrite, copy the data or code file aside, make the smallest break (for
   example a duration of 0), run the focused test and record the FAIL line. Restore from the copy
   and check with `shasum` or `diff -q`. Never re-type the original by hand.
6. **Close.** Sample keep reasons at random and check them. Check that no rewrite dropped a check.
   Run the parse check and every folder again; counts match the baseline except removed tests.

When a planned change or a merge breaks old tests: reproduce each failure, write the test's intent
in 1 line, and probe real values with a temporary test. Decide "stale test" or "wrong code". For a
stale test, change only the expected values, keep the intent, and add 1 assert for the new rule.
Grep for fake classes that copy a changed signature. A pass with 0 edits is a valid result.
