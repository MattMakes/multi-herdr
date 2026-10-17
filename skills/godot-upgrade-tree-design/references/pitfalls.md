# Pitfalls

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Every theme bought the same first spokes | A `level / weight` score ties at level 0 | Score `(level + 1) / weight`; break a tie by the higher weight |
| No buy for hundreds of runs | Weight 0 on more than about 20 % of spokes; the cap share can never be met | Keep weights above 0, or report the deadlock as a finding of the cap rule |
| A spoke takes levels with no effect | A game cap holds the derived stat | Close a spoke when 1 more level changes no derived stat; count it as at the cap; log the closing level |
| About 100 levels sold in 1 shop | 1 price curve plus an income jump at a class unlock | A stage multiplier per unlocked class, fitted per chapter |
| A clean trace, but no boss ever rose | A tool script failed to compile; the trace keys still filled | The smoke check proves key events; a `SCRIPT ERROR` line makes a try invalid |
| Every calibration try was invalid | A tool script did not compile | The same marker; fix the compile before more tries |
| A chapter ends too fast at every price | A metric is missing (for example shop spend) | Stop and add the metric before you go on |
| Replays of 1 snapshot drift | Frame delta smoothing, or bot state not in the snapshot | `--delta-smoothing disable`; save the bot's own state (try count, skill seed) with the snapshot |
| Boss fights still vary after both fixes | Cause not confirmed; threads timed by machine load are a suspect | 3 seeds per policy, 3 replays per probe with the median, no claim smaller than the seed spread |
| A finalist mean mixed 2 data versions | A peer change regenerated the data file between rounds | Store the data file each round played; compare it; score old and new seeds apart |
| A fast policy looked best | Its trace had a run cap, so a different stop rule | Rank only traces with the same stop rule; the report warns |
| A last-chapter rule counted an old kill | The kill was before the chapter start | Count only kills after the chapter begins |
| Run numbers became `12.0`, seeds printed `0.0` | Godot's JSON parser returns floats | Cast counts to integers at load, in the game and in the report |
| A new sweep wrote into an old trace folder | A saved path beat the explicit argument | An explicit argument or environment value always wins |
| Buys before the first run were lost | A restarted bot had no current run row | On load, make the last finished run row current again |
| Files kept changing after a test | Signal handlers outlived diag mode | Check `active` at the top of every handler |
| Once-only events fired every run | A beaten mini boss came back each run | Gate the event on the game's own first-kill flag |
| The report test could not import its module | The test dir was not on the import path | Add the test dir to `sys.path`; delete `__pycache__` before review |
| The contract and a test disagreed on valid stats | The rule was not written | Ask; write the rule into the contract |
| A "wall" in the diag tree | It came from the diag tree only | Check the encounter on the shipped tree before you report a game fact |
| 2 events always fell in the same run | One is the reward of the other | Check that before you claim a cause |
| A skill wall was really a resource wall | The bot always ran out of fuel or health first | Read the failure reasons before you exclude a boss or plan a coach |
| All panel playthroughs failed | zsh did not split a list variable | Use an array or an explicit list; delete the failed rows |
| A wait loop ended at once | Its process pattern matched nothing | Test the pattern with `pgrep -fl` first |
| A `pkill -f` killed your own monitor | The pattern matched the monitor command | A narrow pattern such as `tune[.]py --configs` |
| A task notice came, but the play still ran | The notice came from a short wrapper | Wait for the watcher of the real job |
| Progress showed 0 for a long time | The log printed in job order | Count finished result files |
| The notes held wrong claims | Claims written from memory | Recompute each claim from the result data before you report |
| A pinned test failed after a cap change | The test asserted the old value | Update pinned tests with the generator change |
| A sandboxed agent could not play | It could not write the save folder | Give bot runs to an agent that can write the save folder |
| Probes took close to 1 hour per seed | Shared slots, high load | Plan wall time for the slot queue |
| A lean build failed | The wing gave power and income but none of the run resource (for example fuel) | Report the cause; cost cuts alone do not fix it |
| 2 captures were the same | The upgrade changed only hidden numbers | Report a feedback finding |
| A capstone did nothing of its own | No own effect in code | Report it |
| 2 audit runs corrupted each other | They shared 1 save slot | A slot argument per run |
| Audit runs ended in seconds after a merge | A stale class cache | Import the project first |
| A cue layout cost too much in 1 frame | All cues were timed together | Time only the new part |
| A rare-currency gate blocked a whole branch | The gate node was in another currency | Move gate nodes to the main currency |
| A coach was planned for an unwinnable fight | The bot lost every try with an empty resource | Measure first; a coach cannot teach a fight the resource always ends |
| Nodes counted only in the model | They were in no tree | Audit model rows against tree rows |
| A search returned nothing | The search tool did not support the pattern | Test a search on a known hit before you trust an empty list |
| Tester traces showed "seed 0" and "skill strong" | No human flag in the trace | Add the flag |
| A tester build ran in script mode | It started from a secondary working copy | Run the play command from the main checkout |
| The telemetry held 1.7 s of test data | The log was not from real play | Check the log; use the video |
| The report grouped sessions, not testers | Logs were grouped by file | Group by tester folder |
| Pass marks had no telemetry event | The question was not observable | An observer check on the same sheet |
| Interview asked about missing systems | The questions were generic | Ask only about this build |
| The model paid strips, the game paid areas | The model and the game differed | Note the gap for the model owner |
| A redesign reversed an earlier decision | Nobody checked the decision history | Name the decision with its date; offer fixes |
| An answer box held a worker report | Pasted text looked like direction | Only the owner's own words are direction |
