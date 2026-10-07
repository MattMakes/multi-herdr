# Live check: Godot seats on a real task

A Godot fleet seat does 1 real task on Godot 4.7.2 and records the result.
The `godot-gameplay-programmer` seat builds "Coin Dash", a small 2D GDScript
project, and proves it with the commands in its expected skills. The
`godot-qa-engineer` seat then tests the same project from the feature spec.
Godot wave step 5 (status item 18).

## How to run

The task is the plan `ai_docs/plans/godot-gaps/g2-gameplay-trial.md`, with
the API contract in `ai_docs/plans/godot-gaps/feature-spec.md`. An
orchestrator assigns it to a `godot-gameplay-programmer` worker. The worker
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

## qa-engineer

pending
