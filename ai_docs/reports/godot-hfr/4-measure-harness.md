# HFR measurement and the 300 fps harness

## Scope and evidence

This report covers measurement at 200–300 fps for Godot 4.7. It does not rank gameplay or rendering optimizations; it ranks measurement techniques by how quickly they expose frame-time savings. Measurement has no direct frame-time gain, so each entry names the thread whose avoidable cost it can reveal.

Verified locally on 2026-11-24 with `Godot 4.7.2.stable.official.ed1daf0bf`, its `--help` output, and the 4.7.2 engine source. No project benchmark was run because this ticket supplies no game project or benchmark scene. External-profiler operation is sourced from official documentation and is marked not verified.

Frame budgets:

| Target | Budget |
| --- | ---: |
| 200 fps | 5.00 ms |
| 240 fps | 4.17 ms |
| 300 fps | 3.33 ms |

## Corrections to the current skills

1. **A project setting cannot turn V-Sync back on after `--disable-vsync`.** `skills/godot-perf-measure/SKILL.md:54-56` says to use a scratch settings file because saved settings can force V-Sync back on. Godot 4.7.2 describes the flag as forcing V-Sync off even when the project enables it; only driver-level enforcement can defeat it. Keep the scratch settings file for reproducibility, but do not state this incorrect precedence. The same behavior is correctly described in `skills/godot-optimization/references/measuring-performance.md:26-28`. Evidence: [4.7 command-line behavior](https://docs.godotengine.org/en/4.7/tutorials/rendering/jitter_stutter.html#input-lag) and [4.7.2 `main.cpp` flag parsing](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L1973-L1975).
2. **`Performance.TIME_PROCESS` is not a fresh per-frame sample.** `skills/godot-perf-measure/SKILL.md:48-50` and `:122-123` can lead an implementer to log it per frame. In 4.7.2, `Main::iteration()` retains the maximum process and physics duration during an approximately one-second interval, then updates the `Performance` values when it updates FPS. Per-frame rows would repeat a one-second peak, not describe each frame. Use monotonic callback-to-callback wall time for frame samples and label `TIME_PROCESS`/`TIME_PHYSICS_PROCESS` as one-second peak diagnostics. Evidence: [4.7.2 `main.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L5135-L5143) and [4.7.2 `performance.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/main/performance.cpp#L239-L250).
3. **Do not name Metal as a driver that returns zero GPU timers in Godot 4.7.** `skills/godot-perf-measure/SKILL.md:57-60` gives Metal as its example. Godot 4.7.2's native Metal driver implements timestamp query creation, writes, result retrieval, and time conversion. A harness must detect and report zero or unavailable timers at runtime, but it should not prescribe MoltenVK merely because the active driver is Metal. Evidence: [4.7.2 Metal timestamp interface](https://github.com/godotengine/godot/blob/4.7.2-stable/drivers/metal/rendering_device_driver_metal.h#L489-L499). This source check does not prove every Metal device returns accurate timestamps.
4. **The existing percentile code needs a named convention.** `skills/godot-optimization/references/measuring-performance.md:105-110` indexes `int(n * p)` without stating a quantile convention and omits 1% low. A CI gate needs one stable definition. Use nearest-rank percentile `ceil(p*n)-1`; define 1% low FPS as `1000 / p99_frame_ms`. Do not average FPS values because FPS is the reciprocal of frame time.

The existing skills already cover release-versus-debug comparisons, interleaved A/B runs, warm-up, fixed scenarios, load recording, render-scale isolation, built-in profilers, render statistics, and basic percentiles. The techniques below only add missing precision, a correction, or a concrete harness contract.

## Harness the teammate runs

### Scenario contract

Create one release-export benchmark scene per representative workload. The scene must use a fixed seed, fixed initial save/state, scripted inputs or camera path, fixed resolution, and a duration in seconds rather than a fixed frame count. Run in a foreground window for rendering measurements. A headless run is valid only for deterministic simulation or script micro-benchmarks because the headless display driver uses the dummy renderer.

The scene has three phases:

1. Load and settle until resources are ready.
2. Warm up for 5 seconds without recording.
3. Record for 30 seconds, write results atomically, and quit with `0`; quit nonzero on invalid configuration or missing samples.

Recommended command for a release export:

```bash
./MyGame --windowed --resolution 1920x1080 --disable-vsync --max-fps 0 -- --hfr-bench=city --warmup-seconds=5 --sample-seconds=30 --hfr-out=/absolute/path/city-new-01.json
```

The harness must also set or assert:

- `Engine.max_fps == 0` (`--max-fps 0` overrides `application/run/max_fps`).
- requested V-Sync is disabled and the observed frame distribution is not quantized to the display period; a driver may still enforce V-Sync.
- `OS.low_processor_usage_mode == false`; its default 6,900 microsecond sleep is about 145 fps and therefore makes a 200–300 fps test impossible.
- `application/run/frame_delay_msec == 0`, no `--fixed-fps`, and real-time synchronization remains enabled.
- foreground, fixed renderer/driver, fixed window size, fixed graphics settings, and no editor-hosted run.

The configuration precedence is source-verified: Godot loads `application/run/max_fps`, then applies the CLI value when supplied, and separately loads low-processor mode and its sleep duration ([4.7.2 `main.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L2251-L2268)). The OS combines low-processor sleep and max-FPS delay by taking the larger delay ([4.7.2 `os.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L690-L735)).

### Output contract

Write one JSON document per take. Keep raw frame samples so the summarizer can be corrected without rerunning the game.

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
    "one_percent_low_fps": 276.24,
    "over_3_333_ms": 921,
    "over_4_167_ms": 17
  },
  "frames": [
    {"i": 0, "frame_ms": 3.18, "render_cpu_ms": 0.41, "render_gpu_ms": 2.62, "draw_calls": 812, "primitives": 145200}
  ]
}
```

Use callback-to-callback `Time.get_ticks_usec()` deltas for `frame_ms`. Enable viewport render-time measurement and sum every viewport drawn each frame for render CPU/GPU values. Record `null`, not `0`, when a timer is unavailable. Render information is not valid until at least two frames have rendered, and GPU power-state changes can distort GPU timing under a cap ([RenderingServer class](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html#class-renderingserver-method-viewport-get-measured-render-time-gpu)).

### Regression rule

Run one untimed warm-up and 5 interleaved baseline/candidate pairs on a dedicated machine. Invalidate a pair if either take loses foreground status, changes thermal state above nominal, detects a cap, changes renderer/settings, has fewer than 20 seconds of valid samples, or overlaps another engine process.

Call the candidate a **frame-time regression** only when all conditions hold:

1. At least 3 pairs are valid.
2. At least 4 of 5 valid pair deltas are positive, or all available deltas are positive when only 3–4 pairs remain.
3. Median paired p99 delta is greater than `max(0.10 ms, baseline_p99 * 0.05)`.

Also call it a **300 fps budget failure** whenever the candidate's median take p99 exceeds 3.333 ms, and a **240 fps budget failure** whenever it exceeds 4.167 ms. Budget failures are absolute and do not require a baseline regression. CI prints the pair table and returns nonzero for either named failure. The 0.10 ms floor prevents tiny timer noise from consuming review time, while the 5% relative threshold scales to slower targets.

## Ranked techniques

### 1. Fixed, windowed scenario harness

- **What:** Replay a deterministic 30-second representative scene in a foreground release-export window, after a separate warm-up.
- **Why/thread:** It reveals the largest main, render, physics, or GPU cost under real rendering; the harness itself saves no time.
- **Measure:** Raw wall-frame samples plus summed viewport render CPU/GPU times, draw calls, primitives, and scenario invariants.
- **Version:** Godot 4.7; the design is version-independent, but APIs and flags were checked on 4.7.2.
- **Source:** [Official Godot benchmark project](https://github.com/godotengine/godot-benchmarks) and local 4.7.2 `--help`.
- **Evidence:** Verified from source/help; not runtime-verified against a game.
- **Trap:** Headless mode removes the renderer, and fixed frame counts cover less gameplay as the candidate gets faster.

### 2. Wall-frame percentiles and 1% low

- **What:** Report nearest-rank p50, p95, p99, worst frame, threshold counts, and `1% low FPS = 1000 / p99_ms` from raw frame durations.
- **Why/thread:** It exposes rare work on any thread that average FPS hides, directing optimization toward the slow tail.
- **Measure:** `Time.get_ticks_usec()` between consecutive `_process` callbacks; retain every sample.
- **Version:** Godot 4.7.
- **Source:** [Time class](https://docs.godotengine.org/en/4.7/classes/class_time.html) and the source correction above.
- **Evidence:** Verified API/source; percentile convention is this harness's explicit design choice.
- **Trap:** Sorting FPS rather than frame time reverses the tail, and mixing quantile conventions changes small-sample gates.

### 3. Remove every engine-side throttle

- **What:** Combine `--disable-vsync`, `--max-fps 0`, zero frame delay, real-time sync, and low-processor mode off.
- **Why/thread:** It removes main-thread sleeps and presentation waits that conceal CPU/render/GPU headroom.
- **Measure:** Assert runtime settings and reject a histogram clustered at display periods or 6.9 ms.
- **Version:** `--max-fps` exists since Godot 4.2; all listed controls apply to 4.7.
- **Source:** [4.7 command-line documentation](https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html), [ProjectSettings](https://docs.godotengine.org/en/4.7/classes/class_projectsettings.html), and [4.7.2 `os.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L690-L735).
- **Evidence:** Verified with local help and engine source.
- **Trap:** A graphics driver can force V-Sync, and unlimited rendering heats a laptop quickly.

### 4. Two-axis CPU/GPU isolation

- **What:** Compare native versus 0.5 3D scale, then compare two window resolutions while holding the scenario fixed.
- **Why/thread:** A large scale response points to GPU pixel work; a window-resolution-only response can expose 2D/UI/composition work; little response points toward CPU, draw submission, geometry, or synchronization.
- **Measure:** Paired wall p50/p99 and summed viewport render CPU/GPU times at each setting.
- **Version:** Godot 4.7; `Viewport.scaling_3d_scale` affects 3D, not 2D.
- **Source:** [Resolution scaling](https://docs.godotengine.org/en/4.7/tutorials/3d/resolution_scaling.html) and [internal rendering architecture](https://docs.godotengine.org/en/4.7/engine_details/architecture/internal_rendering_architecture.html#d-and-3d-rendering-separation).
- **Evidence:** Verified from official docs.
- **Trap:** A 0.5 per-axis scale renders one quarter of the pixels, not half, and changing resolution can change visibility or UI layout.

### 5. Built-in script profiler for hotspot names

- **What:** Capture the standard Profiler briefly in a debug run and rank script functions by self time, total time, and calls.
- **Why/thread:** It identifies high-cost GDScript and engine categories on the main/physics thread before release timing proves the gain.
- **Measure:** Debugger > Profiler frame selection; reproduce the same frame in the harness afterward.
- **Version:** Godot 4.7.
- **Source:** [The Profiler](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/the_profiler.html).
- **Evidence:** Verified from official docs.
- **Trap:** Profiling is intrusive, absolute debug timing is not release timing, and the built-in profiler does not support C# scripts.

### 6. Visual Profiler and `--gpu-profile`

- **What:** Use the Visual Profiler for render CPU/GPU pass timelines and `--gpu-profile` for a once-per-second console breakdown.
- **Why/thread:** It names expensive render-thread and GPU passes such as shadows, opaque, transparent, and post-processing.
- **Measure:** Short Visual Profiler capture at the fixed viewport size; correlate its selected frame with harness spikes. `--gpu-profile` uses the same GPU profiling data source.
- **Version:** Godot 4.7.
- **Source:** [Debugger panel](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/debugger_panel.html#visual-profiler), [logging](https://docs.godotengine.org/en/4.7/tutorials/scripting/logging.html), and local 4.7.2 `--help`.
- **Evidence:** Verified from official docs/help.
- **Trap:** Visual Profiler CPU time covers rendering only, results depend on viewport resolution, and Compatibility on macOS is unsupported.

### 7. Native sampling profiler

- **What:** Sample a production-optimized Godot build with symbols using Instruments on Apple platforms, `perf`/Hotspot on Linux, or Superluminal on supported platforms.
- **Why/thread:** It reveals engine, GDExtension, driver, lock, and worker-thread stacks that the script profiler cannot see.
- **Measure:** Select the harness's sample window and rank self and inclusive CPU samples by thread and stack.
- **Version:** Godot 4.7; profiler availability is platform-specific.
- **Source:** [Godot C++ profiler guide](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/index.html), [Instruments](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/instruments.html), [Superluminal](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/superluminal.html), and [Linux `perf` guidance](https://docs.kernel.org/process/debugging/userspace_debugging_guide.html#diving-deeper-with-perf).
- **Evidence:** Not verified locally; official documentation.
- **Trap:** Official Godot binaries lack engine symbols; a debug template's stack names are useful but its timings are not production-representative.

### 8. Tracy timelines for engine and Rust GDExtension

- **What:** Build Godot with `profiler=tracy` for engine zones and conditionally instrument hot Rust GDExtension functions with `tracy-client`.
- **Why/thread:** A unified timeline exposes waits and overlap among main, render, worker, and extension threads.
- **Measure:** Capture the same 30-second scenario, then inspect frames above p99 and their nested zones.
- **Version:** Godot 4.7; `tracy-client` version must match the extension's Rust toolchain.
- **Source:** [Godot Tracy setup](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/tracy.html) and [`tracy-client`](https://docs.rs/tracy-client/latest/tracy_client/).
- **Evidence:** Not verified locally; official Godot docs and crate documentation.
- **Trap:** A Tracy-instrumented GDExtension alone shows extension zones, not Godot's internal zones. Full engine tracing requires a Tracy-enabled engine build. Tracy may broadcast discovery and expose source/assembly data, so enable it only in profiling builds.

### 9. RenderDoc frame inspection

- **What:** Capture one representative slow GPU frame to inspect passes, draw events, resources, pipeline state, and overdraw causes.
- **Why/thread:** It finds GPU work and render-thread submission patterns after the timing tools identify a costly pass.
- **Measure:** Correlate capture contents with Visual Profiler pass names and draw-call/primitives deltas; do not use capture duration as benchmark timing.
- **Version:** Godot 4.7; Vulkan source-level shader debugging can use `--generate-spirv-debug-info`.
- **Source:** [4.7 command-line tutorial](https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html#debug-options).
- **Evidence:** Not verified locally; official Godot documentation confirms the integration flag.
- **Trap:** Capturing perturbs timing, and Metal uses Xcode GPU tools rather than RenderDoc.

### 10. Performance singleton with correct cadence

- **What:** Use built-in monitors as low-rate context metrics, not as the raw wall-frame clock.
- **Why/thread:** Counts and one-second peaks indicate main, physics, memory, render-submission, and object-growth causes behind a wall-time change.
- **Measure:** Log `TIME_FPS`, `TIME_PROCESS`, `TIME_PHYSICS_PROCESS`, draw calls, primitives, object/node counts, and memory once per second with units in column names.
- **Version:** Godot 4.7.
- **Source:** [Performance class](https://docs.godotengine.org/en/4.7/classes/class_performance.html) and [4.7.2 update source](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L5135-L5143).
- **Evidence:** Verified from engine source.
- **Trap:** `TIME_FPS` and process monitors update about once per second; memory monitors are unavailable in release builds; repeated per-frame reads create duplicated values.

### 11. Cheap custom monitors for workload context

- **What:** Register numeric workload counts such as active agents, queued jobs, streamed chunks, or visible projectiles with `Performance.add_custom_monitor()`.
- **Why/thread:** It correlates main/physics/worker workload size with slow intervals so the teammate can reproduce the same load.
- **Measure:** Display in Debugger > Monitors and sample once per second into the harness output.
- **Version:** Custom monitors exist since Godot 4.0 and apply to 4.7.
- **Source:** [Custom performance monitors](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/custom_performance_monitors.html) and [4.7.2 callable implementation](https://github.com/godotengine/godot/blob/4.7.2-stable/main/performance.cpp#L549-L571).
- **Evidence:** Verified from official docs/source.
- **Trap:** The callable runs whenever queried. A getter that scans the scene tree changes the performance being measured; maintain an O(1) counter instead.

### 12. Thermal and power-state validity gate

- **What:** Start laptop takes at a stable temperature on external power, record thermal state before/after, and cool down or invalidate changed-state takes.
- **Why/thread:** CPU/GPU frequency throttling changes all thread and GPU times independently of the code under test.
- **Measure:** Platform thermal/power state plus drift in repeated baseline p50; on Apple platforms, record `ProcessInfo.thermalState` through a native helper or the runner.
- **Version:** Platform technique; applies to Godot 4.7.
- **Source:** [Apple `thermalState`](https://developer.apple.com/documentation/foundation/processinfo/thermalstate-swift.property) and [thermal-state meanings](https://developer.apple.com/documentation/foundation/processinfo/thermalstate-swift.enum).
- **Evidence:** Not runtime-verified; official Apple documentation.
- **Trap:** Unlimited 300 fps runs create the thermal problem. Keep takes short and interleaved; never compare a cold baseline with a hot candidate.

### 13. `--benchmark` and `--benchmark-file` only for startup

- **What:** Use the editor-only benchmark flags to time engine startup sections and emit console or JSON startup marks, not to measure gameplay frames.
- **Why/thread:** They isolate main-thread startup, server initialization, extension loading, and game loading; they do not save steady-state frame time.
- **Measure:** `godot --path PROJECT --quit --benchmark-file /absolute/path/startup.json` with an editor build.
- **Version:** Verified on Godot 4.7.2; flags are editor-only (`E`) in help.
- **Source:** [4.7.2 CLI registration](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L729-L730), [startup marks](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L4013-L4016), and [JSON writer](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L792-L835).
- **Evidence:** Verified with local help and source.
- **Trap:** Export templates do not expose these editor-only flags, and their JSON contains named startup durations rather than frame samples.

### 14. Headless logic companion benchmark

- **What:** Run a separate deterministic headless benchmark for pure simulation, pathfinding, generation, serialization, or Rust micro-benchmarks.
- **Why/thread:** It isolates main/physics/worker CPU logic from presentation and GPU variability.
- **Measure:** Explicit timed regions with `Time.get_ticks_usec()`, many iterations, warm-up, checksum, and medians across processes.
- **Version:** Godot 4.7.
- **Source:** Local 4.7.2 `--help` states that `--headless` selects headless display and dummy audio; [Godot benchmark project](https://github.com/godotengine/godot-benchmarks) separates per-frame and non-scene benchmarks.
- **Evidence:** Verified from local help and official benchmark design.
- **Trap:** Never compare headless FPS, GPU time, draw calls, render memory, or rendering changes with a windowed build.

### 15. Paired CI frame-time gate

- **What:** Compare baseline and candidate p99 in alternating pairs, use an absolute noise floor plus a relative threshold, and report an explicit target-budget failure.
- **Why/thread:** It catches regressions from any thread or GPU while resisting host noise; the gate itself saves no frame time.
- **Measure:** The regression rule above, raw take artifacts, paired p50/p95/p99 table, validity reasons, and nonzero exit status.
- **Version:** Harness policy for Godot 4.7.
- **Source:** Existing `skills/godot-perf-measure/SKILL.md:64-84` covers paired A/B methodology; this report supplies the missing pass/fail rule.
- **Evidence:** Design recommendation; not validated against a project history.
- **Trap:** Shared cloud CI is often too noisy for sub-millisecond gates. Use a dedicated labeled runner, retain artifacts, and treat fewer than 3 valid pairs as infrastructure failure rather than pass.

### 16. Hotspot escalation ladder

- **What:** Escalate from harness counters to built-in profiles, native sampling, tracing, and finally a GPU frame capture only as each earlier tool narrows the question.
- **Why/thread:** It minimizes observer overhead while resolving ownership across main, physics, render, worker, extension, and GPU work.
- **Measure:** Require every escalation to name a timestamp/frame range from the harness and return a function, pass, wait, or draw event that explains it.
- **Version:** Godot 4.7.
- **Source:** [Godot profiling overview](https://docs.godotengine.org/en/4.7/engine_details/development/profiling/index.html) distinguishes sampling and tracing tools.
- **Evidence:** Official tool behavior plus recommended workflow; not runtime-verified.
- **Trap:** Starting with an intrusive trace or GPU capture can manufacture the hitch or produce a large trace with no reproducible frame to inspect.

## Detection

Use this sequence in a real project:

1. Search `project.godot` and startup code for `application/run/max_fps`, `application/run/low_processor_mode`, `application/run/frame_delay_msec`, `Engine.max_fps`, `low_processor_usage_mode`, `window_set_vsync_mode`, `--fixed-fps`, and custom settings loaders. Reject the run until every cap is explained.
2. Run the fixed windowed scenario. If p99 exceeds 3.333 ms, inspect the paired raw row and its render CPU/GPU values. If the p99 tail is much larger than p50, use a hitch-oriented capture; if both are high, use a steady-state profile.
3. Halve `scaling_3d_scale`. A large GPU and wall-time drop identifies pixel-bound 3D work. Then change window resolution. A resolution response not explained by 3D scale points toward 2D, UI, composition, or another viewport.
4. If process/physics one-second peaks rise with the slow interval, use the built-in Profiler. Search project scripts for `_process`, `_physics_process`, per-frame signals, and custom monitor getters; rank by profiler self time and call count.
5. If render CPU rises, inspect draw calls, shadow draw calls, primitives, and viewport count. Search for `SubViewport`, unique materials, lights with shadows, and per-object draw paths.
6. If GPU time rises, use Visual Profiler or `--gpu-profile` to name the pass, then capture one matching frame with RenderDoc or the platform GPU tool.
7. If wall time is high but built-in categories are low, sample the native process. Look for driver calls, locks, GDExtension/Rust functions, allocator work, file I/O, and worker waits. Add Tracy zones only around the unresolved native region.
8. If results drift over consecutive takes, compare baseline-first and baseline-last p50, thermal state, process count, foreground state, and renderer. Invalidate the take instead of attributing drift to code.
9. In CI, print `FRAME_TIME_REGRESSION`, `300_FPS_BUDGET_FAILURE`, `240_FPS_BUDGET_FAILURE`, or `INFRASTRUCTURE_INVALID` exactly. Preserve both summary JSON and raw samples for every non-pass.
