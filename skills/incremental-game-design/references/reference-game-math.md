# Reference game math

The formulas and growth rates of the reference game (Idle Obelisk Miner), for use as
comparison values when you judge or design the math of another game.

**Sources.** (W) the game's wiki, including its cost templates and data modules. (C) the
formulas inside the community calculators that the wiki links: 16 public spreadsheets and 6
web tools, read 2026-11-13. A value from (C) is what a player-made tool encodes. The tools
agree with each other on the main formulas, and some of them check their results against the
game, but none is official. Treat each number as a reference point, not as a rule.

## Contents

1. The wall: how the spine asks for power
2. The answer: how power stacks
3. Cost curves by system
4. Where walls come from
5. Chance and expected value
6. The premium currency
7. Offline, online, active
8. Clocks
9. Resets
10. The event as a small copy of the game
11. What players calculate (and what that says about the design)
12. Rules of thumb to carry to another game

## 1. The wall: how the spine asks for power

| Quantity | Formula | Source |
| --- | --- | --- |
| Obelisk health | 1e5 x 2.8^min(L-1, 60) x 30^max(0, L-60) | C (fight calculators) |
| Obelisk armor | x2.8 for each level to level 60, then x9.5 | W, C |
| Health / armor | about 1e4 for levels 10 to 60 | C |
| Hit on the Obelisk | (damage - armor), then the crit chain. Zero if damage is at or below armor | W, C |
| One fight | 30 seconds of attacks. Damage carries over between fights. Cooldown 20 minutes | C |
| Fights needed | ceil(health / (damage for each second x fight length)) | C |
| Base pickaxe damage | (L+1)(L+2)/2 for upgrade level L: quadratic | C |
| Ore health | x2.5 for each ore in order, with jumps at world borders (about x5 into worlds 2 and 3, about x62,000 into world 4) | W, C |
| Floor clear requirement | Floor x 5, x 15, x 35, x 105 in worlds 1 to 4 | W |

Three things to take from this:

- **A wall has two tests.** Armor is a threshold: below it the player does nothing, and crit
  cannot help because armor is subtracted first. Health is a volume: it is paid over many
  timed attempts. The threshold gives a sharp "cannot, then can" moment. The volume gives a
  countdown of real days. Just above the threshold, +10% damage can double the damage dealt.
- **The base grows as a polynomial and the wall as an exponential.** The quadratic base can
  never keep up with x2.8 for each step. All real progress must come from multipliers in
  other systems. This is the engine: the spine asks, the ribs answer.
- **Game speed does not buy fight damage.** It cancels out of the fight formula. The fight is
  paced in real time by its cooldown. Speed helps farming, not the wall.

## 2. The answer: how power stacks

| Fact | Source |
| --- | --- |
| Pickaxe damage = base x one factor for each source menu. Two independent tools count 14 and 16 menus, with 33 and 37 separate factors | C |
| Inside one menu, "+N%" sources add into one pool: (1 + sum). Each "+0.0Nx" source is a factor of its own | C |
| Of 237 stats on the wiki: multiplier stats mostly multiply across menus (48 of 61). Chance stats mostly add across menus (47 of 67). Caps add | W |
| Early upgrades in a menu share one additive pool. Later upgrades in the same menu are each their own factor | C |
| "For each X owned" terms feed damage: statues, cards, polychrome cards, relics opened, challenges done, contracts done, skill nodes, Obelisk level | C |
| A second layer of "all X" multipliers sits over families (all floor multipliers, all star multipliers, all bomb damage) | C |
| Crit tiers nest: crit, super, ultra, omega. Each tier multiplies the tier before it, and needs it | W, C |
| Obelisk armor reduction is additive and has four sources only | C |

Consequences:

- **A new system brings its own factor.** It adds power with no re-balance of the old
  systems, and a +10% in any of 15 factors is worth the same +10%. The player's question
  becomes "which +x% is cheapest now", which is a real decision.
- **The label tells the rule.** "+10%" goes into a pool and fades as the pool grows. "x1.1"
  never fades. The game moves from the first form to the second when the pool is so large
  that one more +10% no longer matters.
- **Chances add and stay bounded. Multipliers multiply and do not.** A chance above 100% is
  wasted, which pushes the player to the next source.
- **Collections convert breadth into depth.** "For each X owned" makes each collection a
  damage source, so no system is dead content.

## 3. Cost curves by system

Cost growth for each level, from the wiki cost templates (W) and the calculators (C).

| System | Currency | Growth for each level | Shape |
| --- | --- | --- | --- |
| Gem upgrades (Archaeology) | Premium | x1.05, capped at 1,000 | Near-flat |
| Event gem upgrades | Premium | +25 or +50 for each level | Linear |
| Relic reroll, drone fuel | Premium | Flat (200, 5) | Flat |
| Archaeology fragment upgrades | Minigame currency | x1.2 | Constant |
| Player level | Experience | x1.2 (total = 250 x (1.2^(L-1) - 1)) | Constant |
| Skill node levels | Skill points | x1.25 | Constant |
| Event upgrades | Event currency | x1.25 | Constant |
| Prestige artifacts | Prestige points | x1.25 to x1.35. Tier bases 25, 300, 10k, 1.5m | Constant, with tier jumps |
| Workshop, worlds 1 and 2 | Bars | x1.3 | Constant |
| Workshop, world 3 | Bars | x1.2 | Constant |
| Workshop, world 4 | Bars | x1.45 | Constant |
| Contract upgrades | Contract points | x1.3. Bases 14-42, 75-280, 300-550, 2,500-6,500 by world | Constant, with world jumps |
| Drone upgrades | Drone points | x1.45 to x1.7 | Constant |
| Drone suit upgrades | Drone points | Linear to level 10, then a x5.5 jump at level 11 | Stepped |
| Generator | Fuel (premium) | Settles at x1.5 | Accelerating, then constant |
| Coal upgrades | Coal | Constant step | Linear |
| Drone grade | Drone experience | base x grade | Linear for each grade, quadratic total |
| Card gilding | Gold + prestige points + gems | Flat or linear, with one x1e7 step after world 2 | Stepped |
| Workshop base price by world | Bars | 50, 1e5, 7.5e10, 1e27 | World jumps |

Patterns:

- **The ratio is constant inside a system and different between systems.** x1.05 for premium,
  x1.2 to x1.3 for the common systems, x1.45 to x1.5 for the late or strong ones.
- **Gains are linear for each level (+3%, +0.1x, +1) while costs are geometric.** The value
  of a level falls as 1/n and its cost grows as r^n. Each line has a natural stop level. The
  final limit is the cap, not the cost.
- **Slow and premium currencies get flat or linear prices.** Each purchase stays reachable.
  The limit is a cap, and progress raises the cap.
- **The last item of a system is most of its cost.** The fourth artifact tier is 99.99% of
  all prestige points. The last two drone suits are almost all drone points. 69% of the skill
  tree cost sits in its 11 most expensive nodes. The tail of a system is a long-term goal.
- **Late content multiplies old prices in place of new tables.** Ascension multiplies the
  same Archaeology costs by 5 and 10 (and gem costs by 25 and 50).

Levels for each x10 of cost: 12.6 at x1.2, 8.8 at x1.3, 6.2 at x1.45, 5.7 at x1.5.

Premium prices and rewards along the spine (W):

| Item | Value |
| --- | --- |
| Gem reward for each main level | 100 at level 1, 8.5 million at level 75. Growth for each level: x1.05 (levels 1 to 20), x1.07 (to 30), x1.14 (to 40), x1.08 (to 50), x1.21 (to 60), x1.42 (to 70), x1.53 (to 75) |
| Pets 1 to 16 (gems) | 1, 350, 750, 1,000, 1,500, 1,750, 2,000, 2,250, 2,500, then 4,000, 7,500, 15,000, 250,000, 2,500,000 for the late members |
| Star discovery (gems) | 1, then 250 x (stars owned + 1) for stars 2 to 19, then 1 million and more |
| World monuments (gems) | 2,000, then 7,500, then 1,000,000, each plus three vein types |
| Generator levels 22 to 30 | 1,231,615 gems in total |

The gem reward and the wall are different units: gems do not turn into damage at a fixed
rate. Read the reward series as "the premium income grows slowly for 50 levels, then fast",
which matches the size of the late sinks.

## 4. Where walls come from

Cost growth for each level is low, so the next level is always near. The walls come from
three other places:

1. **Base-price jumps at world borders.** A new world multiplies base prices by 1e3 to 1e17.
2. **Level caps.** A line stops at its cap. Systems far away raise the cap: one drone grade
   cap is 25 + a star level + a fishing notice upgrade + a platinized statue. One pet cap
   needs a statue, a Black Hole level, and a fish tribute. One cap forces progress in three
   or four systems, and "+1 cap" re-opens cheap old levels and gives old currencies a use.
3. **Enemy steps.** The Obelisk change at level 61 and the ore jump into world 4.

Farming has a different gate from the spine. For farming, damage is a yes or no test ("can
one bomb clear the rock"). Past that test, more damage has zero value on that floor, and
income is screens for each hour x about nine chance-and-multiplier pairs. Floors past 65
carry debuffs that target the strongest multipliers, and portals bring old ores to deep
floors. Both keep the full floor range in use and stop one best floor from winning.

## 5. Chance and expected value

| Mechanic | Model | Source |
| --- | --- | --- |
| Independent multiplier rolls (2x and 3x can both hit: 6x) | Expected value = product of (1 + p x (k - 1)) | C |
| Nested tiers (golden, rainbow, galactic, prismatic) | Each tier needs the tier before, and multiplies it. Top-end 5 x 50 x 10 = 2,500 at small chances | C |
| Combo bonus | Two chances that hit together pay an extra multiplier | C |
| Rare drops (drone cards, shards) | Flat odds for each tick, 1 in 86,400 to about 1 in 1,000,000. No pity. Wait scales with 1 / game speed | C |
| Fish cards | Chance for each catch = amount caught / cost: power raises the odds | C |
| Divine relic | 1 in 25,000 for each chest. Reroll: flat 200 gems against a binomial tail. Cost about doubles for each extra Divine | W, C |
| Capped random rewards | When a perk is at its cap, its share flows to the perks with no cap | C |
| Loot tables | Weights change when content unlocks (a reward for a finished system goes to weight 0) | W, C |
| Contract points | Base x independent 2x, 3x, 5x, 10x rolls. Saved example: mean 261, standard deviation 598 | C |

Formulas: expected multiplier of one roll = 1 + p x (k - 1). Mean wait for a drop = 1 / (p x
rolls each hour). Wait that 95% of players stay under = ln(0.05) / ln(1 - p) rolls, which is
about 3 / p.

Notes for a designer:

- Stacked independent rolls keep the expected value simple and make the rare all-hit moment
  large. That moment is juice that the math gives for free.
- High variance with a cheap reset invites rerolls. Expect players to do it, and price it.
- No pity means the mean wait is not the wait that a player lives through. For a drop with
  chance p for each roll, half the players wait 0.69 / p rolls and one in twenty waits
  3 / p. If a 1-in-N drop gates progress, bound the wait.
- Path-dependent systems (stamina, cooldowns, caps, unlock thresholds) needed simulators.
  All other systems had closed forms. Where the community needed a simulator, the design has
  real decision depth.

## 6. The premium currency

- **Many small faucets in play, each with its own stats.** One calculator sums nine sources
  of gems for each hour (random pickups, a drone that makes more pickups, supply drops,
  freebies, a sell bonus, a buff drone, a fishing drone, a gem bomb, frogs), minus paid buffs
  and fuel. Each faucet has its own upgrade stats: one more upgrade track for each faucet.
- **Some purchases pay back, and the break-even point moves with progress.** In one sample,
  fuel for a drone loses 63 gems an hour at grade 0, breaks even at grade 4, and earns 291
  gems an hour at grade 45. The player invests in the system first, then gets a clear "now it
  pays" moment.
- **Free and paid buffs share one uptime budget.** A paid buff is worth only the minutes that
  free buffs do not cover. Gem spend on buffs limits itself.
- **Sinks grow by orders of magnitude and are known in advance.** Fishing alone lists 15
  million gems of gilds and tributes. A late guide quotes budgets of 50 thousand, 1.3
  million, 2 million, and 4 million and more.
- **Linear cost with a flat tail.** One line changes to a flat price after level 199: a sink
  that never ends and never gets out of reach.
- **Two currencies on one stat, as separate multipliers,** keep both worth buying:
  (1 + 0.04 x fish upgrade) x (1 + 0.05 x gem enhancement).
- **Paid tiers change rates, not rules.** They shorten timers, add 10% to 13% speed, and
  raise bank caps. The community sheet has a "what if I was free to play" switch: a direct
  fairness indicator.
- **Banks with caps set the check-in rhythm.** Recommended check-in = the shortest fill time
  of the banks. Bank caps are upgrades.

## 7. Offline, online, active

Offline play keeps about 85% to 92% of income (W, C). One tool rates active play at x1.25
game speed. The premium for active play is small and explicit, about x1.5 between the two
extremes. Offline income uses average expected multipliers.

## 8. Clocks

| Clock | Period | What changes it |
| --- | --- | --- |
| Free pack | 10 minutes, down to about 6. Bank of 2, up to 184 | Skills, gem upgrades |
| Random visitor | 20 game minutes. Banked | Its own stats, a drone |
| Main-level attempt | 30-second fight, 20-minute cooldown | An artifact, a relic |
| Star floors | They move each 30 minutes of real time | A follow skill, a hold skill |
| Real-time minigame (Archaeology) | Real time. Game speed does not change it | Banked seconds from visitors |
| Offline | Cap 12 hours (24 paid) | - |
| Event | One month in each two | - |
| Late premium sink | About one week of income for one step | - |
| Rare drone drop | "10 to 20 days per drone average" | Game speed |

A game needs a spread of clocks: one in minutes, one in tens of minutes, one in hours, one in
days or weeks. The shortest bank fill time is the check-in interval.

## 9. Resets

- Prestige takes the player level and the per-run resources. It never takes the spine.
- Prestige points = 12 x 1.084^(level - 10), from level 20, with +5% for each level above 200
  (W calculator). Experience needs x1.2 for each level. So x10 experience income gives 12.6
  more levels and only x2.8 prestige points: points are about (experience)^0.44. The reward
  is strongly sub-linear: reset often, and raise the experience multiplier. Use 0.4 to 0.5
  as a first value for a reset exponent.
- Artifact tiers open on the spine (levels 8, 14, 19) with base prices 25, 300, 10 thousand,
  1.5 million. Each tier is a new sink for the reset currency.
- The minigame reset (Ascension) keeps the idols and multiplies costs. See pattern 12 in
  reference-game-patterns.md.

## 10. The event as a small copy of the game

From the event simulator (C): enemy health 4 + 7w and attack 2.5 + 0.6w at wave w (linear).
Currency (w^2 + w) / 2 (quadratic). Upgrade cost x1.25. Flat gains (+1 attack, +2 health).
Prestige gives +10% for each prestige on attack and health, and upgrades open at prestige
counts 0 to 10. Four currencies, from each wave and each 5th, 10th, and 15th wave.

Twice the wave gives four times the currency, and costs grow x1.25 for each level, so each
run buys a few more levels. The event repeats the full loop (wall, upgrades, reset) in a
short span and pays into the main game. It is a useful template for a first minigame: linear
enemies, quadratic income, geometric costs, a reset that multiplies.

## 11. What players calculate (and what that says about the design)

Each tool exists because a decision in the game is real and not obvious.

| Community tool computes | The design fact behind it |
| --- | --- |
| Ranked "cheapest +x% damage now" | Many multiplicative sources with different prices |
| Attempts and real time to defeat the next wall | The wall is a threshold plus a timed volume |
| Best floor for a resource, as a share of a perfect floor | Locations trade share, debuffs, and portals |
| Tool A against tool B break-even | Two routes to one resource |
| Net premium currency for each hour, by source and sink | An earnable premium economy |
| Payback of a premium purchase and its break-even level | Purchases that turn positive with investment |
| Total cost to max a system, and years at the free rate | Long tails as long-term goals |
| Expected time to a rare drop | Rare drops with no pity |
| Reroll cost to reach k rare results, with go or no-go advice | Flat reroll price on a steep tail |
| Check-in interval from bank fill times | Capped banks |
| Paying against free income gap | Paid tiers that change rates |
| Late-game checklist in order, with income gates | A web of cross-system gates |
| Model against actual (calibration ratio) | The game exports its stats |

The last row is a design feature. The game exports its full stat state as JSON. Players
build tools on it and check them against the game. A stat export in a Godot build costs
little (a `JSON.stringify` of the stat dictionary) and it also feeds this skill: the export
is the fastest way to calibrate a model.

A late-game guide states its gates as income rates ("offline star income of 300 million an
hour or more"), not as levels. Players measure readiness by income for each hour.

## 12. Rules of thumb to carry to another game

1. Give the spine an exponential requirement and a polynomial base. Make other systems
   supply the difference.
2. Let each new system bring its own multiplicative factor on the key stat. Keep sources
   inside a system additive.
3. Keep chances additive and capped. Keep multipliers multiplicative.
4. Use a low, constant cost ratio inside a system (x1.2 to x1.45). Put the walls in tier
   jumps, world jumps, and caps.
5. Make caps a reward. Let a far system raise the cap of a near one.
6. Price slow and premium currencies flat or linear. Limit them with caps.
7. Put most of a system's cost in its last items, and show the player the distance.
8. Give a wall a threshold (a sharp moment) and a volume (a countdown).
9. Nest rarity tiers when you need a new multiplicative axis late in the game.
10. Make one or two premium purchases turn positive after investment.
11. Pay a small, explicit premium for active play. Keep offline near 85%.
12. Export the stats.
