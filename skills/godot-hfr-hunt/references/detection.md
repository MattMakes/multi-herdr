# Detection: find the hot spot in a real project

This page collects the detection passes of the 5 research reports behind this
skill. It groups them by the 5 hot-spot classes of workflow step 4: script
CPU, render thread, GPU, physics and spikes. Each class gives the profiler or
monitor signature, the `rg` searches, and the technique file to open. The
searches find candidates, not proof. Prove every change with interleaved
release A/B takes (`hfr_bench.py compare`) on the target hardware.

Use a debug capture to locate candidates. Prove the gain in an uncapped
release export.

## Before you classify

**Cap search.** Search `project.godot` and the startup code for these names,
and for any custom settings loader. Reject the run until every cap is
explained:

- `application/run/max_fps`
- `application/run/low_processor_mode`
- `application/run/frame_delay_msec`
- `Engine.max_fps`
- `low_processor_usage_mode`
- `window_set_vsync_mode`
- `--fixed-fps`

**Triage threshold.** Investigate a subsystem when it takes 5% of the budget:
0.20 ms at 240 fps, 0.17 ms at 300 fps (0.25 ms at 200 fps). These are triage
thresholds, not acceptance limits. At 300 fps, a 0.2 ms change is already 6%
of the frame.

**Evidence triggers.** Each of these needs an explanation. None is an
automatic fix:

- any repeated frame over 4.17 ms (240 fps) or over 3.33 ms (300 fps);
- any gameplay-time `Draw` or `Surface` pipeline compilation;
- any managed allocation or GC event that lines up with a slow frame;
- a boundary loop that makes thousands of native calls per frame;
- `PHYSICS_*_COLLISION_PAIRS` that grow while the entity count stays flat;
- physics above 60 TPS without a measured need.

**Frame ownership.** Run the fixed, uncapped, windowed release scenario and a
short debug Visual Profiler capture.

1. If p99 is over the target budget, inspect the raw frame rows of the slow
   frames and their render CPU and GPU values.
2. If the p99 tail is much larger than p50, it is a spike problem: use a
   hitch capture and go to [Spikes](#spikes). If p50 and p99 are both high,
   it is a steady cost: use a steady-state profile.
3. If the summed viewport GPU time approaches the 3.33 to 5.00 ms budget while
   process and physics stay low, go to [GPU](#gpu).
4. If `get_frame_setup_time_cpu()` plus the summed viewport render CPU time is
   high, go to [Render thread](#render-thread).
5. If the process or physics 1-second peaks rise with the slow interval, go to
   [Script CPU](#script-cpu) or [Physics](#physics).

## Script CPU

Technique file: [script-cpu.md](script-cpu.md). For a native port, also
[native-memory-hitches.md](native-memory-hitches.md).

**Signature.** The `Performance.TIME_PROCESS` or `TIME_PHYSICS_PROCESS`
1-second peak rises with the slow interval. In Debugger > Profiler, rank
script functions by `Self` and by `Calls`. Search the project scripts for
`_process`, `_physics_process`, per-frame signals and custom monitor getters.

1. **Dormant and numerous callbacks.** In Debugger > Profiler, sort by
   `Calls`. Compare the `_process` and `_physics_process` totals with
   `Performance.TIME_PROCESS`, `TIME_PHYSICS_PROCESS` and `OBJECT_NODE_COUNT`.
   Search: `rg -n "func _process|func _physics_process" --glob '*.gd'`.
2. **Repeated node lookup.** Inspect the high-`Self` callbacks. Search:
   `rg -n '\$[^ ]+|get_node\(|get_node_or_null\(' --glob '*.gd'`, and check
   whether each expression runs every frame.
3. **Dynamic hot code.** Follow warnings and untyped values through the whole
   top-profiled function. Search the hot files for `: Variant`, `.call(`,
   `.get(` and `.set(`.
4. **Async and deferred churn.** Search:
   `rg -n 'await |call_deferred|\.bind\(|func\(' --glob '*.gd'`. In the
   Profiler, match the creation call counts to the entity count, and watch
   `OBJECT_COUNT` during sustained play.
5. **Signal fan-out.** Start from the high-call emitters. Call
   `get_signal_connection_list()` in a diagnostic build and list every
   listener. Search: `rg -n '\.emit\(|emit_signal\(' --glob '*.gd'`.
6. **Collection and allocation hot paths.** Inspect the top `Self` loops for
   `map`, `filter`, `reduce`, custom sorts, `slice`, front mutation,
   dictionary double probes, string formatting and new output on every frame.
   Micro-benchmark the exact container size and type.
7. **Object-heavy data.** Compare `OBJECT_COUNT`, `OBJECT_NODE_COUNT` and
   `MEMORY_STATIC` with the live gameplay entity count. A much larger count
   points to per-element `Object` or `RefCounted` records, or to hidden node
   fan-out.
8. **Fine native calls.** Search:
   `rg -n 'GDExtension|gdextension|godot::|Gd<|ClassDB::bind_method|#\[func\]|\.call\('`.
   Signature: a hot script function has a very high call count and little
   work per call.
9. **Native-port candidates.** Sort the script Profiler by `Self`, then
   inspect only the top arithmetic and data loops. Signature: script self time
   is material, and engine or GPU time is not the bottleneck.
10. **Packed buffers and struct-of-arrays.** Search:
    `rg -n 'Array\[(float|int|Vector)|Godot\.Collections\.Array|Dictionary|Variant|Vec<' --glob '*.{gd,cs,rs,cpp,hpp}'`.
    Signature: container iteration or conversion dominates, and memory rises
    with the entity count.
11. **C# interop batching.** Search inside loops:
    `rg -n 'Position|Transform|GetNode|GD\.Call|CallDeferred|new StringName|new NodePath' --glob '*.cs'`.
    Signature: managed method self time grows with the native property call
    count.

## Render thread

Technique file: [render-gpu.md](render-gpu.md).

**Signature.** `RenderingServer.get_frame_setup_time_cpu()` plus the summed
viewport render CPU time is high, with high draw calls, objects or
primitives. Halving `scaling_3d_scale` changes little. Prioritize submissions,
culling and materials. Inspect draw calls, shadow draw calls, primitives and
the viewport count. Search for `SubViewport`, unique materials, lights with
shadows and per-object draw paths.

1. **Shadows.** Log
   `get_render_info(RENDER_INFO_TYPE_SHADOW, RENDER_INFO_DRAW_CALLS_IN_FRAME)`.
   High or camera-dependent shadow draws point to removing or constraining
   real-time shadow casters, and to shadow map size, atlas, cascade and filter
   tuning. Search the scenes for `shadow_enabled = true`,
   `directional_shadow_max_distance` and `light_angular_distance`.
2. **Geometry and submission.** High visible draw calls, objects or
   primitives point to mesh LOD, HLOD with visibility ranges, MultiMesh,
   static mesh merging and instance uniforms. Search the `.tscn` and `.scn`
   import metadata for repeated mesh paths, duplicated material subresources,
   `visibility_range_`, `lod_bias` and `MultiMeshInstance`. Inspect the import
   settings for generated LODs.
3. **Occlusion.** If wireframe shows rooms or blocks behind walls and the
   visible-object count stays high, test occlusion culling. Search for
   `OccluderInstance3D`. Its absence in a blocked-view level is a candidate,
   not proof.
4. **2D batching.** High canvas draw calls with repeated sprites or text point
   to preserving automatic 2D batches. Search for `CanvasGroup`, unique
   `CanvasItemMaterial` or `ShaderMaterial` resources, frequent texture
   alternation, clips and large transparent sprites. Remove `CanvasGroup` only
   in a scratch A/B: the visual compositing can need it.
5. **Server and MultiMesh path.** Count the homogeneous nodes and scenes.
   Search: `rg -n 'MultiMesh|RenderingServer|PhysicsServer'`. Signature:
   thousands of nodes, high process or render CPU time, and many draw calls.
6. **Thread model.** Test the separate render-thread model only when global or
   render CPU setup is material and the GPU time has headroom. Reject it if
   p99 gets worse or any target-platform stress scenario fails.

## GPU

Technique file: [render-gpu.md](render-gpu.md).

**Signature.** The summed viewport GPU time is near the budget while process
and physics stay low, and halving `scaling_3d_scale` gives a large drop.

1. **Pixel cost.** Halve `scaling_3d_scale`. A large GPU-time and wall-time
   drop identifies pixel-bound 3D work: lower the render scale, use
   LightmapGI, constrain shadows, remove post effects, tune shadow maps,
   reduce real-time GI, and simplify shaders. Toggle GI and post effects 1 at
   a time. The Visual Profiler category names identify the owner.
2. **Window resolution.** Then change the window resolution. A response that
   the 3D scale does not explain points to 2D, UI, composition or another
   viewport.
3. **Pass name.** Use the Visual Profiler or `--gpu-profile` to name the
   costly pass. Then capture 1 matching frame with RenderDoc or the platform
   GPU tool.
4. **Shaders and materials.** GPU time scales with resolution while
   primitives stay stable, or RenderDoc names long fragment draws. Search the
   `.gdshader` files for `discard`, `hint_screen_texture`, loops, texture
   samples, dynamic `if` and alpha blend modes. Search the scripts and scenes
   for `.duplicate()` on materials, and convert eligible values to
   `instance uniform`.
5. **Deep capture.** After a Visual Profiler category names the costly pass,
   capture 1 representative frame in RenderDoc. Inspect the event count,
   render-target sizes, repeated state and material switches, overdraw, and
   the longest draw or dispatch. Do not infer a gain from a capture. Confirm
   the change with interleaved A/B frame times.

## Physics

Technique file: [physics-servers-threads.md](physics-servers-threads.md).

**Signature.** `TIME_PHYSICS_PROCESS`, `PHYSICS_*_COLLISION_PAIRS`,
`PHYSICS_*_ACTIVE_OBJECTS` or `TIME_NAVIGATION_PROCESS` own the time.

Read the monitors correctly:

- `Performance.TIME_PHYSICS_PROCESS` is 1 physics frame, not every rendered
  frame. Match it to the number of ticks and to the wall frame time.
- `Performance.TIME_NAVIGATION_PROCESS` includes navigation map updates and
  avoidance, but not script-side time around path-query callers. Add a custom
  time monitor for those callers.
- Read `PHYSICS_*_ACTIVE_OBJECTS`, `PHYSICS_*_COLLISION_PAIRS` and
  `PHYSICS_*_ISLAND_COUNT` together. A time drop with changed counts can mean
  that the scenarios are not equal.
- Compare equal play duration, not equal frame counts. A 240 fps run otherwise
  covers 4 times less simulated time than a 60 fps run.

1. **Cadence and spiral risk.** Search:
   `rg -n "physics_ticks_per_second|max_physics_steps_per_frame|physics_interpolation|_physics_process" --glob 'project.godot' --glob '*.gd' --glob '*.cs' .`.
   Flag physics above 60 TPS without a measured need, interpolation off at
   high frame rates, and a step cap below the intended worst-case catch-up. In
   the Profiler, compare `TIME_PHYSICS_PROCESS` with the 3.33 to 5.00 ms frame
   budget.
2. **Physics backend and thread.** Search:
   `rg -n "physics/3d/physics_engine|run_on_separate_thread|jolt_physics_3d" project.godot`.
   If physics dominates the main-thread trace, benchmark Jolt and the separate
   physics thread as independent variants.
3. **Pair pruning.** Search:
   `rg -n "collision_(layer|mask)|set_collision_(layer|mask)_value|monitoring|monitorable|max_contacts_reported|contact_monitor" --glob '*.{gd,cs,tscn}'`.
   A high `PHYSICS_*_COLLISION_PAIRS` count, or one that grows while the
   entity count stays flat, points to layer and mask design.
4. **Shape choice.** Search:
   `rg -n "ConcavePolygonShape3D|ConvexPolygonShape3D|CylinderShape3D|HeightMapShape3D|convex_decomposition" --glob '*.{gd,tscn,tres}'`.
   Inspect every concave shape on a moving body and every prop decomposed into
   many hulls.
5. **Areas and queries.** Search:
   `rg -n "Area[23]D|get_overlapping_|intersect_(ray|shape|point)|RayCast[23]D|ShapeCast[23]D|force_raycast_update" --glob '*.{gd,cs,tscn}'`.
   Many always-enabled casts, or 1 Area per agent, merit an equal-work A/B.
6. **Sleeping and contact cost.** In Monitors, compare the active bodies with
   the total rigid bodies after the scene settles. Search:
   `rg -n "can_sleep\s*=\s*false|sleeping\s*=\s*false|contact_monitor|max_contacts_reported" --glob '*.{gd,cs,tscn}'`.
7. **Server candidates and stalls.** Search:
   `rg -n "RenderingServer|PhysicsServer[23]D|body_get_state|multimesh_get_buffer|instance_get" --glob '*.{gd,cs}'`.
   Thousands of simple nodes and a high SceneTree or process time point to
   RIDs. Getters inside frame callbacks point to synchronization stalls.
8. **Navigation.** Search:
   `rg -n "NavigationAgent|NavigationServer|target_position|query_path|avoidance_enabled|neighbor_distance|max_neighbors|bake_from_source" --glob '*.{gd,cs,tscn}'`.
   Spikes in `TIME_NAVIGATION_PROCESS`, all agents repathing together, or
   spikes on unreachable targets point to asynchronous navigation sync,
   bounded path searches, avoidance partitioning and runtime parse stalls.
9. **Worker jobs.** Search:
   `rg -n "WorkerThreadPool|add_group_task|wait_for_.*completion|process_thread_group" --glob '*.{gd,cs,tscn}'`.
   A wait just after the dispatch, 1 trivial item per group index, or tree
   access in the worker is a red flag.
10. **Frame-rate test.** Run 1 fixed gameplay duration at 200 fps, at 240 fps
    and uncapped. A system whose call count follows rendered frames, not
    physics ticks, is a likely main-thread hot spot. A system whose cost
    follows TPS belongs in physics and navigation tuning.

## Spikes

Technique file: [native-memory-hitches.md](native-memory-hitches.md). For
pipeline compilation, also [render-gpu.md](render-gpu.md).

**Signature.** A slow frame lines up with a pipeline `SURFACE` or `DRAW`
counter delta, a load, an `instantiate()` or `add_child()` event, or a
managed GC event. Keep a preallocated ring of frame events and dump the frames
around each frame over the budget. Attribute each top spike to the main,
render, physics or GPU split and to the nearby event. Leave an unattributed
spike explicit.

1. **Pipeline compilation.** Replay the first-use events while you watch the 5
   pipeline compilation monitors. A gameplay-time `Surface` or `Draw`
   increment on the hitch frame points to pipeline precompilation. Search for
   runtime `instantiate()`, `load()`, material overrides, particles and
   graphics-feature toggles, and:
   `rg -n '\.(gdshader|material|tres)|ShaderMaterial|Particle|material_override|MSAA|VoxelGI|SDFGI'`.
2. **Threaded loading and staged integration.** Search:
   `rg -n '\b(load|ResourceLoader\.load|GD\.Load|LoadThreadedGet|load_threaded_get|instantiate|Instantiate|add_child|AddChild)\b' --glob '*.{gd,cs}'`.
   Signature: 1 spike at the request, get, instantiate or add, with a jump in
   process time and node count.
3. **C# GC.** Search:
   `rg -n '(_Process|_PhysicsProcess|foreach|Select\(|Where\(|ToList\(|new |\$\")' --glob '*.cs'`.
   Signature: the allocation rate, the Gen 0 count or a GC pause rises beside
   the p99 spikes. A GC event that lines up with a slow frame is an evidence
   trigger.
4. **Buffer reuse.** Search:
   `rg -n '\.(append|push_back|Add|ToArray|ToList|resize|Resize)\(' --glob '*.{gd,cs,rs,cpp,hpp}'`.
   Signature: periodic capacity growth and copies, or allocation bursts.
5. **Scene-tree churn.** Search:
   `rg -n '(add_child|remove_child|queue_free|AddChild|RemoveChild|QueueFree|instantiate|Instantiate)' --glob '*.{gd,cs}'`.
   Signature: node, object or active-body count jumps, or a frame heavy with
   `_ready` calls and notifications.
6. **Script-built scenes.** Search:
   `rg -n '(Node|Node2D|Node3D|Control)\.new\(|new (Node|Node2D|Node3D|Control)' --glob '*.{gd,cs}'`.
   Signature: many script and native calls during construction.
7. **Pooling.** Search:
   `rg -n '(pool|Pool|queue_free|QueueFree|instantiate|Instantiate)' --glob '*.{gd,cs}'`.
   Signature: a repeating spawn or despawn spike. Compare it with the steady
   cost of the pooled nodes.
8. **Resource residency.** Search:
   `rg -n 'CACHE_MODE|CacheMode|preload\(|GD\.Load|ResourceLoader' --glob '*.{gd,cs}'`.
   Signature: return visits reload, or memory has an unbounded slope or OS
   pressure.
9. **Boundary loops.** A loop that makes thousands of native calls per frame
   is an evidence trigger. Batch it into coarse calls with packed buffers (see
   Script CPU item 8).

## Unexplained wall time and drift

- **High wall time, low categories.** If wall time is high but every built-in
  category is low, sample the native process. Look for driver calls, locks,
  GDExtension and Rust functions, allocator work, file I/O and worker waits.
  Add Tracy zones only around the native region that sampling did not explain.
  This is the escalation ladder in [measure-harness.md](measure-harness.md).
- **Drift.** If results drift over consecutive takes, compare the
  baseline-first and baseline-last p50, the thermal state, the process count,
  the foreground state and the renderer. Invalidate the take. Do not blame the
  drift on code.

## Acceptance

- Run 3 to 5 interleaved A/B pairs with V-Sync disabled and `--max-fps 0`.
  Report p50, p95, p99, the worst frame, `TIME_PROCESS`,
  `TIME_PHYSICS_PROCESS` and identical game-state results. A lower average
  with a worse p99 does not meet the high-refresh goal.
- In CI, `hfr_bench.py compare` prints exactly 1 of `PASS`,
  `FRAME_TIME_REGRESSION`, `<target>_FPS_BUDGET_FAILURE` or
  `INFRASTRUCTURE_INVALID`. Keep the summary JSON and the raw frames of every
  take that is not a pass.

## Sources

- Script CPU: [Godot 4.7 The Profiler](https://docs.godotengine.org/en/4.7/tutorials/scripting/debug/the_profiler.html) and [Performance class](https://docs.godotengine.org/en/4.7/classes/class_performance.html).
- Render thread and GPU: [RenderingServer class](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html) and [resolution scaling](https://docs.godotengine.org/en/4.7/tutorials/3d/resolution_scaling.html).
- Physics: [Performance class](https://docs.godotengine.org/en/4.7/classes/class_performance.html).
- Spikes: [RenderingServer monitors](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html) and [.NET runtime counters](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/available-counters).
- Caps: [4.7.2 `main.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/main/main.cpp#L2251-L2268) and [4.7.2 `os.cpp`](https://github.com/godotengine/godot/blob/4.7.2-stable/core/os/os.cpp#L690-L735).
