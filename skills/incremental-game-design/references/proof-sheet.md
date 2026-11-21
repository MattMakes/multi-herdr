# The proof sheet

`scripts/pw_proof.py` computes the 13 proof obligations (PO1 to PO13) of `references/balance-math.md` for a model or for a scanned game. The proof sheet is the gate of the skill: the skill does not present a design as ready when 1 obligation is not `pass` or `not applicable`.

The symbols are the symbols of `references/balance-math.md` (section 1, definitions D1 to D12). This file does not change them.

## Command line

```
python3 scripts/pw_proof.py <model.json> [--json FILE] [--speed] [--strict-ranges]
python3 scripts/pw_proof.py --systems <systems-out.json> [--pattern continuous|check_in] [--R VALUE] [--json FILE] [--strict-ranges]
```

- `<model.json>`: the full sheet. The script computes the law values with the closed formulas and runs the players of the simulator (`pw_players.py`) for the measured values. The run takes about 1 second for each test model.
- `--systems FILE`: the sheet of a built game from the catalog map of `scripts/scan_godot.py <project> --systems-out FILE`. The map holds no simulation, so each obligation that needs a measured value is `not computed`.
- `--pattern continuous|check_in`: with `--systems` only. The play pattern of the game sets the core set of the core rows: `continuous` tests S01, S02, S03, S05, S24 and S28 (no presence gate), and `check_in` tests S01 to S06 and the presence-gate slot (`play_patterns` of `assets/systems.json`). The script does not guess the pattern. With no `--pattern`, the sheet has 1 failed row `play pattern` (not set), and 1 row with the status `pattern not set` for each missing core system of each pattern. A model has `game.play_pattern`, so `--pattern` with a model is a usage error.
- `--R VALUE`: with `--systems` only. The R of the run phase that has the reset layer. With no `--R`, the script uses the upper bound from the lines of the map.
- `--json FILE`: write the sheet as JSON (see "JSON").
- `--speed`: add the largest play-test speed that keeps PO13 (requirement R13). The formula gives `floor(T_X / (10 x D_h))`. The speed test of the players runs at 9 speeds and gives the largest tested speed with a slow-down under 1.1. The block also gives `play_test_speed`, the smaller of the 2 values (x1 when no tested speed is under 1.1).
- `--strict-ranges`: a parameter outside its proven range that an obligation uses makes the sheet not ready (exit code 1). Use it for a new design: the design solver must take each parameter from a proven range. The default mode stays usable for a built game and for worked example 2.

Exit code: 0 when the result is `ready`, 3 when it is `simulated`, 1 when it is `not ready`, 2 for a usage error, a missing file, or a model error (section "The result").

## Rows and statuses

Each row has the id, the statement in plain words, the value, the limit, the status, the law, the source of the value, and for a failure the smallest change that fixes it. The rows are in this order:

1. With `--systems`: 1 failed row for each core system of the play pattern that the map does not show as found, and for the presence-gate slot when the pattern is `check_in` and no gate is found. A core system that the map does not hold (S28 in a scan of version 1) is missing, and its value says "the scan does not test it: confirm in the code".
2. The 13 obligations.
3. With `game.play_pattern` `continuous` (model mode only): the 4 pattern rows PC1 to PC4 (see "Rows of the play pattern `continuous`").
4. With the model key `character` (a rebalance of the design solver): the row CK (see "Row CK: character kept").
   Before it, with a play pattern and a simulation: the row CL (see "Row CL: core line life").
   After it, with the play pattern `continuous` and a simulation: the row CG (see "Row CG: content gaps").
5. 1 row with the status `simulation only` for each system that the laws do not cover (section 7 of the balance math).
6. 1 row with the status `outside proven range` for each parameter outside its proven range in `assets/systems.json`.
7. With the model key `simulation_path`: 1 row `part: <variant>` of the kind `simulated` for each part that no law proves (`references/simulation-path.md`).

| Status | Meaning | Ready |
| --- | --- | --- |
| `pass` | The obligation holds | yes |
| `pass (in range)` | A target row: the value is past the target and inside the range of the target (see "Target rows have a range"). The value text gives each miss | yes |
| `not applicable` | The obligation has no meaning for the game, for example PO2 with no reset layer. The row states the reason | yes |
| `fail` | The obligation does not hold. The row gives the smallest change that fixes it | no |
| `not computed` | An input is missing, or the law does not apply. The row states the reason | no |
| `model and math differ` | The formula passes, but the simulation differs from it by more than 20% | no |
| `simulation only` | A system that the laws do not cover. Only the simulation checks it | not counted |
| `outside proven range` | A warning. The row names each obligation that uses the parameter | not counted; with `--strict-ranges`, a row that names an obligation makes the sheet not ready |
| `simulated, not proved` | No law covers the row or the part, and the simulation meets the limit. Never `pass` | the result is `simulated`: the design can go to a build, and a play test must confirm the row |
| `stub: fill by hand` | A part of `simulation_path` that a person must write | no; the result is `not ready` |
| `not modelled` | A part of `simulation_path` that the model does not need, with the reason (for example `S04.none`) | not counted |

The sheet is ready when each row that decides the verdict is `pass`, `pass (in range)` or `not applicable` and no core row fails. The rows that decide the verdict are the kinds `obligation`, `pattern`, `purchase` (PV, CL, CK), `upkeep` and `content` (CG) (`READY_KINDS`). The JSON field `all_in_proven_ranges` is true when the sheet has no `outside proven range` row.

### How the status is decided

A wrong `pass` is the worst error of the sheet. So these rules apply in this order:

1. The formula decides when it is computed. The simulation is then a check: a difference above 20% is a finding, and a `pass` becomes `model and math differ`.
2. When the formula is not computed, a **target** obligation (PO6, PO10, PO11, PO12, PO13) takes the status of the simulation.
3. When the formula is not computed, a **law** obligation (PO1 to PO5, PO7, PO8, PO9) takes only a failed simulation. A simulation that is inside the limit does not prove a law, so the status is `not computed`.
4. An extra condition that only the simulation measures (the largest gap of PO10, the first 5 minutes of PO11) makes the row `fail` when it fails, and `not computed` when it did not run.
5. When the median later run of early_reset is within 5% of `min_run_seconds` (the run floor), L6 does not set the run length. PO5, PO10, PO12 and PO13 then show `formula not valid here (...); simulated value used`, and the simulation decides the status (PO5: the simulated run against the band of sigma).
   The same rule applies when the persistent factors of the runs differ from those of the trajectory: `F_held / F_traj`
   or its inverse is above `(1 + 0.2)^(1 / (1 - R))` (`factor_bound`). One A of L6 then does not hold for the runs.
   The trajectory with no reset (A, `F_traj`) does not count the ladder grants of the economy currency: a grant of
   50 x 2^n coin at each stage made A 14 times too large in a test game (T_X 128 s by the formula, 488 s simulated).
   The same rule applies to PO4, PO10 and PO12 when the walls come from the timing fit (the model key
   `walls_fitted`: 1 wall for each stage, not 1 ratio w of L8; `WALL_ROWS`). The row then says "the walls of the
   stages come from the timing fit by simulation".
6. When the reset loop has `lambda > 1` (or `R >= 1`), runs get shorter with no limit (L6). So PO4, PO5 and PO13 fail with no number, and PO6, PO7, PO10 and PO12 have no formula.
7. A model with no reset layer is outside the laws of the reset loop. A row that only the simulation decides gets `simulated, not proved` when the simulation meets its limit (`no_law`). A failed row stays `fail`.
8. 1 long run (section "1 long run with R = 1") is the exception of rule 7: the formulas of that section compute PO6, PO10, PO12 and PO13, so rule 1 applies to them. PO2 to PO5 are `not applicable` with the reason "no reset layer".

## The result

The sheet line starts with "Result: x." and the JSON key `result` holds x. The next line "Meaning: ..." gives the text of the table (`RESULT_TEXT`, JSON key `result_text`). x has 3 values:

| Result | Condition | Meaning | Exit code |
| --- | --- | --- | --- |
| `ready` | Each row that decides the verdict is `pass`, `pass (in range)` or `not applicable` | The design can go to a build | 0 |
| `simulated` | Each row that decides the verdict passes or is `simulated, not proved`, 1 or more rows are `simulated, not proved`, and no part is a stub | The design can go to a build, and a play test must confirm the simulated rows | 3 |
| `not ready` | A row fails, a row is not computed, or a part is a stub | Do not build from this design | 1 |

`simulated` is not a failed result. It says that the simulated player met each target, and that 1 or more rows have no law. The report lists these rows under "Simulated, not proved": they are the list for the play test. The result is more robust when `pw_scaffold.py --vary` holds each target and 2 buyers agree (`references/simulation-path.md`, "How to judge a simulated result"). A row with a law that fails still makes the result `not ready`.

A game with no reset layer is a valid design. Its result has the same meaning as the result of a game with a reset: the sheet does not rate it lower. Its rows of the reset loop (PO2 to PO5) are `not applicable`, and `not applicable` counts as `pass`.

## Target rows have a range

A target is a range, not an exact limit. A value of 10.30 minutes against a target of 10 minutes is not a fault of the design when no game measures that target. The sheet has 1 mechanism for it (`zone`, `tolerance_of`).

1. Each target limit `L` has a tolerance. It depends on the source of the target (`TOLERANCE`): 0.20 for a target with no game source (a designer target, or a target that the model lists in `game.targets.unsupported`), and 0.05 for a target with a source. `game.targets.tolerances` sets the tolerance of 1 target (`references/model-schema.md`).
2. A row has 3 zones. Inside `L`: `pass`. Past `L` and inside `L x (1 + tolerance)` (a floor: `L x (1 - tolerance)`): `pass (in range)`, with the text of the miss in the value, for example "10.30 min from 141.9 to 152.2 against a target of about 10 min (no source), inside the range to 12 min". Outside the range: `fail`. Only `fail` changes the result.
3. The limit of such a row is printed as "about L (range to X)".
4. The sheet prints the line "Rows in range: n (ids)", and the JSON key `in_range_rows` holds the ids. A design with many rows in range is at the edge of its targets. Each row has the JSON key `in_range`: 1 text for each miss.

| Row | Limit | Kind of limit | Source class | Default tolerance |
| --- | --- | --- | --- | --- |
| PC2 | the largest purchase gap, the first purchase | target, range | sourced (`SG:180-181`) | 0.05 |
| PC4 | the unlock gap of the whole length, of the first hour, and of the last gap | target, range | no source (`SG:198`) | 0.20 |
| PC6 | the purchases in 1 minute | target, range | no source | 0.20 |
| PV | the purchase value floor | target, range (a floor) | no source (a designer target) | 0.20 |
| CG | the FM-1 band of the fun sheet | target, range | no source | 0.20 |
| PC1, PO12 | the length | target with its own band: `length_hours` holds the tolerance of the designer, and PO12 its 15% | the band is the range | none added |
| PO5, PO6, PO10, PO11, PO13 | sigma, the idle bands, the unlock rate, the first cost, the play-test speed | target with its own band or rule | the band is the range | none added |
| PO1 to PO4, PO7, PO8, PO9 | `R <= 1`, `lambda <= 1`, `s > 1`, `f <= 1`, a presence tool | law, exact | - | 0 |
| PC3, PC5, CL, CK | an empty content slot, an element not reached, a generator line that ends early, the character | rule, exact | - | 0 |
| a part of `simulation_path` | a stub | exact | - | 0 |

A law row has no range: `R = 1.01` with a reset layer fails PO1. The design solver aims its stages at 80% of the unlock gap targets (`UNLOCK_SHARE`), so a solved design has room before the limit. The timing fit (`pw_timing.py`) still aims at the limit.

## Sources of each value

"Model" is a field of the model (`references/model-schema.md`). "Target" is a field of `game.targets`. "Simulation" is a result of the players.

| Code | Inputs and their source | Value | Limit |
| --- | --- | --- | --- |
| PO1 | r of each line (model: `cost_growth`, the `mult` value; an additive line with `count_steps`: `r_eff = ln m / (n ln b)`). The lines are the carriers paid in the economy currency: the `mult` lines that multiply it, and the additive lines with count steps. The economy currency is `prestige.basis_currency`, else `game.main_currency`, else the currency that pays for the most purchases | Formula: `R` = the sum of `r` of these lines, the largest R of the run (each line open at once). Simulation: the R of each phase of active_greedy, from the first purchase and the cap time of each line. This is information and not a check | `< 1` with a reset layer. With no reset layer: R after the caps `< 1`, or `= 1` with no uncapped additive line. With a generator carrier (each uncapped additive line has count steps, no reset): `R + 1 / (k_mid ln b) + R_content <= 1` within 0.05 |
| PO2 | R (PO1); p (model: `prestige.exponent`, formula `power`); q (model: `prestige.reward.exponent`, plus r of each uncapped `mult` line paid in the reset currency) | Formula: `lambda = p q / (1 - R)`. With `R >= 1`, no limit. Simulation: from the run lengths of early_reset, `T_n ~ L_n^e` with `e = (1 - lambda) / s`, so `lambda = q / (e + q)`. The fit skips the first run and each run that the buyer ended at `min_run_seconds` | `<= 1` |
| PO3 | p; R after the caps (the uncapped lines only, the smallest R of the run) | Formula: `s = p / (1 - R)` | `> 1` |
| PO4 | s (with the largest R); `X_e = 1` and `X_p = 10` (the players early_reset and patient_reset); Dg (target: `policy_gap` high end, default 2) | Formula: `(ln(1 + X_p) / ln(1 + X_e)) (X_e / X_p)^(1/s)`. Simulation: the policy gap of the players | `<= Dg` |
| PO5 | s, alpha; c = `prestige.base x k^(-1/p)` (model); A (simulation: the run earnings of a run with no reset after 10^6 s, `A = E / x^alpha`, as `traj_A` of `scripts/verify_math.py`, times `F_held / F_traj`: F is the product of the persistent factors on the economy currency (each persistent milestone, each level of a line that the reset keeps), `F_traj` at the end of that run, `F_held` at the start of the median later run of early_reset); sigma (model: `session_minutes`) | Formula: `T_X = X_e^(1/s) (c / A)^(1/alpha) / K`, with `K` the product of `((b - 1) / a1)^r` of the uncapped lines of the reset currency (BM L6, the constant `K` of D8; the value shows `K` when it is not 1). Simulation: the median run length of early_reset over the later half of its runs | `sigma` within 20% |
| PO6 | kappa = `f alpha (V / T_X)^(alpha - 1)`; f_open, f_closed (PO8); V (model: `presence.visit_minutes`); the closed cap (model: `presence.offline_cap_minutes`); the bands (target: `idle`, else 0 to 1) | Formula: rho of L4b at 1 h, 8 h, 24 h and 168 h. Closed: `T -> min(T, T_cap)`, as `ex2_idle_pred` of `scripts/verify_math.py`. The formula needs `|lambda - 1| <= 0.05`. Simulation: the idle value of the 8 players open_1h to closed_168h | each point in its band |
| PO7 | p, kappa (the larger of open and closed), X_e | Formula for `p > 1`: `p kappa X^(1/p)` against `e ln(1 + X)`. The value shown is `rho_max`, the quotient. For `p <= 1`: the largest rho of L4b over the tested waits inside the domain of L4b (see "Domain of L4b"). With no reset layer: L4 property 3 (`f <= 1` and `R > 0`). Simulation: the largest idle value; for `p <= 1`, the idle value at the point of the formula | `< e ln(1 + X)`; for `p <= 1`, `< 1` |
| PO8 | The away share: the income of the economy currency with no active-only effect, divided by the full income, at the start (simulation state at t = 0); `presence.open_rate`; `game.offline_rate` | Formula: `f_open = open_rate x share`, `f_closed = offline_rate x share` With an active-only effect on the economy currency that is not in effect at the start (a purchase, or a milestone with a requirement): the start share is not the open share, so the simulation decides: the largest open idle value of the players | `<= 1` |
| PO9 | f_open; `presence.open_cap_minutes`; `bank_cap` of each currency; ladder timers (`attempt_seconds`, `cooldown_seconds`, `needs_visit`); the longest absence that must pay (the largest open idle target, at least 24 h) | Rule of L5: a presence tool limits open idle (a scale tool `f_open < 1`, an open cap, a bank cap, or a spine step that needs a visit), and no open cap is shorter than the longest absence With an active-only effect that is not in effect at the start and no other tool, the simulation decides: the largest open idle value is below 1 | 1 tool or more, no open cap below the absence |
| PO10 | alpha, q, T_X, X_e; w (model: `requires.growth` of the axis); sigma; `unlocks_per_hour` (target, optional) | Formula: `ldot = 3600 q ln(1 + X) / T_X`, `u = alpha ldot / ln w` (the inverse of the L8 formula for w), and `1 / u`. With a target band, u is also in the band. Simulation: the median gap between 2 axis steps of active_greedy after hour 1. Extra condition (simulation): the largest gap after hour 1 is at most `20 / 15 x sigma` (target T2 of `assets/systems.json`: median at most 15 min, largest at most 20 min) | `1 / u <= sigma` |
| PO11 | a_1: the cheapest cost at level 0 of a line with no requirement, paid in the economy currency (model); I_0: the full income of the economy currency at the start (simulation state at t = 0). Extra condition (simulation): the unlocks of active_greedy in the first 300 s | Formula: `a_1` against `15 I_0` | `a_1 <= 15 I_0`, and at least 3 unlocks in the first 5 minutes |
| PO12 | H (target: the middle of `length_hours`); N (model: the axis `count`); w; u (PO10); u_1 and P(1 h) (simulation: the axis steps of active_greedy in hour 1, and its highest axis power in hour 1) | Formula: `H = 1 + (N - u_1 - ln(P(1 h) / h_u1) / ln w) / u`, the L8 formula for N solved for H. Simulation: the hours of active_greedy to the end of the axis | `H` within 15% |
| PO13 | T_X (PO5); k (target: `play_test_speed`, default 10); `D_h = 2` s | Formula: T_X against `10 k D_h`. Simulation: the median run of early_reset, and the slow-down of the speed test at k (under 1.1). The value shows the largest speed of the formula and of the simulation | `T_X >= 10 k D_h`, and the extra condition: slow-down at k under 1.1 |

**Unlock.** An unlock is an event that gives the player a new thing: a new step of the progress axis, the first purchase of a line, or a milestone, after t = 0. PO11 counts the unlocks of active_greedy in the first 5 minutes with this definition. PO10 and PO12 count axis steps only, as L8 does.

**Formula and simulation both.** PO2, PO4, PO5, PO6, PO7, PO10, PO12 and PO13 show both values when both are computed. The tolerance of the comparison is 20%. For a point of the idle curve (PO6), the tolerance is 20% plus 1 / (visits of the idle player): that player finishes the axis at a visit, so its rate has that resolution. PO1 shows the measured phases as information only, because the largest R of the run and the R of 1 measured phase are different quantities.

## 1 long run with R = 1

A model with no reset layer (no prestige block, or `prestige.enabled` false) is 1 long run in 2 cases
(`pw_proof.long_law`, BM section 3a):

1. R after the caps is within 0.01 of 1, and no additive line of the economy is uncapped.
2. **A generator carrier.** Each uncapped additive line of the economy has count steps (`count_steps`). The sheet runs the model with no reset and no stage, and reads the level `k_mid` of the line at
   the middle of `game.target_hours` (`step_inputs`). The case holds when `R + 1 / (k_mid ln b)` is within 0.05 of 1
   (`LONG_STEP_TOL`), with `R` the sum of `r` of the carriers and `b` the cost ratio of the line. A second
   generator line with count steps is in the group of the first (D6), so `R` takes the largest `r_eff` of these
   lines 1 time. With named content, the sum also holds the content share `R_content`: the sum of `ln F` of the
   constant factors of the content (each `mult` effect of a milestone with a requirement, each capped `mult` line
   at its cap) over `g x` the length (BM 3a, "Content in 1 long run"). The JSON inputs `k_mid`, `R_count` and
   `R_content` hold the values. When `R_content` is above 0.05 (`LONG_STEP_TOL`), or the model has 2 generator lines, PO6 has no
   formula: the idle formula takes the carriers as the whole growth, so the simulation decides it.

L2 gives `I ~ e^(g t)` for it. The sheet measures `g` in the run of the model with no reset and no stage: the slope
of `ln I` of the economy currency from hour 1 to `game.target_hours` (`run_growth`; the JSON inputs `g_run` and
`T_e = 1 / g`). The formulas:

| Row | Formula | Limit |
| --- | --- | --- |
| PO1 | R after the caps (as for each model with no reset). Case 2: `R + 1 / (k_mid ln b)` | `= 1` with no uncapped additive line. Case 2: `<= 1` within 0.05, and `g` of the 2 halves within 20% |
| PO2 to PO5 | none: `not applicable`, reason "no reset layer" | none |
| PO6 | `rho(T) = (ln(1 + f g T) + g V) / (g (T + V))` at each tested wait, with `f` of the state and `V` the visit; a closed wait counts up to its cap | each point in its band |
| PO10 | `u = 3600 g / ln w`, `1 / u` | `1 / u <= sigma`, `u` in the band |
| PO12 | the L8 formula of the table above, with `u` of PO10 | `H` within 15% |
| PO13 | `T_e = 1 / g` against `10 k D_h` (L9: `g k D_h <= 0.1`); the largest speed of the formula is `floor(0.1 T_e / D_h)` | `T_e >= 10 k D_h`, and the slow-down at k under 1.1 |

In case 2, PO1 has 2 clauses (`po1_carrier`, `growth_clauses`). The formula clause is the sum of the table. The
simulated clause is on `g`: L1 proves `R` of the carriers, and no law proves that `g` stays constant while the factor
`k` of the generator grows. So `g` of the 2 halves of the window after hour 1 must agree within 20%, the limit of
each comparison of math and simulation. PO1 is `pass` when both clauses hold, and `fail` when 1 does not. The value
of the row gives both halves. So a game with no reset can be `ready`, as a game with a reset can. The rows PO10 and
PO12 compare the formula with the simulation, as for each model.

With an auto-buy (S24) in the model, PO6 has no formula (the auto-buy buys while the game is open), as for each
model. Each other row with no reset keeps rule 7.

## Domain of L4b and of the PO7 formula

These 2 domains came from the first design of `scripts/pw_design.py` (ticket 13). Worked example 2 has `p = 3` and `kappa u >= 1.5` at each tested wait, so it did not show them.

1. **PO7 formula: `p > 1` only.** The estimate `p kappa X^(1/p) / (e ln(1 + X))` is the maximum of rho of L4b over the wait only when `p > 1`. For `p <= 1`, rho falls as the wait rises, so the largest value is at the shortest wait. So for `p <= 1`, PO7 takes the largest rho of L4b over the tested waits inside the domain of L4b, with the limit `< 1`. The simulated value is the idle value at the same point, with the tolerance of that point of PO6.
2. **L4b: `kappa u >= 1`.** L4b assumes a reset at each visit with the gain `X_i = X (kappa u)^p`, `u = T / T_X`. The rule `G >= X L` permits that reset only when `X_i >= X`, so only when `kappa u >= 1` (closed: with the capped time). At a wait with `kappa u < 1` the formula is not valid. The row shows the text "formula not valid here (kappa u < 1); simulated value used" for that point, the JSON entry of the point has the source `simulation`, and an extra condition of PO6 (and of PO7 for `p <= 1`) checks the simulated value. A pass never comes from a formula outside its domain.

## No offline gain (play pattern `continuous`)

A model with no offline gain (`game.offline_rate` 0, or no offline rule in the `--systems` mode) has no closed-game part. PO6, PO7, PO8 and PO9 then check the open game only. Each of these rows has 1 entry in `parts`: the closed game, status `not applicable`, reason "no offline gain by design". The open-game part stays computed: a player who leaves the game open must stay below the active player.

## Rows of the play pattern `continuous`

A model with `game.play_pattern` `continuous` gets 6 more rows of the kind `pattern`. They come from changes 3, 6 and 8 of `docs/research/steam-continuous-pattern.md` section 5. The 13 obligations keep their ids. Each pattern row counts for the verdict like an obligation. The rows read the events of the active player `active_greedy` of the simulator, so their source is `simulation`. A model of `check_in`, or a model with no `play_pattern`, has no pattern row.

| Id | Statement | Inputs (`game.targets`) | Value | Limit |
| --- | --- | --- | --- | --- |
| PC1 | The active player reaches the hard end inside the length target | `hard_end`: `{"axis_step": n}` or `{"purchase": id}` (card S28); `length_hours` | the hours of active_greedy to the hard end | inside `length_hours`. No `hard_end` is `fail` (decision D10) |
| PC2 | The largest gap between 2 purchases is at most the gap target, and the first purchase comes inside the first-purchase target | `largest_purchase_gap_minutes` (default 2, CT3, `SG:181`, single source); `first_purchase_minutes` (default 2, CT2, `SG:180`) | the largest gap in minutes between t = 0, each purchase up to the hard end, and the hard end | `<=` the gap target. Extra condition: the first purchase `<=` its target |
| PC4 | The largest gap between 2 unlocks is at most the unlock gap target, over the whole length and in the first hour | `largest_unlock_gap_minutes` (default 10), `first_hour_unlock_gap_minutes` (default 5), and the optional `ending_unlock_gap_minutes` (the limit of the last gap before the hard end only): designer targets with no source (CT7, `SG:198`) | the largest gap in minutes between t = 0, each unlock up to the hard end, and the hard end; and the same in the first hour | `<=` each target, with the range of a target with no source. The row lists each gap outside the range in `needs` |
| PC5 | The active player buys each purchase line and reaches each milestone before the hard end | `hard_end`; the purchases and the milestones of the model | the count of elements that active_greedy reaches before the hard end | each element. No `hard_end` uses the whole run |
| PC6 | The purchases of the active player in each minute stay at or below the purchase rate target | `purchase_rate_max_per_minute` (default 30: a designer target with no source); the model key `automation` | the largest count of purchases in 1 minute up to the hard end, without the purchases that auto-buy makes | `<=` the target |
| PC3 | Each part of the game past the demo end adds new content | `demo_end`: `{"axis_step": n}` or `{"purchase": id}`; `hard_end`; `extension_part_hours` (default 1: no source, a default of the sheet) | for each part: its hours and the ids of its new content | each part has 1 or more new items. No `demo_end` is `not applicable` |

Rules of PC3:

1. The parts run from the demo end to the hard end. Each part is `extension_part_hours` long. A last part under half a part joins the part before it.
2. New content is a first purchase of a line or a first milestone (the unlock definition of PO11). An axis step alone is not new content: a longer wall of the same content is the failure case (`SG:185`).
3. An empty content slot is not new content. A slot is empty when it is a milestone with no effect and no grant, and no purchase, milestone or automation entry requires it (`empty_slots`). The design solver writes such a slot (`content_<k>`) in each part past the demo. A part with only an empty slot fails, and the fix asks for a named element. PC4 and CG do not count an empty slot either. PC5 still asks that the player reaches it.
4. The JSON row has `extension`: 1 entry for each part with `part`, `content` (the ids) and `empty_slots`. The report of the solver names each part and its content.

Rules of PC4:

1. An unlock is a new axis step, a first purchase of a line, or a milestone (the unlock definition of PO11). Unlocks at the same time count as 1 mark. An empty content slot (rule 3 of PC3) is not an unlock.
2. The first-hour part runs from t = 0 to 1 hour, or to the hard end when it comes first.
3. For each gap above its limit, the row gives the count of more unlocks that the gap needs: `ceil(gap / limit) - 1`. The JSON row has `needs`: 1 entry for each long gap with `start_minutes`, `end_minutes`, `gap_minutes` and `more`. The fix text gives the total. A gap that starts in the first hour and ends after it is listed 2 times: 1 time with the first-hour limit, 1 time with the other limit.
4. With `ending_unlock_gap_minutes`, the last gap (the gap that ends at the hard end) has that limit, and
   `largest_unlock_gap_minutes` tests each gap before it. With no such target, the last gap has the limit of the whole
   length.
5. A gap past its limit and inside the range of the target is in the key `in_range`, not in `needs`.
6. Ticket 19 added the row. A first test design had a gap of 22 minutes in the first hour and the sheet said "ready". With PC4 that design is not ready.

Rules of PC2: the JSON row has `where` (`start_minutes`, `end_minutes`) of the largest gap. The fix names these
minutes.

Rules of PC5:

1. An element is reached when active_greedy makes its first purchase, or reaches the milestone, at or before the
   hard end.
2. The JSON row has `unreached`: 1 entry for each element not reached with `id`, `kind`, `requires` (the texts of
   its requirements) and `change`. The change is an earlier lifetime total, more income of its currency, an earlier
   axis step, or a lower first cost.

Rules of PC6:

1. The simulator buys with the auto-buy (S24) of the model from `from_minutes` of each automation entry. Those
   purchases have the event key `auto` and do not count. The row counts the purchases by hand only.
2. When the row fails, `plan` gives the automation that brings the busiest minute down to 0.8 x the target (a margin
   of 20%, `RATE_MARGIN`: the game had 33 purchases against 30 with no margin). The plan takes the line with the most
   purchases in the minutes above that limit, from the first such minute. It repeats until no minute is above it
   or no line is left. Each entry has `line`, `from_minutes` and `purchases_in_busy_minutes`.
3. The JSON row also has `automation` (the model key) and `busiest_minute`. The value gives the busiest minute by
   hand, the busiest minute with the auto-buy purchases, the mean of the last hour, and the count of lines with
   auto-buy.
4. The design solver writes the plan into the model key `automation` when the selection has S24, and proves again,
   at most 3 times: the auto-buy moves the purchases, so a new plan can follow.

With an auto-buy in the model, PO6 has no formula (L4b assumes no purchase while idle) and the simulation decides
it. PO7 holds by dominance: the auto-buy buys for the idle and the active player alike, and the active player also
buys by hand. A clause checks that the largest simulated idle value is below 1.

PC1 tests the active player. With a reset layer, its value also gives the end of the axis for each reset player, with
`BAND OUT` when that end is outside the length band. A model with no reset layer has no reset player, so the value
names the active player only.

When the axis tests the income of a second currency `y` (BM L10), PO10 and PO12 use `kappa_y = R_y`: the sum of `r`
of the uncapped mult lines on `y` paid in the economy currency. `u = kappa_y alpha ldot / ln w`.

The time to the hard end of PC1 uses the same player as the simulated length of PO12. PO12 tests the end of the axis against the L8 formula. PC1 tests the declared hard end against the length target.

## Row PV: purchase value

A model with a `game.play_pattern` (`continuous` or `check_in`) gets the row `PV` of the kind `purchase`. It counts
for the verdict like an obligation. A model with no `play_pattern` (a model of version 1) gets `not applicable`.

Statement: at the time when the active player buys it, 1 level of each line raises the income, or the stat that the
line affects, by the floor or more. Input: `game.targets.purchase_value` = `{"floor": f, "source": text}`. The default
`f` = 0.10 is a designer target: the low end of the proven step `d` 1.1 to 2 of S02.levelled_factor (`O:188`,
`D:195`).

1. Formula part, for each level of the line (L1, D4): a `mult` line gains `d - 1`. An `add` or `pct` line gains
   `v` over the sum of its pool (the base income, or 1 for a `pct` group) at the full caps, less `v`. An uncapped
   `add` or `pct` line gains 0. The pool holds each additive line that adds to it, so the additive lines of 1 pool
   share its levels (the solver gives line i `1 + (v_i / f - P_i) / v_i` levels from the smallest value to the
   largest, with `P_i` the pool before it, design-solver.md). A line that requires the line before it at its levels
   (the PV order gate, a `level` requirement) is not in the pool of that smaller line: the smaller line is tested
   with the pool before the gated lines (`gated_after`). Rule: the
   floor applies to each level of an additive line, the late levels too. Reason: the last level is the one that the
   player buys with the least effect, and a level below the floor is a purchase that the player does not feel. A
   line that needs more levels needs a value that grows with each level: count steps (rule 7).
2. Simulation part, for each level that active_greedy buys: the income of the target currency after the purchase
   against the income before it. Each change of the income is an event of the simulator, so the income of the event
   before the purchase is the income before it.
3. A cap raise has no income gain. It is below the floor when a line that it raises is below the floor.
4. A line is below the floor when its formula gain or its smallest simulated gain is below `f`.
5. A cap raise is dead when it opens no level that the player buys. Its line has no cap, or active_greedy never buys
   a level of the line above its base cap.
6. The row is `fail` when 1 or more lines are below the range of the floor or 1 or more cap raises are dead. The
   fix names each line with its gains and each dead raise with its reason. The floor is a designer target with no
   source, so a line below `f` and inside the range of `f` is in range ("Target rows have a range").
7. **A line with count steps** (`count_steps`, card S12.count_steps). The tested gain is the gain of 1 count block:
   the `n` levels up to a step, with the step. Formula: `m - 1` or more (the `n` levels also add). Simulation: the
   smallest gain of a full block of `n` purchases of active_greedy. The gain of 1 level is in the key `level_gain`
   and in the value text. It is reported and not tested. Reason: the player buys such a line for the step, as in
   the games of the card (x2 at 25 owned).
8. The fix of a line below the floor names each way out: a larger `d` for a `mult` line; for an additive line count
   steps (S12), a larger value or fewer levels; and a lower floor. The floor is a designer target, so a line whose
   mechanic is that each level adds less (falling returns) keeps its mechanic with a lower floor.

JSON keys of the row, added to the keys of each row: `floor`, `floor_source`, `lines` (1 entry for each line:
`id`, `formula_gain`, `simulated_gain`, `levels_bought`, `raises`, `status` `pass`, `in range` or `below`, and for a
line with count steps `block` and `level_gain`), `below` (the ids),
`dead` (1 entry for each dead cap raise: `id`, `target`, `reason`), and `windows` (continuous only, 1 entry for each 10 minutes of active_greedy up to the hard end: `start_minutes`,
`purchases`, `unlocks`, `gain_min`, `gain_max`). The design report shows `windows` as the table "What the player
buys".

**Cap reduced.** Each entry of the model key `caps_reduced` (`id`, `game_cap`, `cap`, `reason`) gives a row
`cap reduced: <id>` of the kind `warning` with the status `warning`. A warning does not change the verdict.

## Row CL: core line life

A model with a play pattern and a simulation gets the row `CL` of the kind `purchase`. It counts for the verdict like
PV.

Statement: each generator line (a purchase line of the system S01) lives for the core of the game. The active player
makes its last purchase, or the line reaches its cap, at half of the length to the hard end or later.

1. The length is the time when active_greedy reaches the hard end, else the end of its run.
2. A line fails when its last purchase is before `0.5 x` the length (`LINE_LIFE_SHARE`). A line at its cap has its
   last purchase at the cap.
3. The limit 0.5 is a designer rule with no source. It is exact: the row has no range.
4. The fix: no cap and count steps (S12), or a higher cap, or a lower cost growth.
5. A model with no line of the system S01 gets `not applicable`.
6. JSON key `lines`: 1 entry for each generator line with `id`, `level`, `last_minutes`, `capped` and `share`.

Reason: the first forward design with no reset had a generator with a cap of a few levels. The line was complete at
minute 1.2 of 150, and no row said so. The generator is the fantasy of such a game.

## Row CK: character kept

A model with the key `character` (the design solver writes it in the rebalance mode) gets the row `CK` of the kind
`purchase`. It counts for the verdict like PV.

Statement: the rebalance keeps the character of the content. Each line, reward, side system and grant of the content
file is 1 element. S03 (the stages) and S04 (the reset) do not count: the solver sets their values for the timing.

1. Each field of an element is `kept`, `moved` or `changed`. The fields: each effect value, the cap, the cost ratio
   (a line), the values of a side system or a grant.
2. `kept`: the model value is within 25% of the game value. A step (a mult, a pct as `1 + v`, an exp cost ratio)
   compares by its log size, so x1.25 against x1.015 is changed. An add value compares as its share of the base
   income of its stat.
3. `moved`: the solver moved the value into the proven range of its system (the range is in the note).
4. `changed`: each other difference. The note names the obligation of the solver row that changed it.
5. The element takes its worst field state. The share line gives kept, moved, changed and the total.
6. The row is `fail` when more than half of the elements are changed, with the reason "the rebalance does not keep
   the game". The fix names each changed element.
7. Structure (`references/design-solver.md`, "Structure first"): each rule change (the stage after a reset, the test
   of the stage, the rounding of the reset gain), a count of resets above `max_resets` of the game, a cap state change
   of a line (a cap that the game does not have, or a cap that the model removes) and a milestone that a cap moves
   onto the level of another counts as 1 changed element. The value shows them in their own line: `structure: n
   changes (name: game ..., model ..., accepted or not accepted)`. The JSON row has `structure`. A structure change
   that the designer did not accept (`--accept-change`) makes the row `fail`, with the reason "the model changes the
   structure of the game, and the designer did not accept it".

## Row WS: wall steps

A model whose axis has a wall table (`requires.table`, 2 walls or more; the timing fit writes it) gets the row `WS` of
the kind `purchase`. It counts for the verdict like PV.

Statement: each step of the wall table after the first step is a wall. The ratio of each wall to the wall before it
is inside the g range of S03.exponential (`assets/systems.json`). The value gives the count of later steps, the
smallest, the median and the largest later step, and the walls whose step is outside the range. A step outside fails
the row: a step of x1.0 or x1.04 is not a wall, and a step of x3990 is a jump. The JSON row has `first_step`,
`smallest`, `median`, `largest` and `outside`. A model with a wall formula and no table has no row: the range row of
g checks it.

The first step (wall 1 to wall 2) is not tested. The value prints it with the mark "first step, not tested". Reason:
the first step is the start of the game, and the g range of the variant describes the later growth. A table of 6, 24,
44, 65, 94, 110 has a first step of x4 and passes, because each later step is inside the range.

### Why PO13 has 2 largest speeds

The formula `T_X >= 10 k D_h` counts about 1 human decision for each run: the run must be 10 decision delays long. The active player of the simulation decides at each purchase. A test rebalance has a median of 232 purchases in a run of 417 s, so the formula gives x24 and the simulation gives x2. The smaller value decides: the extra condition "slow-down at k under 1.1" fails when k is above the simulated speed, and the fix names the smaller value. The design solver names PO13 last in a conflict, because it is a speed of the play test and a larger s does not fix it.

## Row CG: content gaps

A model with the play pattern `continuous` and a simulation gets the row `CG` of the kind `content`. The kind is in
the ready kinds, so the row decides the result. Its status is `pass`, `pass (in range)` or `fail`.

Statement: the largest gap with no new content element of the active player is inside the FM-1 band of the fun sheet,
in the first hour and over the whole length.

1. A content element is a first purchase of a line, a first milestone or the first reset. An axis step is not a
   content element, and an empty content slot is not one (rule 3 of PC3). This is the definition of FM-1
   (`references/fun-analysis.md`).
2. The band is the FM-1 band of `game.targets.fun` (keys `max` and `max_h1`), else the default band of the fun sheet
   (20 minutes, and 10 minutes in the first hour). The band is a target with no source, so the row has the range of
   such a target ("Target rows have a range"), or `game.targets.tolerances.content_gap`.
3. The gaps run from 0 to the end of the run of PC4. A first-hour gap ends at minute 60.
4. Each gap outside the range needs `ceil(gap / band) - 1` more elements. A gap that starts in a long first-hour gap
   counts from minute 60 only, so that no part of it counts 2 times. The keys `needs` (start, end, gap and count of
   each such gap) and `missing` (the sum) give the result. A gap inside the range is in `in_range`.
5. The fix names the count of more elements and the minutes of each long gap.

A skeleton of the design solver (1 generator, the multiplier groups and the stages) has no content after the first
minutes, so CG fails for it. That is the correct result: the skeleton is the balance of a game, not a game. The
designer adds named content elements, or relaxes the band. PC4 does not change: it counts axis steps too. The report
of the design solver gives CG in the section "Gaps between content elements".

## The `--systems` mode

The map gives these inputs:

| Input | Source in the map |
| --- | --- |
| Economy currency | the most common `currency` of `lines`, other than the reset currency |
| R | `--R`, else the sum of `r` of the lines paid in the economy currency whose `stat` is `reset.multiplies` (an upper bound: each line open at once). R after the caps: the uncapped lines only |
| p, q, c | `reset.p` (gain family `power`), `reset.q`, `reset.divisor` |
| f_closed, closed cap | `offline.rate`, `offline.cap_hours`. With no offline rule: f_closed = 0, and the closed-game parts are not applicable |
| f_open | 1 when the active action S05 is not found: the game has no income that only active play gets (decision D3: an open game has no separate rule) |
| Presence tools | none when the pattern is `check_in`, no presence gate is found, and S05 is not found. Else not computed |

The map holds no A, no sigma, no axis count, and no targets. So PO5 to PO7 and PO10 to PO13 are `not computed`, unless the loop has `lambda > 1`: then PO4, PO5 and PO13 fail. PO11 and PO12 are always `not computed`.

## Systems that the laws do not cover

The sheet adds 1 `simulation only` row for each of these cases (section 7 of the balance math):

1. Limit 2: a currency other than the economy currency, the spine currency and the reset currency pays for a `mult` line on the economy or the spine (for example a garden of a side currency).
2. Limit 4: a line whose cost formula is not `exp`.
3. Limit 9: a ladder step with `attempt_seconds`, `cooldown_seconds` or `needs_visit`.
4. With `--systems`: each found system whose variant has `r` "not covered by L1" in `assets/systems.json`.

## Parameters outside the proven range

The model has no link to a catalog variant, so the script maps each parameter to the variants that describe it:

| Model parameter | Variant | Obligations that use it |
| --- | --- | --- |
| `cost_growth` and `mult` value of a `mult` line on the economy | S02.levelled_factor (b, d) | PO1 to PO5 |
| `cost_growth` and `mult` value of another `mult` line | S02.levelled_factor (b, d) | none |
| `cost_growth` and `pct` value of a `pct` line | S02.add_in_group (b, v) | none |
| `cost_growth` of an `add` line | S01.geometric, S01.steep or S01.ladder (b, inside 1 of the 3 ranges) | none |
| `cost_growth` of a line paid in the reset currency | S15.geometric (b) | PO2, PO4, PO5 |
| `prestige.exponent` | S04.power (p) | PO2 to PO7 |
| the axis `growth` | S03.exponential (g) | PO10, PO12 |
| f_closed, the closed cap in hours | S06.penalty_cap (f_closed, T_cap_h) | PO6, PO8 (f_closed); PO6 (T_cap_h) |

With `--systems`, the rows come from `out_of_range` of the map, with the same table of obligations.

The sheet gives no range row to a single purchase (`max_level` 1: its cost ratio has no effect) and to a line paid in
the economy currency whose effect is only on another currency (it feeds a side system and adds nothing to R).

## The gain of a reset by stage

A prestige with `formula` `exp` and `basis_ladder` pays `k g^n` at stage n. When stage n needs `base w^n` of the
economy, `n = ln(E / base) / ln w`, so the gain is `k (E / base)^p` with `p = ln g / ln w` and `c = base k^(-1/p)`
(`ladder_gain_exponent`). The sheet uses this p and c in L6 and in each row that needs p.

## Fixes inside the proven ranges

A fix text never names a value outside the proven range of its parameter. When the fix of PO3 (R after the caps),
PO10, lambda or a gap row needs a value outside the range, the fix names another change: for example, lengthen the
run or relax the target (`in_range`, `fix_r_end`, `fix_lambda`, `fix_gap`). The w of the PO10 fix has a guard: an
exponent too large to compute counts as outside the range.

## A null offline rate

With `--systems`, an offline rule with `found` `unsure` and `rate` null is not read: f_closed is `not computed`
with the name `f_closed`, and the sheet does not stop.

## JSON

`--json FILE` writes 1 object:

| Key | Content |
| --- | --- |
| `schema_version` | 1 |
| `mode` | `model` or `systems` |
| `source` | the input path |
| `result` | `ready`, `simulated` or `not ready` (section "The result") |
| `result_text` | the meaning of the result |
| `in_range_rows` | the id of each row with the status `pass (in range)` |
| `ready` | true only when each row that decides the verdict is `pass`, `pass (in range)` or `not applicable`, no core row fails, and (with `--strict-ranges`) no range row names an obligation |
| `all_in_proven_ranges` | true when the sheet has no `outside proven range` row |
| `strict_ranges` | true with `--strict-ranges` |
| `inputs` | the law values: `econ`, `R_max`, `R_end`, `R_source`, `p`, `q`, `c`, `A`, `alpha`, `s`, `s_end`, `lam`, `X_e`, `X_p`, `Dg`, `sigma` (s), `T_X` (s), `f_open`, `f_closed`, `kappa`, `kappa_closed`, `V` (s), `open_cap_h`, `closed_cap_h`, `absence_h`, `I0`, `a1`, `w`, `N`, `k`, `H` (h), `ldot`, `u_model` (unlocks for each hour), `econ_lines`, `F_ratio`, `F_held`, `F_traj` and `A_traj` (only when F_ratio is not 1), and `measured` (model mode: `policy_gap`, `length_hours`, `unlocks_first_hour`, `slowdown`, `T_sim`, `lambda_sim`, `median_gap`, `largest_gap` (min), `early_unlocks`, `P1h`, `phases`, `runs_early`, `idle`) |
| `rows` | 1 object for each row: `id`, `kind` (`core`, `obligation`, `pattern`, `purchase`, `content`, `warning`, `simulation only`, `range`, `simulated`), `statement`, `law`, `value`, `limit`, `status`, `source` (`formula`, `simulation`, `both`, or empty), `formula` and `simulation` (`value`, `num`, `ok`, or null), `difference` (the relative difference, or null), `fix`, `reason`, `clauses` (the extra conditions: `text`, `ok`), `parts` (the closed-game part when the model has no offline gain: `part`, `status`, `reason`), `points` (PO6: 1 entry for each tested point: `state`, `hours`, `formula`, `simulation`, `source`, `band`, `note`), `finding` (only when math and simulation differ), and `uses` (range rows), `needs` and `missing` (row CG), `in_range` (the text of each miss of a target row in its range), and `lines` (rows PV and CL) |
| `findings` | 1 text for each row where math and simulation differ by more than the tolerance |
| `speed` | with `--speed`: `human_decision_seconds`, `rule`, `T_X`, `largest_speed_formula`, `largest_speed_simulation`, `simulation_rows`. Else null |

## Results of the test models

`tests/test_proof.py` checks the result of each test model (`tests/models/`). The references give no result of a named
game.
