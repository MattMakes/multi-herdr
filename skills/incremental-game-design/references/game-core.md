# The game core: math that runs with no game window

A game from this skill has 3 layers. The math is in 1 layer only, and that layer can run alone. The game window is
1 face of the math. The tuning bench (`references/tuning-bench.md`) and a test are other faces.

The reason is time. A bot that plays the game window needs hours for 1 game. The core alone runs the same game in
seconds to minutes, so a balance change or a bug fix has its answer before the designer loses the thread. The
time is the count of steps x the cost of 1 step: a sample game of 6.5 h ran in 0.4 s, and a real game of 18 h
with 47 purchase kinds ran in 24 s for 1 player. A pure core is not faster than the old code for each step. It is
faster because no window, no frame and no real clock are in the loop.

Contents:

1. The pattern and its sources
2. The 3 layers
3. The core contract
4. The rules of a pure core (`pw_bench.py corecheck`)
5. The session (the controller)
6. The view
7. Time: fixed steps, large steps, offline progress
8. A worker thread
9. Refactor an existing game (the safe order)
10. Decisions that the refactor of an existing game needs
11. What stays hard

## 1. The pattern and its sources

Web MVC is a poor fit as it is: it has a request and an answer, and a game has a clock that always moves. Game
development uses the same separation with 4 added rules. The names below are the usual names of the parts.

| Part | Rule | Source |
| --- | --- | --- |
| Functional core, imperative shell | The logic takes values and returns values. A thin shell does input, output and time | G. Bernhardt, talk "Boundaries" (not checked again for this file) |
| Simulation and presentation split, fixed time step | The simulation moves only in steps of a given `dt`. The view draws the last state | G. Fiedler, "Fix Your Timestep!", gafferongames.com/post/fix_your_timestep (checked) |
| Commands in, events out | Input becomes a command. The simulation answers with events. The view never writes state | R. Nystrom, "Game Programming Patterns", chapters Command, Event Queue, Observer (not checked again) |
| Replay | The same start, the same seed and the same list of commands give the same end state | event sourcing; used here as a test |
| Golden master | Before a refactor, record what the code does. Refactor until the record is the same | M. Feathers, "Working Effectively with Legacy Code" (not checked again) |
| Thread rules of Godot | "Interacting with the active scene tree is not thread-safe" | Godot documentation, "Thread-safe APIs" (checked) |

An entity component system (ECS) gives the same split. It is too much for an incremental game: take the split and
not the framework.

## 2. The 3 layers

| Layer | Folder | It is | It knows | It never does |
| --- | --- | --- | --- | --- |
| Core | `core/` | Static functions over plain state | The data and the state that the caller gives | Nodes, scenes, autoloads, signals, clocks, files, random numbers with no seed |
| Session | `session/` | 1 autoload node, the controller | The core, the data files, the save file, the frame clock | Math. It has no formula and no balance number |
| View | `view/` (scenes, UI, animation, sound) | Nodes | The session: its queries, its `dispatch`, its events | Write state. Compute a price, an income or a condition |

The dependency goes 1 way: view → session → core. The core names nothing above it. This is the inversion that lets
another face (the bench, a test, a command line) use the core with no game window.

## 3. The core contract

`core/game_core.gd`, `class_name GameCore extends RefCounted`, static functions only. The state is plain data:
dictionaries, arrays, numbers, strings, booleans. The save file is thus the state as JSON.

| Function | Meaning |
| --- | --- |
| `new_state(data: Dictionary, seed: int = 1) -> Dictionary` | The state of a new game. `data` maps each data file to its parsed content |
| `step(state, data, dt: float) -> Array` | Advance by `dt` game seconds. Return the events |
| `actions(state, data) -> Array` | Each thing the player can do or see now (the action format of `references/tuning-bench.md` section 6) |
| `apply(state, data, action_id: String) -> Array` | Do 1 action. Return the events. A refused action returns 1 event of the kind `refused` and changes nothing |
| `active(state, data, seconds: float) -> Array` | The player does the manual action for this time |
| `away(state, data, seconds: float) -> Array` | The player closes the game for this time. The core applies the offline rule of the data |
| `rate(state, data) -> float` | Main income for each second, with no manual action |
| `progress(state, data) -> float` | 0 at the start, 1 when the game is complete |
| `wealth(state, data) -> float` | The amount of the main currency |
| `max_step(data) -> float` | The largest `dt` that gives a correct result |

`new_state`, `step`, `actions`, `apply`, `rate` and `progress` are required; `active`, `wealth` and `max_step` are
optional, and so is `away`. A core can
have `refresh(state, data)`: the session calls it after new data or a loaded save, to compute derived values again.
Other scripts load the core by path (`const Core := preload("res://core/game_core.gd")`), because a class name is
not known in a headless run of a project that the editor did not import.

When the game has a class with the name `GameCore` already, the pure core is a new file with no class name, and
the old class stays as the wrapper (section 10).

`step`, `apply` and `active` change only the state that the caller gave. No function changes `data`. A function can
return more queries for the view (for example the price of 10 levels); each one is a static function of `state`
and `data`.

**The data index.** The bench gives the core the raw content of the data files. A lookup by id in a list on each
call is slow. `refresh` can put a derived index of the data into the state (for example the key `ix`). A derived
key is not saved.

**`progress` of a game with no end value.** Count the things that a player completes 1 time (stages, moves,
collection entries) and divide by their total. The value never goes down, also not at a reset. Say in the report
what 1.0 is.

**An action that the bot must not do.** `actions` gives each thing that the player can do, because the view
builds its controls from it. A free choice that is always possible (a selector, a switch) has `"bot": false`: the
bot and the meters ignore it.

**A manual action that is a hold.** When income is larger while the player holds a button, `active(state, data,
seconds)` adds hold time to a credit in the state, and `step` uses the credit. The session calls `active(dt)`
before `step(dt)` while the button is down.

An event is a dictionary: `{"kind": "bought" or "unlock" or "advance" or "reward" or "reset" or "refused", "id":
..., "label": ..., and the numbers that the view needs}`. A purchase that the game makes by itself (an auto-buy in `step`) is an event of the kind `bought` with a `count`.
An event says what occurred. It does not say how to show
it. A game can add kinds of its own (for example `kiln`, `prepared`); the bench reads only the kinds above.

With this contract the tuning bench needs no adapter: `bench.json` has `"core": "res://core/game_core.gd"`.

## 4. The rules of a pure core (`pw_bench.py corecheck`)

```bash
python3 <skill_dir>/scripts/pw_bench.py corecheck <project> [--core <dir or file> ...]
```

The check reads each script of the core and fails on:

- a base class that is not `RefCounted`, `Resource` or `Object`;
- the scene or the engine: `get_tree`, `get_node`, a `$` or `%` node path, `add_child`, `_ready`, `_process`,
  `_input`, `Engine.`, `Time.`, `OS.`, `Input.`, `DisplayServer.`;
- a file or the network: `FileAccess`, `DirAccess`, `ResourceLoader`, `load` or `preload` of a file outside the
  core, `ProjectSettings`;
- hidden random numbers: `randf`, `randi`, `randomize`, a new `RandomNumberGenerator`. The seed is in the state;
- `await`, `signal`, `emit`, `connect`, `Thread`, `Mutex`;
- the name of an autoload;
- a variable at script level. The state comes in as an argument.

The comment `# corecheck: ok <reason>` at the end of a line accepts the findings of that line. The check prints each accepted
finding. It also prints the count of balance numbers of the ledger that are in a script outside the core: each one
is math that is not in the core yet.

The check is static. `bench_cli.gd --replay` is the running proof: it records the commands of a run, replays them,
and compares the end state.

## 5. The session (the controller)

`session/game_session.gd`, the autoload `Session`. It is the only owner of the state of the game that runs.

- It loads the data files and calls `GameCore.new_state`, or loads the save file.
- In `_process` it adds the frame time to an accumulator and calls `GameCore.step` in fixed steps. It does not
  drop game time: an idle game runs in a hidden or slow window, and a hard limit on the frame time makes it run
  slow there. After a long frame it takes larger steps, up to `max_step`, and keeps the rest for the next frame.
- `dispatch(action_id: String)` calls `GameCore.apply`.
- It emits `event(e: Dictionary)` for each event of the core, and `changed()` after a step or an action.
- `reload_data(data: Dictionary)` takes new numbers into the game that runs. The tuning bench uses it.
- It gives the queries of the core to the view (`actions()`, `rate()`, `progress()`).

- In a headless run (the bench command line, a test) the autoload still exists. It then does not load and does
  not write the save file of the player.
- A view takes its session from a property, with the autoload as the default. A test or a second viewport gives
  the view a session of its own.

The session has no `if` on a price, an income or an unlock. When a rule of the game must change, the change is in
the core.

## 6. The view

- The view builds each list from `Session.actions()`. It has no list of upgrades in a scene or a script. A new
  upgrade in the data appears with no change of the view.
- Each number on the screen comes from the session at the time of the draw. The view keeps no total of its own.
- A text with a game number is built from the data at the time of the draw. `"+25% pot value per level"` as a
  literal is a copy of a balance number: it shows the old value after a change. The inventory lists each such text
  (`texts` in `bench.json`), and `check --strict` fails while 1 is open.
- A button calls `Session.dispatch`. It does not test if the purchase is possible: the action says `affordable`.
- An animation starts on an event (`bought`, `unlock`, `advance`, `reset`). Its length is a presentation number.
  It is not derived from a balance number, and no rule of the game waits for an animation to end.
- A layout must hold at large numbers and at a long list: the bench changes both.

Add 1 smoke test of the view: each action id that the core gives during a bench run has a control in the view.

## 7. Time: fixed steps, large steps, offline progress

The live game steps with a small fixed `dt`. The bench and the offline progress call the same `step` with a larger
`dt`, up to `max_step`. A large step is wrong when the step has a threshold, a cap or an event inside it. Thus:

- Give `max_step` honestly. Use a closed form inside `step` only where the math permits it.
- Test it: 1 step of `max_step` against many small steps over the same time must give the same state, inside a
  stated tolerance.
- Offline progress is `away`: `step` in a loop over the time away, with the offline rule of the game as data (a
  rate, a cap). It is not a second formula. The session calls it when it loads a save, with the time from the
  save to now.

## 8. A worker thread

Start with no thread in the live game: the step of an incremental game is small. The bench is the place for a
thread: it runs whole games while the designer moves a slider. A pure core makes this safe, because it touches
nothing that a thread must not touch. The rules:

1. The thread owns its state. It makes its own state with `new_state`. It never gets the state of the session.
2. The input of the thread is a copy of `data` and a list of settings, given before it starts.
3. The output is a result that the thread makes and the main thread reads after the thread ends (or with
   `call_deferred`).
4. The thread never touches a node, the scene tree or an autoload. The core check proves this for the core.
5. No resource object is shared between threads.

A game whose math is in an autoload cannot use the thread. The bench then starts a second Godot process, which is
slower and still safe.

## 9. Refactor an existing game (the safe order)

Do not rewrite. Move the math in small steps, with records that prove that nothing changed. Make 1 commit for each
step. In one trial on a real game (537 lines of math, 47 purchase kinds), 13 moves each gave the same bytes.

1. **Install the bench with an adapter** on the game as it is (`references/tuning-bench.md`). The adapter drives
   the existing code.
2. **Record 3 masters.**
   - The bench result: `pw_bench.py run <project> --out bench/golden.json`. Later: `run --compare
     bench/golden.json`. The exit code of a plain `run` is about the balance, not about the refactor.
   - The game's own tests, and its autoplay output if it has one. The bench result covers only what the bot does:
     the actions, the rate and the progress. It does not see the signals and their order, the save and the load,
     the queries of the view, a refused action, or an action that the bot never does.
   - The view texts: `bench_cli.gd --view-texts --out <file>` in 2 or 3 known states.
   Fix each cause of a different result first (a random number with no seed, a real clock): each one is also a
   bug for a player.
3. **List the math.** `pw_bench.py inventory` gives each number and each text with a number. Each balance number
   in a script, each formula in `formulas`, and each text is a thing to move. Give the designer the size of the
   work now: the count of each, and the count of scripts that mix math with nodes.
4. **Put the state into 1 dictionary first.** The old state object keeps its properties as getters and setters
   over the dictionary. No math moves. A static function cannot change a state that is still the variables of an
   object, so this step comes before the next.
5. **Move 1 system at a time** into `core/` as static functions: data access, income, prices, purchases, side
   systems, the reset, time and the manual action, then the contract functions. The old script calls the core
   function and keeps its public methods and its signals (section 10), so the view and the tests do not change
   yet. After each system: compare with the 3 masters.
6. **Add the session.** It owns the state dictionary. The old autoloads become thin wrappers.
7. **Switch the bench to the core** (`"core"` in `bench.json`, remove the adapter). The result must still be the
   record. Run `corecheck`, and `bench_cli.gd --replay --out <file>`: it records the command list and a hash of
   the end state, and `--replay --against <file>` proves them again after a later change.
8. **Lift:** each balance literal of the core goes to a data file, with the same value.
9. **Clean the view, as a ladder. Stop after each rung, with the masters equal.**
   1. Each button sends an action id. Each panel names its ids. The smoke test of section 6 passes.
   2. No balance literal and no text with a balance number in the view. The view-text master is equal before and
      after, and a changed number in the data changes the text.
   3. Lists from `actions()`. For a view that is laid out by hand, with art for each card, this is a new view. Do
      it only with the designer.
10. Only now change the balance.

When a record must change (a bug that the refactor shows), change it in a step of its own and write why.

## 10. Decisions that the refactor of an existing game needs

No tool shows a red light for the first 3. Each one can damage the game of a player.

- **The save file.** Keep its format and its version. Save the old keys only: a derived key (an index) and a key
  that the refactor adds for its own use are not saved, or get a default at load. `refresh` runs after a load.
  Add a test that loads a save file written before the refactor.
- **The clock.** The old game lost no time in a slow window. The session must not lose time (section 5).
- **Tool runs.** An autoload session exists in each headless run. It must not load or write the real save there.
  Run tools on a copy with `--home <dir>` (`references/tuning-bench.md` section 13).
- **Signals of the old code.** 1 function in the wrapper turns each event of the core into the old signals, in
  the order of the events. A method that returned a value (`unload() -> float`) takes the value from the event.
- **3 wrapper shapes.** A query calls a core function with the state. A command calls `apply` and passes the
  events on. A command with a return value does the same and reads the event.
- **Names.** The folder and the class name of the old math can be the names of the contract. Use a new folder
  `core/` and no class name.
- **`progress`, `"bot": false`, the hold action, the data index:** section 3.

## 11. What stays hard

- A golden master keeps bugs. It proves "the same", not "correct". It proves it only for what it records.
- A replay of commands is no longer valid after a balance change. Use it for a refactor, not across settings.
- GDScript does not enforce purity. The static check finds names, not each possible side effect.
- A mini-game with skill (timing, aim) has no pure player. Give the core an outcome parameter (for example a hit
  quality from 0 to 1), and let the bench players set it.
- The view can still be dull with good numbers. The bench does not measure juice.
