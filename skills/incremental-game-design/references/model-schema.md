# The game model

The model is one JSON file. It describes the progression of a game in enough detail to
simulate it. `assets/example_model.json` is a complete small example: copy it and change it.

Keep the model small. Model each purchase line, each milestone, and each reset. Do not model
combat, physics, or art. When a mechanic has chance in it, model its mean rate. When a number
is a guess, say so in `game.assumptions`: the report must list them.

The script checks the model when it loads it. An unknown id, a missing field, or a wrong
formula name stops the run with one line that starts with `model error:`. Fix the model and
run again. Things that are allowed but probably not intended are printed as `WARNING:` lines
and written to the Dashboard.

## Top level

```json
{
  "game": {}, "currencies": [], "systems": [], "purchases": [], "milestones": [],
  "ladders": [], "prestige": {}, "interactions": [], "bands": {}, "review": {}, "upkeep": {}
}
```

Only `currencies` is necessary, and a model with no `purchases` has nothing to simulate. Each
id is a short lower-case word. Ids are unique inside their list.

## game

| Field | Meaning | Default |
| --- | --- | --- |
| `name` | Name of the game | file name |
| `session_minutes` | Length of one play session | 20 |
| `target_hours` | Hours of play to simulate | 20 |
| `active_fraction` | Share of time that the player does the active action (0 to 1). It scales `add` effects that have `"active": true` | 1.0 |
| `progress_axis` | The id of the ladder that is the spine of the game. The Reference chart, the Prestige sheet, and the Sensitivity sheet use it. An id that is not a ladder gives a warning | none |
| `primary_profile` | The buyer profile that fills the Timeline and the Dashboard: `eager`, `patient`, `doubler`, or `model` (see prestige). The command-line option `--profile` overrides it | `eager` |
| `pct_grouping` | `currency`: all `pct` effects on one currency add in one pool. `system`: the default pool of a `pct` effect is the system of its source, and pools multiply (see effects) | `currency` |
| `away_minutes` | Minutes away from the game after each session. 0 = continuous play (see "Sessions with away time") | 0 |
| `offline_rate` | Share of income that runs while the player is away and the game is closed (0 to 1) | 0 |
| `main_currency` | The main currency of the game. The proof sheet takes it as the economy currency when the prestige has no `basis_currency`. The model as built of the design solver writes it from `currencies.main` of the content file | the currency that pays for the most purchases |
| `presence` | The rules of the time away for the players (see "game.presence"). It also turns on the players | none |
| `targets` | The bands of the designer for the indicators of the players (see "game.targets") | none |
| `assumptions` | List of strings: each number that is a guess, and each mechanic that the model simplifies | none |
| `play_pattern` | `continuous` or `check_in` (references/interview.md). `continuous` adds the rows PC1 to PC4 to the proof sheet | none |

## currencies

Anything that the game counts and compares: coins, gems, prestige points, and also stats such
as damage.

| Field | Meaning |
| --- | --- |
| `id`, `name` | |
| `system` | The system that owns it. The link analysis needs this field |
| `base_income` | Income each second before effects |
| `start` | Amount at the start |
| `stat` | `true` for a value that is only compared with requirements and never spent (damage, speed). It needs no sink |
| `bank_cap` | Players only. While the player is away, the income of this currency goes into a bank that stops at this amount. The next visit adds the bank to the balance. Use it for a store that fills while away |

Income for a currency = (`base_income` + sum of `add`) x product over pools of (1 + sum of
`pct` in the pool) x product of `mult`. With the default `pct_grouping` there is one pool.

## systems

| Field | Meaning |
| --- | --- |
| `id`, `name`, `description` | |
| `loop` | `core`, `meta`, `minigame`, `collection`, `automation`, or `event` |
| `persists` | `true` if it survives the reset. Default: true for meta and collection. The script warns when this field says `true` and `prestige.resets.purchase_systems` names the system |
| `clock` | `game` or `real`. It is the default `clock` of each purchase, milestone, and ladder of the system (see "Clock and game speed") | `game` |

## purchases

A purchase line is anything bought many times with a rising cost: a generator, an upgrade, a
skill node with levels.

| Field | Meaning | Default |
| --- | --- | --- |
| `id`, `name`, `system` | | system `core` |
| `cost_currency` | Currency id | necessary (or `costs`) |
| `base_cost`, `cost_growth`, `cost_formula` | `exp`: base x growth^level. `linear`: base + growth x level. `poly`: base x (level+1)^growth | 1, 1.15, `exp` |
| `costs` | A list, in place of the three fields above, for a line that costs more than one currency: `[{"currency": "coin", "base": 50, "growth": 1.5, "formula": "exp"}, {"currency": "gem", "base": 2, "growth": 1, "formula": "linear"}]`. The buyer needs all of them. The first entry is the one that the Curves and Purchases sheets show | none |
| `max_level` | The base cap. `cap` effects of other entries raise it | none |
| `effects` | List of effects, applied once for each level | none |
| `requires` | List of requirements. All must be true | none |
| `excitement` | Weight of the first purchase of this line | 2 |
| `clock` | `game` or `real` (see "Clock and game speed") | the clock of its system |
| `count_steps` | `{"every": n, "mult": m}`: count steps (card S12.count_steps). Each `add` effect of the line takes the factor `m` at each `n` owned levels: the line adds `value x level x m^floor(level / n)`. See "count_steps" below | none |
| `place_minutes` | The target minute of the first purchase, for `pw_scaffold.py --fit` (`references/simulation-path.md`). The fit sets the unlock point (a lifetime total of the main currency) and the first cost of that minute. `null`: the fit takes the middle of the largest gap of the first hour. Milestones and `automation` entries take it too. The simulator ignores it | none |

A line whose cost is 0 at each level and that has no `max_level` is an error. A cost that does
not grow with no `max_level` gives a warning: the buyer can buy it without end.

A purchase with no effects is a pure unlock: use `requires` on other entries to make it
matter. The buyer gives it full value only while a purchase, a milestone, or the prestige
still needs it. If nothing needs it, the buyer buys it late.

### count_steps

`"count_steps": {"every": 10, "mult": 1.55}` on a purchase line is the compact form of "a multiplier at each 10 owned
levels". The line needs 1 or more `add` effects. `every` is a whole number of 1 or more, and `mult` is above 0.

- **The income.** The steps multiply the `add` effects of the line only: the output of that line, as in the games
  of the card. They do not multiply the base income or another line.
- **The buyer.** A buyer that sees 1 level at a time never sees the step. So both buyers also value the block of
  levels up to the next step: its cost, the wait for that cost, and the income gain with the step. The payback buyer
  takes the smaller of `wait + cost / gain` of 1 level and of the block. The buyer of version 1 takes the better of
  the 2 priorities. This is the assumption "the player buys toward the step".
- **The laws.** The line is a growth carrier with `r_eff = ln m / (n ln b)` (`references/balance-math.md` section
  3a). The proof sheet counts it in `R`, and the row PV tests the gain of 1 count block for it.
- **Why not a list of milestones.** A milestone with a `level` requirement and a `mult` effect can also express 1
  step: `{"requires": [{"type": "level", "id": "gen", "level": 25}], "effects": [{"target": "coin", "kind": "mult",
  "value": 2}]}`. Use that form for a few steps at levels of your choice (25, 50, 100, 200). It has 4 limits: a
  line with no cap needs a list with no end; the `mult` of a milestone multiplies the whole income, not the line;
  the buyer does not plan toward a milestone; and each milestone counts as an unlock (PC4) and as a content element
  (CG, PC3), so a list of steps would pass the content rows with no content. A step of `count_steps` is not an
  event, not an unlock and not a content element.

## effects

`{"target": "<currency id or all>", "kind": "add|pct|mult", "value": 1.5, "active": false, "group": "optional pool name"}`

`{"target": "<purchase id>", "kind": "cap", "value": 5}`

- `add`: + value each second for each level.
- `pct`: + value to an additive bonus pool for each level (0.1 = +10%). Bonuses in one pool
  add together. Pools multiply with each other. The pool is `group` when the effect sets it.
  Otherwise it is the one pool of the currency (`pct_grouping: "currency"`) or the system id
  of the source (`pct_grouping: "system"`).
- `mult`: x value for each level. It compounds. A value below 1 is a debuff.
- `"active": true`: the effect needs the player at the screen. In the version 1 run it scales
  an `add` effect by `active_fraction`. For a player (see "Players"), an effect of any kind with
  `"active": true` is off while the player is away. A requirement still compares the full
  income, so a stat does not fall while the player is away.
- `cap`: + value to the `max_level` of the target purchase for each level of the source (for
  a milestone: one time; for a ladder: for each step). The real cap = `max_level` + all
  raises. A `cap` effect on a line with no `max_level` does nothing and gives a warning.

The choice between `pct` and `mult` is the most important balance decision in the model. Ten
sources of +100% in one additive pool give x11. Ten separate x2 multipliers give x1024. Model
each source in the pool that the game code uses. Many games add inside one system and
multiply between systems: `pct_grouping: "system"` is that rule.

## requirements

| Type | Fields | True when |
| --- | --- | --- |
| `total` | `currency`, `amount` | lifetime earned is at or above the amount |
| `run_total` | `currency`, `amount` | earned in this run is at or above the amount |
| `have` | `currency`, `amount` | current amount is at or above the amount |
| `rate` | `currency`, `amount` | income each second is at or above the amount. Use this for a wall that compares power with a threshold |
| `level` | `id`, `level` | the purchase is at or above the level |
| `milestone` | `id` | the milestone is done |
| `ladder` | `id`, `step` | the ladder reached the step at any time |
| `run_ladder` | `id`, `step` | the ladder is at or above the step in the current run. Use it for a reset that tests the stage of the run: a `ladder` requirement stays true after the first reset |
| `time` | `amount` | seconds played is at or above the amount |
| `prestiges` | `amount` | resets done is at or above the amount |

A `rate` requirement on the next ladder step or on an open milestone makes the buyer plan
toward the wall: it counts a gain on that currency three times.

## milestones

One-time events: a system opens, a feature opens, a reward arrives.

| Field | Meaning |
| --- | --- |
| `id`, `name`, `system` | |
| `kind` | `system_unlock` (weight 5), `feature` (3), `content` (2), `reward` (2) |
| `requires` | List of requirements |
| `effects` | Effects while the milestone is done |
| `grants` | `[{"currency": "gem", "amount": 10}]`. Paid the first time only |
| `excitement` | Overrides the weight of the kind |
| `persists` | `true`: the milestone and its effects survive the reset. `false`: the reset clears it, the player earns it again in each run, and each repeat has weight 1. Default: it follows its system (`false` when `prestige.resets.purchase_systems` names the system) |
| `clock` | `game` or `real` (see "Clock and game speed"). Default: the clock of its system |

Use `system_unlock` exactly once for each system that is not open at the start. The Dashboard
and the Reference sheet count them.

When a milestone with effects is in a system that the prestige resets and the model does not
set `persists`, the script prints a warning. Set the field in each such milestone: a permanent
x2 and a x2 for each run are different games.

## ladders

A numbered sequence with a requirement that grows: stages, depths, bosses, levels. A ladder
is the usual spine of the game.

```json
{"id": "depth", "name": "Depth {n}", "system": "depth", "count": 30, "excitement": 3,
 "requires": {"type": "rate", "currency": "power", "base": 4, "growth": 1.9},
 "grants": [{"currency": "coin", "base": 50, "growth": 2.0}],
 "effects": [{"target": "coin", "kind": "pct", "value": 0.05}]}
```

`requires` is one object, not a list. Its type is `total`, `run_total`, `have`, `rate`,
`time`, or `prestiges`. `base` is necessary. `growth` defaults to 1 and `count` to 50.

Step n needs `base x growth^(n-1)`. A grant with `base` and `growth` pays
`base x growth^(n-1)` on the first time only. Effects apply once for each step reached.
`effects_persist` (default `true`) keeps the effects of the best step for ever; `false` makes
them follow the current run. The script warns when a ladder that the prestige restarts has
effects and the model does not set this field. The default weight of a new step is 2: raise
`excitement` to 3 if a step is a real event in the game. If no event in the run reaches the
beat weight, the script warns and the whole run is one lull.

| Field | Meaning | Default |
| --- | --- | --- |
| `requires.table` | A list of the amounts of the first steps: `"table": [6, 24, 44]`. Step n after the table needs (last entry) x growth^(n - length). The Walls and Curves sheets do not read the table: they show base x growth^(n-1). The timing fit of the rebalance writes 1 entry for each stage (the value of the spine stat at that stage in its run), and the proof sheet then reads the walls as fitted (`walls_fitted`) | none |
| `attempt_seconds` | Players only. Each step costs this many seconds of play after the step before. A step that the requirement allows waits for this time | none |
| `cooldown_seconds` | Players only. The ladder takes at most 1 step in each period of this length | none |
| `needs_visit` | Players only. `true`: the ladder takes a step only while the player is present. Away time gains 0 steps | false |
| `clock` | `game` or `real`. A ladder with the clock `real` counts its timers in real seconds | the clock of its system |

With no timer field, the ladder takes all the steps that its requirement allows at once, as in
version 1.

## prestige

```json
{"enabled": true, "system": "rekindle", "currency": "ember", "basis_currency": "coin",
 "formula": "power", "base": 100000, "exponent": 0.5, "k": 1,
 "requires": [{"type": "milestone", "id": "open_rekindle"}],
 "min_run_seconds": 300,
 "resets": {"currencies": ["coin"], "purchase_systems": ["forge", "crew"], "ladders": ["depth"]}}
```

`"prestige": {"enabled": false}` or no `prestige` block: the game has no reset.

| Field | Meaning | Default |
| --- | --- | --- |
| `currency` | The currency that the reset pays | necessary |
| `basis_currency` | Gain from the earnings of this currency in the run. `power`: floor(k x (earnings / base)^exponent). `log`: floor(k x log10(earnings / base)) | necessary (or `basis_ladder`) |
| `basis_ladder` | Gain from the step that the run reached on this ladder. `power`: floor(k x step^exponent). `exp`: floor(k x growth^step) | none |
| `formula`, `base`, `exponent`, `k`, `growth` | See the two rows above | `power`, 1, 0.5, 1, 1.5 |
| `requires` | Requirements for the reset | none |
| `min_gain` | The buyer does not reset for less | 1 |
| `min_run_seconds` | The buyer does not reset before this run length | 60 |
| `run_seconds` | A run-based game: each run ends after this many seconds of play. The buyer has no choice, and `requires`, `min_gain`, and `policy` are not used | none |
| `policy`, `peak_tolerance`, `policy_ratio` | The reset rule of the extra buyer profile `model` (see below) | `peak_rate`, 0.9, 1.0 |
| `unlock_in_run` | `true`: each `ladder` requirement of the reset tests the step of the current run (the loader writes it as `run_ladder`) | false |
| `max_resets` | The most resets of the game. The simulator makes no reset after this count, for each buyer and each player | none |
| `resets` | The currencies set to zero, the systems whose purchase levels go to zero, and the ladders that restart. A repeat ladder step is logged with weight 1 and pays no grant | none |
| `grants` | `[{"currency", "base", "growth", "count"}]`. Paid at each reset, after the gain: reset n pays `base x growth^(n - 1)`, only while n <= `count` (optional). A grant of the game with `trigger: reset` goes here, not to a ladder | none |
| `needs_visit` | Players only. `true`: a reset happens only while the player is present (the rule of version 1). `false`: the reset rule also runs while the player is away (an auto-prestige) | true |
| `floor` | `false`: the gain is not rounded down. Use it for a game that pays fractions of the reset currency | true |
| `peak_on_floor` | `true`: the `peak_rate` rule compares the rounded-down gain for each minute, not the gain before the round down. Some games decide with the rounded value | false |
| `reward` | `{"target": "coin", "exponent": 0.27}`: the income of the target (a currency id or `all`) is multiplied by (1 + L)^exponent. L is the lifetime total of the reset currency. Use it when the reset pays a multiplier that the model does not show as a purchase | none |

**Buyer profiles.** When the buyer decides when to reset, the lulls depend on that decision.
So the script runs the whole simulation under three reset rules:

| Profile | Rule |
| --- | --- |
| `eager` | `peak_rate` with tolerance 0.95: reset when the prestige gain for each minute of the run has fallen below 95% of its best value in this run |
| `patient` | `peak_rate` with tolerance 0.75 |
| `doubler` | `ratio` 1.0: reset when the gain is at or above the lifetime total of the prestige currency (the rule of the first version of the script) |
| `model` | Only when the model sets `prestige.policy`: `peak_rate` with `peak_tolerance`, or `ratio` with `policy_ratio` |

One profile is the primary one (`game.primary_profile`, default `eager`). It fills the
Timeline and the Dashboard. The Profiles sheet and the analysis JSON give the main indicators
for each profile. Each lull of the primary run is marked "robust" (a lull overlaps it in each
profile) or "depends on reset policy". A game with no prestige, or with `run_seconds`, runs
one profile.

The `peak_rate` rule does not fire while the gain for each minute still rises. If the primary
profile never resets and another profile does, the script prints a warning.

**Novelty.** The first prestige has weight 5. A later prestige has weight 3 only if its run
reached a new beat that is not a prestige. Otherwise it has weight 1 and is not a beat.

The model has one reset layer. For a second layer, model its currency as a slow income with a
milestone that grants it, and say so in the assumptions.

## Sessions with away time

With `game.away_minutes` above 0, the buyer plays a block of `session_minutes`, then is away
for `away_minutes`, and so on. While it is away, income runs at `offline_rate` and it buys
nothing. Milestones and ladder steps that became true while it was away are logged at the
start of the next session (and so they stack: see "Stacked beats"). `target_hours` and all
indicators count hours of play. The Timeline has a real-time column and a played-time column.
Each row of the Sessions sheet is then one real session.

## game.presence

```json
"presence": {"open_rate": 1.0, "open_cap_minutes": null, "offline_cap_minutes": 480, "visit_minutes": 2}
```

The players use these fields. The version 1 run does not use them. Between 2 visits the game
is in 1 of 2 states:

- **open**: the game runs with no input. The game clock runs. Income is the away income x
  `open_rate`.
- **closed**: the game is shut. The game clock stops. Income is the away income x
  `game.offline_rate`.

The away income is the income with no `"active": true` effect. A cap stops the income of its
state after that many minutes. A visit restarts the cap.

| Field | Meaning | Default |
| --- | --- | --- |
| `open_rate` | Share of the away income that runs while the game is open (0 to 1) | 1.0 |
| `open_cap_minutes` | The open income stops after this many minutes away | no cap |
| `offline_cap_minutes` | The closed income stops after this many minutes away. Use it for an offline cap | no cap |
| `visit_minutes` | The length of 1 visit of a player who visits on a schedule | 2 |

An empty block `"presence": {}` turns on the players with all the defaults.

## game.targets

```json
"targets": {"idle": {"open": {"8": [0.27, 0.39]}, "closed": {"24": [0.2, 0.4]}},
            "policy_gap": [1.59, 2.09], "unlocks_first_hour": [3, 8], "length_hours": [18, 22]}
```

Each target is a band `[low, high]` that the designer wants. A target replaces the default band
of its indicator on the Presence sheet and the Dashboard. references/indicators.md gives each
indicator and its default band.

| Field | Indicator |
| --- | --- |
| `idle.open.<hours>`, `idle.closed.<hours>` | The idle value after a wait of 1, 8, 24, or 168 hours |
| `policy_gap` | Time of early_reset / time of patient_reset |
| `unlocks_first_hour` | Progress axis steps of active_greedy in the first hour |
| `unlocks_per_hour` | The median of the progress axis steps of active_greedy for each later hour |
| `length_hours` | Hours of active_greedy to the end of the progress axis |
| `slowdown` | The slow-down at the play-test speed |
| `play_test_speed` | A number, not a band: the speed of the play-test build. Default 10 |
| `hard_end` | 1 of `{"axis_step": n}`, `{"purchase": id}`, `{"resets": k}` (the k-th reset) or `{"milestone": id}`: the end of the game (card S28). The simulator records its time (`Sim.end_real`). An end of kind `resets` or `milestone` stops each player (section "End and length"). Proof row PC1 |
| `demo_end` | `{"axis_step": n}` or `{"purchase": id}`: the end of the demo. The design solver also writes `minutes` and `before_system` (the targets of the interview), so the game reads the demo end from the model. Proof row PC3 |
| `largest_purchase_gap_minutes` | A number: the largest gap between 2 purchases of the active player. Default 2. Proof row PC2 |
| `first_purchase_minutes` | A number: the latest first purchase. Default 2. Proof row PC2 |
| `largest_unlock_gap_minutes` | A number: the largest gap between 2 unlocks over the whole length. Default 10. Proof row PC4 |
| `first_hour_unlock_gap_minutes` | A number: the largest gap between 2 unlocks in the first hour. Default 5. Proof row PC4 |
| `extension_part_hours` | A number: the length of 1 part past the demo end. Default 1. Proof row PC3 |
| `purchase_rate_max_per_minute` | A number: the most purchases that a person makes in 1 minute. Default 30 (designer target). Proof row PC6 |
| `purchase_value` | `{"floor": f, "source": text}`: 1 level of each line raises the income by `f` or more. Default 0.10 (designer target). Proof row PV, for a model with a `play_pattern` |

### Targets of the proof rows with a range

| Key of `game.targets` | Meaning | Default |
| --- | --- | --- |
| `ending_unlock_gap_minutes` | PC4: the limit of the last unlock gap, the gap that ends at the hard end | none: the last gap has the limit `largest_unlock_gap_minutes` |
| `tolerances` | `{target key: tolerance}`: the range of a target limit as a share of the limit (`references/proof-sheet.md`, "Target rows have a range"). The keys: `largest_purchase_gap_minutes`, `first_purchase_minutes`, `largest_unlock_gap_minutes`, `first_hour_unlock_gap_minutes`, `ending_unlock_gap_minutes`, `purchase_rate_max_per_minute`, `purchase_value`, `content_gap` | the default of the source class of the target (`references/proof-sheet.md` gives the 2 numbers) |
| `unsupported` | a list of target keys of `tolerances` that have no source in this game. Each gets the tolerance of a target with no source | none |

## Clock and game speed

A play-test build often runs the game clock faster (x10, x100). A source with the clock `real`
does not follow that speed: the simulator divides its `add` effects by the speed. A ladder
with the clock `real` counts its timers in real seconds. Set `clock` on the entry, or on its
system. Only the speed test of the players uses a speed. The version 1 run and the other
players play at x1.

## Players

The option `--players` runs players in addition to the version 1 buyer profiles. Each player
is a set of rules: how often it decides, when it is present, and its reset rule. The players
need `game.progress_axis`, or a model with 1 ladder.

| Player | Present | Decides each | Reset rule | Buyer |
| --- | --- | --- | --- | --- |
| `active_greedy` | all the time | 1 s | the primary profile of the model | payback |
| `active_slow` | all the time | 5 s | the primary profile of the model | payback |
| `open_1h`, `open_8h`, `open_24h`, `open_168h` | 1 visit each 1, 8, 24, or 168 hours. The game is open between visits | 1 s in a visit | the primary profile of the model | payback |
| `closed_1h`, `closed_8h`, `closed_24h`, `closed_168h` | 1 visit each 1, 8, 24, or 168 hours. The game is closed between visits | 1 s in a visit | the primary profile of the model | payback |
| `early_reset` | all the time | 1 s | G >= 1 x L | payback |
| `patient_reset` | all the time | 1 s | G >= 10 x L | payback |
| `eager`, `patient`, `doubler` | all the time | continuous | the version 1 profile | version 1 |

G is the gain of the reset now. L is the lifetime total of the reset currency. A player with
visits starts with a visit, and plays at most 40 visit intervals. A purchase waits for the next
multiple of the decision interval.

**Buyers.** The version 1 buyer scores each purchase by its gain for the wait. The **payback
buyer** is the buyer of the measured game (Moss Works, tests/pacing_buyer.gd):

1. It buys the cheapest affordable line that costs a currency other than the main currency.
2. Else it buys the main-currency line with the lowest wait + cost / value. The value is the
   main income x the sum of the relative income gains of 1 more level.

The new players use the payback buyer. The reason: with the version 1 buyer, the early game of
Moss Works was 28% slower than the game (Stage 10 at 42.8 minutes, measured 33.5), and the
reset players did not match the measured gap. The profiles `eager`, `patient`, and `doubler`
and the default run keep the version 1 buyer, so a model of version 1 gives the result of
version 1.

**Round players.** `round_1m` and `round_5m` buy only each 1 or 5 minutes: 1 purchase round of 1 s, then 1 or 5
minutes of open game with no input, again and again. The game stays open, so the away income runs at `open_rate`
with no active-only effect. Name them in `--players`; `all` also runs them.

**End and length.** The simulator records the time of the hard end (`game.targets.hard_end`) in `Sim.end_real`
for each kind. A hard end of kind `resets` or `milestone` (`pw_sim.STOP_KINDS`) stops each player, and the `end` of
a player row is its time. A hard end of kind `axis_step` or `purchase` does not stop a player: the player plays to
the end of the axis, because the solver can put the hard end before the last stage. No player resets after
`prestige.max_resets`. A player plays at most 3 x the length, so that a slow player can reach the end. `no_input`
never ends, so it plays the length only.

**Purchase rate.** The band row "purchases by hand in the busiest minute (active_greedy)" compares the busiest
minute of active_greedy with `game.targets.purchase_rate_max_per_minute` (default 30). A purchase of the auto-buy
does not count. Proof row PC6 gives the same check with the automation plan.

## Output keys of the design solver

The rebalance mode of `scripts/pw_design.py` (`--content`) writes the keys `field_map`, `rules`, `caps_reduced`,
`identity` and `character`. Each design with S24 in the selection also gets `automation`. The simulator reads `automation` only:
the auto-buy buys each affordable level of the line from `from_minutes` (minutes of play), the cheapest level
first, also while the game is open and the player is idle (not while the game is closed). The player does not buy
the line by hand after that minute. Each purchase of the auto-buy is an event with the key `auto: true`. An entry with
`requires` (the rebalance writes a lifetime total of the main currency) opens at that progress instead, and stays
open; `from_minutes` then is only the minute that the report gives.

| Key | Content |
| --- | --- |
| `field_map` | `{model id: {"kind", "game_id", "place", "fields"}}`. `kind` is the kind of game data that holds the id: `line`, `reward`, `side system`, `stage reward`, `stage rule`, `reset rule`, `currency`, `bank line` or `bank`. `game_id` is the id in the game, or null for a new entry. `fields` names the game fields to set. `stat` gives the game stat of each effect and the model target, as `grow_power -> coin`. `unlock` gives the unlock kind and the game field: `lifetime total: <field>`, `count of resets: <field>`, `stage: <field>`, `milestone: opens with the element <id>` or `time: <field>` |
| `rules` | The rules that the game copies: `reset_gain_rounding`, `stage_after_reset`, `stage_rule_income` and `reset_opens` |
| `caps_reduced` | 1 entry for each line whose cap fell below the cap of the game: `id`, `game_cap` (null for no cap), `cap`, `reason`. The proof sheet gives each 1 warning row `cap reduced: <id>` |
| `identity` | `{"pairs", "findings"}`. Each pair is 2 lines of the same kind: `a`, `b`, `kind`, `differs` (the traits that differ: `step`, `first cost`, `cost ratio`, `cap`, `stat`, `unlock`, `currency`) and `same_in_game` (true when the 2 lines have the same shape in the game). `findings` names each pair whose shape (step, cost ratio, cap, stat) does not differ, unless it has the same shape in the game |
| `character` | The measure "character kept" (proof row CK): `kept`, `moved`, `changed`, `total`, `share` (kept / total), `verdict` (`ready`, or `not ready` when more than half are changed), `reason`, and `elements` (1 entry for each element: `id`, `kind`, `state`, `fields`, each field with `field`, `game`, `design`, `state` and `note`) |
| `automation` | 1 entry for each line with auto-buy (S24): `line`, `from_minutes`, `purchases_in_busy_minutes`, `reason`, and in the rebalance `requires`. The simulator buys the line from that minute (S24), and those purchases do not count for PC6 |

## simulation_path

The simulation path (`references/simulation-path.md`) writes the key `simulation_path`. The proof sheet reads it, and
`scripts/pw_players.py` leaves out the reset players of a model with this key and no prestige. The simulator ignores it.

| Key | Content |
| --- | --- |
| `parts` | 1 entry for each selected system and each part written by hand: `system` (null for a part with no catalog card), `variant`, `name`, `mark` (`law`, `simulated` or `not written`), `fill` (true for a stub that a person must write), `entries` (the model ids of the part), `reason`, and for a stub `to_fill` and `defaults` |
| `hand_set` | 1 entry for each number set with no source: `name` and `path` (the keys and indexes from the model root to the number). `pw_scaffold.py --vary` changes each by -20% and +20% |
| `to_add` | 1 text for each stub that a person must still write |
| `fit` | The record of the timing fit: `hard_end_target_h`, `hard_end_sim_h`, `passes`, `rows` (the target and the simulated minute of the first purchase and of each stage), and `flat_stages` (the stages before which the income does not rise) |

## upkeep

The key `upkeep` holds the card S30 (`references/systems-catalog.md`): an uptime factor `u` between 0 and 1 on the
income of 1 currency. The key is optional. The simulator multiplies the income of `target` by
`u = max(floor, min(1, C / N)^exponent)`, and `u = 1` when `N` is 0.

```json
"upkeep": {"system": "s30", "target": "coin", "rule": "need_capacity", "exponent": 1, "floor": 0,
           "base_need": 0, "base_capacity": 3,
           "need": [{"purchase": "gen", "per_level": 1}, {"purchase": "m1", "per_level": 1}],
           "capacity": [{"purchase": "care", "kind": "mult", "value": 1.2}],
           "band": [0.69, 1.0], "min_switches": 4}
```

| Key | Content |
| --- | --- |
| `target` | The currency whose income `u` multiplies |
| `rule` | `need_capacity`, the only rule now |
| `exponent`, `floor` | `e` (default 1) and the lowest `u` (default 0, at most 1) |
| `base_need`, `base_capacity` | `N0` and `C0` |
| `need` | `N = N0 + sum of per_level x level` of each named purchase line (the producer lines) |
| `capacity` | Each care line: `kind` `mult` multiplies `C` by `value` for each level, `add` adds `value` for each level. A care line has no income effect: `effects` is empty |
| `band` | `[low, high]` of `u` for the row PU1 |
| `min_switches` | The row PU4: the least count of switches between producer and care purchases (default 4) |

The buyer values a care level by the income gain from the rise of `u`. When `u` is 1, it values a care level by the
gain that it frees for the best open producer line. Each event of the simulator has the key `u`. The proof sheet adds
the rows PU1 to PU4 (`references/balance-math.md` section 3b). PV of a need line leaves out the change of `u`.

## interactions

`{"from": "dive", "to": "depth", "type": "buff|gate|resource|unlock", "effect": "text"}`

The script derives links from costs, effects, grants, requirements, and the prestige basis.
Declare a link here only when the model cannot show it (a design intent, or a mechanic that
is not modelled).

## bands

Overrides for the Dashboard bands, each `[low, high]`: `first_purchase`, `first_system`,
`first_prestige`, `beats_per_hour`, `beats_first_hour`, `lulls`, `gap_ratio`, `dry_share`,
`options`, `no_option`, `links_per_system`.

## review

The script writes these onto the sheets. Fill them after the first run, then run again.

```json
{"findings": ["Text for the Dashboard, one finding in each string."],
 "lull_notes": [{"lull_at_hours": 2.9, "diagnosis": "text", "fix": "text"}],
 "ideas": [{"idea": "", "archetype": "", "problem": "", "feeds": "", "fed_by": "", "unlock": "",
            "persists": "", "scope": "", "juice": "", "risk": ""}]}
```

`lull_at_hours` is the start of the lull. A note goes to the nearest lull that starts within
0.25 hours, or within 10% of the start time if that is more. Each note is used one time. A
note with no lull that near is not written.

## Command line

```
python3 progression_workbook.py model.json out.xlsx [--hours H] [--analysis PATH]
        [--profile eager|patient|doubler|model] [--compare BEFORE.analysis.json] [--no-sensitivity]
        [--beat-weight 3] [--merge-seconds 30] [--lull-ratio 1.0] [--max-gap-sessions 2]
        [--late-ratio 0.25] [--min-lull-minutes 10] [--ratio-floor-minutes 10]
        [--target-first-gap 0.5] [--target-stretch 1.12] [--players all|fast|none|NAME,NAME]
```

- `--players`: the players to run (see "Players"). `all`: each player, and the speed test at
  9 speeds. `fast`: active_greedy, open_8h, closed_24h, early_reset, patient_reset, and the
  speed test at x10 and x100. A model with `game.play_pattern` `continuous` leaves out each
  `closed_*` player and each closed-idle band, because the game has no offline gain by design.
  The script prints 1 line `LEFT OUT:` that states it, the Presence sheet says it in its note, and
  the `presence` block gets the keys `left_out` and `left_out_note`. A comma list of names always adds active_greedy. Default: `fast`
  when the model has `game.presence` or `game.targets`, else `none`. With players, the workbook
  gets the Players and Presence sheets and a block of band rows on the Dashboard, and the
  analysis JSON gets 2 keys: `players` (1 row for each player) and `presence` (`mode`,
  `players_run`, `idle_curve`, `growth_slope`, `policy_gap`, `speed_test`, `never_bought`,
  `unbounded`, `bands`).

- `--compare`: the analysis JSON of an earlier run (the "before"). The Compare sheet then
  shows each indicator before and after, and one chart of cumulative beats for both runs. Use
  the same thresholds and the same profile for both runs.
- `--no-sensitivity`: skip the Sensitivity sheet. It re-runs the primary profile with one
  number changed at a time (ladder growth, prestige exponent, cost growth of each line, x0.95
  and x1.05), within a time limit of about 6 seconds.
- A beat is an event at the beat weight or more. Beats closer than the merge seconds are one
  moment. A gap is a lull when it is at least the min lull minutes and (gap / time played
  before it is above the lull ratio, or the gap is longer than both the max gap in sessions
  and the late ratio x time played).

The threshold options are also yellow cells on the Params sheet. Each result is in the
workbook two times: "at build" (a value of the script run, readable with no recalculation)
and "live" (a formula that follows Params).

## Common modelling moves

| Game mechanic | Model |
| --- | --- |
| Tap or click income | An `add` effect with `"active": true`, and `active_fraction` |
| Enemy or boss with HP | A ladder with a `rate` requirement on a damage stat |
| Random drop, 1 in N for each action | An `add` income of (actions each second) / N on a drop currency |
| Skill tree | One purchase line for each node, with `requires` of type `level` on the parent node |
| Timed minigame that pays each T seconds | An `add` income of reward / T, opened by a milestone |
| Unlock bought for a one-time price | A purchase with `max_level` 1 and no effects, plus `requires` of type `level` on what it opens |
| Readiness check before new content | A milestone with several requirements |
| Cap raiser (a later system lifts the max level of an old line) | A purchase, milestone, or ladder with an effect of kind `cap` on the old line: `{"target": "bellows", "kind": "cap", "value": 3}` |
| Shared sink (one purchase eats two currencies) | `costs` with one entry for each currency |
| Bonuses that add inside a system and multiply between systems | `pct` effects with `game.pct_grouping: "system"`, or a `group` name on each effect |
| Upkeep: each unit adds a need, care meets it, and output falls when the need is above the care (S30) | The key `upkeep` with the producer lines in `need` and the care lines in `capacity` |
| Removable debuff | A milestone with a `mult` below 1 from the start (no requirements), and a second milestone or purchase with the inverse `mult` |
| Run-based game (a run ends by a timer or by death, and pays by how far it got) | `prestige.run_seconds` = the mean run length, and `prestige.basis_ladder` = the spine ladder |
| Idle game that is played in short sessions | `game.away_minutes` and `game.offline_rate` for the version 1 run, and `game.presence` for the players |
| Offline cap | `game.presence.offline_cap_minutes`. The players use it. The version 1 run (`away_minutes`) does not: state that in the assumptions |
| A store or a bank that fills while away | `bank_cap` on the currency |
| Income that needs the player at the screen | `"active": true` on the effect |
| A fight or a step that takes time to try | `attempt_seconds` or `cooldown_seconds` on the ladder, and `needs_visit` if the player must be present |
| Auto-prestige | `"needs_visit": false` in `prestige` |
| A timer or a source that follows the wall clock | `"clock": "real"` on the entry or on its system |
| A milestone bonus that the reset takes away | `"persists": false` on the milestone |
