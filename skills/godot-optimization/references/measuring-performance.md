Adds measurement discipline: which build to measure, V-Sync and frame caps, the Visual Profiler for GPU time, render statistics from code, frame-time percentiles instead of average FPS, and a repeatable benchmark run from the command line; read it before you claim a speed-up, or when profiler numbers and player reports disagree.

# Measuring Performance Correctly

> ← Back to [SKILL.md](../SKILL.md). Reading the CPU profiler is in SKILL.md section 1; custom monitors and an in-game overlay are in **godot-debugging** [headless-and-in-game-diagnostics.md](../../godot-debugging/references/headless-and-in-game-diagnostics.md).

All code targets Godot 4.7. Command-line flags are from `godot --help` on 4.7.2.

---

## 1. Find Hot Spots in Debug, Prove Gains in Release

- The editor's **Profiler** and **Visual Profiler** need a debug run. Use
  them to find **where** time goes.
- A debug build adds checks that a release export does not have, so its
  absolute timings are higher. Prove the size of a gain with frame times
  from a **release export** of the same scene, before and after.
- Compare like with like: same scene, same camera path, same window size,
  same hardware, several runs.

## 2. Remove the Frame Cap While Measuring

With V-Sync on, a frame that takes 4 ms still reports 16.7 ms at 60 Hz, so
two very different builds look equal. Measure with:

- `--disable-vsync` on the command line (it cannot override a V-Sync that the
  driver forces), or `DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)`.
- `--max-fps 0` / `Engine.max_fps = 0` for no frame limit.

Restore the player's settings after the test.

## 3. CPU or GPU?

The CPU profiler shows script and engine time on the CPU. When it shows
little but the frame is still slow, the GPU is the limit.

- **Debugger → Visual Profiler** shows CPU and GPU time per render step
  (shadows, opaque pass, transparent pass, post-processing). Open it for a
  short capture only; leaving it recording costs frame time itself.
- `--gpu-profile` prints the most expensive GPU tasks of the run.
- Quick test: halve the render resolution (`Viewport.scaling_3d_scale = 0.5`).
  If the frame time drops a lot, the cost is per pixel (fill rate, post
  effects, overdraw). If it does not, look at draw calls, geometry or the CPU.

## 4. Render Statistics from Code

Read draw calls, objects and primitives for the frame to see what a change
does to the renderer:

```gdscript
extends Node

## Prints render statistics for the current viewport once per second.
var _timer: float = 1.0


func _process(delta: float) -> void:
	_timer -= delta
	if _timer > 0.0:
		return
	_timer = 1.0
	var vp := get_viewport()
	var draws := vp.get_render_info(Viewport.RENDER_INFO_TYPE_VISIBLE, Viewport.RENDER_INFO_DRAW_CALLS_IN_FRAME)
	var objects := vp.get_render_info(Viewport.RENDER_INFO_TYPE_VISIBLE, Viewport.RENDER_INFO_OBJECTS_IN_FRAME)
	var prims := vp.get_render_info(Viewport.RENDER_INFO_TYPE_VISIBLE, Viewport.RENDER_INFO_PRIMITIVES_IN_FRAME)
	var shadow_draws := vp.get_render_info(Viewport.RENDER_INFO_TYPE_SHADOW, Viewport.RENDER_INFO_DRAW_CALLS_IN_FRAME)
	print("draws %d (+%d shadow) | objects %d | primitives %d" % [draws, shadow_draws, objects, prims])
```

`RenderingServer.get_rendering_info()` gives the same totals for all
viewports together. A high shadow draw count often points at too many
shadow-casting lights or a long shadow distance.

## 5. Percentiles, Not Average FPS

An average of 90 FPS can hide a stutter every second. Record frame times and
report the median and the slow tail (95th and 99th percentile, and the worst
frame).

```gdscript
extends Node

## Records frame times for `duration` seconds, then prints percentiles.
@export var duration: float = 10.0
var _samples: PackedFloat32Array = PackedFloat32Array()
var _elapsed: float = 0.0
var _last_usec: int = 0


func _ready() -> void:
	_last_usec = Time.get_ticks_usec()


func _process(_delta: float) -> void:
	var now := Time.get_ticks_usec()
	var frame_ms := (now - _last_usec) / 1000.0
	_last_usec = now
	_samples.append(frame_ms)
	_elapsed += frame_ms / 1000.0
	if _elapsed >= duration:
		_report()
		set_process(false)


func _report() -> void:
	var sorted := _samples.duplicate()
	sorted.sort()
	var n := sorted.size()
	print("frames %d | median %.2f ms | p95 %.2f ms | p99 %.2f ms | worst %.2f ms" % [
		n, sorted[n / 2], sorted[int(n * 0.95)], sorted[int(n * 0.99)], sorted[n - 1]])
```

Skip the first second or two after a scene loads: shader compilation and
resource loading inflate those frames.

## 6. A Repeatable Benchmark Run

Make a benchmark scene that plays a fixed camera path or a scripted fight,
records frame times as above, writes them to `user://`, and quits. Run it
the same way each time:

```bash
# Release export, V-Sync off, no frame cap. Arguments after "--" are the game's own.
./MyGame.x86_64 --disable-vsync --max-fps 0 -- --bench=city
```

```gdscript
extends Node

## Main scene hook: switch to a benchmark scene when asked on the command line.
const BENCH_SCENES: Dictionary[String, String] = {
	"city": "res://bench/bench_city.tscn",
}


func _ready() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--bench="):
			var key := arg.trim_prefix("--bench=")
			if BENCH_SCENES.has(key):
				get_tree().change_scene_to_file.call_deferred(BENCH_SCENES[key])
```

`--fixed-fps <n>` is useful for a deterministic gameplay replay, but it
disables real-time sync, so do not use it to measure speed. `--frame-delay`
only simulates a slow CPU. `--benchmark` and `--benchmark-file` time engine
startup and loading in editor builds; they are not a frame-time benchmark.

## 7. Micro-Benchmarks

For a single function, time many calls with `Time.get_ticks_usec()` (not
`get_ticks_msec()`) and compare medians of several rounds. Warm up first,
keep the result of the work so it is not dead code, and run in a release
build when the numbers matter. Measuring a 2 µs function once tells you
nothing; measuring it 100,000 times does.
