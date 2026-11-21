# Worked examples

`assets/examples/` holds 2 complete designs of the play pattern `continuous`. Copy the shape of a
file when you write your own. The model uses the generic ids of the solver (`coin`, `gen`, `m1`,
`m2`, `care`). `tests/test_interview_order.py` runs each file in its script.

Each folder has these files:

| File | What it is | The script that reads it |
| --- | --- | --- |
| `selection.json` | The selection record of the interview (`references/interview.md` section 8) | `scripts/pw_design.py` |
| `targets.json` | The targets. Its `notes` show the band rule and the rule of `run_min` | `scripts/pw_design.py` |
| `model.json` | The model that the solver wrote | `scripts/pw_proof.py`, `scripts/progression_workbook.py` |
| `proof.json`, `report.md` | The proof sheet and the report of the solver | read them |
| `content.json` | The content file of the content step: the named elements of the game, each with its target time `opens_min` (`references/content-step.md`) | `scripts/pw_content.py need`, `scripts/pw_design.py --content` |

Both designs have the same targets: 2.5 hours (the band 2 to 3 hours of interview A3.1 b, so
`value` 2.5 and `tolerance` 0.2), a largest purchase gap of 2 minutes, and unlock gaps of 5 and
10 minutes. Both have no reset layer, so `targets.json` has no `reset_gap` and no `run_min`.

## `assets/examples/ready-long-run/`: a ready design

Starter set K9 with no change: 1 long run with no reset layer and a hard end. The result is
`ready` (exit code 0): each proof row passes by a law or by its target. The content file names
the content of the game (section "The content file"), so the content gap row CG passes with the
default band: the largest gap with no new content is 18.73 minutes, 9.52 in the first hour.

- The laws of 1 long run (`references/balance-math.md` section 3a) prove PO1 and PO6 to PO13, and
  PV passes by its formula. The report lists them under "Proved". The marks of S04 and S24 stay
  simulated (interview B2.3 and B1.6), and the result is still `ready`, because each row passes.
- The generator has no cap. It has count steps, and the active player owns 180 levels at the
  hard end (row CL).
- The simulation checks the rows of the targets: PC1 (the hard end at stage 28 at 2.45 hours),
  PC2 (largest purchase gap 1.12 minutes) and PC4 (largest unlock gap 10.05 minutes against a
  target of about 10 with a range to 12, and 5.13 minutes in the first hour against about 5 with
  a range to 6: `pass (in range)`).
- It shows what the interview gives for a pitch with 1 run and no reset: the confirm question of
  the set, and the targets file with no reset targets.

## `assets/examples/simulated-upkeep/`: a simulated design

Starter set K9 with the upkeep card `S30.need_capacity` and 3 S02 entries: `generator`, `speed`,
and `care` with the role `care`. The result is `simulated` (exit code 3): each target is met, and
PU1, PU2 and PU4 have no law. The content file names the content, as in the example above; its
care line has the role `care`.

- It shows a selection with more than 1 entry of a system (`id` and `role`,
  `references/design-solver.md`, "Selection entries"), and the care line of the upkeep.
- PU3 is proved by its formula (a care level gains 20%). The simulation gives the rest: `u` stays
  inside 0.69 to 1 (lowest 0.783), the longest stall is 1.4 minutes, and the best purchase
  switches 46 times between producers and care.
- A simulated result can go to a build. The report lists each part that a play test must confirm.

## The content file

`content.json` is the content file of the content step (`references/content-step.md`): 1 element
for each line of the selection (it opens at the start), and 11 named elements with a target time.
The names are generic. `scripts/pw_content.py need` prints the content need of the targets (5
elements in hour 1, 9 over 150 minutes) and finds no shortfall. It is the shape to copy: change
the ids, the names and the times to the elements of your game, and keep the keys.

The solver takes the file with `--content`:

```bash
python3 <skill_dir>/scripts/pw_design.py selection.json targets.json --content content.json --out model.json --report report.md --proof proof.json
```

A selection with no reset layer and a content file of the content step stays on the law path of 1
long run (`references/design-solver.md`, "1 long run", step 7). The model holds each named element,
and the report has the section "Named content". With no content file the same selection is a
skeleton: the content gap row CG fails, and the result is `not ready`.
