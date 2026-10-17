# Bot balance loop: the full pitfall table

Read this before a long batch, a wall triage or a fan-out. Each row is a fault that a real project hit. The rows are
grouped by the method section they belong to, in `SKILL.md` or in a reference file. Numbers are examples from 1 project, not targets.

## Summary: the costliest faults

These 12 rows are the summary that `SKILL.md` shows in part. The sections below hold the full table.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Replays of 1 save differ by about 10 points | Frame delta smoothing from the wall clock | Add `--delta-smoothing disable` next to `--fixed-fps 60`; restart the work from before the missing flag |
| No encounter rises in any run for a whole day | A tool script named a `class_name` at compile time | Load scripts by path at run time; grep logs for `SCRIPT ERROR`; a smoke must prove key events |
| A wall of hundreds of runs | A bot fault, for example straight lines where the encounter needs curves | Trace 1 fight before you call it balance |
| Rates are only 0 % or 100 % | A bot that never misses | Count distinct outcomes; add 1 shared seeded skill draw |
| The coached line loses | The bot overshot the guide, or followed another shape than the game drew | Read the coach from game code; add a "follow exactly" steering |
| Parallel runs corrupt each other | Tool copies share 1 save role | 1 save tag per copy and per run |
| A batch mixes 2 rules | An edit to the tool, scene or data during the batch | Stop, edit, restart; merge the base only between batches |
| Runs use the wrong steering or an empty argument | zsh does not split an unquoted variable into words | Use explicit arguments, arrays or `bash`; print the knob values in each row |
| A lever does nothing | A derived value moves with it, or a per-window cap binds the result | Read how the data derives values and which rule caps the window first |
| The model and the game disagree | The model misses a game rule | The game table decides; report the gap to the model owner |
| Losses are not resource losses | The run timer or a fight cap ended the try | Log the end reason in each row; compute the run end before the batch |
| Old tables mislead | The economy or a rule changed after the measure | Make the meeting states again and re-measure |

## Fight tables

| Symptom | Cause | Fix |
| --- | --- | --- |
| Rates are only 0 % or 100 %; for example, 40 seeds give only 5 distinct fights (a sample value) | A bot that never misses | Count distinct outcomes; add 1 shared seeded skill draw; do not let each unit invent its own |
| The coach check passes only after a change to the tight bot | The tight line was made weaker to pass | Keep the real player line; add a wide line as an extra steering |
| The coached line loses | The bot followed a ring while the game drew a lane, or overshot the guide dots | Read the coach target from the game code; add a "follow exactly" steering |
| A tight line closes 0 loops | It switched between 2 radii | Hold 1 tight radius for the whole fight |
| A steering behaves like the old game after a merge | The tool copy did not follow a new game setting | After each merge, read what changed in the code the tool drives; smoke again |
| A timeout or speed is wrong | The tool kept its own copy of a game value (ring size, lap speed, miss timeout) | Read the game constant or the run's measured speed at run time |
| Every row of a batch is empty or invalid after a merge | A stale class cache after a new class landed | Import before every measure; grep logs for `SCRIPT ERROR` |
| Later fights of 1 batch measure another rule | An edit to the tool, the scene or the data during the batch | Stop the batch, edit, then start again; merge the base only between batches |
| Runs use the wrong steering, an empty argument, or a probe row stays changed | zsh does not split a variable into words | A batch script with explicit arguments, arrays or `bash`; restore rows with a `trap`; print knob values in each row |
| A data edit changes rows you do not own | A `sed` replace matched more than 1 row | An assert-guarded replace with exactly 1 match; diff the export at once |
| A row tool runs, but the fights use the old row | The row setter ran outside the repo root | Run row tools from the root; discard runs made on the wrong row |
| Parallel copies corrupt each other | Tool copies share a hard-coded save role | 1 save tag per copy and per run |
| Losses are not resource losses | The run timer or a fight cap ended the try | Log the end reason per row; compute the run end before the batch |
| Results near the fight time cap jump per seed | Fights that end near a cap are chaotic | Pool runs and report counts, not single values |
| A forced-loss capture still wins; a HUD bar disagrees with the log | A refill saved the try | Spend the refill in loss mode; log the resource at each event |
| The model says 2 % and the game says 100 % | The model misses a game rule | The game table decides; report the gap and the missing rule to the model owner |
| A lever goes the wrong way, cancels out, or does nothing | Not monotonic; a derived value moves with it; a per-window damage cap binds | Measure each value; read how the data derives values and which rule caps the window before you pick a lever |
| A table cell is wrong | It was written from memory | Copy every number from the log or the export |
| Seeds or logs do not match the table | A seed key encodes seed x 10 + try; log names have part suffixes | Decode keys in the table script; list the log folder before the table call |
| Old tables mislead | An economy or rule change landed after the measure | Make the meeting states again and re-measure |
| Headless and window results differ | Different frame timing | Treat window runs as captures; keep the headless table as the measure |
| A run is cut, or moves to the background | It passed the tool time limit | Split by seed; check every log for the full row count |
| A long run writes into the log of a newer run | 2 runs share 1 log tag | A unique log tag per run; stop old runs first |
| The bot sits inside the arena at the rise, and the encounter moves away | The approach is wrong | Trace the approach; stage outside the arena |
| A data edit is denied by a permission rule | The rule protects shared data | Send the change as a proposal with its table; do not work around the denial |
| The ram line wins most fights | Cheap play (for example contact damage) is too strong | Report it as a design question with options |
| Work is lost when a rule change lands under a running sweep | The sweep kept going | On a pause order, revert trial rows, commit tools and old-rule tables, and wait |

## Start states

| Symptom | Cause | Fix |
| --- | --- | --- |
| Tables are wrong after a merge | The meeting run moved | Run the states script again after each merge |
| An old tool meets the wrong encounter | A major encounter needs its gate encounter killed first | Add the gate kill to the state |
| A default level list uses ids that do not exist | The tree changed, or holds ids in a group | Check the game tree data before you add or drop ids |
| Some states never fight | The run ended before the encounter rose | Compute the run end per state; mark "no fight" apart from "loss" |
| A scratch copy of the model fails | Model scripts read the game folder by a relative path | Link the game folder into the scratch copy |
| A tool script fails to compile | It names a `class_name` class | Load the script at run time |

## Determinism

| Symptom | Cause | Fix |
| --- | --- | --- |
| 6 replays of 1 save give 71 to 82 % health left | Frame delta smoothing from the wall clock | `--delta-smoothing disable` with `--fixed-fps 60`; restart the work from the step before the flag was missing |
| Boss fights still vary with both flags (for example 23 s to 55 s) | Not confirmed; worker-thread timing under load is a suspect | Medians and win counts; treat health gaps under about 15 points as noise |
| A replay plays try 1 again | The tool state did not travel with the snapshot | A state file beside each snapshot |
| Income moves by up to 25 % | Unseeded random draws | Seed every draw |
| Captures do not compare by pixels | Shaders use wall-clock time | Hold time-driven effects still for pixel compares |
| Exit-leak lines come and go | Noise, or a real leak | Call them noise only after 2 more runs show it |

## Rule probes

| Symptom | Cause | Fix |
| --- | --- | --- |
| 1 batch mixes 2 rules | Each run loads the game script fresh, and the script changed mid-batch | Edit a temp copy; apply it between batches |
| The table mixes before and after logs | 1 row key for both | Put the log name in the row key |
| Window runs take 154 s and sometimes hang | Real-time rendering | Headless with `--fixed-fps 60` (35 s); retry hung seeds with a longer alarm |
| `--seed=` does nothing | It came after the lone `--` | Engine flags before `--`, script flags after it |
| A probe of 40 runs passes the tool time cap | Too many runs in 1 call | Run in the background and wait for the exit notice |
| A preload order error before any measure | Gameplay scripts loaded at parse time | Load them at run time |
| Frame times look wrong | Headless frame time is not render time | Count nodes and draw calls headless; time rendering in a window |
| A designed pause looks like a frozen-object bug | Designed holds counted as faults | Count designed holds apart; make the designed state read as intended |
| Leak lines appear in probe runs | The source is not known | Check whether the suite prints them too before you report them |

## Shared pipeline

| Symptom | Cause | Fix |
| --- | --- | --- |
| A patch names a field the tool does not log | The tools differ | Grep each tool for its logged fields before the patch |
| A copied tool fights the wrong encounter | The copy lacks the case seam | A pick guard and the seams in every copy |
| A long batch outlives its monitor | The monitor had a fixed time limit | Re-arm the monitor; check progress by log line count |
| A claim names an item at a state where it is at level 0; a log count is wrong | The report was not checked | Check state claims before the report; send a correction at once |

## Wall triage

| Symptom | Cause | Fix |
| --- | --- | --- |
| A wall of 296 runs | A bot fault: the bot drove straight lines and the variant needs curves | Trace 1 fight before you call it balance |
| The player orbits a waypoint for 6 s | The turning circle is wider than the arrive radius | Arrive radius above the turning circle |
| The bot loses 62 % of its resource in 1 attack | It drove through an attack area | Add a dodge |
| A chapter is locked for 32 runs | A wait rule for the encounter never ends | Wait-streak logs, a lower wait floor and a wait cap |
| The bot circles a moving encounter at its spawn point | It used the spawn, not the current position | Follow the game's coach target every frame; route to where the encounter rests |
| A batch used an old emulation of a mechanic | Not the real code path | Run again on the real path |
| A stated cause is wrong | 2 changes in 1 run, or no check | 1 change per run; verify with a log line before the claim |
| A mean path radius is too large | It included the long approach | A sampled median in the fight phase only |
| A drain looks like area damage | It was body contact | A contact bucket in the hit log |
| A baseline replay gives 71 % where the original gave 74 to 82 % | Missing determinism flags | See the determinism table |
| No save near the wall exists | Snapshots were not kept | A snapshot per encounter kill and per try |
| A model column reads as a rate | Units were not checked | Confirm column units with the model owner |

## Full playthrough

| Symptom | Cause | Fix |
| --- | --- | --- |
| No encounter rises in any playthrough for a whole day | A `-s` tool named a game class at compile time | Load scripts by path at run time; scan logs for compile errors |
| Replays start at try 1 | The tool state did not travel with snapshots | A state file beside each snapshot |
| Progress is lost at a crash | The state file was written only at segment end | Short segments |
| Each segment restart loads the snapshot again | The resume flag is read every segment | Resume on the first segment only |
| The monitor reports failures on normal segment switches | A filter `rc=[1-9]` matches the switch codes | Exclude the switch codes from the filter |
| A stop script does not stop at the try cap | A shell fault in the script | Test stop scripts; treat a try cap as a probe, not a verdict |
| Late chapters look easy | Runs after a bot wall are over-levelled | Replay from the chapter snapshot; keep the over-levelled branch as a flow check only |
| A per-process record repeats after a segment restart | A segment restart acts as a game restart | Store such records in the state file |
| The bot starts far from the encounter | It ignored the warning marker | Move to the warning marker that a player sees |
| Frame times are distorted | In-run captures stall frames | A measure flag with no captures, vsync off |
| A hidden window stops with no error | The OS throttles a hidden background window | Watch the output; let it finish, or run headless |
| A fresh save runs dry before late events | No mid-game start | A mid-game start option |
| The run worker gets conflicting orders | 2 leads direct it | 1 lead directs the run worker |

## Coach text

| Symptom | Cause | Fix |
| --- | --- | --- |
| The hint never shows for this player | A past kill with the same move turned the coach off | Read the coach level rules first |
| Coached and tight lines give the same rows | Decoys stood all fight | Compare verbose traces before you name the winner |
| The coach is quiet for a known verb at a new gate | No gate line per encounter | A gate line per encounter |
| A coach was planned for a fight the bot lost 50 of 50 times with an empty resource | Nobody measured first | Measure first; a coach can not teach a fight that the resource always ends |

## Telegraph parity

| Symptom | Cause | Fix |
| --- | --- | --- |
| Late encounters fire 0 bullets | Field bullets counted against the encounter cap | Count only the encounter's own bullets |
| A hold-fire fix makes an early encounter too easy | No named threshold | A named threshold constant with a design reference |
| A telegraph warns on the safe lane | The shape is wrong | 2 arcs with the safe lane clear, plus a test |
| A ring attack draws an arc telegraph | The telegraph and the hit read different data | The shape test |
| A repeated attack can pass the bullet budget | Spawns are not capped | A budget test; cap spawns by the remaining budget |
| An assertion passes with no data | The fixture has no live content | Use a real field window in the fixture |
| The encounter sprite covers the player on a pass | Draw order or path | Report it with the capture |
