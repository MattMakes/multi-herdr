# Measured-value buy order

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this when you want the game itself, not a person or a weight list, to say which upgrade is worth most at each
shop.

1. Add an off-by-default bot flag that saves a snapshot at each shop, and an "order" buy mode.
2. Write a driver whose scoring is pure functions with unit tests.
3. Prove replay quality: replay 1 snapshot and compare it with the real next step. If replays are exact, use 1
   replay per candidate. Else use R replays and the mean.
4. At each shop, replay the next run (or the next boss try) once per candidate with +1 free level of it.
5. Score the gain (for example boss health removed plus a haul ratio). Buy by the scores through the game's own shop.
6. Record the position of each upgrade's levels in the buy order. Play 3 seeds and compare with the baseline.
7. Freeze the mean buy positions of 3 seeds as a fixed order policy. Play it on 3 seeds to compare.

Read saturated rows as ties: when 1 level kills the boss, every candidate scores the same. Buys can come in blocks as
a tool artifact; say so when you read the order. Plan the wall time for the real slot queue, not for 1 free core.
