# The interview: targets and systems

The skill follows this script with the designer before it computes any number. The script sets
the targets (part A), the variant of each core system (part B), and the optional systems
(part C). The result is a selection record. The balance math (`references/balance-math.md`)
then balances only the selected systems.

## The order of the interview

Ask the questions in this order. Each question uses only the answers of the questions before it.
`assets/systems.json` holds the order as `interview_order`, and `tests/test_interview_order.py`
checks it.

1. **The play pattern**: P1 (section 1).
2. **The targets that depend on nothing** (section 2). `continuous`: A3.1, A3.2, A3.2b and A2.4 in
   the first call (they depend on nothing), then A1.3 and A1.4 in a later call (they depend on
   A3.2b). `check_in`: A1.1, A1.2, A2.1, A2.2 and A2.4, then A1.3 and A1.4.
3. **The starter set or the core systems.** Offer the starter sets (section 3). After a set, ask the
   1 confirm question of section 3. With no set, ask part B (section 4). Then ask the line groups of
   S02 (B2.2e).
4. **The targets that depend on a core system** (section 4, "Targets after the core systems"): the
   reset gap A2.3 and the run length (only with a reset layer), and the active factor A3.4 (only
   with `S05.active_lane`).
5. **The optional systems in groups** (section 5): first run the content need and tell the designer
   the shortfall ("The content need", section 5). Then groups C1 to C7 as multi-select questions,
   then C8 for each mechanic of the pitch that no answer covers. Apply the answer rules (section 6)
   after each group. The variant and the parameters of each system come right after its yes.
6. **The targets that depend on an optional system**: the demo end A3.3 (section 5, "After the
   optional systems"), because the demo ends before a system, and that system can be optional.
7. **The confirmation** (section 8): show the whole record and its class in 1 question, then write
   `selection.json` and `targets.json`.

After the confirmation, the content step turns the pitch into named content elements
(`references/content-step.md`). It is a step of `SKILL.md`, not a question of this file.

What depends on what:

| Question | Depends on | Reason |
| --- | --- | --- |
| A1.3, A1.4 | A1.2 or A3.2b | The gap target sets the smallest unlock count. |
| B2.2e | B2.2 | Each line group is an entry of the S02 variant. |
| A2.3, run length | B2.3 | They exist only with a reset layer (rules R1 and R11). |
| A3.4 | B1.3 | The factor is the parameter `a` of `S05.active_lane`. |
| C1 to C7 | part B and the groups before them | The answer rules and the needs of section 6. |
| A3.3 | part C | `before_system` names a selected system. |

## Contents

1. How to ask, and the play pattern
2. Part A: targets
3. Shortcut: a starter set
4. Part B: the core systems
5. Part C: the optional systems
6. Answer rules
7. When the designer is not available
8. The selection record

## 1. How to ask

- Ask in groups of 4 questions or fewer. Each question has 2 to 4 options. The designer can
  always give another answer as text.
- Put the Recommended option first, and add "(Recommended)" to its label. Only the rule of
  "The Recommended label" (below) gives the label. A label of this file with a condition
  ("Recommended when ...") is a label only when the condition is true: show it as "(Recommended)"
  then, and leave it out else.
- **The words of the game first, the catalog code second.** A question that the designer sees names
  a system in the words of the pitch, then the catalog name and the id in brackets: "Food, water and
  rest (upkeep, S30)", "A new start with a bonus (reset layer, S04)". Never show an id alone.
- **No row code without its meaning.** A question never shows a proof row code alone. Add 1 plain
  clause from "The proof rows in plain words" (below): "the longest wait for something new (row
  PC4)".
- **A target is "about X", with its range in the same sentence.** Ask "about 10 minutes (a little
  more is still inside the range, up to <the end of the range>)", never "at most 10 minutes". Section 2, "Targets are ranges", gives the rule.
- State in each option what it gives (below). Offer an option with no model last.
- In a tool that shows questions with options, use 1 question for each numbered item. In plain
  text, write the question, then the options as a list with the letters a to d. Part C is the
  exception: 1 multi-select question for each group (section 5).
- A tool for questions has 4 options at most, and it adds the free-text answer itself. So a group of
  4 systems has no room for an option "None". Section 5, rule 1, says how to ask such a group.
- Follow "The order of the interview" (above). Put each question in a later call than each
  question that it depends on (the table above). For example, A1.3 and A1.4 come in a later call
  than A3.2b, and group C6 comes in a later call than group C1.
- Apply the answer rules of section 6 after each group. Do not ask a question that a rule
  removes.
- Citations use the prefixes of `references/systems-catalog.md`. `BM` is `references/balance-math.md`.
  The system ids and variant ids are those of the catalog and `assets/systems.json`.

### What each choice gives

Each starter set and each variant has 1 proof class. `assets/systems.json` holds it as
`proof_class`, and a run of the solver and of the scaffold finds it
(`tests/test_interview_honesty.py`). Each option text states the class in these words:

| Class | Words in the option text |
| --- | --- |
| `proved` | "a proved model": a law covers the part, and the solver writes it. |
| `simulated` | "a simulated model: timing from the simulator, marked not proved". |
| `none` | "no model: the tools cannot do this yet". The text also says "offered last". |

1. The class of a selection is the lowest class of its parts.
2. A choice that no law covers takes the simulation path (`references/simulation-path.md`). The
   option text says so. On that path, each other part has its class `proof_class_sim_path`. For
   example, `S04.power` is proved, but the scaffold cannot write a reset layer: `S04.power` with a
   choice that takes the simulation path gives no model for the reset layer.
3. Before part B, and after each group, state the class of the selection so far.

### The Recommended label

1 rule gives each "(Recommended)" label of this file. An option is Recommended only if its value
is inside its sourced range, and 1 of these is true:

1. Its class is `proved`.
2. No option of the question is proved, and the option has the best evidence: the highest class,
   then the most sources.

A target has no class. Its Recommended value is inside the sourced band of the target. A target
with no source has no Recommended option, only a default. When an earlier answer forces a value,
that value is Recommended. The other files of the skill name no recommendation: they point here.

**The tie-break: the shape of the pitch.** When 2 or more options have the same class, the label
goes to the option that has the shape of the pitch. Only when the pitch does not decide, the option
with the most sources gets it. The label never goes to an option of a lower class. Section 3 applies
the tie-break to the starter sets.

**Part C.** In a group of optional systems, the answer "none" is Recommended only when the content
shortfall is 0 (section 5, "The content need"). With a shortfall above 0, no option of the group is
"none (Recommended)".

### The proof rows in plain words

Use these clauses when a question or an option names a row. `references/proof-sheet.md` holds each
row in full.

| Row | What it measures |
| --- | --- |
| PO3 | A reset pays: the player is stronger after a reset than before it |
| PO4 | A patient player and an early player finish in about the same time |
| PO6 | What the open game pays with no input, against active play |
| PO10 | How often something new opens, against the session length |
| PO11 | The first 5 minutes open 3 new things or more |
| PO13 | The largest game speed at which a human play test is still valid |
| PC3 | Each hour past the demo end adds new content |
| PC4 | The longest wait for something new: a stage, a first purchase or a milestone |
| CG | The longest wait for a new content element; a stage alone does not count |
| CL | The line that the pitch grows keeps growing: its last purchase comes in the second half of the game |
| PV | Each purchase gives a gain that the player feels |
| PU1 to PU4 | The care keeps up with the need of the producers |

### The play pattern (asked first)

**P1 Play pattern.** How does the player play the game? Decision D9 of the catalog.

| Option | Effect |
| --- | --- |
| a. `continuous`: a desktop or Steam game, played in long sittings | No idle or offline benefit. S06 is optional with the default "none". The presence gate slot is optional. S04 is optional. S05, S24 and S28 are core (decision D10). Part B gives the variants. Part A uses group A3. A session is the whole sitting. |
| b. `check_in`: a mobile idle game, short sessions of about 15 minutes | Offline gains. S06 is core. 1 or more presence gates. Part A asks all idle targets. |

There is no recommended option: the answer comes from the platform of the game. The generator
lines still pay while the game is open in both patterns. Rule R10 applies the pattern.
Its evidence is in `references/short-games.md` (prefix `SG`).

## 2. Part A: targets

The skill compares each answer with the evidence below. It does not take the numbers of a design
note as fixed. Rule for a target that no proven game supports:

1. The skill says that no proven game supports the value.
2. The skill gives the nearest proven value and its source.
3. The skill asks 1 time: "Keep your value, or use the proven value?"
4. If the designer keeps the value, the record marks the target `"unsupported": true`. The proof
   sheet then tests that value. The skill does not ask again.

A target that no source supports at all (section 5 of `references/short-games.md`) is a
"designer target, not supported by a source". The skill warns 1 time and keeps the value. It does
not refuse it, and it does not invent a band for it. The record marks it `"unsupported": true`.

**A new value as the answer to "keep or proven".** When the designer answers that question with a
third value, compare it 1 time with the evidence, like a first answer. A value inside the sourced
band of an option of the question is supported: write it with `"unsupported": false` and name the
option and its source in `notes`. A value outside each band is kept with `"unsupported": true`. Do
not ask again.

**"No source" and `"unsupported": true`: 1 rule.** "No source" is what the skill says to the
designer. `"unsupported": true` is how the record holds it. A group of the targets file gets
`"unsupported": true` in 2 cases: the designer keeps a value outside the sourced band of the group,
or no value of the group has a source (`unlocks`, `play_test_speed`). The group `purchase_gap` holds
2 sourced values (`largest_max_min`, `first_max_min`) and the 2 unlock gaps of A3.2b, which never
have a source. Its flag describes the 2 sourced values only. The solver report marks the unlock gaps
as designer targets in each case.

**Targets are ranges.** A target is not an exact limit. The proof sheet gives each target row 3
zones: pass, pass in range (past the target, inside its tolerance) and fail (outside the range).
`references/proof-sheet.md` holds the tolerance of a sourced target and of a target with no game
source, and the key `tolerance` of a target holds the tolerance of the designer.

1. Ask each target as "about X", and say its range in the same sentence: "about X minutes: a value
   up to <X x (1 + tolerance)> is inside the range". Compute the end of the range from the tolerance
   of the proof sheet. Do not copy the tolerance into this file.
2. When a target has no game source, say so in the question, and say that it is loose: "No game
   measures this. It is a rule of thumb, so the range is wide."
3. An option of a target question is a range or an "about" value. It is never a bare limit ("at
   most 10 minutes").
4. A law is exact. The interview asks no law: the laws belong to the solver.

For the play pattern `continuous`, ask group A3 in place of A1.1, A1.2, A2.1 and A2.2. Ask A2.4 in
the first call with A3.1, A3.2 and A3.2b, and mark it "no source" (`SG:204`). Ask A1.3 and A1.4 in
the next call, after A3.2b, and mark each answer "no source" (`SG:198`). For `check_in`, ask A1.3 and A1.4 after A1.2. The gap targets set the
smallest unlock counts, so no option of A1.3 or A1.4 can contradict them. A2.3 and A3.4 come after
the core systems, and A3.3 comes after part C ("The order of the interview").

**The idle values for each pattern.** `check_in` asks A2.1 and A2.2. `continuous` asks no idle
value: the targets file keeps the law band 0 to 1 for each open value and has no closed value
(decision D10).

**A band to 1 value and 1 tolerance.** The targets file holds 1 `value` and 1 `tolerance` (a
share of the value). For an answer that is a band from `lo` to `hi`:

1. `value` = the middle, `(lo + hi) / 2`.
2. `tolerance` = the half width as a share of the middle, `(hi - lo) / (hi + lo)`. Round it to 2
   decimals.
3. Example: "2 to 3 hours" gives `value` 2.5 and `tolerance` 0.5 / 2.5 = 0.2. "3 to 6 hours"
   gives `value` 4.5 and `tolerance` 0.33.
4. A band with no upper end ("more than 10 hours") has no middle. Ask 1 value, and use the
   tolerance 0.15 of the default band of A1.1.
5. An answer that is 1 value with "within x%" is already a value and a tolerance.
6. The rule applies to an answer of the designer. With no answer (section 7), copy the values of
   `assets/default_targets.json`: its length 6 hours within 0.15 is 1 value inside the default
   band, not the middle of the band.

The local material has no measured play times of the reference game (`RP:8`, `RP:72-76`). So
some targets have a default band from worked example 2 of the balance math and no proven game.
The skill states this when it gives the default.

### Group A1

**A1.1 Length.** How many active hours does the full game take?

| Option | Evidence |
| --- | --- |
| a. About 20 hours, within 15% (Recommended) | Worked example 2 meets it: 20.33 hours (`BM:388`, `BM:449`). No proven game measures it. |
| b. About 6 hours | Moss Works active greedy play reaches Stage 40 in 6.25 hours (`IA:7`). That build fails other targets. |
| c. About 25 hours or more | IOM needs at least 17 to 25 hours of attempts for its 75 spine levels (`RP:97`). This is a floor, not a play time. |
| d. Weeks or months | IOM spans months (inference, `RP:8`). The laws of `BM` need a phase for each new system. |

Default band: 20 hours, within 15%. Proven game: none measured.

**A1.2 Session.** How long is a normal session?

| Option | Evidence |
| --- | --- |
| a. About 15 minutes: a median gap of about 15 minutes and a largest gap of about 20 (Recommended) | Worked example 2 meets it: median 13.2, largest 17.1 (`BM:389`, `BM:451`). |
| b. About 20 minutes | The IOM check-in interval at the start is 20 minutes (`RP:338-341`). The IOM spine attempt has a 20-minute cooldown (`RP:37`). |
| c. About 5 minutes | A design blog: "If players wait more than 5 minutes for something meaningful, pacing has failed" (`D:362`). |
| d. Hours | The IOM check-in interval grows to 18.6 hours late in the game (`RP:132-136`). |

Default band: median gap at most 15 minutes, largest gap at most 20 minutes. Proven game: IOM at
the start (20 minutes).

**A1.3 First hour.** How many unlocks does the first hour give? Ask it after the gap target. Let
`g1` be the largest gap in the first hour: `largest_gap_max` of A1.2, or
`unlock_first_hour_max_min` of A3.2b for `continuous`. The first hour then needs
`n1 = ceil(60 / g1) - 1` unlocks or more: 2 for g1 = 20, 11 for g1 = 5. Compute n1, and show it
in the question. Do not offer a count below n1. The 5-minute count comes from `PO11` (`BM:302`).

For `check_in`:

| Option | Evidence |
| --- | --- |
| a. About 3 or more in the first 5 minutes, and `max(6, n1 + 1)` within 1 in the first hour (Recommended) | `PO11` of `BM` (`BM:302`). Worked example 2 gives 7 (`BM:450`). |
| b. Many at the start, few later: about 3 or more in the first 5 minutes, and `max(12, n1 + 1)` within 1 in the first hour | IOM opens 12 systems in the first 10 minutes and at most 3 more in the first hour (`RP:167-168`). |
| c. The smallest count: about 3 or more in the first 5 minutes, and `n1 + 1` within 1 in the first hour | The gap target permits no fewer. Moss Works has a 23-minute gap with no unlock in its first hour (`IA:11`). |

Default band: option a. Proven game: IOM (front-loaded).

For `continuous`, the gap forces the band `[n1, 60]` (`pw_design.first_hour_band`). A lower count
conflicts with PC4. Show the forced value:

| Option | Evidence |
| --- | --- |
| a. About 3 or more in the first 5 minutes, and about n1 or more in the first hour (Recommended) | Forced by the gap target of A3.2b and by `PO11` (`BM:302`). No source counts unlocks (`SG:198`). |
| b. Change the first-hour gap target: ask A3.2b again | A larger gap gives a smaller n1. |

Write `unlocks.first_hour` = n1 and `unlocks.first_hour_tolerance` = 0.

**A1.4 Later hours.** How many unlocks does each later hour give? Ask it after the gap target.
The gap sets the smallest count n, and the skill shows n in the question:

- `check_in`: `n = ceil(60 / sigma)`, with `sigma` the median gap `median_gap_max` of A1.2 in
  minutes (`PO10`: `1 / u <= sigma`, `BM:301`). 4 for a 15-minute session.
- `continuous`: `n = ceil(60 / g) - 1`, with `g` the gap `unlock_max_min` of A3.2b. 5 for
  g = 10.

| Option | Evidence |
| --- | --- |
| a. About n or more (Recommended) | `check_in`: `PO10` (`BM:301`). `continuous`: forced by the gap target, no source (`SG:198`). |
| b. About n + 1 or more (`check_in` only) | Worked example 2 predicts 5 for each hour with a 15-minute session (`BM:242`). |
| c. Change the gap target: ask A1.2 or A3.2b again | A count below n needs a longer gap. 1 unlock each 30 minutes needs a session of 30 minutes or more (`PO10`). |

Default band: n or more for each hour. Write `unlocks.each_later_hour_min` = n. Proven game: none
measured (`RP:176-177`).

### Group A2

**A2.1 Idle value, game open.** An open game runs with no input. What is idle play worth
against active play, for the same wall-clock time? The value is `rho` of `BM` L4: idle gain
divided by active gain.

| Option | Evidence |
| --- | --- |
| a. 30% to 50% at 8 hours, falling with time, always above 0 and below 1 (Recommended) | Worked example 2: 0.56 at 1 hour, 0.33 at 8 hours, 0.16 at 24 hours, 0.036 at 7 days (`BM:438-441`). |
| b. Near the active rate inside the gate, and no spine progress | IOM pays x1 open against x1.25 active (`RP:273`), but no spine attempt runs without a visit (`RP:298-301`). |
| c. Above the range: more than about 50% at 8 hours | No proven game supports it. Moss Works idle beats active by 31 to 1 for each active minute (`IA:8`). |

Default band: `open_8h` 0.30 to 0.50. `open_1h`, `open_24h` and `open_7d` above 0 and below 1,
in falling order. The falling order is an inference from `BM` L4. Proven game: none measured.

**A2.2 Idle value, game closed.** What does time with the game closed pay against active play?

| Option | Evidence |
| --- | --- |
| a. 10% to 25% at 24 hours, falling with time (Recommended) | Worked example 2: 0.13 at 24 hours (`BM:393`, `BM:443`). |
| b. Like the reference game | IOM ore income against active play: 74% at 1 and 8 hours, 37% at 24 hours, 12% at 72 hours (`RP:298-301`). These are income shares, not `rho`. |
| c. About 100% up to a cap | Moss Works pays 100% for 8 hours (`SA:12`). It conflicts with a small S05 (catalog compatibility table). |

Default band: `closed_24h` 0.10 to 0.25. `closed_1h`, `closed_8h` and `closed_7d` above 0 and
below 1, in falling order. Proven game: IOM, as income shares.

**A2.4 Play-test speed.** At what game speed will a human test the game?

| Option | Evidence |
| --- | --- |
| a. About x10 (Recommended) | Worked example 2 is valid at x10 with 2-second decisions (`BM:420`). |
| b. About x1: the game speed only | Each design is valid at x1. |
| c. About x100 | It needs runs of `T_X >= 10 x 100 x D_h`: 33 minutes with `D_h` = 2 s (`BM:258`). Moss Works finishes in 3.75 minutes at x100 (`IA:13`). |

Default band: `k = 10`, `D_h = 2` s, `T_X >= 10 x k x D_h` (`PO13`). Proven game: none.

Fixed band, not asked: active slow play (a decision each 60 s) is within 1.25 times of active
greedy play (`BM:393`).

### Group A3 (play pattern `continuous` only)

The bands come from 11 short games (`references/short-games.md` section 4). There are no idle-value
targets: the record keeps only the law band 0 to 1 for the open game (decision D10).

**A3.1 Length to the hard end.** How many active hours does the active player need to reach the
hard end (S28)?

| Option | Evidence |
| --- | --- |
| a. 3 to 6 hours (Recommended) | The middle of the short games is 3 to 5 hours, and a 6-hour game is supported (`SG:177-178`). |
| b. 2 to 3 hours | Digseum 2 to 2.5 h, A Game About Digging a Hole 2 h (`SG:37-38`). |
| c. 6 to 10 hours | Tower Wizard 5 to 7 h, Scritchy Scratchy 7 to 10 h, Paperclips 9.2 h (`SG:178`). |
| d. Above the range: more than about 10 hours | Outside the band. Only the Gnorp Apologue is above 10 hours (`SG:177`). |

Default band: 3 to 6 hours. Evidence band: 2 to 10 hours (`SG:177`).

**A3.2 Largest gap between purchases.** This target replaces the session target A1.2: the session
is the whole sitting. How long may the player wait for the next purchase?

| Option | Evidence |
| --- | --- |
| a. About 2 minutes, and the first purchase inside about 2 minutes (Recommended) | Shelldiver: 4 to 5 purchases after each run of 1 to 2 minutes (`SG:180-181`). Single source. |
| b. About 5 minutes | A design blog: "If players wait more than 5 minutes for something meaningful, pacing has failed" (`D:362`). No short game measures it. |
| c. Longer | No source. The rule for a designer target applies. |

Default band: largest gap at most 2 minutes, first purchase at most 2 minutes (`SG:180-181`).

**A3.2b Largest gap between unlocks** (a new stage, a first purchase, or a milestone). Ask 2
questions in 1 call: the first hour, and the whole length. No source counts unlocks in time
(`SG:198`), so no option is Recommended, and the designer sets both: designer targets (band CT7).
Say in each question that no game measures the value, so the range is wide ("Targets are ranges").
The row PC4 (the longest wait for something new) checks them, and the solver report states how many
more unlocks the game needs, and where in time.

"In the first hour, about how long may the player wait for something new?"

| Option | Evidence |
| --- | --- |
| a. About 5 minutes (the default) | No source (`SG:198`). It forces about 11 unlocks in the first hour (A1.3). |
| b. About 3 minutes | No source. It forces about 19 unlocks in the first hour. |
| c. About 10 minutes | No source. It forces about 5 unlocks in the first hour. |

"After the first hour, about how long may the player wait for something new?"

| Option | Evidence |
| --- | --- |
| a. About 10 minutes (the default) | No source (`SG:198`). It forces about 5 unlocks in each later hour (A1.4). |
| b. About 5 minutes | No source. It forces about 11 unlocks in each later hour. |
| c. About 15 minutes | No source. It forces about 3 unlocks in each later hour. |

Put the default first. Do not offer a whole-length value below the first-hour value. The designer
can type another value. The counts come from `ceil(60 / gap) - 1` (A1.3 and A1.4).

## 3. Shortcut: a starter set

Ask this after the targets of step 2 and before part B: "Start from a starter set, then change
it?" Each set is proven by 1 or more games. A set answers each question of part B and each
optional system that it names. Each option states what the set gives: the lowest class of its
parts (section 1). `assets/systems.json` gives the variant, the `pattern` and the `proof_class` of
each set. The catalog gives the rules of the sets.

**The confirm question after a set.** Do not select the answers of the set in silence. Ask 1
question:

1. Show 1 line for each system of the set: the system, the variant and its class.
2. Name each answer whose class is below `proved`, with its reason: for example "S24.auto_buy:
   simulated, no law covers its timing".
3. Name each known conflict of the set and the change that removes it: for example "S28.full_tree:
   no model; S28.last_step gives a simulated model".
4. The options are: "a. Keep each answer", "b. Take the changes of item 3" (only when the set has a
   conflict), and "c. Change other answers" (the designer names them, and the skill asks the part B
   question of each named system).
5. Then ask only what the set does not answer: B2.2e, the targets of step 4, and the groups of
   part C without the systems that the set names.

Offer only the sets of the play pattern. For `continuous`:

| Option | Set | What you get |
| --- | --- | --- |
| a. K7: short active push with a reset and a hard end, proven by Nodebuster and Scritchy Scratchy (Recommended when the pitch names a reset, a prestige or a rebirth) | S01.geometric, S02.levelled_factor, S03.exponential, S04.power, S05.active_lane, S24.auto_buy, S28.last_step | A simulated model: timing from the simulator, marked not proved. The laws prove each part except S24 (automation): no law covers its timing. No set of this pattern is proved. K7 and K9 have the highest class, and the shape of the pitch decides between them. |
| b. K9: 1 long run with no reset and a hard end, proven by Digseum, Spaceplan and Universal Paperclips (Recommended when the pitch names no reset, no prestige, no rebirth, no runs and no shifts) | S01.geometric, S02.levelled_factor, S03.exponential, S04.none, S05.active_lane, S24.auto_buy, S28.last_step | A simulated model: timing from the simulator, marked not proved, for S24 and for the mark of S04 (an auto-buy plan moves the idle row PO6 to the simulation). The laws of 1 long run (`references/balance-math.md` section 3a) prove the growth, the unlock rate, the time to the hard end and the play-test speed. The rows of the reset loop are not applicable. Offer it when the pitch has 1 run, no reset and no run on a limit. |
| c. No starter set | Ask part B and part C in full. | The class of the answers. The Recommended answers of part B give the class of K7. |
| d. K8: run on a limit with no reset, proven by Shelldiver and A Game About Digging a Hole (offered last: no model) | S01.geometric, S02.levelled_factor, S03.exponential, S04.none, S05.active_lane, S24.auto_buy, S28.full_tree, S29.air | No model: the tools cannot do this yet. The scaffold has no writer for S28.full_tree. With S28.last_step in its place, K8 gives a simulated model: timing from the simulator, marked not proved. No law covers a game with no reset, so the solver takes the simulation path. |

**The set with the shape of the pitch is Recommended.** K7 and K9 have the same class, so the
tie-break of section 1 decides. Read the pitch, and take the first rule that matches:

1. The pitch names a reset, a prestige, a rebirth or a new start with a bonus: K7 has the shape.
2. The pitch names runs, shifts, dives or attempts that end on a limit, and no reset: K8 has the
   shape. K8 gives no model, so it gets no label (the label never goes to a lower class). Say that
   K8 has the shape, and offer the change that gives a model (S28.last_step in the place of
   S28.full_tree).
3. The pitch names none of these words: K9 has the shape. A pitch that says "not an idle game" or
   "no offline progress" names no reset.
4. The pitch does not decide (no pitch, or 2 rules match): K7, the set with the most proof games
   that the solver proves in the most parts.

Put the Recommended set first, with the label "(Recommended)", and say why in 1 clause: "the pitch
names no reset". Exactly 1 set has the label, or none (rule 2). A game with no reset is a valid
design: 5 of 10 short games have none (`SG:187`). Do not rate it below a game with a reset.

For `check_in`:

| Option | Set | What you get |
| --- | --- | --- |
| a. No starter set (Recommended) | Ask part B and part C in full. | With the Recommended answers of part B (S01.geometric, S02.levelled_factor, S03.exponential, S04.power, S05.active_lane, S06.penalty_cap, S08.timed_bank), a proved model. Each set of this pattern gives no model. |
| b. K2: slow idle with a second clock, proven by IOM (offered last: no model) | S01, S02, S03, S04, S05, S06, S07, S08, S09, S10, S11, S13, S19, S23, S26 | No model: the tools cannot do this yet. S02.add_in_group and S03.regime_change take the simulation path, and no tool writes S04.power, S07, S08, S09, S10.banked, S11, S19, S23 and S26 on that path. |
| c. K1: small active clicker, proven by Cookie Clicker (offered last: no model) | S01, S02, S03, S04, S05, S06, S10, S11, S12, S14, S20, plus S08 by decision D2 | No model: the tools cannot do this yet. S05.click_income takes the simulation path, and no tool writes S04.power, S08, S11, S14 and S20 on that path. |
| d. K3: run-based push with a strong reset, proven by Trimps (offered last: no model) | S01, S02, S03, S04, S05, S06, S08, S15, S17, S27 | No model: the tools cannot do this yet. S04.steps and S08.storage take the simulation path, and no tool writes S04, S08, S15, S17 and S27 on that path. |

The designer can also name K4 (layered exponent game, Antimatter Dimensions), K5 (skill tracks
with no reset, Melvor Idle) or K6 (business idle, AdVenture Capitalist) as text. Each of them gives
no model: K4 and K5 take the simulation path, and the solver leaves S12 and S14 of K6 out of the
model. K2 has all 3 presence gates and the most cited formulas. It is also the largest set.

## 4. Part B: the core systems

Each question asks for a variant, not for yes or no. Each option names its formula family, the
decision it gives, its scope, its `r`, and what it gives (section 1). The parameters and their
ranges are in the variant table of each card, and the column "Gives" of the card holds the class.

### Group B1: the core shape

**B1.1 Spine (S03).** Which wall shape do you want? It gives the push-or-build decision. Scope M.
Other variant: `S03.poly_then_exp` (no model).

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S03.exponential` (Recommended) | `h_1 x g^(n - 1)`, `g` 1.145 to 2.8 | 0 | A proved model. On the simulation path: a simulated model: timing from the simulator, marked not proved. The timing fit sets each wall. |
| b. `S03.regime_change` (offered last: no model) | `g` 2.8, then 30 after step 60 | 0 | No model: the tools cannot do this yet. |
| c. `S03.piecewise` (offered last: no model) | linear, then quadratic, then exponential steps | 0 | No model: the tools cannot do this yet. |
| d. `S03.xp_doubling` (offered last: no model) | the requirement doubles each 7 levels | 0 | No model: the tools cannot do this yet. |

For the play pattern `continuous`, do not ask B1.2 and B1.4 (rule R10). Ask B1.5 and B1.6 in
their place, so group B1 still has 4 questions.

**B1.2 Presence gate (slot S07, S08, S09).** Which limits on idle progress do you want? Select 1
or more. It gives the check-in decision. Decision D2 needs 1 or more.

| Option | Family | Scope | `r` | What you get |
| --- | --- | --- | --- | --- |
| a. S08 bank with a cap (Recommended, default of D2) | `S08.timed_bank`: `min(bank + elapsed / period_s, cap)` | S | 0 | A proved model. On the simulation path: no model. |
| b. S07 attempt on a cooldown | `S07.cooldown_attempt`: 30 s fight, 840 to 1200 s cooldown | M | 0 | A simulated model: timing from the simulator, marked not proved. The solver writes it, and no law covers it. On the simulation path: no model. |
| c. S09 real-time clock | `S09.rotation`, `S09.daily_cycle` or `S09.speed_free_drop` | S to M | 0 | A simulated model: timing from the simulator, marked not proved. The solver writes it, and no law covers it. On the simulation path: no model. |

Use multiple selection, or 1 or more letters in plain text. S09 needs a system on the clock (R7).

**B1.3 Active action (S05).** Which active premium do you want? It gives the attention decision.
Scope S to M. Other variants: `S05.kill_lane`, `S05.click_chance`, `S05.timing_skill` (each: no
model).

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S05.active_lane` (Recommended) | income x1.25 while active (A3.4) | 0 | A proved model. |
| b. `S05.manual_then_automate` | the manual action is the only income first; an S24 unlock replaces it at about half the game (`SG:65-66`) | 0 | A simulated model: timing from the simulator, marked not proved. No law covers the phase change (`BM` section 7). It takes the simulation path: with `S04.power`, the reset layer then has no model. |
| c. `S05.visitor` (offered last: no model) | a visitor with no bank (`S10.window`), decision D4 | 0 | No model: the tools cannot do this yet. |
| d. `S05.click_income` (offered last: no model) | a click pays a base value; no sourced range | 0 | No model: the tools cannot do this yet. |

`S05.route_reserve` (idle route `(L + 1)^2`, active route `(L + 1)^2.5`) also gives no model. For
`continuous`, 10 of 10 short games start with an active action, and automation replaces it later
(`SG:65-66`). Option b models that pattern. Option a models only a constant premium, and the laws
prove it.

**Which default wins.** Decision D10 of the catalog names option b as the default of the pattern.
The label of this question follows the class rule of section 1, so it stays on option a. Each
starter set of `continuous` uses option a. When the 2 differ for a pitch, do not choose in silence.

**When the pitch describes manual work that automation replaces** (the player clicks, cranks or
works by hand first, and a bought thing then does the work), ask this question in place of the
table above, also after a starter set. Use the words of the pitch for the manual work and for the
thing that replaces it:

"The pitch starts with <manual work>, and <thing> then does it. How do you want the model to hold
this?"

| Option | What you get |
| --- | --- |
| a. Keep the manual work as a bonus for the whole game (active premium, S05) | A proved model for this part. The model does not hold the phase where the manual work is the only income. The report names this as a difference from the pitch, and a play test must confirm the first minutes. |
| b. Model the change from manual work to automation (S05) | A simulated model: timing from the simulator, marked not proved. The solver takes the simulation path. State before the answer each selected system that has no writer on that path (section 5, "A system that loses its writer"). |

Neither option has the label: option a has the higher class, and option b has the shape of the
pitch. Say both facts. Write the answer and its reason in `notes`, and show it in the confirmation
(section 8).

**B1.4 Offline rule (S06).** Which closed-game rule do you want? It gives the return decision.
Scope S to M.

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S06.penalty_cap` (Recommended) | `min(away, T_cap) x f_closed x rate`, `f_closed` 0.85 to 0.92, `T_cap` 12 to 24 h | 0 | A proved model. On the simulation path: a simulated model: timing from the simulator, marked not proved. |
| b. `S06.decay_table` (offered last: no model) | 100% for hour 1, 70% for hours 2 to 6, then 50%; no cap is stated | 0 | No model: the tools cannot do this yet. |
| c. No closed progress (offered last: no model) | `f_closed = 0`. No source in the catalog uses it. | 0 | No model: `f_closed` 0 is outside the proven range 0.85 to 0.92, so the solver refuses it (exit code 2). |

**B1.5 Hard end (S28), `continuous` only.** How does the game end? It gives the push-for-the-end
decision. Scope S. Decision D10 refuses a continuous selection with no hard end (rule R12).

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S28.last_step` (Recommended) | the game ends at the last step N of the spine | 0 | A proved model. On the simulation path: a simulated model: timing from the simulator, marked not proved. The fit sets N. |
| b. `S28.full_tree` (offered last: no model) | the game ends when each tree node is bought | 0 | No model: the tools cannot do this yet. The scaffold has no writer for it. |
| c. `S28.last_purchase` (offered last: no model) | a last purchase ends the game; no sourced range | 0 | No model: the tools cannot do this yet. |

**B1.6 Automation unlock (S24), `continuous` only.** Which automation ends the manual phase? It
gives a purchase decision at the unlock. Scope S.

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S24.auto_buy` (Recommended) | an auto flag runs a buyer; no sourced range | 0 | A simulated model: timing from the simulator, marked not proved. No law covers its timing. No variant of S24 is proved, and this one has a writer on both paths. |
| b. `S24.auto_reset` | a reset when the gain rate falls, after `k` x the last run time, `k` 3 (`D:48`) | 0 | A simulated model: timing from the simulator, marked not proved. On the simulation path: no model. |

### Group B2: the core economy

**B2.1 Generator line (S01).** Which cost family do you want? It gives the next-purchase
decision. Scope S.

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S01.geometric` (Recommended) | `a x b^owned`, `b` 1.07 to 1.15 | 0 | A proved model. |
| b. `S01.steep` | `b` 1.2 to 1.3 | 0 | A proved model. |
| c. `S01.ladder` | 1 ratio for each tier, 1.16 to 1.19 | 0 | A proved model. |
| d. `S01.polynomial` | `(n + 1)^2.25 x 1e6` in a later era | not covered by L1 | A simulated model: timing from the simulator, marked not proved. It takes the simulation path: with `S04.power`, the reset layer then has no model. |

**B2.2 Multiplier upgrade (S02).** Which stacking rule do you want? It gives the cheapest-gain
decision. Scope S.

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S02.levelled_factor` (Recommended) | value `d^L`, cost `b^L` | `ln(d) / ln(b)`, 0.14 to 0.68 | A proved model. |
| b. `S02.add_in_group` | add inside a group, multiply between groups; `+v` for each level at cost ratio 1.2 to 1.5 | 0 | A simulated model: timing from the simulator, marked not proved. With `r = 0`, the reset loop has `s < 1` (PO3). It takes the simulation path: with `S04.power`, the reset layer then has no model. |
| c. `S02.multiply_all` (offered last: no model) | every line is its own factor. Warning: the numbers explode (`O:127`) | sum of all `r` | No model: the tools cannot do this yet. |

**B2.3 Reset layer (S04).** Which gain shape do you want? It gives the when-to-reset decision.
Scope M. Other variants: `S04.log`, `S04.power_of_log` (each: no model).

| Option | Family | `r` | What you get |
| --- | --- | --- | --- |
| a. `S04.power` (Recommended) | `(E / c)^p`, `p` 0.21 to 0.5 | 0; it sets `lambda = p q / (1 - R)` | A proved model. On the simulation path: no model, because the scaffold cannot write a reset layer. |
| b. `S04.none` | no reset; only with S22 as the spine (decision D1). For `continuous`: with S29 or S22 (decision D10), or as 1 long run with S28 and no S29 and no S22 (decision D11) | 0 | A simulated model: timing from the simulator, marked not proved. With S29 or S22, it takes the simulation path: no law gives a run length. As 1 long run, the laws prove the timing (`references/balance-math.md` section 3a), but the mark of S04 stays simulated: an auto-buy plan (S24) moves the idle row PO6 to the simulation. |
| c. `S04.threshold` (offered last: no model) | 1 point for each unit above 70 | 0 | No model: the tools cannot do this yet. |
| d. `S04.steps` (offered last: no model) | sub-exponential with steps at story points | 0 | No model: the tools cannot do this yet. |

For `continuous`, S04 is optional: 5 of 10 short games have no reset (`SG:187`). With no reset,
rule R11 needs S29 (a run with a permanent tree), S22, or S28 (1 long run to the hard end, starter set K9).

**B2.2e Line groups of S02.** Ask it after B2.2, or after the confirm question of a starter set.
"Which groups of upgrades does the game have?" List each group that the pitch names (for example
upgrades of the generator, of the speed, of the care). Each group is 1 S02 entry with its own `id`
and a `role` (section 8, rule 2). A group that raises a factor below 1 toward 1 (care that meets a
need) has the role `care`. A care group selects S30 (upkeep): group C6 then asks only the variant
of S30, not yes or no. With 1 group or no answer, write 1 S02 entry with no `id`.

### Targets after the core systems

Ask these targets after the starter set or part B, because each one exists only for 1 answer of
a core system.

**A2.3 Reset gap** (only with a reset layer: S04 is not `S04.none`). How much faster may a
patient player (reset at `X_p = 10`) be than an early player (reset at `X_e = 1`)?

| Option | Evidence |
| --- | --- |
| a. About 2 times or less (Recommended) | Worked example 2: 1.84 (`BM:392`, `BM:453`). It needs `s <= 4.2` (`BM:192`). |
| b. About 1.5 times or less | `PO4` with a smaller `Dg`. It needs a smaller `s`. `s > 1` is still necessary (`PO3`). |
| c. About 3 times or less | Moss Works: the patient rule reaches Stage 40 in 2.26 hours against 6.25 hours, 2.8 times (`IA:7`, `IA:12`, computed). That build fails other targets. |

Default band: `Dg = 2`. Proven game: none measured. With `S04.none`, or with no S04, do not ask
it, and leave `reset_gap` out of the targets file (rules R1 and R11).

**A3.4 Active factor** (`continuous`, only with `S05.active_lane`, optional). How much more does
active play pay than an open game with no input? The factor is the parameter `a` of `S05.active_lane`. The short games give no number
(`SG:199`). The catalog gives 1 sourced value: x1.25 (`TB:61`, `TA:507`).

| Option | Evidence |
| --- | --- |
| a. About x1.25, the sourced value (Recommended) | `S05.active_lane` (`TB:61`, `TA:507`). IOM pays x1 open against x1.25 active (`RP:273`). |
| b. A larger value: designer target, above the sourced value | No source. The skill warns 1 time, keeps the value, and marks `active_factor` `"unsupported": true`. The solver report repeats the warning. |

Default: x1.25. With no answer, leave `active_factor` out: the solver takes x1.25. With another
variant of S05, do not ask it.

**T.run Run length (`run_min`).** The meaning of `run_min` depends on the reset layer:

1. With a reset layer: `run_min` is the length of 1 run from 1 reset to the next, in minutes. The
   default is `median_gap_max` of A1.2 for `check_in`, else 15. In the rebalance mode of a
   `continuous` game, the solver uses `min(run_min, 0.8 x unlock_max_min)`, so `run_min` is an
   upper limit (`references/design-solver.md`, "The run length"). Ask it only when the pitch names a
   run length. Else leave it out, and the solver takes the default.
2. With no reset layer (`S04.none`, or no S04): leave `run_min` out. On the path "1 long run" the
   solver does not read it. It does not set the shift length of S29.
3. The shift length of S29 is not a target. It is the parameter `run_s` of `S29.air` (60 to 120 s,
   default 120, `SG:78`). Ask it with the variant of S29 in group C1. The simulator plays 1
   continuous run, so `run_s` does not change the model. The report names the shift rhythm as a
   part that a play test must confirm (`references/simulation-path.md`).

## 5. Part C: the optional systems

Ask part C in the groups C1 to C7, in this order, then group C8. `assets/systems.json` holds the
groups as `interview_order.groups`. Each numbered item below is the text of 1 option: the system,
the decision that it gives, its scope (S, M or L), the systems that it needs, and the line "Model:"
with the class of the best variant of the system (section 1).

1. Ask each group as 1 multi-select question: "Which of these systems do you want? Select none, 1
   or more." In plain text, list the items with the letters a to d, and ask for the letters or
   "none". A tool for questions has 4 options at most:
   - A group with 4 systems left: the 4 options are the 4 systems. Write in the question text:
     "Type 'none' as your own answer to take none of them." "None" is then not an option, so it
     cannot have a label. When rule 2 makes "none" Recommended, say it in the question text: "The
     content shortfall is 0, so none is the recommended answer."
   - A group with 2 or 3 systems left: the systems, then the option "None of these" last. It gets
     "(Recommended)" and the first place only when rule 2 says so.
   - A group with 1 system left: 1 question with the options "Yes" and "No". "No" gets
     "(Recommended)" and the first place only when rule 2 says so.
2. **"None" is Recommended only when the content shortfall is 0** ("The content need", below). With
   a shortfall above 0, no option is "none (Recommended)". State the shortfall in each group
   question, and state for each system its line "Content:". A system that adds content elements is
   then a way to close the shortfall, and it is not Recommended for that reason alone: a system must
   add a decision that the designer wants (`references/content-step.md` section 1, "Simple"). More
   content inside a selected system closes the shortfall too. Rule 7 gives the 1 system that is
   Recommended. A system with no model says "no model: the tools cannot do this yet", and it is the
   last option.
3. Leave out each system that an answer rule removes, that the starter set names, or that part B
   asked. Do not ask a group with no system left.
4. After each group, ask the variant and the parameters of each selected system with the variant
   table of its card, and state the class of each variant from its column "Gives". Put each
   offered system that the designer did not select into `refused`. Then apply the answer rules and
   the needs of section 6. Then ask the next group.
5. 2 systems where 1 answer removes or changes the other are never in 1 group, and each group uses
   only the answers of part B and of the groups before it. `tests/test_interview_order.py` checks
   both rules.
6. Label a system that only multiplies "multiplier only".
7. **With no reset layer, the count milestones are Recommended for the generator line.** When S04 is
   `S04.none` or not selected, put the milestone multipliers on count (S12) first in group C4 with
   "(Recommended)", and say why: "With no reset, the generator line is additive. Each new unit adds
   less and less, so the line stops to matter after the first minutes. A multiplier at fixed counts
   keeps each unit worth a purchase, and each count step is a content element. The solver adds count
   steps to the generator in each case, so a yes here lets you set their size and their spacing."
   The label follows the tie-break of section 1: each option of the group has the same class with a
   proved selection, and S12 has the shape of a game with no reset.
8. Ask each system in the words of the game first: "Food, water and rest (upkeep, S30)". The items
   below give the catalog name. Put the words of the pitch before it.

### The content need (before group C1)

Run this before the first group, with the targets file and the selection so far (the starter set or
part B, and the line groups of B2.2e):

```bash
python3 <skill_dir>/scripts/pw_content.py need <work>/targets.json --selection <work>/selection.json
```

`references/content-step.md` gives each formula. Tell the designer 3 numbers in plain words before
group C1: "The game needs about N things to buy, unlock or light up, so that the wait for something
new stays short (rows CG and PC4). The systems so far give M. So about K are missing: K1 in the
first hour." The stages of the spine do not count: a stage alone is not a content element. Repeat
the shortfall in each group question. After part C, the content step names the elements
(`references/content-step.md`).

Each item below has the line "Content:": what the system adds as content elements, with the number
of its catalog card, or "no number" when the card gives none. A system with "no number" can still
hold content: the content step names it.

### A system that loses its writer

A choice that takes the simulation path (S29, S22, `S05.manual_then_automate`, `S01.polynomial`,
`S02.add_in_group`) changes the class of each other system to its `proof_class_sim_path`
(section 1). Before the designer answers such a choice, read `proof_class_sim_path` of each selected
system in `assets/systems.json`. For each system whose class falls to `none`, add 1 line to the
question: "If you take <choice>, <system in the words of the game> has no model: the tools cannot
write it on the simulation path." Group C1 asks S29 and S22, so it gives this warning first. Do
not name a system from memory: the class in `assets/systems.json` decides (upkeep, S30, has a
writer there now). The designer then keeps 1 of the 2, or keeps both and gets a stub that a person writes.

### Group C1: far systems (asked first)

1. Do you want a **collection with set bonus (S19)**? It gives the which-item decision. It costs
   M. It needs a drop source: S03, S10 or S22.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 milestone for each item and each full set. No number.

2. Do you want a **skill track (S22)**? It gives the which-skill decision. It costs M. It needs
   actions in other systems.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 milestone for each level that opens content. No number.

3. Do you want a **minigame with export (S23)**? It gives the attention decision. It costs L. It
   needs 1 link in and 2 links out.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 new system, and the lines of its own tree. No number.

4. Do you want a **run on a limit (S29)**? It gives the end-the-run and which-node decisions.
   It costs M. It needs a run income (S01 or S05) and a tree of S02 lines in the run currency.
   Model: no model with a proved selection: the solver leaves it out of the model. On the simulation path: a simulated model: timing from the simulator, marked not proved. The simulator plays 1 continuous run, and the run limit is not in the model.
   Content: 1 single purchase for each node of the tree. No number for the node count
   (`SG:205`); the card gives 4 to 5 purchases after each run (`SG:79`).

### Group C2: reset extras (asked only if S04 is not `S04.none`)

1. Do you want a **prestige shop (S15)**? It gives the which-permanent-upgrade decision. It
   costs M. It needs S04.
   Model: a proved model (`S15.geometric`). On the simulation path: no model.
   Content: 1 purchase line for each perk of the shop. No number.

2. Do you want **challenge runs (S17)**? It gives the which-challenge decision. It costs M. It
   needs S04.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 milestone for each challenge. No number.

### Group C3: active and presence extras

1. Do you want a **bank with a cap (S08)** as a second gate? It gives the check-in decision. It
   costs S. It needs a timed reward or a resource with storage.
   Model: a proved model (`S08.timed_bank`). On the simulation path: no model.
   Content: 1 purchase line (the solver writes 1 bank line).

2. Do you want a **random visitor (S10)**? It gives the tap decision. It costs S. It needs a
   reward table that pays S01 currency, S11 or S26.
   Model: no model with a proved selection: the solver leaves it out of the model. On the simulation path: a simulated model: timing from the simulator, marked not proved (`S10.window`, chance as its mean rate).
   Content: none: a visitor is an event, not a content element.

3. Do you want an **automation unlock (S24)**? It gives a purchase decision at the unlock. It
   costs S. It needs the manual system that it automates. It does not run while the game is
   closed (decision D5).
   Model: a simulated model: timing from the simulator, marked not proved (`S24.auto_buy`).
   Content: 1 single purchase for each line that it automates.

4. Do you want a **hard end (S28)**? It gives the push-for-the-end decision. It costs S. It needs
   S03 or a last purchase line. Ask it only for `check_in`: `continuous` asks B1.5.
   Model: as B1.5: a proved model (`S28.last_step`).
   Content: none: the end is a goal, not a new element.

### Group C4: line growth

1. Do you want **milestone multipliers on count (S12)**? It gives the buy-an-old-line decision.
   It costs S. It needs S01. Multiplier only.
   Model: no model with a proved selection: the solver leaves it out of the model. On the simulation path: a simulated model: timing from the simulator, marked not proved (`S12.count_steps`).
   Content: 1 milestone for each count step of each line: 2 steps up to 50 owned,
   then 1 for each 100 (`O:202-203`), or 1 for each 25 (`O:203`) or each 10 (`A:117`).

2. Do you want a **second reset layer (S16)**? It gives the deep-reset decision. It costs M to
   L. It needs S04 and S15.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1: the first deeper reset. Its shop is content of S15. No number.

3. Do you want **chained producers (S18)**? It gives the which-tier decision. It costs M. It
   needs S01. It changes the presence math (decision D7).
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 purchase line for each tier. No number.

4. Do you want a **line rebirth (S27)**? It gives the rebirth-now decision. It costs S. It needs
   S01.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 milestone for each line with a rebirth. No number.

### Group C5: breadth and economy

1. Do you want a **task list with a shop (S20)**? It gives the what-next decision. It costs S to
   M. It needs 3 or more systems for the tasks to name.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 milestone for each task: the card gives 100 tasks in 3 tiers (`P:329-332`).

2. Do you want an **allocation pool (S21)**? It gives the split decision. It costs M. It needs a
   pool resource and 2 or more sinks.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 element for each sink, 2 or more. No number above that.

3. Do you want a **rotating event (S25)**? It gives the push-before-the-end decision. It costs L
   and live operation. It needs S03.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 new system with its own short ladder. No number.

4. Do you want a **premium currency with many faucets (S26)**? It gives the faucet and sink
   decisions. It costs M to L. It needs 3 or more faucets and a sink ladder.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 element for each faucet (3 or more) and each permanent unlock. No number above that.

### Group C6: systems that need an earlier answer

1. Do you want a **real-time clock (S09)** as a second gate? It gives the visit decision. It
   costs S to M. It needs a system to put on the clock: S23, S19 or S22.
   Model: a simulated model: timing from the simulator, marked not proved. The solver writes it, and no law covers it. On the simulation path: no model.
   Content: none alone: the system on the clock holds the content.

2. Do you want a **timed buff (S11)**? It gives the when-to-use decision. It costs S to M. It
   needs S01 and a buff source: S10, S19 or S05.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: 1 element for each buff. No number.

3. Do you want a **cap and cap raiser (S13)**? It gives the which-system-to-push decision. It
   costs S. It needs a far system: S19, S22, S23, or the spine S03 with `S13.raised_cap`.
   Model: no model with a proved selection: the solver leaves it out of the model. On the simulation path: a simulated model: timing from the simulator, marked not proved (`S13.raised_cap`).
   Content: 1 single purchase or milestone for each cap raise. No number.

4. Do you want **upkeep (S30)**? It gives the more-producers-or-more-care decision: each unit adds
   to a need (food, rest, power), and care capacity meets it, so the output is times `u` <= 1.
   It costs M. It needs a producer line (S01); care lines are an S02 entry with the role `care`,
   or an allocation pool (S21). Ask it when the pitch names upkeep, needs or maintenance.
   Model: `S30.need_capacity` gives a simulated model: the solver writes the upkeep block and 1
   care line, PU3 is proved by its formula, and PU1, PU2 and PU4 are simulated
   (`references/balance-math.md` section 3b). On the simulation path: no model (a stub). The other
   variants give no model. Offer it with `S30.need_capacity`.
   Content: 1 purchase line for each care line (the solver writes 1). The needs of the pitch (for
   example food, water, rest) can be 1 care line each: no number.

### Group C7: hold bonus (asked only if S04 is not `S04.none`, or S26 is selected)

1. Do you want a **hold bonus (S14)**? It gives the spend-or-hold decision. It costs S. It needs
   S04 or S26.
   Model: no model: the tools cannot do this yet. It comes last in its group.
   Content: none: it changes when the player spends.

### Group C8: pitch mechanics with no card

Ask it after group C7. Read the pitch again, and list each mechanic that no answer of part B or
part C covers. Ask 1 question for each mechanic:

1. State the mechanic in the words of the pitch.
2. Give the nearest card (`references/system-archetypes.md` maps a symptom to a card) and the
   decision that it adds, or say that no card covers the mechanic.
3. The options are: "a. Use <the nearest card>" (only when a card covers the decision; then ask
   its variant), "b. Keep it as a mechanic with no card: simulated or estimated, not proved", and
   "c. Leave it out of the design".
4. **A rule in free text becomes a formula.** When the designer answers with a rule in words (an
   own answer, or a detail after a, b or c), do not keep the words alone:
   - Write the rule as 1 formula with named inputs, and give each constant a value and its origin
     ("my choice, no source" when the designer gave no number).
   - Compute 3 example values, so that the designer sees the shape: the first unit, a middle count
     and a large count.
   - Ask 1 question: "I read your rule as <formula>. Examples: <3 values>. a. Yes, this is my rule.
     b. Change the formula. c. Change a number."
   - Write the confirmed formula into `notes` beside the words of the designer, and mark each
     constant that has no source. The formula is then simulated or estimated, not proved.
   - Say what the formula does to the row PV (each purchase gives a gain that the player feels): a
     rule of falling returns gives a small gain for a late unit, so the designer may need a lower
     floor for that line.
5. For b, add 1 line to `notes`: "no card: <mechanic>: <what the player decides>". The solver does
   not write it. After the solve, write it as model lines by hand (`references/simulation-path.md`,
   "Complete a stub" and its worked example of a mechanic with no catalog card), and run the proof
   sheet again. The result is then simulated at best.

### After the optional systems: the demo end

Ask A3.3 after part C, because `before_system` can name an optional system.

**A3.3 Demo end.** Where does the demo end?

| Option | Evidence |
| --- | --- |
| a. A cliffhanger just before a new system opens, 15 to 30 minutes in (Recommended) | The guide consensus is 15 to 30 minutes, with a cliffhanger end (`SG:183-184`). |
| b. 40 to 60 minutes, for a strategy or management game | The same guide (`SG:183`). |
| c. About 2 hours or more | Flag: longer than the guide band. The nearest proof is the free Tower Wizard prototype of 1 to 2 hours (`SG:189`). The full game must then add new content in each part past the demo (`SG:185`). |
| d. No demo | The store page of Nodebuster states none (`SG:56`). |

Default: option a. The record states the demo end in minutes and the system that opens after it.
The proof sheet checks the parts past the demo (row PC3 of `references/proof-sheet.md`).

Ask A3.3 only for `continuous`. After an answer a, b or c, ask 1 more question: "Which system
opens just after the demo end?" The options are the selected systems that are not open at the
start (not S01, S02, S03 or S05), at most 4. Write its id as `demo_end.before_system`. With no such
system, write `null`: the demo then ends before a new stage of the spine. Use the band rule of
section 2 for a band of minutes: "15 to 30 minutes" gives `minutes` 22.5. For d, leave `demo_end`
out of the targets file.

## 6. Answer rules

`assets/systems.json` holds these rules as `answer_rules` (ids `AR1` to `AR12`). The rule R<n>
here is the rule AR<n> there.

| Rule | When | Effect |
| --- | --- | --- |
| R1 | S04 is `S04.none` | Skip group C2 and S16. Drop the targets A2.3 and `run_min`. S14 stays only if S26 is selected. S22 is required: add it, and ask for its variant. |
| R2 | The gate includes S08 | Leave S08 out of group C3. |
| R3 | The gate includes S09 | Leave S09 out of group C6. |
| R4 | S05 is `S05.visitor` | S10 is selected with `S10.window`. Leave S10 out of group C3. |
| R5 | S15 is refused | Leave S16 out of group C4. |
| R6 | S10 and S19 are refused, and S05 is not `S05.active_lane` or `S05.kill_lane` | Leave S11 out of group C6. S11 has no buff source. |
| R7 | S23, S19 and S22 are refused | Remove S09 unless the gate selected it. If the gate selected it, S09 uses `S09.rotation` or `S09.daily_cycle`. |
| R8 | S19, S22 and S23 are refused | Offer S13 only with `S13.raised_cap`. |
| R9 | S24 is selected | Record that S24 does not run while the game is closed. |
| R10 | The play pattern is `continuous` | S06 is optional with the default "none". The gate is optional. S04 is optional. Ask group A3 in place of A1.1, A1.2, A2.1 and A2.2. Ask B1.5 and B1.6 in place of B1.2 and B1.4. |
| R11 | The play pattern is `continuous` and S04 is `S04.none` or refused | Skip group C2 and S16. Drop the targets A2.3 and `run_min`. Group C1 asks S29 and S22. With S29 or S22, the solver takes the simulation path. With neither, the game is 1 long run to the hard end S28 (decision D11, K9). Rule R1 does not apply. |
| R12 | The play pattern is `continuous` and S28 is refused | Refuse the selection. S28 is core (decision D10). |

Selections that need a second system:

| Selection | Needs |
| --- | --- |
| S16 | S15 |
| S11 | S10, S19 or S05 |
| S09 | S23, S19 or S22 (or the gate rule of R7) |
| S14 | S04 or S26 |
| S13 | S19, S22, S23 or `S13.raised_cap` |
| S19 | S03, S10 or S22 |
| S21 | 2 or more sinks |
| S30 | S01, and care lines (an S02 entry with the role `care`) or S21 |
| S26 | 3 or more faucets and a sink ladder |

When a selection lacks its second system, the skill asks 1 question: "<system> needs
<systems>. Add <first option>, or drop <system>?"

A new idea from the skill must come from the catalog, pass the compatibility table of the
catalog, and state the decision it adds.

## 7. When the designer is not available

The skill does not stop:

1. It takes the selection with the highest class (section 1). For `continuous`, that is the set
   with the shape of the pitch by the rules of section 3 (K9 for a pitch that names no reset and no
   runs, else K7): a simulated model, because no law covers S24. For `check_in`, each starter set gives no model,
   so it takes the Recommended answers of part B: S01.geometric, S02.levelled_factor,
   S03.exponential, S04.power, S05.active_lane, S06.penalty_cap and S08.timed_bank, a proved
   model. The record then has no `starter_set`.
2. It takes the default band of each target in part A.
3. It takes the default of each parameter from the variant table.
4. It writes a selection record with `"designer_answered": false`.
5. It does the content step with no question (`references/content-step.md`): it names the elements
   from the pitch, and it says "content is missing: N elements" for the rest. It invents no content.
6. It states this first in the report: "The designer did not answer. The design uses <set or the
   Recommended answers of part B> and the default bands. It gives <class>."

## 8. The selection record

**The confirmation** (step 7 of the order). Before you write the files, ask 1 question. Show
the selected systems with their variants, the refused systems, the targets with their values and
tolerances, each target with `"unsupported": true`, each note "no card", and the class of the
selection (section 1). The options are "a. Write the files" and "b. Change an answer" (the
designer names it, and the skill asks that question again and then each later question that
depends on it).

The interview ends with 1 selection record as JSON. The design solver (`scripts/pw_design.py`)
reads it as 2 files: the record without `targets` (the selection), and an object with
`play_pattern` and `targets` (the targets). `assets/example_selection.json` and
`assets/example_targets.json` are the Moss Works example. The solver also reads 3 optional target
fields: `length_h.extend_to_h` (a longer game with the same formulas), `active_factor` (the active
income factor of S05, with `unsupported`), and `run_min` (the run length of a reset layer, T.run in section 4).
For `continuous`, the targets have `purchase_gap` (`largest_max_min`, `first_max_min`, and the unlock
gaps `unlock_first_hour_max_min` and `unlock_max_min` of A3.2b) in place of
`session_min`, `demo_end` (`minutes`, `before_system`, `unsupported`), and no closed idle value.
`length_h` is the length to the hard end. Each pattern can also give `purchase_value` (`floor`, `source`): the
smallest gain of 1 level of a line that the player must feel. The default 0.10 is a designer target: the low end
of the proven step of S02.levelled_factor. `references/design-solver.md` gives the exact keys.

```json
{
  "designer_answered": true,
  "starter_set": "K2",
  "targets": {
    "length_h": {"value": 20, "tolerance": 0.15, "unsupported": false},
    "session_min": {"median_gap_max": 15, "largest_gap_max": 20, "unsupported": false},
    "unlocks": {"first_5_min_min": 3, "first_hour": 6, "first_hour_tolerance": 1, "each_later_hour_min": 4, "unsupported": false},
    "idle": {"open_1h": [0.0, 1.0], "open_8h": [0.30, 0.50], "open_24h": [0.0, 1.0], "open_7d": [0.0, 1.0],
             "closed_1h": [0.0, 1.0], "closed_8h": [0.0, 1.0], "closed_24h": [0.10, 0.25], "closed_7d": [0.0, 1.0],
             "unsupported": false},
    "reset_gap": {"X_e": 1, "X_p": 10, "Dg": 2, "unsupported": false},
    "play_test_speed": {"k": 10, "D_h_s": 2, "unsupported": false},
    "active_slow_max": 1.25
  },
  "selected": [
    {"system": "S01", "variant": "S01.geometric", "parameters": {"b": 1.15}},
    {"system": "S04", "variant": "S04.power", "parameters": {"p": 0.5}},
    {"system": "S08", "variant": "S08.timed_bank", "parameters": {}},
    {"id": "speed", "system": "S02", "variant": "S02.levelled_factor", "role": "speed upgrades", "parameters": {}}
  ],
  "refused": ["S16", "S25"],
  "notes": ["S24 does not run while the game is closed (decision D5)."]
}
```

Rules for the record:

1. `selected` has each core system of the play pattern and the presence gates that the pattern
   needs: `check_in` needs S01 to S06 and 1 or more of S07, S08 and S09; `continuous` needs S01,
   S02, S03, S05, S24 and S28. With no S04, the record is 1 long run, or it has S29 or S22 (rule R11).
2. Each entry of `selected` names a variant id of `assets/systems.json`. An empty `parameters`
   object means the defaults. An entry can have an `id` and a `role`. A system can have more than
   1 entry, and then each entry needs its own `id`: for example 3 S02 entries for generator
   upgrades, speed upgrades, and care upgrades with the role `care` (S30). Question B2.2e asks
   the line groups. `references/design-solver.md` gives the rules.
3. A parameter value outside its proven range is a finding. The skill states the parameter, the
   value and the range.
4. `refused` has each system that the designer refused. A system that a rule removed is not in
   `refused`.
5. Each target with `"unsupported": true` came from the rule of part A.
