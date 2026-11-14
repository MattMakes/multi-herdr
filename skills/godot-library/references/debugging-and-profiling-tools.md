# Debugging and profiling tools

Target engine: Godot 4.7. This shelf covers the measuring and debugging tools past the basics:
profilers, monitors, debugger messages, command line flags, loggers and outside profilers.

## Use this shelf when

- You must find which system, function or render pass uses the frame time.
- You need numbers from an exported build or from a device, not from the editor.
- You want your own graphs, debugger tab or error reports.
- You need a repeatable benchmark or a capture that is the same on every run.
- Object counts or memory grow and you must find what holds the objects.
- The cost is inside the engine, where the script profiler cannot see.

## How the engine works here

The editor debugger is a network link. The editor runs a debug server (TCP, port 6007 by default)
and the game connects to it with `--remote-debug`. Breakpoints, the remote scene tree, monitors
and profiler frames all travel over this link, so they work the same for a game on a phone.
`EngineDebugger` is the game side of the link and `EditorDebuggerPlugin` the editor side.

The Profiler tab times script functions and the engine's main steps. The Visual Profiler times
render passes on CPU and GPU, and nothing else. Monitors sample counters all the time, also when
their graph is closed. The two profilers are off until you press Start, because they slow the game.

Limits: the script profiler does not support C# (the docs name JetBrains dotTrace), time spent
waiting for servers may be missing from it (a known bug), and breakpoints do not work in `@tool`
scripts. Some `Performance` monitors return 0 in release exports.

Command line flags come in classes: all builds, debug builds only, editor only. An unknown flag
is ignored without a warning, so a debug-only flag on a release export does nothing, silently.

## Advanced techniques

### Read the Profiler by Self time

Inclusive time counts callees; Self time counts only the function body. A function high in
Inclusive and low in Self waits for something it calls. Switches: `--profiling` starts the script profiler from launch, the editor setting
`debugger/profile_native_calls` adds engine methods (slower), and
`debugger/profiler_frame_history_size` (default 3600) sets how far back the graph goes.

### Read the Visual Profiler with care

It shows only render work. A name
with "(Parallel)" means several GPU tasks ran together, so turning one off saves less than its
bar shows. Results depend on viewport size: compare runs at the same size. It does not work on
the Compatibility renderer on macOS. `--gpu-profile` (or
`ProjectSettings.debug/settings/stdout/print_gpu_profile`) prints the same data each second
without the editor.

### Measure render time in the running game

`RenderingServer.viewport_set_measure_render_time()` turns on per-viewport timers. Unlike FPS,
the values stay correct under V-Sync or a frame cap. Full CPU render cost is the sum of all
drawn viewports plus `get_frame_setup_time_cpu()`. Trap: under a low load the GPU lowers its
clock, so GPU time can read higher with a cap than without.

```gdscript
extends Node

var _vp: RID

func _ready() -> void:
	_vp = get_viewport().get_viewport_rid()
	RenderingServer.viewport_set_measure_render_time(_vp, true)
	Performance.add_custom_monitor("render/cpu", _cpu, [], Performance.MONITOR_TYPE_TIME)
	Performance.add_custom_monitor("render/gpu", _gpu, [], Performance.MONITOR_TYPE_TIME)

func _cpu() -> float:
	var ms: float = RenderingServer.viewport_get_measured_render_time_cpu(_vp)
	return (ms + RenderingServer.get_frame_setup_time_cpu()) / 1000.0

func _gpu() -> float:
	return RenderingServer.viewport_get_measured_render_time_gpu(_vp) / 1000.0
```

### Custom monitors as project instruments

`Performance.add_custom_monitor(id, callable, arguments, type)` adds a graph to Debugger >
Monitors. One slash in the id sets the category. The type formats the value:
`MONITOR_TYPE_QUANTITY`, `MONITOR_TYPE_MEMORY` (bytes), `MONITOR_TYPE_TIME` (give seconds, shown
as ms) and `MONITOR_TYPE_PERCENTAGE` (give a fraction). The editor calls the callable about once
per second, so return a stored value, not a fresh scan. Negative values are clamped to 0.
`Performance.get_custom_monitor()` works in exported builds, release too: build an in-game
overlay from the same monitors. Good design-side monitors: enemies alive, pool use, chunks loaded.

### Built-in monitors worth logging

`Performance.get_monitor()` with `TIME_PROCESS`, `TIME_PHYSICS_PROCESS` and
`TIME_NAVIGATION_PROCESS` splits the frame. `OBJECT_ORPHAN_NODE_COUNT` finds leaked nodes.
`PHYSICS_2D_COLLISION_PAIRS` and `PHYSICS_3D_ISLAND_COUNT` explain physics cost. The
`PIPELINE_COMPILATIONS_*` monitors count pipeline builds, the hitch at first draw.

### ObjectDB profiler (since 4.6)

Debugger > ObjectDB Profiler takes a snapshot of every live `Object` in the running game. Take
one after a level load, play, return to the same point, take a second, and use Diff Against.
Classes shows count deltas per class. Nodes lists orphan nodes after the tree. RefCounted shows
reference counts and detected cycles, which is how you find a resource that never frees. Objects
shows inbound and outbound references. Snapshots are `.odb_snapshot` files in
`user://objectdb_snapshots/`. Native memory of classes not exposed to scripts is not shown.

### Your own debugger channel

Send structured data from the game to an editor tab.

```gdscript
extends Node

func _ready() -> void:
	if EngineDebugger.is_active():
		EngineDebugger.register_message_capture("balance", _on_editor_message)
		EngineDebugger.send_message("balance:wave", [3, 41.5])

func _on_editor_message(message: String, data: Array) -> bool:
	if message == "set_speed":
		Engine.time_scale = float(data[0])
		return true
	return false
```

On the editor side an `EditorDebuggerPlugin` returns true from `_has_capture("balance")`, handles
`_capture(message, data, session_id)` and adds UI with `get_session(id).add_session_tab()`.
Trap: the game side gets the message without the prefix (`set_speed`), the editor side gets it
with the prefix (`balance:wave`). For a custom profiler, subclass `EngineProfiler` and register
it with `EngineDebugger.register_profiler()`. Procedure: `godot-debugging`.

### Viewport debug draw modes

Set `Viewport.debug_draw`. `DEBUG_DRAW_OVERDRAW` shows stacked pixels, `DEBUG_DRAW_WIREFRAME`
triangle density, `DEBUG_DRAW_UNSHADED` and `DEBUG_DRAW_LIGHTING` split material from light
cost, `DEBUG_DRAW_DISABLE_LOD` shows what mesh LOD saves, `DEBUG_DRAW_OCCLUDERS` the occlusion
buffer. Traps: overdraw and lighting
modes replace custom shaders, so vertex displacement vanishes. Several modes work only on
Forward+ or Mobile. Wireframe on Compatibility needs
`RenderingServer.set_debug_generate_wireframes(true)` before meshes load.

### Command line flags for measuring

| Flag | Builds | Use |
|---|---|---|
| `--disable-vsync` | all | Remove the display cap (a driver setting can still force it) |
| `--print-fps` | all | FPS to stdout each second |
| `--fixed-fps <fps>` | all | Same `delta` each frame, no real-time sync |
| `--frame-delay <ms>` | all | Imitate a slow CPU |
| `--quit-after <n>` | all | Stop after n iterations |
| `--profiling`, `--gpu-profile` | all | Script profiler on, GPU report to stdout |
| `--remote-debug <uri>` | all | Connect to an editor debug server |
| `--debug-collisions`, `--debug-paths` | debug | Draw shapes and paths |
| `--debug-navigation`, `--debug-avoidance` | debug | Draw navigation and avoidance |
| `--debug-canvas-item-redraw` | debug | Flash each 2D redraw request |
| `--benchmark`, `--benchmark-file <path>` | editor | Startup timings, console or JSON |

A repeatable run fixes the frame count and the step:

```sh
godot --path . --disable-vsync --fixed-fps 60 --quit-after 3600 --print-fps -- --bench=arena
```

Arguments after `--` reach the game through `OS.get_cmdline_user_args()`.

### Movie Maker mode for captures that repeat

`--write-movie out.avi` (or `.png`, `.ogv`) with `--fixed-fps` renders every frame with the same
step, however long a frame takes, so no frame is dropped. Use it for visual regression clips
and trailers of heavy scenes. `OS.has_feature("movie")` is true in this mode and `Engine.get_write_movie_path()` gives
the file. It is not for recording live play.

### Custom Logger and script backtraces (since 4.5)

Subclass `Logger` and pass it to `OS.add_logger()` in `_init()` of an autoload. `_log_error()`
gets file, line, text, type and `ScriptBacktrace` objects; `_log_message()` gets plain prints. Both are called from any thread, possibly at once: guard shared state with a `Mutex`.
Do not print from inside them. `push_error()` and `push_warning()` arrive in `_log_error()`,
not in `_log_message()`. GDScript errors carry a backtrace in editor and debug builds. For
release exports turn on `ProjectSettings.debug/settings/gdscript/always_track_call_stacks`.
`always_track_local_variables` also keeps locals for `Engine.capture_script_backtraces(true)`,
at about 50 bytes per local variable in every class. Procedure: `godot-debugging`.

### Debug on the device

Debug > Deploy with Remote Debug makes a one-click deploy connect back to the editor. Small
Deploy with Network Filesystem serves project files over the network, which cuts deploy time for
large games. Keep Debug Server Open accepts games started outside the editor. Only debug
templates can be debugged remotely.

### Outside profilers

- Sampling (VerySleepy on Windows, Hotspot on Linux, Instruments on Apple): needs an engine build
  with symbols, `production=yes debug_symbols=yes`, not stripped.
- Tracing (since 4.6, reported by the 4.6 beta 1 post): Tracy, Perfetto and Instruments
  signposts; the engine already marks its main events. Tracy: build with
  `profiler=tracy profiler_path=<dir>` and press Connect in Tracy before the game starts.
  Perfetto is the Android tracer: category `godot` is cheap, `godot_scripting` traces every
  script call and is slow, so use it to find one bad call in a hitch.
- Sanitizers need a custom build: `use_asan=yes` (about 2x slower), `use_ubsan=yes`,
  `use_tsan=yes`. Address, memory and thread sanitizers cannot be combined. Use them for
  engine, module and GDExtension memory bugs.

## Niche but useful

- The Evaluator tab is a REPL at a breakpoint: it reads members and locals and calls methods.
- A C++ crash backtrace needs a build with debug symbols. It is written to the session log: at
  next start, read the previous log in `user://logs/` for "Program crashed with signal".
- Release builds flush stdout only at exit. Dedicated servers that feed a log service set
  `ProjectSettings.application/run/flush_stdout_on_print`.
- The `breakpoint` keyword lives in the script, so it travels through version control.
- Debug > Customize Run Instances starts several instances, each with its own arguments and
  feature tags: one host and two clients from one key press.
- `--gpu-validation` turns on graphics API validation layers for driver-level errors.
- `print_verbose()` prints only with `--verbose`; such lines can stay in shipped code.

## Traps

- Profiling with V-Sync on, or in the editor build, and reading the result as shipping cost.
- A debug-only flag on a release template: ignored without a message.
- Outdated belief: release builds cannot give script stack traces. Since 4.5 they can.
- Trusting a 0 from a `Performance` monitor in release: some exist only in debug builds.

## Version notes

- 4.4: no change to the tools on this shelf was found in the docs.
- 4.5: `Logger` and `OS.add_logger()`; GDScript backtraces on errors; `ScriptBacktrace`;
  `always_track_call_stacks` for release.
- 4.6: ObjectDB Profiler tab; tracing profiler support (Tracy, Perfetto, Instruments).
- 4.7: official Perfetto export templates are published with each stable release, so an Android
  trace no longer needs an engine build.

## Related skills

- `godot-debugging` - step-by-step Logger, debugger plugin, overlay and leak-check procedures.
- `godot-perf-measure` - A/B measurement method on a noisy machine.
- `godot-optimization` - what to change once the tool has named the bottleneck.
- `godot-headless-runs` - running the engine from scripts and CI without a window.
- `godot-visual-verification` - screenshot and movie based checks.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/overview_of_debugging_tools.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/debugger_panel.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/the_profiler.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/custom_performance_monitors.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/objectdb_profiler.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/logging.html
- https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
- https://docs.godotengine.org/en/stable/tutorials/animation/creating_movies.html
- https://docs.godotengine.org/en/stable/engine_details/development/profiling/index.html
- https://docs.godotengine.org/en/stable/engine_details/development/profiling/tracy.html
- https://docs.godotengine.org/en/stable/engine_details/development/profiling/perfetto.html
- https://docs.godotengine.org/en/stable/engine_details/development/debugging/using_sanitizers.html
- Class reference 4.7: Performance, EngineDebugger, EditorDebuggerPlugin, Logger, Viewport
- https://godotengine.org/article/dev-snapshot-godot-4-6-beta-1/ (search summary only; page blocked)
