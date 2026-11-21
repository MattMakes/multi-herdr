# Short incremental games for continuous play

This file holds the facts of 11 short incremental games. 10 are on Steam. The player plays each
game continuously, in 1 or a few sittings. The interview (`references/interview.md`) and the
catalog (`references/systems-catalog.md`) cite this file with the prefix `SG`. The source of the
facts is the research document `docs/research/steam-continuous-pattern.md` of 2026-11-14,
section 3.

## Contents

1. Marks and sources
2. Summary table
3. The games: G1 to G11
4. Target bands for continuous play
5. Facts that no source gives
6. Source list

## 1. Marks and sources

Each fact has 1 mark:

- **(S) sourced:** a page at the cited URL states the fact.
- **(M) measured:** a play-time site, a reviewer's own play time, or a speed-run board gives the
  number.
- **(I) inferred:** the reasoning of the research. The reason follows the mark.

A citation `[Wn]` names a URL in section 6. The play-time sites are crowd data. HowLongToBeat
refused a fetch, so its numbers come from pages that quote it. `[W8]` is a guide of low quality,
used for structure only.

## 2. Summary table

| Game | Length to finish | Reset | Hard end | Run on a limit |
| --- | --- | --- | --- | --- |
| Nodebuster | 3 to 4 h | Prestige level for each boss | Yes | Yes |
| Shelldiver | 3 to 5 h | None | Yes | Yes |
| Digseum | 2 to 2.5 h | None | Yes | No |
| A Game About Digging a Hole | 2 h | None | Yes | Yes |
| Magic Archery | 1 h | Not stated | Yes | No |
| Tower Wizard | 5 to 7 h | Prestige, 3 walls | Yes | No |
| Scritchy Scratchy | 7 to 10 h | Prestige (Soul Machine) | Yes | No |
| Spaceplan | 2 to 9 h | None | Yes | No |
| Universal Paperclips | 5 to 10 h | None (eras) | Yes | No |
| (the) Gnorp Apologue | 11 to 19 h | Not stated | Yes | No |
| MMO98 | Not stated | Not stated | Not stated | Not stated |

## 3. The games

### G1 Nodebuster (2024, Steam, $2.99)

- Developer text: "a short, experimental incremental game" (S) [W1]. 97% positive of 10,730
  reviews (S) [W1]. The game sells in a Steam bundle named "Short and Sweet Incrementals"
  (S) [W1].
- Length: about 3 hours (M) [W2]. Just over 4 hours (M) [W3]. Median 4 hours (M) [W4].
  Fastest listed run 1 h 41 min, from a cached board snapshot (M) [W5].
- Demo: the store page states none (S) [W1].
- Systems: 2 screens at the start, the skill tree and the breach session. The milestones tab
  opens third, then the crypto mine, then the lab (S) [W3]. 6 currencies: bits, nodes, skill
  points, cores, netcoin, processors (S) [W3]. About 97 tree entries (S, low reliability) [W8].
- Run: a breach session ends at the boss kill (S) [W3], or at health 0, or by a manual stop
  (S) [W8]. Enemy health and damage rise during a run (S) [W8]. Uncollected loot is lost at the
  run end (S) [W8].
- Reset: each boss kill gives a core and raises the prestige level. Enemies get stronger and
  pay more (S) [W3]. The guide names prestige levels 24 and 25 for late farming (S) [W8].
- Active play: early damage comes only from the cursor on an enemy (S) [W2]. "It takes about
  half the game to unlock any damaging mechanics" (S) [W2]. "Very little idling time, only in the
  late game if you're going for 100%" (S, a player quoted in [W2]). The sessions "ease into a
  more idle-friendly pace as you near the endgame" (S) [W3].
- End: the lab deploys the "godvirus" for 1,000 netcoin. "This marks the end of the game"
  (S) [W3].
- Active factor: early, a run with no input pays 0, so the factor has no upper limit
  (I: the only damage source needs the cursor [W2]).

### G2 Shelldiver (2025, Steam, £4.19)

- Length: about 3 hours for a full clear and all achievements (M) [W6]. About 4 hours (M) [W7].
  Median 5 hours, average 4 hours, from 89 playthroughs (M) [W9].
- Run: each dive lasts 1 to 2 minutes (S) [W6]. Air is a limit in a level (S) [W7].
- Purchases: "something like four or five upgrades in the skill tree" after each run (M) [W7].
  The skill tree is "disproportionally huge", and each node has several levels (S) [W7].
  1 currency: cash from sold catch (S) [W7].
- Automation: buildings on the island "automate most of the processes" (S) [W7].
- Reset: none stated. Reviewers ask for prestige "in the future" (S) [W9].
- Structure: the loop repeats in 4 areas (S) [W9]. End: unlock every node of the tree (S) [W6].

### G3 Digseum (2024, Steam, $2.99)

- Developer text: "Short and sweet experience (~2.5hr playtime)". 12+ excavation sites, 50+
  relics, a skill tree of upgrades (S) [W10].
- About 2 hours of active play for a full clear and all achievements (M) [W6].
- Active play: the player digs with the mouse. Each manual find raises the base income and the
  multiplier (S) [W6]. The museum pays passive income from visitors (S) [W6].
- Reset: none stated (I: no source names one, and [W10] lists none).
- Cadence: 12 sites in 2.5 hours is about 1 new site each 12.5 minutes (I: arithmetic on [W10],
  with an even spread assumed).

### G4 A Game About Digging a Hole (2025, Steam)

- Length: main story 2 hours, main and extras 3 hours (M) [W11]. 4 to 5 hours for all
  achievements (M) [W12].
- Run: dig until the pockets are full or the battery is low, then go up and sell (S) [W12].
  At battery 0 or health 0, the player drops all carried minerals (S) [W12].
- Upgrades: 4 stats: shovel, inventory size, battery, jet pack. Costs rise fast, and deeper
  minerals pay more (S) [W12].
- Reset: none (I: [W12] describes none).

### G5 Magic Archery (Steam)

- About 1 hour for the clear and all achievements (M) [W6]. Click income and upgrades. It is the
  smaller predecessor of Tower Wizard (S) [W6].

### G6 Tower Wizard (itch.io 2024, Steam 2025, about €3)

- Length: main story 5 h 24 min (M) [W13]. "5–7 hours of active engagement, plus optional full
  completion"; average 12 hours (M) [W14]. 12 hours for all achievements (M) [W6].
- Free prototype on itch.io: players report a finish in 1 h 6 min, 1 h 8 min and about 2 hours
  (M) [W15].
- Systems: 3 resources, magic, knowledge and wood. Spirits are assigned to buildings
  (S) [W14]. "An active incremental/upgrade game", progressed "without long idle periods"
  (S) [W14].
- Reset: "prestige resets that add new mechanics and bonuses" and "three obstacle walls"
  (S) [W14]. End: "Definite ending", with the achievement "Reach the ending" (S) [W14].
- Developer change after player feedback: "The tower requirements are milestones instead of
  upgrades that consume resources" (S) [W16].

### G7 Scritchy Scratchy (2026, Steam)

- Length: "seven to ten hours" to the ending and the credits (M) [W17].
- Active play: the player scratches tickets by hand. Later "a machine that scratches the tickets
  for you" and a "digital assistant to buy the tickets on your behalf" take over (S) [W17].
- Tiers: the player moves from cheap, low-yield tickets to the next tier (S) [W17].
- Reset: the Soul Machine resets progress for a permanent currency. A run of 5 hours on the
  first attempt takes about 30 minutes on the second (M) [W17].

### G8 Spaceplan (2017, Steam, Devolver)

- Length: average 7 hours (M) [W18]. One reviewer logged 30 hours with the game open, about 5
  of them in active management (M) [W19].
- "A finite scenario to enjoy", and the player can replay the ending (S) [W20].
- Reset: none. "No prestige or ascension mechanics" (S) [W4b].

### G9 Universal Paperclips (2017, browser and mobile, not Steam)

- Length: player-logged main story 9.2 hours (M) [W21]. The game "is a silent game until about 5
  hours in" (S) [W22].
- Eras: the player first makes clips by hand at $0.25. Later eras add trust, operations,
  creativity, yomi, matter, power, drones and probes (S) [W23]. Each era changes the currencies.
- Numbers: Improved AutoClippers cost 750 ops for +25%. Probes cost 100 quadrillion clips
  (S) [W23]. End: "a very definitive conclusion" (S) [W23].

### G10 (the) Gnorp Apologue (2024, Steam)

- Length: about 11 hours to the clear (M) [W6]. About 16 hours, quoted from HowLongToBeat
  (M) [W24]. Median 19 hours (M) [W25].
- Active play: early the player clicks the rock, then hires gnorps (S) [W6].
- Designer on a controlled character: "that ... stands in the way of the enjoyment players
  derive from watching numbers grow" (S) [W26].
- After the end, a time-trial mode opens with 20 talents (S) [W24].

### G11 MMO98 (2025, Steam)

- 2 developers, about 100 days, $73,886 gross in about 3 weeks, 27,000 wishlists at launch, more
  than 5,000 copies in a few hours (S) [W27].
- Reviews fell to Mixed (59%). Some players "were disappointed it was short", because most buyers
  see only the trailer and the screenshots, not the text "this is a short incremental game"
  (S) [W27].
- "Late game matters even in short games", because some players push the systems much harder
  than expected (S) [W27].

## 4. Target bands for continuous play

These bands come from the games above (research section 4, Q1). The interview uses them for the
play pattern `continuous`.

| Target | Band | Evidence |
| --- | --- | --- |
| Length of a full short game | 2 to 10 hours. The middle is 3 to 5 hours | 9 of 11 games have a measured length inside 1 to 11 hours (section 2). Gnorp is the 1 game above 10 hours on most sites |
| Length of a 6-hour full game | Supported | Tower Wizard 5 to 7 h (M) [W13, W14], Scritchy Scratchy 7 to 10 h (M) [W17], Spaceplan 7 h (M) [W18], Paperclips 9.2 h (M) [W21] |
| Length of a free first version | 1 to 2 hours | The Tower Wizard prototype, 1 to 2 h (M) [W15], before a 5 to 12 h Steam game |
| Time to the first purchase | Inside the first run of 1 to 2 minutes | Shelldiver: 4 to 5 upgrades after each 1 to 2 minute dive (M) [W6, W7]. No game states the first purchase time. (I: the first dive ends inside 2 minutes and the shop follows.) |
| Largest gap between purchases | 2 minutes or less | Shelldiver: a run of 1 to 2 minutes (S) [W6], then 4 to 5 purchases (M) [W7]. Single source. (I: the purchases come between runs, so the largest gap is about 1 run.) |
| Expert gap | About x2 | Nodebuster fastest run 1 h 41 min against 3 to 4 h (M) [W5, W2, W3]. (I: ratio 1.8 to 2.4.) |
| Demo length | 15 to 30 minutes; 40 to 60 for a strategy or management game | "Keep it 15-30 minutes"; "nobody has conclusive data" (S) [W28]. No studied game states its demo length |
| Demo end | A cliffhanger just before a new system or era opens | Cliffhanger endings beat "complete experience" endings in wishlist conversion, with no cited study (S) [W28]. (I: the placement before a system.) |
| Extension past the demo | New eras, areas or reset mechanics in each part, not longer walls of the same content | New eras [W23], new areas [W9, W10], prestige levels [W3], prestige that adds mechanics [W14]. The Tower Wizard prototype lost players at a 350,000-resource requirement (S) [W29] |
| Hard end | Each game with structure data has 1 | 10 of 10 games (section 2): the godvirus for 1,000 netcoin [W3], the full tree [W6], "Reach the ending" [W14] |
| Reset | 0 resets, or many short runs | 5 of 10 games have no reset (section 2). Nodebuster has a prestige level for each boss (S) [W3] |

A 2-hour demo is longer than the guide band of 15 to 30 minutes [W28]. It equals the full length
of Digseum, Digging a Hole and Magic Archery. The nearest proof is the free Tower Wizard prototype
of 1 to 2 hours [W15]. (I: a 2-hour demo is a whole short game, so the full game must add new
systems, not more of the same.)

## 5. Facts that no source gives

The interview marks each of these targets "no source". It does not give a band for them.

1. Unlocks in the first 10 minutes and in the first hour. No source counts unlocks in time.
2. A numeric active factor. No source measures active income against open idle income. Nodebuster
   pays 0 to an idle player early (I from [W2]).
3. The demo length of any studied game. No store page states a demo.
4. The count of resets in a full game. Only [W8], of low reliability, gives about 25 for
   Nodebuster.
5. A play-test speed. No source states one.
6. Cost and value formulas. No source gives them, except the Paperclips project costs [W23].

## 6. Source list

- [W1] https://store.steampowered.com/app/3107330
- [W2] https://blog.lauramichet.com/played-nodebuster/
- [W3] https://mancunion.com/2025/05/08/nodebuster-review-a-short-and-sweet-incremental-game/
- [W4] https://gamebrain.co/game/nodebuster
- [W4b] https://gamebrain.co/game/spaceplan
- [W5] https://www.speedrun.com/ru-RU/Nodebuster
- [W6] https://www.gamespark.jp/article/2026/01/02/161181.html
- [W7] https://www.thexboxhub.com/shelldiver-review/
- [W8] https://www.mejoress.com/the-nodebuster-bible-complete-walkthrough-skill-tree-and-prestige-strategy/
- [W9] https://gamebrain.co/game/shelldiver
- [W10] https://store.steampowered.com/app/3361470
- [W11] https://www.dekudeals.com/items/a-game-about-digging-a-hole?platform=all
- [W12] https://www.lifeisxbox.eu/xbox-review-a-game-about-digging-a-hole/
- [W13] https://gg.deals/game/tower-wizard/
- [W14] https://rawg.io/games/tower-wizard-2
- [W15] https://barribob.itch.io/tower-wizard/comments
- [W16] https://barribob.itch.io/tower-wizard/devlog/855536/good-news-and-bad-news
- [W17] https://www.neonlightsmedia.com/blog/scritchy-scratchy-review
- [W18] https://rawg.io/games/spaceplan
- [W19] https://www.pcgamer.com/spaceplan-is-a-short-and-tasty-clicker-game-where-you-shoot-potatoes-into-orbit/
- [W20] https://toucharcade.com/2017/05/08/spaceplan-review-delicious-potatoes/
- [W21] https://www.grouvee.com/games/272890-universal-paperclips/
- [W22] https://medium.com/the-strange-games-review/universal-paperclips-is-a-silent-game-until-about-5-hours-in-67412299de26
- [W23] https://if50.substack.com/p/2017-universal-paperclips
- [W24] https://medium.com/@TheElisaDay/the-best-idle-game-ive-ever-played-the-gnorp-apologue-review-3a0adc782444
- [W25] https://gamebrain.co/game/the-gnorp-apologue
- [W26] https://gamedeveloper.com/design/interview-the-gnorp-apologue
- [W27] https://gamedevreport.beehiiv.com/p/lessons-from-a-hit-incremental-game-and-john-carmack-s-advice
- [W28] https://presskit.gg/field-guides/how-to-build-game-demo
- [W29] https://barribob.itch.io/tower-wizard/comments?after=175
- [W30] https://howtomarketagame.com/2025/10/20/steam-next-fest-october-2025-checking-in-on-the-games-that-broke-through/

Notes on the sources:

- The research opened [W1] to [W3], [W6] to [W10], [W12], [W14], [W17], [W23], [W26] to [W28] and [W30].
  The other URLs come from search summaries. Open them to confirm the exact words.
- [W5] is a cached snapshot with no category label. The time can be out of date.
- [W29]: a search summary named this page for the 350,000 stop. The exact page is not confirmed.
