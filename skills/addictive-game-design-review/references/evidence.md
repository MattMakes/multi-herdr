# The evidence

This page gives the research behind the review, what each finding supports, what it does not support, and how firmly
you can state a change that rests on it. A design heuristic is not an experimental result. Connect each change to a
finding, give its grade, and write the change as a hypothesis with a test.

## Contents

1. Evidence grades
2. The 3 things a player must feel
3. The research findings
4. The counter-evidence
5. Risk, reward and coercion
6. Sources

## 1. Evidence grades

| Grade | Meaning | How to write the finding |
| --- | --- | --- |
| `E` | An experiment or a field study measured the effect on people | "The research supports ..."; name the study and its limit |
| `T` | A theory or a review proposes the mechanism | "The theory proposes ..."; a play test must confirm it |
| `D` | A developer said it, or a released game does it | "Game X does this ..."; it worked in that game, at that pace |
| `H` | A design hypothesis of this skill or of the reviewer | "Hypothesis: ..."; never state it as a fact |

No grade makes a change certain for this game. `E` means the mechanism is real, not that the change is right. A
finding of grade `H` with a cheap test can rank above a finding of grade `E` with a high cost.

## 2. The 3 things a player must feel

| Feeling | The player says | The game gives it with |
| --- | --- | --- |
| Progress | "I am becoming more powerful." | Purchases and automation that make earlier work faster, easier or unnecessary |
| Agency | "My decisions matter." | Limited slots, bottlenecks and trade-offs, so that no single shopping list exists |
| Discovery | "I want to know what comes next." | New interactions, mechanics and changes of the world that revise what the player expects |

A game with progress only is a number that grows. A review looks first for the feeling that is missing.

## 3. The research findings

### F-1 Competence and autonomy (grade E)

Ryan, Rigby and Przybylski, 2006, 4 studies. Perceived competence and perceived autonomy were associated with
enjoyment, preference and immersion in video games. Relatedness also mattered in multiplayer play.

- Supports: upgrade trade-offs that are not trivial, a visible result for each choice, and mastery that removes
  repeated work.
- Does not support: a claim that any 1 choice system raises enjoyment.
- Checks: R-1, R-3, R-7.

### F-2 The goal gradient (grade E)

Kivetz, Urminsky and Zheng, 2006, field and laboratory studies. People in reward programmes worked faster as the
reward came near.

- Supports: a next step that is visible and attainable, with its progress shown.
- Does not support: any exact distance between upgrades. No study gives the best interval.
- Checks: R-10, R-12.

### F-3 Reward schedules are personal (grade E)

Nagle, Wolf, Riener and Novak, 2014, N = 210, a memory training game. Variable-ratio rewards did better than
fixed-ratio rewards on average. Schedules matched to the preference of the player helped on several outcomes.

- Supports: bounded variation in optional rewards, and control that the player can increase.
- Does not support: randomness on essentials. A random block on basic progress is a fault.
- Checks: R-6.

### F-4 Reward variety (grade E)

Johnson and colleagues, 2018, N = 59, repeated measures. A more generous and more varied reward condition raised some
measures of enjoyment, effort and presence.

- Supports: a mix of reward types: a number, a capability, a visible change of the world, a piece of story, a new
  strategic option.
- Does not support: a claim about which reward type caused the effect. The study does not separate them.
- Checks: R-13.

### F-5 More feedback is not always better (grade E)

"The Effects of Juiciness in an Action RPG", 2020, N = 3,018. The version with no sensory effects and the version
with extreme effects both did worse than the medium and high versions on several outcomes.

- Supports: restrained effects for ordinary production, and large effects for large achievements only.
- Does not support: "add juice" as a general fix.
- Checks: R-16.

### F-6 Reward prediction error (grade T for games)

Schultz, Dayan and Montague, 1997. Dopamine neurons of primates signalled the difference between the expected reward
and the reward received. This supports theories of learning from surprising outcomes.

- Supports, as a hypothesis: alternate predictable gains with breakthroughs that the player earned and that are
  larger than the player expected.
- Does not support: a claim that every surprise in a game raises enjoyment.
- Checks: R-14.

### F-7 Curiosity and the information gap (grade T)

Loewenstein, 1994, a review. A gap that a person perceives in their knowledge can cause curiosity.

- Supports, as a hypothesis: show a little of a locked mechanic. That is stronger than the full tech tree, and
  stronger than no hint.
- Checks: R-9.

### F-8 Accelerating numbers defy intuition (grade T)

Deterding, Andersen, Kiverstein and Miller, 2022, a theory paper that discusses a known cookie game. Exponential
production can exceed intuitive expectation again and again. This is a proposed explanation, not a causal proof.

- Supports, as a hypothesis: show the acceleration and the large effect of multipliers that interact. Do not show
  raw totals only.
- Checks: R-14.

## 4. The counter-evidence

Mekler and colleagues, 2017 (grade E). Points, levels and leaderboards raised the quantity of work in a task. They did
not significantly raise intrinsic motivation or competence satisfaction.

What it means for a review:

1. More clicks and longer sessions are not a better experience. Do not use them alone as the measure of a change.
2. A layer of points, badges or levels on a solved loop does not open the loop. Do not propose it as the fix of R-1.
3. Measure voluntary return and reported satisfaction together with time played (R-18).

## 5. Risk, reward and coercion

Kahneman and Tversky, 1979 (grade E for the general effect). People do not judge gains, losses and uncertain outcomes
symmetrically. A loss weighs more than an equal gain.

In an incremental game the most productive risk is not the loss of hours of progress. It is **opportunity cost**: the
player invests in 1 attractive option and gives up another for a time.

| Good strategic uncertainty | Coercive loop |
| --- | --- |
| The player asks "which route is stronger?" | The player says "I have to keep playing." |
| Limited slots | Punitive near-misses |
| Future offers that are not certain | Artificial time pressure |
| Investments that compete | Scarcity that is false or that the game hides |
| Combinations to discover | Random loss of progress |
| A configuration that the player can reverse | Progress timed to demand attention, or a penalty for a pause |

A review proposes only from the left column. Each thing in the right column that the game has is a `REMOVE` finding
(R-17), and it ranks first.

The definition of success: a player ends a session with the feeling that they built something more capable. They
understand why their choices worked. They can see the world that they changed. They are curious about the next
system. The objective is voluntary engagement through mastery, discovery, clear progress and good decisions. It is not
maximum compulsion.

## 6. Sources

Research:

- Ryan, Rigby and Przybylski (2006), "The Motivational Pull of Video Games", Motivation and Emotion.
  https://selfdeterminationtheory.org/SDT/documents/2006_RyanRigbyPrzybylski_MandE.pdf
- Schultz, Dayan and Montague (1997), "A Neural Substrate of Prediction and Reward", Science.
  https://pubmed.ncbi.nlm.nih.gov/9054347/
- Kivetz, Urminsky and Zheng (2006), "The Goal-Gradient Hypothesis Resurrected", Journal of Marketing Research.
  https://doi.org/10.1509/jmkr.43.1.39
- Nagle, Wolf, Riener and Novak (2014), "The Use of Player-centered Positive Reinforcement to Schedule In-game
  Rewards", International Journal of Serious Games. https://doi.org/10.17083/ijsg.v1i4.47
- Johnson and colleagues (2018), "Greater Rewards in Videogames Lead to More Presence, Enjoyment and Effort".
- "The Effects of Juiciness in an Action RPG" (2020), Entertainment Computing.
  https://doi.org/10.1016/j.entcom.2020.100359
- Loewenstein (1994), "The Psychology of Curiosity: A Review and Reinterpretation", Psychological Bulletin.
  https://doi.org/10.1037/0033-2909.116.1.75
- Deterding, Andersen, Kiverstein and Miller (2022), "Mastering Uncertainty: A Predictive Processing Account of
  Enjoying Uncertain Success in Video Game Play". https://pmc.ncbi.nlm.nih.gov/articles/PMC9363017/
- Mekler and colleagues (2017), "Towards Understanding the Effects of Individual Gamification Elements on Intrinsic
  Motivation and Performance". https://doi.org/10.1016/j.chb.2015.08.048
- Kahneman and Tversky (1979), "Prospect Theory: An Analysis of Decision under Risk", Econometrica.

Games (grade D). The descriptions in `patterns.md` come from these pages. A mechanic can differ by edition, platform
and patch:

- Vampire Survivors: https://vampire.survivors.wiki/w/Level_up, https://vampire.survivors.wiki/w/Weapon,
  https://vampire.survivors.wiki/w/PowerUps
- (the) Gnorp Apologue: https://www.gamedeveloper.com/design/interview-the-gnorp-apologue,
  https://gnorp.wiki.gg/wiki/Compression, https://gnorp.wiki.gg/wiki/Talents
- Antimatter Dimensions: https://antimatterdimensions.wiki.gg/wiki/Time_Studies,
  https://antimatterdimensions.wiki.gg/wiki/Reality, https://antimatterdimensions.wiki.gg/wiki/Glyphs,
  https://antimatterdimensions.wiki.gg/wiki/Perks
- Cookie Clicker: https://cookieclicker.wiki.gg/wiki/Grimoire, https://cookieclicker.wiki.gg/wiki/Pantheon,
  https://cookieclicker.wiki.gg/wiki/Garden, https://cookieclicker.wiki.gg/wiki/Ascension
- Universal Paperclips: https://www.franklantz.net/work, https://www.decisionproblem.com/paperclips/
- A Dark Room: https://press.doublespeakgames.com/adr/index.html, https://adarkroom.doublespeakgames.com/
