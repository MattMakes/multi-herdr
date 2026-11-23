# Lamp Road: the sample game of the tuning bench

Lamp Road is a small incremental game. It is original. It is a test fixture and a worked example for the tuning
bench (`references/tuning-bench.md`). It is not a product.

The player makes light. The player buys lamps and upgrades, lights 10 miles of a road, and starts a new night
(the reset) for embers. The game has 6 lamps, 12 upgrades, 10 miles, 1 manual action, 1 reset layer and 3 phases
(Dusk, Night, Dawn). The active player of the bench ends the game in about 6.5 hours.

The game has 2 more rules, as data in `data/economy.json`. They are there because most incremental games have them:

- **The lamplighter** (`auto_buy`): after 30 M light of all nights the game buys candle levels by itself, inside
  `step`. The core returns 1 event `bought` for each one. The bench shows them as auto purchases.
  The player can switch the lamplighter off and on. The switch is the action `lamplighter` with `"bot": false`:
  a free choice that the view shows and that the bot of the bench never does. Without the key the bot would
  repeat the free action with no end.
- **The offline rule** (`offline`): `away(state, data, seconds)` gives the income x `rate` for at most
  `cap_minutes`. The idle player of the bench closes the game for 30 minutes after each 60 minutes of play, so the
  2 offline numbers change its run.

## The 3 layers

| Path | Layer | Rule |
| --- | --- | --- |
| `core/game_core.gd` | The core: the math | Static functions over a plain-data state. No node, no autoload, no file, no clock |
| `session/game_session.gd` | The session: the autoload `Session` | Loads the data, owns 1 state, calls the core. No math |
| `view/main.tscn`, `view/main.gd` | The view | Builds its lists from the actions of the core. Sends intents to the session. Never writes the state |

The view is one face of the core. The tuning bench is another face of the same core.

The session follows 3 rules (the head of `session/game_session.gd`):

1. No game time is lost. A long frame is stepped in larger steps, up to `max_step` of the core. 1 frame catches
   up at most 4 x `max_step`; the rest waits for the next frames.
2. In a headless run (the command line of the bench, a test) the autoload does not read the save file, does not
   write it and does not move the clock.
3. A view takes its session from its property `session`. The default is the autoload. A view in a test or in a
   SubViewport gets a session of its own, which never uses the save file.

The manual action is a click. The comment "A HOLD ACTION" in `core/game_core.gd` shows the pattern for a game in
which the player holds a button.

## The numbers

Each balance number is in `data/*.json`. 3 balance numbers are in the code of the core on purpose. The ledger
lists them with `"tunable": false` and a reason. 1 number is dead on purpose: `click.hold_power` in
`data/economy.json`. No code reads it.

The ledger shows each case of a role: the factor of an upgrade has the role `effect_factor` (a control changes
only its part above 1), the divisor `reset.gain_div` has `"sense": -1` (a control applies 1 / k), and the phase
controls "purchase punch" have `"op": "role"` (each number gets the op of its role).

## The bench files

| File | Use |
| --- | --- |
| `bench/bench.json` | The normal bench file. It names the core (`"core"`), so the game needs no adapter. The self-check passes |
| `bench/bench.legacy.json` | The same game through the hand-written adapter `bench/legacy_adapter.gd`. That adapter drives the autoload `Session`, as an adapter of a game with no core does. The bench runs it in a second process |
| `bench/bench.dead-number.json` | The dead number has no `dead_ok`. The self-check fails and names the number |

## Run it

The folder is a Godot 4 project. The addon is not in the folder. Copy it first:

```bash
cp -R ../tuning_bench addons/tuning_bench   # make the folder addons/ first
godot --path .                               # the game; the key F2 opens the bench
godot --headless --path . -s res://addons/tuning_bench/bench_cli.gd -- --run --out /tmp/result.json
godot --headless --path . -s res://addons/tuning_bench/bench_cli.gd -- --selfcheck --out /tmp/check.json
godot --headless --path . -s res://addons/tuning_bench/bench_cli.gd -- --replay --out /tmp/replay.json
godot --headless --path . -s res://addons/tuning_bench/bench_cli.gd -- --replay --against /tmp/replay.json --out /tmp/replay-check.json
godot --headless --path . -s res://addons/tuning_bench/bench_cli.gd -- --view-texts --state start --state bot:1 --out /tmp/view-texts.txt
godot --headless --path . -s res://addons/tuning_bench/bench_cli.gd -- --run --profile --out /tmp/result.json
```

`--replay --out` writes the record of the commands of the active player and the hash of the end state.
`--replay --against` does these commands again on the core of now: exit code 1 when the end state differs.
`--view-texts` writes each text of the view, 1 line each, for a new game and for the game of the bot after 1 hour.
Compare 2 such files with `diff`. Godot writes its log below the home folder; set `HOME` to another folder for a
run that must not touch it.

The test `tests/test_tuning_bench_godot.py` does the copy into a temporary folder and runs these commands.
