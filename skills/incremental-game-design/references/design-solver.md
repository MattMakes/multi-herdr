# The design solver

`scripts/pw_design.py` turns the targets of a designer and the selected systems into a model whose
parameters come from proven ranges. It then runs the proof sheet (`scripts/pw_proof.py
--strict-ranges`) on that model. The result is a model that is ready to build, or a stated conflict.

Version 1 of the skill took the numbers of a design note as fixed and tuned them by trial. The solver
replaces that. Targets go in, parameters come out, and each parameter has a source. The symbols are
those of `references/balance-math.md` (BM).

## Command line

```
python3 scripts/pw_design.py <selection.json> <targets.json> [--out MODEL] [--report FILE] [--proof FILE]
                             [--content CONTENT] [--accept-change NAME ...]
python3 scripts/pw_design.py [targets.json] --content CONTENT --as-built --out MODEL
```

`--content` turns on the rebalance mode (section "Rebalance mode"). On a conflict the solver writes the model and the
proof sheet of the round with the fewest failed rows (`best_round`), and the report adds the full report of that
round.

`--accept-change NAME` (rebalance mode, more than 1 permitted) accepts 1 named change of the structure of the game
(section "Structure first"). With no such option, the model keeps the structure of the game.

`--as-built` writes the model of the game with its present numbers and changes nothing (section "The game as
built"). With no targets file, the solver reads `assets/default_targets.json`: the example targets with no demo end,
for a game whose designer gives no answer.

| Exit code | Meaning |
| --- | --- |
| 0 | The model is ready: each obligation is `pass` or `not applicable` with strict ranges. 1 long run: the result is `ready` |
| 1 | No model inside the proven ranges is ready, an element of the content is not fitted, or a selected system has no writer and no element of the model writes it (section "Selected systems not written" of the report). The report states the conflict. On the simulation path: the result is `not ready` (a stub or a failed row) |
| 3 | The simulation path or 1 long run: the result is `simulated`. Each row passes or is `simulated, not proved`, and no part is a stub. The design can go to a build, and a play test must confirm the simulated rows (`references/proof-sheet.md`, "The result") |
| 2 | Bad input: a file that does not parse, an unknown variant, or a selection that breaks a rule |

The solver is deterministic: the same input gives the same bytes. A solve with no content file takes about 1 second.

## Inputs

The interview (`references/interview.md` section 8) ends with 1 selection record. The solver reads it
as 2 files, so that a designer can change the targets and keep the selection.

1. **Selection** (`assets/example_selection.json`): the record without `targets`. The keys are
   `designer_answered`, `starter_set` (a set id or null), `selected` (system, variant, parameters, and the
   optional `id` and `role`), `refused` and `notes`. A starter set fills each system that `selected` does not
   name. A null variant in a starter set takes the recommended variant of the interview, else the first variant
   of the card. Section "Selection entries" gives the rules of `id` and `role`.
2. **Targets** (`assets/example_targets.json`): `play_pattern` (`continuous` or `check_in`) and
   `targets` with the keys of the record. 3 optional keys serve the solver:
   - `length_h.extend_to_h`: a longer game with the same formulas (more content).
   - `active_factor` (`value`, `unsupported`): the active income factor of S05.
   - `run_min`: the run length of a reset layer, from 1 reset to the next. The default is the median unlock gap of the
     session target (15 minutes). With no reset layer, the solver does not read it: leave it out. It does not set the
     shift length of S29, which is the parameter `run_s` of `S29.air` (interview, T.run).

   **A band to 1 value.** A target that the designer gives as a band from `lo` to `hi` becomes `value = (lo + hi) / 2`
   and `tolerance = (hi - lo) / (hi + lo)`, the half width as a share of the middle. Example: 2 to 3 hours gives
   `value` 2.5 and `tolerance` 0.2, so PC1 and PO12 accept 2.0 to 3.0 hours (interview section 2).

   For `continuous`, the targets of group A3 of the interview replace the session target:
   - `length_h.value`: the length to the hard end, in active hours (band 2 to 10, default 3 to 6, `SG:177`).
   - `purchase_gap`: `largest_max_min` and `first_max_min` (default 2 and 2, `SG:180-181`), and the unlock gaps
     `unlock_max_min` and `unlock_first_hour_max_min` (default 10 and 5: designer targets with no source, band CT7,
     interview A3.2b). The model gets them as `largest_unlock_gap_minutes` and `first_hour_unlock_gap_minutes`.
   - `purchase_gap.unlock_max_min_ending` (optional): the limit of the last unlock gap only, the gap that ends at
     the hard end. PC4 then tests each earlier gap with `unlock_max_min`. The model gets it as
     `ending_unlock_gap_minutes`. With no such key, the last gap has the limit `unlock_max_min`. It is a designer
     target with no source. The solver does not use it to space the stages.
   - `tolerance` (optional) in `purchase_gap`, `purchase_value` or `purchase_rate`: the range of the target as a
     share of its limit (`references/proof-sheet.md`, "Target rows have a range"). In `purchase_gap` it is 1 number
     for each limit of the group, or an object with 1 number for each key (`unlock_max_min`: 0.1). The model gets
     `game.targets.tolerances`. With no `tolerance`, the proof sheet takes the default of the source class of the
     target (that section gives the 2 numbers). `purchase_gap.unsupported` true gives the 2 purchase gap limits the
     class "no source" (the model key `game.targets.unsupported`).
   - `demo_end`: `minutes`, `before_system` and `unsupported`. With no `demo_end`, the game has no demo.
   - The first-hour unlock band of the model (`unlocks_first_hour`) comes from the first-hour gap target g:
     `[ceil(60 / g) - 1, 60]`. A band below `ceil(60 / g) - 1` conflicts with PC4, so `unlocks.first_hour` of the
     targets file sets the band only for `check_in`.

   The hard end is the variant of S28 in the selection. The solver proves only `S28.last_step`: the
   last stage N is the hard end.

### Selection entries

Ticket 41 (repair 4). An entry of `selected` is `{"id", "system", "variant", "role", "parameters"}`. `id` and
`role` are optional, so an old record (1 entry for each system) still loads.

1. A system can have more than 1 entry. Each entry then needs its own `id`. 2 entries with the same id (or 2
   entries of 1 system with no id) are bad input (exit 2).
2. With no `id`, the id is the system id in lower case (`s02`).
3. The entries of `selected` for a system replace the entry of the starter set for that system.
4. `role` is free text that names the use of the entry (`generator upgrades`). 1 role has a rule: `care` is for
   an S02 entry only, and it needs S30 in the selection. Its line is the care line of the upkeep (section
   "Upkeep").
5. The solver takes the variant of the first entry of each system for the rules of step 1 (validate). The
   parameters of 2 entries of 1 variant go to that variant: the last entry wins.
6. 2 or more S02 entries that are not `care` give 1 multiplier group each (`m1`, `m2`, `m3`, ...) with the first
   costs 100, 1000, 10000, .... Each line has the key `entry`. `R` is shared by the groups: `r = R / groups`.
   With 1 S02 entry (the old format), the solver writes the 2 groups of worked example 2.
7. The report lists each entry of a system with 2 or more entries under "Selected systems", and the column
   "Entries" of "Systems and marks" gives the lines of each entry.

### Upkeep

When the selection has `S30.need_capacity`, the solver writes the key `upkeep` of the model
(`references/model-schema.md`) and 1 care line (system `s30`, no income effect). The line has the id of the S02
entry with the role `care`, else `care`. The rules:

1. **The need.** Each level of the generator and of each multiplier group adds 1 (`per_level` 1, `base_need` 0).
2. **The capacity.** `base_capacity` 3 (the first 3 producer levels are met). Each care level multiplies the
   capacity by `c = (1 + 2 x floor)^(1 / e)`, with the PV floor (1.2 at the default floor 10% and `e = 1`), so
   that PU3 passes with a margin.
3. **The care cost.** The first cost of group 1 / 2, with the growth `b` of the groups. On the path "1 long run":
   5 x the first cost of the generator (50), because the group costs come late there.
4. **The band of u.** `[c^(-2e), 1]`: at most 2 care steps below 1 (0.69 at `c = 1.2`).
5. **The rows.** PU1 to PU4 (`references/balance-math.md` section 3b). PU3 is proved by its formula. PU1, PU2 and
   PU4 are simulated, so the result is at best `simulated` (exit 3).

The solver writes S30 on the path "1 long run" and on the rounds of a reset layer (the reset clears the care line
with the lines of S01 and S02). The scaffold of the simulation path has no writer for S30: the part is a stub, and
a person writes the key `upkeep` by hand (`references/simulation-path.md`, "Complete a stub"). `S30.pool_share` and
`S30.decay_refill` have no writer.

Result with K9, S30 and 3 S02 entries at 2.5 hours (`tests/designs/upkeep/`): each balance row holds, and the result
is `not ready` (exit 1) on the content gap row CG only, because the skeleton names no content. With a content file
it is `simulated` (exit 3, `assets/examples/simulated-upkeep/`).

## Order of the solve

Each step uses only the steps before it. The reason for each step is below it.

1. **Validate the selection.** Each core system of the play pattern has a variant. The presence gate
   slot has the number of systems that the pattern needs (`check_in` 1 or more, `continuous` 0 or
   more). Each `needs` is met. For `continuous`, the core is S01, S02, S03, S05, S24 and S28, so a
   selection with no hard end is refused. With `S04.none` or no S04, the selection needs S29 or S22
   (decision D10, "rule no reset"), or S28 for 1 long run (decision D11). A failure gives exit code 2 and names
   the rule.
   Reason: a balance of an incomplete set proves nothing about the game.
2. **Compare the targets with `target_bands`.** A target outside the evidence is a warning with the
   band and its source. A target whose band has no proven game (length, reset gap, play-test speed)
   is marked "default, not from a proven game". An active factor above the sourced 1.25 is marked
   "designer target, above the sourced value". An idle value of 1 or more is a conflict at once.
   For `continuous`, the solver compares the targets with the bands CT1 to CT6 of the play pattern in
   `assets/systems.json`: the length, the purchase gap, the first purchase, and the demo end. A demo
   end inside the band of a strategy or management game (40 to 60 minutes, `band_management` of CT4, `SG:183`)
   is a note, because the solver does not know the genre. A demo end outside both bands is flagged with the
   nearest proof (`SG:189`). The targets with no source (unlocks,
   the active factor, the play-test speed) are stated as designer targets, not supported by a source.
   Reason: the designer decides, but each decision must be visible (requirement R5).
3. **Check the families.** The laws prove only some families: S01 geometric, steep or ladder, S02
   `levelled_factor`, S03 `exponential`, S04 `power`, S05 `active_lane`, S06 `penalty_cap`, S08
   `timed_bank`, S28 `last_step`. Each other selected variant of these systems is a conflict, with its
   cause. A missing S04 counts as `S04.none`. `S04.none` is not a conflict in 1 long run (section "1 long run"):
   play pattern `continuous`, no S29 and no S22. `S05.manual_then_automate` is a conflict: its manual phase
   is a phase change that the laws do not cover (BM section 7).
   Reason: the solver does not leave the laws to get a number.
   A variant conflict sends the solve to the simulation path (`references/simulation-path.md`): the solver writes
   the model of `pw_scaffold.py`, fits it by simulation, and runs the proof sheet. Its report starts with
   "(simulation path)" and has the lists "Proved" and "Simulated, not proved".
4. **p, R and q.** `p` keeps its default (0.5, the top of 0.21 to 0.5). The solver takes `s` from a
   fixed list (1.1 up to 3.0) and stops at the first `s` that is ready. `s` stays at or below the
   largest `s` that PO4 permits (4.2 for a gap of 2). Then `R = 1 - p / s` and `q = (1 - R) / p`, so
   `lambda = 1`.
   Reason: PO3 needs `s > 1`, so with `p <= 0.5` the run needs `R > 0.5`. Worked example 2 used
   `p = 3` with `R = 0.2`; that `p` is outside the proven range (weak point W1).
5. **The lines.** 1 additive generator (cost ratio of the S01 variant, cap 40; 1 long run: no cap and count
   steps, section "1 long run") and 2 multiplier groups
   with `r = R / 2` each. Each group takes the finest steps inside the range: `d` at its minimum 1.1
   and `b = d^(1/r)`.
   Reason: coarse steps (b = 2) keep the trajectory far from `A x^alpha` for hours (BM section 7,
   limit 6). The simulated runs then do not follow L6.
6. **Presence.** `f_open = 1 / a` with the active factor `a`. With `continuous`, the closed game pays 0.
   With `check_in`, S06 gives `f_closed` and the cap. S08 gives a bank currency with its period and
   cap, and 1 bank line with a cap of 1 level.
   Reason: an active income term is the scale tool of L5. A capped bank line adds a constant factor
   and nothing to R (L7). An uncapped bank line is a second clock, and it moved the trajectory by a
   factor of 4 in the first test.
7. **c.** `A` comes from the no-reset trajectory of the model (the function of the proof sheet).
   Then `c = A (K T_X)^alpha X^(-alpha/s)` with `T_X = run_min`. `K` is the constant of the lines of the
   reset currency (BM L6, `K = ((b - 1) / a1)^r` for each uncapped line); with no such line `K = 1`.
   Reason: L6 gives the run length from `c`. The solver computes `c`; it does not tune it. Without `K`, the
   simulated runs of a test game were 46% longer than `T_X`. When the proof shows that the later runs
   end at the run floor (outside the domain of L6), the solver solves the round again with `K = 1`.
   A PV fail with a dead cap raise does not stop the rounds of s: a larger s gives cap room (BM L7).
8. **The spine.** `w` is the S03 default (2.8) when it gives at least the unlock target for each
   hour; else `w = e^(alpha ldot / u)` for the target `u`. The first stages sit on the trajectory of
   active_greedy at the target times of the first hour. `N` comes from L8.
   Reason: note that `alpha q = 1 / p`. So the growth of `ln P` for each hour does not depend on R:
   with `p = 0.5` and 15-minute runs it is 5.5, and `w = 2.8` gives 5.4 unlocks for each hour.
9. **Ends (continuous only).** The model gets `game.play_pattern`, and `game.targets` gets `hard_end`
   (the last stage N), `largest_purchase_gap_minutes` and `first_purchase_minutes`. With a demo end, it
   also gets `demo_end`: the stage that active_greedy reaches at the demo time (the first-hour table
   before 1 hour, else L8). Then the solver puts 1 content slot in each part past the demo: a milestone
   `content_<k>` with no effect at the stage of the middle of the part.
   Reason: the proof row PC3 needs new content in each part (`SG:185`). The solver does not invent
   that content. The slot shows where it goes, and the designer fills it. An empty slot is not content: PC3 fails
   for its part until the slot has a named element with an effect, a grant, or a line that needs it.
10. **Prove.** The solver writes the model and runs the proof sheet with strict ranges and the speed
   test. For `continuous`, the sheet has the rows PC1 to PC3 (`references/proof-sheet.md`). With
   `extend_to_h`, it also proves the longer model. A round is ready only when both are ready. Else the
   next `s` is tried. The round list stops early when only PC4 fails, or when an element is not fitted: a larger
   `s` adds no content. A round proves without the speed test, and only a ready round and the last round run it.
   With no ready round, the solver solves the round with the fewest failed rows again (`best_round`). Among equal
   rounds it takes the fewest failed rows that are not content rows (PC2, PC4, PC5, PC6, PV, PO13), then the fewest
   identity findings, changed elements and changed fields of row CK (a larger s gives the caps more room), then
   the first. The model, the proof and the report are of that round, and the report names it below the round list.

S15 (prestige shop) gets its reward through the reset reward `M = (1 + L)^q` of the model. A shop
line with steps lags that formula by up to 1 step. In the first test, that lag made the runs about
15% longer than L6, and the unlock gaps of the simulation then differed from L8 by more than 20%.

## How to read the report

1. **Ready line and rounds.** Each round names `s` and its failed obligations. A short game can need several rounds:
   the simulated policy gap of a short game can differ from the formula by more than 20%.
2. **Selected systems.** A system that the solver does not write as a line (for example S13 or S23)
   is marked. Simulation and judgment balance it. Each conflict report also has the sections "Systems and marks"
   (each selected system with the mark `law`, `simulated` or `not written`) and "Targets checked" (each target that
   the solver could check before it stopped). A selected system that the solver does not write and that no element
   of the model writes (S12: a milestone at a level of a line; S13: a cap effect on a capped line) makes the result
   not ready, and the section "Selected systems not written" names it.
3. **Target warnings** (step 2).
4. **Hard end and demo end** (continuous only): the stage of each, the rows PC1 to PC3, and the
   content slots.
5. **Parameters.** Each row: the value, the proven range (or "derived"), the source, and the target
   or obligation that set it. "Moved from the default" names the reason.
6. **Idle value.** The L4b values and the simulated values. A point outside the domain of L4b
   (`kappa u < 1`) shows "formula not valid here (kappa u < 1); simulated value used".
7. **Play-test speed.** The largest speed from the formula and from the simulation (requirement R13).
8. **Extension.** The stage count of the longer game and its proof sheet.
9. **Proof sheet.** The full sheet of `references/proof-sheet.md`.

A skeleton with no `--content` can fail PC4: the report then asks for more unlocks. `tests/designs/` keeps the
results of the test games; the references give no result of a named game.

## Rebalance mode

`--content <file>` reads the content file of a game (`scan_godot.py --content-out`,
`references/reverse-engineering.md` section 7) and keeps each element in the model. The solve of the reset loop (p,
s, R, q, c, the spine) stays as above. `scripts/pw_rebalance.py` holds the rules below.

### The content file

The solver owns the format of the content file. `scan_godot.py --content-out` writes it, and a person corrects it
(`references/reverse-engineering.md` section 7). The loader (`pw_rebalance.read_content`) reads this format and the
older format of version 2 (the keys in brackets). It turns each element into 1 internal form before the solve.

Top keys:

| Key | Content |
| --- | --- |
| `currencies.main` | The currency that buys most lines. Never null: the loader stops with exit code 2 and names the key |
| `currencies.reset` | The reset currency. Never null when the content has a reset (an element with `system` S04) |
| `currencies.side` | Each other cost currency (a list) |
| `currencies.unsure` | Optional. A reason, only when the scan is not sure of `main` or `reset` |
| `stats` | `{stat: currency}`. A stat that maps to the main currency is a part of the main income |
| `spine_stat` | Optional. The stat that the stages test (default: the main currency). The model as built keeps it as a currency of its own. The rebalance merges it into the main income when `stats` maps it there, and the report states this in "Limits of the rebalance" |
| `base_income` | Optional. A list of `{stat, value, active_only}`: the income that the game pays with no purchase. The model as built uses it. The rebalance keeps its own generator start |
| `elements` | 1 entry for each purchase line, reward, system and grant |

Keys of each element:

| Key | Content |
| --- | --- |
| `id`, `name`, `kind` | `kind` is `line`, `reward`, `system` or `grant` |
| `currency` | The cost currency of a line |
| `cost` | `base`, `ratio`, and the optional `formula`: `exp` (default, `base x ratio^n`) or `linear` (`base + ratio x n`) |
| `levels` | An integer: the count of levels. 1 is a single purchase. null is no limit (`cap`) |
| `effects` | A list. Each effect has `stat` (the stat or the id of a line for a cap), `kind` (`add`, `pct`, `mult`, `cap`), `value` and `active_only` (true: it pays only while the player plays, S05). The old key `target` has the same value as `stat` |
| `unlock` | 1 of `{}` (open at the start), `{"stage": n}`, `{"total": currency, "amount": x}` (a lifetime total), `{"resets": k}` (a count of resets), `{"milestone": id}` (it opens with another element; old `{"system": id}`), `{"level": id, "n": k}` (line `id` at level `k`). Another key stops the loader with exit code 2 |
| `role` | null, or a hint: `generator` (the add line that the game automates), `manual` (the manual action of S05), `main`, `support`, `late`, `permanent` |
| `values` | A side system: `rate_per_second` (the income of its currency), or the old keys `initial_beds x dew_per_bed / drip_seconds`. The stages (S03): `base`, `growth`, `test` (`rate`: the income for each second, or `run_total`: the total of the run) and `stat`. The reset (S04): `formula` (`power`: `(E / divisor)^exponent`, or `exp`: `k x growth^stage`), `min_gain`, `min_run_seconds`, `floor` (true: the gain is rounded down) and `keeps_stage`. The rebalance replaces the values of S03 and S04 with its own solve. The model as built uses them |
| `value` of a grant | `base`, `growth`, the optional `currency` (default: the main currency), `trigger` (`stage`, default, or `reset`) and the optional `count` (the game pays it at most `count` times) |
| `place` | The file and the line in the game |

The old element keys `family`, `affects`, `value` and `cap` still load: with no `effects`, the loader makes 1 effect from
`family`, `affects` and `value`, and with no `levels`, it takes `cap`.

### Roles

The hint `role` of the content file decides, else the default rule:

1. `generator`: the line with the hint `generator`, else the first add line of the main currency with no
   `active_only` effect (`generator_of`). It opens at the start.
2. `manual`: the line with the hint `manual`. It is the manual action of S05. It keeps the family add and its
   `active_only` flag, and its unlock does not move. It takes the value, the cost ratio and the levels of the
   generator rule (`fit_manual`).
3. `main`: the first 2 multiplicative lines of the main currency that no cap effect raises, the generator, the stages
   and the reset.
4. `permanent`: each line of the reset currency.
5. `support`: each other element.

### Parameters of each element

**The method.** The effects come from the game: each step, value, cap and cost ratio keeps its game value when the
laws permit it, else it moves into the proven range of its system, else the solver changes it and names the
obligation. The timing comes from the timing fit by simulation (see "The timing fit"): the walls, the unlock
points, the first costs and the reset follow the target timeline, and each effect stays. The laws are checks: a
row outside its domain uses the simulated value and says so. A reward keeps its sense and its rank, because it keeps its game value. The measure "character kept"
(row CK of the proof sheet) counts each element as kept, moved or changed; more than half changed is not ready.

Each purchase must give the player a gain that the player feels: the row PV of the proof sheet
(`references/proof-sheet.md`). The floor `f` is a designer target, default 10% (`targets.purchase_value.floor`).
The content carries the growth: each line of the main currency with a constant step is a line of R. The method and
its proof are in `references/balance-math.md`, section "Content that carries the growth".

| Element | Rule | Reason |
| --- | --- | --- |
| Main and support multiplicative line of the main currency (the game family `mult` or `pct`) | `d` is the game step, moved into [`1 + f`, `step_top`] when outside. A line keeps its game cap and its game cost ratio while its `r = ln d / ln b` fits in the cap room (a raised line first, then the smaller r); else its cost ratio rises to the b that fits, at most `b max`; else its step falls to `b max^room` when that is at least `1 + f`, so that the cap stays (`capped_lines`). The uncapped lines take their game cost ratios times 1 exponent, so that their r sum is `R - R_capped` (`scaled_ratios`). When a cost ratio passes `b max`, the uncapped steps shrink by 1 exponent (`shrink_steps`). First cost `line_first_cost` (100) | BM section 6a, items 6 to 8. A bound cap drops lambda to `(1 - R) / (1 - R_end)`, and PO2 permits 20% |
| Main line unlock | At 1 minute and 4 minutes (m1 and m2 of worked example 2). Each other line of R opens inside the first hour | R holds in each run (L6) |
| Additive line of the main income (each line of the main currency with the family `add`) | The game value, the game first cost and the game cost ratio (moved into the nearest S01 range). The lines add to 1 pool from the smallest game value to the largest (`pool_order`): line i gets `1 + (v_i / f - P_i) / v_i` levels, at least 1, with `P_i` the base income and the earlier lines at their levels (`pool_caps`). A game cap below that count stays. A line with no game cap keeps no cap (section "Structure first"): PV then fails on it and names it, unless the designer accepts `cap:<id>`. The PV order gate (`pool_gate`): a line opens only when the line before it in the pool order is at its levels, if that smaller line falls below the floor with the full pool | Level k of the pool adds `v / (B + v (k - 1))` (BM L1, D4). Each reset sets the levels to 0, and without the gate the buyer bought a line of add 0.5 after a line of add 6 at 10 levels: 0.8% (PV). PV tests each level of each line. The game cost ratio spreads the levels in time |
| Line of the reset currency | The game step inside [`1 + f`, `d max`] and the game cost ratio inside S15.geometric. A line with a game cap keeps it. An uncapped line takes q in the unlock order while its r fits in `q_lines` of q; a line that does not fit gets the step `b^room` when that is at least `1 + f`, else a cap of 1 level (`fit_perm_mult`). The reset reward `(1 + L)^q` keeps the rest, less the share `q_held` of the factors that a count of resets opens (see "The timing fit"). They open at the first reset. The first cost is the game first cost times `seed_scale` of the timing fit | BM D8. At 60% and 90% of q, the shop line lagged the formula and the runs grew (lambda measured 0.5) |
| Cap raise (a line or a reward) | The value of the game. It opens when its line reaches its cap | Each raise opens levels of a line at the floor step. A raise of a line with no cap opens no level: PV fails and names it |
| Line of the reset currency with a linear cost, or with the family `pct` | The rule of the line of the reset currency (`fit_perm_mult`) | BM D8 needs a geometric cost and a factor for each level |
| Single purchase (`levels` 1) | A constant factor, as a reward that the player buys, with its game effects. A factor between 1 and `1 + f` rises to `1 + f`. A pct effect on the main income becomes mult `1 + v` (`single_factor`). A line of the main currency costs 20 s of the income at its unlock. A line of a side currency costs at most 80% of what the player earns of that currency before the hard end (`side_cost_scale`) | PV: the pct effects add in 1 pool, so purchase k of the pool adds only `v / (1 + v (k - 1))` (24 pct purchases of a test game fell to 3.7%) |
| Line of the main currency that feeds a side currency (its effect is on a side stat) | The rules of a side line (`feeds_side`) | Such a line adds nothing to R. As a line of R, the R of the round differed from the R of the proof |
| Additive line of a side currency | Cost and value kept, cap `1 + 1/f - B/v` | PV |
| Mult line of a side currency | Cost, step and cap kept; a step below `1 + f` rises to it. Status `simulation only` | A persistent constant factor (F of L6, measured by the proof sheet); a second clock (L7) |
| Reward | The game effects (`game_effects`): a factor on the main income between 1 and `1 + f` rises to `1 + f`; a penalty (v below 1) and each other effect keep their values. A stage unlock is scaled to N | The game value keeps the sense and the rank of each reward. The run length takes the persistent factors by the measured F (proof sheet, PO5) |
| Side system | Values kept. Status `simulation only` | BM section 7 limit 2 |
| Grant | Values kept; a grant of the reset currency times `seed_scale` of the timing fit. A grant with `trigger: stage` pays at each stage (a ladder grant); a grant with `trigger: reset` pays at each reset (`prestige.grants`). Status `simulation only` | BM section 7 limit 2 |
| S08 bank | The line `bank`, 1 charge, cost growth 1.15, x1.1, `bank_levels` levels (1; 0 gives no bank line) | PV |

**The first reset.** The reset opens at the gain `factor_actual^(-s)` and after a run of `0.5 x T_X` or more
(`first_gain`, prestige keys `min_gain` and `min_run_seconds`). Without the run floor, a content with large rewards
gave a first reset after some seconds. A first reset at the gain 1 made the PC2 gap of a test game 16.6 minutes, so
the gain rule stays.

**No factor budget.** Version 3 of the solver cut each persistent factor to a share of the budget
`(1 + 0.2)^(1 / (1 - R))`, so a factor of x1.25 fell to x1.015 and the content lost its character. Now each factor
keeps its game value. The proof sheet measures the persistent factor F that the runs hold (`F_held`) against F of the
trajectory with no reset (`F_traj`) and puts `A x F_held / F_traj` into L6. When that ratio is outside
`(1 + 0.2)^(+-1 / (1 - R))`, L6 is outside its domain and the simulation decides PO5, PO10, PO12 and PO13.

**Cap reduced.** The cap state of each line stays (section "Structure first"). A line whose cap falls below the cap of the game gets the model entry
`caps_reduced` and the warning row `cap reduced: <id>` with the reason. A line that the rule makes uncapped has
more levels than in the game, never fewer.

**The run length.** In a continuous game the spine tests the full income, and the full income peaks at the end of
each run. The rebalance mode of a continuous game sets `T_X = min(run_min, 0.8 x unlock_max_min)`. `run_min` stays the
upper limit. When the content gives `max_resets` (the game permits at most k resets) and the designer does not accept
`reset_count`, the count of resets of the game stays: `T_X = length / (k + 1)`, and a game count of resets k opens at
design count k. After the first hour, the timing fit takes 1 stage at the first new high of each run, and 1 more
stage when the last stage is `0.8 x unlock_max_min` old. The generic mode keeps `T_X = run_min`.

### The timing fit

`scripts/pw_timing.py` sets the timing numbers by simulation after the placement rounds; the effects of the content
are fixed before it. The report gives the table "Target timeline": the first purchase, each stage of the first hour
(the stage table of the targets), the first reset at T_X, the run length T_X after it, 1 stage in each later run,
the demo end and the hard end, each with the time of active_greedy.

1. **The walls.** `StageSim` plays active_greedy with a ladder of no walls. It takes a stage of the first hour at the
   first new high of the spine stat (a value at or above each value before it) at or after the target time of the stage, and after the first hour 1 stage in
   each run, at the first new high of that run. The wall of each stage is the value at that stage, and N is the
   count of stages up to the hard end. A wall equal to a new high is first passed at that new high, so the final
   model plays the same run, except while the stat stays at 1 value: the model then takes those stages together.
2. **The unlock points and the first costs.** Each pass sets the lifetime total and the first cost of 20 s of the
   income of each placed element from its run (`freeze`, then `reach_fix`).
3. **The reset counts.** A game count k of resets goes to the time `first reset + (last slot - first reset) (k - 1)
   / (k max - 1)`, and the design count is the count of resets of the timeline at that time: 1 at the first reset
   and 1 more for each T_X (`reset_map`). The game counts keep their spacing. Kept as counts, the factors of a
   test game (about 1e7) came in the first 8 resets. With `max_resets` kept, each game count stays.
4. **The share of q of these factors.** L6 holds lambda = 1 only when the reward is the only factor that a reset
   adds. The factors on the main income of each milestone, and of each capped purchase not paid in the reset
   currency, that require a count of resets (F: each mult value^levels times 1 + the sum of each pct value x levels)
   carry `q_held = ln F / (n ln(1 + X))` of q, with n the largest such count. The design and the proof read it from
   the model with 1 function (`pw_proof.held_q`). The reward keeps `q - q_lines - q_held`, at least 0.
5. **The reset.** c moves by bisection in ln c over the passes, else by 1 secant step of slope `p q / alpha`, until the
   median later run (the runs after the first that start before the hard end) is inside [0.85, 1] x T_X: PO5
   permits 20% of sigma, and PO10 needs the median gap between stages at or below sigma. A run at the run floor
   counts as below. The lines and the grants of the reset currency scale by `seed_scale = (c0 / c)^p`, so they come
   at the same point of the game. The first-reset gain is `k (E / c)^p` of active_greedy with no reset at T_X.
6. **The passes.** Each pass plays 1 time. The fit stops when the first reset and the run length agree and each
   stage of the first hour is inside 10% of its target, or after 12 passes, and it keeps the pass nearest to the
   timeline (resets first, then the stages of the first hour, then the distance of the run length). With the
   automation plan of PC6, the fit runs again with the auto-buy.
7. **The ceiling of the second income.** When each line of the second income has a game cap and the stages of the
   fit stop before the last slot, the solve runs again 1 time with the cap of the line with the largest r dropped
   (`SPINE_CEILING`). The row of the line names the reason.

**The run floor.** Factors that open at a count of resets can put lambda above 1, so that each later run ends at the
run floor. Steps 3 and 4 correct this cause.

### Unlock points

The placement runs 3 rounds inside each round of `s`. Each round solves c and the spine, runs active_greedy, and
puts each movable element in the middle of the largest gap between 2 unlocks (gap / limit, so the first hour with
its limit of 5 minutes fills first). The movable elements are the lines and the side systems of the main currency
(except the main lines and the first generator) and the cap raisers of the reset currency (after the first reset).
They keep the order of the game. Each element has a window: the lines of R open inside the first hour, a side system
that `targets.demo_end.before_system` names opens within 1 hour after the demo end, the lifetime totals of a side
currency come after that currency starts, and a cap raiser opens when its line reaches its cap. The last slot is 1
unlock gap target before the hard end. The unlock is a lifetime total of the currency at the placed time (requirement
`total`), and a line of the main currency costs 20 s of the income at its unlock. The last round does not move the
unlocks: the final model is the model of its run. After it, an element that the active player does not reach
moves down to the lifetime total of the player at the hard end (`reach_fix`). "Fitted" means that the active player
reaches the element: an element that PC5 does not reach in a round gets the status "not fitted: not reached", with
the change that PC5 names (`mark_unreached`). The mark holds for 1 round. The placement ladders have 2 times the
stages (`PLACE_HEADROOM`), so N is the stage of the run at H (PC1), not the count from the formula. In the rebalance mode,
the stages alone come each 80% of the unlock gap target: u is at least `60 / (0.8 x unlock_max_min)` for each
hour, and the first-hour table has a stage each 4 minutes.

### Fun targets

The top-level key `fun` of the targets file (else `targets.fun`) holds the bands of the fun sheet in the keys of
`references/fun-analysis.md`. The export of the tuning panel writes it. The solver copies it into `game.targets.fun`
of the model. An unknown row id or band key stops the solver with exit 2. A fun target never trades against a proof
row, and a fun row never makes the result ready or not ready.

The FM-1 band (`max_h1` and `max`, default 10 and 20 minutes) is the content gap band of the placement (`limit_at`),
with the unlock gap targets as the fallback. The placement still counts axis steps as fixed marks. The content repair
after the timing fit (`content_slots`) is in the code but off (`REPAIR_ROUNDS = 0`). It moves a movable element into
each content gap above the band and runs the passes again. In a test game, every variant of it made PO5 fail, and some
also made PC5, PC6 or PV fail. The cause: the fit moves the first reset and the stages after the placement, and a move
of an element of large effect changes the c fit. So the report names the missing content (row CG) and does not move
it.

### Stated inputs

The block `solver` of the targets file holds 4 inputs. Each one replaces a constant of version 2.

| Input | Default | Use |
| --- | --- | --- |
| `q_lines` | 0.5 | The part of q that the lines of the reset currency take. The reset reward keeps the rest |
| `bank_levels` | 1 | The levels of the bank line. 0 gives no bank line |
| `line_first_cost` | 100 | The first cost of a line of R |
| `step_top` | the largest d of S02.levelled_factor (2) | The largest step `d` of a line of R |

`tests/test_identity.py` (StatedInputsTest) solves a test game with each input 20% lower and 20% higher, and with
`bank_levels` 0 and 2.

### Automation

When the selection has S24 (auto-buy), the solver writes the plan of row PC6 into `automation` of the model and
proves again, at most 3 times. Each entry names a line and the minute from which the simulator buys it with the
auto-buy. The plan keeps a margin of 20% under the purchase rate target.

### Second income

When `spine_stat` of the content names a stat that is not a currency of the content, the stat is a second income
(BM L10). The model keeps it as its own currency. The stages test its income. A line paid in the main currency
whose first effect raises it is a line of the second income: a mult or pct line keeps its game step (moved into
[`1 + f`, `d max`] when outside), its game cost ratio (moved into the range of S02.levelled_factor when outside) and
its game cap (`spine_fit`). `R_y` is the r sum of the uncapped lines: a result, not a target, because the timing fit
sets each wall from the simulated income. An add line keeps its game value and gets the levels of the floor. The
reset reads the main income only.

### Cap raisers

A cap raise on a line of R that keeps no cap goes to the first capped line of R (row `cap target` of the element).
With no capped line of R, the row states that the game must change the element.

### Report

The report adds the section "Content": the count of elements in and out, the subsection "Character kept" (the share
line of row CK and 1 table row for each field that is not kept, with its note), the base income of the game and of
the model, 1 table row for each changed field of each element (old value, new value, reason, the obligation or target
that set it; "kept" only when the value did not change), the elements not fitted (each with the reason and 2 ways
out), the bank, the rules that the game must change (`rules` in the model: the rounding of the reset gain, the stage
after a reset, the income that the stage rule compares, when the reset opens, each with the rule of the game beside
it) and the field map (`field_map` in the model, `references/model-schema.md`) with the stat that the game reads, the
unlock kind, and the new name of a moved milestone (`new_name`). The subsection "Identity" compares each pair of lines
of the same kind (the kind of the first effect) on step, first cost, cost ratio, cap, stat, unlock and currency. Only
the shape (step, cost ratio, cap, stat) decides: a pair with no different shape trait is a finding, unless the 2 lines
have the same shape in the game (`same_in_game`). A finding makes the design not ready (exit 1).
The subsection "Limits of the rebalance" names each stat of the content that the model merges into another currency
(`merged_stats`), and the spine stat when the stages of the game test a stat that is not the main currency.
The section "Reachable" gives PC5 and each element that the active player does not reach. The section "Purchase
rate" gives PC6 and the automation plan. The section "Gaps between unlocks" gives PC4
and, for each long gap, the count of more unlocks and its minutes. The section "Purchase value" gives the row PV,
each line below the floor, and the table "What the player buys": for each 10 minutes of the active player, the
count of purchases and unlocks and the smallest and the largest gain of a purchase.
The section "Gaps between content elements" gives the row CG (`references/proof-sheet.md`) and, for each content gap
above the FM-1 band, the count of more elements and its minutes. The section "Fun rows" gives the fun result FM-1 to
FM-8 and the rows FM-1, FM-2, FM-4 and FM-5 (`FUN_SOLVER_ROWS`), each with its band and, when it is out, its cause and
the change to try. These 2 sections apply to the play pattern `continuous` only.

### Structure first

The rebalance keeps the structure of the game as hard inputs from the content file. The solver fits the costs, the
walls, the thresholds and the effect sizes inside that structure (`pw_rebalance.game_structure`, `keep_cap_state`):

| Structure | Content key | Name for `--accept-change` |
| --- | --- | --- |
| What a reset does to the stage | `values.keeps_stage` of S04 | `stage_after_reset` |
| What the stage tests: the income or the run total | `values.test` of S03 | `stage_test` |
| The rounding of the reset gain | `values.floor` of S04 | `reset_gain_rounding` |
| The most resets of the game | `values.max_resets` of S04 (read from the reset guard, or asked in the interview) | `reset_count` |
| The cap state of each line: no cap stays no cap, a cap stays a cap, and a line keeps the levels that a level unlock needs | `levels` of each line | `cap:<line id>` |

`--accept-change all` accepts each change. A key that the content file does not state is "not in the content file":
the model keeps the rule of the solver, and the report names the key.

When no round passes inside the structure, the solver does not change the structure by itself. The result is not
ready with the failed rows. The section "Structure of the game" of the report lists:

1. Kept: each rule, the count of resets and each cap state that the model keeps.
2. Changes: each structure change in the model, accepted or not. A change that the designer did not accept fails CK.
3. Options: each structure change that the method of the solver uses (1 stage in each run of a rate test that survives
   a reset, a gain with fractions, the run length of the unlock gap target, the cap of PV), with its cost and the
   failed rows that it can change. The designer decides: "the game changes this rule".

Row CK counts each structure change as a changed element and shows them in its own line. Row WS checks each step of
the wall table (`references/proof-sheet.md`). The table "Parameters" gives the ratios of the wall table, not g, when
the model has a table.

`tests/designs/` holds the results of the test games, and the tests check them. Do not read them before a trial on
a new game: a result of the same game makes the trial not blind.

### The game as built

`--accept-change NAME` (rebalance mode, more than 1 permitted) accepts 1 named change of the structure of the game
(section "Structure first"). With no such option, the model keeps the structure of the game.

`--as-built` writes the model of the content file with its numbers: each line, reward and side system as in the
game, the stages and the reset from `values` of S03 and S04, the spine stat as a currency of its own, and
`base_income`. The active player plays the whole time, so an `active_only` effect pays its full value. Run
`pw_proof.py <model> --strict-ranges --speed` on it to compare the game with the rebalance. The model as built reads the
reset policy of the simulated player (G >= X L), not the reset rule of the game: compare its count of resets with
the game before you use it as the "before".

## 1 long run

A `continuous` selection with `S04.none` (or no S04), no S29 and no S22, and no `--content`, is 1 long run to the
hard end (decision D11, starter set K9). The laws of section 3a of BM cover it, so the solver does not take the
simulation path. With `--content` and a content file of the content step (1 element or more has `opens_min`), the
solver stays on this path and places each named element (step 7). With `--content` and a content file with no
`opens_min` (rebalance mode), such a selection takes the simulation path. A game with no reset
is a valid design: its result has the same meaning as the result of a game with a reset. The solver solves 1 round,
with no `s`:

1. **The generator.** The generator of the S01 variant has no cap. It is a growth carrier (BM 3a, "The generator as
   a carrier"): it has count steps, a multiplier `m` at each `n` owned levels (card `S12.count_steps`), with
   `r_eff = ln m / (n ln b)`. `n` is the selection parameter `n` of `S12.count_steps`, else the smallest `n` of
   the proven range (10: the finest steps). `m = b^(r n)` with `r` the share of 1 carrier, inside the proven range
   1.025 to 4. The model holds the steps as the purchase key `count_steps` (`references/model-schema.md`).
   Reason: a generator with a cap of 9 levels was complete at minute 1.2 of 150 in the first forward design. The
   proven games do the opposite: the player buys hundreds of units, and a multiplier at an owned count gives the
   jumps (`references/other-games-math.md`).
   **S12 refused.** When the selection refuses S12, the solver still adds the count steps, and the report says so in
   1 line with the reason. The way out is a generator with a cap, which then fails the row CL.
2. **The carriers.** The carriers are the generator and 1 multiplier group for each S02 entry (2 groups with 1
   entry). They share `R = 1 - 1 / (k_mid ln b)`, `r = R / carriers` each, with `k_mid` the generator level at the
   middle of the length and `b` the cost ratio of the generator. With the local exponent `1 / (k ln b)` of the
   factor `k` of the generator, the sum is 1 at the middle of the length (BM 3a). Each group has `d` at its minimum
   1.1 and `b = d^(1 / r)`.
   Reason: `R = 1` over the carriers with an uncapped additive line is the knife edge of L2. A trial solve gave an
   income of 6e97 at minute 20.
3. **The group costs.** `g = a v k_mid prod (r (b_c - 1) / a_c)^r` over the carriers (BM 3a). The solver scales
   the group costs 100, 1000, ... by 1 factor `F`, so that `g` is `g_goal = u ln(2.8) / 3600`: `u` unlocks for
   each hour at the default wall ratio. `u` is the largest of the unlock target, `60 / median gap` and
   `60 / (0.8 x unlock_max_min)`, so the stages alone keep each gap at 80% of the target (PC4). The generator
   keeps its first cost 10 and its value 0.5, so it is the first purchase and the groups open later.
4. **g and k_mid.** The solver measures `g` and `k_mid` in the run with no stage (`pw_proof.growth_run`: the slope
   of `ln I` from hour 1 to `H`). While `g` differs from `g_goal` by more than 10%, or `R + 1 / (k_mid ln b)`
   differs from 1 by more than 0.025, the carriers take a new `k_mid` and `F` takes the ratio
   `(g / g_goal)^(1 / share)`, with `share` the part of `R` of the groups. The new `k_mid` is the measured level
   moved to `g_goal`: `k + ((g_goal - g) H / 2 - ln(g_goal / g)) / ln b` (`level_at_goal`: L1 gives the spend
   `I / g` at the middle of the length, and level `k` costs `c b^k`). At most 6 steps. The test designs need 2.
5. **The spine.** `w` is the S03 default when it gives `u`, else `w = e^(3600 g / u)`. The first stages sit on the
   trajectory of active_greedy at 0.5 and 2 minutes, then each `0.8 x unlock_first_hour_max_min` minutes up to 1 hour.
   `N` comes from L8 with `u = 3600 g / ln w`. Each step of these first walls after the first step stays inside the
   `g` range of S03 (row WS, `wall_steps`): a step below the range moves its wall up, and a step above the range
   gets walls between at equal ratios.
6. **Ends, automation and proof.** As steps 9 and 10. The model has no prestige block and no reset currency. When
   PC6 gives an automation plan (S24), the solver writes it, measures `g` and solves the spine again with the
   auto-buy in the model.
7. **Named content.** With a content file of the content step, each element with `opens_min` goes into the model
   with its name (`place_named`). Its requirement is the lifetime total of active_greedy at `opens_min` in the run
   with no stage, and its first cost is 30 seconds of the income at that time. A milestone keeps its effects. A
   line with a `mult` effect is a side line with a cap; a line with an `add` effect is a second generator line with
   the cost ratio, the count steps and the `r` of the generator (BM 3a, "Content in 1 long run"). The carriers
   give the content share of the growth to the content (step 2: `R = 1 - 1 / (k_mid ln b) - R_content`), and step 4
   places the content again at each step. The model has no content slot. The report has the section "Named
   content" with the target time and the solved time of each element, and it lists each field of the content file
   that the solver does not write. `pw_design.SOLVER_WRITES` holds what the solver writes for a selected system with no content
   file: the system, the model id and a text.

The report starts with "1 long run with no reset" and the result: `ready` (exit 0) or `simulated` (exit 3). With the
result `not ready` (exit 1) the report is the conflict of the lead failed row (a balance row first; PC3 and CG, the
rows that only ask for named content, after it; PO13 last), with the section "1 long run with no
reset". Both forms have the line on the count steps, the lists "Proved", "Simulated, not proved" and "Failed", and
the section "Systems and marks". The rows PO2 to PO5 are `not applicable: no reset layer`.

**The class of the result.** PO1 passes by its formula and its clause on `g` (`references/proof-sheet.md`, "1 long
run", case 2). So 1 long run can be `ready`. It is `simulated` (exit 3) when a row is `simulated, not proved`, as the
upkeep rows are: the design can go to a build, and a play test must confirm the simulated rows.

**The skeleton has no content.** The solver writes the balance of a game: 1 generator, the groups and the stages. It
has no named content after the first minutes, so the rows CG (content gaps) and, with a demo end, PC3 (an empty slot
in each part) fail, and the result is `not ready` until the designer names the content (a content file), or sets
the content gap targets (`targets.fun`, row FM-1). The balance rows (PO1 to
PO13, PC1, PC2, PC4 to PC6, PV, CL) show whether the skeleton holds the targets.

Results of K9 with no override (`tests/designs/long-run/`) and of the forward test (`tests/designs/generator-growth/`):

| Design | Result | Rows that do not hold | Hard end | Generator | PC4 |
| --- | --- | --- | --- | --- | --- |
| `long-run/default-6h/` | not ready (exit 1) | CG | stage 54 at 5.91 h | level 330, last purchase at minute 347 of 355 | 11.07 min, first hour 5.60 min: pass (in range) |
| `long-run/length-2.5h/` | not ready (exit 1) | CG | stage 29 at 2.54 h | level 170, last purchase at minute 148 of 153 | 11.70 min, first hour 4.95 min: pass (in range) |
| `generator-growth/skeleton/` (S30, S12 refused, a demo end) | not ready (exit 1) | PC3, CG | stage 30 at 2.51 h | level 180, last purchase at minute 148 of 150 | 9.12 min, first hour 4.68 min: pass |
| `generator-growth/named/` (the same with 16 named elements) | not ready (exit 1) | PC2 (2.20 min), CG (28.98 min after the last element) | stage 28 at 2.65 h | level 140, last purchase at minute 154 of 159 | 10.95 min, first hour 5.20 min: pass (in range) |

With a content file of 11 named elements, the 2.5 hour design is `ready` (exit 0,
`assets/examples/ready-long-run/`), and the design with S30 is `simulated` (exit 3,
`assets/examples/simulated-upkeep/`).

## What to do with a conflict

The conflict names the target, the obligation, the range, and 2 or more ways out. Choose 1:

1. Relax the target to the stated value. Mark it `"unsupported": true` when no proven game supports it.
2. Select another variant. The conflict names the variant that the laws prove.
3. Add a named system, for example a presence gate.

Do not leave a proven range to get a pass. The solver never does it. If the designer insists on a
value outside the range, the proof sheet without `--strict-ranges` still reports it, and the report
must say that the value has no proven game.

Results of the 6 starter sets with the default targets (play pattern `check_in`): each set is a
conflict. Each set has `S02.add_in_group` (K2 by name, the others by the recommended variant). Its
`r = 0` gives `R = 0`, so `s = p <= 0.5` and PO3 fails for each `p` of the proven range. The sets
also name other variants that the laws do not prove: K1 `S05.click_income`, K2 `S03.regime_change`,
K3 `S04.steps` and `S08.storage`, K4 `S03.piecewise` and `S04.power_of_log`, K5 `S03.xp_doubling` and
`S04.none`. A designer who starts from a check_in set selects `S02.levelled_factor` first.

Results of the starter sets of the play pattern `continuous` with `assets/example_targets.json`,
with no override:

- K7 (the solver variants, S24 and S28): 1 round at s = 1.1, not ready on PO13, PC4 and PV. These
  are stated rows: the play-test speed, the content plan and the gain of a purchase. A skeleton
  has no content, so PC4 is the content plan.
- K9 (1 long run, no reset, no S29): not run with these targets. K9 with S30 is in section "Upkeep". The results with the default targets and with 2.5 hours are in section "1 long run".
- K8 (no reset, S29): a variant conflict on `S04.none` and `S28.full_tree`. The laws give no run length with
  no reset, so the solver takes the simulation path. The result is `not ready`, because the scaffold leaves
  `S28.full_tree` a stub that a person must write.

## What the solver does not do

- Theme and content names. The model uses generic ids (`coin`, `seed`, `dew`, `gen`, `m1`, `m2`).
  The designer names them.
- Juice, art, sound and the feel of a beat.
- The systems of BM section 7: chance, several currencies with conversion, chained producers, costs
  that are not geometric, phase changes, a second reset layer, and real-time fights. These need
  simulation and judgment. The solver lists each selected system that it does not write.
- A proof of no reset layer with S29 or S22 (`S04.none` with S29 or S22), the run on a limit S29, and the manual phase of
  `S05.manual_then_automate`. The laws of the solver need a reset layer and a constant active factor. The
  selection is valid, so the solver takes the simulation path: the result is at best `simulated`, not proved.
- The content of a slot past the demo. In the rebalance mode, the content file is the content: the solver puts no
  slot.
- A search for the best design. It takes the first ready round of a fixed list. It does not optimize
  run time.

## Limits that the first design found

1. A short game (2 hours) does not reach the asymptotes of the laws. At 900 s the trajectory of the
   example is at about 70% of `A x^alpha`. So the simulated values differ from the formulas by 10% to
   25%, and some rounds fail only because the difference is above 20%.
2. The PO7 estimate holds only for `p > 1`, and L4b holds only for `kappa u >= 1`. The proof sheet now
   states both domains (`references/proof-sheet.md`, "Domain of L4b").
