# What-if and lever search

Read this file before you tune pacing or economy levers in the model.

Use the fast model to test pacing and economy changes before you touch game data.

1. Save a baseline output of every module and profile. Write 1 cause per failing target row.
2. Build a beat map: each run, its minute, and each new beat (a boss, an enemy type, a find,
   a new tree wing, a new rule). Set targets, for example the run of the first boss and the
   longest stretch with no beat.
3. Write what-if scripts outside the model folder. They import the model and take
   `row.field=value` overrides or a JSON override file. They print the target line and the rows
   that move. A what-if never writes the export files.
4. Run the base, then each override alone, then the candidate set. Report which single
   override causes each FAIL. Join the overrides that pass alone and run the joined set again.
5. Before a search, measure each approved lever alone against all targets. A lever that does
   not move the output is limited by something else, for example a time cap and not health.
   Find that limit first.
6. For a fit, write a runner that copies the whole model folder to a temp dir, applies a lever
   set, and saves the report. A score script adds a penalty per FAIL key plus the distance of
   each chapter length from its target. Build a fresh model state for each candidate.
7. Run a coordinate search, then a joint grid over the 2 or 3 coupled levers. Search coupled
   values together; one at a time lets an early value set the floor for a later one. Tune rows
   at run boundaries last, because a buy at a boundary makes the fit jump.
8. Round the best fit to plain numbers (2 or 3 significant digits). Write only the approved
   constants into the source, then run the main Method of SKILL.md from step 5. Check that later chapters
   stay within their length band (for example plus or minus 15 %).
9. Re-measure the new states in the game over 2 or 3 seeds. Update the MEASURED table and the
   doc tables.
10. When no approved lever can pass, do not force numbers. Compute the exact range that each
    rule allows, show that the ranges do not overlap, and hold the edit. Report the levers you
    tried and the closest set. Print a value that needs unbuilt data as PENDING.
11. When the design owner accepts a target miss, add a named, bounded constant for that row,
    so the model prints the decision on every run.

Prefer slow gains plus spike moments to a smooth slow curve. A smooth curve can turn one
chapter into dozens of farm runs (see `godot-upgrade-tree-design`).
