---
name: godot-bot-balance-loop
description: Tune bosses, encounters and game rules with scripted bots that play the real Godot game headless, then compare the measured tables against written target bands. The loop writes the bands first, builds a fight tool with several bot steerings (coached, tight, wide, wait, ram), runs fixed seeds with repeatable flags, builds a table of first-try wins, mean tries, resource left and fight length, names each odd result BOT or GAME, and changes 1 lever at a time. It also covers model start states, rule probes, a shared pipeline, wall triage by snapshot replay, a full-playthrough bot, coach text from winning fights and telegraph-to-hit tests. Use it whenever someone wants to balance or retune a boss, check a kill rate or a difficulty band, find out why players or bots stall at an encounter, measure a rule change with encounter bots or a full-playthrough bot, or run a bot playthrough of the whole game, even if nobody says "bot table" or "target band".
---

# Bot balance loop: bot fights, measure tables and wall diagnosis

Target engine: **Godot 4.7**. The Python block uses the standard library only.

A designer can not feel 20 seeds of a boss fight, and a balance model can not see what the real game does.
This skill makes scripted bots play the real encounter headless, over fixed seeds and several player types, and
prints a table that you compare with target bands written before the first run. You change 1 lever at a time and
measure again on the same seeds. It works because the table comes from the real game code, the bands are fixed
before the numbers exist, and every odd number is traced to a cause in the bot or in the game before any data moves.

Read `references/terms.md` for the terms this skill uses: encounter, steering, band, meeting state, resource,
rise, the import and slot runner.

## When to use

- A boss or encounter must land in a target band for first-try wins, tries to the first kill, resource left or length.
- A rule, cap or effect change needs before-and-after numbers from real play.
- A bot or a playtester stalls at 1 encounter, and you must decide if the bot, the economy or the encounter is at fault.
- You need a bot that plays the whole game from a fresh save and reports runs, tries and walls per chapter.
- Players report hits that the warning shape did not show.
- Players stall at a boss, and an in-game hint must show the move that really wins.

## When not to use

- The question is which upgrade order or tree shape is best. Use `godot-upgrade-tree-design`.
- The question is a number inside the offline balance model only. Use `godot-balance-model-data`.
- You need rendered frames to judge a look. Use `godot-visual-verification`; a headless run draws nothing.
- The game has no bot that can play the real scene, and nobody will build one. Build the bot first (see the
  full-playthrough section), or use human playtests.

## Method

1. **Write the bands first.** Write 1 band per encounter class and measure before any run. For example: a mini wins
   60 to 80 % of first tries; a major needs 2 to 5 mean tries with at most 35 % first-try wins; plus a resource-left
   band and a length band. Read the fight: phases, verbs, telegraphs, the base class and its data row. Write a short
   summary of what a player must do. Check: the bands are in a file before the first table exists. Bands written
   after the numbers bend to fit them.
2. **Build the fight tool.** Copy the closest existing fight tool into your own work folder, or write one as a
   `SceneTree` script. Give the copy its own role name, save tag and log folder, so parallel copies never share a
   save. Add options for the start state, seed, steering, try count, variant and output folder. Load the meeting
   state (see "Start states from the model"). Print 1 JSON line per try. Check: the tool parses, and a dry call prints
   1 valid line.
3. **Add steerings for player types.** Add a coached line that follows what the game draws or tells the player, a
   tight line, a wide line, a passive wait line, and a ram line that tries the cheap or off-verb play. Make the bot
   call the game's own helpers (the coach target, a ring size, the run's real speed). Never copy a game constant into
   the tool, because the copy goes stale at the next data change. Check: grep the tool for numbers that also live in
   game data.
4. **Smoke each tool and steering.** Run 1 fight per steering with a trace flag. Check that the encounter rises, that
   every verb fires, and that each log line has every field. Grep the log for `SCRIPT ERROR` and parse errors. When a
   reference table exists, reproduce it exactly first. Check: 0 script errors, and the reference row matches.
5. **Run the "before" table.** Run each steering over a fixed seed list (5 to 20 seeds), up to K tries per seed. Run
   each fight as its own headless process with `--fixed-fps 60 --delta-smoothing disable`, through the slot runner,
   with a time alarm. Split long seed sets into chunks that end inside your tool time limit. Check: every log has the
   full row count; a cut run has fewer rows.
6. **Build the table with a script.** Read only the log lines, never memory. Show first-try wins, mean tries to the
   first kill, resource left at the kill, median length, end reasons and hits by source. Count a win after a refill
   or rescue item as a loss, because the band is about clean wins. The script in `references/script-fight_table.md` does this for
   the log format it documents; copy the block into a file. Check: the seed and try counts in the table equal the counts you ran.
7. **Compare with the bands.** Pool the honest steerings (every steering except wait and ram) and compare the pool
   with the band. Run the coach check: the coached line wins at least as many first tries as the best other line,
   minus 1. Run the cheap check: the ram line wins fewer first tries than the coached line. Check: each band and
   check prints PASS or FAIL.
8. **Trace before you tune.** When a number looks wrong, trace 1 or 2 fights. Log the state every 0.25 s, the
   resource spent by source and the damage per window. Name the cause BOT or GAME. Fix a bot or tool fault before
   any data change. Check: the trace shows the cause in a log line, not in a guess.
9. **Tune 1 lever at a time.** Use a fixed order: local timing and geometry of the encounter, then per-phase caps,
   then a global health multiplier last. Local levers come first because they change 1 encounter only. Sweep on a
   scratch row first. Bracket the value: coarse on 5 seeds, then finer, then 20 seeds to confirm. Restore every probe
   edit after each run. Check: `git diff` on the data source is empty between sweeps.
10. **Confirm near a band edge.** When a result is within about 5 points of a band edge, run a second seed batch and
    pool both before you decide. Check: the decision cites both batches.
11. **Propose, then apply.** Send the before and after tables to the owner of the data. After approval, apply the
    row in the data source, add 1 test per behaviour change, and commit each row alone with its table. A row
    committed alone can be reviewed and reverted alone. Check: each commit has 1 row and 1 table.
12. **Re-measure after merges.** After each merge of the base branch, run the import, smoke each tool again, and
    re-measure the unchanged encounter as the new "before". After a global rule change, re-test only the encounters
    that take a large share (for example 25 % or more) of their damage from the changed source. Check: the new
    "before" table is dated after the merge.

## Start states from the model

Moved to `references/start-states.md`.

## Determinism for headless bots

Moved to `references/determinism.md`.

## Rule and effect probes

Moved to `references/rule-probes.md`.

## One shared measure pipeline

Moved to `references/shared-pipeline.md`.

## Wall triage

Moved to `references/wall-triage.md`.

## Full playthrough bot

Moved to `references/full-playthrough.md`.

## Coach text from a winning bot

Moved to `references/coach-text.md`.

## Telegraph and hit parity

Moved to `references/telegraph-parity.md`.

## Pitfalls

The 6 costliest faults. `references/pitfalls.md` holds all 100 rows.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Replays of 1 save differ by about 10 points | Frame delta smoothing from the wall clock | Add `--delta-smoothing disable` next to `--fixed-fps 60`; restart the work from before the missing flag |
| No encounter rises in any run for a whole day | A tool script named a `class_name` at compile time | Load scripts by path at run time; grep logs for `SCRIPT ERROR`; a smoke must prove key events |
| A wall of hundreds of runs | A bot fault, for example straight lines where the encounter needs curves | Trace 1 fight before you call it balance |
| Rates are only 0 % or 100 % | A bot that never misses | Count distinct outcomes; add 1 shared seeded skill draw |
| The coached line loses | The bot overshot the guide, or followed another shape than the game drew | Read the coach from game code; add a "follow exactly" steering |
| Losses are not resource losses | The run timer or a fight cap ended the try | Log the end reason in each row; compute the run end before the batch |

## Artifacts to build

- **Fight tool**: a `SceneTree` script per encounter type with options for start state, seed, steering, tries,
  variant, output folder and trace; 1 JSON line per try.
- **Steering set**: coached, tight, wide, wait and ram lines in the tool, each calling game helpers, not copies.
- **Band file**: 1 row per encounter class with ranges for first-try rate, mean tries, resource left and length.
- **Run script**: 1 encounter, 1 steering, 1 seed list, 1 headless call with the determinism flags, through the slot
  runner with an alarm.
- **Slot runner**: a wrapper that caps parallel game processes and kills a process at a time limit.
- **Table script**: reads JSON-lines logs and prints the table, the pooled honest row and PASS or FAIL per band and
  check. Copy the block in `references/script-fight_table.md` into a file as a start.
- **States script**: prints 1 JSON meeting state per case from the balance model, in a fixed order.
- **More artifacts**: read `references/artifacts.md` for the skill draw, generator, row setter, wall finder,
  full-run bot, findings and proposal formats, and the telegraph test.

## Related skills

- `godot-upgrade-tree-design`: chooses the tree, the buy policies and the fun bands, and gives the upgrade list for wall
  probes; this skill owns wall diagnosis (snapshot replay and +1 level probes) and measures fights at a state.
- `godot-balance-model-data`: owns the model, the row edit, the export and model-to-game parity; this skill gives it game
  tables.
- `godot-headless-runs`: owns the `-s` tool rules, the flags, process traces and run cleanup.
- `godot-test-loop`: owns the test loop and the proof that a new test can fail.
- `godot-safe-refactor`: proves that a refactor keeps behaviour with a deterministic trace.
- `godot-visual-verification`: owns window captures of telegraphs, coach cards and phases.
- `godot-fleet-game-dev`: owns the proposal and approval protocol, long waits, the slot runner policy and landing.

## References

- `references/terms.md`: the terms of this skill. Read it first.
- `references/script-fight_table.md`: the table and band checker. Copy the block into a file at step 6.
- `references/pitfalls.md`: the full pitfall table. Read it before a long batch or a wall triage.
- `references/start-states.md`: meeting states from the model. Read it before any fight measure.
- `references/determinism.md`: repeatable headless runs. Read it before the first table.
- `references/rule-probes.md`: read it when the question is 1 rule or effect.
- `references/shared-pipeline.md`: read it before many workers measure.
- `references/wall-triage.md`: read it when a bot stalls at 1 encounter.
- `references/full-playthrough.md`: read it before a whole-game bot run.
- `references/coach-text.md`: read it when players stall at a boss.
- `references/telegraph-parity.md`: read it when players report unfair hits.
- `references/artifacts.md`: read it for the artifacts not listed above.
