# The simulation path

The solver proves only some variants (`references/design-solver.md`, "Order of the solve", step 3). A designer can
select a shape that the laws do not cover: no reset layer (`S04.none` with S29 or S22), a manual phase
(`S05.manual_then_automate`), a chain of producers, an allocation pool, or a mechanic that the catalog does not have.
The simulator can play each such model. A simulated balance with honest marks is useful; a refusal is not.

This path ends with a model, a simulation with several players, a workbook, and a result that marks each part of the
design as proved or as simulated.

## When to take this path

Take it when the solver reports a variant conflict (way out 2: "keep the variants and balance the game by
simulation"). The solver takes it by itself: on a variant conflict it writes the model of the scaffold, fits it, and
runs the proof sheet. Its report starts with "(simulation path)".

Do not take it for a conflict of a proof row (for example PC4 or PO5) of a proved selection. The fix text of that row
is the way out.

Take way out 1 (another variant) when the designer does not care about the variant. The laws then prove the design.

## The marks

The model key `simulation_path` (`references/model-schema.md`) has 1 part for each selected system, and 1 part for
each mechanic that a person writes by hand. Each part has 1 mark:

| Mark | Meaning | Proof sheet status of the part |
| --- | --- | --- |
| `law` | written, and a law covers it also with no reset layer: S01 (L1), S02 (L1), S05.active_lane (L5) | no row |
| `simulated` | written; the simulation alone checks it | `simulated, not proved` |
| `not written` with `"fill": true` | a stub: a person must write it | `stub: fill by hand` (the result is not ready) |
| `not written` with `"fill": false` | the model does not need it, with the reason (`S04.none`; `S29`, see below) | `not modelled` |

## Commands

```bash
python3 <skill_dir>/scripts/pw_design.py <work>/selection.json <work>/targets.json --out <work>/model.json --report <work>/report.md --proof <work>/proof.json
python3 <skill_dir>/scripts/pw_scaffold.py <work>/selection.json <work>/targets.json --out <work>/model.json
python3 <skill_dir>/scripts/pw_scaffold.py --fit <work>/model.json <work>/targets.json --out <work>/model.json
python3 <skill_dir>/scripts/pw_proof.py <work>/model.json --strict-ranges --speed --json <work>/proof.json
python3 <skill_dir>/scripts/progression_workbook.py <work>/model.json <work>/<game>-progression.xlsx --players all
python3 <skill_dir>/scripts/pw_scaffold.py --vary <work>/model.json <work>/targets.json
```

1. The solver (line 1) writes the scaffold, fits it, and proves it. Exit code 3 is "simulated", 1 is "not ready".
   Line 2 writes the same scaffold without the solver (`--no-fit` skips the fit).
2. Complete each stub by hand (section "Complete a stub"). Then fit again (line 3). The fit keeps your effects and
   sets only the walls, the unlock points and the first costs.
   Lines 2 and 3 exit 1 when the fit misses a target of the timeline (section "The timing fit", step 5).
3. Run the proof sheet (line 4). Exit code 0 is ready, 3 is simulated, 1 is not ready.
4. Run the players and the workbook (line 5). The script leaves out the reset players of a model with no reset layer,
   and prints 1 line `LEFT OUT:`. Copy it into the report.
5. Judge the result (line 6 and section "How to judge a simulated result").

## What the scaffold writes

`pw_scaffold.py` writes each system that it can express with the default values of `assets/systems.json`:

| System | Model |
| --- | --- |
| S01 | line `gen`: cost 10, the cost ratio of the variant, add 0.5 |
| S02 | 1 multiplier group `m1`, `m2`, ... for each S02 entry of the selection that is not a care entry, with the key `entry` (the entry id), as the solver does on the path "1 long run". With 1 S02 entry: the 2 groups `m1` and `m2` with no entry id. `levelled_factor`: mult d; `add_in_group`: pct v. Placed at 1 and 4 minutes, then each 3 minutes later (hand-set) |
| S04.power | the reset layer of the solver with the default p; q from the written lines (BM L6), and the divisor c gives a gain of 1 at `run_min` (hand-set) |
| S03.exponential | ladder `stage` with a rate wall on `coin`; the fit sets each wall |
| S05 | `active_lane`: milestone `active_play`, x a while the player plays, base income 1 / a. `manual_then_automate`: milestone `manual`, add 1 while the player plays, base income 0 |
| S06.penalty_cap | `game.offline_rate` and `game.presence.offline_cap_minutes` |
| S10.window | milestone `visitor`: the mean factor of the chance while the player plays (hand-set) |
| S12.count_steps | milestones `count_1` to `count_4` at n, 2n, 3n and 4n generators |
| S13.raised_cap | the cap c0 on `gen`, and c1 more for each stage |
| S24.auto_buy | `automation` of `gen`, placed at 25% of the length (hand-set) |
| S28.last_step | `game.targets.hard_end`: the last stage |
| S30.need_capacity | the key `upkeep` of the solver: the need of each S01 and S02 line, and 1 care line. The care line has the id of the S02 entry with the role `care` (else `care`), and `upkeep.capacity` names the same id |

Each other variant is a stub with its defaults and a text `to_fill`. A stub of S03 also writes a placeholder ladder, so
that the players have an axis.

**S13 against the purchase value floor.** With S13.raised_cap, the scaffold writes the generator cap as c0 + c1 for each
stage (fixed range c0 30, c1 5). With 26 stages, the generator reaches about 30 + 5 x 26 = 160 levels. 1 more level
then gives about 1/160 of the generator income: under 1%, below a PV floor of 10%. The path "1 long run" sets the
generator cap from the PV floor, and the simulation path does not. The 2 ranges conflict, and the scaffold does not
choose. Report it as a designer decision: keep the S13 range, or lower the cap so that 1 level keeps the PV floor.

## Systems that the model cannot express

The model has no construct for 2 systems. Their stub text says so, and gives the fold-in method and the direction of
its error. No fold-in proves the system. Mark the written part `simulated`, and state the error in the report.

| System | Why the model cannot express it | Fold-in method | Direction of the error |
| --- | --- | --- | --- |
| S21 (allocation pool) | No construct splits 1 pool between 2 or more sinks, so "speed = share^e for each sink" has no model line. | Write 1 purchase line for each sink (for example producer levels and care levels). The buyer of the simulator chooses between them. | Each line is linear in its levels (e = 1 in place of e < 1): the model has no diminishing return inside a sink. It overstates the income of a player who puts the pool in 1 sink. The simulated pace is an upper bound for that player. |
| S18 (chain of producers) | An effect adds income to a currency. No effect adds levels to a purchase line, so "x_i <- x_i + x_(i+1) x rate_i x dt" has no model line. | 2 currencies: tier 2 is a line that makes a side currency, and the side currency buys the levels of tier 1. | Tier 1 does not grow by itself: the model has no power-of-time growth of the chain. It understates the late income, so the simulated hard end is later than the hard end of the game. |

**S29 with no reset.** The simulator plays 1 continuous run and buys at any time. A run limit that only stops the
purchases has no effect on the economy of the model, so the scaffold does not write it (`not modelled`). If the game
stops the income between runs, add an uptime factor (a mult below 1) by hand, as the worked example does.

## The timing fit

`pw_scaffold.fit` sets the timing numbers of a model with no reset by simulation, with the method of the rebalance
mode (`references/design-solver.md`, "The timing fit"):

1. **The timeline.** A stage each 80% of the first-hour unlock gap target in the first hour, then each 80% of the unlock
   gap target, and the last stage at the length target `length_h.value`. The last stage is the hard end, and the demo
   end is the stage at `demo_end.minutes`.
2. **The placement passes (4).** Each pass plays active_greedy and takes each stage at its target time. The wall of the
   stage is the value of the spine stat then. Each entry with the key `place_minutes` gets the unlock point of that
   minute (a lifetime total of the main currency), and each placed line costs 20 s of the income then. An entry with
   `place_minutes: null` takes the middle of the largest gap of the first hour.
3. **The refine passes (6).** Each pass plays the model with its walls, and moves each wall to the value of the spine
   stat at the target time of its stage. The fit keeps the pass whose stage times are nearest to the timeline.
4. **The record.** `simulation_path.fit` holds the target and the simulated minute of the first purchase and of each
   stage, the simulated hard end, and `flat_stages`: the stages whose wall is not above the wall before them. Such a
   stage comes with the stage before it.
5. **The check.** The record also holds `misses` and `causes`. A miss is a target of the timeline outside its range:
   - the hard end outside the length tolerance of the targets (`length_h.tolerance`);
   - the first purchase after its latest minute;
   - a stage outside 20% of its target minute (`stage_tolerance`; the stage timeline has no game source).

   With 1 or more misses, `misses` also names the flat stages and each group of stages at the same minute, and
   `causes` gives the probable cause of each miss:
   - a stage that comes early, with no flat wall: the income of the spine currency falls before it (a penalty such as
     the upkeep, or a cap). The income then meets a wall before its target time;
   - a flat or late stage: the placed entries that the run never reaches or buys, and the lines at their cap at the
     end of the run. With neither, no content raises the income there.

   The script prints 1 line `FIT MISS` for each miss and 1 line `FIT CAUSE (probable)` for each cause, writes the
   model, and exits 1. A cause is probable, not confirmed: check it in the model before you change the model.

The fit does not add content. A purchase gap (PC2) or an unlock gap (PC4) that it cannot close is content to add.

## Complete a stub

A stub names the family of its card and its defaults. Write the family as model lines and effects
(`references/model-schema.md`, "Common modelling moves"). Then:

1. Add the entries to the model. Give each entry that opens later the key `place_minutes` (the target minute), so that
   the fit sets its unlock point and first cost.
2. In `simulation_path.parts`, set the part to `"mark": "simulated"`, `"fill": false`, its `entries`, and a `reason`
   that states the simplification.
3. Add each number that you set with no source to `simulation_path.hand_set` as a path, for example
   `{"name": "upkeep uptime", "path": ["milestones", "upkeep", "effects", 0, "value"]}`.
4. Add 1 line for each simplification to `game.assumptions`.
5. Remove the text of the part from `simulation_path.to_add`, and fit again.

### Worked example: a mechanic with no catalog card

The pitch makes upkeep the main mechanic: workers stop to eat, drink and rest, and care upgrades reduce the downtime.
The catalog card S30 (upkeep) now covers this mechanic. The solver writes it on the path "1 long run" and on the
rounds of a reset layer (`references/design-solver.md`, "Upkeep"), and the scaffold writes it on this path (S30 in the
table above). The method below writes a mechanic with no card by hand. Write it
as 1 penalty and 3 capped factor lines, so that the uptime stays below 1:

```json
"milestones": [
 {"id": "upkeep", "name": "Upkeep (uptime)", "system": "upkeep", "kind": "feature", "persists": true,
  "effects": [{"target": "coin", "kind": "mult", "value": 0.6}]}],
"purchases": [
 {"id": "feeder", "name": "Feeder", "system": "upkeep", "cost_currency": "coin", "base_cost": 100, "cost_growth": 3,
  "max_level": 2, "place_minutes": 6, "effects": [{"target": "coin", "kind": "mult", "value": 1.1}]},
 {"id": "water", "name": "Water", "system": "upkeep", "cost_currency": "coin", "base_cost": 100, "cost_growth": 3,
  "max_level": 2, "place_minutes": 12, "effects": [{"target": "coin", "kind": "mult", "value": 1.1}]},
 {"id": "bed", "name": "Bed", "system": "upkeep", "cost_currency": "coin", "base_cost": 100, "cost_growth": 3,
  "max_level": 1, "place_minutes": 18, "effects": [{"target": "coin", "kind": "mult", "value": 1.1}]}]
```

The part is `{"system": null, "name": "upkeep", "mark": "simulated", "entries": ["upkeep", "feeder", "water", "bed"],
"reason": "no catalog card or law covers an uptime factor below 1 that upgrades raise"}`. The caps keep the penalty a
penalty: 0.6 x 1.1^5 = 0.966. Add `systems` entry `upkeep`, the 3 hand-set numbers, and fit again.
`tests/designs/sim-path/going-ham/` holds the full run on the inputs of the failed forward-design test.

## How to judge a simulated result

A simulated result says that 1 simulated player met each target. It does not say that the design is robust. Check 2
things before you report it:

1. **2 or more buyers.** The workbook runs the payback buyer (the players) and the buyer of version 1 (`eager`,
   `patient`, `doubler`). Compare their hours to the end. A large difference means that the timeline depends on how
   the player buys.
2. **Each hand-set number by 20%.** `pw_scaffold.py --vary` changes each number of `simulation_path.hand_set` by -20%
   and +20%, and prints the hard end, the largest purchase gap and the largest unlock gap of the active player, each
   with "holds" or "MISSES". Exit code 1 means that 1 or more changes miss a target. A number that moves a target out of
   its band is a number that a play test must set. Say so in the report. The rules of the change:
   - A ratio changes by its part above 1: the value of a `mult` effect, a `mult` capacity step of the upkeep, and a
     cost ratio (`cost_growth`). x1.1 at -20% becomes x1.08, a smaller bonus, not a penalty. The line says "bonus".
     Each other number (an `add` or `pct` value, a total, a capacity) changes by 20% of its value.
   - Each line gives the value before and after, and each purchase line that the change moves outside its proven range
     (the range rows of the proof sheet).
   - When the 2 changes of a number give the result of the model as fitted, the line says "no effect on these
     targets". This is true, for example, when the line reaches its cap early.
   - A path walks dict keys, list indexes, and the ids of the entries of a list. A path that does not resolve, or that
     ends at a value that is not a number, is a bad input: exit code 2.

## The report rule

The report has 2 lists: "Proved" and "Simulated, not proved". The solver report and the template of `SKILL.md` use
them.

- **Proved**: each row that passes by a formula, and each part with the mark `law`.
- **Simulated, not proved**: each row with the status `simulated, not proved`, each target row that passes in the
  simulation, and each part with the mark `simulated`. For each item, state what a play test must confirm.
- State each part that is `not modelled` with its reason, and the `LEFT OUT:` line of the workbook.
- A result `not ready` keeps the gate of `SKILL.md`: name each failed row with its fix.
