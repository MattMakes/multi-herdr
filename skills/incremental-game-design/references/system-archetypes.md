# System archetypes for ideation

`references/systems-catalog.md` is the authority for each system and each formula. This file maps
a symptom to catalog ids. It gives no formula. Take the formulas, the parameter ranges and `r`
from the variant table of the card.

Start from the problem, not from the system. Each idea must name the lull or weakness that it
fixes, the value that it pays to the core loop, and the place where it attaches. A new idea comes
from the catalog, passes the compatibility table of the catalog, and states the decision that it
adds. A system that only multiplies is labelled "multiplier only". Propose only systems that the
designer selected in the interview (`references/interview.md`), or ask about a new one with the
part C question form.

## How to choose

| Symptom in the workbook | Catalog ids to consider |
| --- | --- |
| Long gap with nothing to buy (income flat, few options) | S09; S23; S06 |
| Long gap, many options, none exciting | S03 rewards for each step; S19; S17 |
| Wall on the main axis | S04; S13; a chain (no card) |
| Early game ends before the first new system | A front-loaded unlock (no card, see `PO11`); S24 |
| Systems are orphans (no links) | A shared sink (no card); S22; S13 |
| Finished system that nobody opens again | S19 (a higher tier); S13 |
| Sessions with no beat late in the game | S25; S09 (`S09.daily_cycle`); S09 (`S09.rotation`) |
| A currency with no sink | A shared sink (no card); S19 (a higher tier); an exchange (no card) |
| Too many options (choice overload) | S24; a loadout or respec (no card) |
| Buying more of the same is always right (no trade-off between lines) | S30 (`S30.need_capacity`); S21 |
| Active play has no pulse | S10; S07; S11 |
| No reason to return after a break (play pattern `check_in` only: a `continuous` game has no idle benefit by design) | S08; S06; S09 |
| The rare currency has nothing to decide | S26; a shared sink (no card); S19 (a higher tier) |
| Players do not know what to try next | S20; a readiness list (no card); S20 (`S20.completion_point`) |

"No card" marks an archetype that the catalog does not hold as a system. It is a link, a sink,
a pacing rule or a screen. The skill can propose it only as a change to selected systems, and it
labels it "not a catalog system".

## The archetypes

Each entry gives: what it is, the reference pattern, what it fixes, what it must pay to the
core loop, scope, juice, and the main risk. Scope is S (days), M (one to two weeks), or L (a
month or more) for a small team with the core game already built.

### Reset layer (prestige): S04
Reset a part of the game for a currency that buys permanent power.
- Reference: pattern 12. Only "this run" systems reset. Tiers of permanent upgrades open on
  the main axis.
- Fixes: a wall on the main axis; a flat income curve.
- Pays: a multiplier that makes the next run visibly faster. Target: the second run reaches
  the old wall in a third of the time or less.
- Scope: M. Juice: a large one-time moment (screen change, sound, count-up of the reward).
- Risk: the first reset feels like a loss. Show the gain before the player commits.

### Second reset layer: S16
A deeper reset that also resets the first layer and pays a rarer currency.
- Fixes: prestige runs that all feel the same (run time stops shrinking).
- Pays: changes to rules, not only larger multipliers (start with automation, keep a system).
- Scope: M to L. Risk: it must protect the systems that the player built last.

### Minigame with export: S23
A small game with its own currency and tree. Its rewards are permanent buffs to the core
loop.
- Reference: pattern 7.
- Fixes: lulls where the core loop only waits. It gives active play a purpose.
- Pays: a multiplier or a cap that no other system gives. If the core loop can get the same
  value more cheaply, players skip the minigame.
- Scope: L. Juice: its own feel (different input, different tempo).
- Risk: it becomes a second game that the core loop does not need. Give it two or more links
  out and one or more links in.

### Second clock: S09
A system that moves on real time, on actions, or on ticks, and not on core income.
- Reference: pattern 7 (Archaeology ignores game speed; star targets move each 30 minutes).
- Fixes: sessions with no beat when core costs are far away. There is always a short timer
  about to finish.
- Pays: a small, regular reward that scales with core progress.
- Scope: S to M. Risk: it feels like a chore if it punishes absence.

### Milestone ladder with rewards: S03
A numbered sequence on the main axis. Each step pays now and opens something later.
- Reference: patterns 1 and 2.
- Fixes: a main axis with steps that give nothing.
- Pays: a reward on each step, a feature on each fifth or tenth step.
- Scope: S if the axis exists. Juice: a banner and a reward animation for each step.

### Collection with tiers and set bonuses: S19
Items that drop from play. Each item gives a small bonus. A full set gives a large one. A
later tier upgrades the same items.
- Reference: patterns 6 and 8.
- Fixes: many options but none exciting; a currency with no sink (tier costs).
- Pays: multipliers tied to the place where the item drops.
- Scope: M. Juice: the drop moment, the first-time reveal, the set completion.
- Risk: pure chance with no pity timer makes dry spells. Bound the wait.

### Action-fed collection: S22
Companions, tools, or badges that level from actions in other systems.
- Reference: pattern 8. A total level gates later members.
- Fixes: orphan systems; players who use only one system.
- Pays: a bonus to the system that feeds it, and a gate on the total.
- Scope: M. Risk: hidden rules. Show what each member levels from.

### Cap raiser: S13
A reward that raises a maximum in an older system.
- Reference: pattern 5.
- Fixes: finished systems; walls.
- Pays: new levels in something the player already understands.
- Scope: S. It needs no new screens. Risk: none large. This is the cheapest idea in the list.

### Tier on existing content: S19
A second (third) quality tier for items that exist, with a new cost currency and a set rule.
- Reference: pattern 6.
- Fixes: a need for late content with a small budget; a currency with no sink.
- Scope: S to M. Juice: a visible change of the item (colour, frame, effect).

### Cross-system chain: no card
A prize that needs steps in two or three systems in order.
- Reference: pattern 3.
- Fixes: the span after level gates run out.
- Pays: a large, visible prize (a new slot, a new unit, a new system).
- Scope: S to M per chain if the systems exist. Risk: the player cannot see the chain. Show
  the steps.

### Shared sink (multi-currency goal): no card
A large goal that costs output from three or more systems.
- Reference: pattern 9.
- Fixes: orphan systems; a currency with no sink; one dominant system.
- Scope: S. Risk: the slowest currency sets the pace. Check it in the simulation.

### World or zone with a removable debuff: no card
A new area that is hard on entry and that play makes normal.
- Reference: pattern 11.
- Fixes: a late game with no new place to go.
- Scope: L (content). Risk: players enter early and feel punished. Show a readiness list.

### Task list with a tier shop: S20
A list of named tasks, each in a different system. Each task pays a coin. The coins buy
permanent rewards from a short shop, in the order that the player chooses.
- Reference: pattern 25. 100 tasks in three tiers; a tier opens when the tier before is done.
- Fixes: players who do not know what to try next; orphan systems; flat menus.
- Pays: permanent rewards of the player's choice, and a count that can gate later content.
- Scope: S to M. It needs counters for actions, a list screen, and a small shop.
- Risk: tasks that only say "wait longer". Each task must point at an action.

### Rule-change runs: S17
Runs with a changed rule (no upgrades of one type, a time limit) that pay a permanent
reward. This is a genre convention. The reference game does not use it.
- Fixes: reset runs that feel the same; a need for goals with skill in them.
- Scope: M. Risk: rewards that are required for progress turn a choice into a chore.

### Random visitor: S10
Something appears on the main screen on a timer. A tap gives a small reward from a weighted
table. A better reward costs a little premium currency.
- Reference: pattern 21.
- Fixes: active play with no pulse; sessions with no beat; no small premium sink.
- Pays: short buffs, small currency amounts, time for a minigame.
- Scope: S. Juice: the arrival, the tap, the reveal. Risk: it becomes a chore with no bank.

### Bank with a cap: S08
A timed reward stacks while the player is away, up to a cap that upgrades raise.
- Reference: pattern 19.
- Fixes: no reason to return; a return with nothing to do; no long-term use for a timer.
- Pays: the timed reward, in a burst on return. The cap raise is a reward from other systems.
- Scope: S.

### Self-funding premium purchase: S26
A purchase with the rare currency that raises the income of the rare currency, and that
turns positive only after investment in another system.
- Reference: pattern 17.
- Fixes: a rare currency with nothing to decide; a late system that nobody invests in.
- Scope: S. Risk: if it pays back at once, it is not a choice. Check the break-even point.

### Consumable buffs: S11
Items that give a timed buff. They drop from several systems, and uptime is the real stat.
- Reference: pattern 22.
- Fixes: a need for frequent small rewards that do not inflate permanent power.
- Scope: S to M. Risk: inventory work. Keep use to one tap, or automate it later.

### Boss attempt on a cooldown: S07
A step on the spine is a short timed fight. Damage carries over. A threshold stat gates the
start.
- Reference: pattern 26.
- Fixes: a spine that only ticks over; no moment where the player tests the full build.
- Scope: M. Juice: the attempt is a natural peak. Risk: a cooldown with nothing to do in it.

### Completion score: S20
One number that counts each permanent unlock in each system.
- Reference: pattern 14.
- Fixes: a late game with no visible total; players who do not see how much is left.
- Scope: S.

### Automation reward: S24
The game takes over an action that the player did by hand.
- Reference: pattern 13.
- Fixes: fatigue; choice overload; a weak reward at a mid-game milestone.
- Scope: S. Juice: the first time the game acts by itself is a strong moment. Show it.

### Offline bank: S06
Progress while away, with a cap, paid on return.
- Fixes: sessions that open with nothing to do; a need for a return reason.
- Scope: S to M. Risk: an uncapped bank removes the reason to play actively.

### Rotating event: S25
A timed layer with its own short ladder and a reward that carries into the main game.
- Reference: pattern 24. One month in each two, with its own currencies and prestige. Main
  power does not enter. Rewards scale with the highest world.
- Fixes: late-game sessions with no beat.
- Scope: L, and it needs live operation. Do this last.

### Upkeep: S30
Each producer unit adds a need (food, rest, power); care capacity meets it; output is times `u` <= 1.
- Reference: Factorio power (machines slow down in proportion to the power, sourced); Evolve morale and
  entertainers (sourced). `references/systems-catalog.md` S30 gives the URLs.
- Fixes: a run where 1 more producer is always the best purchase; it adds the more-producers-or-more-care decision.
- Pays to the core loop: a factor on the main income; care lines are S02 entries with the role `care`.
- Scope: M. Juice: a unit that stops to eat or sleep, and a need icon. Risk: a stable best split ends the decision,
  and too little care gives a stall (`references/balance-math.md` section 3b).

### Readiness list: no card
A screen that shows the recommended state of other systems before a large step.
- Reference: pattern 14.
- Fixes: walls that feel unfair; players who enter new content too early.
- Scope: S. It adds no content. It makes the content that exists visible.

## The idea record

Write each idea with these fields. They are the columns of the Ideas sheet.

| Field | Content |
| --- | --- |
| idea | A name in the theme of the reviewed game |
| archetype | A catalog id and its variant id, or "not a catalog system" |
| problem | The lull or weakness, with its time or level from the workbook |
| feeds | The core-loop value that it pays (which currency, multiplier, or cap) |
| fed_by | What it needs from the systems that exist |
| unlock | The point on the main axis or the chain where it opens |
| persists | Does it survive each reset layer |
| scope | S, M, or L, with the main cost driver |
| juice | The one moment that the player will remember |
| risk | The most probable way it fails |

Three good ideas are better than ten. Rank them by (size of the problem fixed) / scope. Then
add the best idea to a copy of the model and run the simulation again: the lull must shrink.
An idea that does not move the Timeline is decoration. Then run the proof sheet
(`scripts/pw_proof.py --strict-ranges`) on the changed model: an idea that fails an obligation
makes the design not ready, also when the lull shrinks.

The model can express: a new income source, new purchase lines, a multiplier, a milestone, a
ladder, a cap raise, a cost in several currencies, and a run of fixed length (see
model-schema.md). It cannot express chance, a second reset layer, or a rule change. For an
idea of that kind, do the arithmetic by hand (what it adds to the gating stat at the lull,
against what the wall needs) and label the result "estimated, not simulated".
