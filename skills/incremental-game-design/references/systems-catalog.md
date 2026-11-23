# The catalog of systems for incremental games

This file lists the systems that incremental games use. It separates the core systems from the
optional systems. A designer selects a subset. The balance math (`references/balance-math.md`)
balances that subset. This file gives no proofs. It gives the formulas and the parameters that
the math and the skill use.

This file is the authority for the systems and their formulas. `assets/systems.json` holds the
same facts for scripts. The interview that selects a subset is in `references/interview.md`.

## Contents

1. How to read this file
2. The diagnosis that this catalog answers
3. Summary of the classification
4. The cards: S01 to S30, each with a variant table
5. Core and optional
6. Compatibility of the optional systems
7. Starter sets
8. Decisions

## How to read this file

A **system** is a part of a game with its own state, its own player decision, and its own
formulas. A formula alone is not a system. A softcap, for example, is a formula family inside a
system card (S04, S13).

Source prefixes. A citation is `prefix:line`.

A file under `research/`, `docs/research/` or `tools/` is a research note of the skill's authors. It is not in the
skill. The citation stays as the record of where the number came from.

| Prefix | File |
| --- | --- |
| `O` | `references/other-games-math.md` |
| `A` | `research/other-games/a_cookie_ad_synergism.md` |
| `B` | `research/other-games/b_realm_ngu_clickerheroes.md` |
| `C` | `research/other-games/c_kittens_trimps_evolve_melvor.md` |
| `D` | `research/other-games/d_exponential_others_general.md` |
| `P` | `references/reference-game-patterns.md` |
| `M` | `references/reference-game-math.md` |
| `X` | `references/system-archetypes.md` |
| `TA`, `TB` | `research/tools_a.md`, `tools_b.md` |
| `SA` | `docs/research/skill-audit.md` on branch `fleet/display-settings` |
| `IA` | `docs/research/moss-works-idle-vs-active.md` on branch `fleet/display-settings` |
| `RP` | `docs/research/reference-game-pacing.md` on branch `fleet/display-settings` |
| `SG` | `references/short-games.md` of this skill: 11 short games for continuous play, with the marks S, M and I of that file |

Conventions:

- "Reference game" means Idle Obelisk Miner. "IOM" in an evidence cell is the same game.
- A game name appears only as evidence. The system names are neutral.
- **(inference)** marks a claim that no source states directly.
- **(computed)** marks a number from `tools/skill-v2/catalog/r_values.py`. That script computes
  `r` and the earnings factor for a doubled reset gain.
- `r = log(value ratio) / log(cost ratio)` for each level, as `O:184-189` defines it. A line with
  linear value for each level and geometric cost has `r = 0`: its value grows as the log of the
  spend (inference from `O:184-189` with a value ratio that goes to 1).
- "Single source" marks a system with evidence in fewer than 2 games.
- Presence has 3 columns: **active** (the player gives input), **open** (the game runs with no
  input), and **closed** (the game is not running).
- `BM` is the prefix for `references/balance-math.md` of this skill. `BM L6` names a law of that
  file. The symbols `r`, `R`, `p`, `q`, `lambda`, `f_closed` and `T_cap` have the meanings of
  that file.

The variant tables. Each card ends with a table of its formula families. The table has 1 row
for each family, and `assets/systems.json` has the same rows.

- **Kind.** A `variant` is a choice in the interview. A `modifier` is a rule that the designer
  can add to a variant. A `helper` is a derived formula or a planning rule.
- **Parameters.** Each parameter has its proven range, its default and its source. A single
  value is a range with 1 point: 1 game gives it. A default is always a sourced value inside the
  range.
- **No sourced range.** The sources give no number for the family. The design solver uses only
  the families with a sourced range.
- **`r`.** The line exponent of the family (`BM D5`). `0` means that the family adds no
  exponent to `R`. "Not covered by L1" means that the laws of the balance math do not compute it,
  and a simulation must check it (`BM` section 7).
- **Gives.** The proof class of the variant: what the designer gets when the selection holds it.
  `proved`: a law covers the part, and the solver writes it. `simulated`: a tool writes the part (or
  the model does not need it), and only the simulator checks it; the result is marked "simulated, not
  proved". `none`: no tool writes the part yet; the model has a stub or leaves the part out. The first
  class holds when the rest of the selection is proved. "Simulation path: x" is the class when another
  choice takes the simulation path (`references/simulation-path.md`). A run of the solver and of the
  scaffold finds each class (`tests/test_interview_honesty.py`), and `assets/systems.json` holds it as
  `proof_class` and `proof_class_sim_path`.

## The diagnosis that this catalog answers

- The first version of the skill had no active action, no offline rule with a cap, no bank with a
  cap, no spine that needs a visit, and no real-time clock as a system (`SA:6`, `SA:8`, `SA:10`).
- The Moss Works build has no income that only active play gets (`IA:15`). A visit each 24 hours
  finishes the game with 12 active minutes. Active greedy play needs 375 (`IA:8`).
- A global time scale multiplies all clocks, so x100 finishes the game in 3.75 real minutes
  (`IA:13`). A stage clear costs no time (`IA:14`).
- The 8-hour offline cap does not limit a game that stays open (`IA:16`).
- The reference game pays 85% to 92% offline, caps offline at 12 hours, and moves its spine only
  by fights that need a visit (`RP:9-11`, `RP:16`).

## Summary of the classification

| Id | System | Class | Presence role |
| --- | --- | --- | --- |
| S01 | Generator line | Core | - |
| S02 | Multiplier upgrade | Core | - |
| S03 | Spine with a wall | Core | - |
| S04 | Reset layer | Core (decision D1). Optional for `continuous` (decision D10) | Needs a visit to cash |
| S05 | Active action | Core | Active premium |
| S06 | Offline rule | Core. Optional for `continuous` (decision D9) | Closed-game rule |
| S07 | Attempt on a cooldown | Core slot "presence gate" (1 of S07, S08, S09). The slot is optional for `continuous` (decision D9) | Spine needs a visit |
| S08 | Bank with a cap | Core slot "presence gate" | Bank with a cap |
| S09 | Real-time clock | Core slot "presence gate" | Real-time clock |
| S10 | Random visitor | Optional, can fill the S05 slot (D4) | Active pulse |
| S11 | Timed buff | Optional | Active bonus |
| S12 | Milestone multiplier on count | Optional | - |
| S13 | Cap and cap raiser | Optional | - |
| S14 | Hold bonus | Optional | Grows while idle |
| S15 | Prestige shop | Optional | - |
| S16 | Second reset layer | Optional | Needs a visit |
| S17 | Challenge run | Optional | - |
| S18 | Chained producers | Optional (decision D7) | Grows as a power of idle time |
| S19 | Collection with set bonus | Optional | - |
| S20 | Task list with a shop | Optional | - |
| S21 | Allocation pool | Optional | - |
| S22 | Skill track | Optional | - |
| S23 | Minigame with export | Optional | Has its own clock |
| S24 | Automation unlock | Optional. Core for `continuous` (decision D10) | Moves active work to open idle |
| S25 | Rotating event | Optional | Real-time clock |
| S26 | Premium currency with many faucets | Optional | Faucets bank |
| S27 | Line rebirth | Optional | - |
| S28 | Hard end | Optional. Core for `continuous` (decision D10) | - |
| S29 | Run on a limit | Optional | The run needs input |
| S30 | Upkeep | Optional | Active: the player buys care |

Core count for `check_in`: 6 systems and 1 core slot. The slot needs 1 or more of S07, S08 and S09.
Optional count: 24 systems, including the 3 slot systems when a second one is added.

Core count for `continuous`: 6 systems (S01, S02, S03, S05, S24, S28). S04, S06 and the slot are
optional. Section "Core and optional" gives the reasons (decisions D9 and D10).

## The cards

### S01 Generator line

| Field | Content |
| --- | --- |
| What it is | A purchase that adds a fixed income for each unit owned. Each unit costs more than the unit before it. |
| Decision | Which line to buy next. When to stop the purchase of an old line. |
| Evidence | Cookie Clicker `A:36`. AdVenture Capitalist `D:296-297`. Kittens Game `C:40`. Idle Wizard `C:255`. Synergism `A:201`. Universal Paperclips `D:196-197`. Trimps `C:103`. Evolve `C:159`. IOM workshop `M:93-95`. Antimatter Dimensions `A:115-116`. |
| Formula families | Cost `base x ratio^owned`, ratio 1.07 to 1.15 for generators (`O:31`, `O:48`). Ratio 1.2 to 1.3 (Trimps, Evolve, `O:39-41`). Ratio 2.5 for housing (`O:40`). A ladder 1.16 to 1.19, one ratio for each tier (`C:255`). Income `base_rate x owned x multipliers` (`D:297`). Bulk cost `base x ratio^k x (ratio^n - 1) / (ratio - 1)` and max affordable `floor(log_ratio(c x (ratio - 1) / (base x ratio^k) + 1))` (`O:53-54`). Polynomial cost `(n + 1)^2.25 x 1e6` in a later era (`D:202`). Ratio floor 1.005 (`O:52`). |
| Clock | Game time. |
| Presence | Pays in all 3 modes at the same rate unless S06 changes the closed rate. Production needs no visit. A purchase needs a visit or S24. |
| Links | Needs a currency. Pays income to every sink. Gets multipliers from S02, S12 and S15. |
| Growth role | Source. `r = 0` for 1 line alone (inference, see conventions). |
| Balance risks | A line with a low ratio dominates (`O:193-194`). "If optimal buying always picks the top tier, balance is off" (`D:340`). A ratio that a reduction can push to 1 makes the line free (`O:52`). |
| Scope | S. UI: a row with count, cost, a buy button and bulk buy. |
| Signature in code | A `count` or `owned` field. `base_cost * pow(ratio, count)`. An income sum of `rate * count`. |

Variants of S01:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S01.geometric` | variant | `cost a x b^owned; income v x owned x multipliers` | `b` 1.07 to 1.15, default 1.15 (`O:48-49`) | Cookie Clicker, Clicker Heroes, Kittens Game | `0`: additive value for each unit | proved |
| `S01.steep` | variant | `cost a x b^owned; income v x owned x multipliers` | `b` 1.2 to 1.3, default 1.2 (`O:39-41`) | Trimps, Evolve | `0`: additive value for each unit | proved |
| `S01.ladder` | variant | `cost a_i x b_i^owned, 1 ratio b_i for each tier i` | `b_i` 1.16 to 1.19, default 1.16 (`C:255`) | Idle Wizard | `0`: additive value for each unit | proved |
| `S01.polynomial` | variant | `cost (n + 1)^k x 1e6 in a later era` | `k` 2.25, default 2.25 (`D:202`) | Universal Paperclips | not covered by L1: polynomial cost; balance-math limit 4 | simulated |
| `S01.ratio_floor` | modifier | `b >= b_floor after each reduction` | `b_floor` 1.005, default 1.005 (`O:52`) | Evolve | `0`: a floor keeps the line from being free | - |
| `S01.bulk` | helper | `bulk cost a x b^k x (b^n - 1) / (b - 1); max affordable floor(log_b(c x (b - 1) / (a x b^k) + 1))` | - | genre | `0`: derived from the line parameters (O:53-54) | - |

### S02 Multiplier upgrade

| Field | Content |
| --- | --- |
| What it is | A purchase that multiplies the output of 1 line or of all income. Its effect joins an additive group or opens a new multiplicative group. |
| Decision | "Which +x% is cheapest now" (`M:69-70`). |
| Evidence | Cookie Clicker `A:46-50`. Universal Paperclips `D:205-208`. Realm Grinder `B:56-58`. Melvor Idle `C:218`. NGU Idle `B:146`, `B:190`. Evolve `C:160`. IOM `M:57-60`. AdVenture Capitalist `D:301`. Idle Slayer `D:104-105`. |
| Formula families | Groups `base x (1 + sum of group 1) x (1 + sum of group 2)` (`O:121`). A published order `(A_base x M_base x M_temp + A_temp) x M_illus + A_illus` (`O:120`). A levelled line: cost `a x b^L`, value `c x d^L` (`O:184`). Linear value (+3%, +0.1x) against cost ratio 1.2 to 1.45 (`M:107-111`, `M:292`). Marketing: value x1.1 for cost x2 (`D:195`). |
| Clock | None. It is a purchase. |
| Presence | A passive multiplier. The same in all 3 modes. |
| Links | Needs a currency. Multiplies S01 or another source. |
| Growth role | Multiplier in an old group (an additive term) or in a new group (a factor). `r` = 0.14 for x1.1 value at x2 cost, 0.30 for x2 at x10, 0.68 for x1.1 at x1.15 (`O:188`, computed). `r = 0` for linear value (inference). Factors add their exponents. Terms in a sum keep only the largest exponent (`O:189`). |
| Balance risks | `r` near 1 gives a line that never slows. Example: a prestige line at cost x2.0 and value x2.0 with no cap (`SA:11`). All damage sources multiply in Legends of IdleOn, and its numbers explode (`O:127`, `C:293`). A hidden softcap stops the player from planning (`O:155-156`). |
| Scope | S. UI: an upgrade list. Show the effect as "+N%" for a pool term and "xN" for a factor (`M:71-73`). |
| Signature in code | A `mult` or `bonus_pct` field. A product over groups. An effect `pow(d, level)`. |

Variants of S02:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S02.add_in_group` | variant | `base x (1 + sum of group 1) x (1 + sum of group 2); line cost a x b^L, value +v for each level` | `b` 1.2 to 1.5, default 1.2 (`M:107-108`); `v` 0.03 to 0.1, default 0.03 (`M:109`) | IOM, Cookie Clicker | `0`: linear value against geometric cost (M:109-111) | simulated |
| `S02.levelled_factor` | variant | `line cost a x b^L, value d^L, in a new group` | `b` 1.15 to 10, default 2 (`O:188, D:195`); `d` 1.1 to 2, default 1.1 (`O:188, D:195`) | Universal Paperclips, genre | `ln(d) / ln(b)`: 0.14 at d 1.1 and b 2; 0.30 at d 2 and b 10; 0.68 at d 1.1 and b 1.15 (computed) | proved |
| `S02.multiply_all` | variant | `each line is its own group: power = base x product of all factors` | - | Legends of IdleOn | `sum of r over all lines`: factors add their exponents (O:189); numbers explode (O:127) | none |
| `S02.published_order` | helper | `(A_base x M_base x M_temp + A_temp) x M_illus + A_illus` | - | Realm Grinder | largest r in each sum: O:120 | - |

### S03 Spine with a wall

| Field | Content |
| --- | --- |
| What it is | One number measures progress: a stage, a zone or a main level. Each step needs a requirement that grows faster than the base power of the player. |
| Decision | Push the next step, or build power first. Where to end the run. |
| Evidence | IOM `M:29-38`, `P:50-58`. Trimps `C:106`. Clicker Heroes `B:247-249` (search snippet). Realm Grinder `B:36-38`. Antimatter Dimensions `O:165-167`. Melvor Idle `O:176-178`. Moss Works `IA:14`. |
| Formula families | Requirement `base x g^step`: g = 2.8 (IOM, `M:31`), about 2 (Trimps, `O:160-161`), 1.145 to 1.155 (Clicker Heroes, `B:248`, snippet), 100 for each reset step (Realm Grinder, `O:163-164`). Regime change: x9.5 armor and x30 health after level 60 (`M:31-32`). Polynomial then exponential: `(RP + 1)^6.5`, then `1e-30 x 1.033^(0.33 x RP)` (`O:168-169`). Linear, then quadratic, then exponential steps (`O:165-167`). XP that doubles each 7 levels (`O:176-178`). Polynomial base `(L + 1)(L + 2) / 2` (`M:37`). A threshold test plus a volume test (`M:43-46`). |
| Clock | Game time, unless S07 puts the step on real time. |
| Presence | A step that clears when a stat passes the requirement moves in all 3 modes. Moss Works clears stages with no time cost, so saved income becomes stages in 1 burst (`IA:14`). |
| Links | Needs power from S01, S02 and other systems. Pays a reward for each step, unlocks, and run depth to S04 (`P:52-55`). |
| Growth role | Wall and content gate. The spine level is about log(power) / log(g) (`RP:212-217`, an inference in that report). |
| Balance risks | A polynomial base cannot keep up, so other systems must supply power (`M:47-49`). A wall with nothing beside it is a lull (`P:162-163`). A spine on income alone lets x100 and open idle finish the game (`IA:13`). |
| Scope | M. UI: a step counter, a progress bar, and the requirement against the current value. |
| Signature in code | A `stage` or `level` field. `stage_hp(n) = base * pow(growth, n)`. A compare such as `power >= requirement`. |

Variants of S03:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S03.exponential` | variant | `requirement h_n = h_1 x g^(n - 1)` | `g` 1.145 to 2.8, default 2.8 (`M:31, O:160-161, B:248`) | IOM, Trimps, Clicker Heroes | `0`: the spine adds no power; its level is about ln(P) / ln(g) (RP:212-217) | proved; simulation path: simulated |
| `S03.regime_change` | variant | `h_n = h_1 x g1^(n - 1) up to step n_c, then ratio g2` | `g1` 2.8, default 2.8 (`M:31`); `g2` 30, default 30 (`M:31-32`); `n_c` 60, default 60 (`M:31-32`) | IOM | `0`: the spine adds no power | none |
| `S03.poly_then_exp` | variant | `(RP + 1)^e up to RP_c, then 1e-30 x g^(0.33 x RP)` | `e` 6.5, default 6.5 (`O:168-169`); `g` 1.033, default 1.033 (`O:168-169`); `RP_c` 10000, default 10000 (`O:168-169`) | Realm Grinder | `0`: the spine adds no power | none |
| `S03.piecewise` | variant | `step +s each, then +2 more for each step past n1, then +0.2% of the total for each step past n2` | `s` 60, default 60 (`O:165-167`); `n1` 100, default 100 (`O:165-167`); `n2` 800, default 800 (`O:165-167`) | Antimatter Dimensions | `0`: the spine adds no power | none |
| `S03.xp_doubling` | variant | `requirement doubles each N levels` | `N` 7, default 7 (`O:176-178`) | Melvor Idle | `0`: the spine adds no power | none |
| `S03.threshold_volume` | helper | `a threshold test (stat >= armor) plus a volume test (damage over time >= health)` | - | IOM | `0`: M:43-46 | - |
| `S03.polynomial_base` | helper | `power base (L + 1)(L + 2) / 2` | - | IOM | `0`: M:37; other systems must supply power (M:47-49) | - |

### S04 Reset layer

| Field | Content |
| --- | --- |
| What it is | The player gives up run progress for a currency that gives permanent power. The gain is a root or a log of the run earnings. |
| Decision | When to reset. |
| Evidence | Cookie Clicker `A:42-45`. AdVenture Capitalist `D:298-302`. Realm Grinder `B:34-35`. Idle Wizard `C:254`. Trimps `C:105`. Kittens Game `C:43-44`. Evolve `C:161`. Egg, Inc. `D:250`. Synergism `A:188-192`. Clicker Heroes `B:225`. IOM `M:232-239`. Melvor Idle has no reset (`C:189`). |
| Formula families | Gain `k x (earnings / scale)^p`: p = 0.5 (AdVenture Capitalist, Realm Grinder, Idle Wizard, `O:62-63`), 1/3 (Cookie Clicker, `O:64`), 0.21 (Egg, Inc., `O:65`), about 0.44 (IOM, `M:235-239`). 1 point for each unit above a threshold of 70 (`O:69`). `floor(5 x log10(total))` (`O:68`). Steps 1, 5, 10, 30 at story points (`O:70`). Gain cap `cap + overflow / (overflow + cap) x cap` (`O:143`). Effect: +1% to +10% for each point, additive (`O:75-77`). Reset rule: gain at +50% to +200% of held currency (`O:86-88`). Best multiplier `e^(z(y + 1) / (1 - x))`, about 2.7 (`O:92-95`). Threshold efficiency `1 - 70 / K` (`O:96-98`). |
| Clock | Player action. |
| Presence | Active: the player picks the moment. Open and closed: the uncashed gain grows. A visit is needed to cash it. IOM auto-reset does not run offline (`RP:12`). |
| Links | Needs run earnings (S01) and run depth (S03). Pays S15 or a direct multiplier on S01. |
| Growth role | Reset. With an additive effect for each point, the multiplier grows as `earnings^p` (inference from `O:75-77`). Earnings factor for 2x gain: 4 at p = 0.5, 8 at 1/3, 27 at 0.21, 4.8 at 0.44 (computed). |
| Balance risks | A patient reset gets 6,769 lifetime points where the greedy buyer gets 15 (`IA:12`). A shop line with `r = 1` and no cap turns the gain into a multiplier near the gain (`SA:11`). The first reset feels like a loss (`X:39`). |
| Scope | M. UI: a reset button with a gain preview, a confirmation, a count-up of the reward. |
| Signature in code | A `prestige_currency` field. `sqrt(run_total / k)` or `pow(run_total / k, p)`. A `reset()` that clears run state and keeps a permanent dictionary. |

Variants of S04:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S04.power` | variant | `G = (E / c)^p; effect +e for each point, additive` | `p` 0.21 to 0.5, default 0.5 (`O:62-65, M:235-239`); `e` 0.01 to 0.1, default 0.02 (`O:75-77`) | AdVenture Capitalist, Realm Grinder, Idle Wizard, Cookie Clicker, Egg Inc., IOM | `0`: the layer adds no line; it sets lambda = p q / (1 - R) (BM D12) | proved; simulation path: none |
| `S04.log` | variant | `G = floor(k x log10(total))` | `k` 5, default 5 (`O:68`) | Clicker Heroes | `0`: the source is a third layer; s is near 0, so no best reset exists (BM L6) | none |
| `S04.threshold` | variant | `1 point for each unit above a threshold K0; efficiency 1 - K0 / K` | `K0` 70, default 70 (`O:69, O:96-98`) | Kittens Game | `0`: the threshold sets the run length (O:69) | none |
| `S04.steps` | variant | `round(a(z - z0) + g^sqrt(a(z - z0))) x a multiplier that steps 1, 5, 10, 30 at story points` | `a` 1.35, default 1.35 (`O:70`); `z0` 19, default 19 (`O:70`); `g` 1.23, default 1.23 (`O:70`) | Trimps | `0`: sub-exponential in the zone z | none |
| `S04.power_of_log` | variant | `G = floor(m x 10^(log10(x) / 308 - 0.75)) (rebuilt)` | `root` 308, default 308 (`O:67`) | Antimatter Dimensions | `0`: about a 308th root for each layer (O:67) | none |
| `S04.none` | variant | `no reset layer` | - | Melvor Idle | `0`: permitted only when S22 is the spine (decision D1, C:189) | simulated |
| `S04.gain_cap` | modifier | `above the cap: cap + overflow / (overflow + cap) x cap` | - | Evolve | `0`: the gain can at most double (O:143) | - |
| `S04.reset_rule` | helper | `reset when G >= X x L` | `X` 0.5 to 2, default 1 (`O:86-88`) | genre | `0`: the rule gain at +50% to +200% of held currency | - |
| `S04.best_multiplier` | helper | `best mean reset multiplier e^(z(y + 1) / (1 - x))` | - | Exponential Idle | `0`: about 2.7 (O:92-95) | - |

### S05 Active action

| Field | Content |
| --- | --- |
| What it is | An input that only an active player can give, and that pays income or a bonus. Variants: a click for income, an active-only multiplier lane, a timing skill. |
| Decision | Where to put attention now. Which active bonus to use. |
| Evidence | Realm Grinder clicks `B:54`. Idle Slayer `D:107`, `D:121`. Clicker Heroes active route `B:235`. IOM star taps `RP:274` and active play x1.25 `TA:507`. Egg, Inc. boost windows `D:268`. Exponential Idle timing skill `A:358`. Moss Works has none (`IA:15`, `SA:10`). Short games: the manual action is the only income first, and automation replaces it later: Nodebuster (`SG:65-66`), Digseum (`SG:91-92`), Scritchy Scratchy (`SG:129`), Gnorp Apologue (`SG:155`), Shelldiver (`SG:82`). |
| Formula families | Mode factors: closed 0.85, open 1, active 1.25 (IOM, `TB:61`). Active multiplier lane: x100 souls for each kill (Idle Slayer, `D:107`). Click count into a chance: `clicks^0.25 %` (Realm Grinder, `B:54`). Reserve rule by route: `(L + 1)^2` idle, `(L + 1)^(5/2)` active (Clicker Heroes, `B:235`). A boost before the reset gain formula (Egg, Inc., `D:268`). A base click value: no sourced formula. |
| Clock | Player action. |
| Presence | Active only. Open and closed pay 0. |
| Links | Needs a target stat. Pays income or a factor. Can feed S11 and S04. |
| Growth role | Source (click) or multiplier in a new group (active lane). |
| Balance risks | With no active action, idle beats active by 31 to 1 for each active minute (`IA:8`, `IA:15`). A large active premium turns the game into a pure clicker (inference). The reference game keeps the premium near x1.5 between the extremes (`M:211-213`). |
| Scope | S to M. UI: a tap target, a visible bonus, feedback. |
| Signature in code | An input handler (`_input`, `_gui_input`). A `click_value`. An `active` flag that scales an effect. |

Variants of S05:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S05.active_lane` | variant | `income x a while active; x1 open; x f_closed closed` | `a` 1.25, default 1.25 (`TB:61, TA:507`) | IOM | `0`: a constant factor adds no exponent | proved |
| `S05.kill_lane` | variant | `a second currency x m for each active kill` | `m` 100, default 100 (`D:107`) | Idle Slayer | `0`: a constant factor adds no exponent | none |
| `S05.click_chance` | variant | `chance % = clicks^e` | `e` 0.25, default 0.25 (`B:54`) | Realm Grinder | `0`: the chance has no purchase line | none |
| `S05.route_reserve` | variant | `reserve (L + 1)^e_idle for the idle route, (L + 1)^e_active for the active route` | `e_idle` 2, default 2 (`B:235`); `e_active` 2.5, default 2.5 (`B:235`) | Clicker Heroes | `0`: the route has no purchase line | none |
| `S05.click_income` | variant | `a click pays a base value` | no sourced range | Realm Grinder, Cookie Clicker | `0`: no sourced formula for the base click value | none |
| `S05.timing_skill` | variant | `a timing skill pays a bonus` | no sourced range | Exponential Idle | `0`: A:358 names it with no formula | none |
| `S05.visitor` | variant | `S10 with no bank fills the slot (decision D4)` | - | Cookie Clicker | `0`: the parameters are those of S10.window | none |
| `S05.boost_window` | modifier | `a boost before the reset gain formula` | no sourced range | Egg Inc. | `0`: D:268 gives no constant | - |
| `S05.manual_then_automate` | variant | `phase 1: only the manual action pays, and open and closed pay 0; an S24 unlock replaces the manual action at a share z of the game` | `z` 0.5, default 0.5 (`SG:65-66`) | Nodebuster, Digseum, Scritchy Scratchy, Gnorp Apologue, Shelldiver | `0`: a phase change; the laws of BM do not cover it, so a simulation must check it (BM section 7) | simulated |

The default variant of S05 for the play pattern `continuous` is `S05.manual_then_automate`, with S24 as
core (decision D10). For `check_in`, the default stays `S05.active_lane`.

### S06 Offline rule

| Field | Content |
| --- | --- |
| What it is | The rule that pays progress for time with the game closed. It sets a rate penalty, a cap on paid time, and the systems that stop. |
| Decision | When to come back. What to set up before the player leaves (`P:271-272`). |
| Evidence | IOM `RP:9-10`, `P:267-277`. Melvor Idle `C:207`. Realm Grinder `B:52`. A design blog `D:362`. Moss Works build `SA:12`. |
| Formula families | `paid = min(away, cap) x penalty x rate` (`RP:303`). Penalty 0.85 to 0.92, cap 12 hours (24 paid) (`RP:9-10`). Decay table: 100% for hour 1, 70% for hours 2 to 6, then 50% (`D:362`). Cap 24 hours (`C:207`). A bonus `(ln(1 + x))^2.5 %` on offline production (`B:52`). Expected values replace random rolls (`P:269-273`). |
| Clock | Real time. |
| Presence | It defines the closed column. It does not limit an open game (`IA:16`). |
| Links | Reads every rate system. S08 banks fill in the same time. |
| Growth role | Cap on time, with a rate softcap. |
| Balance risks | 100% for 8 hours makes idle stronger than the reference allows (`SA:12`). Past the cap the share falls: 46% at 24 hours, 15% at 72 hours (`RP:10`). |
| Scope | S to M. UI: a return summary with the time away, the cap and the gain. |
| Signature in code | A saved time stamp. `min(now - saved, cap)`. A `penalty` factor. A signal such as `offline_progress_applied`. |

Variants of S06:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S06.penalty_cap` | variant | `paid = min(away, T_cap) x f_closed x rate` | `f_closed` 0.85 to 0.92, default 0.85 (`RP:9-10, RP:250-252`); `T_cap_h` 12 to 24, default 12 (`RP:9, C:207`) | IOM, Melvor Idle | `0`: a cap on time; past the cap the share falls as 1 / T (RP:303) | proved; simulation path: simulated |
| `S06.decay_table` | variant | `rate 100% for hour 1, 70% for hours 2 to 6, then 50%` | `f_h1` 1.0, default 1.0 (`D:362`); `f_h2_6` 0.7, default 0.7 (`D:362`); `f_after` 0.5, default 0.5 (`D:362`) | a design blog | `0`: no cap is stated; check PO9 | none |
| `S06.offline_bonus` | modifier | `bonus (ln(1 + x))^e % on offline production` | `e` 2.5, default 2.5 (`B:52`) | Realm Grinder | `0`: a rate factor | - |
| `S06.expected_values` | helper | `expected values replace random rolls offline` | - | IOM | `0`: P:269-273 | - |

### S07 Attempt on a cooldown

| Field | Content |
| --- | --- |
| What it is | A spine step is a short timed attempt that the player starts. Damage carries over, and a real-time cooldown limits the attempts. |
| Decision | When to attempt. What to build before the attempt. |
| Evidence | IOM `P:338-347`, `M:35`, `TA:803`. **Single source.** |
| Formula families | Attempts `ceil(health / (damage per second x fight length))` (`M:36`). Fight 30 seconds, cooldown 20 minutes (`M:35`, `M:221`). Time = attempts x 1200 s x cooldown multiplier / game speed (`TA:803`). Game speed cancels out of fight damage (`M:50`, `TA:821`). |
| Clock | Real time. |
| Presence | Active only. No source shows an offline fight (`RP:11`). The step needs a visit. |
| Links | Needs power. Pays spine progress (S03). |
| Growth role | Wall and clock. |
| Balance risks | A cooldown with nothing to do in it (`X:170`). |
| Scope | M. UI: an attempt button, a cooldown timer, a health bar that keeps the damage. |
| Signature in code | A `cooldown_until` time stamp. A `carried_damage` field. A `fight_seconds` constant. |

Variants of S07:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S07.cooldown_attempt` | variant | `attempts = ceil(health / (dps x fight_s)); 1 attempt each cooldown_s of real time` | `fight_s` 30, default 30 (`M:35`); `cooldown_s` 840 to 1200, default 1200 (`M:35, RP:38`) | IOM | `0`: a clock; stages for each visit <= ceil(V / C) (BM L5) | simulated; simulation path: none |
| `S07.time_to_clear` | helper | `time = attempts x 1200 s x cooldown multiplier / game speed` | - | IOM | `0`: TA:803; the sources differ on game speed (RP:371) | - |

### S08 Bank with a cap

| Field | Content |
| --- | --- |
| What it is | A timed reward or a resource stacks while the player is away, up to a cap. Upgrades raise the cap or shorten the timer. |
| Decision | When to check in. Which cap to raise. |
| Evidence | IOM `P:255-265`, `TB:382`, `TA:261`. Trimps storage `C:103`. |
| Formula families | Bank `min(bank + elapsed / period, cap)` (inference from `P:257-259`). Fill time `60 x cap / bugs per hour` (`TB:382`). Check-in interval = the shortest fill time of the banks (`TB:382`, `M:229-230`). Packs per hour `3600 / (cooldown / game speed)`, base cooldown 600 s (`TA:238`). Bank 2 at the start, up to 184 (`P:258-259`). Storage: an upgrade costs 1/4 of the current maximum and doubles the maximum (`C:103`). |
| Clock | Game time in IOM, because the cooldown divides by game speed (`TA:238`). |
| Presence | Fills in all 3 modes. Pays on a visit. Time past the cap pays 0. |
| Links | A cap raise is a reward from other systems (`X:149`). Feeds S26 faucets. |
| Growth role | Cap and clock. |
| Balance risks | A small cap turns the timer into a chore (`X:143`). A bank cap longer than the S06 cap gives a second offline income (inference). |
| Scope | S. UI: "n / cap", a fill timer, a claim button. |
| Signature in code | `bank`, `bank_cap` and `last_fill_time` fields. A `min(..., cap)` on fill. |

Variants of S08:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S08.timed_bank` | variant | `bank = min(bank + elapsed / period_s, cap) (inference from P:257-259)` | `period_s` 364 to 600, default 600 (`TA:238, RP:41`); `cap` 2 to 184, default 2 (`P:258-259`) | IOM | `0`: a cap: T_cap = cap x period_s (BM L5) | proved; simulation path: none |
| `S08.storage` | variant | `an upgrade costs share x the current maximum and multiplies the maximum by m` | `share` 0.25, default 0.25 (`C:103`); `m` 2, default 2 (`C:103`) | Trimps | `0`: a cap on stock | none |
| `S08.check_in` | helper | `fill time = 60 x cap / items per hour; check-in interval = shortest fill time` | - | IOM | `0`: TB:382, M:229-230 | - |

### S09 Real-time clock

| Field | Content |
| --- | --- |
| What it is | A system that moves on real time and ignores game speed. Examples are a minigame on wall-clock seconds and a daily cycle. |
| Decision | When to visit the clock. What to put on it. |
| Evidence | IOM `RP:13`, `P:125-128`, `M:217-227`. Cookie Clicker 24-hour cycle `A:63`. Melvor Idle speed-independent drops `C:219`. |
| Formula families | A 24-hour cycle. Each level gives +1% to its building (`A:63-64`). Drop chance `action seconds x level / 25,000,000`, so speed does not change drops for each hour (`O:215-216`). Targets move each 30 minutes of real time (`M:222`). |
| Clock | Real time. |
| Presence | Moves in all 3 modes when it banks. IOM banks Archaeology seconds while away (`RP:13`). A time scale cannot compress it. |
| Links | Pays permanent exports to the main loop (S23). |
| Growth role | Clock. |
| Balance risks | It feels like a chore if it punishes absence (`X:64`). A global time scale that also speeds it removes the clock (`IA:13`). |
| Scope | S to M. UI: a timer in real time. |
| Signature in code | Deltas from `Time.get_unix_time_from_system()`, not `delta * time_scale`. |

Variants of S09:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S09.daily_cycle` | variant | `a 24-hour cycle; each level gives +e to its building` | `period_h` 24, default 24 (`A:63`); `e` 0.01, default 0.01 (`A:63-64`) | Cookie Clicker | `0`: a clock | simulated; simulation path: none |
| `S09.speed_free_drop` | variant | `drop chance = action seconds x level / k` | `k` 25,000,000, default 25,000,000 (`O:215-216`) | Melvor Idle | `0`: speed does not change drops for each hour | simulated; simulation path: none |
| `S09.rotation` | variant | `targets move each period_s of real time` | `period_s` 1800, default 1800 (`M:222`) | IOM | `0`: a clock | simulated; simulation path: none |

### S10 Random visitor

| Field | Content |
| --- | --- |
| What it is | Something appears on screen on a timer. A tap gives a reward from a weighted table. |
| Decision | Tap now. Pay a little premium currency for a better reward (`P:281-283`). |
| Evidence | Cookie Clicker `A:55-58`. IOM `P:279-289`, `TB:366`. |
| Formula families | Spawn between 300 s and 900 s, chance for each frame `((t - Tmin) / (Tmax - Tmin))^5` (reconstructed, `A:55-56`). Reward `min(0.15 x bank, 900 x income) + 13` (`A:57`). A x7 burst for 77 s (`A:58`). Each 20 game minutes, banked (`M:220`). Expected value of a roll `1 + p x (k - 1)` (`M:168`). |
| Clock | Game time. |
| Presence | Active pays in full. IOM banks visitors for later (`M:220`). The fate of a missed visitor in Cookie Clicker has no source. |
| Links | Pays S11 buffs, currency and premium currency. |
| Growth role | Source in bursts. |
| Balance risks | It becomes a chore when it has no bank (`X:143`). |
| Scope | S. UI: an animated sprite, a tap, a reveal. |
| Signature in code | A `spawn_timer`. A weight table. `randf()` against weights. |

Variants of S10:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S10.window` | variant | `spawn between t_min and t_max, chance for each frame ((t - t_min) / (t_max - t_min))^5 (reconstructed); reward min(share x bank, k x income) + 13; no bank` | `t_min_s` 300, default 300 (`A:55-56`); `t_max_s` 900, default 900 (`A:55-56`); `share` 0.15, default 0.15 (`A:57`); `k_s` 900, default 900 (`A:57`) | Cookie Clicker | `0`: a burst source | none; simulation path: simulated |
| `S10.banked` | variant | `1 visitor each interval_s of game time, banked` | `interval_s` 1200, default 1200 (`M:220`) | IOM | `0`: a banked visitor also pays idle players, so it cannot fill S05 (decision D4) | none |
| `S10.expected_value` | helper | `expected value of a roll 1 + p x (k - 1)` | - | IOM | `0`: M:168 | - |

### S11 Timed buff

| Field | Content |
| --- | --- |
| What it is | A temporary multiplier with a duration. Uptime is the real stat. |
| Decision | When to use a buff. Which buffs to stack. |
| Evidence | Cookie Clicker `A:58`. IOM `P:291-299`, `TB:70-78`. Egg, Inc. `D:268`. Idle Slayer `D:107`. Realm Grinder spells `B:42-44`. |
| Formula families | Uptime `u = buff minutes per hour / 60` (`TB:78`). Rate `raw x u x k + raw x (1 - u)` (`TB:64-65`). Free and paid buffs share 1 budget of 60 minutes each hour (`P:295-296`, `M:194-195`). Spell cost `C^(1 + (0.6 - M) x T)` by tier (`B:43`). |
| Clock | Game time. |
| Presence | Active. IOM uses buffs offline if the options are on (`RP:260`). |
| Links | Fed by S10, S19 and S05. Multiplies S01. |
| Growth role | Multiplier in a new group, temporary. |
| Balance risks | Inventory work (`X:163`). Uptime near 100% removes the decision and the value of a paid buff (`P:299-300`). |
| Scope | S to M. UI: a buff bar with timers. |
| Signature in code | A `buff_until` time and a `buff_mult`. A list of active buffs. |

Variants of S11:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S11.uptime` | variant | `rate = raw x u x k + raw x (1 - u), u = buff minutes per hour / 60; free and paid buffs share 1 budget` | `k` 7, default 7 (`A:58`); `duration_s` 77, default 77 (`A:58`); `budget_min_per_h` 60, default 60 (`P:295-296`) | Cookie Clicker, IOM | `0`: a temporary factor (TB:64-65, TB:78) | none |
| `S11.spell_cost` | variant | `spell cost C^(1 + (c0 - M) x T) by tier T` | `c0` 0.6, default 0.6 (`B:43`) | Realm Grinder | `0`: a cost rule | none |

### S12 Milestone multiplier on count

| Field | Content |
| --- | --- |
| What it is | A line gets a multiplier at fixed counts owned. It keeps old lines worth a purchase. |
| Decision | Buy an old line up to its next step. |
| Evidence | AdVenture Capitalist `D:297`, `D:314`. Clicker Heroes `O:203`. NGU Idle `B:156-157`. Idle Wizard `B:388`. Antimatter Dimensions `A:117`. |
| Formula families | x2 at 25 and 50, then each 100 (`O:202-203`). x4 each 25 (`O:203`). x2 each 10 bought (`A:117`). x1.025 each 10 levels (`B:157`). x1.10 at 10 up to x1.50 at 100 (`B:388`). Stepwise (2, 10) acts as base 1.072 with a sawtooth (`O:190-191`). |
| Clock | None. |
| Presence | Passive. |
| Links | Needs S01. Pays S01. |
| Growth role | Multiplier in a new group. `r` = 0.41 for x2 each 25 at cost 1.07, 0.20 at cost 1.15, 0.82 for x4 each 25 at 1.07, 0.10 for x2 each 10 at x1e3 each 10 (computed). |
| Balance risks | A high `r` lets 1 line dominate (`O:193-194`). A level is worth more just after a step (`O:191`). |
| Scope | S. UI: progress to the next step on the line row. |
| Signature in code | `floor(count / 25)`. `pow(2, floor(count / 25))`. |

Variants of S12:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S12.count_steps` | variant | `x m for each n owned` | `m` 1.025 to 4, default 2 (`O:202-203, A:117, B:157`); `n` 10 to 100, default 25 (`O:202-203, A:117, B:157`) | AdVenture Capitalist, Clicker Heroes, Antimatter Dimensions, NGU Idle, Idle Wizard | `ln(m) / (n x ln(b))`: 0.41 for x2 each 25 at b 1.07; 0.82 for x4 each 25 at b 1.07 (computed) | none; simulation path: simulated |
| `S12.ladder` | variant | `x1.10 at 10 owned, rising to x1.50 at 100` | - | Idle Wizard | `ln(m) / (n x ln(b))`: B:388 | none |

### S13 Cap and cap raiser

| Field | Content |
| --- | --- |
| What it is | A line or a stat has a maximum. A later or distant system raises the maximum. |
| Decision | Which system to push to open more levels. |
| Evidence | IOM `P:103-110`, `M:141-146`. Realm Grinder `B:59`. NGU Idle `B:114-116`. Evolve `C:163`. Kittens Game `C:44`. Melvor Idle `C:218`. |
| Formula families | Cap `30 + 5 x spine level` (`P:52-53`). Hard caps 80%, 95%, 100% (`O:150`). Asymptote 200% (`O:142`). Held cap `250 + second currency` (`O:144`). Triangular root `(sqrt(1 + 8x / k) - 1) / 2` (`O:141`). Marginal-rate table (`O:146`). Self-limiting state `dq/dt = 8 q1 q2 / (1 + q)` (`O:152`). Rate cap 50 levels each second (`O:149`). Hard cap 25 levels (`B:59`). |
| Clock | None. |
| Presence | Passive. |
| Links | Raised by far systems. One cap can need progress in 3 or 4 systems (`M:142-145`). Pays new levels in old lines. |
| Growth role | Cap or softcap. `r = 0` above a hard cap. |
| Balance risks | A hidden softcap stops the player from planning (`O:155-156`). A cap with no raiser ends a system (`P:110`). |
| Scope | S. It needs no new screen (`X:94`). UI: "level / cap" and the source of the next raise. |
| Signature in code | `min(level, cap)`. A `max_level` from a sum of sources. |

Variants of S13:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S13.raised_cap` | variant | `cap = c0 + c1 x spine level` | `c0` 30, default 30 (`P:52-53`); `c1` 5, default 5 (`P:52-53`) | IOM | `0`: r = 0 above the cap | none; simulation path: simulated |
| `S13.hard_cap` | variant | `level <= cap; a percentage <= pct_cap` | `cap` 25, default 25 (`B:59`); `pct_cap` 0.8 to 1.0, default 0.95 (`O:150`) | Realm Grinder, Melvor Idle | `0`: r = 0 above the cap | none |
| `S13.asymptote` | variant | `bonus rises to a limit; triangular root (sqrt(1 + 8x / k) - 1) / 2` | `limit` 2.0, default 2.0 (`O:142`) | Kittens Game | `0`: double the input gives about x1.41 (O:141) | none |
| `S13.held_cap` | variant | `cap = c0 + second currency` | `c0` 250, default 250 (`O:144`) | Evolve | `0`: a second currency lifts the cap of the first | none |
| `S13.rate_table` | variant | `marginal-rate table: 100% of the first block, 50% of the next, 40% of the next` | - | Synergism | `0`: O:146 | none |
| `S13.self_limiting` | variant | `dq/dt = 8 q1 q2 / (1 + q)` | - | Exponential Idle | `0`: O:152 | none |
| `S13.rate_cap` | variant | `at most k levels each second` | `k` 50, default 50 (`O:149`) | NGU Idle | `0`: more resource then gives nothing | none |

### S14 Hold bonus

| Field | Content |
| --- | --- |
| What it is | Unspent currency pays a bonus while the player holds it. A purchase lowers the bonus. |
| Decision | Spend now or hold. |
| Evidence | Clicker Heroes `B:234-235`. Idle Champions `B:300`, `B:315`. AdVenture Capitalist `D:299-300`, `D:316`. Cookie Clicker `A:57`, `A:65`. Melvor Idle `C:215`, `C:233`. |
| Formula families | +1% for each unspent unit (`B:300`). +1% for each unspent unit up to 100 (`A:65`). Reserve `(L + 1)^2` (`B:234-235`). Spend at most 20% for x2, up to 60% for x10 or more (`D:300`). Best bank 100 minutes of income (`A:57`, `O:221-222`). Checkpoints at 10, 25, 50 and 95% of a pool (`C:215`). |
| Clock | None. |
| Presence | Passive. It grows while idle if the held currency grows (inference). |
| Links | Needs a currency from S04 or S26. Competes with S15 for the same currency. |
| Growth role | Multiplier in a new group. `r = 1` in the held amount for an additive bonus for each unit (inference). |
| Balance risks | A long idle stock gives a large bonus (inference). |
| Scope | S. UI: show the bonus that a purchase removes. |
| Signature in code | A bonus term that reads the unspent `currency`, not `total_earned`. |

Variants of S14:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S14.per_unit` | variant | `+e for each unspent unit, up to n_max units` | `e` 0.01 to 0.02, default 0.01 (`B:300, D:299`); `n_max` 100, default 100 (`A:65`) | Idle Champions, AdVenture Capitalist, Cookie Clicker | `1`: in the held amount, for an additive bonus for each unit (inference) | none |
| `S14.reserve` | variant | `reserve (L + 1)^e` | `e` 2, default 2 (`B:234-235`) | Clicker Heroes | `0`: a reserve rule | none |
| `S14.checkpoints` | variant | `bonuses at 10, 25, 50 and 95% of a pool` | - | Melvor Idle | `0`: C:215 | none |
| `S14.spend_limit` | helper | `spend at most 20% for x2, up to 60% for x10 or more; best bank 100 minutes of income` | - | AdVenture Capitalist, Cookie Clicker | `0`: D:300, A:57, O:221-222 | - |

### S15 Prestige shop

| Field | Content |
| --- | --- |
| What it is | A shop of permanent upgrades that the reset currency buys. |
| Decision | Which permanent upgrade to buy, by cost for each % of gain (`C:129`). |
| Evidence | Trimps `C:104`. IOM `M:240-241`. Clicker Heroes `B:230-233`. Kittens Game `C:41`, `C:67`. AdVenture Capitalist `D:300`. Egg, Inc. `D:267`. |
| Formula families | Cost `b x 1.3^l` (`C:104`). Steeper ratios 1.5, 2 and 5 to limit a perk (`C:104`). Ratios 1.25 to 1.35 with tier bases 25, 300, 10 thousand and 1.5 million (`M:92`, `M:240`). A cut of 1% to 2.25% on every price ratio (`O:50-51`). Best level `sqrt(souls / 5)` for cost n, `(souls / 5)^(2/5)` for cost n^1.5 (`O:211-212`). |
| Clock | Player action. |
| Presence | None. |
| Links | Needs S04. Pays any group. A ratio cut changes the cost of S01. |
| Growth role | Multiplier in a new group. `r = 0` for linear value at 1.3 (inference). A ratio cut from 1.15 to 1.07 gives about x2.07 more units for the same spend (computed from the rule at `C:67`). |
| Balance risks | A line at cost x2.0 and value x2.0 with no cap has `r = 1` (`SA:11`). Linear value against geometric cost gives each line a natural stop (`M:109-111`). |
| Scope | M. UI: a shop or a tree. |
| Signature in code | Purchases priced in the reset currency. A dictionary that survives `reset()`. |

Variants of S15:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S15.geometric` | variant | `cost a x b^l, linear value; a few perks at a steeper ratio b_limit` | `b` 1.25 to 1.35, default 1.3 (`C:104, M:92`); `b_limit` 1.5 to 5, default 2 (`C:104`) | Trimps, IOM | `0`: linear value (M:109-111) | proved; simulation path: none |
| `S15.ratio_cut` | variant | `each point cuts every price ratio by cut` | `cut` 0.01 to 0.0225, default 0.01 (`O:50-51`) | Kittens Game | `0`: 1.15 to 1.07 gives about x2.07 units for the same spend (computed from C:67) | none |
| `S15.best_level` | helper | `best level sqrt(souls / 5) for cost n; (souls / 5)^(2/5) for cost n^1.5` | - | Clicker Heroes | `0`: O:211-212 | - |

### S16 Second reset layer

| Field | Content |
| --- | --- |
| What it is | A deeper reset that also resets the first layer and pays a rarer currency. Each layer gain is a slower function of the layer below. |
| Decision | When to do the deep reset. |
| Evidence | Antimatter Dimensions `A:133-140`. Synergism `A:188-198`. Clicker Heroes `B:223-225`. Realm Grinder `B:36-48`. Evolve `C:161-164`. Idle Slayer `D:122`. |
| Formula families | Gain powers 0.5, then 0.03, then 0.01 (`O:66`). `floor(mult x 10^(log10(x) / 308 - 0.75))` (rebuilt, `O:67`). `floor(5 x log10(souls))` (`O:68`). Wall `1e27 x 100^R` with an offset at the next layer (`O:163-164`, `B:38`). Old bonuses compressed as `B^(0.1 / A)` (`B:47`, `O:145`). The same formula with new constants (`C:161`, `C:181`). |
| Clock | Player action. |
| Presence | Needs a visit. |
| Links | Needs S04. Pays rule changes, not only larger multipliers (`X:44`). |
| Growth role | Reset. A gain power of 0.03 needs about x1.1e10 earnings for 2x gain (computed). |
| Balance risks | It must protect the systems that the player built last (`X:45`). Each layer starts tiny (`O:66`). |
| Scope | M to L. UI: a second reset screen and its shop. |
| Signature in code | A second reset currency. Nested `reset()` functions. |

Variants of S16:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S16.layer_power` | variant | `G2 = (G1 total / c2)^p2` | `p2` 0.01 to 0.03, default 0.03 (`O:66`) | Synergism | not covered by L1: a second layer; L7 is a conjecture here (BM limit 7) | none |
| `S16.log` | variant | `G2 = floor(k x log10(souls))` | `k` 5, default 5 (`O:68`) | Clicker Heroes | not covered by L1: BM limit 7 | none |
| `S16.wall_offset` | variant | `wall 1e27 x g^R; the next layer subtracts an offset from R` | `g` 100, default 100 (`O:163-164, B:38`) | Realm Grinder | not covered by L1: BM limit 7 | none |
| `S16.compress` | modifier | `old bonuses become B^(c / A) at the new layer` | `c` 0.1, default 0.1 (`B:47, O:145`) | Realm Grinder | not covered by L1: BM limit 7 | - |

### S17 Challenge run

| Field | Content |
| --- | --- |
| What it is | A run under a changed rule. It pays a permanent reward on completion or by record. |
| Decision | Which challenge to run, and when. How far to push the record. |
| Evidence | Trimps `C:109`, `C:132`. Synergism `A:189`, `A:226`. Kittens Game `A:284`. NGU Idle `B:132-139`. The reference game does not use it (`X:131-133`). |
| Formula families | Record reward `5 x (floor(z / 100)^2 + floor(z / 100)) + (floor(z / 100) + 1) x floor((z mod 100) / 10)`, additive (`C:109`). Reset power `0.5 + completions / 100` (`A:189`). A softcap on the exponent `(product)^DR` (`A:209-210`). Boss base 2^n, then 1.5^n, then 1.2^n (`B:133-134`). Speed divided by fixed constants (`B:136-137`). |
| Clock | Player action. |
| Presence | Active to start. No source states how a challenge runs idle. |
| Links | Needs S04 to start a run. Pays the S04 exponent or a factor. |
| Growth role | Multiplier in a new group, or a change of an exponent. |
| Balance risks | A reward that progress requires turns a choice into a chore (`X:135`). |
| Scope | M. UI: a challenge list, the rule text, the record. |
| Signature in code | An `active_challenge` flag that changes constants. A `best_zone[challenge]` table. |

Variants of S17:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S17.record` | variant | `5 x (floor(z / 100)^2 + floor(z / 100)) + (floor(z / 100) + 1) x floor((z mod 100) / 10), additive` | - | Trimps | `0`: C:109 | none |
| `S17.exponent` | variant | `reset power p0 + completions x step` | `p0` 0.5, default 0.5 (`A:189`); `step` 0.01, default 0.01 (`A:189`) | Synergism | `0`: it raises p, so recheck lambda (BM L6) | none |
| `S17.boss_base` | variant | `boss requirement g^n, with g lower for later bosses` | `g` 1.2 to 2, default 2 (`B:133-134`) | NGU Idle | `0`: a wall | none |
| `S17.exponent_softcap` | modifier | `(product of multipliers)^DR` | no sourced range | Synergism | `DR x sum of r`: A:209-210 gives no DR value | - |
| `S17.speed_divider` | modifier | `speed divided by fixed constants` | no sourced range | NGU Idle | `0`: B:136-137 gives no constants | - |

### S18 Chained producers

| Field | Content |
| --- | --- |
| What it is | Tier n produces tier n - 1. Each tier adds 1 power of time to growth. |
| Decision | Which tier to buy. When the next tier is worth it. |
| Evidence | Antimatter Dimensions `A:110-113`. Exponential Idle `A:335`, `D:53`. A design article `D:331`. |
| Formula families | Tier 1 makes 1 unit each second for each owned. Other tiers make 0.1 (`A:111-112`). `x_i <- x_i + x_(i-1) x dt` (`A:340`). The series `x^n / n!` lags `e^x` (`D:331`). Cost x1e3 to x1e15 for each 10 bought (`A:116`). |
| Clock | Game time. |
| Presence | All 3 modes. Output grows as a power of elapsed time (`O:196-198`). |
| Links | Needs a currency. Replaces or extends S01. |
| Growth role | Source. A new tier changes the degree of growth, not only the rate (`O:197-198`). |
| Balance risks | A long idle gap pays as `t^n` (inference from `O:196-198`). |
| Scope | M. UI: a tier list with the rate of each tier. |
| Signature in code | An array of tiers. `amount[i] += amount[i + 1] * rate * dt`. |

Variants of S18:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S18.tier_chain` | variant | `x_i <- x_i + x_(i+1) x rate_i x dt; tier 1 rate 1, other tiers rate k; cost x c for each 10 bought` | `k` 0.1, default 0.1 (`A:111-112`); `c` 1000 to 1e15, default 1000 (`A:116`) | Antimatter Dimensions, Exponential Idle | not covered by L1: each tier adds 1 power of t (O:196-198, BM limit 3) | none |

### S19 Collection with set bonus

| Field | Content |
| --- | --- |
| What it is | Items that play gives, often by chance. Each item gives a small bonus. A full set or a higher tier gives a larger one. |
| Decision | Which item to chase. Which tier to upgrade. |
| Evidence | Cookie Clicker `A:52-53`. IOM `P:112-118`, `M:61`, `M:76-77`. Melvor Idle `C:219`. Idle Champions `B:295-298`. |
| Formula families | 4% for each achievement. Each upgrade multiplies `(1 + milk x factor)` (`A:52-53`). Terms "for each X owned" in the main stat (`M:61`). Gear `base x (1 + (ilvl - 1) x 0.004)`, raised to the stack count (`B:296-298`). Drop value `1 + p x (k - 1)`. Mean wait `1 / (p x rolls each hour)`. The unlucky 1 in 20 waits about `3 / p` (`M:168-170`). Chance independent of speed (`O:215-216`). |
| Clock | Player action or game time (rolls). |
| Presence | Drops in all 3 modes. IOM gives 1 roll for the full absence offline (`RP:259`). |
| Links | Fed by S10, S03 and S22. Pays S02 terms. Can raise S13 caps. |
| Growth role | Multiplier in a new group, and content gate. |
| Balance risks | Pure chance with no pity gives dry spells. Bound the wait (`M:177-179`, `X:80`). |
| Scope | M. UI: a collection grid, set progress, a drop reveal. |
| Signature in code | An `owned_items` dictionary. A set check. A weight table. |

Variants of S19:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S19.per_item` | variant | `+e for each item; each upgrade multiplies (1 + milk x factor)` | `e` 0.04, default 0.04 (`A:52-53`) | Cookie Clicker | `0`: a factor for each item | none |
| `S19.gear_level` | variant | `base x (1 + (ilvl - 1) x k), raised to the stack count` | `k` 0.004, default 0.004 (`B:296-298`) | Idle Champions | `0`: linear value for each level | none |
| `S19.drop_roll` | variant | `drop value 1 + p x (k - 1); mean wait 1 / (p x rolls each hour); the unlucky 1 in 20 waits about 3 / p` | no sourced range | IOM | `0`: M:168-170 gives the formulas, not a range for p | none |
| `S19.per_owned_term` | variant | `terms 'for each X owned' in the main stat` | no sourced range | IOM | `0`: M:61 gives no constant | none |

### S20 Task list with a shop

| Field | Content |
| --- | --- |
| What it is | A list of named tasks in different systems. Each task pays a point or a coin for a shop or a global bonus. |
| Decision | Which task to do next. Which reward to buy. |
| Evidence | IOM `P:327-336`. Cookie Clicker `A:52`. Archetype `X:122-129`. |
| Formula families | 4% for each achievement (`A:52`). 10 coins for each task, 100 tasks in 3 tiers (`P:329-332`). 1 completion point for each permanent unlock (`P:203-204`). |
| Clock | Player action. |
| Presence | Mostly active (inference). |
| Links | Points at all other systems. Pays S02 terms or shop rewards. The count can gate content (`P:332-333`). |
| Growth role | Content gate and multiplier. |
| Balance risks | A task that only says "wait longer" (`X:129`). |
| Scope | S to M. UI: counters for actions, a list screen, a small shop (`X:128`). |
| Signature in code | Counters for each action. A task table with thresholds. |

Variants of S20:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S20.task_coins` | variant | `c coins for each task; n tasks in t tiers; a tier opens when the tier before is done` | `c` 10, default 10 (`P:329-332`); `n` 100, default 100 (`P:329-332`); `t` 3, default 3 (`P:329-332`) | IOM | `0`: a content gate | none |
| `S20.percent_per_task` | variant | `+e for each achievement` | `e` 0.04, default 0.04 (`A:52`) | Cookie Clicker | `0`: a factor for each task | none |
| `S20.completion_point` | variant | `1 completion point for each permanent unlock` | - | IOM | `0`: P:203-204 | none |

### S21 Allocation pool

| Field | Content |
| --- | --- |
| What it is | The player divides a pooled resource across several sinks. Each sink turns its share into progress at a rate. |
| Decision | The split. |
| Evidence | NGU Idle `B:185-187`, `B:113`, `B:161-162`. Kittens Game `C:42`. |
| Formula families | Level N to N + 1 takes N + 1 times the base time (`B:113`). Speed as the 6th root of each resource (`O:148`, `B:161`). Speed as the square root of power (`B:151`). Speed for each slot 100%, 70%, 57%, 49.3% for 1 to 4 slots (`B:162`). A stable ratio 1 : 37,500 : 1 (`B:166`). Production with job output (`C:42`). |
| Clock | Game time. |
| Presence | All 3 modes. The split persists. |
| Links | Needs a pool. Pays each sink. |
| Growth role | Source or multiplier. Roots force a spread (`B:187`). |
| Balance risks | Players settle on 1 stable ratio, and the decision ends (inference from `B:187`). |
| Scope | M. UI: sliders or +/- buttons for each sink. |
| Signature in code | An `allocated[sink]` table. `pool - sum(allocated)`. `pow(alloc, 1.0 / 6)`. |

Variants of S21:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S21.root_speed` | variant | `speed = share^e for each sink` | `e` 0.1667 to 0.5, default 0.5 (`O:148, B:151, B:161`) | NGU Idle | `0`: no purchase line; roots force a spread (B:187) | none |
| `S21.level_time` | variant | `level N to N + 1 takes (N + 1) x base time` | - | NGU Idle | `0`: B:113 | none |
| `S21.slot_speed` | variant | `speed for each slot 100%, 70%, 57%, 49.3% for 1 to 4 slots` | - | NGU Idle | `0`: B:162 | none |

### S22 Skill track

| Field | Content |
| --- | --- |
| What it is | A level track that gains experience from actions. Levels open content and raise stats. |
| Decision | Which skill to train. |
| Evidence | Melvor Idle `C:213-215`. IOM `M:89`. Idle Wizard `B:382`. Legends of IdleOn `B:342`. |
| Formula families | `floor((L - 1 + 300 x 2^((L - 1) / 7)) / 4)` (`O:177`). Total `250 x (1.2^(L - 1) - 1)` (`M:89`). `1.09^(L - 1) x 1500` (`B:382`). A falling base `1.208 - min(0.164, 0.215 x lvl / (lvl + 100))` (`O:153`). |
| Clock | Player action or game time. |
| Presence | Melvor trains offline up to its 24-hour cap (`C:207`). |
| Links | Fed by actions in other systems. Pays content gates and stats. |
| Growth role | Content gate. Multiplier when levels pay a stat. |
| Balance risks | An exponential curve with no end. A falling base avoids it (`B:356`). |
| Scope | M. UI: a skill list with level and XP bar. |
| Signature in code | `xp`, `level_from_xp()`. Action handlers that add XP. |

Variants of S22:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S22.xp_doubling` | variant | `XP floor((L - 1 + 300 x 2^((L - 1) / N)) / 4)` | `N` 7, default 7 (`O:177`) | Melvor Idle | `0`: a content gate | none |
| `S22.geometric` | variant | `XP for level L = a x b^(L - 1)` | `b` 1.09 to 1.2, default 1.2 (`M:89, B:382`) | IOM, Idle Wizard | `0`: a content gate | none |
| `S22.falling_base` | variant | `base 1.208 - min(0.164, 0.215 x lvl / (lvl + 100))` | - | Legends of IdleOn | `0`: O:153; it avoids an exponential curve with no end (B:356) | none |

### S23 Minigame with export

| Field | Content |
| --- | --- |
| What it is | A game inside the game with its own currency, tree and pace. It pays permanent power into the main loop. |
| Decision | Where to spend attention. Which export to buy. |
| Evidence | IOM `P:120-134`. Exponential Idle `A:303`, `D:11`, `D:56`. |
| Formula families | Total export = product over the sub-games (`D:56`). Sub-game growth `d(rho)/dt = K x rho^x x t^y x rho_pub^z` (`D:45`). A small copy: enemy `4 + 7w`, currency `(w^2 + w) / 2`, cost x1.25 (`M:247-249`). |
| Clock | Its own clock. IOM runs some on real time (`P:125`). |
| Presence | Depends on its clock. |
| Links | Needs 1 or more links in. Gives 2 or more links out (`X:55-56`). |
| Growth role | Multiplier in a new group (the export). |
| Balance risks | A second game that the core loop does not need (`X:55`). Players skip it if the core gets the same value more cheaply (`X:52-53`). |
| Scope | L. UI: its own screen and input. |
| Signature in code | A separate state dictionary and currency. An export multiplier read by the main loop. |

Variants of S23:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S23.small_copy` | variant | `enemy 4 + 7w; currency (w^2 + w) / 2; cost ratio b` | `b` 1.25, default 1.25 (`M:247-249`) | IOM | `0`: the export is a factor | none |
| `S23.product_export` | variant | `total export = product over the sub-games` | - | Exponential Idle | `sum of r over the sub-games`: D:56 | none |
| `S23.subgame_growth` | variant | `d(rho)/dt = K x rho^x x t^y x rho_pub^z` | no sourced range | Exponential Idle | `x`: D:45 gives no range for x, y, z | none |

### S24 Automation unlock

| Field | Content |
| --- | --- |
| What it is | The game takes over an action that the player did by hand. It is a reward. |
| Decision | None at use. A purchase decision at the unlock. A rule setting for an automatic reset (`D:47`). |
| Evidence | IOM `P:189-197`. Exponential Idle `D:47-49`. AdVenture Capitalist managers `D:276` (named only). Short games: Shelldiver buildings (`SG:82`), Scritchy Scratchy machine and assistant (`SG:129`), Gnorp hires (`SG:155`). Nodebuster opens its first damage source with no cursor after about half the game (`SG:65-66`). |
| Formula families | Reset condition `timer(pt x d(ln(ln(db / b + 1))) < 1) > 3 x tr && db > b` (`D:48`). The value of automation: no sourced formula. |
| Clock | Game time. |
| Presence | Moves the automated action from active to open. IOM keeps auto-reset off offline and switches automation off in the spine fight (`P:193-195`, `RP:12`). |
| Links | Needs the manual system that it automates. With `S05.manual_then_automate`, it is the second phase of S05. |
| Growth role | Automation. |
| Balance risks | Automation that runs while closed removes the active premium (inference from `P:193-195`). |
| Scope | S. UI: a toggle and a first-time moment (`X:182`). |
| Signature in code | An `auto_*` flag that runs a buyer or a reset in `_process`. |

Variants of S24:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S24.auto_reset` | variant | `reset when timer(pt x d(ln(ln(db / b + 1))) < 1) > k x tr and db > b` | `k` 3, default 3 (`D:48`) | Exponential Idle | `0`: off while closed (decision D5) | simulated; simulation path: none |
| `S24.auto_buy` | variant | `an auto flag runs a buyer` | no sourced range | IOM, AdVenture Capitalist | `0`: no sourced formula for the value of automation | simulated |

### S25 Rotating event

| Field | Content |
| --- | --- |
| What it is | A timed layer with its own short ladder. Main-game power does not enter it, and its rewards go to the main game. |
| Decision | How far to push before the event ends. |
| Evidence | IOM `P:315-325`, `M:225`. Idle Champions `B:302`, `B:318`. |
| Formula families | 1 month in each 2 months (`M:225`). The small copy formulas (`M:247-249`). A concave conversion: 2 million to about 500 thousand, 24 million to about 730 thousand, 57.4 million to 833 thousand (`B:302`). |
| Clock | Real time. |
| Presence | Active. The event ends on real time. |
| Links | Pays the main game. Rewards scale with main progress (`P:320-321`). |
| Growth role | Clock and content. |
| Balance risks | It needs live operation (`X:194`). |
| Scope | L. UI: an event screen, dates, its currencies. |
| Signature in code | Start and end dates. A separate currency set. |

Variants of S25:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S25.periodic` | variant | `an event of on_days in each period_days, with the small copy formulas` | `on_days` 30, default 30 (`M:225`); `period_days` 60, default 60 (`M:225`) | IOM | `0`: a clock | none |
| `S25.concave_conversion` | helper | `event points to rewards: 2 million to about 500 thousand, 24 million to about 730 thousand, 57.4 million to 833 thousand` | - | Idle Champions | `0`: B:302 | - |

### S26 Premium currency with many faucets

| Field | Content |
| --- | --- |
| What it is | A slow second currency from many small sources. Each source has its own upgrade stats, and the currency buys permanent unlocks. |
| Decision | Which faucet to upgrade. Which sink to save for. Which self-funding purchase to make. |
| Evidence | IOM `P:220-253`, `M:184-207`, `TB:343-366`. Egg, Inc. `D:248-249`, `D:265`. |
| Formula families | Net each hour = sum of faucets - sinks (`TB:346-354`). `EB% = SE x (10 + SF) x (1 + (5 + PB) / 100)^PE` (`D:249`). Flat or linear prices (`M:85-87`, `M:112`). A break-even that moves: -63 an hour at grade 0, +291 an hour at grade 45 (`M:190-193`). |
| Clock | Game time. Faucets run on timers. |
| Presence | Faucets bank (S08). |
| Links | Fed by S08, S10, S03 rewards. Pays permanent unlocks and S14 if held. |
| Growth role | Source of a second currency. The rare currency compounds as `(1.05)^PE` (`D:265`). |
| Balance risks | A gap above x10 with no sink (`P:252-253`). Paid items must be a higher rate on a free stat (`P:361-364`). |
| Scope | M to L. UI: a currency display, a shop, a faucet list. |
| Signature in code | A `gems` field with many `+=` sites. Flat or linear price tables. |

Variants of S26:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S26.faucet_sum` | variant | `net each hour = sum of faucets - sinks; flat or linear prices` | no sourced range | IOM | `0`: TB:346-354 and M:85-87 give no price range | none |
| `S26.compounding` | variant | `EB% = SE x (10 + SF) x (1 + (5 + PB) / 100)^PE` | - | Egg Inc. | `0`: the rare currency compounds as 1.05^PE (D:249, D:265) | none |
| `S26.break_even` | helper | `a break-even that moves: -63 an hour at grade 0, +291 an hour at grade 45` | - | IOM | `0`: M:190-193 | - |

### S27 Line rebirth

| Field | Content |
| --- | --- |
| What it is | A reset of 1 line inside a run. The line level goes back to 1 for a permanent change to that line. |
| Decision | Rebirth this line now, or keep the levels. |
| Evidence | Trimps `C:103`, `C:130`. Revolution Idle `D:149`, `D:168`. |
| Formula families | Cost `base x 1.75^G x 1.4^L`. Each G gives 20% more capacity (`C:103`). Gain for each step x10 and +10 maximum levels (`D:149`). |
| Clock | Player action. |
| Presence | Active. |
| Links | Needs S01. Pays S01. |
| Growth role | A local reset and a multiplier. |
| Balance risks | No sourced example. |
| Scope | S. UI: a rebirth button on the line row. |
| Signature in code | A `rebirths` counter for each line in its cost and value. |

Variants of S27:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S27.rebirth_cost` | variant | `cost a x g^G x b^L; each G gives +k capacity` | `g` 1.75, default 1.75 (`C:103`); `b` 1.4, default 1.4 (`C:103`); `k` 0.2, default 0.2 (`C:103`) | Trimps | `0`: a local reset | none |
| `S27.step_gain` | variant | `each step: gain x m and +n maximum levels` | `m` 10, default 10 (`D:149`); `n` 10, default 10 (`D:149`) | Revolution Idle | `0`: a local reset | none |

### S28 Hard end

| Field | Content |
| --- | --- |
| What it is | A last goal that ends the game: the last step of the spine, a last purchase, or the full tree. The game states the end. |
| Decision | When to push for the end. |
| Evidence | 10 of 10 short games with structure data have a hard end (`SG:186`). Nodebuster: the godvirus for 1,000 netcoin (`SG:69-70`). Shelldiver: each node of the tree (`SG:84`). Tower Wizard: "Reach the ending" (`SG:122`). Spaceplan: "a finite scenario" (`SG:139`). Paperclips: "a very definitive conclusion" (`SG:149`). |
| Formula families | Time of the active player to the end: 2 to 10 hours, the middle 3 to 5 hours (`SG:177`). The cost of a last goal: no sourced formula (`SG:205`). |
| Clock | Player action. |
| Presence | Active. The proof sheet measures the time to the end with the active player of the simulator (row PC1 of `references/proof-sheet.md`). |
| Links | Needs S03 (the last step) or a last purchase line. A post-game mode can follow: a time trial with 20 talents in the Gnorp Apologue (`SG:158`). |
| Growth role | End. |
| Balance risks | Players of a short game "were disappointed it was short" (`SG:164`). "Late game matters even in short games" (`SG:167`). A long wall of the same content before the end loses players (`SG:185`). |
| Scope | S. UI: an end screen, the credits, an optional post-game mode. |
| Signature in code | A `game_won` or `ending` flag. A last stage or a last purchase that sets it. An end scene. |

Variants of S28:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S28.last_step` | variant | `the game ends at the last step N of the spine S03` | - | Tower Wizard, Spaceplan | `0`: N comes from BM L8 and the length target | proved; simulation path: simulated |
| `S28.last_purchase` | variant | `a last purchase of cost K ends the game` | no sourced range | Nodebuster, Universal Paperclips | `0`: 1 game gives 1 cost in its own currency (SG:69) | none |
| `S28.full_tree` | variant | `the game ends when each node of the tree is bought` | - | Shelldiver | `0`: the end is the sum of the tree costs | none |
| `S28.time_to_end` | helper | `hours of the active player to the hard end` | `T_end_h` 2 to 10, default 4 (`SG:54, SG:177`) | genre | `0`: proof row PC1 | - |

### S29 Run on a limit

| Field | Content |
| --- | --- |
| What it is | A short attempt that ends at a limit: health, air, battery or a full inventory. The player brings back the run currency and spends it between runs on a permanent tree. The tree is not reset. |
| Decision | When to end the run. Which tree node to buy. |
| Evidence | Nodebuster breach sessions (`SG:60-62`). Shelldiver dives with air as the limit (`SG:78`). A Game About Digging a Hole: battery and pockets (`SG:101-102`). The permanent tree: Nodebuster (`SG:57-59`), Shelldiver (`SG:79-80`), Digseum (`SG:88-89`). S07 is a different system: an attempt on a cooldown, a presence gate. |
| Formula families | Run length 1 to 2 minutes (`SG:78`). 4 to 5 purchases after each run (`SG:79`). Enemy health and damage rise during a run (`SG:61`). The cost of a tree node: no sourced formula (`SG:205`). |
| Clock | Game time in the run. Player action between runs. |
| Presence | Active. The run needs input: early damage in Nodebuster comes only from the cursor (`SG:65`). Open and closed: no run. |
| Links | Needs a run income (S01 or S05). Pays the tree: lines of S02 bought with the run currency. With no reset layer, the tree is the permanent progress (decision D10). |
| Growth role | Clock: the run sets the purchase rhythm with no real-time clock. Source: the run income. |
| Balance risks | The loss of carried loot punishes a long run (`SG:102`). A tree that is "disproportionally huge" (`SG:80`). |
| Scope | M. UI: a run screen, a limit bar, a run summary, a tree screen. |
| Signature in code | A `run_timer`, `air`, `battery` or `health` that ends the run. A `carried` amount that the run end banks or loses. Tree nodes with a `level` and a `parent`. |

Variants of S29:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S29.health_pressure` | variant | `the run ends at health 0; enemy health and damage rise during the run; uncollected loot is lost` | no sourced range | Nodebuster | `0`: SG:60-62 give no constant | none; simulation path: simulated |
| `S29.air` | variant | `the run ends when the air runs out` | `run_s` 60 to 120, default 120 (`SG:78`) | Shelldiver | `0`: a clock of game time | none; simulation path: simulated |
| `S29.battery_inventory` | variant | `the run ends at a full inventory or a low battery; at battery 0 or health 0 the carried loot is lost` | no sourced range | A Game About Digging a Hole | `0`: SG:101-102 give no constant | none; simulation path: simulated |
| `S29.purchase_rhythm` | helper | `n purchases after each run` | `n` 4 to 5, default 4 (`SG:79`) | Shelldiver | `0`: the rhythm, not a line | - |
| `S29.permanent_tree` | modifier | `nodes with several levels each, bought with the run currency between runs; no reset clears them` | no sourced range | Nodebuster, Shelldiver, Digseum | `0`: no sourced cost formula (SG:205) | - |

### S30 Upkeep

| Field | Content |
| --- | --- |
| What it is | Each producer unit adds to a need: food, rest, power or morale. Care capacity meets the need. The output is multiplied by an uptime factor `u` between 0 and 1, and `u` falls when the need is above the capacity. |
| Decision | More producers or more care. A producer raises the output and the need, so it can lower the output of each other producer. The best next purchase changes between the 2. |
| Evidence | Factorio: "all machines will slow down proportionally to the power available"; 180 kW for a demand of 300 kW gives 60% speed (https://wiki.factorio.com/Electric_system, sourced). Kittens Game: job output is multiplied by happiness (`C:42`, sourced). Evolve: below 100% morale, "1 point for each point below 100" of production; job stress "will rise as your population rises", and entertainers counter it (https://wooledge.org/~greg/evolve/guide.html, sourced). Oxygen Not Included: each unit needs 1000 kcal for each cycle, holds 4000 kcal, and dies at 0 (https://oxygennotincluded.wiki.gg/wiki/Duplicant, sourced). |
| Formula families | `u = max(floor, min(1, C / N)^e)` with the need `N = N0 + h x units` and the capacity `C = C0 x c^k` (a factor care line) or `C0 + c x k` (an add care line). Factorio gives `e = 1` (sourced). A pool share: the carers come from an allocation pool and produce nothing (Evolve, sourced; no constant). A stock that each unit drains and care refills (Oxygen Not Included, sourced; no incremental gives a constant). A linear penalty `u = M / 100` below 100 morale (Evolve, sourced). |
| Clock | Game time. The need follows the unit count at once. A stock drains in game time. |
| Presence | Active: the player buys care. Open: `u` holds, because the counts do not change with no input. Closed: no change for `continuous`. |
| Links | Needs a producer line (S01) whose units add to the need. Care comes from care lines (an S02 entry with the role `care`, or the care line of the solver) or from an allocation pool (S21). Works with a cap system (S13): a cap on care holds `u` below 1. Works with S24: an auto-buy of producers lowers `u`, so the player buys care. |
| Growth role | A bounded factor (`BM L7`) while the capacity keeps up with the need: it adds nothing to `R`. A brake when the need grows faster than the capacity (`BM` section 3b). |
| Balance risks | A stable best split ends the decision: with `e < 1`, `I ~ n^(1 - e) C^e`, so a fixed share of the spend is best (inference, `BM` 3b). A death spiral: `u` falls, and the income cannot buy care (Oxygen Not Included: death at 0 kcal, sourced; Evolve: "a spiraling food crisis", sourced). A producer level that lowers the income when `f < (1 + h / N)^e` (`BM` 3b). |
| Scope | M. UI: a need bar or an icon on each unit, the `u` value, and care lines in the shop. |
| Signature in code | A factor between 0 and 1 on income: `uptime`, `efficiency`, `satisfaction`, `happiness` or `morale` that purchases raise. A need that grows with a count: `need = units * per_unit`, `hunger`, `upkeep`, `consumption`. `min(1.0, supply / demand)`. |

Variants of S30:

| Id | Kind | Formula | Parameters: range, default (source) | Game | `r` | Gives |
| --- | --- | --- | --- | --- | --- | --- |
| `S30.need_capacity` | variant | `u = max(floor, min(1, C / N)^e); N = N0 + h x units; C = C0 x c^k for k care levels (or C0 + c x k)` | `e` 1, default 1 (Factorio wiki, Electric system, sourced) | Factorio, Kittens Game | `0`: a bounded factor (BM L7) while the capacity keeps up; a brake when N grows faster than C (BM 3b) | simulated; simulation path: none |
| `S30.pool_share` | variant | `u = min(1, carers x c / (h x workers)); the carers come from a pool (S21) and produce nothing` | no sourced range | Evolve | `0`: job stress rises with the population (sourced) | none |
| `S30.decay_refill` | variant | `each unit drains a stock at h per second; care refills the stock; an empty stock stops the unit (u = 0)` | no sourced range | Oxygen Not Included | `0`: 1000 kcal for each cycle, a stock of 4000 kcal (sourced); no incremental gives a constant | none |
| `S30.penalty_points` | modifier | `below 100 morale, 1 point of production for each point; above 100, half a point for each point` | no sourced range | Evolve | `0`: u = M / 100 below 100 (sourced) | - |
| `S30.switch_point` | helper | `the best purchase is care while u < 1 and c^e / cost of care > f (N / (N + h))^e / cost of the producer; else a producer` | no sourced range | genre | `0`: BM 3b (inferred) | - |

The solver writes `S30.need_capacity` (`references/design-solver.md`, "Upkeep"). The model holds it as the key
`upkeep` (`references/model-schema.md`). The proof sheet adds the rows PU1 to PU4: PU3 is proved by its formula, and
PU1, PU2 and PU4 are simulated (`references/balance-math.md` section 3b).

## Core and optional

### Core systems

| Id | Why it is core | Count in the studied games |
| --- | --- | --- |
| S01 | The source of income. | 10 of the 20 games have a cited ratio (`O:33-44`). No studied game lacks a purchase line (inference). |
| S02 | The answer to the wall. The stacking rule is in most games (`O:113-114`). | 10 games in `O:116-127`. |
| S03 | The one number that measures progress (`P:57-58`). | 6 games cited in S03. |
| S04 | The common answer to a wall. | 17 of 20 games: the 11 in S04, plus `A:133`, `B:91`, `B:278`, `D:11`, `D:122`, `D:130`. Melvor Idle has none (`C:189`). See D1. |
| S05 | The pacing rule needs active play to beat idle play. Moss Works fails with no active action (`IA:8`, `IA:15`). | 6 games cited in S05. Core by the pacing rule, not by the count. |
| S06 | The pacing rule needs a closed-game penalty and a cap. Moss Works pays 100% for 8 hours (`SA:12`). | 3 games cited (IOM, Melvor Idle, Realm Grinder). Core by the pacing rule. |
| Slot: S07, S08 or S09 | The pacing rule needs a limit on progress from open idle and from a time scale. An offline cap does not limit an open game (`IA:16`). | IOM has all 3 (`RP:16`). S07 is single source. |

The table above is the core set of the play pattern `check_in`. The play pattern `continuous` has
its own core set (decisions D9 and D10):

| Id | Class for `continuous` | Reason | Count in the short games |
| --- | --- | --- | --- |
| S01, S02, S03 | Core | As for `check_in`. | S02 in 10 of 10 games with structure data (`SG:57-59`, `SG:79-80`, `SG:103-104`). |
| S05 | Core. The interview recommends `S05.active_lane` (proved). `S05.manual_then_automate` (the default of D10) gives a simulated model | The manual action is the only income first in most short games. | 10 of 10 games have an active action (`SG:65`, `SG:91`, `SG:129`, `SG:155`). |
| S24 | Core | It ends the manual phase of S05. | Shelldiver, Scritchy Scratchy, Gnorp Apologue (`SG:82`, `SG:129`, `SG:155`). |
| S28 | Core | The game has a measurable finish. | 10 of 10 games with structure data (`SG:186`). |
| S04 | Optional. `S04.none` is permitted with S29 or S22 | 5 of 10 games have no reset. | `SG:187`. |
| S06 and the slot | Optional, S06 default "none" | No idle or offline benefit. | No short game uses an offline rule or a presence gate (inference: no game record names one). |

### The systems that control presence

| System | Controls | Core | Note |
| --- | --- | --- | --- |
| S05 Active action | The active column | Yes | The reference premium is about x1.25 (`TB:61`). |
| S06 Offline rule | The closed column | Yes | 85% to 92%, 12-hour cap (`RP:9-10`). |
| S07 Attempt on a cooldown | A spine that needs a visit | Slot | Single source. |
| S08 Bank with a cap | The check-in interval | Slot | Check-in = shortest fill time (`TB:382`). |
| S09 Real-time clock | Progress that a time scale cannot compress | Slot | `IA:13` shows the failure without it. |
| S10 Random visitor | An active pulse | Optional | Can fill the S05 slot (D4). |
| S11 Timed buff | An active bonus | Optional | Uptime is the stat. |
| S24 Automation unlock | Moves active work to open idle | Optional. Core for `continuous` | Keep it off while closed (`P:193-195`). |

## Compatibility of the optional systems

### Pairs

| Pair | Verdict | Reason and proof |
| --- | --- | --- |
| S04 + S15 | Works | Trimps perks (`C:104`). IOM artifacts (`M:240-241`). |
| S04 + S14 | Works | Unspent reset currency pays +1% or +2% each: Idle Champions (`B:300`), AdVenture Capitalist (`D:299`). |
| S14 + S15 | Works | The spend-against-hold decision: AdVenture Capitalist angel upgrades (`D:300`, `D:316`). |
| S04 + S16 | Works | Clicker Heroes (`B:223-225`). Antimatter Dimensions (`A:133-140`). |
| S04 + S17 | Works | Trimps (`C:109`). Synergism (`A:189`). |
| S01 + S12 | Works | AdVenture Capitalist (`D:297`). Clicker Heroes (`O:203`). |
| S03 + S07 | Works | IOM (`P:338-347`). |
| S09 + S23 | Works | IOM real-time minigame (`P:125-126`). |
| S10 + S11 | Works | Cookie Clicker visitor pays a x7 burst (`A:58`). IOM visitor pays buffs (`P:282`). |
| S10 + S19 | Works | IOM visitor pays chests (`P:282`). |
| S08 + S26 | Works | IOM faucets bank (`TB:382`). |
| S13 + S19 or S23 | Works | Far systems raise caps (`P:105-108`). |
| S19 + S20 | Works | Cookie Clicker achievements pay milk for collection upgrades (`A:52-53`). |
| S04 + S27 | Works | Trimps line rebirth inside a reset run (`C:103`, `C:130`). |
| S05 (`S05.manual_then_automate`) + S24 | Works | Shelldiver, Scritchy Scratchy, Gnorp Apologue (`SG:82`, `SG:129`, `SG:155`). |
| `S04.none` + S29 | Works | Shelldiver and A Game About Digging a Hole have runs and no reset (`SG:83`, `SG:105`). |
| S04 + S29 | Works | Nodebuster raises a prestige level at each boss kill (`SG:63-64`). |
| S30 + S21 | Works | Evolve: entertainers are workers taken from the jobs, and they raise morale (https://wooledge.org/~greg/evolve/guide.html, sourced). |
| S30 + S24 | Care | An auto-buy of producers lowers `u` with no decision of the player; the player must buy care (inference, `BM` 3b). |
| S30 + S13 | Care | A cap on the care lines holds `u` below 1 for good; the cap raiser then pays the output (inference). |
| S16 + S15 | Care | Old bonuses must shrink at a new layer: `B^(0.1 / A)` (`B:47`). |
| S12 + S01 | Care | `r` = 0.82 for x4 each 25 at cost 1.07 (computed). 1 line can dominate (`O:193-194`). |
| S14 + S06 | Care | The held currency grows while closed, so idle raises the bonus (inference). |
| S18 + S06 | Care | Output grows as a power of elapsed time, so a long absence pays more than linear (inference from `O:196-198`). |
| S24 + S06 | Care | Automation while closed removes the active premium. IOM keeps auto-reset off offline (`RP:12`). |
| S11 + S24 | Care | Automatic buff use moves the uptime to open idle (inference). |
| S08 + S06 | Care | A bank cap longer than the offline cap is a second offline income (inference). |
| S06 at 100% with no cap + S05 small | Conflict | Idle beats active by 31 to 1 for each active minute (`IA:8`, `SA:12`). |
| S15 line with `r >= 1` and no cap + S04 | Conflict | The multiplier grows without limit with the reset gain (`SA:11`). |
| S09 + a global time scale on all clocks | Conflict | The clock is no longer real time. x100 finishes the game (`IA:13`). |
| S25 + main-game power inside the event | Conflict | IOM keeps main power out so that all players start level (`P:318-320`). |

### Minimum set for each optional system

| Id | Needs at least |
| --- | --- |
| S07 | S03 |
| S08 | A timed reward (S10, S26) or a resource with storage (S01) |
| S09 | A system to put on the clock (S23, S19 or S22) |
| S10 | A reward table that pays S01 currency, S11 or S26 |
| S11 | S01 and a source of buffs (S10, S19 or S05) |
| S12 | S01 |
| S13 | A line with a cap and a far system that raises it |
| S14 | A held currency (S04 or S26) |
| S15 | S04 |
| S16 | S04 and S15 |
| S17 | S04 |
| S18 | S01 (it replaces or extends the line) |
| S19 | A drop source (S03, S10 or S22) |
| S20 | 3 or more systems for the tasks to name |
| S21 | A pool resource and 2 or more sinks |
| S22 | Actions in other systems |
| S23 | 1 link in and 2 links out (`X:55-56`) |
| S24 | The manual system that it automates |
| S25 | S03 (rewards scale with main progress), live operation |
| S26 | 3 or more faucets and a sink ladder |
| S27 | S01 |
| S28 | S03 (`S28.last_step`) or a last purchase line |
| S29 | A run income (S01 or S05) and a tree (S02 lines in the run currency) |
| S30 | A producer line (S01) whose units add to the need, and care lines (S02 with the role `care`) or a pool (S21) |

## Starter sets

| Code | Name | Pattern | Systems | Proof | Gives |
| --- | --- | --- | --- | --- | --- |
| K1 | Small active clicker | check_in | S01, S02, S03, S04 (cube root), S05 (click), S06, S10, S11, S12, S14, S20 | Cookie Clicker (`A:35-65`) | none |
| K2 | Slow idle with a second clock | check_in | S01, S02, S03, S04 (power 0.44), S05 (x1.25 lane), S06 (0.85, 12 h), S07, S08, S09, S10, S11, S13, S19, S23, S26 | IOM (`P:50-347`, `RP:9-16`) | none |
| K3 | Run-based push with a strong reset | check_in | S01 (1.2, 1.24), S02, S03 (x2 for each zone), S04 (sub-exponential with steps), S05, S06, S08 (storage), S15 (1.3), S17, S27 | Trimps (`C:103-110`) | none |
| K4 | Layered exponent game | check_in | S01, S02, S03 (number limit), S04 (power of log), S05, S06, S12 (x2 each 10), S16, S17, S18 | Antimatter Dimensions (`A:110-146`) | none |
| K5 | Skill tracks with no reset | check_in | S01, S02, S03 (XP doubling), S05, S06 (24 h cap), S09 (speed-independent drops), S13, S14 (pool checkpoints), S19, S22 | Melvor Idle (`C:189-237`) | none |
| K6 | Business idle | check_in | S01 (1.07 to 1.15), S02, S03, S04 (square root), S05, S06, S12, S14, S15, S24 | AdVenture Capitalist (`D:274-317`) | none |
| K7 | Short active push with a reset and a hard end | continuous | S01 (geometric), S02 (levelled factor), S03 (exponential), S04 (power), S05 (active lane), S24 (auto buy), S28 (last step) | Nodebuster (`SG:49-72`), Scritchy Scratchy (`SG:126-133`) | simulated |
| K8 | Run on a limit with no reset | continuous | S01 (geometric), S02 (levelled factor), S03 (exponential), S04 (none), S05 (active lane), S24 (auto buy), S28 (full tree), S29 (air) | Shelldiver (`SG:74-84`), A Game About Digging a Hole (`SG:97-105`) | none |
| K9 | 1 long run with no reset and a hard end | continuous | S01 (geometric), S02 (levelled factor), S03 (exponential), S04 (none), S05 (active lane), S24 (auto buy), S28 (last step) | Digseum (`SG:86-95`), Spaceplan (`SG:135-140`), Universal Paperclips (`SG:142-149`) | simulated |

Rules for the starter sets:

1. K5 has no S04. Decision D1 permits this, because S22 is the spine of K5.
2. K1, K4 and K6 have no cited presence gate. Decision D2 adds the default S08 (`S08.timed_bank`)
   to each of them. The proof game does not prove S08 in these sets.
3. K4 has S16, and S16 needs S15. The skill adds S15 to K4, and the designer picks its variant.
4. K3, K4 and K6 have no cited S05 or S06 formula. The designer picks the variant in the
   interview (inference).
5. `assets/systems.json` gives the variant of each system in each set. A null variant means that
   the proof game has no cited formula for that system.
6. K1 to K6 are for the play pattern `check_in`. K7, K8 and K9 are for `continuous`. The short games
   of K7, K8 and K9 give the pattern, not the formulas.
7. K7 solves with no override of a variant. Its class is `simulated`: the laws prove each part
   except S24 (automation), whose timing comes from the simulator. With the Moss Works targets, the
   solver reports "not ready" on stated rows (PO13, PC4, PV), not a variant conflict.
8. K8 has no S04. Decision D10 permits this, because S29 gives the permanent progress. The laws give
   no run length with no reset, so the solver takes the simulation path. The scaffold has no writer
   for `S28.full_tree`, so the class of K8 is `none`: no model. With `S28.last_step` in place of
   `S28.full_tree`, the set gives a simulated model.
9. The column "Gives" is the class of the set: the lowest class of its parts in a run of the
   solver. K1 to K6 are `none`: each has 1 or more parts that no tool writes.
10. K9 has no S04, no S29 and no S22. Decision D11 permits this: S28 ends the 1 long run, and the
   run is the permanent progress. Its games have no reset and no run on a limit. They pace the run
   with sites or eras that open in order (`SG:94-95`, `SG:146-147`), a manual action and automation
   that replaces it (`SG:91-92`, `SG:146-148`), and a hard end (`SG:139`, `SG:149`). The solver
   proves it with no simulation path (`references/balance-math.md` section 3a): R = 1, the growth
   class (PO1), the idle value (PO6), the unlock rate (PO10), the time to the hard end (PO12) and the
   play-test speed (PO13). PO2 to PO5 are not applicable: no reset layer. Its class is `simulated`
   only because no law covers S24. K9 solves with no override. With a content file that names the
   content, the result is ready (`assets/examples/ready-long-run/`). With no content file the model
   is a skeleton: row CG fails and the result is not ready (`tests/designs/long-run/`).

## Decisions

The operator decided the 8 open questions of phase 1 on 2026-11-13, added D9 and D10 on 2026-11-14, and D11 on 2026-11-15 (ticket 40). The evidence for each
decision is the evidence of the old question. The interview (`references/interview.md`) and
`assets/systems.json` apply these decisions.

| Code | Date | Decision | Evidence |
| --- | --- | --- | --- |
| D1 | 2026-11-13 | S04 is core. The variant `S04.none` is permitted only when S22 is the spine. | 17 of 20 games reset. Melvor Idle has no reset and uses skill tracks (`C:189`). |
| D2 | 2026-11-13 | Each game needs 1 or more presence gates (S07, S08, S09). The default is S08. | IOM uses all 3 (`RP:16`). S07 has a single source. S08 has 2 sources and scope S. |
| D3 | 2026-11-13 | An open game with no input has no separate rule. The presence gate covers it. | No studied source caps an open game. An offline cap does not limit it (`IA:16`). A decay for open idle has no sourced formula. |
| D4 | 2026-11-13 | S10 can fill the S05 slot only when the visitor does not bank (`S05.visitor` with `S10.window`). | Cookie Clicker visitors pay only active players (`A:55-58`). IOM banks visitors, so they also pay idle players (`M:220`). |
| D5 | 2026-11-13 | S24 does not run while the game is closed, by default. | IOM keeps auto-reset off offline (`RP:12`, `P:193-195`). No other source states a rule. |
| D6 | 2026-11-13 | S26 is optional. | IOM and Egg, Inc. have one. The other studied games have no cited premium faucet. |
| D7 | 2026-11-13 | S18 is a separate optional system, because it changes the presence math. | 3 sources (`A:110-113`, `A:335`, `D:331`). It changes the growth degree in time (`O:196-198`). |
| D8 | 2026-11-13 | S07 stays a presence gate of the core slot. It can fill the slot. It is not a core system alone. | IOM only (`P:338-347`). |
| D9 | 2026-11-14 | The skill has 2 play patterns. `continuous`: S06 is not core (default "none"), the presence gate slot is optional, and the targets have no closed idle value. `check_in`: the rules D1 to D8 as they are. The interview asks the pattern first. | The operator: "The operator builds games for release on Steam. The player plays continuously: the demo in 1 sitting, the full game possibly in 1 sitting. Such a game has no idle or offline benefit. A mobile idle game is the other pattern: offline gains and short check-in sessions of about 15 minutes." |
| D11 | 2026-11-15 | For `continuous`: `S04.none` is also permitted with S28 and with no S29 and no S22. The game is 1 long run to the hard end, and the run is the permanent progress. The laws L1, L2, L3, L8 and L9 prove it (`references/balance-math.md` section 3a). | 5 of 10 short games have no reset (`SG:187`). Digseum, Spaceplan and Universal Paperclips have no reset and no run on a limit (`SG:93`, `SG:140`, `SG:146-149`). |
| D10 | 2026-11-14 | For `continuous`: S05 is core with the default `S05.manual_then_automate`. S24 and S28 are core. S04 is optional, and `S04.none` is permitted with S29 (the run with a permanent tree) or with S22. A continuous selection with no S28 is refused. `check_in` keeps D1 to D9. | 10 of 10 short games with structure data have a hard end (`SG:186`). 10 of 10 have an active action, and automation replaces it later (`SG:65-66`, `SG:82`, `SG:129`, `SG:155`). 5 of 10 have no reset (`SG:187`). Shelldiver and Digseum have a permanent tree with no reset (`SG:79-83`, `SG:88-93`). |
