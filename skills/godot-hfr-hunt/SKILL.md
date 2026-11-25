---
name: godot-hfr-hunt
description: >-
  Find and remove what keeps a Godot 4.7 game from 200 to 300 fps. A 5-step workflow: uncap the
  frame rate, run the hfr_bench.py harness on 1 fixed scenario, read p50/p95/p99/worst/1% low
  against the 5.00, 4.17 and 3.33 ms budgets, classify the hot spot as script CPU, render thread,
  GPU, physics or spikes by its signature, and apply the matching technique, proven by a paired
  compare. Use it when a game must run at high refresh rates (144, 165, 240 or 360 Hz displays),
  when frame times sit above 3.33 to 5 ms, when the 1% low is poor, or when a change must not
  make a 240 fps game slower, even if nobody says "HFR".
---

# Godot High Frame Rate Hunt

This skill finds the cost that keeps a Godot 4.7 game from 200 to 300 fps. It
works on frame time in milliseconds, never on average FPS. Follow the 5 steps in
order. Each step names the reference file that holds the detail.

| Target | Frame budget | 5% triage threshold |
| --- | ---: | ---: |
| 200 fps | 5.00 ms | 0.25 ms |
| 240 fps | 4.17 ms | 0.20 ms |
| 300 fps | 3.33 ms | 0.17 ms |

A subsystem that takes 5% of the budget is worth a look. At 300 fps, a 0.2 ms
change is already 6% of the frame.

## Workflow

1. **Uncap.** Run a release export in a foreground window with
   `--disable-vsync --max-fps 0`. `--disable-vsync` forces V-Sync off even when
   the project enables it; only a driver can still force it on. Assert these
   values at run time, and stop the run when one is wrong:
   - `Engine.max_fps == 0` (`--max-fps 0` overrides `application/run/max_fps`);
   - `OS.low_processor_usage_mode == false` (its default 6,900 microsecond
     sleep caps a run near 145 fps);
   - `application/run/frame_delay_msec == 0`;
   - no `--fixed-fps` argument.

   Reject a run whose frame times cluster at the display period or at 6.9 ms:
   a cap is still active. Do not time an editor-hosted run. Do not time a
   `--headless` run for rendering: `--headless` uses the dummy renderer. A
   headless run is valid only for pure simulation or script micro-benchmarks.
   Find every cap in the project with the cap search in
   [references/detection.md](references/detection.md).

2. **Run the harness.** Copy the GDScript recorder from
   [references/measure-harness.md](references/measure-harness.md) into the
   game. Make 1 fixed benchmark scenario: a fixed seed, a fixed start state,
   scripted input or a camera path, a fixed resolution, and a duration in
   seconds, not in frames. The scenario settles, warms up for 5 s, and records
   for 30 s. Then run `scripts/hfr_bench.py`:

   ```bash
   python3 scripts/hfr_bench.py compare --baseline ./build-base/MyGame \
     --candidate ./build-new/MyGame --scenario city --out /tmp/hfr-city --target-fps 240
   python3 scripts/hfr_bench.py summarize /tmp/hfr-city/city-candidate-01.json
   ```

   Each take runs `<export> --windowed --resolution 1920x1080 --disable-vsync
   --max-fps 0 -- --hfr-bench=<scenario> --warmup-seconds=5 --sample-seconds=30
   --hfr-out=<path>` and writes 1 JSON take file with the raw frames. Keep laptop
   takes on external power and at a stable temperature. Never compare a cold
   baseline with a hot candidate.

3. **Read the percentiles.** Read p50, p95, p99, the worst frame and the
   1% low (`1000 / frame_ms_p99`) of the take against the 5.00 ms (200 fps),
   4.17 ms (240 fps) and 3.33 ms (300 fps) budgets. The harness uses
   nearest-rank percentiles (`ceil(p * n) - 1` on sorted frame times) and never
   averages or sorts FPS values. Then read the shape:
   - a p99 much larger than p50 is a spike problem: find the event on the slow
     frames;
   - a high p50 and a high p99 is a steady-cost problem: find the part that
     owns every frame;
   - a lower average with a worse p99 is not an improvement at high refresh.

   `Performance.TIME_PROCESS` and `TIME_PHYSICS_PROCESS` are 1-second peaks,
   not per-frame samples. Use them as context, never as the frame clock.

4. **Classify the hot spot.** Put the hot spot in 1 of 5 classes by its
   signature, then open that reference file. The signatures and the `rg`
   searches for each class are in [references/detection.md](references/detection.md).
   - **Script CPU**: the `TIME_PROCESS` or `TIME_PHYSICS_PROCESS` 1-second peak rises with the slow interval, and the Profiler ranks script functions by `Self` and `Calls` -> [references/script-cpu.md](references/script-cpu.md).
   - **Render thread**: `RenderingServer.get_frame_setup_time_cpu()` plus the summed viewport render CPU time is high, with high draw calls, objects or primitives -> [references/render-gpu.md](references/render-gpu.md).
   - **GPU**: the summed viewport GPU time is near the budget, and halving `scaling_3d_scale` gives a large drop -> [references/render-gpu.md](references/render-gpu.md).
   - **Physics**: `TIME_PHYSICS_PROCESS`, `PHYSICS_*_COLLISION_PAIRS`, `PHYSICS_*_ACTIVE_OBJECTS` or `TIME_NAVIGATION_PROCESS` own the time -> [references/physics-servers-threads.md](references/physics-servers-threads.md).
   - **Spikes**: a slow frame lines up with a pipeline `SURFACE` or `DRAW` counter delta, a load, an `instantiate()` or `add_child()` event, or a managed GC event -> [references/native-memory-hitches.md](references/native-memory-hitches.md).

   When wall time is high but every built-in category is low, follow the
   escalation ladder in [references/measure-harness.md](references/measure-harness.md):
   a native sampling profiler, then Tracy, then a GPU frame capture. Each
   escalation names a frame range from the harness and returns a function, a
   pass, a wait or a draw event that explains it.

5. **Apply the matching technique.** Pick the highest-ranked technique in the
   reference file whose signature matches. Each technique gives **What**,
   **Why/thread**, **Measure**, **Version**, **Evidence** (verified, not
   verified, or design recommendation) and **Trap**. Read the Trap before you
   change code. Prove the change with `hfr_bench.py compare`: 1 untimed
   warm-up take, then 5 interleaved baseline and candidate pairs. Accept the
   change only when the verdict is `PASS` and the game state at the end of the
   scenario is the same. The verdict lines are exact:
   - `PASS` (exit 0);
   - `FRAME_TIME_REGRESSION` (exit 1): at least 3 valid pairs, at least 4 of 5
     positive p99 deltas (all positive when only 3 or 4 pairs are valid), and a
     median p99 delta above `max(0.10 ms, baseline_p99 * 0.05)`;
   - `<target>_FPS_BUDGET_FAILURE` (exit 1): the median candidate p99 is above
     `1000 / --target-fps` ms (`--target-fps` is 200 to 300, default 240);
   - `INFRASTRUCTURE_INVALID` (exit 2): fewer than 3 valid pairs. It is never a
     pass. Fix the machine, not the code.

   Then go back to step 3 with the new take. Stop when p99 is inside the
   target budget on the target hardware.

## Rules

- Measure a release export. A debug build names the hot function; only a
  release take proves the gain.
- Change 1 thing per compare. A compare with 2 changes proves neither.
- Compare equal play duration, not equal frame counts. A faster candidate
  covers less gameplay in a fixed frame count.
- Shared cloud CI is too noisy for sub-millisecond gates. Run `compare` on a
  dedicated machine and keep every take file of a non-pass.
- Remove a load, pipeline first-use, GC or tree-mutation spike before you port
  code to another language.

## References

- [references/measure-harness.md](references/measure-harness.md): the
  scenario and take-file contract, the GDScript recorder, the regression rule,
  and the 16 measurement techniques with the escalation ladder.
- [references/detection.md](references/detection.md): the cap search, the
  triage thresholds, and every `rg` search and profiler signature, by class.
- [references/script-cpu.md](references/script-cpu.md): GDScript and main-thread CPU.
- [references/render-gpu.md](references/render-gpu.md): render thread and GPU.
- [references/physics-servers-threads.md](references/physics-servers-threads.md):
  physics, navigation, servers and worker threads.
- [references/native-memory-hitches.md](references/native-memory-hitches.md):
  loading, pipeline compilation, native calls, memory and GC hitches.

## Sources

The skill is written from 5 research reports checked against Godot
`4.7.2.stable.official.ed1daf0bf`. Main sources:

- [Godot 4.7 command-line tutorial](https://docs.godotengine.org/en/4.7/tutorials/editor/command_line_tutorial.html)
- [Godot 4.7 `Performance` class](https://docs.godotengine.org/en/4.7/classes/class_performance.html)
- [Godot 4.7 `RenderingServer` class](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html)
- [Godot 4.7 The Profiler](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/the_profiler.html)
- [Godot 4.7 resolution scaling](https://docs.godotengine.org/en/4.7/tutorials/3d/resolution_scaling.html)
- [4.7.2 `main.cpp`, frame-cap precedence](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L2251-L2268)
- [4.7.2 `main.cpp`, 1-second monitor update](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L5135-L5143)
- [4.7.2 `os.cpp`, low-processor sleep and max-FPS delay](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L690-L735)
- [Official Godot benchmark project](https://github.com/godotengine/godot-benchmarks)
