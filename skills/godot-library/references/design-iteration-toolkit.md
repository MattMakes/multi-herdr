# Design iteration toolkit

Target engine: Godot 4.7. Engine features that shorten the loop between a design idea and
seeing it run: designer-facing data, live editing, debug surfaces, capture and build variants.

## Use this shelf when

- A designer or a balance pass needs to change numbers without touching code.
- Testing one moment of the game costs minutes of menu clicks and walking.
- You need a trailer, a regression screenshot set or a repeatable capture.
- You ship a demo or playtest build that differs from the full game by a few settings.

## How the engine works here

- The editor and the running game are two processes joined by a debugger socket. Everything
  "live" (remote tree, script reload, synced scene edits, monitors, the Game screen tools)
  travels over that socket. It works the same on a phone or a second machine (Debug > Deploy
  with Remote Debug; Debug > Keep Debug Server Open accepts games started outside the editor).
- An exported property is a serialized value plus an Inspector hint. A `Resource` script
  with exports is therefore a typed, validated data sheet with an editor UI for free.
- Project settings are layered: `project.godot`, then feature-tag variants of a key
  (`key.tag`), then `override.cfg` next to the project or the exported binary. Most settings
  are read once at startup.
- Time is engine-controlled: `Engine.time_scale` scales every `delta`, `--fixed-fps` cuts the
  link to the real clock. That is what makes slow motion, fast-forward and frame-exact
  capture possible.

## Advanced techniques

### Tuning sheets as custom Resources

One `Resource` subclass per tunable concept, saved as `.tres`, referenced by scenes. Group
and bound the fields so the Inspector guides the designer and rejects nonsense.

```gdscript
@tool
class_name EnemyTuning
extends Resource

@export_group("Movement")
@export_range(0.0, 20.0, 0.1, "suffix:m/s") var speed: float = 4.0
@export var accel_over_time: Curve
@export_group("Combat")
@export_range(1, 500, 1, "or_greater") var health: int = 40
@export_range(0.05, 5.0, 0.05, "exp", "suffix:s") var attack_interval: float = 1.0
@export var damage: int = 5
@export_tool_button("Print DPS", "Callable") var print_dps_action: Callable = print_dps

func print_dps() -> void:
	print(damage / attack_interval)
```

`@export_tool_button` (since 4.4) turns a `Callable` into an Inspector button: validate the
sheet, simulate 100 fights, rebuild a preview. `@export_subgroup`, `@export_category`,
`@export_enum`, `@export_flags` and `@export_custom` (raw hint strings such as `suffix:`)
cover the rest. Cost: every scene using the same `.tres` shares one instance; mutate a
`duplicate()` at runtime, never the shared file. Trap: since 4.4 a `Curve` clamps points to
`min_value`..`max_value`; widen the range before authoring values above 1.

### Curves and gradients in place of formulas

Export a `Curve` for anything shaped over a 0..1 input (damage falloff, XP per level, spawn
rate over a wave) and read it with `Curve.sample_baked`. Export a `Gradient` for color
over time and read `Gradient.sample`. Designers reshape the response without a code change,
and the same resource can drive particles and UI.

### Live editing a running game

- Debug > Synchronize Scene Changes: edits to the Local tree are replicated into the running
  game and are kept on disk. Edits made in the Remote tree or by picking a node in the Game
  screen are lost when the game stops.
- Debug > Synchronize Script Changes: saved scripts are reloaded in the running game.
  Code that already ran (`_ready`) does not run again; plan a debug "reinit" action.
- Game screen (since 4.4): Pause (F9) stops process and physics frames but keeps rendering
  and input; Frame Advance (F10) steps one frame while paused; the speed menu multiplies
  `Engine.time_scale` and scales the physics tick rate with it, so fast-forward stays stable;
  the 2D and 3D interaction modes let you click a node in the running game and edit it in
  the Inspector, with a free debug camera.
- Embedding can be a floating window, the main editor window, or off. The game is always a
  separate process.

### Jump straight to a state

Read launch arguments placed after `--` with `OS.get_cmdline_user_args()` and act on them in
an autoload: load a level, grant items, set a seed. Put the same arguments in Debug >
Customize Run Instances (per-instance arguments and feature tags) to start two clients in
different states with one key press.

```gdscript
extends Node

func _ready() -> void:
	for arg: String in OS.get_cmdline_user_args():
		if arg.begins_with("--level="):
			var path: String = "res://levels/%s.tscn" % arg.trim_prefix("--level=")
			get_tree().change_scene_to_file.call_deferred(path)
```

`--scene <path or uid>` starts any scene directly. Editable test rooms (one small scene per
mechanic) double as regression scenes for automated runs.

### Debug overlays without building UI

`Performance.add_custom_monitor("Game/Enemies", callable)` adds a graph to Debugger >
Monitors; one slash sets the category. Since 4.6 a fourth argument sets the unit type
(`Performance.MonitorType`). The callable is polled by the debugger, so keep it cheap. Read
any monitor in game with `Performance.get_custom_monitor` to feed an in-game overlay.

### A cheat console over the debugger channel

`EngineDebugger.register_message_capture("cheat", callable)` receives messages named
`cheat:<something>` sent by an editor plugin (`EditorDebuggerPlugin` with
`EditorDebuggerSession.send_message`). The callable gets the message and a data array and
returns `true` when handled. This gives an editor dock with "kill all" or "skip wave"
buttons that works on a remote device too, with no cheat UI in the game. Guard
with `EngineDebugger.is_active()`; nothing arrives in a release build without a debugger.

### Data-driven formulas with Expression

`Expression` parses a string once and executes it many times with named inputs. Use it for
formulas that live in data (`"base * (1 + 0.1 * level)"`), dialogue conditions and console
commands. It is independent of GDScript and has no statements or assignments.

```gdscript
var formula: Expression = Expression.new()
var err: Error = formula.parse("base * (1.0 + 0.1 * level)", ["base", "level"])
if err == OK:
	var value: Variant = formula.execute([10.0, 5])
	if not formula.has_execute_failed():
		print(value)
```

Trap: integer division applies when both operands are `int`. Pass a `base_instance` only for
trusted text; it exposes that object's methods to the expression author.

### Hot-reloaded tuning files

For values that change many times per session, keep a `ConfigFile` (or JSON) outside the
pack, poll `FileAccess.get_modified_time` once per second in debug builds and re-read on
change. Text editors and spreadsheets become the tuning UI and the game never restarts. For
`.tres` sheets, `ResourceLoader.load(path, "", ResourceLoader.CACHE_MODE_REPLACE)`
refreshes the cached instance in place, so every holder sees the new values.

### Build variants with feature tags and overrides

Add a custom tag (`demo`, `playtest`) in an export preset, branch with
`OS.has_feature("demo")`, and give any project setting a per-tag value in the Project
Settings dialog. Read tagged settings with `ProjectSettings.get_setting_with_override`;
plain `get_setting` ignores tags. Trap: custom tags are not active when running from the
editor; set them per run instance in Customize Run Instances. `override.cfg` beside an
exported binary changes settings without a re-export (playtest resolution, a test server
address); `ProjectSettings.save_custom` can write it.

### Movie Maker for trailers and deterministic capture

`--write-movie out.avi` (or the movie reel button) renders at a fixed step regardless of
real speed: frames are never dropped, audio stays in sync. Formats: `.ogv`, `.avi` (MJPEG),
`.png` sequence plus WAV. Add `--fixed-fps 30`, `--resolution 1280x720`, `--quit-after N`.
A String metadata `movie_file` on a scene root sets the output per scene. In code,
`OS.has_feature("movie")` lets you raise quality settings only while recording. The same
mode gives automated screenshot runs: a `.png` sequence of a scripted camera path can be
compared frame by frame between builds.

### Scene inheritance and placeholders for variants

An inherited scene stores only the differences from its base: enemy variants, level themes
and UI skins stay in sync with the base. "Load As Placeholder" on an instanced scene swaps
it for an `InstancePlaceholder` at runtime; `create_instance()` loads it on demand. Use it
for optional set pieces in a large level. A placeholder has no transform until replaced.

## Niche but useful

- `@tool` scripts with `Engine.is_editor_hint()` draw gameplay previews in the editor
  (patrol paths, spawn radii, jump arcs) so layout is judged without running.
- Editor feature profiles (Editor > Manage Editor Features) hide the 3D editor, the script
  editor or single node classes; export a `.profile` file per role.
- CSG nodes and `GridMap` block out a level in minutes; since 4.4 a CSG tree bakes to a
  `MeshInstance3D` when the layout settles.
- `--time-scale`, `--max-fps`, `--frame-delay <ms>` (simulate a slow CPU), `--print-fps`.
- A custom `Logger` (since 4.5, `OS.add_logger`) is a telemetry hook: forward errors and
  design events from playtest builds to a file or a server.
- `Timer.ignore_time_scale` and `Tween.set_ignore_time_scale` keep UI moving in slow motion.
- Debug > Small Deploy with Network Filesystem makes device test runs start fast.

## Traps

- `Engine.time_scale` does not change audio speed (`AudioServer.playback_speed_scale` does)
  and does not raise the physics tick rate by itself; large values make physics coarse.
- Remote-tree edits feel saved and are not.
- A `--` in Main Run Args and another in Launch Arguments yields a literal `--` in the user
  argument list.
- Reading an exported value in `_init` returns the default, not the Inspector value.

## Version notes

- 4.4: Game screen with embedding, pause, frame advance, node picking;
  `@export_tool_button`; `Curve` range enforced; CSG bake to mesh.
- 4.5: `Logger`; script backtraces; `@export_file_path`.
- 4.6: monitor unit type on `Performance.add_custom_monitor`; ObjectDB Profiler.
- 4.7: new projects stretch with `canvas_items` and `expand`; captures at other sizes scale.

## Related skills

- `godot-resource-pattern` - how to structure custom Resource data.
- `godot-balance-model-data` and `godot-bot-balance-loop` - balance data and bot runs.
- `godot-headless-runs` - running the game from the command line in CI.
- `godot-visual-verification` - screenshot checks. `godot-debugging` - debugger procedures.
- `godot-addon-development` - writing the editor plugin side of a debug dock.
- Sibling shelves: `debugging-and-profiling-tools`, `editor-tools-plugins`,
  `determinism-and-simulation`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/editor/game_embedding.html
- https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
- https://docs.godotengine.org/en/stable/tutorials/editor/managing_editor_features.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/overview_of_debugging_tools.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/custom_performance_monitors.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/evaluating_expressions.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_exports.html
- https://docs.godotengine.org/en/stable/tutorials/export/feature_tags.html
- https://docs.godotengine.org/en/stable/tutorials/animation/creating_movies.html
- https://docs.godotengine.org/en/stable/tutorials/plugins/running_code_in_the_editor.html
