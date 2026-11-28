# Measure: the 300 fps harness

This page covers measurement at 200 to 300 fps on Godot 4.7: the benchmark
scenario, the take file, the GDScript recorder, the regression rule and 16
measurement techniques in rank order. Measurement saves no frame time by
itself. Each technique names the thread whose cost it can show.

The source report was checked on 2026-11-24 with
`Godot 4.7.2.stable.official.ed1daf0bf`, its `--help` output and the 4.7.2
engine source. No game benchmark was run for it. Each technique gives its
evidence label.

| Target | Frame budget |
| --- | ---: |
| 200 fps | 5.00 ms |
| 240 fps | 4.17 ms |
| 300 fps | 3.33 ms |

## Scenario contract

Make 1 release-export benchmark scene for each representative workload. The
scene uses a fixed seed, a fixed start save or state, scripted input or a
camera path, a fixed resolution, and a duration in seconds, not a fixed frame
count. Run it in a foreground window for rendering measurements. A headless
run is valid only for deterministic simulation or script micro-benchmarks,
because the headless display driver uses the dummy renderer.

The scene has 3 phases:

1. Load and settle until every resource is ready.
2. Warm up for 5 s without recording.
3. Record for 30 s, write the take file atomically, and quit with `0`. Quit
   with a non-zero code on an invalid configuration or missing samples.

The command for 1 take of a release export:

```bash
./MyGame --windowed --resolution 1920x1080 --disable-vsync --max-fps 0 -- --hfr-bench=city --warmup-seconds=5 --sample-seconds=30 --hfr-out=/absolute/path/city-new-01.json
```

The harness sets or asserts these values:

- `Engine.max_fps == 0`. `--max-fps 0` overrides `application/run/max_fps`.
- V-Sync is disabled, and the frame times do not cluster at the display
  period. A driver can still force V-Sync on.
- `OS.low_processor_usage_mode == false`. Its default 6,900 microsecond sleep
  is about 145 fps, so a 200 to 300 fps test is impossible with it on.
- `application/run/frame_delay_msec == 0`, no `--fixed-fps`, and real-time
  synchronization stays on.
- A foreground window, a fixed renderer and driver, a fixed window size, fixed
  graphics settings, and no editor-hosted run.

The precedence is verified in source: Godot loads `application/run/max_fps`,
then applies the command-line value when there is one, and loads the
low-processor mode and its sleep separately
([4.7.2 `main.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L2251-L2268)).
The OS takes the larger of the low-processor sleep and the max-FPS delay
([4.7.2 `os.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L690-L735)).

## Take file contract

The recorder writes 1 JSON document per take. It keeps the raw frames, so
`hfr_bench.py summarize` can recompute the summary without a new run. All
frame times are in milliseconds.

```json
{
  "schema_version": 1,
  "provenance": {
    "git_sha": "...",
    "godot_version": "4.7.2.stable.official.ed1daf0bf",
    "build": "release",
    "scenario": "city",
    "renderer": "forward_plus",
    "rendering_driver": "metal",
    "resolution": [1920, 1080],
    "arguments": ["--disable-vsync", "--max-fps", "0"],
    "thermal_start": "nominal",
    "thermal_end": "nominal"
  },
  "validity": {
    "foreground": true,
    "vsync_period_detected": false,
    "gpu_timer_available": true,
    "warmup_seconds": 5,
    "sample_seconds": 30
  },
  "summary": {
    "frames": 8912,
    "frame_ms_p50": 3.21,
    "frame_ms_p95": 3.47,
    "frame_ms_p99": 3.62,
    "worst_frame_ms": 5.88,
    "one_percent_low_fps": 276.24,
    "over_3_333_ms": 921,
    "over_4_167_ms": 17
  },
  "frames": [
    {"i": 0, "frame_ms": 3.18, "render_cpu_ms": 0.41, "render_gpu_ms": 2.62, "draw_calls": 812, "primitives": 145200}
  ]
}
```

- `frame_ms` is the callback-to-callback `Time.get_ticks_usec()` delta.
- `render_cpu_ms` and `render_gpu_ms` are the sums over every viewport drawn
  in the frame, with `viewport_set_measure_render_time` on. A timer that is
  not available is `null`, never `0`. Render information is not valid until
  at least 2 frames have rendered, and GPU power-state changes can distort GPU
  times under a cap
  ([RenderingServer class](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html#class-renderingserver-method-viewport-get-measured-render-time-gpu)).
- Percentiles use nearest rank: sort `frame_ms` ascending and take index
  `ceil(p * n) - 1`. The 1% low frame time is `frame_ms_p99`, and
  `one_percent_low_fps = 1000 / frame_ms_p99`. Never average or sort FPS
  values: FPS is the reciprocal of frame time.

## Recorder

Add this script as an autoload named `HfrRecorder`. It does nothing unless the
user arguments hold `--hfr-bench=<scenario>`. The main scene reads its
`active` and `scenario` values, loads that scenario, and calls `begin()` when
the scenario has settled. Warm-up starts at `begin()`. The recorder quits the
game itself.

Exit codes: `0` when the take file is written, `2` on an invalid
configuration (a cap, a bad argument, a headless or editor-hosted run), `3`
when no frame was sampled, and `4` when the take file cannot be written. The
process prints the reason with `push_error()`.

Godot has no thermal API. The recorder writes `"unknown"` for
`thermal_start` and `thermal_end`, and reads `git_sha` from the `HFR_GIT_SHA`
environment variable. The runner records thermal state on Apple platforms
with `ProcessInfo.thermalState` through a native helper.

The V-Sync check is a heuristic: it flags a take when at least 90% of the
frames are within 2% of 1 or 2 display periods. A game that runs uncapped at
exactly the display rate can trip it. Then take the run on a display with a
different refresh rate.

```gdscript
extends Node

const EXIT_INVALID_CONFIG := 2
const EXIT_MISSING_SAMPLES := 3
const EXIT_WRITE_FAILED := 4
const VSYNC_TOLERANCE := 0.02
const VSYNC_SHARE := 0.9
const REAL_TIME_TOLERANCE := 0.05

enum Phase { IDLE, SETTLE, WARMUP, SAMPLE, DONE }

var active := false
var scenario := ""

var _warmup_seconds := 5.0
var _sample_seconds := 30.0
var _out_path := ""
var _phase := Phase.IDLE
var _phase_start_usec := 0
var _last_usec := 0
var _warmup_delta_seconds := 0.0
var _warmup_measured_seconds := 0.0
var _count := 0
var _frame_ms := PackedFloat64Array()
var _cpu_ms := PackedFloat64Array()
var _gpu_ms := PackedFloat64Array()
var _draw_calls := PackedInt64Array()
var _primitives := PackedInt64Array()
var _viewports: Array[RID] = []
var _cpu_seen := false
var _gpu_seen := false
var _foreground := true


func _ready() -> void:
	process_mode = Node.PROCESS_MODE_ALWAYS
	_read_arguments()
	if not active:
		set_process(false)
		return
	var problems := _cap_problems()
	if not problems.is_empty():
		_fail(EXIT_INVALID_CONFIG, "; ".join(problems))
		return
	_phase = Phase.SETTLE


func begin() -> void:
	if _phase != Phase.SETTLE:
		return
	var root := get_tree().root
	_viewports.clear()
	_viewports.append(root.get_viewport_rid())
	for node: Node in root.find_children("*", "SubViewport", true, false):
		_viewports.append((node as SubViewport).get_viewport_rid())
	for rid: RID in _viewports:
		RenderingServer.viewport_set_measure_render_time(rid, true)
	_grow(ceili(_sample_seconds * 1000.0))
	_phase = Phase.WARMUP
	_phase_start_usec = Time.get_ticks_usec()
	_last_usec = _phase_start_usec


func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_FOCUS_OUT and _phase in [Phase.WARMUP, Phase.SAMPLE]:
		_foreground = false


func _process(delta: float) -> void:
	if _phase != Phase.WARMUP and _phase != Phase.SAMPLE:
		return
	var now := Time.get_ticks_usec()
	var frame_ms := float(now - _last_usec) / 1000.0
	_last_usec = now
	var cpu := 0.0
	var gpu := 0.0
	for rid: RID in _viewports:
		cpu += RenderingServer.viewport_get_measured_render_time_cpu(rid)
		gpu += RenderingServer.viewport_get_measured_render_time_gpu(rid)
	var elapsed_usec := now - _phase_start_usec
	if _phase == Phase.WARMUP:
		_warmup_delta_seconds += delta
		_cpu_seen = _cpu_seen or cpu > 0.0
		_gpu_seen = _gpu_seen or gpu > 0.0
		if elapsed_usec >= int(_warmup_seconds * 1000000.0):
			_end_warmup(now, elapsed_usec)
		return
	_store(frame_ms, cpu, gpu)
	if elapsed_usec >= int(_sample_seconds * 1000000.0):
		_phase = Phase.DONE
		_finish()


func _end_warmup(now: int, elapsed_usec: int) -> void:
	_warmup_measured_seconds = float(elapsed_usec) / 1000000.0
	var drift := absf(_warmup_delta_seconds - _warmup_measured_seconds)
	if drift > _warmup_measured_seconds * REAL_TIME_TOLERANCE:
		_fail(EXIT_INVALID_CONFIG, "process delta is not real time: --fixed-fps or a time scale is set")
		return
	_phase = Phase.SAMPLE
	_phase_start_usec = now


func _read_arguments() -> void:
	for argument: String in OS.get_cmdline_user_args():
		var parts := argument.split("=", true, 1)
		if parts.size() != 2:
			continue
		match parts[0]:
			"--hfr-bench":
				scenario = parts[1]
				active = true
			"--warmup-seconds":
				_warmup_seconds = parts[1].to_float() if parts[1].is_valid_float() else -1.0
			"--sample-seconds":
				_sample_seconds = parts[1].to_float() if parts[1].is_valid_float() else -1.0
			"--hfr-out":
				_out_path = parts[1]


func _cap_problems() -> PackedStringArray:
	var problems := PackedStringArray()
	if scenario.is_empty():
		problems.append("--hfr-bench is empty")
	if not _out_path.is_absolute_path():
		problems.append("--hfr-out is not an absolute path")
	if _warmup_seconds < 0.0 or _sample_seconds <= 0.0:
		problems.append("--warmup-seconds or --sample-seconds is out of range")
	if Engine.max_fps != 0:
		problems.append("Engine.max_fps is %d, not 0" % Engine.max_fps)
	if OS.low_processor_usage_mode:
		problems.append("OS.low_processor_usage_mode is on")
	if int(ProjectSettings.get_setting("application/run/frame_delay_msec", 0)) != 0:
		problems.append("application/run/frame_delay_msec is not 0")
	if not is_equal_approx(Engine.time_scale, 1.0):
		problems.append("Engine.time_scale is not 1")
	if DisplayServer.window_get_vsync_mode() != DisplayServer.VSYNC_DISABLED:
		problems.append("V-Sync is not disabled")
	if DisplayServer.get_name() == "headless":
		problems.append("a headless run uses the dummy renderer")
	if OS.has_feature("editor"):
		problems.append("an editor-hosted run is not a release take")
	return problems


func _grow(capacity: int) -> void:
	_frame_ms.resize(capacity)
	_cpu_ms.resize(capacity)
	_gpu_ms.resize(capacity)
	_draw_calls.resize(capacity)
	_primitives.resize(capacity)


func _store(frame_ms: float, cpu: float, gpu: float) -> void:
	if _count == _frame_ms.size():
		_grow(maxi(_count * 2, 1024))
	_frame_ms[_count] = frame_ms
	_cpu_ms[_count] = cpu
	_gpu_ms[_count] = gpu
	_draw_calls[_count] = RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_DRAW_CALLS_IN_FRAME)
	_primitives[_count] = RenderingServer.get_rendering_info(RenderingServer.RENDERING_INFO_TOTAL_PRIMITIVES_IN_FRAME)
	_count += 1


func _finish() -> void:
	if _count == 0:
		_fail(EXIT_MISSING_SAMPLES, "no frame samples")
		return
	var sampled_ms := 0.0
	for i in _count:
		sampled_ms += _frame_ms[i]
	var document := {
		"schema_version": 1,
		"provenance": _provenance(),
		"validity": {
			"foreground": _foreground,
			"vsync_period_detected": _vsync_period_detected(),
			"gpu_timer_available": _gpu_seen,
			"warmup_seconds": _warmup_measured_seconds,
			"sample_seconds": sampled_ms / 1000.0,
		},
		"summary": _summary(),
		"frames": _frame_rows(),
	}
	var error := _write_atomically(JSON.stringify(document, "", false, true))
	if error != OK:
		_fail(EXIT_WRITE_FAILED, "cannot write %s: %s" % [_out_path, error_string(error)])
		return
	get_tree().quit(0)


func _provenance() -> Dictionary:
	var info := Engine.get_version_info()
	var version := "%d.%d.%d.%s.%s.%s" % [
		info["major"], info["minor"], info["patch"], info["status"], info["build"], str(info["hash"]).left(9),
	]
	var size := get_window().size
	return {
		"git_sha": OS.get_environment("HFR_GIT_SHA") if OS.has_environment("HFR_GIT_SHA") else "unknown",
		"godot_version": version,
		"build": "debug" if OS.is_debug_build() else "release",
		"scenario": scenario,
		"renderer": RenderingServer.get_current_rendering_method(),
		"rendering_driver": RenderingServer.get_current_rendering_driver_name(),
		"resolution": [size.x, size.y],
		"arguments": Array(OS.get_cmdline_args()),
		"thermal_start": "unknown",
		"thermal_end": "unknown",
	}


func _summary() -> Dictionary:
	var sorted := _frame_ms.slice(0, _count)
	sorted.sort()
	var p99 := _nearest_rank(sorted, 0.99)
	var over_300 := 0
	var over_240 := 0
	for value: float in sorted:
		if value > 1000.0 / 300.0:
			over_300 += 1
		if value > 1000.0 / 240.0:
			over_240 += 1
	return {
		"frames": _count,
		"frame_ms_p50": _nearest_rank(sorted, 0.50),
		"frame_ms_p95": _nearest_rank(sorted, 0.95),
		"frame_ms_p99": p99,
		"worst_frame_ms": sorted[_count - 1],
		"one_percent_low_fps": 1000.0 / p99,
		"over_3_333_ms": over_300,
		"over_4_167_ms": over_240,
	}


static func _nearest_rank(sorted: PackedFloat64Array, p: float) -> float:
	var index := clampi(ceili(p * float(sorted.size())) - 1, 0, sorted.size() - 1)
	return sorted[index]


func _vsync_period_detected() -> bool:
	var refresh := DisplayServer.screen_get_refresh_rate()
	if refresh <= 0.0:
		return false
	var period := 1000.0 / refresh
	var quantized := 0
	for i in _count:
		var frame_ms := _frame_ms[i]
		if absf(frame_ms - period) <= period * VSYNC_TOLERANCE or absf(frame_ms - 2.0 * period) <= period * VSYNC_TOLERANCE:
			quantized += 1
	return quantized >= _count * VSYNC_SHARE


func _frame_rows() -> Array[Dictionary]:
	var rows: Array[Dictionary] = []
	for i in _count:
		rows.append({
			"i": i,
			"frame_ms": _frame_ms[i],
			"render_cpu_ms": _timer_value(_cpu_ms[i], _cpu_seen),
			"render_gpu_ms": _timer_value(_gpu_ms[i], _gpu_seen),
			"draw_calls": _draw_calls[i],
			"primitives": _primitives[i],
		})
	return rows


static func _timer_value(value: float, available: bool) -> Variant:
	if available:
		return value
	return null


func _write_atomically(text: String) -> Error:
	var directory_error := DirAccess.make_dir_recursive_absolute(_out_path.get_base_dir())
	if directory_error != OK:
		return directory_error
	var temporary := _out_path + ".tmp"
	var file := FileAccess.open(temporary, FileAccess.WRITE)
	if file == null:
		return FileAccess.get_open_error()
	file.store_string(text)
	file.close()
	return DirAccess.rename_absolute(temporary, _out_path)


func _fail(code: int, reason: String) -> void:
	push_error("hfr-bench: %s" % reason)
	_phase = Phase.DONE
	get_tree().quit(code)
```

The recorder keeps every sample in pre-sized packed arrays and builds the
JSON only after the capture, so it does not allocate per frame while it
records. `OS.get_cmdline_args()` does not list the flags that the engine
consumes, such as `--fixed-fps`. The recorder therefore detects a fixed frame
rate from its effect: during warm-up, the summed process `delta` must match
the wall time within 5%. For the same reason, `provenance.arguments` holds
only the arguments that the engine did not consume. The cap asserts cover the
engine flags. Viewports that the game adds after `begin()` are not measured.
Add every `SubViewport` of the scenario before `begin()`.

Run evidence: on 2026-11-25 this recorder ran in a macOS release export of
Godot 4.7.2 (Forward+, Metal, Apple M3 Ultra). It wrote a valid take file and
quit with `0`. It quit with `2` with V-Sync on, with `--fixed-fps 60`, and with
a relative `--hfr-out` path. On that host the Metal GPU timer returned 0, so
`gpu_timer_available` was `false` and every `render_gpu_ms` was `null`.

## Regression rule

`hfr_bench.py compare` runs 1 untimed warm-up take and 5 interleaved baseline
and candidate pairs on a dedicated machine. A pair is invalid when either
take lost foreground status, ended with a thermal state above nominal,
detected a V-Sync period, differs from its partner in renderer, driver,
resolution or arguments, has fewer than 20 s of valid samples, or has no take
file. Do not run another engine process during a compare.

The verdict is a **frame-time regression** only when all these conditions are
true:

1. At least 3 pairs are valid.
2. At least 4 of 5 valid pair p99 deltas are positive, or all deltas are
   positive when only 3 or 4 pairs are valid.
3. The median paired p99 delta is greater than `max(0.10 ms, baseline_p99 * 0.05)`.

The 0.10 ms floor stops timer noise from taking review time. The 5% relative
threshold scales to slower targets.

The verdict is a **budget failure** when the median candidate take p99 is
above `1000 / target` ms, with `--target-fps` an integer from 200 to 300
(default 240). A budget failure does not need a baseline regression. The
script also prints the 240 fps (4.167 ms) and 300 fps (3.333 ms) budget
results as information lines. They do not change the verdict or the exit code.

| Verdict line | Exit code |
| --- | ---: |
| `PASS` | 0 |
| `FRAME_TIME_REGRESSION` | 1 |
| `<target>_FPS_BUDGET_FAILURE`, for example `240_FPS_BUDGET_FAILURE` | 1 |
| `INFRASTRUCTURE_INVALID` (fewer than 3 valid pairs) | 2 |

`INFRASTRUCTURE_INVALID` is never a pass. Keep the summary JSON and the raw
frames of every take that is not a pass.

## Escalation ladder

Go to the next rung only when the rung before it has narrowed the question.
Each rung names a timestamp or a frame range from the harness, and returns a
function, a pass, a wait or a draw event that explains it.

1. Harness counters: wall frames, summed viewport render CPU and GPU, draw
   calls and primitives (techniques 1 to 4).
2. Built-in profilers: the script Profiler for main-thread and physics-thread
   script cost, and the Visual Profiler or `--gpu-profile` for render passes
   (techniques 5 and 6).
3. A native sampling profiler, when wall time is high but every built-in
   category is low (technique 7).
4. Tracy zones, only around the native region that sampling did not explain
   (technique 8).
5. A GPU frame capture of 1 representative slow frame (technique 9).

## Ranked techniques

### 1. Fixed, windowed scenario harness

- **What:** Replay 1 deterministic 30-second representative scene in a foreground release-export window, after a separate warm-up.
- **Why/thread:** It shows the largest main, render, physics or GPU cost under real rendering. The harness itself saves no time.
- **Measure:** Raw wall-frame samples, summed viewport render CPU and GPU times, draw calls, primitives and scenario invariants.
- **Version:** Godot 4.7. The design does not depend on the version. The APIs and flags were checked on 4.7.2.
- **Source:** [Official Godot benchmark project](https://github.com/godotengine/godot-benchmarks) and the local 4.7.2 `--help`.
- **Evidence:** Verified from source and help. Not verified at run time against a game.
- **Trap:** Headless mode removes the renderer. A fixed frame count covers less gameplay as the candidate gets faster.

### 2. Wall-frame percentiles and 1% low

- **What:** Report nearest-rank p50, p95 and p99, the worst frame, threshold counts, and `1% low FPS = 1000 / p99_ms` from raw frame durations.
- **Why/thread:** It shows rare work on any thread that average FPS hides, and points the work at the slow tail.
- **Measure:** `Time.get_ticks_usec()` between consecutive `_process` callbacks. Keep every sample.
- **Version:** Godot 4.7.
- **Source:** [Time class](https://docs.godotengine.org/en/4.7/classes/class_time.html) and the [4.7.2 monitor update in `main.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L5135-L5143).
- **Evidence:** Verified from the API and source. The percentile convention is a design choice of this harness.
- **Trap:** Sorting FPS instead of frame time reverses the tail. Mixing quantile conventions changes small-sample gates.

### 3. Remove every engine-side throttle

- **What:** Combine `--disable-vsync`, `--max-fps 0`, zero frame delay, real-time synchronization and low-processor mode off.
- **Why/thread:** It removes the main-thread sleeps and presentation waits that hide CPU, render and GPU headroom.
- **Measure:** Assert the run-time settings, and reject a histogram that clusters at display periods or at 6.9 ms.
- **Version:** `--max-fps` exists since Godot 4.2. All the listed controls apply to 4.7.
- **Source:** [4.7 command-line tutorial](https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html), [ProjectSettings](https://docs.godotengine.org/en/4.7/classes/class_projectsettings.html) and [4.7.2 `os.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L690-L735).
- **Evidence:** Verified with the local help and the engine source.
- **Trap:** A graphics driver can force V-Sync on. Unlimited rendering heats a laptop quickly.

### 4. Two-axis CPU and GPU isolation

- **What:** Compare the native 3D scale with a 0.5 3D scale, then compare 2 window resolutions, with the scenario fixed.
- **Why/thread:** A large response to the 3D scale points to GPU pixel work. A response to window resolution only can point to 2D, UI or composition work. A small response points to CPU, draw submission, geometry or synchronization.
- **Measure:** Paired wall p50 and p99, and summed viewport render CPU and GPU times, at each setting.
- **Version:** Godot 4.7. `Viewport.scaling_3d_scale` changes 3D, not 2D.
- **Source:** [Resolution scaling](https://docs.godotengine.org/en/4.7/tutorials/3d/resolution_scaling.html) and [internal rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#d-and-3d-rendering-separation).
- **Evidence:** Verified from the official documentation.
- **Trap:** A 0.5 scale on each axis renders one quarter of the pixels, not half. A resolution change can change visibility or the UI layout.

### 5. Built-in script Profiler for hot-spot names

- **What:** Capture the standard Profiler for a short time in a debug run, and rank script functions by self time, total time and calls.
- **Why/thread:** It names the costly GDScript functions and engine categories on the main and physics threads before a release take proves the gain.
- **Measure:** Debugger > Profiler frame selection. Then reproduce the same frame in the harness.
- **Version:** Godot 4.7.
- **Source:** [The Profiler](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/the_profiler.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** Profiling is intrusive, debug timing is not release timing, and the built-in Profiler does not support C# scripts.

### 6. Visual Profiler and `--gpu-profile`

- **What:** Use the Visual Profiler for render CPU and GPU pass timelines, and `--gpu-profile` for a console breakdown once per second.
- **Why/thread:** It names the costly render-thread and GPU passes, such as shadows, opaque, transparent and post-processing.
- **Measure:** A short Visual Profiler capture at the fixed viewport size. Match its selected frame to a harness spike. `--gpu-profile` reads the same GPU profiling data.
- **Version:** Godot 4.7.
- **Source:** [Debugger panel](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/debugger_panel.html#visual-profiler), [logging](https://docs.godotengine.org/en/4.7/tutorials/scripting/logging.html) and the local 4.7.2 `--help`.
- **Evidence:** Verified from the official documentation and help.
- **Trap:** The Visual Profiler CPU time covers rendering only, the results depend on the viewport resolution, and the Compatibility renderer on macOS is not supported.

### 7. Native sampling profiler

- **What:** Sample a production-optimized Godot build with symbols: Instruments on Apple platforms, `perf` or Hotspot on Linux, or Superluminal where it is supported.
- **Why/thread:** It shows the engine, GDExtension, driver, lock and worker-thread stacks that the script Profiler cannot see.
- **Measure:** Select the sample window of the harness, and rank self and inclusive CPU samples by thread and stack.
- **Version:** Godot 4.7. Each profiler is available on some platforms only.
- **Source:** [Godot C++ profiler guide](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/index.html), [Instruments](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/instruments.html), [Superluminal](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/superluminal.html) and [Linux `perf` guidance](https://docs.kernel.org/process/debugging/userspace_debugging_guide.html#diving-deeper-with-perf).
- **Evidence:** Not verified locally. Official documentation.
- **Trap:** Official Godot binaries have no engine symbols. The stack names of a debug template are useful, but its timings do not represent production.

### 8. Tracy timelines for the engine and a Rust GDExtension

- **What:** Build Godot with `profiler=tracy` for engine zones, and instrument hot Rust GDExtension functions with `tracy-client` behind a condition.
- **Why/thread:** 1 timeline shows the waits and the overlap of the main, render, worker and extension threads.
- **Measure:** Capture the same 30-second scenario, then inspect the frames above p99 and their nested zones.
- **Version:** Godot 4.7. The `tracy-client` version must match the Rust toolchain of the extension.
- **Source:** [Godot Tracy setup](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/tracy.html) and [`tracy-client`](https://docs.rs/tracy-client/latest/tracy_client/).
- **Evidence:** Not verified locally. Official Godot documentation and crate documentation.
- **Trap:** A Tracy-instrumented GDExtension alone shows extension zones, not the internal zones of Godot. Full engine tracing needs a Tracy-enabled engine build. Tracy can broadcast discovery and expose source and assembly data, so enable it only in profiling builds.

### 9. RenderDoc frame inspection

- **What:** Capture 1 representative slow GPU frame, and inspect passes, draw events, resources, pipeline state and the causes of overdraw.
- **Why/thread:** It finds GPU work and render-thread submission patterns after the timing tools name a costly pass.
- **Measure:** Match the capture to the Visual Profiler pass names and to the draw-call and primitive deltas. Do not use the capture duration as a benchmark time.
- **Version:** Godot 4.7. Vulkan source-level shader debugging can use `--generate-spirv-debug-info`.
- **Source:** [4.7 command-line tutorial, debug options](https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html#debug-options).
- **Evidence:** Not verified locally. The official Godot documentation confirms the integration flag.
- **Trap:** A capture changes the timing. Metal uses the Xcode GPU tools, not RenderDoc.

### 10. Performance singleton at its real cadence

- **What:** Use the built-in monitors as low-rate context metrics, not as the wall-frame clock.
- **Why/thread:** Counts and 1-second peaks point to main, physics, memory, render-submission and object-growth causes behind a wall-time change.
- **Measure:** Log `TIME_FPS`, `TIME_PROCESS`, `TIME_PHYSICS_PROCESS`, draw calls, primitives, object and node counts, and memory once per second, with units in the column names.
- **Version:** Godot 4.7.
- **Source:** [Performance class](https://docs.godotengine.org/en/4.7/classes/class_performance.html) and the [4.7.2 update source](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L5135-L5143).
- **Evidence:** Verified from the engine source.
- **Trap:** `TIME_FPS` and the process monitors update about once per second. Memory monitors are not available in release builds. Reads on every frame give duplicated values.

### 11. Cheap custom monitors for workload context

- **What:** Register numeric workload counts, such as active agents, queued jobs, streamed chunks or visible projectiles, with `Performance.add_custom_monitor()`.
- **Why/thread:** It matches the main, physics and worker workload size to the slow intervals, so the teammate can reproduce the same load.
- **Measure:** Show them in Debugger > Monitors, and sample them once per second into the harness output.
- **Version:** Custom monitors exist since Godot 4.0 and apply to 4.7.
- **Source:** [Custom performance monitors](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/custom_performance_monitors.html) and the [4.7.2 callable implementation](https://github.com/godotengine/godot/blob/4.7.2-stable/main/performance.cpp#L549-L571).
- **Evidence:** Verified from the official documentation and source.
- **Trap:** The callable runs on every query. A getter that scans the scene tree changes the performance that it measures. Keep an O(1) counter instead.

### 12. Thermal and power-state validity gate

- **What:** Start laptop takes at a stable temperature on external power, record the thermal state before and after, and cool down or invalidate a take whose state changed.
- **Why/thread:** CPU and GPU frequency throttling changes every thread time and the GPU time, whatever the code under test does.
- **Measure:** The platform thermal and power state, and the drift of repeated baseline p50. On Apple platforms, record `ProcessInfo.thermalState` through a native helper or the runner.
- **Version:** A platform technique. It applies to Godot 4.7.
- **Source:** [Apple `thermalState`](https://developer.apple.com/documentation/foundation/processinfo/thermalstate-swift.property) and [thermal-state meanings](https://developer.apple.com/documentation/foundation/processinfo/thermalstate-swift.enum).
- **Evidence:** Not verified at run time. Official Apple documentation.
- **Trap:** Unlimited 300 fps runs make the thermal problem. Keep takes short and interleaved, and never compare a cold baseline with a hot candidate.

### 13. `--benchmark` and `--benchmark-file` for startup only

- **What:** Use the editor-only benchmark flags to time engine startup sections and to write startup marks to the console or JSON. Do not use them to measure gameplay frames.
- **Why/thread:** They isolate main-thread startup, server initialization, extension loading and game loading. They save no steady-state frame time.
- **Measure:** `godot --path PROJECT --quit --benchmark-file /absolute/path/startup.json` with an editor build.
- **Version:** Verified on Godot 4.7.2. The help marks both flags editor-only (`E`).
- **Source:** [4.7.2 CLI registration](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L729-L730), [startup marks](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L4013-L4016) and the [JSON writer](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L792-L835).
- **Evidence:** Verified with the local help and source.
- **Trap:** Export templates do not have these editor-only flags. Their JSON holds named startup durations, not frame samples.

### 14. Headless logic companion benchmark

- **What:** Run a separate deterministic headless benchmark for pure simulation, pathfinding, generation, serialization or Rust micro-benchmarks.
- **Why/thread:** It isolates main, physics and worker CPU logic from presentation and GPU variation.
- **Measure:** Explicit timed regions with `Time.get_ticks_usec()`, many iterations, a warm-up, a checksum, and medians across processes.
- **Version:** Godot 4.7.
- **Source:** The local 4.7.2 `--help` says that `--headless` selects the headless display and dummy audio. The [Godot benchmark project](https://github.com/godotengine/godot-benchmarks) separates per-frame and non-scene benchmarks.
- **Evidence:** Verified from the local help and the official benchmark design.
- **Trap:** Never compare headless FPS, GPU time, draw calls, render memory or rendering changes with a windowed build.

### 15. Paired CI frame-time gate

- **What:** Compare baseline and candidate p99 in alternating pairs, with an absolute noise floor and a relative threshold, and report an explicit target-budget failure.
- **Why/thread:** It catches regressions from any thread or the GPU and resists host noise. The gate itself saves no frame time.
- **Measure:** The regression rule above, the raw take files, the paired p50, p95 and p99 table, the invalid reasons and a non-zero exit status.
- **Version:** Harness policy for Godot 4.7.
- **Source:** `skills/godot-perf-measure/SKILL.md` covers the paired A/B method. The source report adds the pass and fail rule.
- **Evidence:** Design recommendation. Not validated against a project history.
- **Trap:** Shared cloud CI is often too noisy for sub-millisecond gates. Use a dedicated labeled runner, keep the take files, and treat fewer than 3 valid pairs as an infrastructure failure, not a pass.

### 16. Hot-spot escalation ladder

- **What:** Go from harness counters to built-in profiles, native sampling, tracing and at last a GPU frame capture, only as each earlier tool narrows the question.
- **Why/thread:** It keeps observer overhead low while it finds the owner among the main, physics, render, worker, extension and GPU work.
- **Measure:** Each escalation names a timestamp or frame range from the harness, and returns a function, pass, wait or draw event that explains it.
- **Version:** Godot 4.7.
- **Source:** The [Godot profiling overview](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/index.html) separates sampling and tracing tools.
- **Evidence:** Official tool behavior and a recommended workflow. Not verified at run time.
- **Trap:** An intrusive trace or a GPU capture at the start can make the hitch itself, or give a large trace with no reproducible frame to inspect.
