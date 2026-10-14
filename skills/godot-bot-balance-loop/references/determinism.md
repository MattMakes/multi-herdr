# Bot balance loop: determinism for headless bots

This file is part of `godot-bot-balance-loop`. Read it before the first table. 2 runs of 1 seed must give 1 result before you trust a table.

A table is only as good as its repeat. Make 2 runs of 1 seed give 1 result before you trust any comparison.

1. Add `--fixed-fps 60` and `--delta-smoothing disable` to every headless Godot call of every bot tool. Godot smooths
   the frame delta from the wall clock even at a fixed frame rate. Without the second flag, 1 save can replay with
   about 10 points of spread.
2. Seed every random draw (weather, spawns, skill draws). Turn off wall-clock effects in the tool, for example a hit
   pause that restores the time scale on a real-time timer.
3. Save the tool's own state (try count, seeds, coach level, skill-draw seed) in a state file next to every snapshot.
   Without it, a replay plays try 1 again.
4. In Python generators, iterate in a fixed order. Never iterate a set for output.
5. Run 1 case 2 to 4 times and compare the outputs by hash. Replay 1 snapshot 3 or 4 times and record a key measure.
6. If runs still differ, record the spread with more repeats. Boss fights can still vary with both flags (one
   observed suspect is worker-thread timing under load). Use medians of 3 replays and win counts over 10 or more
   fights, and make no claim smaller than the spread.
7. A bot that never misses gives only 0 % or 100 %. Count distinct outcomes. For example, 40 seeds can
   give only 5 distinct fights (a sample value). Add 1 shared seeded "skill draw" that adds misses, and use the same
   draw in every tool.
