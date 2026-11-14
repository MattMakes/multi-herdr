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
  "ladders": [], "prestige": {}, "interactions": [], "bands": {}, "review": {}
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
| `offline_rate` | Share of income that runs while the player is away (0 to 1) | 0 |
| `assumptions` | List of strings: each number that is a guess, and each mechanic that the model simplifies | none |

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

Income for a currency = (`base_income` + sum of `add`) x product over pools of (1 + sum of
`pct` in the pool) x product of `mult`. With the default `pct_grouping` there is one pool.

## systems

| Field | Meaning |
| --- | --- |
| `id`, `name`, `description` | |
| `loop` | `core`, `meta`, `minigame`, `collection`, `automation`, or `event` |
| `persists` | `true` if it survives the reset. Default: true for meta and collection. The script warns when this field says `true` and `prestige.resets.purchase_systems` names the system |
| `clock` | `game` or `real`. Documentation only: it marks a system with its own pace |

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

A line whose cost is 0 at each level and that has no `max_level` is an error. A cost that does
not grow with no `max_level` gives a warning: the buyer can buy it without end.

A purchase with no effects is a pure unlock: use `requires` on other entries to make it
matter. The buyer gives it full value only while a purchase, a milestone, or the prestige
still needs it. If nothing needs it, the buyer buys it late.

## effects

`{"target": "<currency id or all>", "kind": "add|pct|mult", "value": 1.5, "active": false, "group": "optional pool name"}`

`{"target": "<purchase id>", "kind": "cap", "value": 5}`

- `add`: + value each second for each level.
- `pct`: + value to an additive bonus pool for each level (0.1 = +10%). Bonuses in one pool
  add together. Pools multiply with each other. The pool is `group` when the effect sets it.
  Otherwise it is the one pool of the currency (`pct_grouping: "currency"`) or the system id
  of the source (`pct_grouping: "system"`).
- `mult`: x value for each level. It compounds. A value below 1 is a debuff.
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
| `resets` | The currencies set to zero, the systems whose purchase levels go to zero, and the ladders that restart. A repeat ladder step is logged with weight 1 and pays no grant | none |

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
        [--target-first-gap 0.5] [--target-stretch 1.12]
```

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
| Removable debuff | A milestone with a `mult` below 1 from the start (no requirements), and a second milestone or purchase with the inverse `mult` |
| Run-based game (a run ends by a timer or by death, and pays by how far it got) | `prestige.run_seconds` = the mean run length, and `prestige.basis_ladder` = the spine ladder |
| Idle game that is played in short sessions | `game.away_minutes` and `game.offline_rate` |
| Offline cap, or a bank that fills while away | Not modelled. `offline_rate` has no cap. State it in the assumptions |
| A milestone bonus that the reset takes away | `"persists": false` on the milestone |
