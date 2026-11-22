---
name: incremental-game-design
description: "Use when designing, proving, rebalancing or reviewing the math of an incremental, idle or upgrade-driven game (idea, design document or Godot project): systems, costs, unlocks, resets, pacing, content. Says not ready when the proof fails; installs a tuning bench in a Godot game."
---

# Incremental game design: prove the math, then build

This skill makes the math of an incremental game before the game exists, or reads it out of a built game. It proves the
math with formulas and a simulation of several player types. A design goes to the build only when the proof passes.

## The spirit of the game comes before the numbers

The numbers serve the game. A first version is simple, lovable and complete (`references/content-step.md` section 1):

- **Simple.** Few systems. Each selected system adds a decision of the player. Never add a system only to meet a number.
- **Lovable.** Before the solver runs, write the **promise of the pitch** in 2 to 4 lines in the words of the designer:
  the fantasy, what grows, and what the player watches get bigger. After the solver, the **promise check** compares the
  design with each line, with numbers. A design that breaks the promise is not ready, whatever its rows say.
- **Complete.** No stub, no empty content slot, a real end, and each selected system in the model.

This copy of the skill has no `tests/` folder and no `tools/` folder. A reference that names a file in `tests/` or
`tools/skill-v2/` names the test set of the skill, which is not in this copy. Do not look for those files.

`<skill_dir>` is the folder of this file. `<work>` is 1 output folder for the whole task. Put it outside the `res://`
tree of a Godot project, so that Godot does not import the files. The scripts need Python 3. Only the workbook needs
`openpyxl` (`python3 -c "import openpyxl"`). If it is missing, install it in a virtual environment, or say that the
workbook was not made. The workbook is a deliverable, so do not drop it in silence.

## Pick the mode

| The user has | Mode | Start at |
| --- | --- | --- |
| An idea, a pitch or a design note, and no code | Design forward | "Design forward" |
| Code or data of a game (a Godot project, data files, a save) | Existing game | "Existing game" |
| A design or a game, and the designer wants to tune the numbers by hand | Tuning bench | "Tuning bench" |
| A request to add 1 system to a design or a game | The mode of what exists, then "Add a system" | The mode, step 1 |
| A small question (1 cost curve, 1 formula) | Answer it with the law of `references/balance-math.md` and 1 run of the script that measures it | Do not run the full flow |

A design note with code beside it is an existing game: the code is the truth, and each difference is a finding.

## Two rules from version 1 that stay

- **Evidence before opinion.** Each claim points at a number: a proof row, a player row, a formula with a file and a
  line. "The mid game feels slow" is not a finding. "PC4 fails: no unlock from minute 10.4 to minute 25.6" is a finding.
- **Patterns, not copies.** The catalog cites real games for each formula. Use the formula and its range. Use the theme,
  the verbs and the names of the user's game. Never propose a system by its name in another game.

## Design forward

### Step 1. Ask the play pattern, then the targets

Read `references/interview.md` sections 1 and 2. Ask the play pattern first, because it changes the core set, the
targets and the proof rows:

- `continuous` (the request names Steam, desktop or PC): long sittings, no idle or offline benefit. The offline rule
  S06 and the presence gate are optional. S05 (an active action), S24 (automation) and S28 (a hard end) are core.
- `check_in` (the request names mobile or phone): an idle game with sessions of about 15 minutes and offline gains. S06
  and 1 or more presence gates (S07, S08, S09) are core. If the request names no platform, ask.

Follow the order of the interview (`references/interview.md`, "The order of the interview"): each question uses only
earlier answers. Ask the targets before any number, in groups of 4 questions or fewer, because the solver derives each
parameter from them. Ask each target as "about X" with its range: a target is a range, not an exact limit, and a target
with no game source is loose (interview section 2). Name each thing in the words of the game first and the catalog code
second. Compare each answer with the evidence that the interview gives. A target that no proven game supports gets 1
question ("keep your value, or use the proven value?") and then `"unsupported": true`. Do not take the numbers of a
design note as fixed: a note is a wish, not a proof.

### Step 2. Choose the systems

Offer a starter set of the play pattern first as a shortcut (section 3): K7, K9 (1 long run with no reset) or K8 for
`continuous`, K1 to K6 for `check_in`. The set with the shape of the pitch has the label: a pitch that names no reset, no
prestige, no rebirth, no runs and no shifts has the shape of K9. A game with no reset is a valid design, and it is not
rated below a game with a reset. Before part C, run the content need of step 3b and tell the designer the shortfall. After a set, show its answers in 1 confirm question, and ask only what it does not
answer. With no set, ask part B (a variant of each core system). Ask part C as multi-select groups of 4 systems or fewer,
and apply the answer rules of section 6 after each group, because an answer removes the questions that depend on it. Each optional question states the player decision that the system adds, its scope and the
systems that it needs. `references/systems-catalog.md` holds each card, and `assets/systems.json` holds the variant ids
and the proven ranges.

Each option states what it gives: a proved model, a simulated model, or no model (interview section 1, `proof_class`
in `assets/systems.json`). The interview holds each recommendation. If the designer picks a variant that no law covers,
keep it: the solver takes the simulation path (step 4b). Say before the solve when a choice gives no model.

**When no person can answer** (a 1-line request, a batch run), do not stop and do not invent answers in silence:

1. Take the play pattern from the platform words of the request. With no platform words, take `continuous`, the default
   pattern of this skill.
2. Take the selection of interview section 7: for `continuous` the set with the shape of the pitch (K9 or K7), for
   `check_in` the Recommended answers of part B.
3. Take the default band of each target. For `continuous`, copy `assets/default_targets.json`.
4. Set `"designer_answered": false` (for `continuous`: `"starter_set": "<set>", "selected": [], "refused": []`), and
   write the first line of the report of interview section 7, with the class of the selection. Do step 3b with no
   question: name the content that the pitch gives, and report the rest as missing.

### Step 3. Write the selection file and the targets file

The interview ends with 1 selection record (interview section 8). Write it as 2 files, so that the designer can change
the targets and keep the selection:

- `<work>/selection.json`: `designer_answered`, `starter_set`, `selected` (system, variant, parameters, and the optional
  `id` and `role`), `refused`, `notes`. Copy the shape of `assets/example_selection.json`. A system can have more than 1
  entry, each with its own `id`: for example 3 S02 groups, 1 with the role `care` for the upkeep card S30
  (`references/design-solver.md`, "Selection entries" and "Upkeep").
- `<work>/targets.json`: `play_pattern` and `targets`. Copy the shape of `assets/example_targets.json` (a `continuous`
  game) and change the values. Its `demo_end` names a system of the example game: remove it when the game has no demo.
  The exact keys of both patterns are in `references/design-solver.md`, "Inputs".

### Step 3b. Turn the pitch into content

Read `references/content-step.md`. Write the promise of the pitch, and count the content elements that the targets need:
```bash
python3 <skill_dir>/scripts/pw_content.py need <work>/targets.json --selection <work>/selection.json
```

A content element is a first purchase of a line, a first milestone or the first reset, not a stage of the spine alone.
List the nouns and the verbs of the pitch that can be content: things that the player buys, unlocks or lights up.
Propose named elements, 1 line each: the name in the words of the pitch, the kind (a purchase line, a single purchase, a
milestone, a new system), the system card, and the part of the game where it opens. Propose the need or more. Ask the
designer to confirm or change the list in groups, with the tool for questions. Write `<work>/content.json`
(`pw_content.py template` writes the frame), run the need again with `--content`, solve with `--content`, and check that the model holds each named element. An empty
or unnamed slot is never a result: say "content is missing: N elements", and name the minutes.

### Step 4. Run the solver

```bash
python3 <skill_dir>/scripts/pw_design.py <work>/selection.json <work>/targets.json --out <work>/model.json --report <work>/report.md --proof <work>/proof.json
```

The solver computes r for each line, R for each phase, lambda and s of the reset loop, and takes each parameter from a
proven range of the catalog (`references/balance-math.md`, section 3, the design procedure). It then runs the proof
sheet with strict ranges. Exit code 0 is ready, 1 is not ready, 2 is bad input, 3 is simulated (step 4b). A failed proof
row gives exit code 1 with the model and the proof of the round with the fewest failed rows. A variant that no law
covers (for example `S04.none`) takes the simulation path of step 4b. A solve takes about 2 seconds.

### Step 4b. The simulation path, when the laws do not cover the selection

On a variant conflict, `scripts/pw_scaffold.py` writes the model (a stub for each system that it cannot express), and a
timing fit sets the walls, unlock points and first costs by simulation. The key `simulation_path` marks each part `law`,
`simulated` or `not written`. Read `references/simulation-path.md`. Then write each stub and each mechanic with no catalog card as model lines ("Complete a stub"), and fit again (line 1). Run
the proof sheet (step 5) and the players (step 6): a row with no law gets "simulated, not proved", never pass. Compare 2
buyers, and change each hand-set number by 20% (line 2). Report 2 lists, "Proved" and "Simulated, not proved", and for
each simulated part what a play test must confirm.
```bash
python3 <skill_dir>/scripts/pw_scaffold.py --fit <work>/model.json <work>/targets.json --out <work>/model.json
python3 <skill_dir>/scripts/pw_scaffold.py --vary <work>/model.json <work>/targets.json
```

### Step 5. Read the proof sheet

Open `<work>/report.md`. The top says "ready", "simulated" (the verdict word is "ready (simulated)") or "not ready", and for "not ready" the target, the obligation, the range
and 2 or more ways out. Read `references/design-solver.md`, "How to read the report", and `references/proof-sheet.md`
for the rows and statuses.

To run the proof sheet again on a changed model, with the play-test speed:
```bash
python3 <skill_dir>/scripts/pw_proof.py <work>/model.json --strict-ranges --speed --json <work>/proof.json
```

Exit code 0 is ready, 3 is simulated and 1 is not ready. PO13 can fail at the default play-test speed x10 when the model has no auto-buy
(section "The play-test speed"). The fix names the speed: write it in `<work>/targets.json` as `play_test_speed.k`, and
solve again. A model with too little content fails PC4 (the longest wait for something new) or the content row CG, and
it can fail PV. Each row names how many more elements the game needs and in which minutes. Add them to the content file
of step 3b (`references/reverse-engineering.md` section 7, examples in `assets/examples/`), and solve again with
`--content`. When no person can answer, do 1 round of content, then report the result with the fix. A loop of tuning
with no designer makes the designer's choices.

Before you read a report, know 3 rules of the solver (`references/design-solver.md`): "Second income" (a person
confirms the spine stat in the code), "Automation" (PC6 counts the manual purchases only) and "Cap raisers".

When the solver reports a conflict, choose 1 way out with the designer (`references/design-solver.md`, "What to do with
a conflict"): relax the target, select another variant, or add a named system. Never leave a proven range to get a pass:
a fix text that names a value outside a proven range is not a way out.

### Step 6. Confirm with the players and the workbook

```bash
python3 <skill_dir>/scripts/progression_workbook.py <work>/model.json <work>/<game>-progression.xlsx --players all
```

The script plays the model with the players of `references/model-schema.md`, "Players". It writes the workbook and
`<game>-progression.analysis.json`, and it prints 1 line for each player and each band that is out (5 to 20 seconds).

Read each `PLAYER` line against the targets, and each `BAND OUT` line (`references/indicators.md`). PC1 tests the active
greedy player only, so check the end of each reset player against `length_h`. A design needs the proof and the players,
because a short game does not reach the asymptotes of the laws. For a `continuous` model, the script leaves out the
closed players and prints 1 line `LEFT OUT:`. Copy that line into the report.

Then do the beat and lull analysis on the workbook (section "Beats and lulls"), the section "Design
review", and then the section "Fun analysis". Then do the promise check (`references/content-step.md` section 1).

### Step 7. Report

Use the template of section "Report", with the promise check first. Apply the gate before you write the verdict. Give
each generic id of the model (`coin`, `gen`, `m1`) its name in the theme of the game in the report, and keep the ids in
the files. In a rebalance, `field_map` of the model gives the game id of each model id.
The build specification of a ready design includes the tuning bench (`references/tuning-bench.md` section 3): the 3 layers
of `references/game-core.md`, each balance number in a data file, and `pw_bench.py corecheck` with exit code 0. The targets
of the design go to the bench: `pw_bench.py init <project> --targets <work>/targets.json`.

## Existing game

Read `references/existing-game.md` for the detail of each step, and `references/reverse-engineering.md` for the
procedure and the content file. Steps 1 to 4 describe the game as built, and steps 5 to 10 change it. When the designer
wants to tune by hand, install the tuning bench before the rebalance (section "Tuning bench").

1. **Scan.** Ask the play pattern first (step 3). For a Godot project, read `references/godot-extraction.md` first.
   ```bash
   python3 <skill_dir>/scripts/scan_godot.py <project_dir> --pattern continuous --out <work>/scan.json --systems-out <work>/systems.json --content-out <work>/content.json
   ```
2. **Confirm the map and the content in the code.** Check each unsure system, `spine_stat` with `stats` (a second
   income, or `spine_unsure`), the `values` of S03 and S04, and the roles. Write each decision with its reason in
   `<work>/edits.json`, and scan again. Then add `base_income` to `content-confirmed.json` by hand, because the edits
   file has no key for it.
   ```bash
   python3 <skill_dir>/scripts/scan_godot.py <project_dir> --pattern continuous --confirm <work>/edits.json --systems-out <work>/systems-confirmed.json --content-out <work>/content-confirmed.json
   ```
3. **Ask the play pattern and the targets** (design forward, step 1). Turn each complaint into a target and its proof
   row.
4. **Prove the map and the game as built**, the "before" of each comparison. Exit code 1 of a proof is "not ready".
   ```bash
   python3 <skill_dir>/scripts/pw_proof.py --systems <work>/systems-confirmed.json --pattern continuous --json <work>/proof-map.json
   python3 <skill_dir>/scripts/pw_design.py <work>/targets.json --content <work>/content-confirmed.json --as-built --out <work>/model-built.json
   python3 <skill_dir>/scripts/pw_proof.py <work>/model-built.json --strict-ranges --speed --json <work>/proof-built.json
   ```
5. **Rebalance: propose the smallest change.** Write `<work>/selection.json` with the systems of the game plus each
   missing core system (copy the shape of `assets/example_selection.json`). The rebalance keeps the rules, the reset count (`max_resets` in the S04 values) and the cap states of the game. The report section "Structure of the game" lists each structure option; only the designer accepts one, with `--accept-change <name>`. Row WS checks the wall table steps. A selected system that nothing writes gives exit code 1. Read the report sections that
   `references/existing-game.md` section 5 names.
   ```bash
   python3 <skill_dir>/scripts/pw_design.py <work>/selection.json <work>/targets.json --content <work>/content-confirmed.json --out <work>/model.json --report <work>/report.md --proof <work>/proof.json
   ```
6. **Design review** of the rebalance against the game as built (section "Design review").
7. **Fun analysis** of the rebalance with `--build <work>/model-built.json` (section "Fun analysis").
8. **Confirm with the players** (design forward, step 6) on `<work>/model-built.json`, then on `<work>/model.json` with
   `--compare <work>/<game>-built-progression.analysis.json`.
9. **Compare the model with the real game** only when the user agrees (`references/existing-game.md` section 9).
10. **Juice audit**, if there is a build (`references/godot-extraction.md` section 5). With none, write "not audited".

## Design review

A design can pass each row and still be a bad game. This review is your own judgment, so label it as judgment in the
report. Do it after the proof sheet, on each design and each rebalance, with evidence from the report, the model or the
workbook:

1. **Does each element keep what makes it different?** Compare each line with the game or the note: the stat, the
   family, the cost shape, the cap and the level count. 10 lines with the same cost and step lost their identity.
2. **Is each element reached?** Read "Reachable" (PC5) and `never_bought` and `never_reached` in the analysis JSON. An
   element that opens after the hard end is cut content.
3. **Does each reward keep its sense?** A penalty stays a penalty. A side system whose only sink has 1 level has no use
   after 1 purchase. A cap raise of a line with no cap is a dead purchase.
4. **Are the first 10 minutes good?** The first purchase, 3 or more unlocks in the first 5 minutes (PO11), and a new
   action before minute 10 (the Timeline sheet).

Report each "no" with its fix. A lost identity or cut content breaks the promise of the pitch: it goes into the promise
check, and the verdict is then not ready.

## Fun analysis

A design can pass each proof row and still be dull. The fun sheet finds the known faults of a dull game. Run it after
the proof sheet and the design review. For a new design, leave out `--build`. For a rebalance, `--build` puts the game
as built beside each row and marks each row that the rebalance made worse:
```bash
python3 <skill_dir>/scripts/pw_fun.py <work>/model.json --build <work>/model-built.json --md <work>/fun.md --json <work>/fun.json
```

The rows FM-1 to FM-8 count: 2 or more out give "likely dull", 1 gives "watch". The rows FM-9 to FM-12 are advice. Copy
each counting row that is out, with its reason and its change, into the report section "Fun analysis". A change with no
control is content to add (case 3 of the gate). `references/fun-analysis.md` gives the rows, their controls, the play
test and tuning by hand. State its limits in the report. The rows are proxies. They count events, sizes and times. The
result is advice with evidence marks, not a proof. It never changes the gate. A sheet with no finding
does not prove that the game is fun. Only a play test tells why a player stops.

## Tuning bench

The solver gives the first numbers. The designer makes the last decisions on the bench: a panel in the Godot game with
controls and meters, which a simulation of the real game code fills in seconds (`references/tuning-bench.md`). A saved
bench setting is the decision of the designer: the skill does not overrule it with the solver.
1. Follow the order of `references/tuning-bench.md` section 3: `install`; name the core or write the adapter (with its side-effect
   list and its `bought` events); `init --phases-only`; a first `run`; `inventory`; class and `confirm` each number in the code.
2. Move each balance number in code to a data file, or record why it stays. Then `init` writes the controls.
3. `check` and `selfcheck` must exit with code 0. A dead number with no proved reason is a finding: report it, do not hide it.
4. Give the designer the meters of `run`, the line "not reviewed" of `check` (0 with `--strict`), and the hotkey of the panel.

The game has 3 layers (`references/game-core.md`). The core holds all the math as static functions over plain state. The session
is the only owner of the state. The view sends intents and animates on events. `pw_bench.py corecheck` proves that the core is
pure. For an existing game, refactor to it in the steps of that reference: the bench result is the before-and-after test.

## Add a system

Design only the systems that the designer selected. A system that the designer did not choose is scope nobody asked for.

1. A new idea comes from the catalog (`references/system-archetypes.md` maps a symptom to catalog ids). Check the
   compatibility table and the minimum set (`references/systems-catalog.md`, "Compatibility of the optional systems").
2. State the player decision that the system adds. A system that only multiplies adds none: label it "multiplier only".
3. Ask the designer with the part C question form of the interview. Add the system only after a yes.
4. Prove it with 3 runs: **before** (the current model), **control** (numbers only, no new system, against the same
   problem) and **with the idea**. Run the solver or the proof sheet on each, and the workbook with
   `--compare <before>.analysis.json`. If the control fixes the problem, say so: the cheapest fix wins.
5. A system that the laws do not cover (chance, a chain of 3 or more currencies, a second reset layer) is "simulated,
   not proved". A system that the model cannot express gets hand arithmetic, labelled "estimated, not simulated".

## The gate

The result has 3 words. **Ready**: each gate row passes. **Ready (simulated)** (exit code 3): each gate row passes or is
"simulated, not proved", and no part is a stub (`references/proof-sheet.md`, "The result"). A design that is ready
(simulated) can go to a build, and the report names what a play test must confirm for each simulated part. **Not
ready**: 1 of the items below is true. A game with no reset layer is a valid design: do not rate it below a reset game.

- **The promise check fails**: the design breaks 1 line of the promise of the pitch (`references/content-step.md`).
- `pw_proof.py --strict-ranges` exits 1 (or `pw_design.py` exits 1). These rows decide the verdict
  (`references/proof-sheet.md`):
  - PO1 to PO13: the 13 obligations of the laws, for each play pattern.
  - PC1 to PC6, for `continuous` only: the hard end inside the length target, the gaps between purchases, new content
    in each hour past the demo end, the gaps between unlocks, each element reached, and the purchases in 1 minute.
  - PV: each level of each line gives at least the purchase value floor (default 10%).
  - A core line that stops early (row CL): a generator line has its last purchase before half of the length, so the
    thing that the pitch grows stops to grow.
  - A content drought (row CG): a wait with no new content element outside its range. A stage alone is not content.
  - An empty content slot (row PC3): a milestone with no effect that only fills a place. It is missing content.
  - A core row: a missing core system of the play pattern (`--systems`).
- 1 player of the workbook misses the range of a target (a `PLAYER` line against the targets, or a `BAND OUT` line).

**Targets are ranges.** A law row is exact. A target row has 3 zones: pass, pass in range (past the target and inside
its tolerance) and fail. Only a value outside the range fails. `references/proof-sheet.md` holds the tolerance of a
sourced target and of a target with no game source. The report counts the rows that are in range but past the target,
and names each one with its value and its target.

A "not ready" result names each failed item and the change that fixes it: the fix text of the proof row, or the way out
of the solver. Do not present such a design as ready to build, as "a proof of direction", "mostly ready" or "good enough
to prototype". A team that builds a failed design builds the failure. "Not ready" with a named fix is the honest result.

Sort each failed row into 1 of 3 cases, because each case has its own fix:

1. The solver can meet it: change a target, a variant or a system, and solve again.
2. The limit is a default with no source (for example PC4): the designer keeps it or sets a value.
3. Content is missing: the report says "content is missing: N elements" with the minutes (CG, PC4, an empty slot, or
   PC2: 1 cheap purchase in a gap). No change of a parameter fixes it. "Not ready" with case 3 only is a valid result:
   give it to the designer with the list of content to add.

When a target has `"unsupported": true`, the design can be ready against that target. Say in the report that no proven
game supports the target.

## The play-test speed

A play-test build runs the game clock k times faster. A human needs about 2 real seconds for each decision, so above a
limit the test measures the human, not the game. Row PO13 (`--speed`) gives 2 largest speeds: the formula
`floor(T_X / (10 x D_h))` and the simulated speed with a slow-down under 1.1. The smaller value decides. State it in the
report. With no auto-buy, the simulated value is often much smaller (`references/proof-sheet.md`). For runs of about 15
minutes, x100 is not valid (law L9).

## Beats and lulls

A proof row says that the gaps are short. The beats say if the player notices the events in the gaps. A beat is an
event that a player remembers: decide the beat weight before the run (`references/model-schema.md`), or run both weights
(`--beat-weight 2`) and report both. The Lulls sheet proposes a cause for each lull (a wall, an empty shop, a flat menu,
the end of content, a reset that does not pay: `references/indicators.md`, `references/system-archetypes.md`). Confirm
it from the Timeline rows of the gap. Report if a lull is "robust" (each buyer profile shows it) or depends on 1 reset
policy. Check the structure on the workbook: orphan systems, currencies with no limited sink, lines that no player buys.

## Report

Write `<work>/<game>-report.md`. Keep `<work>/report.md` of the solver beside it as the full detail, and link to it.
Keep the summary short. The workbook and the proof JSON hold the detail.

```markdown
# <Game>: <design | rebalance> <ready | ready (simulated) | not ready>

<If the designer did not answer: the first line of interview section 7.>
## Promise check
| Line of the promise | Number of the design | Kept |
This table comes first. A broken line makes the verdict not ready.
## Verdict
Ready, ready (simulated) or not ready. For not ready: each failed item (promise line, proof row or player band) with
its value, its limit and the change that fixes it. For ready (simulated): what a play test must confirm. The count of
rows in range but past the target. "Content is missing: N elements" with the minutes, when it is.
## Play pattern and targets
| Target | Value | Band of the evidence | Source | Unsupported |
## Selected systems
| System | Variant | Player decision | Refused systems | Systems that the solver did not write (simulation only) |
## Parameters
| Parameter | Value | Proven range | Source | Set by (target or obligation) |
(For a rebalance: old value, new value, reason.)
## Proof sheet
| Id | Value | Limit | Status | Fix |
PO1 to PO13, PC1 to PC6 (`continuous`), PV, CG and each core row. For PO13, give both largest speeds.
For a rebalance, give the status of the game as built beside each row. For PV, the table "Purchase value" of the
solver report: | Line | Gain of 1 level | Floor | Status |
## Players
| Player | Hours to the end | Active minutes | Resets | Target band | In band |
Idle curve, policy gap, play-test speed (the value of "The play-test speed"). For `continuous`, the `LEFT OUT:` line.
## Design review
The 4 questions of section "Design review", each with its answer, evidence and fix.
## Fun analysis
The result line of the fun sheet. Advice with evidence marks, not a proof.
| Id | Value | Band (mark) | Status | Reason | Change to try |
The counting rows FM-1 to FM-8 (for a rebalance, with the value as built), the advice rows that are out, the limits.
## Beats, lulls and juice
The lulls with start, length, cause, robust or not. The juice audit or "not audited".
## Open risks
Each risk with the row, the system or the assumption it comes from.
## Proved, and simulated, not proved
The 2 lists of `references/simulation-path.md` ("The report rule"), what is hand arithmetic, and what was not compared.
## Files
selection.json, targets.json, model.json, proof.json, fun.md, fun.json, the workbook, the analysis JSON. For an
existing game also: edits.json, the confirmed map and content, model-built.json, proof-built.json.
```

State what you did not verify. If no spreadsheet application opened the workbook, say that the charts and the live
formula cells were not checked.

## References

| File | Read this when |
| --- | --- |
| `references/interview.md` | You ask the play pattern, the targets and the systems, or no person can answer |
| `references/content-step.md`, `scripts/pw_content.py` | The promise of the pitch and its check, the content need, the named content elements, the content file |
| `references/systems-catalog.md` | You need a card: the player decision, formula families, ranges, links, risks, core set, compatibility, starter sets |
| `assets/systems.json` | You write a selection: the variant ids, the proven ranges, the answer rules |
| `references/short-games.md` | The play pattern is `continuous`: the target bands of 11 short Steam games |
| `references/balance-math.md` | You need a law (L1 to L10, L10 for a second income), the design procedure, or a proof obligation |
| `references/design-solver.md`, `scripts/pw_design.py` | You run the solver (a design, a rebalance with `--content`, the game as built with `--as-built`), read its report, or resolve a conflict |
| `references/simulation-path.md`, `scripts/pw_scaffold.py` | The laws do not cover the selection: the scaffold, the fit, the marks |
| `references/proof-sheet.md`, `scripts/pw_proof.py` | You read or run the proof sheet of a model or of a scanned game: the rows, the statuses, the tolerances, the `--systems` mode |
| `references/existing-game.md` | The game exists: the detail of each of the 10 steps, what to check in the code, and how to compare with the real game |
| `references/reverse-engineering.md` | The game exists: the procedure, a formula that fits no family, the model against the game, the content file |
| `references/godot-extraction.md`, `scripts/scan_godot.py` | The game is a Godot project: the scan (`--pattern`, `--systems-out`, `--content-out`, `--confirm`). Also holds the juice audit |
| `references/tuning-bench.md`, `scripts/pw_bench.py`, `assets/godot/` | The designer tunes by hand: the bench procedure, the number ledger, the roles, the controls, the meters, the self-check, the addon and a sample game |
| `references/game-core.md` | You write a build specification, or refactor a game: the 3 layers (core, session, view), the core contract, `corecheck` |
| `references/model-schema.md` | You write or change a model by hand. Also holds the players and the command-line options |
| `references/indicators.md`, `scripts/progression_workbook.py` | You read the workbook, the player lines, or the bands. The script makes the players and the workbook (`--players all`); the `pw_*.py` files beside it are its modules |
| `references/fun-analysis.md`, `scripts/pw_fun.py` | The fun sheet: the 24 fun metrics, the result rule, the controls, the play test, tuning by hand, the limits |
| `references/system-archetypes.md` | You look for a system that fixes a symptom |
| `references/reference-game-patterns.md`, `references/reference-game-math.md`, `references/other-games-math.md` | You diagnose structure against a strong incremental game, or judge formulas, multiplier stacking, premium currency or odds. The last file: what other incremental games use (cost ratios, prestige shapes, softcaps, walls) |
| `assets/example_selection.json`, `assets/example_targets.json`, `assets/example_model.json` | The shape of the 2 input files of the solver (Moss Works, `continuous`), and a small complete model to copy for a hand-written model. `assets/default_targets.json`: the targets of a `continuous` game when no person answers: the default bands, no demo end |
| `references/examples.md`, `assets/examples/` | 2 worked designs (ready, simulated): selection, targets, content file, model, report |
