# The fun analysis

`scripts/pw_fun.py` computes the fun sheet of a model. The proof sheet (`references/proof-sheet.md`) says if a design is stable and on time. A design can pass each proof row and still be dull: 3 rebalanced test games were stable and flat. The fun sheet says where the design is likely dull, why, and 1 change to try. It is like a static analysis of code: it finds known faults, and it cannot prove that a game is good.

The set has 24 metrics, FM-1 to FM-24, in 3 tiers. The script computes tier 1 (FM-1 to FM-12). Tier 2 (FM-13 to FM-20) needs new outputs of the simulator or the scan, and later tickets add them. Tier 3 (FM-21 to FM-24) needs a play test with people. The research is in `docs/research/fun/` of the lab repository: `fun-metrics.md` (the metrics, their bands and the evidence), `fun-sheet-spec.md` (the design of the tool) and `tuning-controls-and-play-data.md` (the controls C1 to C16 and the play log).

## Contents

1. Limits
2. Command line
3. Terms
4. Rows: the statuses, the marks, the counting rows FM-1 to FM-8, the advice rows FM-9 to FM-12, the guards
5. Tier 2 rows: FM-13 to FM-20 and what each needs
6. Tier 3: the play test protocol, FM-21 to FM-24
7. Result
8. The control of each row
9. Tune by hand with the tuning panel, and give the setting back as targets
10. Outputs

## Limits

Read these limits before any value of the sheet.

1. Each row is a proxy. It counts events, sizes and times in a model or a simulated run. It does not measure fun. A sheet with no finding does not prove that a game is fun.
2. The result is advice with evidence marks, not a proof. It never changes the gate of the proof sheet, and it does not set the exit code.
3. No band comes from play data of an incremental game. Most bands are "no source": they are the judgment of a researcher. The sheet shows the mark of each band beside the value.
4. The simulated player is perfect and has no boredom. It buys at once and never misses a button. Only a play test tells whether a player wants a reward, whether a surprise surprises, whether the theme carries the mechanics, and why a player stops.
5. The pair tests use models of 3 games. That is too few for a statistical test. A row that "separates" agrees with the known complaints on these models only.

## Command line

```
python3 scripts/pw_fun.py <model.json> [--build <model-built.json>] [--md FILE] [--json FILE]
```

- `<model.json>`: a model of `references/model-schema.md`.
- `--build FILE`: the model of the game as built. The sheet then computes the build too. It shows the value and the status of the build beside each row, and it marks a row "worse than built" when the design row is out and the build row is not, or when both are out and the design is further from the band. FM-3 also gets the drift.
- `--md FILE`, `--json FILE`: write the sheet. With neither, the script prints the Markdown. With 1 of them, it prints 1 result line.

Exit code: 0 when the sheet is computed (each result, "likely dull" too), 2 for a usage error, a missing file or a model error.

A model with no `game.play_pattern` (a model of version 1) is computed as well. The sheet plays it with `active_greedy` and uses the bands of the `continuous` pattern, and a note in the sheet states this. Reason: the known complaints are on such models (`tests/models/moss-works-built.json`), and the pair tests need their values.

## Terms

- **Active run**: 1 run of `active_greedy` of `pw_players.py`, the run of the proof sheet. The player is present all the time and decides each 1 s. The horizon is 3 x `game.target_hours`. Each row reads the events of this run and the model.
- **Length E**: the real seconds to `game.targets.hard_end`. When the run does not reach it, E is the time of the last axis step. When the run does not reach the axis end either, E is the last event, and the sheet has a note. Events after E do not count.
- **Economy currency c\***: the currency that the most `add`, `pct` and `mult` effects of the purchases target. On a tie, the first in model order wins. The sheet does not use `pw_analysis.main_currency`, because it picks `seal` for Kiln n Kin.
- **Target of a line**: the currency of the first `add`, `pct` or `mult` effect of the line. The target `all` is c\*.
- **Hand purchase**: a purchase event with no `auto` key. **Press**: 1 distinct pair (time, line) of hand purchases. A buy-max button buys all levels of 1 line in 1 press.
- **Content element**: the first purchase of a line (by hand or by the auto-buy), the first time of a milestone, or the first reset. An axis step is not a content element. This is the rule of PC3.
- **Unlock**: a content element or a new ladder step. This is the rule of PO11 and PC4.
- **Beat**: an event with a weight of 2 or more that is not a reset and not an auto purchase.
- **Shift**: the first event of a system that has no event in the first 60 s, or the first reset. Shifts within 30 s of each other are 1 shift.
- **Gaps of a set of times**: sort the distinct times with 0 and E added. Each difference of 2 neighbours is a gap.
- **Count step**: a purchase that brings a line with the model key `count_steps` (`{"every": n, "mult": m}`) to a level that is a multiple of `n`. The output of the line takes the factor `m` at that purchase (`references/model-schema.md`, "count_steps"). The level of a purchase is the number after the last ` L` in the name of its event.
- **Block**: the `n` levels of a line with count steps up to and with 1 count step. **Block gain**: the income of the target after the count step against the income before the first level of the block. A block that starts before the run has no block gain. This is the tested gain of the row PV (`references/proof-sheet.md`, rule 7).

## Rows

Each row has these fields: `id`, `name`, `tier`, `kind` (`counts` or `advice`), `value` with its `unit` and `value_text`, `sub` (the sub-values), `band` with `band_values` and `band_mark`, `status`, `reason`, `change`, `controls`, `where`, `guard` and `evidence`.

The status is 1 of these:

| Status | Meaning |
| --- | --- |
| `ok` | the value is in its band |
| `out` | the value is outside its band |
| `not computed` | an input is missing; the field `missing` names it |
| `not applicable` | the row does not apply to this game; the reason says why |

The band mark is 1 of: sourced, measured, inferred, no source, designer setting. A band of `game.targets.fun` replaces the default band of its row, and its mark is "designer setting". Example: `"fun": {"FM-3": {"min": 0.5}}`. The keys of each band are in the table below.

The controls are C1 to C16 of the tuning document: C1 opening pace, C2 first price, C3 unlock spacing, C4 stage wall, C5 game length, C6 upgrade punch, C7 growth pressure, C8 milestone pay, C9 run length, C10 reset weight, C11 first reset, C12 tend boost, C13 bank, C14 auto-buy start, C15 garden rhythm, C16 feedback. A change that no control can make says "content".

### Counting rows

| Id | Name | Band keys and default (mark) | Controls |
| --- | --- | --- | --- |
| FM-1 | Content drought | `max_h1` 10 min, `max` 20 min (no source) | C3, C5, C15, C1, C2 |
| FM-2 | Felt purchase share | `max` 0.2, `step` 0.05, `model_max` 0 (no source; the step is inferred from the Weber fraction) | C6, C7 |
| FM-3 | Line character | `min` 0.7, `min_drift` 0.5 (no source) | none |
| FM-4 | Largest gap between jolts | `max` 20 min (no source) | C6, C8, C7 |
| FM-5 | End arc | `max` 0.1 (no source) | C5, C3 |
| FM-6 | Chore length and automation timing | `max_presses` 150, `min_hand_share_h1` 0.3 (no source) | C14 |
| FM-7 | Verbs for each hour | `min` 3 (no source) | C14, C15, C9, C12 |
| FM-8 | Comeback ratio | `min` 0.3, `max` 0.7 (no source) | C9, C10, C11, C4 |

**FM-1 Content drought.** Inputs: the content elements of the active run, and E. Procedure: take the gaps of the times of the content elements in (0, E]. Find the largest gap on [0, min(1 h, E)] and the largest gap on [0, E], each with its start and end. The row is out when the hour 1 gap is above `max_h1` or the whole gap is above `max`. The value is the whole gap in minutes. Sub-values: the count of empty 10-minute windows of unlocks (ceil(E / 600) windows), the CV of the unlock gaps, the share of unlocks within 30 s of the unlock before, and the least-squares slope of ln(gap) against ln(midpoint) for the unlock gaps that start after 300 s (nothing for fewer than 4 points). Guards: FM-2 and FM-3, because many tiny lines can fill each gap.

**FM-2 Felt purchase share.** Inputs: the hand purchases of the active run, the income of each currency before and after each one, and the model. Procedure: for each hand purchase whose target income is above 0 before it, gain = income after / income before on the target. The value is the share of gains below 1 + `step`. The model part is the count of lines whose first `mult` is below 1 + `step` or whose first `pct` is below `step`. The row is out when the share is above `max` or the model part is above `model_max`. Sub-values: the share below 1.10 and the median gain less 1. With no hand purchase and a model part of 0, the row is not applicable. **Count steps**: a line with count steps counts each count step by hand with its block gain, and leaves its other levels out. The reason names these lines. Sub-values: `count_steps` (the steps counted) and `block_levels` (the hand levels left out). Reason: the player buys such a line for the step, and 1 level adds a small part of the step (the buyer assumption of `model-schema.md`).

**FM-3 Line character.** Inputs: the purchase lines of the model, and with `--build` the lines of the build. Procedure: 2 lines are cost clones when they have the same cost currency, cost formula, base cost (2 significant digits) and cost growth (3 decimals). 2 lines are effect clones when their sorted lists of (kind, target, value to 4 decimals) are equal. Join the clones into groups. Distinct share = 1 - (lines in a group) / lines. A line with the key `family` (a list on purpose, for example 12 glazes) leaves the distinct share. The level factor of a line is its first `mult` value, or 1 + its first `pct`, else the step factor `m` of its count steps. **Count steps**: the count steps (`n`, `m` to 4 decimals) are part of the effect list of a line, so 2 lines with the same effects and different count steps are not effect clones. Reason: the step factor sets the felt size of the line, as the gain of 1 block in the row PV. Drift = ln(median level factor of the design) / ln(median level factor of the build). The row is out when the distinct share is below `min` or the drift is below `min_drift`. With no `--build`, the drift is not computed, and the distinct share alone decides. Sub-value: the size spread, the standard deviation of log10(level factor - 1).

**FM-4 Largest gap between jolts.** Inputs: the income of c\* at each event, and E. Procedure: an event at time t is a jolt when the income of c\* is at least 1.5 x the income at the first event at or after t - 60 s. A jolt within 60 s of the jolt before it does not count. A count step is also a jolt (**count steps**): the output of its line takes the factor `m` in 1 purchase, and the step is the felt event of the line. When the game resets more than 1 time each 10 minutes, the 60 s after each reset do not count. The value is the largest gap of the jolt times in minutes. Sub-values: the jolts for each hour, and `count_steps` (the count steps in the run). Guard: FM-1, because each reset can give a jolt with no new content.

**FM-5 End arc.** Inputs: the content elements, and E. Procedure: tail = (E - time of the last content element) / E. The row is out when the tail is above `max`. Sub-values: the end ratio (the rate of content elements in the last 10 % of E against the rate over E), the share of content elements after E / 2, and the time of the last shift as a share of E. Guard: FM-11, because a last milestone at the last step makes the tail 0.

**FM-6 Chore length and automation timing.** Inputs: the presses of the active run and the key `automation` of the model. Procedure: for each line, count the presses before the player stops pressing it. That time is `from_minutes` of its automation entry, or the first auto purchase of the line when the entry has `requires`, or E when the line has no entry. The value is the largest count and its line. The second bound is the hand share in hour 1: hand purchases / all purchases up to min(1 h, E). The row is out when the largest count is above `max_presses` or the hand share is below `min_hand_share_h1`. The field `where` holds the presses of each line. Guard: FM-9.

**FM-7 Verbs for each hour.** Inputs: the presses, the resets and the effects of the model. Procedure: in each hour up to E, count the systems with 3 or more presses in the hour, add 1 for a reset in the hour, and add 1 when the model has an effect with `"active": true`. The value is the minimum over the hours from hour 4 on. The floor of 3 presses is the guard of `fun-metrics.md`: a system with 1 tiny purchase is not a verb. The sub-values give each hour with and without the floor. A game of 3 h or less gives not applicable.

**FM-8 Comeback ratio.** Inputs: the reset events and the axis step events of the active run. Procedure: split the events into runs at each reset. P_k is the count of axis step events in run k. For run k + 1, take the time of its P_k-th axis step event less the start of run k + 1, and divide by the length of run k. The value is the median over the pairs where run k + 1 passes P_k. A run k with no axis step does not count. The row is out when the median is outside `min` to `max`, or when no run passes. Sub-value: the passes of the pairs. No reset gives not applicable. Guard: FM-12, because a reset rule that resets very early gives a small ratio.

### Advice rows

These rows show the reason and the change. They do not change the result.

| Id | Name | Band keys and default (mark) | Controls |
| --- | --- | --- | --- |
| FM-9 | Purchase variety | `max` 0.6 (no source) | C14, C2 |
| FM-10 | Felt beat share | `min` 0.3, `step` 0.05 (no source) | C8, C6 |
| FM-11 | Shift stretch | `max` 0.35 (inferred: 3 acts give 0.33) | none |
| FM-12 | Reset novelty share | `min` 0.8 (no source) | C9, C11 |

**FM-9 Purchase variety.** For each hour with 20 or more presses: the presses of the most-pressed line / the presses of the hour. **Count steps**: a press of a line with count steps counts `1 / n` in the share, because the `n` presses of 1 block are 1 choice. The test "20 or more presses" counts each press as 1. The value is the largest share, with its hour and line. No such hour gives not applicable. Guard: FM-2.

**FM-10 Felt beat share.** For each beat whose c\* income is above 0 before it: rise = income after / income before. The value is the share of rises at 1 + `step` or more. The reason counts the flat beats by kind. A count step by hand is also a beat, and its rise is its block gain (**count steps**). The row does not yet count a beat that opens a line or a system as felt.

**FM-11 Shift stretch.** The value is the largest gap of the shift times in (60 s, E], as a share of E. Guard: FM-3, because a model can split 1 screen into many systems.

**FM-12 Reset novelty share.** The simulator gives a reset a weight of 3 or more only when its run reached a new beat. The value is the share of resets with a weight of 3 or more. No reset gives not applicable.

FM-6 counts each press of a line with count steps as 1 press: a block of `n` levels is `n` presses of work. The pair fixtures `tests/fixtures/fun/count-steps-gen.json` and `count-steps-none.json` and `tests/test_fun_count_steps.py` test each count step rule.

### Guards

A row whose guard row is out keeps its status. Its reason gets 1 sentence that names the guard. The field `guard` gives the status of each guard row.

## Tier 2 rows

The script does not compute these rows yet, and the sheet does not list them. Each row needs a new output of the simulator or the scan. FM-13, FM-14 and FM-15 will count for the result only after their pair tests pass in the skill tests. Until then they are advice. The other tier 2 rows are advice.

| Id | Name | Definition | Band (mark) | Needs | Controls |
| --- | --- | --- | --- | --- | --- |
| FM-13 | Choice consequence | The CV (standard deviation / mean) of the times of 13 buyers to the same step: greedy, `cheapest` and 10 noisy buyers. Experience: "my choices change the result" | CV 0.03 to 0.25 (no source) | the buyer set (13 runs) | C6 (weak, inferred) |
| FM-14 | Order sameness | The `difflib` ratio of the first 400 hand purchases of each buyer against greedy. A high value says that a choice is an illusion | below 0.6 (no source) | the runs of FM-13 | none |
| FM-15 | Live choice | The time-weighted mean count of lines within 10 % of the best payback score of their currency, and the share of time with 2 or more. Guard: FM-3, because clones tie | mean 1.5 to 4; tie share 0.3 to 0.8 (no source) | a decision log of the simulator | C2, C6 (weak, inferred) |
| FM-16 | Tend upkeep | 60 / the window seconds of the active bonus, at the start and after hour 1 | below 4 taps/min after hour 1, and falling (inferred) | a scan rule for the window and its readers | C12 |
| FM-17 | Feedback ladder | The rank order of the feedback strength (tween length, scale, volume) against the weight of the beat kind | 0 inversions; reset at least 3 x purchase (no source) | the tween values of the scan | C16 |
| FM-18 | Goal and horizon visible | The next goal with its progress on the main screen; the share of locked items that show their requirement; the share of locked elements visible before their unlock | goal on screen; 1.0; 0.3 to 0.8 (no source) | the label, lock text and visibility of each element | none |
| FM-19 | Decision information on screen | The minimum count over the purchase rows of 4 fields: cost, effect of the next level, gain against the income, time to afford | 4 of 4 (no source) | the scan rules of the purchase rows | none |
| FM-20 | Literal numbers in UI text | The count of UI strings with a number typed by hand that equals a model value. An export or a rebalance makes such a string false | 0 (no source) | a table of UI string literals | the export of C1 to C15 |

Until a tier 2 row exists, check it by hand when the question is about it, and label the answer "judgment, not computed".

## Tier 3: the play test protocol

The 4 tier 3 measures need people. The sheet lists them under "Play test". Only a play test tells why a player stops. The use of these measures is calibration: their results become bands for tier 1 and tier 2 (`tuning-controls-and-play-data.md` section 7).

Setup: a build with the play log and the mark keys (`F7` "this felt good", `F8` "this dragged"). Normal speed (x1). 5 or more players who do not know the team. 1 sitting each, up to the hard end or the stop.

| Id | What to ask or watch | When | How to record |
| --- | --- | --- | --- |
| FM-21 Felt pace probe | 1 question with 3 buttons: too slow, right, too fast | each 10 minutes of play, and at each `F8` mark | the play log event `probe` with the answer, the game time and the axis step. Report the share of "right" for each hour and the first "too slow" |
| FM-22 Goal comprehension | "What are you working toward now, and what does it cost?" with the screen hidden | minutes 5, 20 and 60 | the observer scores 1 for the correct goal and the order of magnitude of the cost. Report the mean. Band: at least 0.8 (no source) |
| FM-23 Sitting length and stop point | the time of the first wish to stop, the stop, and its reason in 1 line | during the sitting, and at the stop | the play log `mark` and `session_end`, and the reason on paper. Report the minute and the axis step. Band: first wish to stop after 2 h or more (inferred) |
| FM-24 Need satisfaction and meaning | the PENS competence and autonomy items, or the PXI Mastery, Autonomy, Curiosity and Meaning items; 1 question for each system: "what does this system do in the world of the game?"; each new plan that the player states aloud | the scales after the sitting; the plans during play | the scale scores, the share of correct answers on systems, and the found plans for each player hour. No band |

Rules:

1. Use the same wording each time, because a new wording gives a new measure.
2. Do not use the GEQ questionnaire. Its 7 factors have no support (measured).
3. Tag a found plan that is a bug or an exploit apart.
4. A player can stop because a chapter ends, which is good. Record the reason.
5. Nothing leaves the machine of the player without the agreement of the player. Record no personal data.

## Result

| Result | Rule |
| --- | --- |
| `likely dull` | 2 or more counting rows are `out` |
| `watch` | 1 counting row is `out` |
| `no finding` | no counting row is `out`, and 6 or more counting rows are `ok` |
| `not enough data` | fewer than 6 counting rows are `ok`, and no counting row is `out` |

The rule "2 or more" is a judgment (no source). In the first values, each build that people called bad had 6 counting rows out. The result line always ends with "advice with evidence marks, not a proof".

The result never changes the gate of `SKILL.md`. A design that is ready with "likely dull" stays ready, and the report says both. A design that is not ready with "no finding" stays not ready. Read a row that is out as a question with a place in the game ("no new content from minute 118 to minute 166"), not as a verdict on fun.

## The control of each row

The controls C1 to C16 are sliders of the tuning panel of a game. Each slider moves many numbers by 1 rule. Each control has a status: **keeps** (it stays inside the proven ranges, and the law rows PO1 to PO9 hold), **re-sim** (the law rows hold, but the simulation rows PC1 to PC6 and PO10 to PO13 must run again) or **leaves** (it can move a parameter outside its proven range).

| Control | Status | Rows that it moves |
| --- | --- | --- |
| C1 Opening pace | re-sim | FM-1 (hour 1) |
| C2 First price | re-sim | FM-1 (hour 1), FM-9, FM-15 (weak) |
| C3 Unlock spacing | re-sim | FM-1, FM-5 |
| C4 Stage wall | leaves above 1 | FM-8, the empty windows of FM-1 |
| C5 Game length | re-sim (needs a new game) | FM-1, FM-5 |
| C6 Upgrade punch | keeps (1.0 to 3.6) | FM-2, FM-4, FM-10, FM-13 (weak), FM-15 (weak) |
| C7 Growth pressure | leaves other than 1 | FM-2, FM-4 |
| C8 Milestone pay | re-sim | FM-4, FM-10 |
| C9 Run length | keeps while PO5 holds | FM-8, FM-12, FM-7 |
| C10 Reset weight | keeps (0.92 to 1.0) | FM-8 |
| C11 First reset | re-sim | FM-8, FM-12 |
| C12 Tend boost | leaves above 1.25 | FM-16, FM-7 |
| C13 Bank | keeps inside its range | none (PO9 only) |
| C14 Auto-buy start | re-sim | FM-6, FM-7, FM-9 |
| C15 Garden rhythm | re-sim (simulation only) | FM-1, FM-7 |
| C16 Feedback | no proof effect | FM-17 |

No control moves FM-3, FM-11, FM-14, FM-18 or FM-19. FM-3 is the most important of them: it found both complaints of lost character, and each purchase control applies 1 rule to all lines, so clones stay clones. Fix it with an edit of each line (step, cost growth, cap) or with a family tag. FM-11, FM-18 and FM-19 need content or UI work. A change of a counting row with no control is content: report it as case 3 of the gate (content is missing).

## Tune by hand with the tuning panel

A row can say where a game drags, but only a person can say which setting feels right. A game with a tuning panel lets the developer move the controls while the game runs. Moss Works has 1: `F2` shows the sliders C1 to C15, the status chip of each one, the live fun values and the proof readouts. Procedure:

1. Play at x1, not at a play-test speed, because the feel of a wait changes with the speed.
2. Watch the live value of the row beside each slider. Example: raise C6 and watch FM-2 and FM-4, or lower C3 and watch the drought of FM-1.
3. Press `F7` at a moment that feels good and `F8` at a moment that drags. The play log keeps each mark with the live values.
4. A chip "leaves" names the proof obligation at risk. Keep the setting only when the proof sheet still passes after step 6.
5. Save the setting with a name, and export it. The export writes the 4 tuned data files and `targets_from_tuning.json` to `data/tuning/export/<name>/` (to `user://tuning/export/<name>/` in an exported build). The data files of the game stay as they are.
6. Give the setting back to the skill. Copy the targets of `targets_from_tuning.json` into `<work>/targets.json`. Each of them has the mark "designer setting" and `"unsupported": true`. Solve again and prove again. The solver gives a proved design near the setting, or it names the target that conflicts.
7. Keep the key `fun` of the export in `<work>/targets.json`. The solver copies it into `game.targets.fun` of the model. The report of the solve gives the section "Fun rows" (FM-1, FM-2, FM-4 and FM-5, and the cause of each row that is out) and the section "Gaps between content elements" (proof row CG). A fun row never makes the result ready or not ready. Run the fun sheet again on the model. Each band from the setting shows the mark "designer setting".

The export writes the band keys of the sheet directly (FM-1 `max` and `max_h1`, FM-2 `max`, FM-4 `max`, FM-6 `max_presses` and `min_hand_share_h1`, FM-8 `min` and `max`). An unknown row id or band key stops the sheet and the solver with exit 2. The message names the key. The export writes FM-16 in the top-level block `fun_not_computed`. The sheet does not read that block.

After an export, run the scan again: an export changes the data files and not the UI strings, so a string with a typed number can become false (FM-20).

## Outputs

The Markdown has the title `# <game>: fun sheet: <result>`, the limits, the result line, the length E, the notes, the table of the counting rows, the table of the advice rows, the rows that are not computed, and the 4 play test measures that the sheet cannot compute. With `--build`, each table has 3 more columns: the value of the build, the status of the build, and "worse than built".

The JSON has these keys:

| Key | Content |
| --- | --- |
| `schema_version` | 1 |
| `game` | the name of the game |
| `source`, `build`, `scan` | the input paths (`scan` is null in this version) |
| `economy_currency` | c\* |
| `length` | E in `seconds` and `minutes`, and its `source`: `hard`, `axis` or `last` |
| `runs` | the player that ran and its horizon in hours |
| `notes` | the notes of the sheet |
| `rows` | 1 object for each row, with the fields of "Rows"; with `--build` also `build_value`, `build_value_text`, `build_status` and `worse` |
| `result` | `likely dull`, `watch`, `no finding` or `not enough data` |
| `counting_out` | the ids of the counting rows that are `out` |
| `limits` | the limits of the sheet |
| `build_result`, `build_counting_out` | with `--build`: the result of the build |

Each float has 4 decimals, so 2 runs write the same bytes.
