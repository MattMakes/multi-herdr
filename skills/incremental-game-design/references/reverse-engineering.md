# Reverse engineering: map a built game to the catalog

This file is the procedure for a game that exists as code. The rule of the operator is: "the math
should be reverse-engineered from the code and we should try to fit it into the formulas of the
proven works in the skill." The catalog is `references/systems-catalog.md`. The math is
`references/balance-math.md` (`BM`). The scan is `scripts/scan_godot.py`.

## Contents

1. When to use this file
2. The procedure
3. A formula that fits no family
4. Model against game (requirement R12)
5. Worked case: Moss Works
6. The JSON of `--systems-out`
7. The content file (`--content-out`) and the rebalance
8. The confirm file (`--confirm`)

## 1. When to use this file

Use it when the designer has a built project and asks why the pacing fails, how to rebalance it,
or which systems it lacks. Do not use it for a design from an idea: that path starts with the
interview (`references/interview.md`).

The reason for a separate path: the code is the truth of a built game. A design note can be old,
and a model that copies the note proves the note, not the game. Each number below comes from a
file and a line, so the designer can check it.

## 2. The procedure

### Step 1. Scan

```bash
python3 scripts/scan_godot.py <project_dir> --pattern <continuous|check_in> --out <work>/scan.json \
  --systems-out <work>/systems.json --content-out <work>/content.json
```

The report ends with the section "Catalog map". `systems.json` holds the full map (section 6).
`content.json` holds the content file (section 7). The scan matches the row "Signature in code"
of each card and runs 5 analyzers: the purchase lines in the data files, the reset gain, the
offline rule, the wall formula of the spine, and the export of a sub-game. Read
`references/godot-extraction.md` for the other report sections.

`--pattern` is the play pattern of the game (`assets/systems.json`, key `play_patterns`). It sets
the core systems of the map. Without it, the map uses the core of the catalog cards, and the report
prints the missing core systems of each pattern.

Many games keep their economy in 1 data file of the model format of this skill: the keys
`purchases`, `currencies`, and `prestige`, `ladders` or `milestones`. The scan reads that file
first (map key `model_file`). It gives the spine, the reset, the rewards, the unlocks and the side
systems from the data, not from the names in the code.

### Step 2. Read the map

Each system has `found` = yes, no or unsure. Yes means a strong signature or an analyzer
matched. Unsure means only a weak signature matched, for example an input handler that can be
a menu. No means that no signature matched.

The scan matches words and simple formulas. It does not run the game. So a yes can be wrong
(a name that looks like a system), and a no can be wrong (a system with names that the table
does not know). The evidence list of each system gives the file and the line to check.

### Step 3. Confirm each unsure item in the code

Open the file and the line of each unsure item. Decide yes or no, and write the reason. Also
check each yes of an optional system: an optional system that the scan reports by its name only
is the most common false yes. Do this before any number, because a wrong system changes `R`.

Also check the items that the map marks as unsure: `currencies.unsure` (2 currencies are near in
weight) and `reset.unsure` (2 functions have a near reset shape). Write each decision in a confirm
file and run the scan again with `--confirm` (section 8). Do not edit `systems.json` or
`content.json` by hand: the confirm file keeps each decision with its reason, and a new scan applies
it again.

### Step 4. Fill what the scan cannot see

The scan does not see these items. Read them in the code:

1. The groups of the multipliers (`BM D6`). The scan gives `r` for each line, but `R` is the sum
   over groups of the largest `r` in each group. Find the function that combines the lines: a
   product over all lines makes each line its own group.
2. The caps that other systems raise (S13). A capped line adds 0 to `R` in the long run, but a
   cap that rises with progress is a line again.
3. The time cost of a spine step. A stage that clears at the moment that power is enough costs
   no time, and a wait then turns into stages in 1 visit (`BM L5`).
4. The clocks: a global time scale, real time, and a bank that fills while the game is closed.
5. Which presence state gets each income: active, open or closed (`BM D9`).

### Step 5. Compute r, R, lambda and s

Use the formulas of `BM`, in this order:

1. `r = ln(d) / ln(b)` for each multiplicative line, and `r = 0` for an additive line (`BM D5`).
   The map gives `r` when the cost and the value are both geometric.
2. `R` = the sum over groups of the largest `r` of each group (`BM L1`). Compute `R` for each
   phase of a run: before and after the caps, and before and after each unlock of a new line.
3. The reset loop: `p` is the gain exponent, and `q` is the reward exponent. The map gives `q`
   as the `r` of the best uncapped line priced in the reset currency (`BM D8`).
4. `alpha = 1 / (1 - R)`, `s = p * alpha`, `lambda = p * q * alpha` (`BM D12`).
5. Fill the proof obligations (`BM` section 3) with these values.

The reason for this order: each value needs the one before it, and a wrong `R` makes each later
value wrong. When `R >= 1` in a phase, `alpha` has no finite value. Report the phase as
finite-time growth (`BM L2`) and do not compute `lambda` for it.

### Step 6. Report the missing core systems first

List each entry of `missing_core` first, then each failed obligation. A missing core system is
the larger finding: no parameter change can make a game pass an obligation that needs a system
it does not have. For example, no offline factor gives active play a premium when the game has
no active action (S05).

### Step 7. Propose the smallest change

For each failed item, propose the smallest change that brings it into a proven range of the
catalog. Prefer, in this order:

1. A parameter change inside the family that the game uses. The `out_of_range` list names the
   parameter, its value and its proven range.
2. A change of the formula family to a proven family of the same card.
3. A new system from the catalog, with its decision and its compatibility row.

The reason for this order: a parameter change keeps the game that the players know, and each
step down the list changes more of it. Compute `R`, `lambda` and `s` again after each change,
because a change in 1 system moves the obligations of the others (`BM L7`).

## 3. A formula that fits no family

The variant tables of the catalog do not hold every formula. When a formula fits no family:

1. Name the nearest family. The scan picks the variant whose range is nearest to the value, and
   says so in `variant_reason`.
2. State the difference in 1 sentence: the term that the family does not have, or the range
   that the value is outside.
3. Decide if the difference changes the math. A constant factor or a rounding (`floor`, `ceil`)
   does not change `r`, `p` or `q`. A term that grows with the spend does.
4. When the difference changes the math, the laws do not cover the formula. Say so, and check
   it by simulation (`BM` section 7).

Example: a cap that rises by 2 for each stage milestone is not `S13.raised_cap`
(`c0 + c1 x spine level`). It is a step function of the spine. The nearest family is
`S13.raised_cap` with `c1` = 2 divided by the stages between milestones.

## 4. Model against game (requirement R12)

When the project can run its economy headless, play the real code and compare it with the
model. The reason: the scan and the model can both miss a rule, but the real code cannot.

A harness has these parts:

1. A runner script that the engine starts headless (in Godot: `godot --headless -s <script>`).
   It waits until the autoloads are ready.
2. The game time under the control of the runner. Set the time scale of the game to 0, and call
   the step function of the economy with a fixed time step.
3. The same players as the model: active greedy, active slow, open idle with a visit each 1, 8,
   24 and 168 hours, closed idle through the real save and load functions, early reset and
   patient reset (requirement R8).
4. A buyer that the game tests already use, when one exists, so that the harness and the tests
   agree on what a purchase is.
5. A table of the results: the time of each stage target, the purchases, the resets and the
   reset currency, for each player.

Compare each value of the table with the model. A difference above the tolerance (state it, for
example 10%) is a finding. Find the rule that the model lacks, or the bug in the game.

The Moss Works repository has such a harness: `tools/research/idle_vs_active.gd` with its
simulation script. An installed skill does not have these files. Use them as an example of the
parts above, not as a dependency.

## 5. Worked case: Moss Works

The fixture `assets/fixtures/moss-works-built/` is a copy of the code and data of the built
game. These are the results of the scan and of the computation of step 5.

### The map

The scan finds 8 systems, 1 unsure and 19 not found (28 systems).

| Found | Systems |
| --- | --- |
| yes | S01 (`S01.steep`), S02 (`S02.levelled_factor`), S03 (`S03.exponential`), S04 (`S04.power`), S06 (`S06.penalty_cap`), S13, S15 (`S15.geometric`), S23 (`S23.product_export`) |
| unsure | S19: `specimens_collected` in `systems/economy.gd:27`. |
| missing core | check_in: S05 Active action and the presence gate slot (S07, S08, S09). continuous: S05, S24 Automation unlock and S28 Hard end |

Step 3 confirms S19 as a partial match: each pressed specimen gives x1.25 spores and +2 to the
Trellis cap, but no set bonus exists. The nearest family is `S19.per_item`. S23 is the Dew
Garden: its own currency, and `Garden.get_spores_mult()` in the income (`systems/economy.gd:314`).

### Lines and exponents

| Line | Currency | Cost ratio `b` | Value ratio `d` | `r` | Cap |
| --- | --- | --- | --- | --- | --- |
| Trellis | spores | 2.0 | 1.6 | 0.678 | 12 |
| Spore Bank | spores | 3.0 | 2.0 | 0.631 | 5 |
| Fogger | spores | 2.2 | 1.5 | 0.514 | 15 |
| Mirror Wall | spores | 2.0 | 1.4 | 0.485 | 20 |
| Mushroom Ring | spores | 3.0 | 2.0 | 0.631 | 10 |
| Canopy Net | spores | 3.0 | 2.0 | 0.631 | 10 |
| Root Memory | mycelium | 2.0 | 2.0 | 1.000 | none |
| Deep Roots | mycelium | 2.2 | 1.5 | 0.514 | none |
| Drip Line | dew | 1.6 | 1.15 | 0.297 | none |
| Morning Mist | dew | 1.5 | 1.25 | 0.550 | none |

Mister, Grow Lamp, Compost and the Dew Bed are additive (`r = 0`). Step 4 finds that `_stat`
multiplies all multiplier lines (`systems/economy.gd:455-469`), so each line is its own group.

- `R` of the spore-income lines before the caps: 0.631 + 0.514 + 0.631 = 1.776.
- `R` of the grow lines before the caps: 0.678 + 0.485 + 0.631 = 1.794.
- `R` after the caps: 0, with log factors from the additive lines.

### Reset loop

The gain is `floor(pow(run_spores / 100000, 0.5))` (`systems/economy.gd:376`), so `p` = 0.5.
Root Memory has `r` = 1.0 with no cap, so `q` = 1.0. So `lambda = 0.5 / (1 - R)`: 0.50 at
`R = 0`, 1.00 at `R = 0.5`, and no limit as `R` goes to 1. Above `R = 1` the run has finite-time
growth until the caps stop it. The run lengths of the patient player give `lambda >= 4.6`
(`BM` worked example 1). At `R = 0`, `s = 0.5`, which is not above 1.

### Offline

`_apply_offline_progress` (`systems/save.gd:236`) pays the income times the capped time with no
other factor: rate 1.0, cap 28800 s = 8 hours (`data/stages.json`).

### Out of range

The scan lists 9 parameters outside their proven ranges:

1. `S06.penalty_cap.f_closed` = 1.0 (range 0.85 to 0.92).
2. `S06.penalty_cap.T_cap_h` = 8 (range 12 to 24).
3. `S15.geometric.b` = 2.0 for Root Memory, Rich Humus and Spore Vault, and 2.2 for Deep Roots
   (range 1.25 to 1.35).
4. `S02.add_in_group.v` = 0.25 for Compost (range 0.03 to 0.1).
5. `S01.steep.b` = 1.18 for the Grow Lamp and 1.4 for the Dew Bed (range 1.2 to 1.3).

### Findings and the smallest changes

1. Missing S05. Add `S05.active_lane` with `a` = 1.25: income x1.25 while active. This is the
   only change that gives active play an income that idle play does not get.
2. Missing presence gate. Add `S08.timed_bank`, the default of decision D2. `S07` is the other
   choice: it pairs with the spine S03 and makes a stage clear need a visit, which also fixes the
   stage clear that costs no time.
3. PO1 and PO2. Move the spore-income multipliers into 1 additive group (`S02.add_in_group`),
   and the grow multipliers into another. Then `R` = 0.631 for income and 0.678 for grow, so
   `R < 1`. With `R` = 0.678, `lambda = 1` needs `q = (1 - R) / p` = 0.64, so the value ratio of
   Root Memory becomes `2^0.64` = 1.56. Then `s = 0.5 / 0.322` = 1.55, which is above 1. These
   values are computed with the laws. A simulation must confirm them.
4. S06. Set `f_closed` = 0.85 and `T_cap_h` = 12, the reference values of the catalog.
5. S15. The cost ratio 2.0 is outside 1.25 to 1.35, but inside the range of `b_limit` (1.5 to 5)
   for a few steep perks. Keep 2.0 for Root Memory only if it is one of those perks.

## 6. The JSON of `--systems-out`

The proof-sheet script reads these keys:

| Key | Content |
| --- | --- |
| `schema_version` | 1 |
| `project` | `name` and `root` |
| `pattern` | the value of `--pattern`, or `null` |
| `systems` | 28 entries (S01 to S28): `id`, `name`, `class`, `found`, `reason`, `evidence` (file, line, text, strength, source), `variant`, `variant_reason`, `parameters` (name, value, place, variant, line). `class` follows the pattern |
| `missing_core` | each core system of the pattern with `found` = no (`kind` = system), and the presence gate slot (`kind` = slot) when it is core and no member has `found` = yes |
| `missing_core_by_pattern` | `{pattern: missing_core}` for each play pattern, also when `--pattern` is given |
| `model_file` | the data file of the model format that the scan read, or `null` |
| `data_skipped` | each data file that the scan left out, with the reason: a folder of documents (`docs`, `design`, `notes`, `research`), or a byte copy of 200 bytes or more of another data file |
| `currencies` | `main`, `reset`, `side`, and `unsure` when 2 currencies are near in weight |
| `spine` | `g` (the growth of the spine wall), `g_place`, `place`, `formula`, `function`, and from a model file also `count` and `stat` |
| `lines` | each purchase line: `id`, `currency`, `system`, `stat`, `cost_base`, `cost_family`, `cost_ratio`, `cost_formula`, `cost_place`, `value_family`, `value_key`, `value_ratio`, `value_step`, `value_place`, `r`, `cap`, `levels`, `effects` (stat, kind, value, active_only), `status` |
| `reset` | `gain_family`, `gain_formula`, `gain_place`, `p`, `divisor`, `currency`, `q`, `multiplies`, `q_line`, `q_method`, `reward_lines`, `clear_function`, `unsure` |
| `offline` | `found`, `function`, `rate`, `penalty`, `rate_reason`, `cap_seconds`, `cap_hours`, `cap_place` |
| `out_of_range` | each extracted parameter outside the `min` to `max` range of its variant |
| `end` | the end of the game: `mark`, `reason`, `place`, `sure` and `confirm` (the order of the scan, item 7) |
| `confirmed` | only after `--confirm`: each edit that the scan applied |

A place is `file:line` relative to the project root. A value that the scan cannot read is
`null`, and the reason fields say why.

The scan finds these values in this order:

1. The main currency is the cost currency with the most purchase levels. A line with no level limit
   counts as 25 levels. On a tie, the currency of the base income wins.
2. The reset is the function that clears 2 or more saved values and adds to another value. A reset
   name (`prestige`, `rebirth`, `ascend` and others) adds to its score. When 2 functions
   are near in score, `reset.unsure` names the other one.
3. The offline rule is in a function that has an offline word and a read of the clock. A gain of
   `income() * offline_seconds` with no cap gives rate 1.0 and `cap_seconds` = `null`. A factor after the cap call
   on the same line (`min(away, cap) * rate()`) multiplies the rate. A call with no argument (`Core.rate()`) has the
   value of the return line of its function, for example a data key. When the scan finds the function but cannot
   read the rate, `found` is `"unsure"`. A confirmed S06 `"no"` clears the offline rule of the map.
4. A line with `levels` = 1 (a single purchase) or a cost that is not geometric has no range row.
5. The reset rule of the player (`reset.rule`): `prestige.policy` of a model file, else the profile of
   `game.primary_profile`, else a rule of a player script (also in the tests and the tools): a peak rule
   `rate > X * best` or a ratio rule `gain >= X * lifetime`. A rule in a script that extends the script of another
   rule comes first. A rule whose factor has the name `patient` or `slow` comes last. Then the default policy of a
   loader of the model format. With none, the rule is `ratio` 1.0 with a question.
6. The most resets (`reset.max_resets`): a guard `resets < N` or `resets <= N` (also `prestiges`) in a function that
   tests or applies the reset. A value from a code default (not from data) gets a question.
7. The end (`end` of the map and of the content file): the last reset when the game has a most count of resets, else
   the last stage of the spine. It always gets a question, because a game can also end at a purchase or a milestone.
8. A reset that needs a run total of the spine stat needs a stage of the current run. Its unlock is that stage, and
   `values.unlock_in_run` is true.

9. The spine of a data rule: a data file with a rule key (`stage_rule`, `level_rule` and others) with a word value,
   a list of numbers (`stage_requirements`, a key that ends in `requirements`, `thresholds`, `table` or `targets`)
   and a growth key with a word of the list key (`requirement_growth`). The code selects its stage function by the
   rule, so another wall key of the same file (an hp growth) is not the spine. The stages get `values.table`
   (the amounts of the first stages), `growth` (for each later stage), `base` and `test`.
10. An unlock key of a count of resets (`unlock_prestiges`, `unlock_resets` and others) gives `{"resets": n}`, not a
    total of a currency.
11. A side system `x` opens by a data key `x_unlock_<...>` that the code reads (for example
    `garden_unlock_lifetime_spores`), before an unlock key of its own file that the code does not read.

The scan does not enter a folder that is not a part of the game: a folder with a `.gdignore` file (Godot does not
import it), a folder with a `SKILL.md` file (an agent skill, with example models of other games), and each folder of
`--exclude <folder>` (relative to the project root; repeat the option for more folders).

## 7. The content file (`--content-out`) and the rebalance

`scan_godot.py <project> --content-out content.json` writes the content file of a built game. The design solver
reads it in its rebalance mode (`pw_design.py --content`, `references/design-solver.md`). The solver keeps each
element: it gives each 1 parameters, or names it as not fitted.

### Keys

| Key | Content |
| --- | --- |
| `currencies` | `main` (the most common cost currency other than the reset currency), `reset` (the reset currency of the map), `side` (each other cost currency) |
| `stats` | `{stat: currency}`: a stat that a line of the main currency affects is a part of the main income. The model merges these stats into 1 income. The spine stat of a second income maps to itself (item 6) |
| `elements` | 1 entry for each purchase line, each reward, each system and each grant |
| `end` | `{"mark": {...}, "reason", "place", "sure", "confirm"}`: the end of the game. `mark` is 1 of `{"axis_step": n}`, `{"resets": k}`, `{"purchase": id}`, `{"milestone": id}`. `pw_design.py --as-built` copies `mark` into `game.targets.hard_end` |

The reset element (system S04) also has `values` from the scan: `policy` with `peak_tolerance` or `policy_ratio`,
`max_resets` when the code has a guard, and `unlock_in_run` when the reset tests the stage of the current run. Its
key `evidence` gives the code place of each value, and `confirm` lists the questions of the values that the scan is
not sure of. `--as-built` copies these values into `prestige` of the model. `values.reward_exponent` (the reward
income x (1 + lifetime reset currency)^exponent) becomes `prestige.reward` on the main currency.

The stages element (system S03) has `values` from a data spine: `base`, `growth`, `test`, and `table` (the amounts
of the first stages) for a data rule (scan order item 9). `--as-built` copies `table` into `requires.table` of the
stage ladder.

2 content keys come only from the confirm file: `base_income` (the income with no purchase, a list of
`{stat, value, active_only}`) and `active_factor` (`{value, stat, place}`: the factor of the active action of
system S05 while the player is active). `--as-built` writes `active_factor` as a milestone with an active effect.
The rebalance ignores `active_factor`, because it writes S05 itself.

Each element has `id`, `name`, `kind` (`line`, `reward`, `system`, `grant`), `system` (the catalog card), `currency`
(the cost currency), `affects` (the stat), `family` (`add`, `pct`, `mult`, `cap`, or the variant of a system),
`cost` (`base`, `ratio`, and `formula` = `exp` or `linear`), `value`, `effects`, `levels`, `cap`, `unlock`, `role`
and `place`. `references/design-solver.md`, section "The content file", is the full format.

1. `currencies.main` and `currencies.reset` are never `null`. When the game has no clear reset currency, the scan
   writes its best guess and names the other choices in `currencies.unsure`.
2. Each effect has `stat`, `kind` (`add`, `pct`, `mult`, `cap`), `value` and `active_only` (the effect applies
   only while the player is active).
3. `levels` is the level limit: 1 is a single purchase, `null` is no limit.
4. `unlock` is `{}`, `{"stage": n}`, `{"total": currency, "amount": x}` (a lifetime total), `{"resets": n}`,
   `{"milestone": id}` or `{"level": id, "n": n}`. When the data has 2 or more unlock keys, `unlock` holds the most
   specific one, and the element key `unlock_also` holds the others.
5. `role` is `manual` for an add line of the active action, and `generator` for an add line of an automation loop.
6. `spine_stat` is the stat that the spine tests, when it is not the main income. When an element or a base income
   of the model file adds to it, it is a second income (BM L10): `stats` maps it to itself, and the solver keeps it
   as its own currency. When nothing adds to it, `stats` maps it to the main currency, and the key `spine_unsure`
   names the 2 candidates. A person reads the code and sets it with `stats` or `spine_stat` of the confirm file.
7. A side system has `values.rate_per_second` when the scan reads its rate. A grant has `value.currency` and
   `value.trigger`.

The scan finds the elements in this way:

1. Lines: the purchase lines of the catalog map.
2. Rewards: each data entry with an id, an effect and an unlock key that is not a line (Moss Works: the 7 pressed
   specimens, Leaf Mould, Glass Dome). In a model file: each milestone with effects.
3. Systems: the stages (S03, with the data key `count`, or a ladder of the model file), the reset (S04, with
   `prestige_unlock_stage` or the unlock of the `prestige` block), and 1 side system for each side currency (S23:
   the data file of its lines and its `unlock_stage`).
4. Grants: `reward_base` and `reward_growth` give a grant of the main currency at each stage. In a model file: the
   grants of each ladder step.
5. Stats: a stat that a line of the main currency affects maps to the main currency. A stat that names a cost
   currency maps to that currency. The spine stat of a second income maps to itself (key item 6).

### What a person must check in the content file

1. Each element of the game is in the list. The scan sees only data entries. A line or a reward that the code
   makes, a burst, a timed event or a hidden feature is absent: add it by hand.
2. `family` matches the code. The scan reads the data key (`mult`, `add`, `bonus_pct`, `cap`). Read the code that
   applies the effect: a `mult` that adds inside a pool is `pct`.
3. `stats` names each stat of the main income. A stat that the game keeps apart (for example a damage stat that is
   not income) needs its own currency.
4. `unlock` is the point where the game opens the element. An unlock in code (a milestone, a story flag) is absent.
5. `role` is null. Set a role where the default of the solver is wrong: `main` (an uncapped line that carries R),
   `support` (a capped constant factor), `late`, or `permanent` (a line of the reset currency).
6. The side system `values` (Moss Works: `drip_seconds`, `initial_beds`, `dew_per_bed`) give the income of the side
   currency. Check them against the code.

### The field map of the result

The model key `field_map` (`references/model-schema.md`) tells the person who edits the game where each model value
goes. Each entry names the game id, the data file and line (`place`), and the game fields to set. 2 more fields
help to check the map against the code:

1. `stat`: the game stat of each effect and the model target, as `grow_power -> coin`. A cap effect shows as
   `trellis -> trellis (cap)`. Check that each stat of the main income maps to the main currency.
2. `unlock`: the unlock kind and the game field. The kinds are `lifetime total: <field>` (Moss Works
   `unlock_lifetime_spores`), `count of resets: <field>` (`unlock_prestiges`), `stage: <field>` (`unlock_stage`),
   `milestone: opens with the element <id>` and `time: <field>` (`unlock_seconds`). A model unlock of a kind that
   the game does not have needs a new field in the game.

### Moss Works

`tests/designs/moss-works-full/content.json` is the content file of `assets/fixtures/moss-works-built/`: 29
elements. The scan wrote it again for the fields `effects[].stat`, `effects[].active_only` and `levels`. The solve
gives the same model as before (`tests/test_rebalance.py`). 16 lines (9 jar lines, 4 Regrow lines, the Dew Bed and 2 tonics), 9 rewards (7 specimens, Leaf Mould,
Glass Dome), 3 systems (stages, reset, garden) and 1 grant (stage rewards). The Lid Dew bank and its burst are not
in the fixture, so the content file has no bank; the solver adds the bank from the selection (S08).


## 8. The confirm file (`--confirm`)

The scan reads words and simple formulas, so some items of the map need a person (step 3). The confirm file holds
the decisions of that person. The scan applies it to the map and to the content file:

```bash
python3 scripts/scan_godot.py <project_dir> --pattern <pattern> --confirm <work>/edits.json \
  --systems-out <work>/systems-confirmed.json --content-out <work>/content-confirmed.json
```

Keep the files of the first scan (`systems.json`, `content.json`). The confirmed files are the input of the next
steps. The reason for a file of edits: the reason of each decision stays with the decision, and the scan can apply
the decisions again after a change to the game.

The confirm file is a JSON object. Each key is optional:

| Key | Content | Applies to |
| --- | --- | --- |
| `systems` | `{id: {"found": "yes" or "no" or "unsure", "variant": id, "reason": text}}` | map. The reason becomes `confirmed in the code: <reason>`. The scan computes `missing_core` again |
| `currencies` | `{"main": id, "reset": id, "side": [ids]}` | map and content. The key `unsure` goes |
| `reset` | keys of the map `reset`, for example `{"p": 0.5, "divisor": 2500, "currency": "relic", "function": "file:line"}` | map. In the content file, `p` sets the value of the reset element, and `currency`, `gain_family` and `function` set its currency, family and place |
| `stats` | `{stat: currency}` | content. A stat that is `spine_stat` makes the key `spine_unsure` go |
| `spine_stat` | a stat | content. The key `spine_unsure` goes |
| `add` | a list of elements in the content format | content |
| `remove` | a list of element ids | content |
| `end` | 1 mark: `{"axis_step": n}`, `{"resets": k}`, `{"purchase": id}` or `{"milestone": id}` | content: the key `end`, sure |
| `change` | `{id: {key: value}}`: each key replaces the key of the element. The keys of `values` add to the values of the scan | content |
| `notes` | a list of text | map and content, in `confirmed` |

Example:

```json
{
 "systems": {"S28": {"found": "yes", "variant": "S28.last_step", "reason": "shaft 25 is the last step (data:42)"},
             "S10": {"found": "no", "reason": "lucky_vein is a milestone, not a visitor"}},
 "currencies": {"main": "ore", "reset": "relic", "side": ["salt"]},
 "end": {"axis_step": 25},
 "change": {"cart": {"role": "generator"}, "reset": {"values": {"max_resets": 6}}},
 "notes": ["the lamp chain is in code only: it is not in the content file"]
}
```

An unknown key, an unknown system id or an unknown element id stops the scan with exit code 1 and a message. Both
output files get the key `confirmed`: the list of the edits that the scan applied.
