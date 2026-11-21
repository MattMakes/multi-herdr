# The tuning bench never changes the running game

The autoload `TuningBench`
(`skills/incremental-game-design/assets/godot/tuning_bench/bench.gd`)
loads `res://bench/bench.json`, holds the value k of each control and
starts the simulations. A simulation never changes the game that runs. An
isolated adapter (a game with a pure core, `core_adapter.gd`) runs on a
thread with its own state. Every other adapter runs in a second Godot
process. The hotkey (default F2) opens the panel. The bench works only in
a debug build, or when `bench.json` has `"release": true`.

## Consequences

- `core_adapter.gd` touches no autoload and no scene, which is what lets
  the panel run the simulation on a thread. A core has static functions
  over a plain-data state: `new_state(data, seed)`, `step(state, data,
  dt)`, `actions(state, data)`, `apply(state, data, action_id)`,
  `active(state, data, seconds)`, `rate`, `progress`, `wealth`,
  `max_step(data)` and `away(state, data, seconds)`. `bench.json` names it
  with the key "core", and the game then needs no adapter of its own.
- The cost of `core_adapter.gd` is 1 call by name per contract call, about
  0.1 microseconds (measured, Godot 4.7). The time of a run is the time of
  the core's functions. `bench_cli.gd --run --profile` prints the time of
  each contract function.
- The contract has no hold argument in `step`. A hold action (higher income
  while the player holds a button) adds a `hold_credit` in `active` and
  uses it in `step`. The session calls `active(dt)` then `step(dt)` each
  frame while the button is down. The bench calls `step` for `decision_s`
  and then `active(decision_s x active_share)` 1 time, so the bot's hold is
  1 decision late and the share of time with the hold is `active_share`.
  `hold_credit` is state: it is in the save file, or the session sets it
  to 0 after a load.
- `sample_game/bench/legacy_adapter.gd` (named by `bench/bench.legacy.json`)
  shows a hand-written adapter for a game with no core. It drives Lamp Road
  through its autoload, so the bench runs it in a second Godot process.
  Lamp Road itself does not need it.
- A game writes `res://bench/adapter.gd`, which extends the base by path:
  `extends "res://addons/tuning_bench/bench_adapter.gd"`. The path works in
  a headless run of a project that the editor did not import. The class
  name does not. All times are game seconds.
- The simulation process runs the whole game with each autoload. Before
  writing `setup()`, list each autoload of `project.godot`, switch off each
  one that writes a file, reads a setting or reacts to an economy signal
  (stop its autosave, disconnect its handlers, stop its `_process`), and run
  the self-check. Its row "No file written" lists `user://` before and after
  the simulation and names each new or changed file.
- An action is `{"id", "kind", "label", "cost", "affordable"}` with the
  optional keys "element", "gain" and "currency". `kind` is "buy", "once",
  "advance", "reset" or "other". "currency" names a cost not in the main
  currency (""). The bot compares 2 costs only within 1 currency, and
  `wealth()` is compared only with a cost in the main currency.
- An action with `"bot": false` is a free choice of the player that can
  repeat with no end (a selector, a switch that goes on and off, a click).
  The view shows it. The bot never does it, and meter 8 does not count it as
  "never used". Without the key such an action stops the run with the error
  "the bot did 5000 actions at 1 decision".
- The bot (`bench_player.gd`) decides at 1 decision with the same rule for
  each game. It resets when a reset is affordable and its gain is large
  enough, or when no purchase came for a long time. Otherwise it takes the
  currency of the first affordable action and buys the lowest cost in that
  currency.
- An adapter's optional `events()` returns what occurred with no action
  since the last call: `{"kind": "unlock" | "advance" | "reward" |
  "bought", "id", "label"}`. A purchase the bot did not ask for with
  `do_action` (an auto-buy) is `{"kind": "bought", "id": <action id>,
  "count": n}`, with "count" 1 when absent. A purchase that `do_action` made
  is not reported, because the bench counts that one itself.
- `bench_cli.gd`, a `SceneTree` script, starts its work in the first
  `_process` frame, not in `_init`, because each autoload of the game is
  ready only then.
- For `--view-texts`, a node with an engine-made name (contains `@`, such
  as `@Label@12`) gets `<class>#<index below its parent>`, which is the
  same in each run.
- `bench_json.gd` is its own JSON parser. Godot's parser makes each number
  a float. The bench keeps an integer as an integer, so an integer number
  of the game stays an integer after a control changes it.
