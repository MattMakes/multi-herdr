# Live check: Godot seats on a real task

A Godot fleet seat does 1 real task on Godot 4.7.2 and records the result.
The `godot-gameplay-programmer` seat builds "Coin Dash", a small 2D GDScript
project, and proves it with the commands in its expected skills. The
`godot-qa-engineer` seat then tests the same project from the feature spec.
Godot wave step 5 (status item 18).

## How to run

The task is "build Coin Dash to the contract in
[Appendix: the Coin Dash contract](#appendix-the-coin-dash-contract), and
prove it with the commands in your expected skills". An orchestrator assigns
it to a `godot-gameplay-programmer` worker, and the same contract to a
`godot-qa-engineer` worker, which tests the project from the contract, not
from the code. (The 2026-10-07 run used the plans
`ai_docs/plans/godot-gaps/g2-gameplay-trial.md` and `g3-qa-trial.md`; `ai_docs/`
is local scratch and not in git.) The worker
writes the project to `.worktrees/_scratch/godot-trial/` (git-ignored).

To re-run only the proofs on an existing project, from the project root:

```bash
GODOT=/Applications/Godot.app/Contents/MacOS/Godot
GH="$PWD/.godot/horch-home"; LOGS="$GH/logs"; mkdir -p "$LOGS"
gd() { HOME="$GH" "$GODOT" --headless --path "$PWD" "$@"; }
gd --import > "$LOGS/import.log" 2>&1; echo "import exit=$?"
gd -s "$GH/parse_check_init.gd" -- --skip=addons,tests > "$LOGS/parse.log" 2>&1; echo "parse exit=$?"
gd -s "$GH/scene_check.gd" -- res://scenes/main.tscn > "$LOGS/scene.log" 2>&1; echo "scene exit=$?"
gd --quit-after 120 > "$LOGS/smoke.log" 2>&1; echo "smoke exit=$?"
grep -n -E 'SCRIPT ERROR|Parse Error|^ERROR:|^WARNING:|PARSE_CHECK FAIL' "$LOGS"/{import,parse,scene,smoke}.log
```

`parse_check_init.gd` is the parse checker from `godot-build-verify`
(`references/parse-check.md`) with `func _init()` renamed to
`func _initialize()`. `scene_check.gd` is the scene check from
`godot-scene-files` (`references/worked-example.md`) with the same rename.
Problem 1 below gives the reason.

It needs Godot 4.7.2 (standard build) at
`/Applications/Godot.app/Contents/MacOS/Godot`. It starts no model session,
so the proofs cost no tokens. The seat run itself is 1 worker session.

## gameplay-programmer, 2026-10-07

Seat: `godot-gameplay-programmer` (worker `godot-gameplay-programmer-1`,
Claude). Engine: Godot 4.7.2.stable.official.ed1daf0bf, macOS, headless only.

### Task

Build Coin Dash from `feature-spec.md` as a new project:

- `project.godot`: 4.7, GL Compatibility, main scene `res://scenes/main.tscn`,
  autoload `GameState`, input actions `move_left`, `move_right`, `move_up`,
  `move_down` (arrow keys and WASD) and `dash` (Space).
- `scripts/game_state.gd`: `score`, `score_changed`, `add_score()`, `reset()`.
- `scripts/player.gd`: `class_name Player`, `CharacterBody2D`, `apply_input()`
  with a timed dash and cooldown, `can_dash()`, `is_dashing()`, `dashed`.
- `scripts/coin.gd`: `class_name Coin`, `Area2D`, collected once by a `Player`.
- `scripts/score_label.gd`: the HUD label, `Score: <n>`.
- `scenes/main.tscn`: 1 `Player`, 5 `Coin` nodes under `Coins`, `HUD/ScoreLabel`.
  Built by a `SceneTree` script (`.godot/horch-home/build_main.gd`) with
  `owner` set on every node, as `godot-scene-files` says for a new scene.

### Skills loaded

- `godot-build-verify` (with `references/commands.md` and
  `references/parse-check.md`)
- `godot-scene-files` (with `references/worked-example.md`)
- `godot-player-controller`
- `godot-input-handling`

The plan also names `godot-project-context`. That skill is not in this seat's
skill bundle, so the seat could not load it (problem 2).

### Proofs

| Step | Command | Exit | Last lines of output |
|------|---------|------|----------------------|
| import | `gd --import` | 0 | `[ DONE ] loading_editor_layout` |
| parse check, skill text as written | `gd -s "$GH/parse_check.gd" -- --skip=addons,tests` | 1 | `PARSE_CHECK FAIL res://scripts/coin.gd`, `PARSE_CHECK FAIL res://scripts/score_label.gd`, `PARSE_CHECK checked=5 failed=2` (false failures, problem 1) |
| parse check, `_initialize()` | `gd -s "$GH/parse_check_init.gd" -- --skip=addons,tests` | 0 | `PARSE_CHECK checked=5 failed=0` |
| scene check | `gd -s "$GH/scene_check.gd" -- res://scenes/main.tscn` | 0 | `SCENE_CHECK res://scenes/main.tscn nodes=22` |
| smoke run | `gd --quit-after 120` | 0 | `Godot Engine v4.7.2.stable.official.ed1daf0bf - https://godotengine.org` (no other line) |
| grep | `grep -E 'SCRIPT ERROR\|Parse Error\|^ERROR:\|^WARNING:\|PARSE_CHECK FAIL'` on the import, parse, scene and smoke logs | 1 | 0 hits |

The parse check skips `addons/` and `tests/`. The QA seat owns them and
wrote them at the same time.

A behaviour probe (`.godot/horch-home/probe_play.gd`, not a test suite)
instanced the main scene and printed:

- each of the 4 move actions has 2 key events; `dash` has 1;
- a diagonal input gives speed 200.0; a dash gives 600.0, `is_dashing()`
  true and `can_dash()` false; a second dash press in the cooldown emits no
  second `dashed`; after 2 s, `can_dash()` is true;
- `add_score(0)` and `add_score(-3)` leave the score at 0;
- the player on `Coin1` gives `Score: 1` on the label and 4 coins left; the
  score stays 1 after 10 more physics frames;
- `reset()` sets the label to `Score: 0`.

### Time

Start 05:33 MST, proofs pass and QA notified 05:36 MST, doc written 05:37
MST (wall clock of the worker host).

### Problems found in the skill texts

1. **Autoloads do not exist in a `SceneTree` script's `_init()`.**
   `godot-build-verify` `references/parse-check.md`, and `godot-scene-files`
   `scene_check.gd` and `build_level.gd`, do their work in `_init()`.
   Measured on 4.7.2: in `_init()`, `root.has_node("GameState")` is false,
   and loading a script that names the autoload prints
   `SCRIPT ERROR: Compile Error: Identifier not found: GameState`. In
   `_initialize()`, the autoload exists and the same script loads with
   `can_instantiate()` true. Effects:
   - the parse check reports a false `PARSE_CHECK FAIL` for every script
     that names an autoload (2 of 4 scripts here);
   - a build script in `_init()` saves the scene without the script on those
     nodes and still prints `BUILD_OK`. The first `main.tscn` lost the script
     on all 5 coins and the label. Only the `SCRIPT ERROR` lines showed it.
   Fix to propose: use `_initialize()` in all 3 scripts, and add the case to
   the "what each wrong edit does" table.
2. **`godot-project-context` is missing.** The plan names it, and
   `godot-build-verify` and `godot-scene-files` both require
   `.agents/godot-project-context.md` and say to send `QUESTION:` without it.
   For a new project, no context file can exist yet. The seat used
   `feature-spec.md` and the engine version from `00-common.md` instead.
   The skills do not say what to do for a new project.
3. **A `-s` script cannot name an autoload at all.** Even in
   `_initialize()`, a `SceneTree` script that names `GameState` fails to
   compile: the script compiles before the autoloads exist. Use
   `root.get_node("GameState")`. No skill says this.
4. **No text form of the Input Map.** `godot-input-handling` says to define
   actions in the editor's Input Map, or in code at runtime. A headless seat
   writes `project.godot` as text. The `[input]` section format
   (`Object(InputEventKey, ... "physical_keycode": ...)`) is in no skill. The
   seat wrote it from memory and proved it with `InputMap.action_get_events()`.
5. **The first import of a new project prints an `ERROR:`.** When
   `run/main_scene` names a scene that the build script has not made yet,
   `--import` prints `ERROR: Cannot open file 'res://scenes/main.tscn'.` and
   exits 0. `godot-build-verify` gives no order for a new project: import,
   build the scene, then import again.
6. **The editor check matches its own shell.** The step 1 command
   `ps -axo pid,args | grep -i '[g]odot'` also matches the worker's own
   shell when its command line contains a path with "godot" in it. Here
   its own command text filled most of a 45 KB tool output. A filter such as
   `grep -v -- '-c '` or a match on the `Godot` binary path is needed.
7. **`.uid` commit rule.** Both skills say to commit the `.uid` sidecars.
   A git-ignored scratch project has nothing to commit. Minor.
8. **Dead skill links.** `godot-player-controller` and
   `godot-input-handling` point to `godot-2d-essentials`, `godot-ui`,
   `godot-save-load` and others, and `godot-scene-files` points to
   `godot-scene-organization`. None of these is in this seat's bundle.

## qa-engineer, 2026-10-07

Seat: `godot-qa-engineer` (worker `godot-qa-engineer-1`, Claude). Engine:
Godot 4.7.2.stable.official.ed1daf0bf, macOS, headless only.

### Task

Write automated tests for Coin Dash from `feature-spec.md` (the contract,
not the game code), run them headless, and report each failure as a bug.
The gameplay seat built the game at the same time. The QA seat set up the
tests first and ran them when the build was ready.

### Framework

GUT 9.7.1 (the release the skill names for Godot 4.7.x). Source:
`https://github.com/bitwes/Gut/archive/refs/tags/v9.7.1.tar.gz`. The seat
copied `addons/gut/` from the tarball into
`.worktrees/_scratch/godot-trial/addons/`. The plugin is not enabled in
`project.godot`: the command line runner does not need it, so the QA seat
changed nothing in `project.godot`.

### Files (in `.worktrees/_scratch/godot-trial/`, git-ignored)

- `.gutconfig.json`: test directories, no colours, exit when done.
- `tests/run.sh`: the 1 command. It imports, runs GUT, then fails (exit 3)
  when the log has `SCRIPT ERROR`, `Parse Error` or `Nothing was run`,
  because GUT exits 0 in those cases.
- `tests/unit/test_game_state.gd` (8 tests), `test_player.gd` (24),
  `test_coin.gd` (12).
- `tests/integration/test_main_scene.gd` (15).

The tests name the autoload and the scripts through `get_node_or_null`
and `load()`, so a missing piece gives a clear failed assert and not a parse
error.

### Command

```bash
.worktrees/_scratch/godot-trial/tests/run.sh
```

It uses `GODOT_PATH`, or `/Applications/Godot.app/Contents/MacOS/Godot`.
It runs `godot --headless --path . -s addons/gut/gut_cmdln.gd -gconfig=res://.gutconfig.json`.

### Result

| Run | Exit | Scripts | Tests | Passing | Failing | Asserts |
|-----|------|---------|-------|---------|---------|---------|
| real game, run 1 | 0 | 4 | 59 | 59 | 0 | 181 |
| real game, run 2 | 0 | 4 | 59 | 59 | 0 | 181 |
| stub game with no once-only guard in `Coin` | 1 | 4 | 59 | 57 | 2 | 181 |

Spec coverage: `GameState.add_score` (positive, zero, negative, signal
count), `reset`; `Player.apply_input` (straight and diagonal speed, zero
input, unnormalized input, dash multiplier, dash duration, cooldown from the
dash start, `can_dash()`, `is_dashing()`, the `dashed` signal, a dash press
in the cooldown, custom exports); `Coin` (Player body adds the value once and
frees the coin, `collected` signal, non-player bodies, once only); `main.tscn`
(main scene setting, 5 input actions, 1 `Player`, 5 `Coin`, collision shapes,
visible placeholder, `HUD/ScoreLabel`, label text follows `score_changed`
and `reset`, 1 real physics collision).

The seat checked that the tests can fail. Before the real build existed, it
ran the suite against a throw-away stub game in `/tmp`. The stub had no
once-only guard in `Coin`. The suite reported 2 failures
(`test_coin_collected_only_once`, `test_second_player_cannot_collect_same_coin`)
and exited 1. With the guard added, the suite passed 59 of 59.

### Bugs found

None in the real game: 0 game bugs, 0 test bugs left. The test files passed
`godot --headless --check-only --script` before the first run on the real game.

### Skills loaded

- `godot-testing` (with `references/running-tests.md` and `gut-reference.md`)

Not loaded: `godot-build-verify` (the gameplay seat ran the import, parse
and smoke proofs and reported them), `godot-debugging` (no failure to debug).

### Problems found in the skill texts

1. **`godot-testing` has no headless install step.** `gut-reference.md`
   gives AssetLib or `git submodule` and then "enable in Project Settings >
   Plugins". A headless seat has no editor. What worked: download the tag
   tarball and copy `addons/gut/`. The plugin does not need to be enabled for
   `gut_cmdln.gd`. The text does not say either fact.
2. **`wait_frames` is deprecated in GUT 9.7.1.** `gut-reference.md` line 207
   and `references/testing-patterns.md` line 157 use `await wait_frames(...)`.
   GUT prints `[DEPRECATED]: wait_frames has been replaced with
   wait_physics_frames ... wait_process_frames`. The text must say
   `wait_physics_frames` (for physics) or `wait_process_frames`.
3. **The CI snippet uses `${PIPESTATUS[0]}`.** `references/running-tests.md`
   uses it in the GUT job. It is bash only. In the default macOS shell (zsh)
   it is empty (zsh uses `pipestatus`). The skill gives no portable form. The
   seat redirected the log to a file and read `$?` instead, in `tests/run.sh`.
4. **No full exit-code recipe for a local run.** The skill says "also check
   the log" but gives the check only inside a CI YAML. A ready script like
   `tests/run.sh` (run, save the log, test the exit code, grep for
   `SCRIPT ERROR|Parse Error|Nothing was run`) belongs in the skill.
5. **No config file example.** `SKILL.md` lists `tests/gut_config.json` as
   optional. GUT reads `-gconfig=res://<file>` and the default
   `res://.gutconfig.json`. The skill shows no keys. The seat used `dirs`,
   `include_subdirs`, `should_exit`, `log_level`, `disable_colors`.
6. **Dead skill links.** `godot-testing` `SKILL.md` line 10 points to
   `godot-code-review` (the bundle has `code-review`) and `godot-export-pipeline`
   (not in the bundle).
7. **Autoloads in tests.** The skill does not say how a test reaches an
   autoload. The seat used `get_node_or_null("/root/GameState")`, which also
   avoids a parse error when the autoload is missing. The gameplay seat found
   the same autoload limit for `-s` scripts in its problem 1 and 3.

### Time

Start 05:33 MST (wall clock of the worker host). The suite takes 0.6 s on
the real game, plus the import in `tests/run.sh`.

## Appendix: the Coin Dash contract

A tiny Godot 4.7.2 GDScript 2D project at `.worktrees/_scratch/godot-trial/`.
g2 (gameplay) builds it. g3 (QA) tests it from this spec, not from g2's code.
The API below is the contract between them. Do not change a name.

### Input actions (in project.godot)

`move_left`, `move_right`, `move_up`, `move_down` (arrow keys and WASD),
`dash` (Space).

### Autoload `GameState` — `res://scripts/game_state.gd`

- `var score: int = 0`
- `signal score_changed(new_score: int)`
- `func add_score(n: int) -> void`: adds `n` and emits `score_changed`
  when `n > 0`. Does nothing (no signal) when `n <= 0`.
- `func reset() -> void`: sets `score` to 0 and emits `score_changed(0)`.

### `Player` — `res://scripts/player.gd`, `class_name Player`, extends `CharacterBody2D`

- `@export var speed: float = 200.0` (px/s)
- `@export var dash_multiplier: float = 3.0`
- `@export var dash_duration: float = 0.15` (s)
- `@export var dash_cooldown: float = 1.0` (s, counted from the dash start)
- `signal dashed`
- `func apply_input(direction: Vector2, dash_pressed: bool, delta: float) -> void`:
  - normalizes `direction` (a diagonal is not faster than a straight move;
    a zero vector gives zero velocity);
  - sets `velocity = direction.normalized() * speed`, times
    `dash_multiplier` while a dash is active;
  - starts a dash when `dash_pressed` and `can_dash()`; emits `dashed`;
  - advances the dash and cooldown timers by `delta`.
- `func can_dash() -> bool`: true when no cooldown is left.
- `func is_dashing() -> bool`
- `_physics_process(delta)` reads the input actions, calls `apply_input`,
  then `move_and_slide()`.

### `Coin` — `res://scripts/coin.gd`, `class_name Coin`, extends `Area2D`

- `@export var value: int = 1`
- `signal collected(value: int)`
- When a `Player` body enters: call `GameState.add_score(value)`, emit
  `collected(value)`, then `queue_free()`. Any other body: nothing happens.
- A coin can be collected only once.

### `res://scenes/main.tscn` (the main scene)

- 1 `Player` with a `CollisionShape2D` and a visible placeholder shape.
- 5 `Coin` nodes, each with a `CollisionShape2D`.
- A `Label` named `ScoreLabel` (under a `CanvasLayer` named `HUD`) that
  shows `Score: <n>` and updates on `GameState.score_changed`.
