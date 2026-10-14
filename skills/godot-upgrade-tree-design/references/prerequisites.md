# Prerequisites

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Check each one first. Report a missing one as a blocker.

1. **A stat pipeline.** Upgrades are data rows that add `value x level` into named stats, and 1 function builds a
   run's stats from the owned levels. The diagnostic tree hooks in there.
2. **A bot that plays the real game.** It plays whole runs headless through the real scenes, reaches the shop, and
   takes flags (a policy, an out folder, a seed).
3. **Determinism.** The same save and seed give the same run. Prove it: replay 1 save 3 times. In Godot, add
   `--delta-smoothing disable` beside `--fixed-fps 60`; else the frame delta follows the wall clock.
4. **Events.** The game emits run start, run end with the haul, boss appear, kill and leave, and class unlock.
5. **A separate save.** The dev mode never writes the player's save.
