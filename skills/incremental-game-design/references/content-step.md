# The content step: from the pitch to named content

A design with the right numbers and no content is not the game of the pitch. This step turns the pitch into named
content elements before the solver runs. `scripts/pw_content.py` counts how many elements the targets need. The
agent proposes the elements in the words of the pitch, the designer confirms them, and the agent writes the content
file. `SKILL.md` runs this step between the system selection and the solver (step 3b). The interview runs the count
before part C (`references/interview.md` section 5).

## Contents

1. The spirit of the game: simple, lovable, complete
2. What a content element is
3. The need
4. What the selection gives
5. The step
6. The content file
7. Missing content
8. Limits

## 1. The spirit of the game: simple, lovable, complete

The numbers serve the game. A first version is small, it is something that a player loves, and it is complete at
its size. It is not a broken part of a larger game. This is a design principle. It is not a source of numbers.

**Simple.** Few systems. Each selected system adds a decision of the player (the interview states the decision of
each system). Do not add a system only to meet a number. When a target needs more content, first add content to a
system that the game already has: a new line, a single purchase or a milestone. A new system is the last choice.

**Lovable.** Write the promise of the pitch before the solver runs: 2 to 4 lines in the words of the designer.

1. The fantasy: what the player is or does.
2. What grows: the thing that the player buys more of.
3. What the player watches get bigger: the thing on the screen that shows the progress.
4. Optional: the end that the pitch promises.

Show the lines to the designer in 1 question ("a. This is the promise", "b. Change a line"). Write them as the
first lines of `notes` in `selection.json`, each with the prefix "promise:".

**The promise check.** After the solver and the players, compare the design with each line of the promise. Give 1
number of the model or of the analysis JSON for each line. A line with no number is not checked: say so.

| Line of the promise | Number to read | The promise is broken when |
| --- | --- | --- |
| What grows | The count of the thing at the hard end, and the minute of its last purchase (`purchases` of the analysis JSON) | The line stops in the first part of the game, or its count at the end is not what the pitch describes ("a huge network" with a count of 9) |
| What the player watches | The count of content elements that show it (milestones, stages with a name), and the largest gap between them | The thing changes fewer times than the pitch names steps |
| The fantasy | The share of the game time in which the player does the action of the fantasy | The action ends early and nothing replaces it |
| The end | The hard end of the model and its name in the theme | The model ends at a stage with no name of the game |

Put the table of the promise check first in the report, before the proof rows. A design that breaks 1 line of the
promise is **not ready**, whatever its rows say. The fix is content or a change of a line, not a looser promise:
only the designer changes the promise.

**Complete.** The design has no stub, no empty content slot and a real end, and the model holds each selected
system. A part that is absent is named as missing. It is never left as a placeholder that passes a row.

## 2. What a content element is

The meaning is that of the proof row CG (`references/proof-sheet.md`, "Row CG: content gaps"), which is the
definition of FM-1 of the fun sheet (`references/fun-analysis.md`).

| Counts | Does not count |
| --- | --- |
| The first purchase of a purchase line | A stage of the spine alone (an axis step) |
| The first time that the player reaches a milestone | A later level of a line that the player already bought |
| The first reset | A longer wall of the same content |
| | A slot with no name, and an element that the model does not hold |

The step uses 4 kinds of content element. Each maps to 1 `kind` of the content file:

| Kind of the step | Content file | Example of the form |
| --- | --- | --- |
| A purchase line | `kind` `line`, `levels` null or a count | A thing that the player buys many times |
| A single purchase | `kind` `line`, `levels` 1 | An upgrade that the player buys 1 time |
| A milestone | `kind` `reward` | A thing that the player reaches or lights up |
| A new system | `kind` `system` | A new screen or a new loop that opens |

A stage of the spine with a name of the game (a place that lights up) becomes a content element only when the
content file holds it as a milestone at that stage.

## 3. The need

```bash
python3 <skill_dir>/scripts/pw_content.py need <work>/targets.json --selection <work>/selection.json --json <work>/content-need.json
python3 <skill_dir>/scripts/pw_content.py need <work>/targets.json --content <work>/content.json
python3 <skill_dir>/scripts/pw_content.py need <work>/targets.json --content <work>/content-confirmed.json --model <work>/model-built.json
```

The third form is for a built game (mode "Existing game"). The content file of the scan has no `opens_min`, so the
times come from the active player of the model as built (`pw_design.py --as-built`): the first purchase of each
line, the first time of each milestone, and the first reset (element `reset`). The game names each element, so the
name check and the card check do not apply. A grant and the stages are not content elements. An element that the
player does not reach before the end is not given, and the script names it. Without `--model`, the script stops with
exit code 2 and asks for it.

The script prints the need, what the selection or the content file gives, and the shortfall, for hour 1 and for the
whole length. Exit code 0 means that no element is missing, 1 that content is missing, and 2 bad input. The script
holds no limit of its own: it reads each limit from the code of the proof rows.

| Value | Formula | Limit and source mark |
| --- | --- | --- |
| Content need of hour 1 | `n1 = ceil(min(60, L) / b1) - 1` | `b1`: the FM-1 band of hour 1, `fun.FM-1.max_h1` of the targets file, else the default of the fun sheet (`pw_design.target_values`, `fun_gap_h1`). No source: a default of the fun sheet. Row CG |
| Content need after hour 1 | `n2 = ceil((L - n1 x b1) / b) - 1` | `b`: the FM-1 band of the whole length, `fun.FM-1.max` (`fun_gap`). No source. Row CG |
| Content need | `n1 + n2` | `L`: `length_h.value` in minutes |
| Shortfall | The sum of `ceil(gap / band) - 1` over each gap above its band, between the times of the given elements | `pw_proof.content_gaps`, the count of row CG |
| Unlock need | The same 2 formulas with the unlock gap targets | `purchase_gap.unlock_first_hour_max_min` and `unlock_max_min` (designer targets, no source: band CT7, `SG:198`). Row PC4 |
| Parts past the demo | Each part of 1 hour from the demo end to the end needs 1 element or more | `pw_proof.PART_HOURS` (no source: a default of the sheet), `pw_proof.extension_parts`. Row PC3 |

1. The content need counts content elements only. The unlock need of row PC4 counts stages of the spine too, and
   the solver meets it with stages. So the script prints the unlock need as information, and the shortfall is the
   shortfall of content elements.
2. The need is the smallest count: it holds only when the elements open at the widest spacing that the band
   permits. Elements that open together fill 1 place.
3. Both bands of row CG have no game source. They are loose limits: the proof sheet gives the tolerance of a limit
   with no source (`references/proof-sheet.md`). The script counts against the band itself, so 1 element more than
   the need is a safe plan.

Example (`tests/designs/content-step/input/targets.json`: 2.5 hours, the default bands 10 and 20 minutes):
`n1 = ceil(60 / 10) - 1 = 5`, `n2 = ceil((150 - 50) / 20) - 1 = 4`, so the need is 9.

## 4. What the selection gives

With `--selection`, the script counts the elements that the solver writes for the selection with no content file
(`references/design-solver.md`, "Selection entries", "Upkeep" and "1 long run"):

| Selected | Elements |
| --- | --- |
| S01 | 1: the generator line |
| S02 | 1 line for each entry that is not `care`; with 1 entry or none, 2 groups |
| `S30.need_capacity` | 1: the care line |
| S08 | 1: the bank line |
| S04 (not `S04.none`) | 1: the first reset |
| Each other system | 0, until the content file names its elements |

The solver opens these lines in the first minutes. So together they fill 1 place of hour 1, whatever their count.
This is the best case, and the printed shortfall is a lower limit. A selected optional system gives no element
here: its elements count when the content file names them with a time.

## 5. The step

Do the step after the selection is confirmed and before the solver runs.

1. **Read the pitch again.** List each noun and each verb that can be content: a thing that the player buys,
   unlocks, upgrades, reaches or lights up. Keep the words of the pitch. A list of the pitch ("a, then b, then c")
   is an order: keep it.
2. **Run the need** (section 3) with the selection.
3. **Propose named elements, 1 line each:** the name in the words of the pitch, the kind (section 2), the system
   card that it belongs to, and the part of the game where it opens (a minute, or "hour 1", "after the demo").
   Propose the need or more. Give each selected system 1 element or more. A noun of the pitch that fits no selected
   system goes to group C8 of the interview (a mechanic with no card), not into a new system.
4. **Do not invent content to meet the count.** When the pitch has fewer nouns than the need, propose what the
   pitch gives, and say "content is missing: N elements" with the minutes (section 7). The designer then names more
   content, or changes the band or the length.
5. **Ask the designer to confirm or change the list**, in groups of 4 elements or fewer by part of the game, with
   the tool for questions. Each question shows the lines of its group. The options are "a. Keep these", "b. Change
   or remove 1" and "c. Add 1 here". Use the words of the game first and the catalog code second.
6. **Write the content file** (section 6), and run the need again with `--content`. The shortfall must be 0, or
   the report says what is missing.
7. **After the solve, check the model.** Each named element must be in `purchases` or `milestones` of the model,
   with its name. An element that the model does not hold is missing content: name it in the report.

## 6. The content file

The format is the content file of the solver (`references/design-solver.md`, "The content file";
`references/reverse-engineering.md` section 7). `pw_design.py --content` reads it. The frame:

```bash
python3 <skill_dir>/scripts/pw_content.py template <work>/targets.json --count 12 --selection <work>/selection.json --out <work>/content.json
```

The frame has 1 element for each line of the selection (it opens at the start) and `--count` new elements at the
times of the need. With no `--count`, the count is the need. A count below the need is refused. Fill each element:

| Key | What to write |
| --- | --- |
| `name` | The name in the words of the pitch. An empty name, the id, or a slot name is refused |
| `pitch_words` | The words of the pitch that the element comes from |
| `kind`, `levels` | The table of section 2 |
| `system` | The catalog card that the element belongs to: a selected system. The script checks only that the card exists |
| `opens_min` | The target time of the first purchase or of the milestone, in minutes. The step reads it; the solver sets the unlock point itself |
| `cost` | `base` and `ratio` of a line. Take the default of the variant from `assets/systems.json`; the solver moves a value into its proven range |
| `effects` | `stat`, `kind` (`add`, `pct`, `mult`, `cap`) and `value` of each effect. Give each milestone an effect or a grant. A milestone with no effect and no grant that no other element needs is an empty slot for the proof sheet (row PC3, `references/proof-sheet.md`) |
| `unlock` | `{}` for an element that is open at the start, else leave `{}`: the solver places the element |
| `role` | `generator` for the generator line; null for the others |

The files `assets/examples/*/content.json` show complete elements.

## 7. Missing content

An empty or unnamed element is never a result. The solver, the proof sheet and this step each say the same words:

> content is missing: N elements: <count> between minute <a> and minute <b>; ...

1. `pw_content.py need --content` refuses a file with an element that has no name, no time, no kind or no system
   card (exit code 2). It names each such element and its minute. With `--model`, it reads the content file of a
   built game (section 3).
2. A slot that a tool wrote (an id such as `content_1`, a name that asks the designer to name it) is missing
   content. Do not count it. The proof row PC3 fails for a part that holds only an empty slot.
3. A report with missing content is "not ready" with case 3 of the gate (`SKILL.md`, "The gate"): the list of
   content to add, with the minutes. The gate rows are the content drought (row CG), the empty content slot (row PC3)
   and the core line that stops early (row CL).

## 8. Limits

1. The step does not judge if the content is good. The designer and a play test do.
2. `opens_min` is a plan. The solver and the timing fit place each element, so the proof row CG of the solved model
   decides, not this count.
3. The script does not read a model. After the solve, read the row CG of the proof sheet for the elements that the
   model holds.
4. A selection with no reset layer and a content file stays on the law path of 1 long run
   (`references/design-solver.md`, "1 long run", step 7). The solver places each element with `opens_min` in the
   model with its name. On the test input (`tests/designs/generator-growth/named/`) the model holds the 16 named
   elements, and each one opens within 3 minutes of its `opens_min`. The solver writes a milestone with its
   effects, a line with a `mult` effect (a side line with a cap) and a line with an `add` effect (a second
   generator line). The section "Named content" of the report lists each field that the solver does not write.
   Step 7 of section 5 still applies: read the row CG of the proof sheet, and do not report the count of the
   content file as the content of the model.
5. The solver places the content. It does not add content. On the test input the content file ends at minute 130
   of 155, so the row CG fails on the last gap. Name 1 more element there, or set the gap target.
