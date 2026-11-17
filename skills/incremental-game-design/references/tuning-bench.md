# The tuning bench

The tuning bench is a panel in the Godot game. The designer moves controls and reads meters. A simulation of the
real game code fills the meters in seconds, so the designer does not play the game to see a result.

Use it in 2 cases: a new game that the skill designed (the build installs the bench), and an existing game (the
skill installs the bench and finds each number).

Contents:

1. Why the bench simulates the game code and not the model
2. The files
3. The procedure
4. The number ledger (`numbers`): no number is missed
5. Roles: each number folds into the same meters
6. The adapter contract
7. `bench.json`
8. Controls (knobs)
9. Players
10. Meters
11. The command line of the bench (headless)
12. The self-check
13. `scripts/pw_bench.py`
14. Limits

## 1. Why the bench simulates the game code and not the model

A model is a second copy of the math. A second copy drifts from the game. The bench has no second copy: a bot buys
through the real economy code of the game, with a clock that the bot moves. The numbers on the meters are thus the
numbers of the game that ships.

The solver and the proof sheet give the first set of numbers. The bench is the place where the designer makes the
last decisions. A bench setting that the designer saves is the decision of the designer: do not "correct" it with
the solver.

## 2. The files

The skill copies the generic part and writes the game part.

| Path in the game | Owner | Content |
| --- | --- | --- |
| `addons/tuning_bench/` | skill, generic, do not edit in the game | autoload `TuningBench`, the bot, the meters, the panel, the command line, the self-check |
| `bench/bench.json` | game | the number ledger, the phases, the controls, the players, the targets |
| `core/game_core.gd` | game | the pure core of the game (`references/game-core.md`). With it the bench needs no adapter |
| `bench/adapter.gd` | game | only for a game with no pure core: the adapter that drives the existing code |
| `bench/settings/*.json` | designer | saved settings (the value k of each control) |

The source of the generic part is `assets/godot/tuning_bench/`. A worked game with its adapter is
`assets/godot/sample_game/`.

## 3. The procedure

The order matters: the phases come from a first run, and the controls come from the confirmed classes.

1. **Install.** `pw_bench.py install <project>` copies the addon and adds the autoload `TuningBench`. It selects a
   free hotkey (F2, F9, F10 or F11) and prints it.
2. **Connect the game.** A game with a pure core (`references/game-core.md`) names it with the key `core` of
   `bench.json`. A game with no pure core needs an adapter (section 6): copy the shape of
   `assets/godot/sample_game/bench/legacy_adapter.gd`. Do the side-effect list of section 6 before the first run.
3. **Phases and players.** `pw_bench.py init <project> --phases-only` writes the phases, the players, the targets
   and `sources`. It writes no control.
4. **First run.** `pw_bench.py run <project>` shows when things occur. Give the phases the words and the limits of
   the game (section 7). Compare the length with what the designer knows of the game.
5. **Inventory.** `pw_bench.py inventory <project>` writes each number of the project into the ledger (section 4).
   The adapter is in the ledger too.
6. **Classify.** Give each number a class, and each balance number a role (section 5), an element and a phase. The
   tool proposes; you confirm in the code. The gate is 0 numbers with the class `unclassified`.
7. **Lift.** Move each balance number that is in code into a data file, or record why it stays (`tunable: false`
   with a `reason`). The panel shows the count of balance numbers that are not tunable.
8. **Controls.** `pw_bench.py init <project>` writes the default controls from the confirmed numbers. After a later
   change of a class or a role: `pw_bench.py init <project> --refresh-controls`. Then `pw_bench.py check <project>`
   must exit 0.
9. **Self-check.** `pw_bench.py selfcheck <project>` must exit 0 (section 12). It proves that each tunable number
   changes the simulation, so a number that is in the ledger and not wired cannot hide. Do not hide a dead number
   to get exit 0: a dead number with no good reason is a finding for the designer.
10. **Run.** `pw_bench.py run <project>` prints the meters. In the game, the hotkey of `bench.json` opens the panel.
11. **After each change of the game**, run steps 5, 6, 8 and 9 again. The inventory reports each new number and
    each number that went. A new number has the class `unclassified`, so the gate fails until you class it. For a
    small change, `selfcheck --only <glob>` checks only the numbers that match.

To state "each number is reviewed", `pw_bench.py check <project> --strict` must exit 0: no open proposal and no
open text with a number (section 4). Without `--strict`, `check`
prints the line "not reviewed: N numbers in M files", and the panel shows the same line. Give that line to the
designer; do not drop it.

For a new game, the build specification must contain these steps and the 3 layers of `references/game-core.md`:
a pure core with all the math, a session that owns the state, and a view that sends intents and animates on
events. Each balance number is in a data file. `pw_bench.py corecheck` must exit 0.

## 4. The number ledger (`numbers`): no number is missed

The inventory is mechanical. It does not select what looks like balance. It writes:

- each numeric leaf of each data file (`.json`, `.tres`, `.cfg`, `.csv`), each scene (`.tscn`) and
  `project.godot`, with a pointer;
- each numeric literal in each `.gd` and `.gdshader` file, with the file, the line and the text of the line.

It skips only: `addons/`, `.godot/`, `bench/settings/`, the JSON files of the bench in `bench/`, a folder whose
name starts with a point, a folder with a `.gdignore`, and each path of the key `skip` of `bench.json`. It reads
`bench/adapter.gd`. It prints each skip and the count of each kind.

Tests, tools and copies of a model are not the game. Name them in `skip` (for example `"skip": ["tests/",
"tools/", "docs/"]`). The inventory prints a hint when more than 25 percent of the numbers are in such folders. It
does not skip them by itself.

An id of a number can move (a line number, an index of an array). The inventory follows a number by its line text
or by the id of its JSON object (the key `anchor`), keeps its class, and reports it as moved.

| Class | Meaning | In a control |
| --- | --- | --- |
| `balance` | The number changes how fast or how far the player goes | yes (or `tunable: false` with a reason) |
| `structure` | A count or an index that the code or the content depends on (a count of slots, a stage id) | no |
| `presentation` | Layout, colour, sound, animation time | no |
| `engine` | A literal with no game meaning (an index 0, an increment 1, a unit conversion) | no |
| `unclassified` | Not decided yet | the gate fails |

The tool proposes a class with simple rules (the key name, the literal, the place in the code). A proposal has
`"proposed": true` and the name of its rule. A proposal is not a decision. Read the file, then confirm:

```bash
python3 <skill_dir>/scripts/pw_bench.py confirm <project> [--file <glob>] [--class <class>] [--rule <rule>] [--id <id> ...] [--set-class <class>] [--role <role>] [--element <e>] [--phase <p>] [--sense 1|-1] [--all] [--from <rules.json>]
```

At least 1 of `--file`, `--class`, `--rule`, `--id` is necessary. The command prints each number that it confirms,
with the text of its line. A number with `"hint": "threshold"` is a literal in a comparison: look at it first.
A balance number in a script, a scene or `project.godot` gets `tunable: false` until it is in a data file. The
inventory prints each `.cs` file as "not read": the tool does not read C# code.

`check` fails while a proposal of any class is not confirmed in a data file of `sources`, in a script of the core
or the adapter, or in an autoload script. These are the files where a wrong `engine` hides a balance number. In
other files an open proposal is a note, and it counts in the line "not reviewed: N numbers in M files". With
`check --strict` each open proposal in any file fails. When you are not sure, use `balance`: the self-check then
shows if the number changes the game. The key `words` of `bench.json` adds words of the game to the rules
(`{"balance": ["excitement"], "presentation": [...]}`).

**Many numbers at one time.** For the data files, write a rules file and run `confirm --from <rules.json>`:

```json
[{"match": {"file": "data/model.json", "pointer": "/purchases/*/requires/*/level"},
  "set": {"class": "balance", "role": "unlock_at", "element": "{owner}", "phase": "p2", "label": "{owner}: needs {2}"}},
 {"match": {"key": "autosave_interval_seconds"}, "set": {"class": "engine"}}]
```

`match` has `file` (glob), `pointer` (`*` is 1 part, `**` is any count of parts), `key`, `class` and `role`. `set`
has `class`, `role`, `element`, `phase`, `label`, `sense`, `tunable`, `reason` and `dead_ok`. `{owner}` is the id of
the item that owns the number; `{1}`, `{2}` are the parts that `*` matched. The first rule that matches wins. The
command prints the count for each rule, and each number that no rule matched. Keep the rules file in `bench/`: it
is the record of the decisions.

A literal that is the default of `.get("<key>", <literal>)` is a fallback copy of a data number: the tool proposes
the class of that number, with `tunable: false`. A number of a data file for a tool or a note (an autosave time,
a test tolerance, a target of the design) is `engine`.

**Texts with a number (`texts`).** A text such as `"+25% pot value per level"` or the item name `Bulk orders
(shop level 7)` holds a copy of a balance number. A control does not change it, so the game then shows an old
value. The inventory lists each such text of a script, a scene, a resource and a data file (`.json`, `.csv`,
`.cfg`). For each one: build the text from the data at the time of the draw, or accept it with a reason
(`confirm --text-id <id or glob> --ok "<reason>"`, or `--text-file <glob>`). A count of content, such as "Glaze
Shelf (12)", is a good reason. `check` prints "texts with a number: N (M accepted)"; `check --strict` fails on
each text with no `ok`. The list leaves out format strings, paths, colours and names with no space. A short label
such as `Level5` is thus not in it.

A formula is not a number. List each formula of the economy (cost, income, reset gain, wall) in `formulas` with its
file and line, so that the designer sees the shape that the numbers go into. The bench does not change a formula.

## 5. Roles: each number folds into the same meters

A game can have any system. Each balance number gets 1 role from this fixed list. The controls and the hints of
the meters work on roles, so a new system needs no new meter.

| Role | The number is |
| --- | --- |
| `cost_base` | the first price of a thing |
| `cost_growth` | the growth of a price for each level (a ratio above 1) |
| `effect` | an amount that a purchase gives (income for each second, a flat bonus) |
| `effect_factor` | a factor that a purchase multiplies income by (1.21 for each level, 1.1 once) |
| `effect_growth` | the growth of an effect for each level or count step |
| `effect_exponent` | an exponent on an effect |
| `base_income` | income with no purchase |
| `active_power` | the size of the manual action (a click, a boost factor) |
| `active_time` | a time of the manual action (a window, a cooldown) |
| `wall` | what the player needs to pass a stage, an era or a gate |
| `wall_growth` | the growth of a wall from step to step |
| `unlock_at` | the threshold that shows new content |
| `reset_need` | what the player needs before a reset |
| `reset_gain` | the size of the gain of a reset (a divisor, an exponent, a factor) |
| `reset_bonus` | the strength of what the reset currency gives |
| `cap` | a maximum level or a maximum amount |
| `timer` | a game time that is not the manual action (a period, a duration) |
| `rate` | the rate of a side system or a second income |
| `auto_at` | when automation starts |
| `offline` | an offline or idle rate or cap |
| `reward` | a one-time payment (a milestone, a stage reward) |
| `reward_growth` | the growth of a reward from step to step |
| `end_at` | where the game ends (the last stage, the last level) |
| `other` | balance with no role above; write the meaning in `label` |

A factor is not an amount. A control multiplies an amount by k. On a factor of 1.1, k = 0.5 gives 0.55, and the
purchase then halves the income. Thus a factor gets `effect_factor`, and a control changes only its part above 1.

A number can have `"sense": -1`: a larger value gives less (a divisor). A control then applies 1 / k to it, so a
larger k has 1 direction for each number of the control. The inventory proposes it for a clear divisor name.

Each balance number also has an `element` (the thing it belongs to, for example an upgrade id) and a `phase`
(section 7). The value `"phase": "all"` is permitted for a number of the whole game.

## 6. The adapter contract

A game with the core contract of `references/game-core.md` section 3 needs no adapter: the addon wraps the core
(`core_adapter.gd`), and `isolated()` is true. This section is for a game with no pure core.

`bench/adapter.gd` extends `BenchAdapter` (`addons/tuning_bench/bench_adapter.gd`). All times are game seconds.

| Method | Required | Meaning |
| --- | --- | --- |
| `setup(data: Dictionary) -> void` | yes | Start a new game from the start. `data` maps each file of `sources` to its parsed content with the controls applied. Read no save file. Write no save file |
| `step(dt: float) -> void` | yes | Advance the economy by `dt` seconds. No frame, no real clock |
| `actions() -> Array` | yes | Each thing the player can do or see now (below) |
| `do_action(id: String) -> bool` | yes | Do 1 action. Return false when the game refuses it |
| `rate() -> float` | yes | Main income for each second now, with no manual action |
| `progress() -> float` | yes | 0 at the start, 1 when the game is complete |
| `max_step() -> float` | no | The largest correct `dt`. Default 1.0 |
| `wealth() -> float` | no | The amount of the main currency. Lets the bot skip checks while nothing is affordable |
| `active(seconds: float) -> void` | no | The player does the manual action for this time |
| `away(seconds: float) -> void` | no | The player closes the game for this time, and the game applies its offline rule |
| `events() -> Array` | no | Things that occurred with no action of the bot since the last call: `{"kind": "unlock" or "advance" or "reward" or "bought", "id": ..., "label": ...}` |
| `apply_live(data: Dictionary) -> bool` | no | Push the values into the game that runs. Without it the panel says "restart to apply" |
| `isolated() -> bool` | no | True when `setup` touches no autoload and no scene. Then the panel can run the simulation on a thread |

An action is a dictionary:

| Key | Meaning |
| --- | --- |
| `id` | stable id |
| `kind` | `buy` (a purchase that can repeat), `once` (a purchase with 1 level), `advance` (pass a wall), `reset`, `other` |
| `label` | name for the panel |
| `cost` | price now, as a number; 0 when free |
| `affordable` | true when `do_action` can succeed now |
| `element` | optional: the `element` of the ledger |
| `gain` | optional, for `reset`: what the reset gives now divided by what the player owns (1.0 = it doubles) |
| `bot` | optional: `false` for a free choice that is always possible (a selector, a switch). The bot never does it and the meters do not count it. Without it the bot repeats the action until the guard stops the run |
| `currency` | optional: the name of the currency of `cost`. Absent = the main currency. The bot compares costs of 1 currency only |

**A purchase that the game makes by itself is an event.** Most incremental games have an auto-buy. Connect the
purchase signal of the game: each purchase that the bot did not ask for with `do_action` is
`{"kind": "bought", "id": <action id>, "count": n}`. Without it, the meters count hand purchases only, and
automation looks like a long wait. A core returns the same event from `step`.

**The side-effect list.** An adapter that drives autoloads runs the whole game in the simulation, with each side
effect. Before the first run, list each autoload of `project.godot`. For each one that writes a file, reads a
setting or reacts to an economy signal, switch it off in `setup()`: stop its autosave, disconnect its handlers,
stop its `_process`. A save on a signal overwrites the real save file of the designer. The self-check row "No file
written" names each file of `user://` that the simulation made or changed. It does not see a write in `_ready`
before the first frame, or a setting that an autoload reads: only the list finds these.

**`active(seconds)`.** The runner calls it 1 time after the step of each decision, with `decision_s x
active_share`. Two examples:

- A click gives income: add `clicks for each second x seconds x click value`.
- A hold gives more income while the button is down: add `seconds` to a hold credit, and let `step` use the
  credit. The hold of the bot is thus 1 decision late.
- A press gives a bonus for 10 s: keep a credit. Add `seconds` to the credit. When the bonus is off and the credit
  is at least 10, press and subtract 10. With a share of 0.8 the bonus is then on for 80 percent of the time.

The bench finds an unlock by itself: the first time that an action id appears after the start is "new content".
The adapter adds other content events (a new screen, a new resource) with `events()`.

The simulation must not change the game that the designer plays. In the default mode the panel starts a second
Godot process with the command line of section 11, so an economy that is an autoload singleton is safe. With
`isolated()` true the panel can use a thread, which is faster.

## 7. `bench.json`

```json
{
  "format": "tuning-bench",
  "version": 1,
  "game": "Sample Game",
  "core": "res://core/game_core.gd",
  "session": "Session",
  "hotkey": "F2",
  "sources": ["res://data/items.json"],
  "numbers": [
    {"id": "data/items.json#/upgrades/2/cost", "file": "res://data/items.json", "pointer": "/upgrades/2/cost",
     "value": 250, "class": "balance", "role": "cost_base", "element": "big_lamp", "phase": "p1",
     "label": "Big lamp: first price"},
    {"id": "systems/economy.gd:41:1", "file": "res://systems/economy.gd", "line": 41, "index": 1,
     "text": "var tick_bonus := 0.05", "value": 0.05, "class": "balance", "role": "effect", "element": "tick",
     "phase": "all", "tunable": false, "reason": "in code; lift to data"}
  ],
  "formulas": [{"name": "price", "file": "res://systems/economy.gd", "line": 88, "text": "base * growth ** level"}],
  "phases": [
    {"id": "p1", "name": "Opening", "until": {"progress": 0.15}, "share": 0.1},
    {"id": "p2", "name": "Middle", "until": {"progress": 0.7}, "share": 0.5},
    {"id": "p3", "name": "End", "until": {"progress": 1.0}, "share": 0.4}
  ],
  "controls": [
    {"id": "all_cost", "group": "Whole game", "name": "All prices", "op": "mul", "min": 0.25, "max": 4.0,
     "select": {"role": ["cost_base"]}},
    {"id": "p1_cost", "group": "Phase: Opening", "name": "Opening: prices", "op": "mul", "min": 0.25, "max": 4.0,
     "select": {"role": ["cost_base"], "phase": ["p1"]}}
  ],
  "players": [
    {"id": "active", "name": "Active player", "decision_s": 2, "active_share": 0.8, "reset_gain": 1.0, "stuck_s": 600},
    {"id": "casual", "name": "Casual player", "decision_s": 30, "active_share": 0.2, "reset_gain": 2.0, "stuck_s": 1800},
    {"id": "idle", "name": "Idle player", "decision_s": 300, "active_share": 0.0, "reset_gain": 2.0, "stuck_s": 3600}
  ],
  "targets": {
    "length_h": {"about": 6, "tolerance": 0.2},
    "upgrade_gap_min": {"median_about": 0.5, "longest_max": 2, "tolerance": 0.2},
    "content_gap_min": {"longest_max": 10, "first_hour_min_count": 11, "tolerance": 0.2},
    "impact": {"low_below": 0.03, "high_from": 0.25, "low_share_max": 0.4, "high_share_min": 0.1, "tolerance": 0.2},
    "idle_ratio": {"min": 1.5, "max": 6.0, "tolerance": 0.2},
    "wait_share_max": {"about": 0.15, "tolerance": 0.2}
  },
  "sim": {"max_hours": 18, "seed": 1}
}
```

A control that `init` wrote has the key `"by": "init:<hash>"`. `init --refresh-controls` writes such a control
again from the present ledger. A control with no `by`, or with a `by` that does not match its content, is the
control of a person and stays. A casual or idle player can have `"away_every_min"` and `"away_min"`: after each
`away_every_min` minutes of play the player is away for `away_min` minutes (section 9). `"review"` is written by
the tool: the count of numbers that nobody confirmed.

`core` names the pure core. A game with no pure core has `"adapter": "res://bench/adapter.gd"` in its place.
`session` names the autoload that takes new numbers into the game that runs (`reload_data`).

A phase ends at the first of its `until` conditions: `{"progress": x}`, `{"event": id}` (an action or event id) or
`{"resets": n}`. Phases are in order and the last phase ends at progress 1. Use the words of the game for the
names: eras, zones, "before the first reset". With no better idea, `init` writes 3 phases by progress. `share` is
the target share of the game length.

Targets are ranges (SKILL.md, "The spirit of the game comes before the numbers"). A target with no source has the
tolerance 0.2. The panel lets the designer change a target, because most targets are made-up rules.

## 8. Controls (knobs)

A control is 1 slider with a value k. It changes each number that its `select` names. `select` filters the tunable
balance numbers by `role`, `phase`, `element` or `id` (each a list; a number must match each key that is present).

| `op` | New value |
| --- | --- |
| `mul` | value x k |
| `growth` | 1 + (value - 1) x k, for a ratio above 1 (k = 1.1 makes a growth of 1.15 into 1.165) |
| `add` | value + k (the slider is centred on 0) |
| `set` | k (1 number only; the slider shows the value itself) |
| `role` | for each number the op of its role: `growth` for `effect_factor` and each role that ends in `_growth`, `mul` for each other role |

A number with `"sense": -1` gets 1 / k in the place of k (`add`: value - k).

A player control has `"player": {"field": "decision_s" or "active_share" or "reset_gain" or "stuck_s", "ids":
[...]}` in the place of `select` (also the fields `away_every_min` and `away_min`); with no `ids` it changes each
player. The panel makes the "All numbers" controls
itself, with the id `n:<number id>`.

The base value of a number is the value in its data file at the start, not the `value` of the ledger. In version
1 a control changes numbers of JSON files only: a balance number in another file kind is in the ledger with
`tunable: false`.

A number with an integer value stays an integer (rounded, at least 1 for a `cap`). When 2 controls select the same
number, both apply, in the order of the file. k = 1 (0 for `add`) gives the base value back.

`init` writes these controls. A group is not written when no number has its role.

- **Whole game** (1 for each role that is present): all prices, price growth, purchase punch (`effect`), purchase
  punch: factors (`effect_factor`), effect growth, effect exponent, base income, manual action power, manual
  action time, walls, wall growth, unlock points, reset need, reset gain, reset bonus, caps, timers, side rates,
  automation start, offline, rewards, reward growth, game end.
- **Each phase**: prices, purchase punch (op `role`, the roles `effect` and `effect_factor`), walls and unlock
  points of the numbers of that phase.
- **Player**: decision time, share of time with the manual action, reset gain that starts a reset. These change
  the bot, not the game.
- **All numbers**: each tunable balance number alone (`op: set`), grouped by element. This is the way to change
  1 upgrade.

Each generated control has 1 direction: a larger k gives a larger number. `init` thus takes 2 kinds of number out
of a role control, and `init` prints each such decision:

- An exponent (the word `exponent` in its key, or the role `effect_exponent`) is in no `mul` or `growth` control.
  It gets the control `<control>_exp` with the op `add` and the range -0.1 to 0.1. An exponent is strong: in one
  trial game +0.1 on the reset exponent changed the length from 6.0 h to 2.6 h.
- A divisor with no `sense` key gets the control `<control>_div`. Confirm it with `--sense -1` to put it into the
  main control with 1 / k.

A role control can still move numbers with different units (stages, spores and reset counts in "unlock points").
`init` prints the key names of each such control. Add a control by hand when the designer needs 1 of them alone.

A game with 3 phases and 8 roles gets about 8 + 9 + 3 controls and 1 control for each number. Add a control by
hand for a decision that the roles do not give (for example "second half of the generators: price").

The panel also has: save and load a setting, compare 2 settings (A against B on each meter), reset to base,
"Rank controls" (it moves each control by 25 percent and lists the change of each meter), "Fit length" (it finds
the k of 1 control that gives the target length), and "Write to data files" (it writes the changed values into
the data files of the game, with a backup copy of each file).

## 9. Players

The bot plays 1 run for each player of `players`. At each decision it does the reset when a reset action is
affordable and its `gain` is at least `reset_gain` (or when no purchase came for `stuck_s`). If not, it buys the
affordable action with the lowest cost, and repeats until nothing is affordable. Between decisions it calls
`active()` for `active_share` of the time. The run ends at progress 1 or at `sim.max_hours`.

The order of 1 decision: the bot buys; the runner reads the events, `rate()` and `progress()`; `step()` for
`decision_s` in parts of at most `max_step()`; `active()`; `away()` when it is due. With an action key `currency`,
the bot buys the lowest cost in the currency of the first affordable action.

A player with `away_every_min` and `away_min` closes the game: the runner calls `away(away_min x 60)` and the
clock moves by that time. This is the only test of the offline numbers. When the game has no `away`, the keys do
nothing and the result says `"away": "not supported"`; give each `offline` number a `dead_ok` with that reason.

The active player runs first, and the panel shows its meters at once. Each other player stops at `max_hours`, or
earlier at the active length x the upper limit of the M6 target x 1.25.

A run also stops when the player stalls: for `sim.stall_hours` of game time (default 2; 0 = off) there was no
action, no `bought` event, and no change of `progress()` or `rate()`. M1 then says "the idle player cannot start:
the game needs the manual action" (0 purchases; M6 is `n/a`) or "the idle player stalls at x h". A player that
waits longer than `stall_hours` for 1 purchase at a constant income is also reported as stalled.

**The time of a run** is the count of steps x the time of 1 step. The count of steps is game hours x 3600 /
min(`decision_s`, `max_step`). The bench adds less than 1 percent. A sample game ran in 0.5 s; a real game of 18 h
ran in 24 s for the active player and 68 s for 3 players, because the casual player played 45 h and did not end.
`bench_cli.gd --run --profile` prints the time of each contract function. When a run is slow: give a larger
honest `max_step`, make `actions()` cheaper, and set `sim.max_hours` near 2 x the target length.

The active player gives the main value of each meter. The other players give meter 6 and the lines "casual" and
"idle" of meter 1.

## 10. Meters

Each meter has a value, a target range, a status (`pass`, `pass (in range)`, `fail`, `n/a`), a value for each
phase, and a hint that names the controls that move it.

| Meter | Value | Source |
| --- | --- | --- |
| M1 Game length | hours to progress 1 for the active player; also casual and idle | end time of each run |
| M2 Time between upgrades | median, 90th percentile and longest time between 2 purchases, in minutes; a second line "by hand" when the game also buys by itself | purchase times of the bot and `bought` events |
| M3 Time between new things | longest time with no new content; count of new content in the first hour | first sight of each action id, and `events()` |
| M4 Impact of an upgrade | share of purchases that are low, medium, high; the median gain | `rate()` after divided by `rate()` before, minus 1 |
| M5 Phase balance | the share of the length that each phase takes, against its `share` | time of each phase end |
| M6 Active against idle | length for the idle player divided by length for the active player | the 2 runs |
| M7 Reset rhythm | count of resets, minute of the first reset, length of each run | reset times; `n/a` with no reset action |
| M8 Waits and dead content | share of the time in a wait above 2 x the longest target gap; count of actions never used; count of numbers with no effect | purchase times, action ids, self-check |

An "about x" target gives `pass` inside half of its tolerance and `pass (in range)` inside the tolerance. M5
passes inside 2.5 points of each share and is in range inside 5 points. M7 passes with no target; the optional
target `resets` has `count_min`, `count_max` and `first_min_max`. M8 fails on an action that the active player
never used and on a dead number with no `dead_ok`. In M2, purchases at 1 decision count as 1 purchase time.

An auto purchase (a `bought` event) counts in M2 and as a use in M8. It has no measured impact: M4 gives the count
("N auto purchases not measured"). M8 takes the dead numbers from `bench/selfcheck.json`. It says "not checked"
with no file, "self-check is stale" when the tunable numbers changed after the check, and "partial" after a check
with `--only`.

A purchase that does not change `rate()` (an unlock, automation) is "no rate change" in M4 and is not in the 3
shares. A run that does not reach progress 1 gives M1 "does not end in `max_hours`" with the status `fail`, and
the other meters use the part that ran.

The panel also draws the timeline of the active player: time on the x axis, the phases as bands, each purchase as
a mark with the colour of its impact, each new content as a tick, and the income as a line on a log scale. A table
lists each action: first time bought, count, median wait, impact class.

## 11. The command line of the bench (headless)

```bash
godot --headless --path <project> -s res://addons/tuning_bench/bench_cli.gd -- --run --out <result.json> [--setting <file>] [--k <control>=<k> ...] [--set <number id>=<value> ...] [--player <id>]
godot --headless --path <project> -s res://addons/tuning_bench/bench_cli.gd -- --selfcheck [--only <glob> ...] [--jobs <n>] [--out <check.json>]
godot --headless --path <project> -s res://addons/tuning_bench/bench_cli.gd -- --rank --out <rank.json>
godot --headless --path <project> -s res://addons/tuning_bench/bench_cli.gd -- --replay --out <replay.json>
```

`--fit <control> [--target-h <hours>]` finds the k of 1 control for the target length (exit code 1 when no k in
the range of the control gives the target inside 5 percent; a length that jumps at a reset can cause this).
`--selfcheck` with no `--out` writes `res://bench/selfcheck.json`, and `--run` reads that file. `--check <file>`
names another file; `--no-check` reads none. `--partial-out <file>` gets the result of the active player before
the other players run.

`--bench <file>` selects another bench file (default `res://bench/bench.json`). `--replay` (core only) records the
commands of the active run, replays them, and compares the end state; exit code 1 when they differ. Its result
file has the command list, a hash of the end state and 19 checkpoints. `--replay --against <record>` runs the
commands of a stored record on the core of now: exit 0 when the end state is the same, and if not, the result
names the time of the first difference. Use it after a refactor step; it is not valid across a balance change.

`--view-texts --out <file> [--scene <res path>] [--state start | bot:<hours> | <state.json>] [--visible-only]`
starts the main scene and writes each text of the view, 1 line for each (`<node path> TAB <property> TAB <text>`,
with `hidden` for a node that is not visible). 2 runs give the same file, so `diff` shows each text that a change
moved. The game needs a session that does not load or write the save in a headless run; a state other than
`start` needs `use_state(state)` on the session. Exit code 1 when the run made or changed a file in `user://`.

`--selfcheck` uses min(4, processors - 1) processes when `--jobs` is not given.

Exit code 0: it ran. 1: a meter fails (`--run`) or a check fails (`--selfcheck`). 2: bad input or an adapter error.

The result file of `--run`:

```json
{
  "format": "tuning-bench-result", "version": 1, "k": {"all_cost": 1.0},
  "players": {"active": {"ended": true, "length_s": 21480, "purchases": 412, "resets": 0}},
  "meters": [{"id": "M1", "name": "Game length", "value": 5.97, "unit": "h", "text": "5.97 h (casual 7.9 h, idle 14.2 h)",
              "target": "about 6 h (4.8 to 7.2)", "status": "pass", "phases": {"p1": 0.6}, "hint": ["all_cost", "all_wall"]}],
  "actions": [{"id": "big_lamp", "kind": "buy", "first_s": 95.0, "count": 25, "median_wait_s": 31.0, "impact": "medium", "median_gain": 0.08}],
  "timeline": {"purchases": [[95.0, "big_lamp", 0.08]], "content": [[95.0, "big_lamp"]], "phases": [["p1", 0.0, 2100.0]], "rate": [[0.0, 1.0]]}
}
```

The same seed and the same k give the same file, byte for byte.

## 12. The self-check

`--selfcheck` proves that the bench tells the truth about the game. It fails with exit code 1 on:

1. **Repeat:** 2 runs with the same input differ.
2. **Clean start:** `setup` 2 times in 1 process gives a run that differs from the first (state leaks).
3. **Dead number:** a tunable balance number that, with its value x 1.5 (growth roles: the part above 1 x 1.5),
   changes no time and no rate of the active run. Such a number is not wired, or the run never reaches it. The
   result lists each one. A number with `"dead_ok": "<reason>"` is listed and does not fail.
4. **Unknown source:** a tunable number of a file that is not in `sources`, or a pointer that does not exist.
5. **Unclassified:** a number with the class `unclassified`.
6. **Contract:** an action with no `id`, `kind`, `cost` or `affordable`; `progress()` outside 0 to 1; `rate()`
   below 0; `do_action` of an affordable action that returns false.
7. **No file written:** the simulation made or changed a file in `user://` (section 6, the side-effect list).

The dead-number check runs 1 simulation for each tunable number. A number with an early effect stops its run
early; a dead number runs the whole game. The cost is thus up to (count of tunable numbers) x (time of 1 active
run): 21 minutes for 205 numbers in one trial game. `--jobs <n>` uses n processes. `--only <glob>` checks the
numbers whose id matches (`*` and `?`) and merges the result into the file. The check prints 1 progress line for
each 10 numbers. It uses the active player; an `offline` number is also tried with each player that goes away.

A dead number has different causes. Find the cause before you write a `dead_ok`:

| Cause | Do |
| --- | --- |
| No code reads the number | Tell the designer. Remove it or wire it. No `dead_ok` |
| The number is read, and its limit never binds (a cap that the player never reaches) | Tell the designer: each reward that raises it gives nothing. No `dead_ok` |
| The probe is masked (a threshold of 0 that x 1.5 does not move; a gate that another gate hides) | Prove it with `--set <id>=<large value>`, then `dead_ok` with the proof |
| The feature is switched off in the data, or the line has 1 level | `dead_ok` with the reason |
| An `offline` number, and the game has no `away` | `dead_ok`, or write `away` |

## 13. `scripts/pw_bench.py`

```bash
python3 <skill_dir>/scripts/pw_bench.py install <project>
python3 <skill_dir>/scripts/pw_bench.py inventory <project>
python3 <skill_dir>/scripts/pw_bench.py init <project> [--phases-only | --refresh-controls] [--targets <targets.json>] [--length-h <hours>]
python3 <skill_dir>/scripts/pw_bench.py check <project> [--strict]
python3 <skill_dir>/scripts/pw_bench.py corecheck <project> [--core <dir or file> ...]
python3 <skill_dir>/scripts/pw_bench.py selfcheck <project> [--only <glob>] [--jobs <n>] [--timeout <s>] [--home <dir>]
python3 <skill_dir>/scripts/pw_bench.py run <project> [--setting <file>] [--k <control>=<k> ...] [--check <file>] [--compare <result.json>] [--out <file>] [--home <dir>]
```

- `install` copies `assets/godot/tuning_bench/` to `addons/tuning_bench/` and adds the autoload to `project.godot`.
  It writes an adapter stub only when the game has no core and no `bench/adapter.gd`. It changes no other game
  file. An adapter extends by path (`extends "res://addons/tuning_bench/bench_adapter.gd"`): a class name fails in
  a headless run before the editor imported the project.
- `inventory` writes or updates `numbers` and `sources` in `bench/bench.json`. On an update it keeps the class, the
  role, the element, the phase and the label of each number that is still there. It prints the new numbers, the
  numbers that went, and the counts by class.
- `init` writes `phases`, `controls`, `players`, `targets` and `sim` where they are absent. `--targets` takes the
  targets file of the solver and converts its length, purchase gap and unlock targets.
- `check` is the static gate, with no Godot: 0 unclassified numbers, a role for each balance number, a reason for
  each number that is not tunable, at least 1 number for each control, phases in order. Exit code 1 on a failure.
  While the phases or the controls are absent it prints a line `NEXT` with the next command, and exits 1. It
  prints "not reviewed: N numbers in M files"; `--strict` fails on each of them. It prints a note when the
  generated controls are not those of the present ledger.
- `corecheck` proves that the core is pure (`references/game-core.md` section 4).
- `selfcheck` and `run` start Godot with the command line of section 11 and print the result as a table. They need
  the program `godot` on the path, or the option `--godot <path>`. `--timeout` is an idle time: the tool stops
  Godot only when no line came for that time (default 900 s for `selfcheck`).
- `run` exits 1 when a meter fails; the last line then says "the run itself is complete". `run --compare <file>`
  compares the new result with a stored result, byte for byte, and exits 0 only when they are equal. Use it for
  each step of a refactor (`references/game-core.md` section 9).
- `--home <dir>` gives Godot another home folder. Godot writes its logs and `user://` into the home folder of the
  user at each run. Use `--home` on a copy of a game, in a sandbox and in CI, so that no file of a player changes.

## 14. Limits

- The bot looks for a new action at most each `sim.scan_s` seconds (default 10) while nothing is affordable, so
  the time of a new thing can be up to 10 s late.
- "Fit length" assumes that the length changes smoothly with the control.
- "Write to data files" changes the data files and not the ledger. Run the inventory after it.
- The bot compares costs of 1 currency. With no `currency` key on the actions, it compares all costs as plain
  numbers.
- An auto purchase is recorded at the decision after its step, so its time can be `decision_s` late.
- A ledger from an older version keeps its roles. Look for a factor with the role `effect`, and give it
  `effect_factor`.
- The exponent and divisor rules read the key name only. Check each control that `init` prints with more than 1
  key name.
- A real cooldown in a view script gets the proposal `presentation`. `check --strict` shows it.
- The bot is not a person. It buys the lowest price first. A meter is an estimate of pacing, not of fun.
- A game with choices that exclude each other (a skill tree with 1 branch) needs a player entry for each route.
- Random effects need a seed that the adapter passes to the game. Without it the repeat check fails.
- The impact classes measure the main income only. A purchase that helps a side system shows as "no rate change".
- The bench does not change formulas or content. When no control setting gives the targets, the game needs more
  content or another formula: that is a design decision (`references/content-step.md`).
