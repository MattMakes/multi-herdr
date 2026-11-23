# Balance math for incremental games

Ticket U2 (T-2). This document gives definitions, 11 laws, a design procedure and 2 worked examples. With them, a skill can prove the balance of a set of systems before the game exists. A second file, `docs/skill-v2/systems-catalog.md`, lists the systems. This document gives the math that balances them.

## Summary

1. Each law has assumptions, a derivation, 1 result formula and a design rule. The script `scripts/verify_math.py` checks each law except L10 with a greedy-buyer simulation. All 10 checks pass. The script runs in about 2 seconds.
2. One number, `R`, sets the growth class of a run. `R` is the sum, over groups, of the largest line exponent `r = ln(d) / ln(b)` in each group (L1, L2).
3. Progress is linear in time only when `ln(power)` is linear in time. A run with `R < 1` gives logarithmic progress. A reset loop with `lambda = p q / (1 - R) = 1` turns this into linear progress (L3, L6).
4. Idle time converts to progress through a logarithm, and active play converts through compounding. So idle gain grows with the wait but its share of active gain falls (L4). Rate tools change the scale of the idle curve. Caps, fights and visit-only resets change its shape (L5).
5. Moss Works fails 8 proof obligations. Its reset loop has `lambda >= 1` (measured `lambda >= 4.6`), so its runs get shorter without a limit (L6, worked example 1).
6. The corrected game of worked example 2 meets all 5 ticket targets for the 5 player types. Its idle value at 8 hours is 33% of 8 active hours. Its patient player is 1.84 times faster than its early-reset player.

## Conventions

- `S/` means the folder of version 1 of this skill. `N/` means the research notes on other games. The 2 folders are not in the skill; `references/other-games-math.md` holds the formulas from `N/`.
- Formulas are plain text. `ln` is the natural logarithm. `^` is a power.
- **Proved** means that the law has a derivation and its numeric check passes. **Estimate** marks a value that is computed from an approximation and is not checked. **Conjecture** marks a statement with no derivation and no check.
- A "check" line gives the predicted value, the simulated value and the tolerance. The script prints the same line.
- Run the checks: `python3 scripts/verify_math.py`.

## 1. Definitions

| Code | Term | Symbol and meaning |
| --- | --- | --- |
| D1 | Currency | `rho`: the amount that the player holds. |
| D2 | Income | `I = d(rho)/dt`: currency for each second. |
| D3 | Run spend, run earnings | `S`: currency spent in the current run. `E`: currency earned in the current run. With full reinvestment, `S` and `E` differ by the unspent cash only. |
| D4 | Purchase line | Level `k` costs `a * b^k`. `b > 1` is the cost ratio. A **multiplicative** line has value `d^k` (`d` is the value ratio). An **additive** line has value `v * k`. A line can have a level cap. |
| D5 | Line exponent | `r = ln(d) / ln(b)` for a multiplicative line. `r = 0` for an additive line. Source: `S/references/other-games-math.md:184-189`. |
| D6 | Group | A set of lines whose values add: group value `V_g = base_g + sum of terms`. Groups multiply: `power = base * product of V_g`. This is "add inside, multiply between" (`S/references/other-games-math.md:113-114`). |
| D7 | Power and the spine | `P`: the stat that the spine tests. In the examples, `P` is the income at the active rate. Spine stage `n` needs `P >= h_n`. With a constant wall ratio `w`, `h_n = h_1 * w^(n - 1)`. |
| D8 | Reset layer | A reset gives `G = (E / c)^p` of the reset currency and sets the run to zero. `p` is the gain exponent. `L` is the lifetime reset currency. The reward multiplies income by `M = (1 + L)^q`. `q` is the reward exponent. A reset rule `X` resets when `G >= X * L`. |
| D9 | Presence states | Active (the player decides each `D` seconds), open idle (the game runs, no decisions, income factor `f_open`), closed (the game is shut, income factor `f_closed`, banked time cap `T_cap`). |
| D10 | Bank | Currency that collects with no purchases. A bank with a cap stops at `B_max`. |
| D11 | Clock | Game time `t` (a speed multiplier scales it) or real time (a speed multiplier does not scale it). |
| D12 | Derived quantities | `alpha = 1 / (1 - R)`. `s = p * alpha`. `lambda = p * q * alpha`. `tau = S / I`: the stock time at a moment. `T_X`: the run length under rule `X`. `l = ln M`: reset progress. |

## 2. Laws

### L1. Spend to power

**Assumptions.** Costs are geometric. The buyer buys the line with the largest `d ln P / cost`. The total spend `S` is large against the first costs.

**Derivation.** For a multiplicative line, the spend `S_j` to reach level `k` is `a b^k * b / (b - 1)`. So `k = ln(S_j) / ln(b) + const`, and `ln(value) = k ln(d) = r ln(S_j) + const`. In a group, the sum of power terms `S_j^r_j` is dominated by the term with the largest `r`. So a group acts as 1 line with `r_g = max r_j`. Between groups, `ln P = sum r_g ln(S_g) + const`. The greedy buyer equalizes the marginal gains `r_g / S_g`. This is the Lagrange condition of the problem "maximize `sum r_g ln(S_g)` with `sum S_g = S`". So `S_g = S * r_g / R`, and `P = K S^R`. An additive line reaches level `k = floor(log_b(S (b - 1) / a + 1))` (bulk-buy formula, `S/references/other-games-math.md:53-54`). Its value grows as `ln S`, which is `r = 0`. Greedy buying is within `1 + O(1 / log M)` of the best plan (Demaine and others, `S/references/other-games-math.md:101-103`).

**Result.**

`P(S) = K * S^R * (ln S)^m`, with `R = sum over groups of max r_j`

`m` is the number of groups whose best line is additive. A capped line adds a constant factor and adds 0 to `R`.

**Design rule.** Compute `r` for each line before you simulate. Add the largest `r` of each group to get `R`. A line with a lower `r` than the best line of its group adds nothing in the long run.

**Check (proved).** 2 groups, `r` = 0.45 and max(0.38, 0.24): `R` predicted 0.830, simulated 0.827 (tolerance 3%). Additive line at `S = 1e9`: level predicted 118, simulated 118 (exact).

### L2. Growth class

**Assumptions.** L1 holds. The player reinvests all income at once. No reset.

**Derivation.** With full reinvestment, `dS/dt = I = K S^R`. Separate the variables.

- `R < 1`: `S = ((1 - R) K t)^(1 / (1 - R))`, so `I` grows as `t^(R / (1 - R))`.
- `R = 1`: `S = S_0 e^(K t)`. For 1 line with `b = d`, each level takes `a / I_0` seconds, so the rate is `g = I_0 ln(d) / a` exactly.
- `R > 1`: `S^(1 - R)` reaches 0 at `t* = S_0^(1 - R) / ((R - 1) K)`. For 1 line, the gaps between purchases are `(a / I_0) (b / d)^k`. They form a geometric series with the sum `t* = (a / I_0) * d / (d - b)`.

This is the closed form (3-3) of the source with `x = R`, `y = 0` (`N/d_exponential_others_general.md:46`).

**Result.**

`I(t) ~ t^(R / (1 - R))` for `R < 1`; `I(t) ~ e^(g t)` for `R = 1`; finite-time growth at `t*` for `R > 1`

**Design rule.** Keep `R < 1` in a run that has a reset layer. `R = 1` is a knife edge: 1 uncapped additive line (`m = 1`) on top of `R = 1` makes `ln S` grow exponentially. Use `R > 1` only for a short phase that a cap ends.

**Check (proved).** `R = 0.5`: exponent predicted 1.000, simulated 0.995 (5%). `R = 1`: rate predicted 0.00953/s, simulated 0.00953/s (2%). `R = 1.91` (1 line, `b = 1.1`, `d = 1.2`): blow-up time predicted 120.0 s, simulated 120.2 s at 300 levels (1%).

### L3. Spine speed

**Assumptions.** L2 holds. Stage `n` needs `P >= h_1 w^(n - 1)`. The spine power is the income, or a set of lines with exponent `R_s`.

**Derivation.** The highest stage at time `t` is `n(t) = 1 + ln(P(t) / h_1) / ln(w)`. So stages are linear in time only when `ln P` is linear in time.

- `R < 1`: `ln P = (R / (1 - R)) ln t + const`. Each doubling of time gives `(R / (1 - R)) ln(2) / ln(w)` stages. Stages for each hour fall as `1 / t`.
- `R = 1`: stages for each hour are `g / ln(w)`, a constant. A reset layer cannot sit on this economy (L6: `lambda` has no limit).
- A reset loop with `lambda = 1` (L6): each run of length `T_X` multiplies `M` by `(1 + X)^q`. A run with multiplier `M` is the `M = 1` run on a faster clock: `P_M(t) = M * P_1(M t)`. With `P_1(x) ~ x^(alpha - 1)`, the power at the end of each run grows as `M^alpha`.

**Result.**

`stages per hour = 3600 * alpha * q * ln(1 + X) / (T_X * ln(w))` for a reset loop with `lambda = 1`

**Design rule.** "Normalize the logarithmic curve to linear gains" means: make `ln P` linear in time. A designer has 4 tools over the life of the game:

1. A reset loop with `lambda = 1`. It gives a constant speed that the designer sets with `X`, `T_X` and `w`.
2. New groups at planned times. Each new group adds its `r` to `R` and lifts the slope again.
3. Wall changes. Set `h_n = P_active(t_n)` with target times `t_n` that are linear in `n`.
4. A second reset layer (L7, conjecture).

**Check (proved).** `R = 0.5`, `w = 2`: stages for each doubling predicted 1.000, simulated 1.000 (0.1 stage). Reset loop with `lambda = 1`: stages for each hour predicted 9.67, simulated 9.34 (5%).

### L4. Return on idle time

**Assumptions.** L1 and L2 hold, with `R < 1` and fine lines. At the start the player has spend `S_0` and income `I_0`, so `tau = S_0 / I_0`. The idle income factor is `f <= 1`. There is no reset in the wait.

**Derivation.** Idle: the bank is `f I_0 T`. 1 visit spends it, so `S` goes from `S_0` to `S_0 + f I_0 T`. By L1, `ln P` rises by `R ln(1 + f T / tau)`. Active: `dS/dt = I_0 (S / S_0)^R`. Integrate from `S_0`: `S_1 / S_0 = (1 + (1 - R) T / tau)^(1 / (1 - R))`. So `ln P` rises by `(R / (1 - R)) ln(1 + (1 - R) T / tau)`.

**Result.**

`rho(T) = idle gain / active gain = (1 - R) * ln(1 + f T / tau) / ln(1 + (1 - R) T / tau)`

Properties of the result:

1. `rho -> f` for `T << tau`, and `rho -> (1 - R)` for `T >> tau`.
2. The idle gain is positive and rises with `T` for each `T` when no cap applies.
3. The idle gain is below the active gain for each `T` when `f <= 1` and `R > 0`. Proof: `u -> ln(1 + u x) / u` falls as `u` rises, and `1 - R < 1 <= 1 / f`.
4. For an economy with `R = 1` and rate `g`: `rho(T) = ln(1 + f g T) / (g T)`. This goes to 0 as `T` rises. When progress is linear in time, `rho` is also the equivalent active time divided by `T`.

**L4b. Idle in a reset loop with `lambda = 1`.** An idle player visits each `T`. A visit spends the bank, resets if `G >= X L`, then plays `V` seconds. The next wait starts with the power after `V` seconds of a run. Then the bank is `E_i = f M P_1(M V) T`, and a full active run earns `E_X = A (M T_X)^alpha`. So `E_i / E_X = kappa * T / T_X`, with `kappa = f * alpha * (V / T_X)^(alpha - 1)`. The reset gain is `X_i = X (E_i / E_X)^p`. Each visit adds `q ln(1 + X_i)` to `l`, and active play adds `q ln(1 + X)` for each `T_X`. With `u = T / T_X`:

`rho(T) = ln(1 + X (kappa u)^p) / (u * ln(1 + X))`

The largest value over `u` is about `p * kappa * X^(1/p) / (e * ln(1 + X))` (estimate: it drops the `+1`).

Domain (added 2026-11-14 by ticket 13). The formula holds only for `kappa * u >= 1`, because the rule `G >= X L` permits the assumed reset at each visit only when `X_i >= X`. The estimate of the largest value holds only for `p > 1`, because for `p <= 1` rho falls as `u` rises and its largest value is at the shortest wait inside the domain.

**Design rule.** The conditions for "idle is a good gain, but never as good as active" are:

1. `f <= 1` in each idle state.
2. No cap on open idle shorter than the longest absence that must still pay. The operator asks for 1 day up to many days.
3. For a reset loop: `p * kappa * X^(1/p) < e * ln(1 + X)`, so `rho < 1` for each `T`.
4. `rho(8 h)` sets the active growth rate. A faster active economy gives idle a smaller share. To raise `rho`, lower `R` (idle loses less compounding) or raise `p` (a long run pays more reset currency). L6 limits `p`.

**Check (proved).** `R = 0.5` at 1 hour of play, `T` from 1 hour to 30 days: worst error of the idle and active gains 3.6% (10%). The idle gain is positive, rises and stays below the active gain. Simulated `rho`: 0.81, 0.64, 0.61, 0.57, 0.55, which goes to `1 - R = 0.5`. L4b is checked by the 6 idle rows of worked example 2: the largest error is 0.019.

### L5. Presence tools

**Assumptions.** L4 holds. 1 tool changes at a time.

**Derivation.** Each tool changes 1 term of the L4 formula.

| Tool | Effect on the idle gain | Class |
| --- | --- | --- |
| Idle income rate `f < 1` | `T -> f T`. The log curve moves down by about `R ln(f)`. The slope in `ln T` stays `R` | Scale |
| Active income term `a > 1` (only active play gets it) | The same math as `f = 1 / a` | Scale |
| Hard cap `T_cap` on banked time | `T -> min(T, T_cap)`. The gain is flat after `T_cap` | Shape |
| Bank that fills and stops at `B_max` | The same math as a cap with `T_cap = B_max / (f I_0)` | Shape |
| Spine step that needs a visit (a fight with cooldown `C`, visit `V`) | Stages for each visit `<= ceil(V / C)`. Power still grows, but stages wait for the follow-up play | Shape |
| Reset that needs a visit | The idle gain in `l` grows as `p q ln T`. If the reset (or buying) runs while the player is away, the gain becomes linear in `T` | Shape |

**Result.**

`idle gain(T) = R ln(1 + f * min(T, T_cap) / tau)`, with the stage gain for each visit limited to `ceil(V / C)`

**Design rule.** Use scale tools to set `rho` at 1 to 8 hours. Use shape tools only past the longest absence that must still pay. A shape tool below 1 day breaks the operator rule. An offline or open-idle reset or auto-buyer turns idle into active play at rate `f`. Then `rho = f` for each `T`, and idle progress is linear.

**Check (proved).** Base state of L4, `T` = 1 day and 4 days. Worst gain error against the formulas: 0.8% (10%). Each tool falls in its predicted class by the slope in `ln T`. Visit reset: slope / `(p q)` = 0.99. Away reset: gain(4 d) / gain(1 d) / 4 = 1.00 (linear).

### L6. Reset loop

**Assumptions.** A run with multiplier `M` is the `M = 1` run on a faster clock: `E_M(t) = E_1(M t)`. This is exact when `M` multiplies all income and costs do not change. `E_1(x) = A x^alpha` for large `x` (L2, `alpha = 1 / (1 - R)`). `G = (E / c)^p`, `M = (1 + L)^q`, and the rule resets at `G = X L`.

**Derivation.** At the reset, `(A (M T)^alpha / c)^p = X L`. With `M ~ L^q`, the run length is

`T_n = (c / A)^(1/alpha) * X^(1/s) * L_n^(1/s - q)`.

Each reset multiplies `L` by `1 + X`. So `T_(n+1) / T_n = (1 + X)^((1 - lambda) / s)`, with `lambda = q s = p q alpha`.

- `lambda > 1`: runs get shorter by a constant factor. Their sum is finite, so the player makes infinite resets in finite time. The loop is not stable.
- `lambda = 1`: `T_X = X^(1/s) * (c / A)^(1/alpha)` for each run. `l` rises by `q ln(1 + X)` for each run. The loop is stable, and the designer sets its factor with `X`, `c` and `q`.
- `lambda < 1`: runs get longer. Progress slows.

Best reset for `lambda = 1`: the speed is `v(X) = ln(1 + X) / T_X ~ ln(1 + X) / X^(1/s)`. Set `dv/dX = 0`: `s X / (1 + X) = ln(1 + X)`. A solution exists only for `s > 1`. For `s <= 1`, the earliest reset is best, and the patient player is slower.

| `s` | 1.2 | 1.5 | 2 | 3 | 3.75 | 5 |
| --- | --- | --- | --- | --- | --- | --- |
| Best `X*` | 0.46 | 1.40 | 3.92 | 15.8 | 37.6 | 142 |
| Income factor for each reset, `(1 + X*)^(1/s)` | 1.37 | 1.79 | 2.22 | 2.56 | 2.65 | 2.70 |

The source gives the best mean reset multiplier `e^(z (y + 1) / (1 - x))` for income `K rho^x t^y rho_pub^z` (`S/references/other-games-math.md:92-95`, `N/d_exponential_others_general.md:46`). In that model the new currency replaces the old one. Map it with `x = R`, `y = 0`, `z = p q`. Then the best income factor is `e^lambda`, which is `e` for a stable loop (estimate: the script checks the source formula in its own model, not this mapping). The additive form `L + G` of this document gives a factor below `e` that rises to `e` as `s` rises (table above).

Gap between 2 reset rules:

`v(X_p) / v(X_e) = (ln(1 + X_p) / ln(1 + X_e)) * (X_e / X_p)^(1/s)`

**Result.**

`T_(n+1) / T_n = (1 + X)^((1 - lambda) / s)`, stable if and only if `lambda = p q / (1 - R) <= 1`

**`p = 0.5`, `q = 1`, no cap.** `lambda = 0.5 / (1 - R)`. This is above 1 for each `R > 0.5`. Moss Works has `R` up to 1.78 inside a run (worked example 1). So the loop is not stable. The patient rule of Moss Works (P1) resets at `G >= 10 L`. The law gives the gains `>= 10 * 51 = 510` and `>= 10 * (51 + 511) = 5,620`. The measured gains are 511 and 6,207. The measured runs took 7 minutes and then less than 1 minute: the collapse that `lambda > 1` predicts.

**Design rule.**

1. Set `lambda = 1` for linear progress: `q = (1 - R) / p`.
2. Set `s = p / (1 - R) > 1` so that a best reset exists.
3. Choose `s` so that the gap between your early and patient rules is below the target. For `X_e = 1`, `X_p = 10` and a gap of at most 2, `s <= 4.2`.
4. Set `c = A * T_X^alpha * X^(-alpha/s)` to get the run length `T_X`. When lines of the reset currency carry a part of `q`, set `c = A * (K T_X)^alpha * X^(-alpha/s)` (the constant `K` below).

**Check (proved).** `lambda = 1.25` (`R = 0.6`, `p = 0.5`, `q = 1`, `X = 10`): run-length ratio predicted 0.619, simulated 0.622 (5%). Moss Works gains: predicted `>= 510` and `>= 5,620`, measured 511 and 6,207 (15%). `lambda = 1`: run length predicted 372 s, simulated 380 s (5%). Patient (`X = 10`) against early (`X = 1`) speed: predicted 1.094, simulated 1.105. Best (`X = 3.92`) against early: predicted 1.161, simulated 1.169 (5%). Closed form of the source, 2 cases, by RK4 integration: worst error 0.0% (5%).

**Lines of the reset currency (the constant `K` of D8).** A line paid in the reset currency with first cost `a1`, cost ratio `b` and step `d` carries `r = ln d / ln b` of `q`. Its `k` levels cost `a1 (b^k - 1) / (b - 1)`. So a spend of `L` buys the factor `d^k = (1 + (b - 1) L / a1)^r`, about `K L^r` with `K = ((b - 1) / a1)^r`, not `L^r`. For several uncapped lines, `K` is the product of their constants. The run total holds `K^alpha`, so the run length is `T_X / K`:

`T_X = X^(1/s) * (c / A)^(1/alpha) / K`

**Factors at a count of resets.** A content factor that opens at a count of resets is a factor that a reset adds,
as the reward. Factors F over n resets add `ln F / n` to `ln M` for each reset, so they carry
`q_held = ln F / (n ln(1 + X))` of q, and lambda = 1 needs the reward exponent `q - q_lines - q_held` (trial Kiln n
Kin: lambda = 0.5 x 0.5 x 4 = 1 by the reward, and the towns and pieces put each later run at the run floor).

Domain: `(b - 1) L / a1` well above 1, so after the first resets, and runs that end by the reset rule. A run that ends at the run floor (`min_run_seconds`) is outside the domain of L6: `G >= X L` comes before the floor, so `c` sets the gain of each run, not its length. The proof sheet then uses the simulated value in the rows that use `T_X`, and the solver keeps `c = A T_X^alpha X^(-alpha/s)` (trial Kiln n Kin: runs of 245 s at a floor of 240 s; with `K` in `c` the game ended at 3.3 h to 4.3 h, with no `K` at 5.35 h, target 6 h). The first runs are longer, and the median of the later half of the runs is the measured value. `K = 1` when `a1 = b - 1`.

**Check (ticket 35, Tide Forge, `a1 = 1`, `b = 1.35`, `r = 0.3176`, `K = 0.716`).** Without `K`, the formula gave 431 s and early_reset ran 630 s (ratio 1.46, and `1 / K` = 1.40). The second income was not the cause: with its line removed, the ratio stayed 1.49, and the wall weight 1 in place of 3 gave the same 630 s. With the line of the reset currency replaced by the full `q` in the reward, the ratio was 1.01. With `a1 = b - 1 = 0.35` (`K = 1`), the ratio was 1.06. With `K` in the formula and in `c`: formula 407 s, simulated 434 s (7%).

### L7. Composition

**Assumptions.** L1 to L6 hold. The designer adds 1 system to a set with exponent `R`.

| Added system | Change to `R` | Other changes | Status |
| --- | --- | --- | --- |
| New source: an additive line in an old group | 0 | Adds a log factor. In a stable loop, an uncapped additive line moves `lambda` slowly above 1. A trial run with an uncapped generator gave runs that fell from 351 s to 76 s in 40 resets. A cap at 40 levels held them at 191 s | Proved for `R`. The trial is an observation and the script does not check it |
| Multiplier in an old group, `r <= max r` | 0 | A short-term reward only | Proved |
| Multiplier in an old group, `r > max r` | `R - r_old + r` | The group changes its best line | Proved |
| Multiplier in a new group | `R + r` | `alpha` and `lambda` rise. Adjust `q` to keep `lambda = 1` | Proved |
| Level cap | `-r` after the cap | A constant factor stays | Proved |
| Softcap: group value to the power `sigma` | `r_g -> sigma * r_g` | 1 dial for a whole group | Proved |
| Second clock: income times `t^y` (a time-only source) | 0 | `I ~ t^((R + y) / (1 - R))`. The source has `y` in the closed form | Proved |
| Second reset layer | 0 inside a run | Apply L6 again with the layer-1 reset currency as the income and `lambda_1` in place of `R` | Conjecture |

**Result.**

`R_new = R - max(r in group g) + max(r in group g after the change)`

**Design rule.** Decide for each new system whether it is a new group (a large reward that changes `R`) or a line in an old group (a small reward). Then compute `R`, `alpha` and `lambda` again, and fix `q` for `lambda = 1`.

**Check (proved).** `R` predicted / simulated (0.02): new group 0.700/0.700, old group with a lower `r` 0.400/0.400, old group with a higher `r` 0.600/0.600, softcap 0.550/0.551, cap 0.400/0.400, additive line in an old group 0.400/0.399. Second clock `y = 0.5`: income exponent predicted 2.00, simulated 2.01 (5%).

### L8. Time scale

**Assumptions.** L3, L4b and L6 hold. The targets are: length `H` hours, `u_1` unlocks in the first hour, `u` unlocks for each later hour, session length `sigma`. An unlock is a new highest stage.

**Derivation.**

1. 1 run for each session: `T_X = sigma`.
2. By L6, `l` rises by `ldot = 3600 q ln(1 + X) / T_X` for each active hour.
3. By L3, the end-of-run power rises by `alpha * ldot` in `ln` for each hour. 1 unlock for each `1 / u` hours needs `w = e^(alpha * ldot / u)`.
4. The first hour is different. Each run starts from 0, and the first run has `tau` near 0. So `ln P` rises fast. Place the first `u_1` stages on the trajectory: `h_n = P_1(t_n)` at target times `t_n`. `P_1` is the L2 trajectory with the reset rule. It has no closed form with a capped additive line, so the script computes it. This is a computation of the model, not a tuning step.
5. The last stage: `N = u_1 + ceil((ln P(1 h) + alpha * ldot * (H - 1) - ln h_(u_1)) / ln(w))`.
6. Cost ratios come from the line exponents: `b = d^(1/r)`.
7. The reset divisor comes from L6: `c = A * T_X^alpha * X^(-alpha/s)`.

**Result.**

`w = exp(alpha * q * ln(1 + X) * 3600 / (T_X * u))` and `N = u_1 + ceil((ln P(1 h) - ln h_(u_1)) / ln(w) + u * (H - 1))`

**First 10 minutes.** The first cost must be at most 15 seconds of the base income: `a_1 <= 15 * I_0`. Place at least 3 unlocks in the first 5 minutes. Use an additive generator line first: its `r = 0` gives growth near `t ln t`, and a new source each minute is affordable. The first 10 minutes need no reset and no idle rule, because `tau` is small and `rho` is near `f`.

**Design rule.** Derive `w`, `N` and `c` from the targets. Do not tune them from a simulation.

**Check (proved).** Worked example 2: length predicted 20.0 h, simulated 20.33 h (15%). First-hour unlocks predicted 6, simulated 7 (1). Later unlocks: 5 for each hour predicted, median gap 13.2 minutes simulated (12 minutes predicted).

### L9. Speed test

**Assumptions.** A speed multiplier `k` scales the game clock. A human decides each `D_h` real seconds, which is `k * D_h` game seconds.

**Derivation.** Between decisions, income collects and is not spent. At the decision, the player spends all of it. With relative growth `g = I / S`, each decision gap `D` multiplies `S` by `1 + g D`. So the rate is `ln(1 + g D) / D` in place of `g`. This is L4 with `f = 1` and `T = D`. A reset sets `tau = S / I` to near 0, so `g` is large at the start of each run.

**Result.**

`slow-down(D) = g D / ln(1 + g D)`

**What a speed multiplier must scale.** Income, drips, cooldowns, run timers, and each open-idle rule. An open-idle rule must count game time, or x100 removes it.

**What it must not scale.** Time while the game is closed. That time is real time, and the speed does not apply. The human decision rate cannot scale.

**Property.** x`k` is the same game played faster when the progress against game time does not depend on the decision gap for gaps up to `k * D_h`. From the result, this needs `g * k * D_h <= 0.1` for the largest `g` that the player meets. In a reset game, this needs `T_X >= 10 * k * D_h` (estimate from worked example 2: `T_X / (k D_h)` = 4.5 gives a slow-down of 1.148, and 45 gives 1.021). Open idle at x`k` still follows the L4 curve, so it stays below active play when the obligations of L4 hold.

**Design rule.** Choose the play-test speed from `T_X`. A game with 15-minute runs supports x10 play tests with a 2-second human, not x100.

**Check (proved).** 1 line with `R = 1`, `d = 1.001`. Slow-down predicted / simulated (5%): `gD` = 0.1: 1.049/1.049; `gD` = 1: 1.442/1.443; `gD` = 3: 2.163/2.163.

### L10. Two incomes

**Assumptions.** The main income `c` buys its own lines (exponent `R_c`, L1) and the lines of a second income `y`
(exponent `R_y`, the sum of `r` of the uncapped mult lines on `y` that `c` pays for). The spine or the reset tests `y`,
not `c`. The player puts a constant share of the spend into the lines of `y`. A reset multiplies `c` by
`M_c = (1 + L)^q_c` and `y` by `M_y = (1 + L)^q_y`.

**Derivation.** By L2, the spend grows as `S ~ (M_c t)^alpha_c` with `alpha_c = 1 / (1 - R_c)`. The income of `y` is
`I_y = M_y K_y (phi S)^R_y`, so it grows as `t^(alpha_c R_y)`, and its run total `E_y` as `t^alpha_y`. Because
`I_y(t; M_c) = I_y(M_c t; 1)`, the run total is `E_y(t) = M_y E_y,1(M_c t) / M_c`. So `M_c` enters `E_y` with the
exponent `alpha_y - 1` and `M_y` with the exponent 1: `y` is not reinvested. Put this into the reset rule of L6.

**Result.**

`alpha_y = 1 + alpha_c R_y`, `s = p alpha_y`, `lambda = p (q_y + q_c (alpha_y - 1))`

`T_(n+1) / T_n = (1 + X)^((1 - lambda) / s)` for a reset on the run total of `y`. For a reset on `c` with
`lambda = p q_c alpha_c = 1` (L6) and stages on the income of `y`, `ln I_y` at the end of each run rises by
`alpha_c R_y q_c ln(1 + X) + q_y ln(1 + X)`: the stages on `y` move `kappa_y = R_y` times as fast as the stages on
`c` (with `q_y = 0`).

**Design rule.** Keep the reset on the main income, so L6 holds as for 1 income. Give the second income at least 1
uncapped mult line paid in the main currency (`R_y > 0`). Set the wall ratio of the stages on `y` from
`w_y = e^(kappa_y alpha_c ldot / u)` (L8 with `kappa_y`). The rebalance mode keeps the game shape of each line of
`y`, so `R_y` is a result, and the timing fit sets each wall from the simulated income (design-solver.md, "The timing
fit"). When each line of `y` has a cap, `I_y` has a ceiling, and no stage comes after it (Kiln n Kin: `rep` stopped at
7.7 for each run, and the fit had 7 stages): the solver then drops the cap of the line with the largest `r`.

**Limits.** 1) An additive line on `y` alone gives `R_y = 0`: `I_y` grows as `ln S`, and no `w_y` in the range of S03
gives a constant unlock rate. 2) The law assumes a constant spend share. A greedy buyer that weights `y` against `c`
moves the share inside a run, and the simulation measures it. 3) A conversion of `y` back into `c` (a chain of more
than 2 steps or a loop) is not covered. 4) A cap on a line of `y` takes its `r` out of `R_y` (L7).

**Check (proved).** `tests/test_two_incomes.py`, an event simulation with `R_c = 0.5`, `R_y = 0.4`: exponent of `E_y`
predicted 1.800, simulated 1.800 (5%). Reset on `E_y` with `p = 0.5`, `q_c = q_y = 0.5` (`lambda = 0.45`): run-length
ratio predicted 1.527, simulated 1.54 (5%). `lambda = 1`: ratio 1.00 (5%). Reset on `c` with `lambda = 1`: rise of
`ln I_y` for each run predicted 0.555 (10%).

### L11. Count steps

**Assumptions.** A generator is an additive line with no cap: level `k` costs `a b^k` and adds `v`. It has count
steps (`count_steps`, card S12.count_steps): the value of the line takes the factor `m` at each `n` owned levels.

**Derivation.** Section 3a, "The generator as a carrier", steps 1 and 2.

**Result.**

1. The growth with the count: `R(k) = sum of r over the other carriers + r_eff + 1 / (k ln b)`, with
   `r_eff = ln m / (n ln b)`. Over a spend window, the log factor is the secant `ln(k_2 / k_1) / ln(S_2 / S_1)`.
2. The value of a count block: the income after the step against the income before the first level of the block is
   at least `(to / (from - 1)) m` and at least `m`. A purchase of another carrier in the block only adds. This is
   the tested gain of the row PV (`references/proof-sheet.md`, rule 7).

**Design rule.** Section 3a.

**Check (proved).** `verify_math.py`, greedy buyer with a fixed spend (`a` 10, `b` 1.15), spend `1e10` to `1e20`:
`n` 10, `m` 1.55: `R` predicted 0.348, simulated 0.348 (3%); `n` 25, `m` 2: predicted 0.233, simulated 0.232 (3%).
With a multiplier line `r = 0.3` in a second group, spend `1e12` to `1e24`, the log factor at the simulated levels:
predicted 0.643, simulated 0.635 (3%). Active greedy to level 150 with that line: 15 blocks, the smallest block gain
2.045 against `m` 1.55, and each block at or above `(to / (from - 1)) m` (1%). The prediction with `r_eff` alone
(no log factor) misses by more than 3%.

## 3. Design procedure

Inputs:

- Length `H` in active hours.
- Session length `sigma`.
- Unlocks `u_1` in the first hour and `u` for each later hour.
- Idle-to-active targets `rho` at 1 hour, 8 hours, 24 hours and 7 days, for open and closed idle.
- Reset rules: early `X_e`, patient `X_p`, and the largest allowed gap `Dg`.
- The selected systems from the catalog.

Steps:

1. List each line of each selected system with `a`, `b`, `d` or `v`, its group and its cap. Compute `r` for each line (L1).
2. Compute `R` for each phase of the run (L1, L7). A phase starts when a line unlocks and ends when a cap is hit.
3. Choose `R` so that `rho(8 h)` can reach its band. A low `R` makes idle lose little compounding (L4, L4b).
4. If there is a reset layer, set `p` from the gap target: `s = p / (1 - R)` with `(ln(1 + X_p) / ln(1 + X_e)) * (X_e / X_p)^(1/s) <= Dg`. Then set `q = (1 - R) / p` (L6).
5. Compute `A` from the L2 trajectory and set `c = A * T_X^alpha * X_e^(-alpha/s)` with `T_X = sigma` (L6, L8).
6. Choose the presence tool and `f_open` so that `kappa = f_open * alpha * (V / T_X)^(alpha - 1)` gives `rho(8 h)` in band and `rho_max < 1` (L4b, L5).
7. Choose the closed rule `f_closed` and `T_cap`. Compute `rho(24 h)` with `T -> min(T, T_cap)` (L5).
8. Compute `w`, the first stage powers and `N` (L8).
9. Choose the play-test speed with `T_X >= 10 * k * D_h` (L9).
10. Check each proof obligation below. Then run the simulation with the 5 player types to confirm.

### Proof obligations

| Code | Obligation | Law |
| --- | --- | --- |
| PO1 | `R < 1` in each run phase that has a reset layer. `R = 1` only with no uncapped additive line. 1 long run with a generator carrier: `R + 1 / (k_mid ln b) <= 1` (section 3a) | L2 |
| PO2 | `lambda = p q / (1 - R) <= 1`, and `lambda = 1` for linear progress | L6 |
| PO3 | `s = p / (1 - R) > 1` | L6 |
| PO4 | `(ln(1 + X_p) / ln(1 + X_e)) * (X_e / X_p)^(1/s) <= Dg` | L6 |
| PO5 | `T_X = X^(1/s) * (c / A)^(1/alpha)` is within 20% of `sigma` | L6, L8 |
| PO6 | `rho(T)` from L4b is in its band at 1 h, 8 h, 24 h and 7 d | L4 |
| PO7 | `p * kappa * X^(1/p) < e * ln(1 + X)`, so idle stays below active for each `T`. This form holds for `p > 1` only. For `p <= 1`, rho of L4b falls with `T`, so the check is rho `< 1` at the shortest tested wait with `kappa * u >= 1` | L4 |
| PO8 | `f_open <= 1` and `f_closed <= 1` | L4 |
| PO9 | No open-idle cap and no shape tool below the longest absence that must pay | L5 |
| PO10 | `1 / u <= sigma` after the first hour, and `w = e^(alpha * ldot / u)` | L3, L8 |
| PO11 | `a_1 <= 15 * I_0`, and at least 3 unlocks in the first 5 minutes | L8 |
| PO12 | `N` from L8 gives the length `H` within 15% | L8 |
| PO13 | `T_X >= 10 * k * D_h` for the play-test speed `k` | L9 |

## 3a. 1 long run with no reset

Ticket 40 (repair 3). A game of 1 long run has no reset layer (`S04.none`), no run on a limit (S29) and no skill
track (S22). The run goes from the start to the hard end (S28). This section states what the laws prove for such a
game, and what only the simulation checks. The solver path is in `references/design-solver.md`, "1 long run".

### What paces a short game with no reset

`references/short-games.md` (SG) has 3 games with no reset and no run on a limit. A fourth game states no reset.

| Game | What paces it | Source |
| --- | --- | --- |
| Digseum | New excavation sites in order: 12 sites in 2.5 hours, about 1 site each 12.5 minutes | SG:89-90, SG:94-95 |
| Digseum | A manual action (dig) raises the base income and the multiplier; a passive source (the museum) pays | SG:91-92 |
| Spaceplan | A finite scenario with an ending; no prestige | SG:139-140 |
| Spaceplan | The game open for a long time, with few active hours | SG:137-138 |
| Universal Paperclips | A manual action first (clips by hand), then automation (AutoClippers) and multiplier upgrades | SG:146-148 |
| Universal Paperclips | Eras that open in order and change the currencies; a definitive end | SG:146-149 |
| Magic Archery | Click income and upgrades, 1 hour; reset not stated | SG:107-109 |

So 1 long run takes its pacing from 5 tools: the walls of the progress axis, new lines and systems that open in
order, caps and their raisers, automation that replaces a manual action, and the hard end. The reset loop (L6) is
not one of them.

### The growth class: R = 1

With no reset, L2 gives the growth class of the whole game:

- `R < 1`: `ln P` grows as `ln t`. By L3, 1 wall ratio `w` gives stages for each hour that fall as `1 / t`. The
  game slows to a stop, and only walls that follow the trajectory (L3 tool 3) keep the unlock rate.
- `R = 1`: `I = g S`, so `I ~ e^(g t)` (L2). By L3, 1 wall ratio gives a constant unlock rate `u = 3600 g / ln w`
  for each hour. The design rule of L2 ("`R = 1` is a knife edge") concerns a reset layer (L6: `lambda` has no limit)
  and an uncapped additive line (`m = 1`). 1 long run has neither, so `R = 1` is the linear-progress choice.
- `R > 1`: growth ends in finite time (L2). Use it only for a short phase that a cap ends.

The solver of 1 long run sets `R = 1` at the middle of the length. Its carriers are the generator with its count
steps and the multiplier groups (the next section).

### The generator as a carrier

**Why.** A capped generator adds 0 to `R` (L1), and the first forward design had one: a cap of 9 levels, complete at
minute 1.2 of 150. The proven games do the opposite. The player buys hundreds of units of a generator line with a
cost ratio of 1.07 to 1.15, and a multiplier at an owned count (x2 at 25, 50, ...) gives the jumps
(`references/other-games-math.md`, card S12.count_steps).

**Assumptions.** L1 holds. The generator is an additive line with no cap: level `k` costs `a b^k` and adds `v`. It
has count steps: the value of the line takes the factor `m` at each `n` owned levels. Each other carrier is a
multiplicative line.

**Derivation.** The value of the generator at level `k` is `v k m^floor(k / n)`.

1. *The step part.* At each step, `m^(k / n) = b^(r_eff k)` with `r_eff = ln m / (n ln b)`. The spend on the line
   up to level `k` is `a b^k / (b - 1)`, so `b^k ~ S_gen` and the step part is `S_gen^(r_eff)`. This is a
   multiplicative line of L1 with the value ratio `m^(1 / n)` for each level. So the generator is a carrier with the
   exponent `r_eff`, and L1 gives it the share `r_eff / R` of the spend.
2. *The factor k.* `k = ln(S_gen (b - 1) / a) / ln b`: the log factor of L1. Its local exponent is
   `d ln k / d ln S_gen = 1 / (k ln b)`. It falls as the level rises: 0.29 at level 25 and 0.05 at level 150 for
   `b = 1.15`.
3. *The growth class.* The local exponent of the game at the generator level `k` is
   `R(k) = R_c + 1 / (k ln b)`, with `R_c` the sum of `r` over the carriers. With `R_c = 1`, `R(k) > 1` at each
   level: `dS/dt = K S ln S`, so `ln S` grows as `e^(K t)`. This is the knife edge of the design rule of L2.
4. *The choice.* Set `R_c = 1 - 1 / (k_mid ln b)` with `k_mid` the generator level at the middle of the length.
   Then `R(k_mid) = 1`. With `L = k ln b`, the growth rate is `g(L) ~ L e^(-L / L_mid)`, which has its maximum at
   `L_mid`. So `g` is flat to first order at the middle: 0.82 of the maximum at `0.5 L_mid` and 0.74 at `2 L_mid`.
   The first hour has lower levels and a faster change of `g`. The first stages sit on the trajectory there (L8
   step 4), so the wall ratio `w` serves only the time after hour 1.

**Result.**

`r_eff = ln m / (n ln b)`; `R(k) = sum of r over the carriers + 1 / (k ln b)`; the solver sets `R(k_mid) = 1`

**The rate g.** L1 gives each carrier the share `r_c / R_c` of the spend. So

`g = a v k_mid * product over the carriers of (r_c (b_c - 1) / a_c)^(r_c)`

at the middle of the length, with `a` the active factor, `v` the value of 1 generator level and `a_c` the first cost
of the carrier. This is an estimate: it is the first value of the solver, which then measures `g` in the run with no
stage and scales the group costs (`references/design-solver.md`, "1 long run").

**Design rule.** Give the generator no cap and count steps from the proven range of S12.count_steps (`m` 1.025 to
4, `n` 10 to 100). Take the finest steps (`n` = 10): a coarse step keeps the trajectory away from the law (section
7, limit 6). Give each carrier `r = R_c / carriers`.

**What is proved, and what is simulated, not proved.**

| Statement | Status |
| --- | --- |
| `r_eff = ln m / (n ln b)` for the step part | Proved: the derivation of L1 for a multiplicative line with the value ratio `m^(1 / n)`. `verify_math.py` checks it in L11 with the log factor |
| The local exponent `1 / (k ln b)` of the factor `k`, and `R(k) > 1` at each level for `R_c = 1` | Proved by the derivation. A trial solve with `R_c = 1` gave an income of 6e97 at minute 20: an observation, the script does not check it |
| `R(k_mid) = 1` gives a `g` that is constant enough from hour 1 to the hard end | Simulated, not proved. The proof sheet measures `g` of the 2 halves of that window and shows both in PO1. PO1 fails when they differ by more than 20%, the limit of each comparison of math and simulation |
| The formula for `g` | Estimate. The solver measures `g` |
| The buyer plans toward a step | An assumption of the simulator: the buyer values the block of levels up to the next step with the factor of the step (`references/model-schema.md`, "count_steps"). A play test must confirm it |

**Check.** `tests/test_generator_growth.py` solves the forward test `tests/designs/generator-growth/`.
The skeleton (2.5 hours, 2 groups and an upkeep line): `R_c` 0.928 + `1 / (k_mid ln b)` 0.072 at level 100 = 1.000.
`g` of the 2 halves after hour 1 is 0.00238 and 0.0021 /s (12% apart). The active player owns 180 generator levels at
the hard end and buys the last one at minute 148.5 of 150.5. With `R_c = 1` and a cap of 9 levels, the generator was
complete at minute 3.0 of 152.5.

### Content in 1 long run

**Why.** A game of 1 long run takes its pacing from new lines and systems that open in order (the table above).
The content file of the content step names them (`references/content-step.md`). The solver must place them and keep
`g`.

**Assumptions.** The section above holds. A content element adds a constant factor `F_i` to the income when the
player reaches it: a milestone with a `mult` effect, or a side line with a cap (`F_i = d^cap`, L7). Each element
opens at a lifetime total of the economy currency.

**Derivation.** With `C(t)` the product of the factors that the player holds, `I = K C S^R` and
`d ln S / dt = K C S^(R - 1)`.

1. With `R = 1`, a constant factor multiplies the rate: `g -> F g`. It does not only move the level. A trial
   solve with `R(k_mid) = 1` and 11 content factors (a product of about e^5.6) gave `g` 7 times the goal: an
   observation, the script does not check it.
2. `ln S` is linear in `t` with the slope `g` when `C ~ S^(1 - R)`, so `ln C(t) = (1 - R) g t`. The content must
   then add to `ln C` at the steady rate `(1 - R) g`, and `ln I = R g t + (1 - R) g t = g t`.
3. With the sum `Lambda = sum of ln F_i` over the length `H`: `1 - R = Lambda / (g H)`. This is the content share
   `R_content`: the part of the growth that the content carries. The carriers hold the rest.

**Result.**

`R_c + 1 / (k_mid ln b) + R_content = 1`, with `R_content = (sum of ln F_i) / (g H)`

**Design rule.** Size the content before the carriers: each factor that the content adds takes `r` from the
carriers. Spread `ln F` evenly over the length. The solver sets the cap of each side line with no stated levels in
proportion to the time to the next such line, and gives all of them `0.3 e`-folds for each line in total
(`SIDE_FOLDS`, a choice of the solver: with 1 e-fold for each line, `g` of the second half of the test design was a
third of `g` of the first half, because the group costs came late). A second generator line (an additive line) is
not a factor: it takes the count steps of the generator and the value `v (c / a)^r`, so the 2 lines are 1 group
with the same `r` (D6).

| Statement | Status |
| --- | --- |
| `g -> F g` for a constant factor at `R = 1` | Proved by the derivation (L2 with `K -> F K`) |
| `R_content = Lambda / (g H)` keeps the mean `g` | Proved by the derivation for content that comes at a steady rate |
| `g` between 2 elements, and with content that comes in steps | Simulated, not proved. PO1 shows `g` of the 2 halves, and fails when they differ by more than 20% |
| The idle value (PO6) | With a content share above the tolerance of the PO1 sum, or with 2 generator lines: no formula. The formula of the table below takes the carriers as the whole growth, so the simulation decides. With a smaller share the formula holds, and the comparison with the simulation checks it |

**Check.** `tests/test_generator_growth.py`, case `named` (16 named elements).
`R_c` 0.820 + `1 / (k_mid ln b)` 0.102 at level 70 + `R_content` 0.101 = 1.023. `g` of the 2 halves is 0.00192 and
0.00185 /s (4% apart), and the hard end is at 2.65 hours (band 2 to 3). Each timed element opens within 3 minutes of
its target time.

### What the laws prove for 1 long run

| Row | Law | Formula with no reset | Status |
| --- | --- | --- | --- |
| PO1 | L1, L2 | `R` after the caps is 1, and no additive line of the economy is uncapped. With a generator carrier: `R + 1 / (k_mid ln b) + R_content <= 1` within the tolerance of the proof sheet | proved for a capped generator. With a generator carrier: the sum is proved, and the constant `g` is simulated (the 2 halves of `g` within 20%) |
| PO6 | L4 | `rho(T) = (ln(1 + f g T) + g V) / (g (T + V))`: a wait of `T` at the share `f` adds `f g T S` to the spend, and the visit of `V` seconds adds `g V` to `ln S` | proved |
| PO7, PO8 | L4 | property 3: `f <= 1` and `R > 0` | proved |
| PO9 | L5 | the active factor `a` is the scale tool `f = 1 / a` | proved |
| PO10 | L3 | `u = 3600 g / ln w`, `1 / u <= sigma` | proved |
| PO11 | L8 | `a_1 <= 15 I_0`, 3 unlocks in the first 5 minutes | proved |
| PO12 | L8 | `H = 1 + (N - u_1 - ln(P(1 h) / h_(u_1)) / ln w) / u`: the length from the stage count | proved |
| PO13 | L9 | `g` of L2 is the relative growth of L9, so `g k D_h <= 0.1` is `T_e = 1 / g >= 10 k D_h` | proved |
| PO2 to PO5 | L6 | the reset loop | not applicable: no reset layer |

`g` comes from the run with no stage, as `A` comes from the trajectory for a reset loop: the proof sheet measures
the slope of `ln I` from hour 1 to `H` (`pw_proof.run_growth`), and the formula of each row uses it. The simulation
half of each row checks the model with its stages. With a generator carrier, `g` is constant only to first order,
so the formulas of PO6, PO10, PO12 and PO13 hold as far as the 2 halves of `g` agree; the comparison of each row
with the simulation (20%) is the check.

These rows do not apply, and the reason is the same for each: the model has no reset currency, no reset rule and no
run length `T_X`. PO2 (`lambda`), PO3 (`s`), PO4 (the gap between 2 reset rules) and PO5 (`T_X`) have no input. The
proof sheet marks them `not applicable` with the reason "no reset layer". They are not failures.

The simulation path stays for each part that no law covers: an auto-buy (S24), chance (S10), a chain of producers
(S18), an allocation (S21), and any `R < 1` model with no reset (the trial Going Ham had `R = 0.468`). Rows of the
play pattern (PC1 to PC6, PV) are targets, and the simulation decides them as for each model.

**Check.** `tests/test_long_run.py` solves starter set K9 with the default targets (6 hours) and with 2.5 hours.
6 hours: `R_c` 0.960 + 0.041 at level 174 = 1.001, `g` 0.00194 /s, halves 0.00198 and 0.00184 /s, PO12 6.01 h
against 5.91 h simulated. 2.5 hours: `R_c` 0.929 + 0.072 at level 100 = 1.000, `g` 0.00193 /s, halves 0.00202 and
0.00185 /s, PO12 2.60 h against 2.54 h simulated.

## 3b. Upkeep: a factor below 1 that care raises

Ticket 41 (repair 4). The card S30 (`references/systems-catalog.md`) multiplies the output by an uptime factor
`u = max(floor, min(1, C / N)^e)`. The need is `N = N0 + sum of h x levels` of the producer lines. The capacity is
`C = C0 x c^k` for a factor care line with `k` levels (or `C0 + c x k` for an add care line). Factorio gives `e = 1`
(https://wiki.factorio.com/Electric_system, sourced). The solver path is in `references/design-solver.md`, "Upkeep".

### What u does to the growth exponents

1. **A bounded factor (L7).** While the capacity keeps up, `u` stays in a band `[u_lo, 1]`. Then
   `ln I = ln I_free + ln u` with `|ln u| <= -ln u_lo`. This is the row "Level cap" of L7: a constant factor stays,
   and `R` does not change. With `R = 1` (section 3a), `g` of L2 does not change.
2. **A brake.** When the need grows faster than the capacity, `u = (C / N)^e` falls. Let `nu` be the elasticity of
   `N / C` in the spend. Then the income exponent of L1 falls from `R` to `R - e nu`. Units bought on a geometric
   cost give `n ~ ln S / ln b`, so `nu` tends to 0: the brake is a factor `(ln S)^(-e)`, and `R` does not change.
   With a fixed capacity and an add producer line of output `v n`, the income is `v C^e h^(-e) n^(1 - e)`: the
   exponent of the line in its units falls from 1 to `1 - e`. With `e = 1`, more units pay nothing.
3. **A best split with `e < 1`.** `I ~ n^(1 - e) C^e` has a fixed best share of the spend for care (inferred). A
   stable best split ends the decision (risk of the card).

### The condition for no death spiral

A producer level with the income factor `f` (`f = d` for a factor line, `f = (B + v) / B` for an add line with the
base `B`) and the need `h` changes the income by `f (N / (N + h))^e` while `u < 1`. A care level never lowers the
income. So:

- **No producer level lowers the income** when `f >= (1 + h / N)^e`. The smallest `N` with `u < 1` is the worst case.
  For `d = 1.1`, `h = 1` and `e = 1`, this needs `N >= 10`.
- **A buyer that sees the income does not buy a level that lowers it**, so with the rule `need_capacity` the income
  never falls with no purchase. The death spiral is a stall: the income stops rising, because the next producer
  lowers it and the next care level costs too much. No stall needs `a_c b_c^k <= T_gap I(u)`: the next care level
  costs at most the unlock gap target `T_gap` of the income at the present `u`.
- **An auto-buy (S24) of producers** buys with no look at the income, so it can push `u` down. A stock variant
  (`S30.decay_refill`) stops a unit at an empty stock, and that is a real spiral (Oxygen Not Included: death at 0 kcal,
  https://oxygennotincluded.wiki.gg/wiki/Duplicant, sourced). The laws do not cover either case.

### Where the best purchase switches

While `C >= N + h` (slack), 1 care level raises nothing: the best purchase is a producer. While `u < 1`, 1 care level
pays `c^e` (a factor care line), and 1 producer level pays `f (N / (N + h))^e`. A buyer that compares the gain for
each cost buys care while `e ln c / cost of care > (ln f - e ln(1 + h / N)) / cost of the producer`. So the best
purchase switches near `C = N`, each time the producers use up the slack. The simulator values a care level that
raises nothing now by the gain that it frees for the best producer (a 2-purchase view), so its buyer buys care just
before the producer that needs it.

### The proof rows of the card

| Row | Statement | Mark |
| --- | --- | --- |
| PU1 | `u` of the active player is inside its band for 90% or more of the play time | simulated |
| PU2 | No death spiral: the income of the active player never stops rising for longer than the unlock gap target, and the player reaches the hard end | simulated (the law gives the condition, not the path of the buyer) |
| PU3 | 1 care level raises the income by the PV floor or more while `u < 1`: `c^e - 1 >= floor` | proved by the formula for a factor care line; the simulation checks that each care purchase gains the floor or closes the shortfall `1 / u - 1` |
| PU4 | The purchases of the active player switch between producers and care `min_switches` times or more (default 4, a designer default with no source) | simulated |

PV of a producer line (a need line) uses the gain of the line with no change of `u`. PU1 to PU4 judge `u`.

### What the laws cannot say

- The path of `u` over the game for a real buyer, and so the count of switches. Only the simulation gives them.
- The effect of an auto-buy of producers, of a stock that drains (`S30.decay_refill`), and of a pool share
  (`S30.pool_share`): the solver writes none of them.
- Whether a human player buys care at the right time. The simulated buyer sees the income at once.

**Check.** `tests/test_upkeep.py` checks each claim with a small simulation: the bounded factor (`g` with and
without the upkeep block within 10%), the brake (`I = v C^e h^(-e) n^(1 - e)` for a fixed capacity), the condition
`f >= (1 + h / N)^e` (the sign of the gain of 1 producer level on each side of `N = 10`), and the switch point (each
care purchase comes while `u < 1` or while the next producer level would put `u` below 1).

## 4. Numeric checks

`scripts/verify_math.py` uses Python 3 and its standard library only. The simulation is event-driven: income is constant between purchases, so the script jumps to the next purchase, reset or decision. A greedy buyer buys the line with the largest `d ln P / cost`, and waits when that line costs too much.

| Law | Player types | Tolerance | Result |
| --- | --- | --- | --- |
| L1 | Greedy buyer with a fixed spend | 3% on `R`; additive level exact | PASS |
| L2 | Active greedy, decision 1 s | 5% exponent, 2% rate, 1% blow-up time | PASS |
| L3 | Active greedy; active greedy with rule `X = 1` | 0.1 stage; 5% stage rate | PASS |
| L4 | Active greedy; open idle with 1 visit | 10% on each gain | PASS |
| L5 | Open idle with each tool; visit reset and away reset | 10% on each gain; class by slope | PASS |
| L6 | Patient (`X = 10`), early (`X = 1`), best (`X = 3.92`) | 5% (15% on the Moss Works gains) | PASS |
| L7 | Greedy buyer with a fixed spend; active greedy with a second clock | 0.02 on `R`; 5% exponent | PASS |
| L8 | Worked example 2, active greedy | 15% length; 1 unlock in hour 1 | PASS |
| L9 | Active with decision gaps 1 s to 3,000 s | 5% | PASS |
| L11 | Greedy buyer with a fixed spend; active greedy to level 150 | 3% on `R`; 1% on each block gain | PASS |

Corrections made during the work. Each change fixed a measurement, not the model:

1. L4 first failed with an error of 17.9%. The lines had `b = 2`, so power moved in large steps, and the law is continuous. The check now uses lines with `b = 1.1`. The error is 3.6%.
2. L6 first failed at 7% and 6%. The rate window counted part of a run, and a 1-second decision grid was coarse against runs of 25 to 50 seconds. The check now measures from reset to reset and uses a 0.05-second grid.
3. L9 first failed at 4% and 11%. The time to the target was counted in whole decision steps. The check now measures the rate from decision to decision.

## 5. Worked example 1: Moss Works

Data: `data/upgrades.json`, `data/stages.json`, `data/garden.json`. Measured facts: `docs/research/moss-works-idle-vs-active.md`.

### Lines and exponents

| Line | Currency | Stat | `r` | Cap |
| --- | --- | --- | --- | --- |
| Mister | spores | spores/s | 0 (additive) | none |
| Grow Lamp | spores | grow | 0 (additive) | none |
| Compost | spores | spores/s | 0 (additive bonus) | 20 |
| Trellis | spores | grow | 0.678 | 12 (+2 for each specimen) |
| Spore Bank | spores | spores/s | 0.631 | 5 |
| Fogger | spores | spores/s | 0.514 | 15 |
| Mirror Wall | spores | grow | 0.485 | 20 |
| Mushroom Ring | spores | spores/s | 0.631 | 10 |
| Canopy Net | spores | grow | 0.631 | 10 |
| Root Memory | Mycelium | spores/s | 1.000 | none |
| Deep Roots | Mycelium | grow | 0.514 | none |
| Drip Line | Dew | grow | 0.297 | none |
| Morning Mist | Dew | spores/s | 0.550 | none |

Each multiplier is its own group in `_stat` (`systems/economy.gd:455-469`).

- `R` of the spore-income lines, all uncapped: 1.776. `R` of the grow lines: 1.794. Both are above 1. So the middle game has finite-time growth until the caps stop it (L2).
- Before Stage 12, the income lines are additive only: `R = 0` with log factors.
- Reset loop: `p = 0.5`. Root Memory costs `2^k - 1` and gives `2^k`, so `q = 1` with no cap. So `lambda = 0.5 / (1 - R)`: 0.50 at `R = 0`, 1.00 at `R = 0.5`, and no limit as `R` goes to 1.
- Dew comes from a clock with no purchase (`drip_seconds` = 60). It is a second clock (L7).
- A stage clear costs no time, and no presence tool applies to open idle.

### Proof obligations that Moss Works fails

1. PO1: `R` = 1.78 in the middle game, with a reset layer.
2. PO2: `lambda > 1` for `R > 0.5`. The P1 run lengths give `lambda >= 4.6` (below).
3. PO4: the patient player P1 finishes in 36% of the A1 time, a gap of 2.8. The loop is not stable, so no `s` bounds the gap.
4. PO6: from Stage 10, a 24-hour wait leaves 7.0 active minutes of the game. 24 idle hours are worth more than the whole remaining game.
5. PO9: open idle has no cap and no rate below 1, and no shape tool applies. The stage clear needs no visit.
6. PO10: the longest gap with no unlock in the first hour is 23.25 minutes, against 15-minute sessions.
7. PO12: the game has no length target. A1 finishes in 6.25 hours, and P1 in 2.26 hours.
8. PO13: late P1 runs last less than 1 minute. x100 with a 2-second human gives 200-second gaps, longer than a run.

PO8 holds at its limit: `f_open` = 1 and `f_closed` = 1 for 8 hours. So no idle state pays more than active play, but active play also has no income that idle play does not have.

### Predictions against the measured facts

| Measured fact | Law and prediction | Status |
| --- | --- | --- |
| A wait of `T` with no purchase, then 1 visit, gives about 1 stage for each doubling of `T` | L4 with `T >> tau`: `R_s ln(2) / ln(w)` stages for each doubling. Trellis alone (`R_s` = 0.678, `w` = 1.9) gives 0.73. The additive Grow Lamp, the stage rewards (`25 * 1.6^(n-1)`) and the Glass Dome event add to it | Approximate: 27% low |
| P1 gains 51, then 511, then 6,207 in 7 minutes | L6: the rule `G >= 10 L` gives `>= 510` and `>= 5,620`. With `lambda > 1` the runs collapse. A run ratio of at most 1/7 with an `L` ratio of 12 gives `1/s - q <= -0.78`, so `s >= 4.6` and `lambda >= 4.6`. This needs an in-run `alpha >= 9.2`, which is `R >= 0.89` (estimate) | Proved for the gains. The `R` value is an estimate |
| Reset gain `floor(sqrt(run_spores / 100000))`, reset upgrade `2^k` for `x2^k`, no cap | L6: `p q = 0.5`, so the loop is stable only for `R <= 0.5`. The middle game has `R` up to 1.78 | Proved |
| `log10(income)` for each hour: 2.7, 4.8, 1.5, 0.8, 0.8, 0.4, 0.4 | Hours 0 to 2: `R > 1` before the caps and `lambda > 1`, so growth speeds up (L2, L6). After the caps and the last Regrow, the run is polynomial, and the slope falls as `1 / t`. `1 / t` predicts a ratio of 3.5 / 5.5 = 0.64 between hour 5-6 and hour 3-4. The measured ratio is 0.43 / 0.84 = 0.51 | Approximate: the ratio is 20% low |
| A stage clears at `grow * 10 >= 20 * 1.9^(n-1)` with no time cost | L5: no shape tool. The bank of a long wait turns into stages in 1 visit | Proved (L5 class) |
| Active play has no income that idle play does not have | L4: `f` = 1, so `rho -> 1` for short gaps. L9: at x100, idle time and active time differ only by the decision gaps | Proved (L4, L9) |

## 6. Worked example 2: a corrected small game

### Targets

| Target | Value |
| --- | --- |
| Length | 20 active hours, within 15% |
| Session | 15 minutes: median unlock gap after hour 1 at most 15 minutes, largest gap at most 20 minutes |
| First hour | 6 unlocks, within 1 |
| Idle at 8 hours | open idle with a visit each 8 hours is worth 30% to 50% of active play |
| Reset gap | the patient player (`X = 10`) is at most 2 times faster than the early player (`X = 1`) |
| Added by this document | closed idle with a visit each 24 hours worth 10% to 25%; idle above 0 and below active from 1 hour to 30 days; active slow within 1.25 times of active greedy |

### Systems

1. Generator line (additive): `a = 10`, `b = 1.15`, `+1/s` for each level, cap 40. The cap keeps `lambda` from rising above 1 (L7).
2. Multiplier group 1: 1 line, `a = 100`, `b = 1.6`, `r = 0.1`, so `d = 1.6^0.1 = 1.048`.
3. Multiplier group 2: 1 line, `a = 1000`, `b = 2.2`, `r = 0.1`, so `d = 2.2^0.1 = 1.082`.
4. Spine on power (income at the active rate). The first 6 stages come from the trajectory, then wall ratio `w`.
5. Reset layer: `G = (E / c)^p`, `M = (1 + L)^q`. The reset currency is not spent: each point is a permanent multiplier through `M`. The game shows `G` with decimals (no floor).
6. Offline rule: `f_closed = 0.5`, `T_cap` = 12 hours.
7. Presence tool: an active income term. Active play doubles income. So open idle and closed time earn `f = 0.5` of the active rate. The game states the active rate as its base.

### Parameters from the procedure

| Step | Formula | Value |
| --- | --- | --- |
| 3 | `R = 0.1 + 0.1` | 0.20, so `alpha` = 1.25 |
| 4 | `s = p alpha` with `X_e = 1`, `X_p = 10`, gap `<= 2`: `s <= 4.2`. Choose `p = 3` | `s` = 3.75, predicted gap 1.87 |
| 4 | `q = (1 - R) / p` | 0.2667, so `lambda` = 1 |
| 5 | `A` from the L2 trajectory | 14.33 |
| 5 | `c = A * 900^1.25` | 70,650 |
| 6 | `kappa = 0.5 * 1.25 * (120 / 900)^0.25` | 0.378 |
| 6 | `rho_max = p * kappa / (e * ln 2)` | 0.60 (below 1) |
| 8 | `ldot = 3600 * q * ln(2) / 900` | 0.739 for each hour |
| 8 | `w = e^(1.25 * 0.739 / 5)` | 1.2030 |
| 8 | First stage powers at 0.5, 2, 5, 10, 25 and 45 minutes | 6, 24, 44, 65, 94, 110 |
| 8 | `N` | 102 |
| 9 | `T_X / (k D_h) >= 10` with `D_h` = 2 s | x10 is valid; x100 is not |

### Simulated player types

The script plays 5 player types. A visit lasts 120 seconds: the player spends the bank, resets if `G >= L`, then plays.

| Player | Result |
| --- | --- |
| Active greedy (`X = 1`, decision each 1 s) | Finish (Stage 102) at 20.33 h. 7 unlocks in hour 1. After hour 1: median gap 13.2 min, largest gap 17.1 min |
| Active slow (decision each 60 s) | Finish at 21.62 h, 1.06 times the greedy time |
| Open idle, visit each 8 h | Idle ratio 0.334 (predicted 0.337). Finish at 64.3 h of wall time with 16 active minutes of visits |
| Closed idle, visit each 24 h | Idle ratio 0.130 (predicted 0.131). Finish at 168.3 h |
| Patient reset (`X = 10`) | Finish at 11.07 h. The early player takes 1.84 times as long. `l` rate ratio 1.87 (predicted 1.87) |

Idle ratio for each visit interval (open idle unless stated):

| Visit each | Simulated | Predicted (L4b) |
| --- | --- | --- |
| 1 h | 0.557 | 0.538 |
| 8 h | 0.334 | 0.337 |
| 24 h | 0.162 | 0.162 |
| 7 d | 0.036 | 0.036 |
| 30 d | 0.010 | 0.011 |
| 24 h closed | 0.130 | 0.131 |

### Targets met

| Target | Result |
| --- | --- |
| Length 20 h within 15% | Met: 20.33 h |
| 6 unlocks in hour 1, within 1 | Met: 7 |
| Session 15 min | Met: median 13.2 min, largest 17.1 min |
| Open idle at 8 h worth 30% to 50% | Met: 33% |
| Patient at most 2 times faster | Met: 1.84 |
| Closed idle at 24 h worth 10% to 25% | Met: 13% |
| Idle above 0 and below active, 1 h to 30 d | Met: 0.557 down to 0.010 |
| Active slow within 1.25 times | Met: 1.06 |
| x100 with 200-second decisions within 1.1 times (extra) | Missed: 1.148. `T_X` is only 4.5 decision gaps. x10 with 20-second gaps gives 1.021 (L9) |

The 7 unlocks in hour 1 are 1 more than the target. The first 6 stage powers sit on the trajectory at 45 minutes and earlier, and the second segment starts at Stage 7 with `w` = 1.203. The trajectory passes Stage 7 before minute 60.

## 6a. Content that carries the growth

**Goal.** Each purchase raises the income by a floor `f` or more (proof row PV), and the content of the game, not
2 lines, carries R.

**Law used.** L1 with D4 and D5, and L6.

1. A multiplicative level raises the income by `d - 1` at each level. So `d >= 1 + f` holds the floor at each level.
2. An additive level k adds `v / (B + v (k - 1))`, with `B` the pool before the line. So the floor holds for at most
   `1 + 1/f - B/v` levels. A `pct` level is the same with `B = 1`. A line with more levels must be multiplicative.
3. In the simulator a mult effect always multiplies, so each mult line is its own group (S02.multiply_all), and
   `R = sum of r` over the open lines. With n lines that share R: `r = R / n` and `b = d^(1/r)`.
4. The trajectory `E_1(x) = A x^alpha` of L6 holds when each line of R is open from the start of each run at a small
   first cost, and when no line hits a cap. A capped line hits its cap in the long trajectory that measures A but not
   in a short run, and a late first cost opens a line only late in each run. Both move A away from the runs.
5. A persistent constant factor F shortens each later run by `F^(-1/alpha) = F^(-(1 - R))` (L6:
   `T ~ (c / (A F))^(1/alpha)`). The runs hold only the factors of their time, but the trajectory of 10^6 s that
   measures A holds each one. So L6 takes `A x F_held / F_traj` (the proof sheet measures both). An uncapped line of
   the reset currency is not in F: its r is a part of q (D8). When `F_held / F_traj` or its inverse is above
   `1.2^(1 / (1 - R))`, one A does not hold for the runs, and the simulation decides the run length. The solver keeps
   each factor at its game value (it does not cut the product to that bound), so that the content keeps its
   character.

6. **Identity of the lines.** Each line keeps its game step: a mult line `v`, a pct line `1 + v`. An add line stays
   additive with the levels of item 2, so it is not a line of R. A step outside [`1 + f`, `step_top`] moves to the nearer end. `step_top` is a stated input; with none, it is the
   largest d of S02.levelled_factor (2).
7. **Cap room.** When a cap binds, the line leaves R (L7), so R falls to `R_end` and lambda falls to
   `(1 - R) / (1 - R_end)`. PO2 holds while this is within its tolerance `t` (20%). So the capped lines can take
   `R_capped <= (1 - R)(1/(1 - t) - 1)`, and PO3 also needs `R - R_capped >= (1 - p) * 1.05`. The solver gives the
   room first to the lines that a cap raiser raises, then to the other capped lines, smaller r first. A capped
   line keeps its game cost ratio when its r fits in the room left, else its cost ratio rises to the b that fits, at
   most the largest `b` of S02 (10), else its step falls to `10^room` when that is at least `1 + f`. A line that does
   not fit the room then loses its cap, and the report states the reason.
8. **Uncapped lines.** The uncapped lines take their game cost ratios times 1 exponent k: `b_i = b_game,i^k` with
   `sum of ln d_i / (k ln b_game,i) = R - R_capped`. So the order of the cost ratios of the game stays. When a `b_i`
   passes the largest b, the uncapped steps shrink by 1 exponent instead.
9. **Placement windows.** The lines of R open inside the first hour, because a line that opens late moves A
   (item 4). A side system that the demo end names opens within 1 hour after the demo end. The lifetime totals of
   a side currency come after that currency starts. A cap raiser opens when its line reaches its cap. The last slot
   is 1 unlock gap target before the hard end. After the last placement round, an unlock that the active player does
   not reach moves down to the total that the player has at the hard end.
10. **First hour.** The rounds put each unlock into the largest gap between 2 unlocks of the active player in its
    window. So the largest first-hour gap falls as the count of unlocks in the first hour rises. The proof row PC4
    states the gap, and the report names the minutes where 1 more unlock closes the largest gap.
11. **Cap raisers.** A cap raise on a line of R that keeps no cap (item 7) opens no level. The solver moves the raise
    to the first capped line of R, so each raise opens levels at the floor step. With no capped line of R, the report
    states that the game must change the element.
12. **Auto-buy.** The simulator buys each affordable level of an auto-buy line (S24) from the minute of the plan,
    the cheapest level first, also while the game is open and the player is idle. L4b assumes no purchase while
    idle, so with an auto-buy the simulation decides PO6, and PO7 holds by dominance: the active player has the same
    auto-buy and also buys by hand.

**Check.** Moss Works (`tests/designs/moss-works-full/`, ticket 36, round s = 1.25) has 7 lines of R. compost keeps
its cap 20 with the step 1.189 (item 7). The 6 uncapped lines take `b = 10` and their steps shrink by 1 exponent from
the game steps (spore_bank, ring and canopy 1.29, trellis 1.188, fogger 1.161, mirror 1.132), so their order stays.
lamp stays additive (add 2). 28 of 28 elements are reached (PC5), and the hard end is at 5.23 h (PC1). With
`b = 10` and all the caps of the game, the same content gave a measured lambda of 0.5 and runs 3 times longer than
L6.

## 7. Limits

The laws do not cover these cases. A simulation must still check each of them.

1. Chance: random drops, critical hits and random events. The laws use expected values only. Variance can make a wall feel longer than its mean.
2. Several currencies with conversion between them. L10 covers 1 second income that the main income buys (a chain of 2 steps). A longer chain, a loop back into the main income, or a conversion rate that depends on the amount held can add a hidden exponent.
3. Generators that produce generators. Each tier adds 1 power of `t` (`S/references/other-games-math.md:196-198`). L2 assumes that 1 currency buys power directly.
4. Lines whose costs are not geometric, such as polynomial costs. L1 assumes `a * b^k`.
5. Phase changes. `R` changes when a line unlocks or hits a cap. The laws hold inside a phase, and the transitions need a simulation. The first hour of worked example 2 is such a transition.
6. Discrete steps. Lines with a large `b` move power in steps. The L4 error was 17.9% with `b = 2` and 3.6% with `b = 1.1`.
7. A second reset layer (L7 is a conjecture).
8. Human behavior that is not greedy: buy orders, waste, and a reset rule that changes with mood. The 5 player types are models.
9. The real-time systems of the reference game, such as fights with cooldowns, are modelled only through L5. Their interaction with a reset loop needs a simulation.
