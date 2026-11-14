# Indicators: how to read the workbook and judge a design

The Dashboard sheet shows each indicator with a low band, a high band, and a status. **The
bands are conventions, not measurements.** They come from the shape of the reference game and
from common practice. No retention data stands behind them. Change them for the game
(`bands` in the model): a 10-minute mobile session and a 2-hour desktop session need
different values. When the project has telemetry, replace the simulation times with real
times and set the bands from real retention.

## What the simulation is

The script plays the model with a greedy buyer. At each step the buyer picks the purchase
with the best gain for the wait, and it takes each milestone when its conditions are true.

The buyer is not a perfect optimiser and its times are not a lower bound. It looks one
purchase ahead, so a player with a plan can be faster. Most real players are slower: they
miss options, leave the game, and buy what looks good. Read the shape of the run (where the
beats crowd, where they stop) as the result. Read the times as an estimate. If the project
has real play times, state the ratio of real to simulated time and use it.

**Buyer profiles.** The moment of a reset is a player decision, and it moves the lulls. The
script runs the full game under three reset rules:

| Profile | Resets when |
| --- | --- |
| `eager` | The prestige gain for each minute falls below 95% of its best value in the run |
| `patient` | The same rule at 75% |
| `doubler` | The gain is at or above the lifetime total of the prestige currency |

The primary profile (default `eager`) fills the Timeline and the Dashboard. The Profiles
sheet compares all of them. A lull is **robust** when a lull overlaps it in each profile.
Other lulls "depend on reset policy": they describe one play style. If the gain for each
minute keeps rising, `eager` and `patient` never reset. The script warns about this: use
`--profile doubler` for such a game. A game with no reset, or with a fixed run length, runs
one profile.

The simulation does not model: skill, luck (model a drop as its mean rate), an offline cap,
a second reset layer, or boredom. It models time away only when the model sets
`away_minutes`.

## Pacing indicators

**Beat.** An event with an excitement weight at or above the beat weight (default 3). Default
weights: system unlock 5, first prestige 5, feature 3, new ladder step 2 (a model can set it
higher), first purchase of a line 2, repeat purchase 1. A later prestige has weight 3 only if
its run reached a new beat. Otherwise it has weight 1: a reset that shows nothing new is not
a beat.

**Moment and stacked beats.** Beats within 30 seconds of each other count as one moment. The
other beats in the moment are "stacked": the player gets them together and feels one of
them. Many stacked beats mean that the game spends its content too fast at that point.
Spread them out.

**Gap and gap ratio.** The gap is the time from one moment to the next. The gap ratio is the
gap divided by the time played before the gap. Incremental games stretch: a 2-hour wait at
hour 20 is normal, and at minute 30 it is not. The ratio removes the stretch. A steady median
ratio means the game slows down smoothly. A ratio that climbs means the game runs out of new
things.

**Lull.** A gap of 10 minutes or more that (a) is longer than the full game before it (ratio
above 1), or (b) is longer than 2 sessions and longer than 0.25 x the time played. Rule (a)
decides early in the game and rule (b) later. The time from the last beat to the end of the
run (the tail gap) counts as a gap. All these thresholds are on the Params sheet.

**Dead zone.** Lulls with less than 2 sessions between them are one dead zone. A dead zone
with a few beats inside is the same problem as one long lull. Fix the zone, not each lull.

**Probable cause (Lulls sheet).** The script tests these rules in order and gives the first
that matches. It is a lead. Confirm it on the Timeline.

| Cause | Rule |
| --- | --- |
| End of content | The buyer stalled, or 60% or more of the capped lines are at the cap and no system opens later |
| Reset that does not pay | Two or more resets in the gap |
| Wall | The next spine step needs x1.2 or more of the current value, and income grew less than x2 in the gap |
| Empty shop | Fewer than 2 options within 5 minutes, on average |
| Flat menu | Purchases happened in the gap, and none is a beat |

| Indicator | Start band | Low means | High means |
| --- | --- | --- | --- |
| Minutes to first purchase | 0 to 1 | - | The loop takes too long to show itself |
| Minutes to first new system | 2 to 15 | No time to learn the core loop | The first session ends with one screen only |
| Minutes to first prestige | 20 to 120 | Reset arrives before the player values the run | The meta loop is hidden behind hours of play |
| Beats for each hour | 3 to 30 | Empty stretches | Unlocks arrive faster than a player can absorb them |
| Beats in the first hour | 3 to 30 | A slow start | The start spends too much content |
| Lulls | 0 | - | Each one needs a diagnosis |
| Longest gap | up to 2 sessions, or 0.25 x the run | - | One or more sessions pass with no beat |
| Median gap ratio | 0.05 to 0.6 | Beats are crowded together | Waits grow faster than the game |
| Share of sessions with no beat | 0 to 25% | - | No reason to come back |

**Beats behind target (Target sheet).** The ideal curve puts the first beat at 0.5 minutes
and makes each gap x1.12 longer than the one before. This gives about 24 beats in the first hour,
33 in 3 hours, and 44 in 10. It is one reasonable shape, not a law: set the two numbers on Params so that
the curve ends at the number of beats that the game has. Then read where the real curve
leaves it. Above the curve early and below it late is the common fault: a crowded first hour
and an empty late game.

## Decision indicators

**Options within 5 minutes.** The number of purchases that the buyer can afford in 5 minutes
or less, weighted by time through the run. Below 2: no real choice, the game is a queue. Above 7:
choice overload; consider automation or grouping.

**Share of time with no option within 5 minutes.** Pure waiting. Above 20% is a problem
unless a second clock or a minigame fills that time.

## Economy indicators

**Payback (Curves and Purchases sheets).** For one purchase: its cost divided by the income
that it adds each second. The sheets give the payback of the first level and of the last
level that the buyer bought.
- Payback that grows with the level: the line slows down and needs other systems to multiply
  it. This is the normal state of a core generator.
- Payback that stays flat or falls: the line never slows by itself. It will dominate, or the
  game will explode. This is correct only for a line with a low maximum level.

**Cost growth.** The reference game uses x1.05 to x1.5 for each level, by system: low values
for lines with hundreds of levels, high values for lines with a small cap
(reference-game-math.md has the table). A value far outside the range of similar lines is a
question to ask, not an error.

**Share of spending (Purchases sheet).** One line that takes most of a currency means that
the other lines are not real options.

**Levels to 10x cost.** How many levels of a line multiply its cost by 10. It gives a feel
for how long a line stays useful.

**Walls sheet.** Each step of the spine with what it needs, when the buyer reached it, and
the wait from the step before. A wait that jumps by x3 or more between two steps is a wall.

**Wall growth against reward growth.** On a milestone ladder, compare the growth of the
requirement with the growth of the reward. The reference game holds requirement growth at
x2.8 for each step and reward growth near x1.05 for 60 steps, then raises both on purpose to
make a wall. A requirement that grows much faster than total power makes a wall. Decide where
the walls are. Do not let them appear by accident.

**Currencies with no source, or with no sink.** A currency with no source cannot be earned in
the model. A currency with no sink piles up and stops mattering. (A stat that is only
compared with requirements, such as damage, needs no sink: mark it `"stat": true`.)

**Purchases never bought, milestones never reached.** The buyer never wanted them or never
could reach them in the simulated time. Either the content is too far away, or its value is
too low, or a requirement is impossible.

**Prestige run lengths (Prestige sheet).** The minutes of each run in order, with the gain,
the highest spine step, and the new beats of the run. Healthy: the first runs get
shorter (the multiplier works), then the lengths settle or grow slowly as each run goes
deeper. Unhealthy: each run is longer than the one before from the start (the reset does not
pay), or all runs are the same (nothing new opens in later runs). "Prestiges after a run
with nothing new" on the Dashboard counts the second case: a high share is a treadmill. Runs
at exactly `min_run_seconds` show your setting, not a result.

**Ledger sheet.** Where each currency comes from and where it goes, with shares.

**Sensitivity sheet.** The script changes one number at a time by x0.95 and x1.05 (ladder
growth, prestige exponent, the cost growth of each line) and runs again. A number that moves
the longest gap is a lever. A number that moves nothing is safe to set for theme or feel. For
an exponential cost the change is half of (growth - 1), and the sheet shows the values used.
It states how many parameters it tested in its time limit.

**Stall.** The buyer had nothing left to buy or reach. The content ends at that time. Compare
it with the target length of the game.

## Structure indicators

**Systems.** Scope. Each system needs content, a screen, balance, and feedback.

**Links for each system.** Declared links plus links implied by costs, effects, and
requirements. Below 1.5: the systems sit side by side. The reference game has many tens of
links for each system; a small game does not need that, but each system needs one link in
and one link out as a minimum.

**Swimlane sheet.** A heat map of systems against time. Read it by rows: a system whose row
goes empty after the first hour is finished content. Read it by columns: a column with one
active row is a point where the game is one system only.

**Orphan, source-only, sink-only (Systems sheet).** An orphan has no link. A source-only
system gives and never receives: it has no reason to grow with the game. A sink-only system
receives and never gives: it is a side game.

**Unlock curve against the reference (Reference sheet).** The chart shows systems open
against progress for the game and for the reference game. The reference curve is steep at the
start, steady in the first half, then flatter while cross-system chains carry the game. A
curve that is flat at the start means a slow first hour. A curve that reaches its maximum at
20% of the game means nothing new for the remaining 80%: check that chains, tiers, or cap
raisers fill that time.

## From indicators to a verdict

Give a verdict for each of the four review areas. Name the evidence for each one.

| Area | Evidence |
| --- | --- |
| Balance | Walls, Curves, Purchases (payback, share of spending), Prestige, Ledger, Sensitivity |
| Scope | Systems count, Swimlane, purchases never bought, stall or end of content against the target length |
| Juice | Beats without feedback in the code (see godot-extraction.md), beat weights against what the game shows |
| Feel | Robust lulls, dead zones, stacked beats, sessions with no beat, options within 5 minutes, the Target curve |

Success targets for a build: write them as indicator values, for example "first new system
before minute 10, no lull before hour 3, each session in the first 5 hours has a beat, each
system has a link in and a link out". The team can then rerun the workbook after each balance
change and see the status cells. Use `--compare` with the analysis JSON of the run before
the change: the Compare sheet shows each indicator before and after.
