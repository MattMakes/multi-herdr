# Contract template: the diag hub, its data file, its runtime API, the trace and the policy

Copy this file into the new game's plan folder as its `diag-contract.md`. Replace every `<...>`. The game side, the
bot and the report all work to this contract, so they can be built in parallel. Keep the field names, or record each
change in the contract: the bundled `report.py` (`script-report.md`; copy the block into a file) reads the trace by these names. The group and chapter field
names are configurable (section 2).

## 1. Terms

- **Diag mode**: the game runs with the diagnostic hub in place of the shipped tree. A flag turns it on
  (`--diag-tree` or `<ENV>_DIAG_TREE=1`). It is off by default and never ships.
- **Spoke**: 1 upgrade path. Kinds: `stat` (levels with no end), `special` (1 level: a unique verb or unlock),
  `ability` (free at its unlock event, as a fixed spike).
- **Group**: the class, vehicle or character that a spoke belongs to; "" = shared by all.
- **Counted spokes**: the open `stat` spokes that are shared, plus those of the group in use.
- **Cap**: `cap_step x tier`; the tier starts at 1 and never falls. While at least `ceil(cap_share x counted)` counted
  spokes are at the cap (or closed), the tier rises by 1.
- **Closed spoke**: 1 more level changes no derived stat (a game cap holds it). It cannot be bought and counts as at
  the cap.
- **N**: the levels bought in the hub (stat and special; abilities do not count).
- **Pool**: the sum of all currency balances under the pool rule (`<1:1 | rates>`), spent in `spend_order`.
- **Stage**: the last unlocked group in the game's unlock order; it selects `stage_x`.

## 2. The data file `<data dir>/diag_tree.json` (generated)

A generator writes it from the shipped tree data. It keeps an existing `price` block (the calibration owns it).

```json
{
  "version": 1,
  "cap_step": 5,
  "cap_share": 0.8,
  "spend_order": ["<rarest>", "...", "<commonest>"],
  "price": {"base": 3.0, "growth": 1.02, "stage_x": {"<group 1>": 1.0, "<group 2>": 10.0}},
  "spokes": [
    {"id": "speed", "name": "Speed", "kind": "stat", "stat": "speed", "step": 0.04, "group": "", "opens_with": ""},
    {"id": "<verb>", "name": "<name>", "kind": "special", "row": "<shipped row id>", "group": "<group>",
     "opens_with": ""},
    {"id": "<ability>", "name": "<name>", "kind": "ability", "row": "<shipped row id>", "boss": "<unlock event id>",
     "group": "", "opens_with": ""}
  ]
}
```

The spoke field `group` and the run field `group` (section 4) hold the group. The run field `chapter` holds the
chapter. If your game already uses other names (for example `hero` and `zone`), pick 1 of 2 options and record it here:

- Pass the names to the tools: `report.py --group-key hero --chapter-key zone` and `make_policies.py --group-key hero`.
- Or write a small adapter that renames the fields into a copy of each trace before the report runs.

The flags keep the game's own vocabulary in its files. The adapter is the better choice when more fields differ.

Generator rules to decide per game (write them down in the contract):
1. Which shipped rows count (every row that some tree lists, plus shared passives).
2. Which rows are abilities (free at an unlock event).
3. Shared stats: the stats of the starting group's tree and of shared passives. Merge stats that the pool makes the
   same (for example per-currency value multipliers become 1 `haul` spoke).
4. Stats that a later group unlocks for everyone (`opens_with`).
5. Group stats: a stat that only 1 group's rows have; 2 or more levels in total make a `stat` spoke, exactly 1 level
   makes each row a `special`.
6. `step`: the median per-level value of the shipped rows with that stat.
7. Order: shared spokes first (in shipped order), then the late shared ones, then each group's, then abilities.

## 3. The runtime API (1 global object, `Diag`)

```text
signal bought(spoke_id, level, price, paid)       signal tier_changed(tier)
active: bool                                      policy_info: dict   ({"name": "human"} by default)
spokes() -> list        is_open(id) -> bool       is_closed(id) -> bool     level(id) -> int
tier() -> int           cap() -> int              total_levels() -> int     price() -> int
pool() -> float         can_buy(id) -> bool       buy(id) -> bool
effective_levels(group) -> dict                   extra_nodes() -> list     reset()     set_paths(state, trace)
```

- `price() = ceil(base x growth^N x stage_x[stage])`.
- `buy` spends in `spend_order`, raises the level and N, re-checks the tier, saves the state, writes the trace, emits
  `bought`, and tells the shop UI to redraw.
- The stat pipeline hook: in diag mode, the stats of a run come from `effective_levels(group)` (each stat spoke as a
  synthetic row `diag_<id>` with value = step) instead of the shipped levels. Everything after (caps, derived stats,
  biome or zone factors) stays the game's own code.
- The shipped shop refuses every buy in diag mode (1 guard line).
- Save: a human start uses its own save file; a tool start uses the tool's scratch save. The hub state file sits next
  to the save (`<save>.diag.json`: levels, tier, N, trace path). An explicit trace path (argument or environment) wins
  over the saved one.
- Every event handler checks `active` first.

## 4. The trace (schema `diag-trace/1`), 1 JSON file per playthrough

```json
{
  "schema": "diag-trace/1",
  "meta": {"started_utc": "", "policy": {"name": "balanced", "kind": "weights", "skip": [], "baseline": "",
           "seed": 0, "skill": "strong"},
           "slot": "", "run_seed": 0, "build": "",
           "data": {"version": 1, "cap_step": 5, "cap_share": 0.8, "spend_order": [], "price": {}},
           "spokes": []},
  "runs": [
    {"run": 1, "group": "<group>", "field": "<level id>", "chapter": "<chapter id>", "end": "<reason>",
     "run_s": 10.8, "game_s": 10.8, "wall_s": 3.2,
     "bosses": [{"id": "", "kind": "mini|major", "result": "killed|left|run_ended", "reason": "",
                 "health_left": 0.0, "first_kill": true}],
     "bosses_beaten": [], "new_field": true,
     "haul": {}, "haul_pool": 0.0, "bank_before": {},
     "purchases": [{"n": 1, "spoke": "health", "level": 1, "price": 3, "paid": {}, "tier": 1, "cap": 5}],
     "bank_after": {}, "tier": 1, "cap": 5, "levels": {}, "events": [],
     "stats": {"power": 1.0}}
  ]
}
```

- A run row is made at the run end; the buys after it (until the next run starts) join it. A restart makes the last
  finished row current again, so its buys are not lost.
- `bank_before` is the bank after the haul is banked, before the shop. Every currency of `spend_order` is a key of
  `haul`, `bank_before`, `bank_after` and `paid`.
- `events` kinds: `tier_up`, `spoke_opened`, `spoke_closed`, `group_unlocked`, `ability` (first unlock only).
- `stats`: 5 to 10 derived stats that explain power and economy (power, health, speed, width or range, ...).
- Rewrite the file through a temporary file and a rename. Cast counts back to integers after a reload.

## 5. The policy file

```json
{"name": "fight", "kind": "weights", "weights": {"dmg_pct": 3}, "default_weight": 1,
 "skip": [], "baseline": "", "specials": "early", "seed": 0}
```

- `weights`: buy the buyable spoke with the lowest `(level + 1) / weight`; a tie goes to the higher weight, then to
  the data order. Weight 0 = never buy.
- `order`: buy the first buyable spoke of `order`; the rest after, in data order.
- `random`: a seeded random pick among the buyable spokes, by weight. Save the generator state between processes.
- `skip`: never buy (leave-one-out). `baseline`: the policy this one is compared with.
- `specials`: "early" (as soon as buyable), "late" (only when no stat spoke is buyable), "never".
- The bot spends until nothing is buyable after every run (players are told to spend each round).
