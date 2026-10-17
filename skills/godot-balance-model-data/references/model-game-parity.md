# Model and game parity

Read this file at Method step 9, or when the model and the game disagree.

The model and the game both predict the same numbers. Treat the model as a tool and the game
as the truth. When they disagree, the measured game table decides.

1. Make the model read the same rows and rules as the game: the same price rule, the same
   parents in the tree, refund rows, per-variant stats. Remove model-only multipliers before
   any tuning. A model that reads its own copy of the tree passes while the game fails.
2. Measure the real game first. Use a bot ladder of real runs with a fixed buy policy, or
   headless measure tools over a matrix (variants x areas x tree states) on a fixed route,
   with 3 or more seeds per state.
3. Find the first gap: a dead end, a stall, a currency the bot never earns. Change the model so
   that it reproduces the gap before you tune any number.
4. Fit the missing model term to the measured ratios. Put the measured table into the model as
   a MEASURED table, with a check that the model stays within a set error of the game.
5. Add a new model term behind a flag. With the flag off, the output stays byte-equal to the
   baseline.
6. Keep a parity test in the game. A fixture generator runs the model on fixed inputs (for
   example every node at level 1 and at max) and writes a fixture file. A GDScript test runs
   the game's port on the same inputs and compares within a tolerance. Hash the fixture before
   and after a regenerate to prove it is stable.
7. When a parity test fails, regenerate the fixture first. If nothing changes, the fixture is
   not stale. Compare the model input rows with the exported rows and find the field that the
   exporter writes but the model does not see. Fix the model, not the export.
8. Put a game rule change into every copy: game code, any GDScript simulator, and the model.
   Grep for every fake and copy of the changed function.
9. Give each fit a guard rule, for example "every price stays between 0.5x and 4x of a
   reference price". A fit can pass every target with absurd values. Code an accepted miss as
   a named limit, so a worse value still fails.
10. Re-measure after a merge that changes game code. Verify a balance result in a temporary
    copy, because a model run rewrites generated files.
