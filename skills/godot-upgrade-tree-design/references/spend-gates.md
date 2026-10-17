# Economy spend gates

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this when players report currency with nothing to buy, or a new node that maxes out at once.

1. Copy the player's save to a temp folder. Read it only.
2. Emulate the game's own unlock and price functions for every node at that save state.
3. List the nodes the player cannot open and the gate that blocks each one.
4. Write testable targets, for example:
   - T1: every currency always has something to buy.
   - T2: no node is bought past 2 levels in the run it opens.
   - T3: the bank after the shop is at most 2 runs of income.
5. Add partial gates (a parent level, not the parent maximum) in code first. With no new data key, the game plays as
   before, so the code change is safe to land alone.
6. Fit the data to T1 to T3 in the balance model and export it.
7. Measure at the player's exact state that the blocked fight can be won after the fix.
8. Audit model rows against tree rows. A row that is in no tree still counts in the model and skews it.

Put gate nodes on the main currency. A gate in a rare currency can hold back a whole branch of another currency.
