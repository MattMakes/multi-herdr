# Design report (continuous): 1 long run with no reset

Result: ready: each row that decides the verdict passes by a law or inside its target: the design can go to a build.

The selection has no reset layer, no S29 and no S22: 1 long run to the hard end. A game with no reset is a valid design, and its result has the same meaning as that of a game with a reset. The generator has no cap: it is a growth carrier with count steps (BM 3a). The solver sets R = 1 over the carriers at the middle of the length, measures g of I ~ e^(g t) (L2), sets 1 wall ratio w for a near constant unlock rate u = 3600 g / ln w (L3), and the stage count N for the length (L8). The rows of the reset loop (PO2 to PO5) are not applicable: no reset layer.

The selection refuses S12, and the solver still adds count steps (S12.count_steps) to the generator: an uncapped additive line with no step adds nothing to R (L1), so it stops growing against the multipliers. The way out is a generator with a cap, which then fails the row CL (core line life).

## Proved

- PO1: R of the carriers 0.899 + 1 / (k ln b) 0.069 of gen at level 103 (the middle of the length) + the content share 0.036 = 1.004, no reset layer; g of the 2 halves after hour 1: 0.00229 and 0.00199 /s
- PO6: open 1 h 0.281; open 8 h 0.068; open 24 h 0.029; open 168 h 0.006 | simulation: open 1 h 0.273; open 8 h 0.061; open 24 h 0.026; open 168 h 0.005; closed game: not applicable (no offline gain by design)
- PO7: L4 property 3: f <= 1 and R > 0 | simulation: largest idle value 0.273; closed game: not applicable (no offline gain by design)
- PO8: f_open = 0.8, f_closed = 0; closed game: not applicable (no offline gain by design)
- PO9: tools: open income share f_open = 0.8 < 1 (scale tool); closed game: not applicable (no offline gain by design)
- PO10: u = 7.50 unlocks for each hour, 1 / u = 8.0 min | simulation: median gap after hour 1 7.4 min
- PO11: a_1 = 10 (gen), I_0 = 1
- PO12: H from N = 28: 2.54 h | simulation: active_greedy finishes at 2.45 h
- PO13: T_e = 1 / g = 473 s at x10; largest speed: formula x23, simulation x20 | simulation: median run n/a s, slow-down at the play-test speed 1.03
- PV: smallest gain 10.0% (element_1); 6 lines, 0 below the floor; gen: the tested gain is that of 1 count block of 10 levels with its step; 1 level gains 0.6% or more (reported, not tested)
- WS: first step x8.86 (first step, not tested); 14 later steps: smallest x1.39, median x1.79, largest x2.47; each later step inside the range
- S01.geometric: a law of the solver covers the variant
- S02.levelled_factor: a law of the solver covers the variant
- S03.exponential: a law of the solver covers the variant
- S05.active_lane: a law of the solver covers the variant
- S28.last_step: a law of the solver covers the variant

## Simulated, not proved

- PC1: active_greedy reaches axis step 28 at 2.45 h
- PC2: largest gap 1.12 min; first purchase at 0.17 min
- PC4: largest gap 10.05 min; first hour 5.13 min; 41 unlocks; in range: 10.05 min from 90.5 to 100.6 min against a target of about 10 min (no source), inside the range to 12 min; 5.13 min from 27.2 to 32.4 min against a target of about 5 min (no source), inside the range to 6 min
- PC5: 15 of 15 elements reached
- PC6: busiest minute 12 purchases by hand (12 with the auto-buy purchases); last hour 1.8 for each minute; 0 lines with auto-buy
- CL: gen: level 180, last purchase at 145.0 min (98% of 147.3 min)
- CG: largest gap 18.73 min; first hour 9.52 min; 14 content elements
- S04.none: 1 long run: no reset layer, so the rows of the reset loop (PO2 to PO5) are not applicable; the laws of 1 long run prove the timing, and an auto-buy plan (S24) moves the idle row PO6 to the simulation
- S24.auto_buy: the solver writes it; no law covers it

## Failed

- none

## Systems and marks

Each selected system has 1 mark: written and proved by a law, written and checked by the simulation only, or not written (with the reason).

| System | Variant | Mark | Entries | Reason |
| --- | --- | --- | --- | --- |
| S01 Generator line | S01.geometric | written, proved by a law | - | a law of the solver covers the variant |
| S02 Multiplier upgrade | S02.levelled_factor | written, proved by a law | - | a law of the solver covers the variant |
| S03 Spine with a wall | S03.exponential | written, proved by a law | - | a law of the solver covers the variant |
| S04 Reset layer | S04.none | written, simulation only | - | 1 long run: no reset layer, so the rows of the reset loop (PO2 to PO5) are not applicable; the laws of 1 long run prove the timing, and an auto-buy plan (S24) moves the idle row PO6 to the simulation |
| S05 Active action | S05.active_lane | written, proved by a law | - | a law of the solver covers the variant |
| S24 Automation unlock | S24.auto_buy | written, simulation only | - | the solver writes it; no law covers it |
| S28 Hard end | S28.last_step | written, proved by a law | - | a law of the solver covers the variant |

## Targets checked

Checked: PO1 (pass), PO6 (pass), PO7 (pass), PO8 (pass), PO9 (pass), PO10 (pass), PO11 (pass), PO12 (pass), PO13 (pass), PC1 (pass), PC2 (pass), PC4 (pass (in range)), PC5 (pass), PC6 (pass), PV (pass), CL (pass), WS (pass), CG (pass).
Not computed: none.

## Selected systems

- S01: S01.geometric
- S02: S02.levelled_factor
- S03: S03.exponential
- S04: S04.none
- S05: S05.active_lane
- S24: S24.auto_buy (selected; the solver writes no line for it: simulation and judgment, see references/design-solver.md)
- S28: S28.last_step

## Target warnings

- the demo end: not stated (no demo); the proof row PC3 is not applicable
- no source: unlocks in the first 10 minutes and the first hour, an active factor, the count of resets, a play-test speed (SG:198-205); each such target is a designer target, not supported by a source

## Hard end and demo end

- Hard end (S28.last_step): stage 28. PC1: active_greedy reaches axis step 28 at 2.45 h (pass).
- Demo end: no demo.
- PC2: largest gap 1.12 min; first purchase at 0.17 min (pass).
- PC3, the parts past the demo: no demo end (not applicable).

## Gaps between unlocks

PC4: largest gap 10.05 min; first hour 5.13 min; 41 unlocks; in range: 10.05 min from 90.5 to 100.6 min against a target of about 10 min (no source), inside the range to 12 min; 5.13 min from 27.2 to 32.4 min against a target of about 5 min (no source), inside the range to 6 min (pass (in range); limit <= about 10 min (range to 12 min); first hour <= about 5 min (range to 6 min)).

## Purchase value

PV: smallest gain 10.0% (element_1); 6 lines, 0 below the floor; gen: the tested gain is that of 1 count block of 10 levels with its step; 1 level gains 0.6% or more (reported, not tested) (pass; limit >= about 10% for each level, or for each count block of a line with count steps (range to 8.0%)). Floor source: designer target; default 10%: the low end of the proven step d 1.1 to 2 of S02.levelled_factor (assets/systems.json, O:188, D:195).

### What the player buys

| Minutes | Purchases | Unlocks | Smallest gain | Largest gain |
| --- | --- | --- | --- | --- |
| 0 to 10 | 48 | 7 | 2.4% | 62.5% |
| 10 to 20 | 19 | 5 | 1.8% | 54.8% |
| 20 to 30 | 16 | 3 | 1.6% | 54.4% |
| 30 to 40 | 18 | 3 | 1.4% | 54.1% |
| 40 to 50 | 19 | 4 | 1.2% | 53.9% |
| 50 to 60 | 23 | 3 | 1.1% | 53.7% |
| 60 to 70 | 20 | 1 | 0.9% | 53.5% |
| 70 to 80 | 21 | 3 | 0.9% | 53.4% |
| 80 to 90 | 19 | 1 | 0.8% | 53.3% |
| 90 to 100 | 19 | 2 | 0.7% | 53.2% |
| 100 to 110 | 17 | 3 | 0.7% | 53.1% |
| 110 to 120 | 18 | 1 | 0.7% | 53.0% |
| 120 to 130 | 18 | 2 | 0.6% | 52.9% |
| 130 to 140 | 13 | 2 | 0.6% | 52.9% |
| 140 to 150 | 15 | 1 | 0.6% | 52.8% |

## Reachable

PC5: 15 of 15 elements reached (pass).

## Purchase rate

PC6: busiest minute 12 purchases by hand (12 with the auto-buy purchases); last hour 1.8 for each minute; 0 lines with auto-buy (pass; limit <= about 30 purchases in 1 minute (range to 36 purchases in 1 minute)). The target is the most purchases that a person makes in 1 minute with no automation (designer target).

## Parameters

| Parameter | Value | Range | Source | Set by |
| --- | --- | --- | --- | --- |
| N | 28 | derived | BM L8: N = u_1 + ceil((ln P(1 h) - ln h_u1) / ln w + u (H - 1)) | PO12 (length) |
| R | 0.8987 | [0, 1] | BM 3a: the carriers share R = 1 - 1 / (k_mid ln b) - the content share; with the factor k of the generator and the constant factors of the named content the sum is 1 at the middle of the length, so I ~ e^(g t) (L2) and 1 wall ratio gives a near constant unlock rate (L3) | PO1, PO10 and PO12 |
| S01.geometric.b | 1.15 | [1.07, 1.15] | O:48-49 | S01 default (the cost ratio of the generator) |
| S02.levelled_factor.b | 1.375 | [1.15, 10] | O:188, D:195 | moved from the default to the finest steps that keep d in range (PO5, BM section 7 limit 6) |
| S02.levelled_factor.d | 1.1 | [1.1, 2] | O:188, D:195 | d = b^r with r = R / 3 = 0.2996 (BM L1, PO1) |
| wall table steps (not g) | smallest 1.393, median 1.812, largest 8.857 | [1.145, 2.8] | the wall table of the timing fit (16 walls) | WS |
| S05.active_lane.a | 1.25 | [1.25, 1.25] | TB:61, TA:507 | S05 default |
| S12.count_steps.m | 1.52 | [1.025, 4] | O:202-203, A:117, B:157 | m = b^(r n) with r = 0.2996 of 1 carrier and b = 1.15 of the generator (BM 3a: r_eff = ln m / (n ln b)) |
| S12.count_steps.n | 10 | [10, 100] | O:202-203, A:117, B:157 | the finest steps of the proven range (BM section 7, limit 6) |
| cost_scale | 34.48 | derived | BM 3a: g = a v k prod (r (b_c - 1) / a_c)^r = 0.09695 / F^0.579 with k = k_mid = 74.4; times (the measured g / g_goal)^(1 / share) (the simulator in the loop) | g = g_goal (PO10, PO13) |
| f_open | 0.8 | [0, 1] | BM L5: an active income term a is the scale tool f = 1 / a | PO8, PO9 and the idle targets |
| first_stages | [3.5, 31, 73, 180, 260, 600, 850, 1700, 2800, 3900, 9400, 1.6e+04, 2.9e+04, 5.1e+04, 7.4e+04, 1.8e+05] | derived | the trajectory of active_greedy at the target times (BM L8 step 4) | PO11 and the first-hour unlock target |
| g_goal | 0.002145 | derived | u ln(g of S03, default) / 3600 with u the unlock target (L3) | PO10 at the default wall ratio |
| g_run | 0.002116 | derived | BM L2: the slope of ln I from hour 1 to H in the run with no stage (pw_proof.run_growth) | PO6, PO10, PO12 and PO13 |
| k_mid | 108 | derived | the generator level at the middle of the length: L1 first, then the run with no stage (pw_proof.step_inputs) | PO1 (R = 1 at the middle of the length) |
| r_group | 0.2996 | derived | BM L1: R is the sum of the largest r of each group | PO1 |
| u | 7.5 | [7.5, ] | BM L8: u = alpha ldot / ln w | PO10 |

## Idle value against active play

- Formula (L4b): open 1 h 0.281; open 8 h 0.068; open 24 h 0.029; open 168 h 0.006
- Simulation: open {'1': 0.2727, '8': 0.0614, '24': 0.0256, '168': 0.0049}; closed: not applicable (no offline gain by design)

## Play-test speed

Largest speed (formula): x23. Largest tested speed (simulation): x20. Play tests run at x20 or slower: the smaller value.

## Gaps between content elements

CG: largest gap 18.73 min; first hour 9.52 min; 14 content elements (pass; limit <= about 20 min (range to 24 min); first hour <= about 10 min (range to 12 min)). An axis step and an empty content slot are not content elements. The band is the FM-1 band of the fun sheet: a target with no source, so the row has the range of such a target. The row decides the result.

## Named content

The content file gives the named elements. An element that is open at the start is a line of the solver with its name. Each other element has its unlock point (a lifetime total) and its first cost from the run of the active player at its target minute. A second generator line takes the count steps of the generator. Each other named line is a side line with a cap: it adds a constant factor and nothing to R (L7).

| Id | Name | Model entry | Target (min) | Active player (min) |
| --- | --- | --- | --- | --- |
| gen | Workshop | gen | 0 | 0.2 |
| m1 | Better tools | m1 | 0 | 6.7 |
| m2 | Faster work | m2 | 0 | 16.8 |
| element_1 | Tool rack | element_1 | 9 | 9.7 |
| element_2 | First district | element_2 | 18 | 18.0 |
| element_3 | Better layout | element_3 | 27 | 27.0 |
| element_4 | Second district | element_4 | 36 | 36.0 |
| element_5 | Training | element_5 | 45 | 45.5 |
| element_6 | Third district | element_6 | 54 | 54.0 |
| element_7 | Night shift | element_7 | 72 | 72.0 |
| element_8 | Fourth district | element_8 | 90 | 90.0 |
| element_9 | Fine tuning | element_9 | 108 | 108.7 |
| element_10 | Fifth district | element_10 | 126 | 126.0 |
| element_11 | Last district | element_11 | 142 | 142.0 |

## Fun rows

The fun sheet (scripts/pw_fun.py, references/fun-analysis.md) gives a second result beside the proof result. It is advice with evidence marks, not a proof: a fun row never makes the result ready or not ready.

Fun result (FM-1 to FM-8): likely dull; counting rows out: FM-2, FM-3, FM-6.

| Id | Name | Value | Band (mark) | Status | Cause | Change to try |
| --- | --- | --- | --- | --- | --- | --- |
| FM-1 | Content drought | hour 1 9.5 min; whole 18.7 min | largest gap with no new content at most 10 min in hour 1, 20 min later (no source) | ok |  |  |
| FM-2 | Felt purchase share | 0.48 below +5 %; model part 0/6 | at most 0.2 of hand purchases below +5 % (step inferred from the Weber fraction); model part at most 0 lines (no source) | out | 0.48 of 303 hand purchases raise their income by less than +5 % (band 0.2); the most are gen. | Raise the step d of gen with C6 (upgrade punch), or lower C7 (growth pressure). |
| FM-3 | Line character | distinct share 0.17 | distinct share at least 0.7; drift at least 0.5 (no source) | out | 5 lines are clones in cost or effect: m1, m2, element_1, element_5 and 1 more (distinct share 0.17; band 0.7). | Give m1, m2, element_1, element_5 and 1 more each its own step, cost growth or cap (content), or tag them as 1 list family; no control moves this row. |
| FM-4 | Largest gap between jolts | 12.3 min | at most 20 min (no source) | ok |  |  |
| FM-5 | End arc | tail 0.04 | tail after the last new content at most 0.1 of E (no source) | ok |  |  |
| FM-6 | Chore length and automation timing | 180 presses on gen; hand share hour 1 1.00 | at most 150 hand presses on 1 line before its auto-buy; hand share in hour 1 at least 0.3 (no source) | out | gen gets 180 hand presses before its auto-buy (band 150); hand share in hour 1 1.00 (band 0.3). Guard FM-9 is out: read this row with FM-9. | Add an automation entry for gen before minute 119 with C14 (auto-buy start). |

## Proof sheet

Result: ready. Ready: yes. All parameters in proven ranges: yes. Strict ranges: on.

Meaning: each row that decides the verdict passes by a law or inside its target: the design can go to a build.

Rows in range: 1 (PC4). A row in range passes: its value is past the target and inside the range of the target. Many such rows show a design at the edge.

| Id | Obligation | Value | Limit | Status | Law | Source | Fix or reason |
| --- | --- | --- | --- | --- | --- | --- | --- |
| PO1 | R < 1 in each run phase that has a reset layer. R = 1 only with no uncapped additive line | R of the carriers 0.899 + 1 / (k ln b) 0.069 of gen at level 103 (the middle of the length) + the content share 0.036 = 1.004, no reset layer; g of the 2 halves after hour 1: 0.00229 and 0.00199 /s | <= 1 (within 0.05) at the middle of the length | pass | L2 | formula |  |
| PO2 | The reset loop is stable: lambda = p q / (1 - R) <= 1, and lambda = 1 for linear progress |  |  | not applicable | L6 | formula | the model has no reset layer |
| PO3 | A best reset exists: s = p / (1 - R) > 1 |  |  | not applicable | L6 | formula | the model has no reset layer |
| PO4 | The gap between the patient and the early reset rule is at most Dg |  |  | not applicable | L6 | formula | the model has no reset layer |
| PO5 | The run length T_X = X^(1/s) (c / A)^(1/alpha) is within 20% of the session sigma |  |  | not applicable | L6, L8 | formula | the model has no reset layer |
| PO6 | The idle value rho(T) of L4b is in its band at 1 h, 8 h, 24 h and 7 d, open and closed | open 1 h 0.281; open 8 h 0.068; open 24 h 0.029; open 168 h 0.006 / simulation: open 1 h 0.273; open 8 h 0.061; open 24 h 0.026; open 168 h 0.005; closed game: not applicable (no offline gain by design) | other points 0 to 1 | pass | L4 | both |  |
| PO7 | Idle stays below active for each T: p kappa X^(1/p) < e ln(1 + X) | L4 property 3: f <= 1 and R > 0 / simulation: largest idle value 0.273; closed game: not applicable (no offline gain by design) | f <= 1 and R > 0 | pass | L4 | both |  |
| PO8 | No idle state pays more than active play: f_open <= 1 and f_closed <= 1 | f_open = 0.8, f_closed = 0; closed game: not applicable (no offline gain by design) | <= 1 | pass | L4 | formula |  |
| PO9 | A presence tool limits open idle, and no open-idle cap is below the longest absence that must pay | tools: open income share f_open = 0.8 < 1 (scale tool); closed game: not applicable (no offline gain by design) | no open-idle cap below 168 h | pass | L5 | formula |  |
| PO10 | 1 unlock for each session after the first hour: 1 / u <= sigma, with w = e^(alpha ldot / u) | u = 7.50 unlocks for each hour, 1 / u = 8.0 min / simulation: median gap after hour 1 7.4 min | <= 15 min, u in [5, 60] | pass | L3, L8 | both |  |
| PO11 | The first cost a_1 <= 15 I_0, and at least 3 unlocks in the first 5 minutes. An unlock is a new axis step, the first purchase of a line, or a milestone, after t = 0 | a_1 = 10 (gen), I_0 = 1 | <= 15 | pass | L8 | formula |  |
| PO12 | The axis count N gives the length H within 15% | H from N = 28: 2.54 h / simulation: active_greedy finishes at 2.45 h | 2.5 h within 15% | pass | L8 | both |  |
| PO13 | The play-test speed k keeps T_X >= 10 k D_h | T_e = 1 / g = 473 s at x10; largest speed: formula x23, simulation x20 / simulation: median run n/a s, slow-down at the play-test speed 1.03 | >= 200 s | pass | L9 | both |  |
| PC1 | The active player reaches the hard end inside the length target | active_greedy reaches axis step 28 at 2.45 h | 2 to 3 h | pass | S28 | simulation |  |
| PC2 | The largest gap between 2 purchases of the active player is at most the gap target, and the first purchase comes inside the first-purchase target | largest gap 1.12 min; first purchase at 0.17 min | <= about 2 min (range to 2.1 min) | pass | CT2, CT3 | simulation |  |
| PC3 | Each part of the game past the demo end adds new content: a first purchase of a line or a milestone, not only an axis step | no demo end |  | not applicable | CT5 | simulation | no demo end stated (no demo) |
| PC4 | The largest gap between 2 unlocks of the active player (an axis step, a first purchase or a milestone) is at most the unlock gap target over the whole length and in the first hour | largest gap 10.05 min; first hour 5.13 min; 41 unlocks; in range: 10.05 min from 90.5 to 100.6 min against a target of about 10 min (no source), inside the range to 12 min; 5.13 min from 27.2 to 32.4 min against a target of about 5 min (no source), inside the range to 6 min | <= about 10 min (range to 12 min); first hour <= about 5 min (range to 6 min) | pass (in range) | CT6 | simulation |  |
| PC5 | The active player buys each purchase line and reaches each milestone before the hard end | 15 of 15 elements reached | each element before the hard end | pass | reachable | simulation |  |
| PC6 | The purchases of the active player in each minute stay at or below the purchase rate target; the automation of the model (S24 auto-buy) takes the rest | busiest minute 12 purchases by hand (12 with the auto-buy purchases); last hour 1.8 for each minute; 0 lines with auto-buy | <= about 30 purchases in 1 minute (range to 36 purchases in 1 minute) | pass | purchase rate | simulation |  |
| PV | At the time when the active player buys it, 1 level of each line raises the income, or the stat that the line affects, by the floor or more | smallest gain 10.0% (element_1); 6 lines, 0 below the floor; gen: the tested gain is that of 1 count block of 10 levels with its step; 1 level gains 0.6% or more (reported, not tested) | >= about 10% for each level, or for each count block of a line with count steps (range to 8.0%) | pass | S02, L1 | both |  |
| CL | Each generator line (system S01) lives for the core of the game: the active player buys it, and it is not at its cap, at half of the length to the hard end or later | gen: level 180, last purchase at 145.0 min (98% of 147.3 min) | the last purchase of each generator line at 50% of the length or later | pass | designer rule, no source | simulation |  |
| WS | Each step of the wall table of the axis after the first is a wall: the ratio of each wall to the wall before it is inside the g range of S03.exponential; the first step is the start of the game | first step x8.86 (first step, not tested); 14 later steps: smallest x1.39, median x1.79, largest x2.47; each later step inside the range | 1.15 <= step <= 2.8 after the first step | pass | S03.exponential g (assets/systems.json) |  |  |
| CG | The largest gap with no new content element of the active player (a first purchase of a line, a first milestone or the first reset; an axis step is not one) is inside the FM-1 band of the fun sheet, in the first hour and over the whole length | largest gap 18.73 min; first hour 9.52 min; 14 content elements | <= about 20 min (range to 24 min); first hour <= about 10 min (range to 12 min) | pass | FM-1 band of the fun sheet (a target with no source) | simulation |  |

## Extra conditions

- PO1: g of the 2 halves after hour 1 differ by 13% (limit 20%; simulated, not proved): ok
- PO10: largest gap after hour 1 10.5 min (limit 20.0 min): ok
- PO11: 4 unlocks in the first 5 minutes (limit 3 or more): ok
- PO13: simulated slow-down at x10 = 1.03 (limit < 1.1): ok
- PC2: first purchase at 0.17 min (limit 2 min): ok
- PC4: first hour: largest gap 5.13 min (limit 5 min): ok

## Inputs

econ = coin, R_max = 2.399, R_end = 0.8987, p = n/a, q = n/a, alpha = n/a, s = n/a, lam = n/a, A = n/a, c = n/a, T_X = n/a, sigma = 900, f_open = 0.8, f_closed = 0, kappa = n/a, u_model = 7.5, w = 2.761, N = 28, H = 2.5, k = 10

## Play-test speed (R13)

Largest speed that keeps PO13 (formula, T_X / (10 x 2 s)): x23.
Largest tested speed with a slow-down under 1.1 (simulation): x20.
Play-test speed: x20, the smaller value. The formula counts about 1 human decision for each run. The simulation counts each purchase.

