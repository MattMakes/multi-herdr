# Bot balance loop: wall triage

This file is part of `godot-bot-balance-loop`. Read it when a bot fails an encounter try after try, or you must find which single upgrade passes a wall.

A wall is an encounter where a bot fails try after try. Decide whether the bot, the economy or the encounter is at
fault before you change data. Many walls turn out to be bot faults.

1. Read the end state of every lost try: encounter health, time left, damage dealt and end reason. If every try ends
   at the same high health, suspect the bot first.
2. Run a control on the same seeds as the earlier table. Stop if it does not match within noise.
3. Copy the wall save, and trace 1 or 2 lost fights from the copy. Log the target, phase, open flags, hits by source,
   resource per second, damage windows and distance at the rise.
4. Read the game rule that deals damage and compare it with what the bot does. Make the bot call the game's helper.
5. Check the usual bot faults: an arrive radius smaller than the turning circle, the staging distance and lead time
   before the rise, no dodge of attack areas, and wait rules that lock a chapter.
6. Name each cause BOT or GAME. Fix a BOT cause in the tool only. Re-run into a new log folder; keep the old folder
   as the "before".
7. Isolate a cause with test-only switches that each remove 1 factor (a resource floor, a hazard hold, a perfect
   cut), or turn off 1 bot behaviour. Run the same seeds per switch, 1 switch at a time.
8. Split a logged value into its causes, for example contact hits against bullet hits. Add small counters that turn
   a question into 1 number per fight.
9. To find which single upgrade passes a wall, replay the try from its snapshot as a baseline. The baseline must
   equal the original try (same seed, same state file, same flags). Then replay with +1 level on each upgrade, 1
   upgrade per probe, and rank the probes by the change in win count or end health. The upgrade list and the buy
   policy come from the caller (for example `godot-upgrade-tree-design`); this section owns the replay and the ranking.
10. Rebuild the encounter states from the bot's own purchase log, and probe levers on those reachable states. Patch
    probe rows locally and restore them before any commit.
11. Report the cause with numbers and 1 fix per factor (F1, F2, ...), measured alone and as a set. If a new player
    can make the bot's mistake, also report what the game shows to teach the move.
