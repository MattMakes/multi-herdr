# Bot balance loop: rule and effect probes

This file is part of `godot-bot-balance-loop`. Read it when the question is about 1 rule or effect, not 1 encounter.

Use a probe when the question is about 1 rule or effect, not 1 encounter: event counts, live enemies, hits by
source, nodes or draw calls.

1. Write a small `SceneTree` probe, or a wrapper around the autoplay or stress driver. Load game scripts at run time
   with `load()`, so the probe does not depend on the global class cache.
2. Run with a fixed seed and `--headless --fixed-fps 60` under an alarm. Put engine flags before the lone `--` and
   your own flags after it. A `--seed=` after `--` reaches only the script.
3. Count only what the question asks.
4. Build the rule so that a value of 0 or a missing data key gives the old game, and add a switch for the old rule.
   The same probe then measures "before" and "after".
5. Run N states x M seeds x 2 or more steerings, 1 process per run, through the slot runner. Put the log name in each
   row key, so before and after logs never mix.
6. Build a table (median, p95, max, per-state factor). Explain each outlier and split counts by source.
7. For a rule choice, run a control batch on today's rule, then 1 batch per candidate. Edit a temp copy and apply it
   only between batches, because each run loads the script fresh.
8. For a bug report, add a verdict line that separates designed states from real faults. Write the regression test
   before the fix.

Headless frame time is not render time. Count nodes and draw calls headless; time rendering in a window run.
