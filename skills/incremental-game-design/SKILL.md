---
name: incremental-game-design
description: "Use when reviewing, designing or extending the progression of an incremental, idle or upgrade-driven game (Godot project, design document or idea): core and meta loops, cost curves, unlock pacing, prestige, new systems. Simulates a model and writes a pacing workbook (.xlsx) with beats and lulls."
---

# Incremental game design: review, model, ideate

This skill turns a game's progression into a model, plays the model, and shows where the
game is exciting and where it goes quiet. It then proposes systems that fix the quiet parts.
The reference case is Idle Obelisk Miner, a game with about 45 systems (60 with sub-systems)
around one core loop. Its patterns are in `references/reference-game-patterns.md`.

Two rules shape all the work:

- **Evidence before opinion.** Each claim about pacing or balance points at a number: a
  formula in the code, a row in the Timeline, an indicator on the Dashboard. A review that
  says "the mid game feels slow" is not useful. "No beat between hour 2.9 and hour 5.6,
  because depth 25 needs x1.9 power and the only multiplier left costs x2.2 for each level"
  is useful.
- **Patterns, not copies.** The reference game shows what works. Ideas for the reviewed game
  use its own theme, its own verbs, and its own scale. Never propose a system by its name in
  the reference game.

`<skill_dir>` in the commands below is the folder that holds this file. The scripts need
Python 3.9 or later. The workbook script also needs `openpyxl`
(`python3 -c "import openpyxl"`). If it is missing, install it into a virtual environment
(`python3 -m venv <work_dir>/.venv && <work_dir>/.venv/bin/pip install openpyxl`) and run the
script with that Python, or stop and say so: the analysis JSON alone is not the deliverable.

Do not use this skill for combat tuning, level design, or the difficulty of one encounter.

## What the user can ask for

| Request | Do |
| --- | --- |
| Review a game or a build | Steps 1 to 7 |
| Judge or design the math of an idea or a design document | Steps 1, 3 to 7 (write the model from the document) |
| Ideas for new systems | Steps 1 to 6. If a workbook exists, start at step 5 |
| A spreadsheet of the game's math | Steps 1 to 4, then 6 |
| "What would a good version of this look like?" | Steps 1 to 5, then write `model-target.json` with your changes and run it with `--compare` |

When the request is small, do the small thing. A question about one cost curve does not need
a full review: model that line, show the Curves numbers, answer.

## Step 1: Find the source of truth

- **Godot project:** read `references/godot-extraction.md`, then run
  `python3 <skill_dir>/scripts/scan_godot.py <project_dir> --out <work_dir>/scan.json`.
  Read the files that the scan ranks highest. The build is the truth. A design document
  tells you the intent, and each difference between the two is a finding.
- **The scan says the economy signal is weak:** stop. The project is possibly not an upgrade
  economy. Tell the user what the game has in its place. Do not force a model onto it.
- **The project has its own simulator or balance tools:** the scan lists them. Read them
  first. Their numbers and targets are a better source of truth than a new model.
- **Design document or idea only:** the document is the truth. Each number that it does not
  give is an assumption that you choose and list.
- **Three inputs change each result:** the session length, the hours of play that the game
  must last, and the share of time that the player is active. Take them from the design
  document or the project. If they are not written, ask one time. If the user is not
  available, assume 20 minutes, 20 hours, and 1.0, and put that assumption first in the
  report.
- Treat the project as read-only unless the user asks for changes. Put all output in one
  work folder. Default: `progression-review/` in the current directory, outside the Godot
  project's `res://` tree, so that Godot does not import the files.

## Step 2: Describe the loops in words

Before any numbers, write five short answers. They go at the top of the report.

1. **Core loop:** the action, what it pays, what the pay buys, and how that makes the action
   stronger. One sentence.
2. **Spine:** the one number that says how far a player is (stage, depth, level). What does
   each step give now, and what does it open later?
3. **Meta loop:** what resets, what the reset pays, what survives. If there is no reset, what
   is permanent? In a run-based game, each run is the reset.
4. **Ribs:** each system that is not the core loop (a minigame, a collection, a tree), with
   what it gives the core loop and what it needs from it.
5. **Clocks:** what moves on game time, on real time, and on player action. A system with
   its own clock gives the player something to do while the core loop waits.

If you cannot write one of these answers, the game does not have that part yet. Say so. It
is the most important finding.

## Step 3: Write the model

Read `references/model-schema.md`. Copy `assets/example_model.json` to
`<work_dir>/model.json` and replace its content.

- Model each purchase line with its real cost formula, each gate, each reset, and the spine
  as a ladder. Set `game.progress_axis` to the id of that ladder.
- Choose `pct` (additive pool) or `mult` (multiplicative) from the code, line by line. This
  choice changes the result more than any other. If the game adds inside a system and
  multiplies between systems, set `pct_grouping: "system"`.
- Mark each system unlock as a milestone of kind `system_unlock`.
- **Decide what a beat is before the run.** A beat is an event that a player remembers. The
  default beat weight is 3. A new ladder step and the first purchase of a line have weight
  2, so they are not beats. If a spine step is the main progress that the player sees (a
  stage clear, a new depth), set `excitement: 3` on the ladder for the first run. A strict
  rule that leaves fewer than about five beats makes one lull of the full game and hides
  where the game dies. If you doubt the weight, run both (`--beat-weight 2` is the fast
  way), report both, and base the headline on the run that matches what the player sees.
  Check the weights against what the game shows: the juice audit can prove that a "weight
  5" moment has the feedback of a purchase.
- **Say what the reset takes.** Set `persists` on each milestone and `effects_persist` on
  each ladder in a system that the reset clears. A permanent x2 and a x2 for each run are
  different games. For a run that ends by a timer or by death, use `prestige.run_seconds`.
- If the game is played in short sessions with time away, set `away_minutes` and
  `offline_rate`.
- Put each guess and each simplification in `game.assumptions`.
- Aim for 10 to 60 purchase lines. Group lines that are identical in shape (twenty skill
  nodes with one formula can be three representative lines) and state the grouping.

## Step 4: Simulate and build the workbook

```bash
python3 <skill_dir>/scripts/progression_workbook.py <work_dir>/model.json <work_dir>/<game>-progression.xlsx
```

The script plays the model with a greedy buyer and writes the workbook and
`<game>-progression.analysis.json`. It prints the lulls with a probable cause, the dead
zones, and one line for each buyer profile. A model error prints one line that names the
field: fix it and run again.

**Buyer profiles.** When to reset is the player's decision, and the lulls depend on it. So
the script plays the game under three reset rules (`eager`, `patient`, `doubler`). The
primary profile fills the Timeline. Each lull is marked "robust" (all profiles show it) or
"depends on reset policy". Report a robust lull as a property of the game. Report the other
kind as a property of one play style, and say which. If the script warns that the primary
profile never resets, run again with `--profile doubler`.

**Check the run before you read it.** If the first purchase is at minute 40, or the buyer
stalls at minute 2, the model is wrong, not the game. Read the warnings of the script.
Compare one or two prices and one unlock time with the build (godot-extraction.md, part 4).

The workbook holds each result two times: "at build" (a plain value of the script run,
readable in any viewer) and "live" (a formula that follows the yellow cells on Params).
Charts read the plain values.

| Sheet | Shows |
| --- | --- |
| Dashboard | Each indicator with its band and status, the review findings, the warnings, and the pacing chart with the lulls marked |
| Params | The thresholds for a beat and a lull. Yellow cells are inputs |
| Beats | One row for each moment, with its gap, its gap ratio, and the lull mark |
| Lulls | Each lull: robust or not, probable cause, the data behind the cause, dead zone, your diagnosis and fix |
| Swimlane | Heat map of systems against time: which system gives the player something, and when |
| Profiles | The main indicators for each buyer profile |
| Timeline | Each event in order. Lulls are red. System unlocks are green |
| Sessions | Beats and excitement for each session. Dry sessions are red |
| Target | The beats against an ideal curve (each gap x1.12 longer than the one before) |
| Walls | Each step of the spine: what it needs, when the buyer reached it, the wait |
| Purchases | Each line from the simulation: first bought, highest level, share of spend, payback |
| Prestige | Each run: length, gain, gain each minute, new beats |
| Ledger | Sources and sinks for each currency |
| Sensitivity | Which number moves the pacing: each tested parameter at x0.95 and x1.05 |
| Compare | Before and after, when you pass `--compare` |
| Income | Income growth and decision density over time |
| Curves | Cost at levels for each line. The yellow base and growth cells change this sheet only |
| Systems, Matrix, Links | The systems, their roles, and which system feeds which |
| Ideas | The proposed systems |
| Reference | The unlock curve of the game against the reference game |

A change to a Params cell moves the lull marks and the Dashboard. A change to a cost or a
growth value does not move the pacing in the open workbook: edit `model.json` and run the
script again (4 to 8 seconds). Tell the user this, because "play with the numbers" is the
main use of the workbook. The Sensitivity sheet shows which numbers are worth a change.

## Step 5: Diagnose

Read `references/indicators.md` for the meaning of each indicator and for the limits of the
simulation. Then explain each problem by its cause.

The Lulls sheet proposes a cause for each lull from the simulation data. Treat it as a lead.
Confirm it from the Timeline rows of the gap, then write your diagnosis.

| Cause | Sign | Direction |
| --- | --- | --- |
| Wall | The next spine step needs much more than the player has, and income is flat | Add a multiplier source, slow the wall, or place a side goal at the wall |
| Empty shop | Options within 5 minutes falls below 2 | A second clock, more purchase lines in that range, or cheaper steps |
| Flat menu | Many purchases in the gap, none is a beat | Milestones, tiers, a collection, or a feature at that point |
| End of content | Stall, or most capped lines at the cap and no new system later | A reset layer, cap raisers, cross-system chains |
| Reset that does not pay | Two or more resets in the gap and no run reached something new | A stronger or earlier prestige multiplier, new content deeper in the run, or automation as a prestige reward |

`references/system-archetypes.md` maps more symptoms to system types.

Then check the structure: orphan systems, currencies with no source or sink (Ledger),
purchases that the buyer never wants (Purchases), prestiges after a run with nothing new
(Prestige), and the unlock curve against the reference curve.

**Juice audit (if there is a build).** For each beat kind on the Timeline, find the row in
the beat hooks table of the scan, open the handler, and fill the audit table
(godot-extraction.md, part 5). A system unlock with the same feedback as a purchase is a
finding. A verdict on juice needs a file and a line. With none, write "not audited".

Use the reference patterns as questions. Each pattern in
`references/reference-game-patterns.md` ends with a question to ask about the game. Answer
the ones that apply. Three to five answers with evidence are better than fifteen without.
For formulas, multiplier stacking, premium currency, and odds, use
`references/reference-game-math.md`. For what other games do (cost ratios, prestige
formulas, softcaps, walls, reset timing), use `references/other-games-math.md`: a verdict on
a formula is stronger with more than one game behind it.

## Step 6: Ideate, then prove

Read `references/system-archetypes.md`. For each real problem, propose one to three systems.
Each idea names the problem with its time or level, what it pays to the core loop, what it
needs, where it opens, its scope, its one memorable moment, and its main risk.

Then test the best idea with three runs:

1. **Before:** the current model. You have its analysis JSON.
2. **Control:** `model-control.json`. Change numbers only (cheaper costs, a slower wall) to
   attack the same lull with no new system. This run answers "is a new system necessary, or
   is this a tuning problem?"
3. **With the idea:** `model-with-<idea>.json`. Add the system: its currency, its purchase
   lines, its unlock milestone, its effect on the core loop.

Run the control and the idea with `--compare <before>.analysis.json`. Report the indicators
of all three. If the control fixes the lull, say so: the cheapest fix wins, and the new
system must earn its place for a different reason (a later lull, a link for an orphan, a
clock). An idea that does not shorten the lull needs a different attachment point or a
stronger effect.

Check that the result is more than a count. A system that adds forty milestones adds forty
beats by definition. Run the idea one more time with its own events below the beat weight:
the spine and the old systems must still move faster. If they do not, the idea adds a clock,
not progress. That can be the correct fix, but say which of the two it is.

The model can express a new income source, purchase lines, multipliers, milestones, a
ladder, a cap raise, a cost in several currencies, and a run of fixed length. It cannot
express chance, a second reset layer, a rule change, or an offline cap. For an idea of that
kind, do the arithmetic by hand (what it adds to the gating stat at the lull, against what
the wall needs) and label the result "estimated, not simulated".

Write the findings, the lull notes, and the ideas into `review` in the model, and run the
script one more time. The workbook then carries the full review.

## Step 7: Report

Use this structure. Keep it short: the workbook holds the detail.

```markdown
# <Game>: progression review

## The loops
Core loop, spine, meta loop, ribs, clocks (from step 2).

## Verdict
| Area | Verdict | Evidence |
| Balance | ... | ... |
| Scope | ... | ... |
| Juice | ... | ... |
| Feel | ... | ... |

## Pacing
First purchase, first new system, first prestige. Beats for each hour. The lulls, each with
its start, its length, its cause, and "robust" or the profile that shows it.

## What to change in the numbers
Specific changes with the expected effect ("bellows growth 1.6 -> 1.45: the hour-3 lull
falls from 160 to 40 minutes"), each from a run of the changed model.

## Systems to add
The ranked ideas, each with the problem it fixes. For the tested idea: before, control, and
with the idea.

## Targets to build toward
Indicator values that define "good" for this game. The team reruns the workbook after each
balance change and reads the status cells.

## Assumptions and limits
Session length, target hours, active share. Each guessed number. What the model does not
cover. The buyer is a fast, greedy player: real players are slower.

## Files
The paths of the model, the workbook, and the analysis JSON.
```

State plainly what you did not verify. If no spreadsheet application was available, say that
the charts and the live formula cells were not opened.

## Files in this skill

| File | Read it when |
| --- | --- |
| `references/godot-extraction.md` | The source is a Godot project. Also holds the juice audit |
| `references/model-schema.md` | You write or change a model. Also holds all command-line options |
| `references/indicators.md` | You read a workbook or set targets |
| `references/reference-game-patterns.md` | You diagnose structure or look for what a strong incremental game does |
| `references/reference-game-math.md` | You judge formulas, multiplier stacking, premium-currency flow, or odds |
| `references/other-games-math.md` | You choose or judge a formula and want what other incremental games use: cost ratios, prestige shapes, softcaps, walls, stacking, reset timing |
| `references/system-archetypes.md` | You propose systems |
| `scripts/scan_godot.py` | Inventory of a Godot project |
| `scripts/progression_workbook.py` | Simulation and workbook (the `pw_*.py` files beside it are its modules) |
| `assets/example_model.json` | A complete small model to copy |
| `assets/reference_unlock_curve.json` | The unlock curve of the reference game, used by the Reference sheet |
