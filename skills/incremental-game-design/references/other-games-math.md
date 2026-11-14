# Math of other incremental games

The reference game is one data point. This file gives the formulas that other well-known
incremental games use for the same decisions, so that a verdict does not depend on one game.
Use it when you choose a formula for a new design, when you judge a formula in a build, and
when you need a second source for a rule.

**How to use it.** Find the decision (cost growth, prestige gain, softcap, wall, stacking,
reset timing). Read what several games do. Then choose for the reviewed game and say why.
The same rule as for the reference game applies: use the shape, not the system.

**What is in this file.** Only formulas that a person read on the source page or in public
source code on 2026-11-13. A formula marked *(rebuilt)* lost its exponents in the page text
and a reader put them back: check it before you quote it. Formulas known only from search
snippets are not here. The sources are at the end.

## Contents

1. Cost growth for each purchase
2. Prestige gain: the shape of the reset reward
3. When to reset
4. Stacking: add or multiply
5. Softcaps and caps
6. Walls
7. Cost and value as one exponent
8. Other patterns
9. Sources

## 1. Cost growth for each purchase

Almost every game uses `cost = base x ratio^owned`. The ratio is the design decision.

| Game | Ratio | Note |
| --- | --- | --- |
| Cookie Clicker | 1.15 for every building | One ratio for the full game. Discounts multiply and stop at about -35% |
| AdVenture Capitalist | 1.07 (first business) | Each business has its own ratio between 1.07 and 1.15 (Pecorella; King) |
| Universal Paperclips | 1.1 and 1.07 in phase 1 | Phase 2 changes to polynomial costs: `(n+1)^2.25 x 1e6` |
| Idle Wizard | 1.16 to 1.19, one for each source tier | Later tiers fall behind unless boosted |
| Trimps | 1.2 equipment, 1.24 huts, 1.3 perks (prestige shop) | A few perks get 1.5, 2 or 5 to limit them |
| Kittens Game | One ratio for each building: 1.12 and 1.15 for production, 2.5 for housing | Reset rewards lower the ratios |
| Evolve | "Cost creep" for each building, for example 1.3 | Reductions stack, with a floor of 1.005 |
| Antimatter Dimensions | x1e3 to x1e15 for each 10 bought, by tier | Past 1.8e308 the ratio itself grows for each purchase |
| Synergism | 1.25^g with g = 1 to 5 by tier | Past a threshold the exponent turns quadratic |
| Exponential Idle | `initialCost x 2^(progress x level)` for all upgrades | One cost family. Pacing comes from the value side |

Rules of thumb with a source:

- "A Multiplier between 1.07 and 1.15" for generators (King, Tuts+). Clicker Heroes sits at
  the low end and Cookie Clicker at the high end.
- A ratio that is itself a reward is strong. Kittens Game spends reset currency on -1% to
  -2.25% of every ratio. A lower base pays more the more the player owns.
- Put a floor under a ratio that can be reduced (Evolve: 1.005). A flat curve is free.
- Bulk buy of n from k owned: `base x ratio^k x (ratio^n - 1) / (ratio - 1)`. Max affordable
  with currency c: `floor(log_ratio(c x (ratio - 1) / (base x ratio^k) + 1))` (Pecorella I).
- Not all costs need to be exponential. Melvor Idle bank slots use a bounded curve for a
  utility purchase. Legends of IdleOn uses power `Rank^1.3` against cost `Rank^1.5`.

## 2. Prestige gain: the shape of the reset reward

| Shape | Game and formula | Run earnings needed to double the reward |
| --- | --- | --- |
| Square root | AdVenture Capitalist: `150 x sqrt(lifetime / 1e15)` | 4x |
| Square root (triangular cost) | Realm Grinder: coins needed `5e11 x G x (G + 1)` for G gems. Idle Wizard: `sqrt(mana / 5e11)`, the same curve | About 4x |
| Cube root | Cookie Clicker: `(lifetime / 1e12)^(1/3)` | 8x |
| Small power | Egg, Inc.: gain scales with `earnings^0.21` (community tool code; an older article gives 0.14) | About 27x (128x at 0.14) |
| Small power, by layer | Synergism: powers 0.5, then 0.03, then 0.01 for layers 1 to 3 | Each layer starts tiny |
| Power of the log | Antimatter Dimensions: `floor(mult x 10^(log10(antimatter) / 308 - 0.75))` *(rebuilt)* | About a 308th root for each layer |
| Logarithm | Clicker Heroes, layer 3: `floor(5 x log10(total souls))` | +5 for each 10x |
| Linear above a threshold | Kittens Game: 1 paragon for each kitten above 70 | Sets the run length (section 3) |
| Sub-exponential with steps | Trimps: `round(1.35(z-19) + 1.23^sqrt(1.35(z-19)))` x a product of multipliers. One multiplier steps 1, 5, 10, 30 at story points | Each step is a visible jump |
| Linear plus log | Evolve: population / divisor + a log of knowledge spent, then a softcap | Later resets change the constants, not the formula |

What the reward does also matters:

- **Additive, small for each point** is the common choice: +1% (Cookie Clicker), +2%
  (AdVenture Capitalist), +3% (Idle Wizard), +10% (Clicker Heroes souls). The count of
  points is then the compounding variable.
- **Two currencies with different shapes.** Egg, Inc.: `bonus % = SE x (10 + research) x
  (1 + (5 + research) / 100)^PE`. One currency adds, the rare one compounds, and late in the
  game the rare one is the main lever.
- **Each deeper layer is a slower function of the layer below** (root, then smaller root,
  then log). The top layer then moves in steady steps while the lower layers explode.

## 3. When to reset

- **Rule of thumb (Pecorella, GDC Europe 2016):** reset when the gain is +50% to +200% of the
  prestige currency. AdVenture Capitalist guides say the same: reset when the gain is at
  least what the player holds.
- **Derived rule (Exponential Idle guides):** maximise the growth ratio for each second,
  `((b + db) / b)^(1 / t)`, and reset when it starts to fall. Ratios compound, so the guide
  takes the power `1 / t` and does not divide by t.
- **Closed form (Exponential Idle, "optimal publication multiplier"):** if income follows
  `d(rho)/dt = K x rho^x x t^y x rho_pub^z`, the long-run best reset multiplier is
  `e^(z(y+1)/(1-x))`. For a balanced system that does not depend much on time, it is a little
  below e (about 2.7x). Worked values in the guides go from 2.5x to above 1000x, by theory.
- **A threshold sets the run length (Kittens Game).** With no reward for the first 70
  kittens, run efficiency is `1 - 70 / K`. The designer sets the ideal run length with one
  number.
- **Exponent method (Idle Wizard).** Measure growth as `profit ~ t^x`. With a square-root
  prestige, the reward grows as `t^(x/2)`, and the best reset point follows from x.
- **Greedy buying is close to optimal (Demaine and others).** Buy as soon as affordable. Buy
  an item when `M / y >= 1 + G / x` (M target, y cost, x rate gain, G current rate). Greedy by
  efficiency is within `1 + O(1 / log M)` of the best plan. The exact problem is NP-hard.
- **Payback rule (Cookie Clicker spreadsheet):** choose the purchase with the lowest
  `cost / income + cost / income gain`.

The three buyer profiles of the workbook script (`eager`, `patient`, `doubler`) are inside
this range: the first two follow the "gain for each minute falls" rule, and the third is the
+100% rule.

## 4. Stacking: add or multiply

The same rule appears in most of these games: **add inside a group, multiply between
groups.**

| Game | The rule as the source states it |
| --- | --- |
| Cookie Clicker | Building output is summed, then "multiplied by several multipliers, one after the other". Some effects add inside a stage |
| Evolve | Production structures add: `quantity x bonus`. Discounts multiply: `base x discount^quantity`, so a cost never reaches zero |
| Realm Grinder | A published order: `(A_base x M_base x M_temp + A_temp) x M_illus + A_illus`. Tooltips say "Multiplicatively increase" for the M terms |
| Melvor Idle | Township: `base x (1 + sum of group 1 / 100) x (1 + sum of group 2 / 100) x efficiency` |
| NGU Idle | Multipliers of one feature "stack additively to each other". Features multiply |
| Synergism | One upgrade family is "other-multiplicative but self-additive" |
| Universal Paperclips | The workhorse boosts add (+25%, +50%, +75%). Rare projects multiply (x1.5, x2, x5, x10) |
| AdVenture Capitalist | Shop, badge and ad multipliers add (x12 and x4 give x16). Milestones and angels multiply |
| Idle Slayer | Bonuses of one kind add (100% + 250% + 100% + 100%). Soul sources multiply (10.125x) |
| Legends of IdleOn | Damage sources all multiply. This is why its numbers explode: a caution |

Design consequences:

- In a sum, only the largest term matters in the end. In a product, exponents add
  (Exponential Idle). A system that only adds will stop mattering unless its group grows.
- A new source in an old group is a small reward. A new group is a large one. Decide which
  each new system is.
- Write the rule down for the player. Realm Grinder does it in tooltip wording.

## 5. Softcaps and caps

| Type | Example |
| --- | --- |
| Triangular root | Kittens Game uses `(sqrt(1 + 8x / k) - 1) / 2` for karma, worship and burned paragon. Double the input gives about 1.41x |
| Asymptote | Kittens Game paragon bonus: 100 points give 100%, 200 give 175%, 10,000 give 199.7%. The limit is 200%, and the next layer is the only way past it |
| Cap on gain for each reset | Evolve: above the cap, `cap + overflow / (overflow + cap) x cap`. The gain can at most double |
| Cap on the held bonus | Evolve: log growth to `250 + phage`, then much slower. A second currency exists to lift the cap of the first |
| Softcap on the exponent | Synergism: `(product of multipliers)^DR`. One dial weakens every multiplier. Realm Grinder does the same to old bonuses at a new layer: `B^(0.1 / A)` |
| Marginal-rate table | Synergism: 100% of the first 3,500, 50% of the next 500, 40% of the next 2,000, and so on. Easy to explain |
| Two branches | Legends of IdleOn drop rate: a power law below 1000, then `(x - 1000) / (x + 2500) x 0.5 + 0.297` |
| Root on allocation | NGU Idle wishes use a 6th root of each resource: double one resource gives +12.5% |
| Rate cap | NGU Idle: 50 levels each second. More resource then gives nothing, and the player moves on |
| Hard cap on a percentage | Melvor Idle: preservation 80%, damage reduction 95%, doubling 100% |
| Slower clock in place of a cap | Antimatter Dimensions replicanti: past 1.8e308 the interval grows x1.2 for each further 1.8e308 |
| Self-limiting state | Exponential Idle theory 4: `dq/dt = 8 x q1 x q2 / (1 + q)`, so q grows as the square root of time |
| Growth base that falls | Legends of IdleOn class XP: the base `1.208 - min(0.164, 0.215 x lvl / (lvl + 100))` falls toward 1.044 |

Players in one game (Revolution Idle) report that hidden softcaps make planning impossible.
If a design has a softcap, show the marginal gain.

## 6. Walls

- **The enemy grows faster than the player (Trimps).** Health is about x1.99 and attack about
  x2.08 for each zone. Equipment costs x1.2 for each level. The run stops where the player
  cannot find x2 more power, and that is the moment to reset.
- **A pure exponential wall on the second layer (Realm Grinder).** Gems needed for reset R:
  `1e27 x 100^R`. The next layer does not remove the wall: it subtracts an offset from R.
- **Stages that get harder (Antimatter Dimensions).** Galaxy cost: +60 each, then the step
  grows by +2 for each galaxy past 100, then +0.2% of the total for each galaxy past 800.
  Linear, quadratic, exponential, at known points.
- **Change of formula (Realm Grinder research).** `(RP + 1)^6.5` up to 10,000, then
  `1e-30 x 1.033^(0.33 x RP)`. The change point is a planned plateau.
- **The number limit as a wall (Antimatter Dimensions).** 1.8e308 is the first reset, and to
  break it is an upgrade.
- **A gate as a ratio (Trimps).** An army upgrade needs population at 3x the army. The gate
  scales with the player.
- **Constants that change with difficulty (NGU Idle).** The boss bonus base goes 2^n, then
  1.5^n, then 1.2^n. Speeds are divided by fixed constants. A new layer with no new formula.
- **XP that doubles on a fixed period (Melvor Idle).** XP for a level is about
  `floor((L - 1 + 300 x 2^((L - 1) / 7)) / 4)`. It doubles each 7 levels, so level 92 is the
  halfway point to 99.

## 7. Cost and value as one exponent

From the Exponential Idle guides (Lemma 1.1 of the optimal publication text):

If `cost = a x b^level` and `value = c x d^level`, then the value that a currency amount rho
buys is `value = K x rho^r` with `r = log_b(d)`.

- One number, r, says how strongly an upgrade line turns currency into power. A x2 value for
  a x10 cost gives r = 0.30. A x1.1 value for a x1.15 cost gives r = 0.68.
- Multiply two lines and the exponents add. Add two lines and the larger exponent wins.
- A stepwise value (the gain for each level grows x2 each 10 levels) acts like a base of
  `2^(1/10)`, about 1.072, with a sawtooth: a level is worth more just after a step.

Use r to compare lines in a model before you run it. A line with r far above the others
will dominate.

Generators that produce generators (Antimatter Dimensions, Exponential Idle theory 2,
Pecorella II) give growth that is polynomial in time: each tier adds one power of t. A new
tier changes the degree of growth, not only the rate.

## 8. Other patterns

- **Milestone multipliers on count owned.** AdVenture Capitalist: x2 at 25 and 50, later
  each 100. Clicker Heroes: x4 each 25. NGU Idle hacks: a multiplier each 10 or 100 levels.
  These keep old lines worth a purchase.
- **Reset inside the run.** Trimps Warpstation: `base x 1.75^G x 1.4^L`. A "Gigastation"
  sets L to 1 and raises G. Revolution Idle does the same for each circle.
- **Spend against hold.** Clicker Heroes keeps unspent souls near `(level + 1)^2` because a
  bonus scales with them. AdVenture Capitalist guides cap an angel spend at 20% for x2 and
  30% for x3. Melvor Idle mastery checkpoints stay active only while the pool is above
  10/25/50/95%.
- **Best level by cost type (Clicker Heroes).** For a cost of n each level, level to
  `sqrt(souls / 5)`. For a cost of `n^1.5`, level to `(souls / 5)^(2/5)`.
- **Carry-over with a root.** Kittens Game chronospheres keep `1.5% x 100 x sqrt(X)` of a
  resource through the reset. A hoard helps the next run and does not replace it.
- **Rare drops that do not depend on speed.** Melvor Idle pet chance for each action is
  `action seconds x level / 25,000,000`. Speed upgrades do not change drops for each hour.
- **Milestones on a Fibonacci sequence.** Universal Paperclips trust: the next milestone is
  the next Fibonacci number x 1000. Each gap is about x1.618, which is softer than x2.
- **A different cost curve for each era.** Universal Paperclips: exponential in phase 1,
  polynomial in phase 2. The change of curve marks the change of era.
- **Bonus on a bank.** Cookie Clicker's "Lucky" pays the smaller of 15% of the bank and 15
  minutes of income. The best bank is 100 minutes of income: a reason to hold.
- **Authoring in exponent space (Idle Idol).** One master value, "cost increase for each
  level", sets the cost of 40 upgrades. Costs pass rewards at a chosen level.

## 9. Sources

Pages read on 2026-11-13. "Code" means public source code of the game or of a community
tool.

| Game or study | Source |
| --- | --- |
| Cookie Clicker | cookieclicker.wiki.gg: Buildings, Ascension, Cookies_per_Second, Golden_Cookie |
| Antimatter Dimensions | antimatterdimensions.wiki.gg: Antimatter_Dimensions, Antimatter_Galaxies, Infinity, Replicanti |
| Synergism | Code: github.com/Pseudo-Corp/SynergismOfficial (`Synergism.ts`, `Buy.ts`, `Calculate.ts`); incrementalgames.miraheze.org/wiki/Synergism_Guide |
| Kittens Game | wiki.kittensgame.com: bonfire, metaphysics, religion, paragon, karma, math-hacks |
| Trimps | trimps.fandom.com: Helium, Zones, Perks, Equipment, Warpstation, Coordination |
| Evolve | pmotschmann.github.io/Evolve/wiki.html, read from the code that builds it (`src/wiki/mechanics.js`, `p_res.js`, `src/functions.js`, `src/resources.js`) |
| Melvor Idle | wiki.melvoridle.com: Experience_Table, Mastery, Township, Pets, Bank, Preservation |
| Realm Grinder | Official 4.3.0 patch notes (developer document) |
| NGU Idle | sayolove.github.io/ngu-guide/en/mechanics (ngu, general-info, augmentation, hacks, wishes) |
| Clicker Heroes | blog.clickerheroes.com (ancient souls, hero souls, ancient levels) |
| Idle Wizard | idlewizard.wiki.gg: Basic_Mechanics, Exile, Class_Growth_Rate_Guide |
| Legends of IdleOn | idleon.wiki: Formulae, Drop_Rate, Damage, Construction |
| Exponential Idle | exponential-idle-guides.netlify.app: optimal-pub, stepwise-variables, theories-1-4, ex-basics; code: github.com/conicgames/theory-sdk (`api/Costs.js`) |
| Universal Paperclips | Code: decisionproblem.com/paperclips (`main.js`, `projects.js`) |
| Egg, Inc. | Code: github.com/fanaticscripter/EggContractor (`solo/player.go`), github.com/fanaticscripter/Egg |
| AdVenture Capitalist | Pecorella parts I and III; gameplay.tips basic guide; Steam threads on angel thresholds |
| Idle Slayer | Steam threads on stacking (no wiki page was read) |
| Revolution Idle | Fan guides only. No formula from this game is used above except the reset inside the run |
| Pecorella, "The Math of Idle Games" I to III | gamedeveloper.com/design/the-math-of-idle-games-part-i (and part-ii, part-iii) |
| Pecorella, "Quest for Progress", GDC Europe 2016 | Slides on slideshare.net; summary on kongregate.com |
| King, "Numbers Getting Bigger" | code.tutsplus.com, 2015 |
| Demaine and others, "Cookie Clicker" | arXiv 1808.07540 |
| Padinha, "Balancing tips: how we managed math on Idle Idol" | gamedeveloper.com, 2020 |

Not verified, and so not used: the hero cost formula on the Clicker Heroes wiki (the 1.07
above is from King), the base building ratio of Realm Grinder, the Idle Slayer point
formula, and the Idle Champions favor formula.
Many game wikis on one large wiki host refused automated requests, so those pages were not
read.
