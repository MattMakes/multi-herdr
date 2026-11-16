# The review checks

This page says what to read in a game directory, gives the journey targets, defines the 18 checks R-1 to R-18 with
the fix of each, and gives the 3 variants that test a proposed change.

## Contents

1. What to read in the source
2. The journey targets
3. The checks
4. The 3 test variants
5. What a play test must record

## 1. What to read in the source

Find the data first. In most incremental games the design is in tables, and the code only reads them.

| You look for | Where it usually is | Search words |
| --- | --- | --- |
| Costs and their growth | Data files, a cost function | `cost`, `price`, `growth`, `scale`, `pow(`, `base_cost`, `multiplier` |
| Production | A tick or process function | `per_second`, `rate`, `income`, `produce`, `tick`, `_process`, `delta` |
| A cap or a stage that limits | The production function | `min(`, `clamp(`, `capacity`, `limit`, `max_`, `storage`, `throughput` |
| Upgrades and their effects | Upgrade tables, an effect switch | `upgrade`, `effect`, `modifier`, `bonus`, `apply`, `match`, `type` |
| Unlock conditions | Tables, signal handlers | `unlock`, `requires`, `visible`, `revealed`, `threshold`, `milestone` |
| Choices that exclude | Slot and branch code | `slot`, `equip`, `loadout`, `exclusive`, `branch`, `respec`, `refund` |
| Randomness | Draw code | `rand`, `weight`, `pool`, `draft`, `offer`, `reroll`, `seed`, `chance` |
| Automation | Auto-buy and worker code | `auto`, `autobuy`, `worker`, `assign`, `policy`, `queue` |
| Resets | Reset and save code | `prestige`, `reset`, `ascend`, `rebirth`, `keep`, `persist` |
| Time pressure and offline | Save, load, timers | `offline`, `elapsed`, `last_seen`, `timer`, `expire`, `decay`, `streak`, `daily` |
| Feedback | Handlers of purchase and unlock | `tween`, `particles`, `play(`, `shake`, `animation`, `emit` |
| Play records | Analytics, logs | `log`, `event`, `journal`, `analytics`, `telemetry`, `stat` |

Rules:

1. Follow each value to its source. A constant in a table can be changed by a multiplier in code.
2. A formula in a comment or a document is the intent. The code is the truth.
3. A system that has data and no code path that reaches it is dead content. That is a finding (`REMOVE` or `REVEAL`).
4. Count things. "Most upgrades" is weak; "31 of 38 upgrades" is evidence.

## 2. The journey targets

These are hypotheses for a play test of a game that progresses during active play. They are not proved intervals
(`evidence.md`, F-2). State each as "about X" with its range. A game with a slower promised pace scales them and
keeps their order.

| Target range | Event | The player gets |
| --- | --- | --- |
| 0 to 5 seconds | Direct cause and effect | The input has a result that the player can see |
| 10 to 30 seconds | First affordable improvement | A purchase that changes the rate or the behaviour of production |
| 30 to 90 seconds | First automation | Something produces without the input of the player |
| 2 to 4 minutes | First meaningful trade-off | 2 or more options with different value, and not enough for all |
| 5 to 10 minutes | First new mechanic | A synergy, a choice of research, or a system that was hidden |
| 10 to 20 minutes | Visible transformation | The world changes, and a larger objective appears |
| 20 to 30 minutes | New strategic responsibility | A new constraint to manage: delivery, storage, or more than 1 place |

After 30 minutes there is no table. The rule is: each new source of decisions arrives before the old source is used
up. On the timeline, mark the time at which the player has seen each system, and the time of the next reveal. A gap
with no reveal, no new decision and no visible change is a finding of R-8 or R-11.

The question "one more what?" must change through the game:

| Early | Middle | Late |
| --- | --- | --- |
| "One more action and I can buy a unit." | "One more choice and my build is complete." | "One more place and I can connect these 2." |
| The player anticipates an action | The player anticipates a complete build | The player anticipates a change of the world |

## 3. The checks

Each check has: the question, how to measure it in the source, the fault, and the fix with its finding kind. The
pattern numbers P-1 to P-6 are in `patterns.md`. The finding numbers F-1 to F-8 are in `evidence.md`.

### R-1 Solved loop

- **Question:** is there 1 obvious shopping list?
- **Measure:** at 5 points of the timeline, list the affordable options. Compute the payback of each (cost divided by
  the gain in output per second). Count the points where 1 option has the best payback by a factor of 2 or more, or
  where only 1 option exists.
- **Fault:** at most points 1 option is best. The player does not decide; the player waits.
- **Fix:** `REBALANCE` to bring the paybacks of 2 or more options within a factor of 2 with different second-order
  results. If the options differ only in size, `REDO` them. If no change of numbers gives a decision, `ADD` with P-1
  or P-3. Do not add points, badges or levels: see the counter-evidence.

### R-2 Upgrade quality

- **Question:** what share of upgrades is class `number` only?
- **Measure:** the count of each upgrade class from the map.
- **Fault:** more than about 70% of upgrades are class `number`, or no upgrade of class `behaviour`, `interaction` or
  `constraint` occurs in the first 10 minutes. The band is a hypothesis.
- **Fix:** `REDO` the upgrades with the highest cost first: the player waits longest for them, so they must change
  the most. Keep `number` upgrades as the cheap steps between them.

### R-3 Constrained builds

- **Question:** must the player pass up 1 good option to take another?
- **Measure:** find slots, branches that exclude, scarce resources that never regenerate, and respec. Then ask: at
  the end of the content, does a player own everything?
- **Fault:** the player owns everything. No choice has an opportunity cost. Or a constraint exists with no respec, so
  an experiment is punished.
- **Fix:** `ADD` 1 set of slots (P-1), or `REDO` a tree into branches. Add a respec that is free or cheap. A hard
  lock on an early choice is a fault of its own (F-1: autonomy).

### R-4 Bottlenecks

- **Question:** does the limit of the output move between stages?
- **Measure:** the production formula. Look for `min(`, a capacity, a queue, a collection step, a delivery step.
  From the timeline, name the stage that limits at each of the 5 points.
- **Fault:** output is 1 product of multipliers, so each purchase always helps. Or stages exist and 1 stage limits
  for the whole game. Or the bottleneck moves and the game does not show which stage limits.
- **Fix:** `ADD` a stage (P-3) by a split of 1 existing multiplier. `REBALANCE` so that the limit moves 3 or more
  times in the content. `SURFACE` the stage that limits now: this is often the cheapest change with the largest
  effect.

### R-5 Combinations

- **Question:** do 3 or more pairs give an effect that is not a sum of percentages?
- **Measure:** for each upgrade, find if its effect reads the state of another upgrade or system. Count the pairs.
- **Fault:** the value of each upgrade is the same whatever else the player owns.
- **Fix:** `REDO` 3 upgrades into 3 pair effects (P-4). Each must be visible on screen, and the game must let the
  player discover it. A combination that only a wiki shows is `SURFACE`.

### R-6 Bounded randomness

- **Question:** are essentials certain, options variable, and can the player steer the draw?
- **Measure:** each call of the random generator. What does it decide? Can a bad result block progress? Does the
  player have a reroll, an exclusion, or a preference?
- **Fault, type A:** no variation. Each session is the same sequence. Not always a fault: a short finite game can be
  deterministic. It is a fault when the game expects to be replayed.
- **Fault, type B:** randomness on something that the player needs to operate, or no control at any stage.
- **Fix:** type A: `ADD` a draft on optional content only (P-2). Type B: `REDO` so that essentials are always for
  sale, and `ADD` control that the player earns (F-3).

### R-7 Automation as promotion

- **Question:** does each mastered chore go away, and does a larger decision replace it?
- **Measure:** list each repeated manual input. For each: the time when the player has mastered it (about the 10th
  repeat), the time when an automation for it opens, and what the player does with the freed attention.
- **Fault:** a chore has no automation. Or the automation arrives very late. Or the automation arrives and nothing
  replaces the chore, so the game becomes a wait.
- **Fix:** `AUTOMATE` the oldest chore first (P-5). Price it in the resource that the chore makes. Then `ADD` or
  `SURFACE` the decision of the next order: a rule, a policy, an allocation.

### R-8 The game changes

- **Question:** does the question "one more what?" change through the game?
- **Measure:** divide the timeline into thirds. For each third, write the main decision of the player and the main
  constraint. Count the reveals of class `reveal` and their times.
- **Fault:** the 3 thirds have the same decision with larger numbers. Or all changes are in the first third.
- **Fix:** `ADD` 1 phase change (P-6) near the middle: a new constraint and a new goal that use the old resources as
  tools. Or `REVEAL`: move an existing system later, to the point where the first loop is used up.

### R-9 Progressive reveal

- **Question:** does the player see a little of the next system before it opens?
- **Measure:** what the first screen shows. For each locked thing: is it hidden, hinted (a silhouette, a name, a
  cost), or fully shown?
- **Fault:** everything is visible at the start, so nothing is left to discover and the screen is too full. Or
  everything is hidden until it opens, so the player has no question to answer.
- **Fix:** `REVEAL`. Show 1 step ahead as a hint, and nothing more (F-7). A new screen is a reward: open it when the
  player has the means to use it.

### R-10 Three horizons

- **Question:** are a near, a medium and a far goal visible at the same time?
- **Measure:** at 5 points of the timeline, name the near goal (seconds), the medium goal (minutes) and the far goal
  (the session or the game), and where the screen shows each with its progress.
- **Fault:** a horizon is missing or not shown. The most common: no far goal, or a far goal with no progress display.
- **Fix:** `SURFACE` the goal with a progress display (F-2). `ADD` a goal only when the game truly has none.

### R-11 Dead time

- **Question:** how long can the player make no useful decision?
- **Measure:** from the timeline, each span where nothing is affordable, nothing can be configured, and the only
  input is to wait or to repeat 1 action. Give the start, the length and the cause (a cost wall, a missing source, a
  late unlock).
- **Fault:** a span longer than about 60 to 90 seconds in the first 30 minutes of an active game, or a span that
  grows at each step with nothing new to do. The band is a hypothesis.
- **Fix:** `REBALANCE` the wall. Or `ADD` or `REVEAL` a decision of configuration that fills the span. Manual
  clicking must never be the only action for minutes.

### R-12 First minutes

- **Question:** do the first events occur in the journey ranges?
- **Measure:** the 7 events of section 2 against the timeline.
- **Fault:** an event is far outside its range, or does not exist. An event that is too early is also a fault: a
  trade-off before the player understands the resource is noise.
- **Fix:** `REBALANCE` the first costs. `REVEAL` in a different order. The first 5 minutes are the cheapest part of a
  game to change and the part that most players see.

### R-13 Reward variety

- **Question:** are the rewards of more than 1 type?
- **Measure:** put each reward of the first 30 minutes in a type: a number, a capability, a visible change of the
  world, a piece of story, a new strategic option. Count each type.
- **Fault:** almost all rewards are numbers (F-4).
- **Fix:** `REDO` the rewards of milestones into the missing types. A milestone that gives only a multiplier is the
  first candidate.

### R-14 Surprise and acceleration

- **Question:** do earned breakthroughs exceed what the player expected?
- **Measure:** the ratio of output after and before each large purchase. Look for steps much larger than the usual
  step. Look at what the screen shows: totals only, or also the rate and its change.
- **Fault:** each step has the same ratio, so the curve is smooth and nothing stands out. Or a large step exists and
  the screen does not show it.
- **Fix:** `REBALANCE` to put a few large steps (about 5 to 10 times the usual step) on upgrades that the player
  works toward. `SURFACE` the rate and the effect of multipliers that interact (F-6, F-8).

### R-15 Visible world

- **Question:** can the player see progress with the counters hidden?
- **Measure:** hide the number labels in your mind. What on the screen is different between minute 1 and minute 30?
- **Fault:** nothing. Progress exists only as text.
- **Fix:** `SURFACE` or `ADD` a persistent visible record of progress: units that the player can see at work, a map
  or a scene that changes and stays changed.

### R-16 Feedback proportion

- **Question:** do small gains stay quiet and large changes get the large effects?
- **Measure:** the feedback hooks of an ordinary tick, an ordinary purchase, an unlock, and a phase change.
- **Fault:** no feedback at all, or the same large effect on each event (F-5).
- **Fix:** 1 finding that names the events without proportion, then a hand-off to the juice seat. If you did not read
  the hooks, the status is `not audited`.

### R-17 Coercion

- **Question:** does anything hold the player by pressure, loss or deceit?
- **Measure:** look for timers that punish absence, rewards that expire, streaks, decay of progress, random loss,
  near-miss displays, false scarcity, and progress timed to demand attention.
- **Fault:** each one found. An offline gain that rewards a return is not coercion. A penalty for a pause is.
- **Fix:** `REMOVE`, and it ranks first. Say what the system was for, and give the replacement from the left column
  of the table in `evidence.md` section 5.

### R-18 Instrumentation

- **Question:** can the game record what a play test must measure?
- **Measure:** find an event log. Compare it with section 5.
- **Fault:** no log, or a log without the events that your findings need.
- **Fix:** `MEASURE`: an event journal with the events of section 5. Cost `S` or `M`. It makes each other finding
  testable, so it is often in "do first".

## 4. The 3 test variants

A richer variant can win only because it gives more power. To test the idea and not the amount, compare 3 variants
with about the same total value of upgrades and about the same length of content:

| Variant | What it has | What it tests |
| --- | --- | --- |
| A, linear | The gain as plain number upgrades. No synergy, no draft | The baseline |
| B, constrained | The limited slots and the combinations, with a deterministic selection | Strategic choice with no randomness |
| C, drafted | Variant B, plus bounded offers and rerolls | If adaptive build craft adds value |

For each `ADD` and each large `REDO`, say which variant your change is, and describe variant A of it in 1 sentence.
If variant A gives the same result in a simulation, the idea added numbers and no decision: withdraw the finding.

## 5. What a play test must record

Behaviour:

- The time to the first meaningful upgrade and to the first automation.
- The time in which the player can make no useful decision.
- The distribution of upgrade choices and research choices. If all players choose the same, the choice is solved.
- How often a bottleneck occurs, and how long it lasts.
- Where and when players stop by their own choice.
- How often players respec or reroll.

Experience, from the player in their own words:

- Enjoyment and the control that they perceive.
- If they can state a strategy.
- If they see the visual effect of an upgrade.
- Which discovery was the most satisfying.
- If they would come back by their own choice.
- If they ended satisfied or frustrated.

Read voluntary engagement and reported satisfaction together. Time played alone can come from friction.

Acceptance tests to use in findings:

| The change claims | The test |
| --- | --- |
| Each action has visible feedback | A new player says what their input changed, with no instructions |
| The next goal is visible | A player can state what they work toward |
| Automation removes a chore | Players stop the old input and put their attention elsewhere |
| A true bottleneck exists | More of the wrong resource does not raise the useful output |
| Choices differ in strategic value | Different players select different upgrades and give reasons |
| Slots constrain the build | To equip 1 attractive option, the player passes up another |
| Offers are bounded and can be steered | A player makes progress with a bad offer, and later earns control |
| Synergies exist | A pair gives an effect that is not a sum of percentages |
| Progress shows in the world | A person recognizes progress with most of the interface hidden |
| Reveal is progressive | Early players are not overloaded, and later players regularly find new capabilities |
