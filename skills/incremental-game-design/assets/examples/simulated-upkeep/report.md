# Design report (continuous): 1 long run with no reset

Result: simulated: each row passes or is simulated, not proved, and no part is a stub: the design can go to a build, and a play test must confirm the simulated rows.

The selection has no reset layer, no S29 and no S22: 1 long run to the hard end. A game with no reset is a valid design, and its result has the same meaning as that of a game with a reset. The generator has no cap: it is a growth carrier with count steps (BM 3a). The solver sets R = 1 over the carriers at the middle of the length, measures g of I ~ e^(g t) (L2), sets 1 wall ratio w for a near constant unlock rate u = 3600 g / ln w (L3), and the stage count N for the length (L8). The rows of the reset loop (PO2 to PO5) are not applicable: no reset layer.

The selection refuses S12, and the solver still adds count steps (S12.count_steps) to the generator: an uncapped additive line with no step adds nothing to R (L1), so it stops growing against the multipliers. The way out is a generator with a cap, which then fails the row CL (core line life).

## Proved

- PO1: R of the carriers 0.893 + 1 / (k ln b) 0.077 of gen at level 93 (the middle of the length) + the content share 0.037 = 1.007, no reset layer; g of the 2 halves after hour 1: 0.00219 and 0.00193 /s
- PO6: open 1 h 0.286; open 8 h 0.070; open 24 h 0.029; open 168 h 0.006 | simulation: open 1 h 0.277; open 8 h 0.062; open 24 h 0.026; open 168 h 0.005; closed game: not applicable (no offline gain by design)
- PO7: L4 property 3: f <= 1 and R > 0 | simulation: largest idle value 0.277; closed game: not applicable (no offline gain by design)
- PO8: f_open = 0.8, f_closed = 0; closed game: not applicable (no offline gain by design)
- PO9: tools: open income share f_open = 0.8 < 1 (scale tool); closed game: not applicable (no offline gain by design)
- PO10: u = 7.50 unlocks for each hour, 1 / u = 8.0 min | simulation: median gap after hour 1 8.6 min
- PO11: a_1 = 10 (gen), I_0 = 1
- PO12: H from N = 30: 2.53 h | simulation: active_greedy finishes at 2.49 h
- PO13: T_e = 1 / g = 489 s at x10; largest speed: formula x24, simulation x20 | simulation: median run n/a s, slow-down at the play-test speed 1.02
- PV: smallest gain 10.0% (element_1); 7 lines, 0 below the floor; gen: the tested gain is that of 1 count block of 10 levels with its step; 1 level gains 0.6% or more (reported, not tested)
- WS: first step x1.41 (first step, not tested); 16 later steps: smallest x1.45, median x1.73, largest x2.44; each later step inside the range
- PU3: formula 20.0%; 25 of 25 care purchases gain the floor or close the shortfall
- S01.geometric: a law of the solver covers the variant
- S02.levelled_factor: a law of the solver covers the variant
- S03.exponential: a law of the solver covers the variant
- S05.active_lane: a law of the solver covers the variant
- S28.last_step: a law of the solver covers the variant

## Simulated, not proved

- PC1: active_greedy reaches axis step 30 at 2.49 h
- PC2: largest gap 1.42 min; first purchase at 0.17 min
- PC4: largest gap 8.92 min; first hour 4.33 min; 44 unlocks
- PC5: 16 of 16 elements reached
- PC6: busiest minute 5 purchases by hand (5 with the auto-buy purchases); last hour 1.8 for each minute; 0 lines with auto-buy
- CL: gen: level 170, last purchase at 149.6 min (100% of 149.6 min)
- CG: largest gap 18.98 min; first hour 10.07 min; 15 content elements; in range: 10.07 min from 36.0 to 46.1 min against a target of about 10 min (no source), inside the range to 12 min
- PU1: u inside 0.69 to 1 for 100% of the time; lowest u 0.783
- PU2: longest stall 1.4 min (limit 10 min); exponent 1
- PU4: 46 switches between producer and care purchases
- S04.none: 1 long run: no reset layer, so the rows of the reset loop (PO2 to PO5) are not applicable; the laws of 1 long run prove the timing, and an auto-buy plan (S24) moves the idle row PO6 to the simulation
- S24.auto_buy: the solver writes it; no law covers it
- S30.need_capacity: the solver writes the upkeep block and 1 care line; no law gives the path of u, so PU1, PU2 and PU4 are simulated, and PU3 is proved by its formula

## Failed

- none

## Systems and marks

Each selected system has 1 mark: written and proved by a law, written and checked by the simulation only, or not written (with the reason).

| System | Variant | Mark | Entries | Reason |
| --- | --- | --- | --- | --- |
| S01 Generator line | S01.geometric | written, proved by a law | - | a law of the solver covers the variant |
| S02 Multiplier upgrade | S02.levelled_factor | written, proved by a law | generator (generator upgrades): m1, speed (speed upgrades): m2, care (care): care | a law of the solver covers the variant |
| S03 Spine with a wall | S03.exponential | written, proved by a law | - | a law of the solver covers the variant |
| S04 Reset layer | S04.none | written, simulation only | - | 1 long run: no reset layer, so the rows of the reset loop (PO2 to PO5) are not applicable; the laws of 1 long run prove the timing, and an auto-buy plan (S24) moves the idle row PO6 to the simulation |
| S05 Active action | S05.active_lane | written, proved by a law | - | a law of the solver covers the variant |
| S24 Automation unlock | S24.auto_buy | written, simulation only | - | the solver writes it; no law covers it |
| S28 Hard end | S28.last_step | written, proved by a law | - | a law of the solver covers the variant |
| S30 Upkeep | S30.need_capacity | written, simulation only | upkeep, care | the solver writes the upkeep block and 1 care line; no law gives the path of u, so PU1, PU2 and PU4 are simulated, and PU3 is proved by its formula |

## Targets checked

Checked: PO1 (pass), PO6 (pass), PO7 (pass), PO8 (pass), PO9 (pass), PO10 (pass), PO11 (pass), PO12 (pass), PO13 (pass), PC1 (pass), PC2 (pass), PC4 (pass), PC5 (pass), PC6 (pass), PV (pass), CL (pass), WS (pass), CG (pass (in range)), PU1 (simulated, not proved), PU2 (simulated, not proved), PU3 (pass), PU4 (simulated, not proved).
Not computed: none.

## Selected systems

- S01: S01.geometric
- S02: S02.levelled_factor
  - entry generator: S02.levelled_factor, role generator upgrades
  - entry speed: S02.levelled_factor, role speed upgrades
  - entry care: S02.levelled_factor, role care
- S03: S03.exponential
- S04: S04.none
- S05: S05.active_lane
- S24: S24.auto_buy (selected; the solver writes no line for it: simulation and judgment, see references/design-solver.md)
- S28: S28.last_step
- S30: S30.need_capacity

## Target warnings

- the demo end: not stated (no demo); the proof row PC3 is not applicable
- no source: unlocks in the first 10 minutes and the first hour, an active factor, the count of resets, a play-test speed (SG:198-205); each such target is a designer target, not supported by a source

## Hard end and demo end

- Hard end (S28.last_step): stage 30. PC1: active_greedy reaches axis step 30 at 2.49 h (pass).
- Demo end: no demo.
- PC2: largest gap 1.42 min; first purchase at 0.17 min (pass).
- PC3, the parts past the demo: no demo end (not applicable).

## Gaps between unlocks

PC4: largest gap 8.92 min; first hour 4.33 min; 44 unlocks (pass; limit <= about 10 min (range to 12 min); first hour <= about 5 min (range to 6 min)).

## Purchase value

PV: smallest gain 10.0% (element_1); 7 lines, 0 below the floor; gen: the tested gain is that of 1 count block of 10 levels with its step; 1 level gains 0.6% or more (reported, not tested) (pass; limit >= about 10% for each level, or for each count block of a line with count steps (range to 8.0%)). Floor source: designer target; default 10%: the low end of the proven step d 1.1 to 2 of S02.levelled_factor (assets/systems.json, O:188, D:195).

### What the player buys

| Minutes | Purchases | Unlocks | Smallest gain | Largest gain |
| --- | --- | --- | --- | --- |
| 0 to 10 | 24 | 8 | 4.4% | 62.5% |
| 10 to 20 | 25 | 5 | 2.3% | 55.5% |
| 20 to 30 | 18 | 5 | 1.8% | 54.4% |
| 30 to 40 | 14 | 4 | 1.7% | 54.0% |
| 40 to 50 | 18 | 3 | 1.5% | 53.7% |
| 50 to 60 | 20 | 3 | 1.2% | 53.5% |
| 60 to 70 | 20 | 1 | 1.1% | 53.3% |
| 70 to 80 | 21 | 3 | 1.0% | 53.1% |
| 80 to 90 | 16 | 1 | 0.9% | 53.0% |
| 90 to 100 | 20 | 3 | 0.8% | 52.9% |
| 100 to 110 | 16 | 2 | 0.8% | 52.8% |
| 110 to 120 | 18 | 1 | 0.7% | 52.7% |
| 120 to 130 | 17 | 2 | 0.7% | 52.6% |
| 130 to 140 | 17 | 2 | 0.6% | 52.5% |
| 140 to 150 | 16 | 1 | 0.6% | 52.5% |

## Reachable

PC5: 16 of 16 elements reached (pass).

## Purchase rate

PC6: busiest minute 5 purchases by hand (5 with the auto-buy purchases); last hour 1.8 for each minute; 0 lines with auto-buy (pass; limit <= about 30 purchases in 1 minute (range to 36 purchases in 1 minute)). The target is the most purchases that a person makes in 1 minute with no automation (designer target).

## Parameters

| Parameter | Value | Range | Source | Set by |
| --- | --- | --- | --- | --- |
| N | 30 | derived | BM L8: N = u_1 + ceil((ln P(1 h) - ln h_u1) / ln w + u (H - 1)) | PO12 (length) |
| R | 0.8929 | [0, 1] | BM 3a: the carriers share R = 1 - 1 / (k_mid ln b) - the content share; with the factor k of the generator and the constant factors of the named content the sum is 1 at the middle of the length, so I ~ e^(g t) (L2) and 1 wall ratio gives a near constant unlock rate (L3) | PO1, PO10 and PO12 |
| S01.geometric.b | 1.15 | [1.07, 1.15] | O:48-49 | S01 default (the cost ratio of the generator) |
| S02.levelled_factor.b | 1.377 | [1.15, 10] | O:188, D:195 | moved from the default to the finest steps that keep d in range (PO5, BM section 7 limit 6) |
| S02.levelled_factor.d | 1.1 | [1.1, 2] | O:188, D:195 | d = b^r with r = R / 3 = 0.2976 (BM L1, PO1) |
| wall table steps (not g) | smallest 1.414, median 1.726, largest 2.438 | [1.145, 2.8] | the wall table of the timing fit (18 walls) | WS |
| S05.active_lane.a | 1.25 | [1.25, 1.25] | TB:61, TA:507 | S05 default |
| S12.count_steps.m | 1.516 | [1.025, 4] | O:202-203, A:117, B:157 | m = b^(r n) with r = 0.2976 of 1 carrier and b = 1.15 of the generator (BM 3a: r_eff = ln m / (n ln b)) |
| S12.count_steps.n | 10 | [10, 100] | O:202-203, A:117, B:157 | the finest steps of the proven range (BM section 7, limit 6) |
| S30.need_capacity.e | 1 | [1, 1] | Factorio wiki, Electric system: machines slow down proportionally to the power available (https://wiki.factorio.com/Electric_system), sourced | S30 default (u = min(1, C / N)^e) |
| care_band | [0.69, 1] | derived | u falls at most 2 care steps below 1 for a buyer that buys care when u < 1 | PU1 |
| care_base | 3 | derived | a choice of the solver: the base capacity meets the first 3 producer levels | PU1 and PU2 (no stall at the start) |
| care_step | 1.2 | derived | c^e - 1 = 2 x the PV floor 0.1 (PU3 with a margin) | PU3 |
| cost_scale | 36.92 | derived | BM 3a: g = a v k prod (r (b_c - 1) / a_c)^r = 0.09695 / F^0.579 with k = k_mid = 74.4; times (the measured g / g_goal)^(1 / share) (the simulator in the loop) | g = g_goal (PO10, PO13) |
| f_open | 0.8 | [0, 1] | BM L5: an active income term a is the scale tool f = 1 / a | PO8, PO9 and the idle targets |
| first_stages | [2.9, 4.1, 9.4, 16.22, 28, 43, 83, 141.1, 240, 470, 750, 1300, 2100, 3600, 6300, 1.1e+04, 1.6e+04, 3.9e+04] | derived | the trajectory of active_greedy at the target times (BM L8 step 4) | PO11 and the first-hour unlock target |
| g_goal | 0.002145 | derived | u ln(g of S03, default) / 3600 with u the unlock target (L3) | PO10 at the default wall ratio |
| g_run | 0.002043 | derived | BM L2: the slope of ln I from hour 1 to H in the run with no stage (pw_proof.run_growth) | PO6, PO10, PO12 and PO13 |
| k_mid | 99.24 | derived | the generator level at the middle of the length: L1 first, then the run with no stage (pw_proof.step_inputs) | PO1 (R = 1 at the middle of the length) |
| r_group | 0.2976 | derived | BM L1: R is the sum of the largest r of each group | PO1 |
| u | 7.5 | [7.5, ] | BM L8: u = alpha ldot / ln w | PO10 |

## Idle value against active play

- Formula (L4b): open 1 h 0.286; open 8 h 0.070; open 24 h 0.029; open 168 h 0.006
- Simulation: open {'1': 0.2771, '8': 0.0623, '24': 0.026, '168': 0.0049}; closed: not applicable (no offline gain by design)

## Play-test speed

Largest speed (formula): x24. Largest tested speed (simulation): x20. Play tests run at x20 or slower: the smaller value.

## Gaps between content elements

CG: largest gap 18.98 min; first hour 10.07 min; 15 content elements; in range: 10.07 min from 36.0 to 46.1 min against a target of about 10 min (no source), inside the range to 12 min (pass (in range); limit <= about 20 min (range to 24 min); first hour <= about 10 min (range to 12 min)). An axis step and an empty content slot are not content elements. The band is the FM-1 band of the fun sheet: a target with no source, so the row has the range of such a target. The row decides the result.

## Named content

The content file gives the named elements. An element that is open at the start is a line of the solver with its name. Each other element has its unlock point (a lifetime total) and its first cost from the run of the active player at its target minute. A second generator line takes the count steps of the generator. Each other named line is a side line with a cap: it adds a constant factor and nothing to R (L7).

| Id | Name | Model entry | Target (min) | Active player (min) |
| --- | --- | --- | --- | --- |
| gen | Workshop | gen | 0 | 0.2 |
| m1 | Better tools | m1 | 0 | 12.2 |
| m2 | Faster work | m2 | 0 | 27.2 |
| care | Maintenance | care | 0 | 0.7 |
| element_1 | Tool rack | element_1 | 9 | 9.5 |
| element_2 | First district | element_2 | 18 | 18.0 |
| element_3 | Better layout | element_3 | 27 | 27.0 |
| element_4 | Second district | element_4 | 36 | 36.0 |
| element_5 | Training | element_5 | 45 | 46.1 |
| element_6 | Third district | element_6 | 54 | 54.0 |
| element_7 | Night shift | element_7 | 72 | 72.0 |
| element_8 | Fourth district | element_8 | 90 | 90.0 |
| element_9 | Fine tuning | element_9 | 108 | 109.0 |
| element_10 | Fifth district | element_10 | 126 | 126.0 |
| element_11 | Last district | element_11 | 142 | 142.0 |

## Fun rows

The fun sheet (scripts/pw_fun.py, references/fun-analysis.md) gives a second result beside the proof result. It is advice with evidence marks, not a proof: a fun row never makes the result ready or not ready.

Fun result (FM-1 to FM-8): likely dull; counting rows out: FM-1, FM-2, FM-3, FM-6.

| Id | Name | Value | Band (mark) | Status | Cause | Change to try |
| --- | --- | --- | --- | --- | --- | --- |
| FM-1 | Content drought | hour 1 10.1 min; whole 19.0 min | largest gap with no new content at most 10 min in hour 1, 20 min later (no source) | out | No new content from minute 36 to minute 46 (10.1 min; band 10 min). Guard FM-2 is out: read this row with FM-2. Guard FM-3 is out: read this row with FM-3. | Move 1 milestone or 1 line into minute 36 to minute 46 (content), or lower C3 (unlock spacing). |
| FM-2 | Felt purchase share | 0.50 below +5 %; model part 0/7 | at most 0.2 of hand purchases below +5 % (step inferred from the Weber fraction); model part at most 0 lines (no source) | out | 0.50 of 280 hand purchases raise their income by less than +5 % (band 0.2); the most are gen. | Raise the step d of gen with C6 (upgrade punch), or lower C7 (growth pressure). |
| FM-3 | Line character | distinct share 0.29 | distinct share at least 0.7; drift at least 0.5 (no source) | out | 5 lines are clones in cost or effect: m1, m2, element_1, element_5 and 1 more (distinct share 0.29; band 0.7). | Give m1, m2, element_1, element_5 and 1 more each its own step, cost growth or cap (content), or tag them as 1 list family; no control moves this row. |
| FM-4 | Largest gap between jolts | 12.9 min | at most 20 min (no source) | ok |  |  |
| FM-5 | End arc | tail 0.05 | tail after the last new content at most 0.1 of E (no source) | ok |  |  |
| FM-6 | Chore length and automation timing | 170 presses on gen; hand share hour 1 1.00 | at most 150 hand presses on 1 line before its auto-buy; hand share in hour 1 at least 0.3 (no source) | out | gen gets 170 hand presses before its auto-buy (band 150); hand share in hour 1 1.00 (band 0.3). | Add an automation entry for gen before minute 132 with C14 (auto-buy start). |

## Proof sheet

Result: simulated. Ready: no. All parameters in proven ranges: yes. Strict ranges: on.

Meaning: each row passes or is simulated, not proved, and no part is a stub: the design can go to a build, and a play test must confirm the simulated rows.

Rows in range: 1 (CG). A row in range passes: its value is past the target and inside the range of the target. Many such rows show a design at the edge.

| Id | Obligation | Value | Limit | Status | Law | Source | Fix or reason |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PO1 | R < 1 in each run phase that has a reset layer. R = 1 only with no uncapped additive line | R of the carriers 0.893 + 1 / (k ln b) 0.077 of gen at level 93 (the middle of the length) + the content share 0.037 = 1.007, no reset layer; g of the 2 halves after hour 1: 0.00219 and 0.00193 /s | <= 1 (within 0.05) at the middle of the length | pass | L2 | formula |  |
| PO2 | The reset loop is stable: lambda = p q / (1 - R) <= 1, and lambda = 1 for linear progress |  |  | not applicable | L6 | formula | the model has no reset layer |
| PO3 | A best reset exists: s = p / (1 - R) > 1 |  |  | not applicable | L6 | formula | the model has no reset layer |
| PO4 | The gap between the patient and the early reset rule is at most Dg |  |  | not applicable | L6 | formula | the model has no reset layer |
| PO5 | The run length T_X = X^(1/s) (c / A)^(1/alpha) is within 20% of the session sigma |  |  | not applicable | L6, L8 | formula | the model has no reset layer |
| PO6 | The idle value rho(T) of L4b is in its band at 1 h, 8 h, 24 h and 7 d, open and closed | open 1 h 0.286; open 8 h 0.070; open 24 h 0.029; open 168 h 0.006 / simulation: open 1 h 0.277; open 8 h 0.062; open 24 h 0.026; open 168 h 0.005; closed game: not applicable (no offline gain by design) | other points 0 to 1 | pass | L4 | both |  |
| PO7 | Idle stays below active for each T: p kappa X^(1/p) < e ln(1 + X) | L4 property 3: f <= 1 and R > 0 / simulation: largest idle value 0.277; closed game: not applicable (no offline gain by design) | f <= 1 and R > 0 | pass | L4 | both |  |
| PO8 | No idle state pays more than active play: f_open <= 1 and f_closed <= 1 | f_open = 0.8, f_closed = 0; closed game: not applicable (no offline gain by design) | <= 1 | pass | L4 | formula |  |
| PO9 | A presence tool limits open idle, and no open-idle cap is below the longest absence that must pay | tools: open income share f_open = 0.8 < 1 (scale tool); closed game: not applicable (no offline gain by design) | no open-idle cap below 168 h | pass | L5 | formula |  |
| PO10 | 1 unlock for each session after the first hour: 1 / u <= sigma, with w = e^(alpha ldot / u) | u = 7.50 unlocks for each hour, 1 / u = 8.0 min / simulation: median gap after hour 1 8.6 min | <= 15 min, u in [5, 60] | pass | L3, L8 | both |  |
| PO11 | The first cost a_1 <= 15 I_0, and at least 3 unlocks in the first 5 minutes. An unlock is a new axis step, the first purchase of a line, or a milestone, after t = 0 | a_1 = 10 (gen), I_0 = 1 | <= 15 | pass | L8 | formula |  |
| PO12 | The axis count N gives the length H within 15% | H from N = 30: 2.53 h / simulation: active_greedy finishes at 2.49 h | 2.5 h within 15% | pass | L8 | both |  |
| PO13 | The play-test speed k keeps T_X >= 10 k D_h | T_e = 1 / g = 489 s at x10; largest speed: formula x24, simulation x20 / simulation: median run n/a s, slow-down at the play-test speed 1.02 | >= 200 s | pass | L9 | both |  |
| PC1 | The active player reaches the hard end inside the length target | active_greedy reaches axis step 30 at 2.49 h | 2 to 3 h | pass | S28 | simulation |  |
| PC2 | The largest gap between 2 purchases of the active player is at most the gap target, and the first purchase comes inside the first-purchase target | largest gap 1.42 min; first purchase at 0.17 min | <= about 2 min (range to 2.1 min) | pass | CT2, CT3 | simulation |  |
| PC3 | Each part of the game past the demo end adds new content: a first purchase of a line or a milestone, not only an axis step | no demo end |  | not applicable | CT5 | simulation | no demo end stated (no demo) |
| PC4 | The largest gap between 2 unlocks of the active player (an axis step, a first purchase or a milestone) is at most the unlock gap target over the whole length and in the first hour | largest gap 8.92 min; first hour 4.33 min; 44 unlocks | <= about 10 min (range to 12 min); first hour <= about 5 min (range to 6 min) | pass | CT6 | simulation |  |
| PC5 | The active player buys each purchase line and reaches each milestone before the hard end | 16 of 16 elements reached | each element before the hard end | pass | reachable | simulation |  |
| PC6 | The purchases of the active player in each minute stay at or below the purchase rate target; the automation of the model (S24 auto-buy) takes the rest | busiest minute 5 purchases by hand (5 with the auto-buy purchases); last hour 1.8 for each minute; 0 lines with auto-buy | <= about 30 purchases in 1 minute (range to 36 purchases in 1 minute) | pass | purchase rate | simulation |  |
| PV | At the time when the active player buys it, 1 level of each line raises the income, or the stat that the line affects, by the floor or more | smallest gain 10.0% (element_1); 7 lines, 0 below the floor; gen: the tested gain is that of 1 count block of 10 levels with its step; 1 level gains 0.6% or more (reported, not tested) | >= about 10% for each level, or for each count block of a line with count steps (range to 8.0%) | pass | S02, L1 | both |  |
| CL | Each generator line (system S01) lives for the core of the game: the active player buys it, and it is not at its cap, at half of the length to the hard end or later | gen: level 170, last purchase at 149.6 min (100% of 149.6 min) | the last purchase of each generator line at 50% of the length or later | pass | designer rule, no source | simulation |  |
| WS | Each step of the wall table of the axis after the first is a wall: the ratio of each wall to the wall before it is inside the g range of S03.exponential; the first step is the start of the game | first step x1.41 (first step, not tested); 16 later steps: smallest x1.45, median x1.73, largest x2.44; each later step inside the range | 1.15 <= step <= 2.8 after the first step | pass | S03.exponential g (assets/systems.json) |  |  |
| CG | The largest gap with no new content element of the active player (a first purchase of a line, a first milestone or the first reset; an axis step is not one) is inside the FM-1 band of the fun sheet, in the first hour and over the whole length | largest gap 18.98 min; first hour 10.07 min; 15 content elements; in range: 10.07 min from 36.0 to 46.1 min against a target of about 10 min (no source), inside the range to 12 min | <= about 20 min (range to 24 min); first hour <= about 10 min (range to 12 min) | pass (in range) | FM-1 band of the fun sheet (a target with no source) | simulation |  |
| PU1 | The upkeep factor u of the active player stays inside its band for 90% or more of the play time | u inside 0.69 to 1 for 100% of the time; lowest u 0.783 | >= 90% | simulated, not proved | BM 3b |  |  |
| PU2 | No death spiral: the income of the active player never stops rising for longer than the unlock gap target, and the active player reaches the hard end | longest stall 1.4 min (limit 10 min); exponent 1 | <= 10 min | simulated, not proved | BM 3b |  |  |
| PU3 | 1 care level raises the income by the purchase value floor or more while u < 1 | formula 20.0%; 25 of 25 care purchases gain the floor or close the shortfall | >= 10% | pass | BM 3b, L1 |  |  |
| PU4 | The decision does not end: the purchases of the active player switch between the producer lines and the care lines the stated count of times or more | 46 switches between producer and care purchases | >= 4 | simulated, not proved | BM 3b |  |  |

## Extra conditions

- PO1: g of the 2 halves after hour 1 differ by 12% (limit 20%; simulated, not proved): ok
- PO10: largest gap after hour 1 10.8 min (limit 20.0 min): ok
- PO11: 5 unlocks in the first 5 minutes (limit 3 or more): ok
- PO13: simulated slow-down at x10 = 1.02 (limit < 1.1): ok
- PC2: first purchase at 0.17 min (limit 2 min): ok
- PC4: first hour: largest gap 4.33 min (limit 5 min): ok

## Inputs

econ = coin, R_max = 2.393, R_end = 0.893, p = n/a, q = n/a, alpha = n/a, s = n/a, lam = n/a, A = n/a, c = n/a, T_X = n/a, sigma = 900, f_open = 0.8, f_closed = 0, kappa = n/a, u_model = 7.5, w = 2.666, N = 30, H = 2.5, k = 10

## Play-test speed (R13)

Largest speed that keeps PO13 (formula, T_X / (10 x 2 s)): x24.
Largest tested speed with a slow-down under 1.1 (simulation): x20.
Play-test speed: x20, the smaller value. The formula counts about 1 human decision for each run. The simulation counts each purchase.

