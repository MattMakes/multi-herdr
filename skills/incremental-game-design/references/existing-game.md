# The mode "Existing game": the detail of each step

`SKILL.md`, section "Existing game", gives the 10 steps and their commands. This file gives the detail of each step:
what to check, what the trials found wrong, and how to read each result. Read `references/reverse-engineering.md`
(below: the reference) for the procedure and the keys of the content file. Do not read the results of earlier
trials in `tests/` before a trial on a new game: a result of the same game makes the trial not blind.

The code is the truth of a built game. A model that copies the design note proves the note, not the game. Steps 1 to
4 describe the game as built, and steps 5 to 10 change it.

## 1. Scan

Ask the play pattern first (step 3): a Steam or desktop game is `continuous`. For a Godot project, read
`references/godot-extraction.md`, then run the scan with `--pattern`, `--out`, `--systems-out` and `--content-out`.

The report ends with the "Catalog map": the systems found, unsure and missing, the missing core systems of the
pattern, the currencies, the spine, the lines with r, the reset, the offline rule, and each parameter outside its
proven range. The line `model file` names a data file in the model format of this skill, which the scan reads first.
The scan also looks in hidden folders (a worktree): check that `project_root` in `scan.json` is the build you mean.

If the economy signal is weak, stop. Tell the user what the game has in place of an upgrade economy. For a project
that is not Godot, write the same map and content file by hand (reference sections 6 and 7).

## 2. Confirm the map and the content in the code

The scan reads words and simple formulas, and in each trial a person had to correct it. Open the file and the line
of each unsure system, and of each optional system that the scan found by its name only. Check the content file
against the code (reference section 7, "What a person must check"). Check these items, because the trials found
them wrong:

- `spine_stat` and `stats`: the stat that the stages test. When it is not the main currency and an element adds to
  it, the scan keeps it apart (`stats` maps it to itself): it is a second income (BM L10). When no element adds to
  it, the scan keeps it in the main income and writes `spine_unsure` with the 2 candidates. Read the code, and set
  it in `stats` of the edits file. A spine stat merged into the income in error makes the game as built end in
  minutes. A second income in error makes a stat with no income that never grows.
- The `values` of the stages (S03: `base`, `growth`, `test`, `stat`, and `levels`) and of the reset (S04: `formula`,
  `divisor` and `exponent` or `k` and `growth`, `min_gain`, `min_run_seconds`, `floor`, `keeps_stage`). The scan does
  not write them. The rebalance keeps each of these rules (`references/design-solver.md`, "Structure first"), so a
  wrong value changes the game.
- The 3 rules that make the model as built play like the game. The scan writes each with its code place (`evidence`
  of the reset element, `place` of `end`) and a question in `confirm` when it is not sure (reference section 7):
  - the reset rule of the player of the game: `values.policy` with `peak_tolerance` or `policy_ratio`;
  - the most resets: `values.max_resets`, from a guard such as `resets < N` in the reset test;
  - the end of the game: the content key `end`, a stage, a count of resets, a purchase or a milestone.
  Also check `values.unlock_in_run`: the reset tests the stage of the current run, not the best stage. Set
  `values.table` of the stages when the game has a table of the first stages, and `values.reward_exponent` of the
  reset when a reset pays a reward of (1 + lifetime reset currency)^exponent. A reset rule
  or an unlock test that is not the rule of the game changes the count of resets, so the model as built is not the
  game. Answer each question with `change` or `end` in the edits file.
- The `unlock` and the `role` of each line of the reset currency (`permanent`), of the manual action (`manual`) and
  of the automation (`generator`).
- Each element that the code makes and the data does not hold: a chain, an action, a burst.

Write each decision with its reason in `<work>/edits.json` (reference section 8), and scan again with `--confirm`.
An example:

```json
{"systems": {"S28": {"found": "yes", "variant": "S28.last_step", "reason": "shaft 25 is the last step"}},
 "stats": {"heat": "heat"},
 "end": {"axis_step": 25},
 "change": {"stages": {"levels": 25, "values": {"base": 6, "growth": 2.2, "test": "rate", "stat": "heat"}},
            "reset": {"values": {"policy": "peak_rate", "peak_tolerance": 0.9, "max_resets": 6}}}}
```

A `change` of `values` adds its keys to the values of the scan: the other values stay.

The 2 confirmed files are the input of each later step. Do not edit them by hand, because the edits file keeps each
reason and a new scan applies it again. The scan does not write `base_income` (the income with no purchase) or
`active_factor` (the factor of the active action, S05). Set both in the edits file, for example
`"base_income": [{"stat": "ore", "value": 1}]` and `"active_factor": {"value": 2, "place": "scripts/core.gd:40"}`.
A second income needs its own entry, for example `{"stat": "power", "value": 1}`.

Then read what the scan cannot see: the groups of the multipliers, the caps that other systems raise, the time cost
of a spine step, the clocks, and which presence state gets each income (reference steps 3 and 4). Do this before
any number, because a wrong system, stat or group changes R.

## 3. Ask the play pattern and the targets

Use design forward, step 1. The built game does not tell you what it should be. Turn each complaint of the user into
a target and its proof row:

| Complaint | Target in `targets.json` | Row |
| --- | --- | --- |
| Too slow at the start | `purchase_gap.first_max_min`, `largest_max_min`, `unlock_first_hour_max_min` | PC2, PC4, PO11 |
| The upgrades feel weak | `purchase_value.floor` (default 0.10: 10% for each level) | PV |
| Too long or too short | `length_h` (the length to the hard end) | PC1, PO12 |
| The middle drags | `purchase_gap.unlock_max_min` | PC4, PC3 |
| Active play must be clearly better | `active_factor.value` (the income factor of S05) | PO8, PO9, the idle curve |

With no person to answer, copy `assets/default_targets.json` (the default bands of group A3, no demo end) and keep
its values. Add `demo_end` only when the request names a demo.

## 4. Prove the map and the game as built

The proof of the map gives the core rows and the R and s of the code. It has no simulation, so some rows are
`not computed`. The model as built (`--as-built`) keeps each number of the game, with the spine stat as a currency
of its own. Its full proof sheet is the "before" of each comparison.

Compare the model as built with the record of the game before you use it as the "before": the first reset, the
count of resets, the run lengths and the end. A record is a results file of a simulator or a pacing test of the
project. Read it as data. Do not run project code for it. The players stop at the end (`game.targets.hard_end`) and
at `prestige.max_resets`. A gap above 20% has a cause: find it in the reset rule, the reset unlock or the end.

Exit code 2 of `--as-built` names a missing key: add it in `edits.json`. Exit code 1 of a proof means that the game
as built is not ready. Check the result against what you know of the game: a hard end in 1 minute, or an income of
0, points at a wrong stat or value in the content. List each missing core system of the play pattern before any
failed obligation, because no parameter change can pass an obligation that needs a system that the game does not
have.

## 5. Rebalance: propose the smallest change

Write `<work>/selection.json` with the systems that the game has, plus each missing core system.
Copy the shape of `assets/example_selection.json`: each system of the confirmed map, plus each missing core system. Give
each system the variant that the solver proves (design forward, step 2), and put the variant of the game in
`notes`. For a click action, estimate the active factor as `1 + clicks each second x value of 1 click / passive
income`, and state the click rate. Refuse S06 for `continuous`.

The solver keeps the id and the place of each element. It can change the family, the value, the cost, the cap and
the unlock of each, or name it as not fitted with 2 ways out. It keeps the round with the fewest failed rows. A solve takes
about 1 minute for 12 elements and about 20 minutes for 68 elements. Read these report sections:

- "Structure of the game": the rules, the count of resets and the cap states that the model keeps, each change (a
  change that the designer did not accept fails CK), and the options. An option is a decision of the designer: "the
  game changes this rule". Put each option in the verdict, with its cost. To accept 1, solve again with
  `--accept-change <name>`.

- "Content": each changed field (old value, new value, reason), and the rules that the game copies.
- "Limits of the rebalance": each stat that the model merges into the main income, and the second income (L10).
- "Gaps between unlocks": the content plan, valid for this round only (design forward, step 5).
- "Purchase value" (row PV), with each cap raise that moves to a capped line. "Reachable" (row PC5).
- "Purchase rate" (row PC6), with the automation plan when the selection has S24.

Prefer a parameter inside the family that the game uses, then another proven family of the same card, then a new
system from the catalog. Each step down the list changes more of the game that players know.

Sort each failed row into 1 of 3 cases (`SKILL.md`, "The gate"). A result "not ready" with honest rows and named
options is a correct result. A result "ready" that changes the structure of the game is not.

## 6. Design review

Do the review of `SKILL.md`, section "Design review", on the rebalance against the game as built.

## 7. Fun analysis

Run the fun sheet of `SKILL.md`, section "Fun analysis", on the rebalance with `--build <work>/model-built.json`.
The sheet then shows the value and the status of the game as built beside each row, and marks each row that the
rebalance made worse. A rebalance that fixes the proof and makes a row worse than built is a finding: report it.
The known case is the loss of character: a rebalance of a test game gave 36 of 42 lines a step below +5 % (FM-2), and
the drift of the level factors fell to 0.03 (FM-3). Only `--build` gives the drift. Read
`references/fun-analysis.md` for the rows.

## 8. Confirm with the players

Use design forward, step 6, on `<work>/model-built.json`, then on `<work>/model.json` with
`--compare <work>/<game>-built-progression.analysis.json`. The `LEFT OUT:` line describes the model. When the map
found an offline rule (S06), the game as built has offline gain: list its removal as a change.

## 9. Compare the model with the real game

Do this when the project can run its economy headless (reference section 4). Write or reuse a runner that plays the
real code with the same players, and state a tolerance, for example 10%. A difference above it is a finding: the
model lacks a rule, or the game has a bug.

The simulator buys with the auto-buy of `automation` in the model, from the minute of active play of each entry. The
model as built has no `automation`. So compare the game as built with a player with auto-buy off. Compare the
rebalance with the same auto-buy lines and minutes in the game. Check that each `field_map` stat is the stat that the
game code reads.

Run project code only when the user agrees, because a tool script can write to the save file. With no person to
answer, do not run it: name the runner of the project in the report as the next check. Else say that the model was
not compared.

## 10. Juice audit

Do it if there is a build (`references/godot-extraction.md` section 5). A system unlock with the same feedback as a
purchase is a finding. A verdict on juice needs a file and a line. With none, write "not audited".
