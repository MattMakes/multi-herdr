# Reference game: the progression patterns of Idle Obelisk Miner

Idle Obelisk Miner (IOM) is the reference case for this skill. It is a mining idle game with
about 45 systems (60 with their sub-systems) around one core loop. This file gives the patterns that make its
progression work, with the evidence for each pattern and a question to ask about the game
under review.

Use the patterns. Do not copy the content: no IOM names, themes, numbers, or systems go into
another game. A pattern is "a collection that levels from actions in other systems". A copy
is "pets that level from bombs".

**Limits of the evidence.** All numbers come from the IOM wiki (shminer.miraheze.org, read
2026-11-13) and from the community calculators that it links. The wiki has gaps and
conflicts. It gives real-time anchors (timers, cooldowns, caps: see the clock table in
reference-game-math.md) but no measured play times. The numbers show structure (what gates
what, how fast costs grow against the main level). They do not show how many minutes a
player waits at each step.

## Contents

1. One spine, many ribs
2. Front-load, then drip
3. The gate type changes as the game ages
4. Systems feed the core loop
5. Caps are a currency
6. Tiers reuse content
7. Minigames have their own clock and export permanent power
8. Actions in one system are experience for another
9. Multi-currency sinks force parallel play
10. Walls are placed on purpose
11. New worlds arrive with a debuff that play removes
12. Reset layers nest
13. Automation is a reward
14. Readiness is shown to the player
15. Scope numbers
16. The premium currency has many small taps
17. Some premium purchases pay for themselves
18. The premium currency always has a larger sink ahead
19. Timers fill a bank, and the bank size is an upgrade
20. Away time pays less than play, by a stated amount
21. A random visitor gives active play a pulse
22. Timed buffs are a third kind of reward
23. Rare results come from nested rolls
24. The event is the core loop again, in one month
25. A task list turns "use this system" into a paid goal
26. The main level is a short attempt on a timer
27. The right to change a build is a reward
28. Money buys a higher rate on a stat that free players also have

## 1. One spine, many ribs

One number measures the full game: the Obelisk level (0 to 75). Each level gives +5 to the
player level cap (cap = 30 + 5 x level), and the level cap opens the 93 core upgrades. Each
level also pays a reward. All other systems hang from this spine: they open at a level, or
they make the next level possible.

*Ask:* What is the one number that a player quotes to say how far they are? Does each step of
it give something now and open something later?

## 2. Front-load, then drip

Twelve systems are open at the start or in the first minutes (mine floors, ores, craft,
upgrades, sell, bombs, workshop, chests, relics, the store, offline progress, and prestige at
player level 20). New menus then open at levels 2, 3, 4, 10, 12, 15, 17, 18, 19, 23, 30, 37,
and 70. In the wiki data, 54 of the 76 levels open something outside the core upgrade
tables. The other 22 give the level reward and more rows of the core tables only.

*Ask:* Does each step of the spine open something? Where is the longest run of steps that
give "more of the same" only?

## 3. The gate type changes as the game ages

Of 1,157 unlock points in the data, 558 are gated by the main level and 599 are gated by
another system. Early unlocks are level gates. Between levels 42 and 60 no new top-level menu opens.
The content in that span is chains across systems:

- Polychrome fish cards for a dock, then its legendary fish, then a tribute that costs stars,
  bars, veins, fish, and gems, then a fourth drone.
- Super Stars, then the Black Hole upgrade, then Black Hole level 1, then a drone
  enhancement, then a new system (Lootfrogs).

The game moves from "reach level N" to "bring system A to point X, and system B opens a
feature". A long span on the spine is not a lull if the side chains give beats.

*Ask:* When level gates run out, what gives the next beat? Are there chains of two or three
systems that end in a visible prize?

## 4. Systems feed the core loop

The data has 2,672 links between systems: 74% buffs, 11% gates, 8% resource flows, 7%
unlocks. Each system has links in and links out. The systems that receive the most links are
the ones that produce core resources (stars, drones, floors, veins, bombs). A new system
pays in a multiplier, a cap, or a currency that the core loop needs. The stacking rule makes
this strong: sources of one stat add inside one system and multiply between systems, so a
new system on a multiplier stat is worth a full factor (see reference-game-math.md, part 2).
The one planned exception is the timed event (pattern 24).

*Ask:* For each system: what does it give the core loop, and what does it need from it? A
system with no link out is a side game. A system with no link in has no reason to grow. When
a new system buffs a stat, is it a new factor or one more term in an old sum? Check the
code.

## 5. Caps are a currency

Many rewards are "cap +1" on an older system: a pet level cap from a star, a statue, a
challenge, a fishing tribute, an idol, and a Black Hole level; a workshop cap from a star and
a tribute; a drone grade cap from an idol. A cap raise costs no new content. It gives a
finished system a new goal, and it sends the player back to old screens.

*Ask:* Which systems have a hard maximum? Which later system can raise it?

## 6. Tiers reuse content

Collections have tiers: cards are standard, gilded, polychrome, then infernal. Statues are
built, gilded, then platinized. Each tier uses the same items with a new cost currency and a
new gate. Set rules add a goal: all 9 statues must be built before one can be gilded.

*Ask:* Which collection can get a second tier in place of 20 new items?

## 7. Minigames have their own clock and export permanent power

Archaeology, Fishing, Stargazing, and Arcanist are games inside the game. Each has its own
currency, its own upgrade tree, and its own pace:

- Archaeology runs on real time. Game speed does not change it. It has stamina, stats, and
  its own reset layer (Ascension).
- Stargazing moves its targets each 30 minutes of real time.
- Fishing counts ticks, and opens docks in order.

Each one exports permanent power: idols, tributes, and stars buff the main game. A separate
clock is important. When the core loop is slow, the minigame is not.

*Ask:* When the core loop stalls, what can the player do that still moves the game forward?
Does that activity pay into the core loop?

## 8. Actions in one system are experience for another

Each pet levels from an action somewhere else: one from basic bombs, one from contracts, one
from golden floors, one from gems spent. The sum of pet levels gates later pets (1, 5, 12,
18, 28, 40, 56, 75, 90, 100, 140, 180, 235, 310, 340). This pays the player for breadth, and
it makes each old system matter again.

*Ask:* Is there a collection that rewards the player for use of all the other systems?

## 9. Multi-currency sinks force parallel play

A monument costs gems and three vein types. A fishing tribute costs a star type, a bar type,
a vein type, fish, and gems. No single system can pay. The player must run several systems at
the same time, and the wiki guide says so directly ("you should really be doing everything at
once").

*Ask:* Does a large goal need output from three or more systems?

## 10. Walls are placed on purpose

Obelisk armor and health grow x2.8 for each level up to level 60. From level 61, armor grows
x9.5 and health x30. The reward grows much more slowly: x1.05 for each level at the start,
x1.2 near level 55, x1.4 to x1.5 after level 60. The wiki guide calls levels 62 to 66 "the
armor wall" and tells the player to leave them and go deep in the side systems. The wall is a
signpost: it says "the spine is closed, the ribs are open".

*Ask:* Where are the walls? At each wall, are there two or more side systems with clear,
reachable goals? A wall with nothing beside it is a lull.

## 11. New worlds arrive with a debuff that play removes

World 3 slows the game by 30%. World 4 slows it by 80%, and each floor has its own debuff. A
Fishing upgrade removes the World 3 debuff. Six World Quests remove 10% each in World 4.
Entry too early is a mistake, and the game says so. The debuff makes the new world a project,
not a jump.

*Ask:* Does new content arrive with a cost that the player can remove by play?

## 12. Reset layers nest

- Prestige resets the player level, gold, ores, bars, contract progress, and the core
  upgrades. It takes the per-run axis (player level) and never the spine (Obelisk level).
  All other systems are permanent. Artifact tiers open at main levels 8, 14, and 19.
- The reward is strongly sub-linear: experience needs x1.2 for each level and prestige points
  grow x1.084 for each level, so points are about (experience)^0.44.
- Contract upgrades have a limited respec.
- Archaeology Ascension resets one minigame, keeps its idols, and multiplies its costs.

A reset never deletes the systems that the player built most recently.

*Ask:* What does each reset take, and what does it never take? Is the spine safe? Does a
later reset layer protect the gains of the earlier layers?

## 13. Automation is a reward

Each manual chore is automated one step later: an auto-bomber skill, a skill that follows
stars between floors, auto-catch for stars, auto-prestige at level 30, drone suits that fire
bombs. The player learns the action by hand, then earns freedom from it. Automation is also
withheld on purpose: auto-prestige does not run offline, and the main-level fight switches
all automation off.

*Ask:* Which action does the player repeat most? When does the game take it over?

## 14. Readiness is shown to the player

Before the World 3 and World 4 monuments, the game shows recommended levels in other systems
(all 9 platinized statues, 68 challenges, generator level 20, and so on). The checklist turns
a vague wall into named goals. The game also shows one global number: 10,117 completion
points, one for each permanent unlock in each system. The community states its late gates as
income rates ("offline star income of 300 million an hour or more"), not as levels.

*Ask:* Before each large step, does the player see what "ready" means? Is there one number
for "how much of the game have I done"?

## 15. Scope numbers

About 45 top-level systems and 15 sub-systems. About 70 named currencies and resources. 76
main levels. 4 worlds and 132 floors. 23 stars. 16 pets. 27 statues. 100 challenges in 3
tiers. 5 mining drones. 4 minigames. (Counts depend on what you call a system. These come
from the wiki extraction.)

This is years of live development. A game in production needs the shape, not the size: one
spine, three to six ribs that feed it, and a plan for where the next rib attaches.

## 16. The premium currency has many small taps, and each tap is upgradeable

Gems come from a free pack each 10 minutes (5 gems at the start, 86 at the end for a free
player), from a bomb type, from random visitors, from gifts, from the event, from challenges,
and from each main level. The wiki lists 16 sources. No tap is large. Each tap has its own
stats (amount, cooldown, bank size, jackpot chance), and other systems raise them. The
premium currency is a second income that the player builds like the first one. The shop
sells permanent unlocks, not only time.

*Ask:* How many sources does the rare currency have? Can the player raise each one by play?
Is there something to buy with it before minute 30?

## 17. Some premium purchases pay for themselves

A few gem purchases raise gem income: a skill that adds gems to the free pack, a paid buff
that makes gem bombs recharge ten times faster (the guide calls it "gem-positive"), fuel for
a drone that finds gems. The return depends on other systems. In one community sample, drone
fuel loses 63 gems an hour at grade 0, breaks even at grade 4, and earns 291 an hour at
grade 45. The player gets a clear moment ("now it pays") and a reason to do arithmetic.

*Ask:* Is there a purchase that raises the income of the currency that buys it? At what
point does it start to pay back, and can the player see that?

## 18. The premium currency always has a larger sink ahead, at a predictable price

Gem prices are flat or linear inside one system. Between systems they rise by a factor of 10
or more: hundreds for the first unlocks, thousands for a collection, tens of thousands for a
minigame, a million for a world. Income rises to match (100 gems for the first main level,
8.5 million for the last). One late step costs "about 1 week of your full gem production".
The player always knows the next price, and the gem balance never stops mattering. The
community guide is a list of what to buy at each stage.

*Ask:* List the sinks of the rare currency in order of size. Is there a gap of more than x10
with no sink in it? Is there a point where the player has nothing left to save for?

## 19. Timers fill a bank, and the bank size is an upgrade

Each timed reward (free pack, random visitor, minigame seconds, exchange offers) stacks in a
bank with a cap. At the start the bank holds 2 free packs: 20 minutes away loses rewards. At
the end it holds 184: more than a day. The cap decides how often the player must come back,
and the game gives freedom from the timer as progress. The community computes the check-in
interval as the shortest fill time of the banks. This is "caps are a currency" (pattern 5)
applied to time.

*Ask:* For each timer: what is lost if the player is away for 1 hour, for 8 hours? What
raises that limit, and when?

## 20. Away time pays less than play, by a stated amount, and skills close the gap

Offline progress is "a reduced simulation of gameplay", not a flat payment. It runs the same
systems with fixed penalties (ore and bar income x0.92, stars and veins x0.85), a 12-hour
cap, and some systems switched off (chest meters, auto-prestige). Star gain falls to one
fifth unless the player owns a skill and sets it up before leaving. Random rolls use their
expected value. One community tool rates active play at x1.25. The three modes (away, open,
active) have fixed ratios, and the player knows what each is worth.

*Ask:* What is one hour away worth against one hour of play? Is that ratio shown? Which
system stops when the game is closed, and is that on purpose?

## 21. A random visitor gives active play a pulse

Each 20 game minutes a creature appears on the main screen. A tap gives a small free reward
from a weighted table (gems, a chest, a short x2 buff). A second, better reward costs a few
gems. The visitor has its own stats (spawn rate, triple chance, golden chance, bank). Its
weights change as content opens: a reward for a finished system goes to weight 0. A later
visitor type is the prize of a long chain. It is a beat that needs no content, and it is the
smallest possible premium purchase.

*Ask:* What happens on the main screen that the player did not cause? How often? Does it
offer a choice?

## 22. Timed buffs are a third kind of reward

Items give a buff for 90 seconds to 13 minutes. They drop from chests, from a minigame, and
from the visitor, and their tier follows the system that drops them. Buff time stacks, so
the real stat is uptime. Free and paid buffs share one budget of 60 minutes an hour: the
more free uptime a player earns, the less a paid buff is worth. A buff is a reward that the
designer can give often, because it expires.

*Ask:* Which rewards are temporary? Can uptime reach 100%, and what happens to the value of a
paid buff when it does?

## 23. Rare results come from nested rolls, and the lever is the number of rolls

A chest rolls a tier (about 48% common down to 1 in 25,000 for the top one). A sell bonus
rolls a jackpot, and the jackpot rolls a better jackpot. Independent multipliers can all hit
together (2x and 3x give 6x). There is no pity timer: the player's control is volume (chests
for each hour) and a flat-price reroll. When a perk is at its cap it leaves the table, so
late chests concentrate on the perks with no cap. The expected value is simple to compute.
The all-hit case is the story that the player tells.

*Ask:* For each chance mechanic: what is the mean wait in hours at this point in the game?
What is the wait for the unlucky one player in twenty? Can the player shorten it by play, or
only by luck?

## 24. The event is the core loop again, in one month, with one-way pay

Each second month an event runs for a month. It has its own wave ladder, four currencies, a
x1.25 upgrade table, and its own prestige. The wiki says that events "are not affected by
anything you do outside of them": main-game power does not enter, so a new player and an old
player start level. The rewards go to the main game, and they double with each world that
the player has opened. It is a fresh start that an old save cannot get in any other way, and
it is the one system that takes nothing from the core loop on purpose.

*Ask:* Is there content where a late player starts from zero again? Does its reward scale
with main progress, so that it stays worth doing?

## 25. A task list turns "use this system" into a paid goal

One hundred challenges in three tiers. Each one names an action in another system ("claim 10
visitor rewards", "mine 10 thousand of one vein") and pays 10 coins of its tier. The coins
buy permanent rewards from a short shop for that tier, so the player chooses the order of
the rewards. A tier opens when the tier before is complete. The count of tasks done is also
a readiness measure for later content ("Challenges: 68").

*Ask:* Is there a list that tells the player what to try next in each system? Does a
finished task pay a choice, or a fixed reward?

## 26. The main level is a short attempt on a timer, and damage carries over

A fight on the spine lasts 30 seconds and can be tried each 20 minutes. Only the hand tool
counts: drones and bombs are off. Below the armor value the hit is zero, which is a hard
gate. Above it, health is a time gate, and damage is saved between attempts, so each attempt
shows progress. The 20-minute timer is one more clock, and the fight is the one place where
the player tests the full build.

*Ask:* Is a step on the spine an event with its own moment, or a number that ticks over? Can
the player see partial progress on a step that is not done?

## 27. The right to change a build is a reward

In the per-run upgrade system the player gets a small number of refunds for each reset, and
later skills add more. In one minigame, respec is free but drains the stamina of the current
run. Where a build matters for a short time, respec is cheap. Where a build defines a run,
respec is rationed, then loosened as a reward.

*Ask:* Where can the player make a wrong choice? What does it cost to undo, and does that
cost fall later?

## 28. Money buys a higher rate on a stat that free players also have

Each paid perk is a larger value of a free stat: 116 gems in a free pack in place of 86, 304
banked packs in place of 184, a 24-hour offline cap in place of 12. No system is closed to a
free player. The economy is balanced for the free values, and the paid values are a bounded
bonus. One community sheet has a "what if I was free to play" switch.

*Ask:* If the game will sell something: is each paid item a rate on a stat that exists? What
is the total income gap between a paying player and a free player, as one number?
