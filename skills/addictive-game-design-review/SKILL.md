---
name: addictive-game-design-review
description: "Use when asked to review the gameplay of a built incremental, idle or upgrade-driven game (a game directory, its src folders and data) and say what to change so that players want one more upgrade: rebalance, redo options, add a feature, remove a feature, automate, reveal. Writes a ranked finding report with a file and a line for each fault."
---

# Addictive game design review

This skill reviews a built incremental game and says what to change to make the gameplay better. The work product is
1 report: a ranked list of findings, each with a file and a line, the change, and a test that shows the change worked.

"Addictive" here means the pull of **one more upgrade**: the player continues because the next step makes them more
capable, shows them something new, or gives them a different problem. It does not mean compulsion. A change that holds
a player by pressure, loss or deceit is a fault, and check R-17 finds it.

The thesis of the research (`references/evidence.md`): the problem is not how to give more rewards. It is how to make
each success change what the next decision means. A good loop does not return to the same problem:

```
Act -> Gain -> Choose -> Transform -> Discover -> Act (from a changed position)
```

A game goes stale when the loop is solved: buy the obvious next thing, wait for the next price, repeat. Most findings
of a review are places where the loop is solved, and the ways the 6 patterns of `references/patterns.md` open it again.

## Limits

1. You review and propose. You change no game file, and you write no game code.
2. A finding is a design inference, not a proof. The research supports general mechanisms; no study tested this game.
   Each finding carries an evidence grade (`references/evidence.md`), and the grade limits how firmly you state it.
3. Every timing target in this skill is a hypothesis for a play test, stated as "about X" with a range. A game whose
   pitch promises a different pace keeps its own pace. The promise of the game comes before a number.
4. More minutes of play is not the goal. A game can add minutes through friction and be worse. Judge each change by
   what the player gains: capability, choice, discovery.
5. You do not prove the math. A rebalance finding gives the direction, the size class and the reason. The proof of the
   new numbers is a task for the `incremental-game-design` skill and its seat.

## Workflow

### 1. Find out what you were given

- **A game directory.** Find the engine, the entry scene or file, the data files (JSON, resources, tables) and the code
  that reads them. The build is the truth. A design document is the intent, and each difference is a finding.
- **Named src folders.** Review those folders. Read outside them only to follow a value to its source.
- **Play data, a save, a balance test or a simulator in the project.** Read it first. A measured time is better than a
  computed time.
- **The pitch.** Find it in a README, a design note or the store text. Write the promise of the game in 2 to 4 lines:
  the fantasy, what grows, what the player watches change. If you find none, write your best reading and mark it as an
  assumption. Each finding must serve this promise.

If the `incremental-game-design` skill is in your skill list, its `scripts/scan_godot.py` reads the cost formulas and
unlock signals of a Godot project, and its timing scripts play a model. Use them for the numbers of steps 2 and 3. If
it is not there, read the numbers from the code and compute by hand, and mark each time as "estimated".

### 2. Map the game

Write the map before any opinion. Each row has a file and a line. Use the tables of `references/report-template.md`.

1. **Loop map.** For each stage of the loop (Act, Gain, Choose, Transform, Discover), what the game has. An empty
   stage is the first finding.
2. **Inventory.** Every action of the player, resource, producer, upgrade, choice point, random event, automation,
   reveal, reset, timer and feedback hook.
3. **Upgrade classes.** Put each upgrade in 1 class:
   - `number`: only a bigger value (+X%, xN, -cost).
   - `behaviour`: a unit or system does something different.
   - `interaction`: 2 things that were independent now affect each other.
   - `constraint`: a limit, a chore or a bottleneck goes away.
   - `reveal`: a new system, screen, goal or rule appears.
4. **Production chain.** Write the formula of useful output per second. Say if it is 1 product of multipliers, or a
   chain of separate stages where the smallest stage limits the output.
5. **Decision list.** Each point where the player selects between options. For each: the options, what the player
   gives up, and if 1 option is always best.

### 3. Build the timeline

Write when each event occurs for a new player, from the data: first action with a visible result, first purchase,
first automation, first trade-off, first new mechanic, first visible change of the world, first new responsibility,
then each later reveal to the end of the content. Mark each time `measured`, `simulated` or `estimated`.

Compare with the journey targets of `references/review-checks.md` section 2. Find the dead time: each span in which
the player can make no useful decision and the only action is to wait or to repeat 1 input.

### 4. Run the checks

Run the 18 checks R-1 to R-18 of `references/review-checks.md`. Each check gives 1 status:

| Status | Meaning |
| --- | --- |
| `ok` | The game meets the check. Name the evidence. This goes in the "keep" list. |
| `finding` | A fault, with evidence. Write 1 or more findings. |
| `not audited` | You could not read what the check needs. Say what is missing. |

Never write `ok` without evidence. `not audited` is an honest result.

The checks in 1 line each:

| Check | Question |
| --- | --- |
| R-1 Solved loop | Is there 1 obvious shopping list? |
| R-2 Upgrade quality | What share of upgrades is class `number` only? |
| R-3 Constrained builds | Must the player pass up 1 good option to take another? |
| R-4 Bottlenecks | Does the limit of the output move between stages? |
| R-5 Combinations | Do 3 or more pairs give an effect that is not a sum of percentages? |
| R-6 Bounded randomness | Are essentials certain, options variable, and can the player steer the draw? |
| R-7 Automation as promotion | Does each mastered chore go away, and does a larger decision replace it? |
| R-8 The game changes | Does the question "one more what?" change through the game? |
| R-9 Progressive reveal | Does the player see a little of the next system before it opens? |
| R-10 Three horizons | Are a near, a medium and a far goal visible at the same time? |
| R-11 Dead time | How long can the player make no useful decision? |
| R-12 First minutes | Do the first events occur in the journey ranges? |
| R-13 Reward variety | Are the rewards of more than 1 type? |
| R-14 Surprise and acceleration | Do earned breakthroughs exceed what the player expected? |
| R-15 Visible world | Can the player see progress with the counters hidden? |
| R-16 Feedback proportion | Do small gains stay quiet and large changes get the large effects? |
| R-17 Coercion | Does anything hold the player by pressure, loss or deceit? |
| R-18 Instrumentation | Can the game record what a play test must measure? |

### 5. Write the findings

Each finding has 1 kind:

| Kind | Use it when |
| --- | --- |
| `REBALANCE` | The system is right and its numbers are wrong: a wall, a gap, a dominant option, a late first event |
| `REDO` | The options of a choice are wrong: 1 always wins, or they differ only in size |
| `ADD` | A stage of the loop or a pattern is missing, and 1 new system gives the player a new decision |
| `REMOVE` | A system adds no decision, hides the core, or holds the player by coercion |
| `AUTOMATE` | A mastered chore stays manual |
| `REVEAL` | The order or the timing of what the player sees is wrong: all at the start, or no hint of what comes |
| `SURFACE` | The game does the right thing and the player cannot see it |
| `MEASURE` | A decision needs play data that the game does not record |

Each finding has these fields. A finding without evidence or without a test is not a finding; delete it.

- **Evidence**: the file and the line, and the value or the formula there.
- **Fault**: what the player experiences, in 1 or 2 sentences. Not "low engagement": say what the player does or
  cannot do.
- **Change**: 1 concrete change. For `REBALANCE`, the direction and the size class, not a proved number. For `ADD`,
  the decision that the player gains, the smallest version, and what it must not randomize or block.
- **Pattern and grade**: the pattern or the research finding behind the change, and its evidence grade.
- **Test**: the acceptance test, as something a play test or a simulation can observe.
- **Cost**: `S` (data only), `M` (1 system changes), `L` (a new system or a new screen).
- **Risk**: what the change can break, and what must stay the same.
- **Owner**: the seat or the skill that does the work.

Rules for a good set of findings:

1. Prefer the smallest change. Try `SURFACE`, `REBALANCE` and `REVEAL` before `ADD`. A new system is correct only when
   it adds a decision that no change of numbers can add.
2. `REMOVE` is a full finding. A game with fewer systems, each of which adds a decision, is better than a game with
   many. Look for something to remove in every review.
3. A powerful upgrade does 1 or more of these: it changes a behaviour, it makes an interaction, or it removes a
   constraint. Upgrades of class `number` are for pacing. They must not be the whole design.
4. Never randomize what the player needs to operate. Randomness goes on options, and the player earns control of it.
5. For each `ADD` and each large `REDO`, write the 3 variants of `references/review-checks.md` section 4, so that a
   play test compares the idea with a numbers-only control of equal value.
6. Write the "keep" list: what works and must not change. A review that lists only faults causes harm when the build
   follows it.

### 6. Rank

Score each finding: impact (1 to 3) x confidence (1 to 3), divided by the cost (S = 1, M = 2, L = 3). Impact is how
much the fault blocks the loop: 3 when the loop is solved or a stage is empty. Confidence comes from the evidence
grade and from how the time was known (`measured` is above `estimated`). Any `REMOVE` for coercion goes first.

Then write the plan: "do first" (the 3 findings of highest score), "next", and "later". Say which findings depend on
each other, and which 1 change you would make if only 1 was possible.

### 7. Write the report

Write to the file the task names, or `ai_docs/game-review/<game>/review.md` when it names none. Keep it outside the
asset tree of the engine (`res://` in Godot). Use `references/report-template.md`. Reply with the path, the verdict in
1 line, the "do first" list, each check that is `not audited`, and your assumptions.

## Hand-offs

- **New numbers** (a cost curve, a wall, a reset): `game-design-incremental-specialist` and the
  `incremental-game-design` skill solve and prove them.
- **Feedback and feel** (R-16): `game-design-juicer` and the `game-juice` skill.
- **The build**: the build seats of the engine. Give them the finding, the test and the "keep" list.

## References

- `references/evidence.md`: the research findings, what each supports and does not support, the evidence grades, and
  the sources.
- `references/patterns.md`: the 6 patterns, how 6 known games use them, and how to select a pattern for a game.
- `references/review-checks.md`: what to grep and read, the journey targets, the 18 checks with the fix of each, and
  the 3 test variants.
- `references/report-template.md`: the report.
