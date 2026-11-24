# Godot 4.7 HFR: physics, servers, and threads

Research revision: repository `39363ae342f0eae0c41c346aa12239f7381b686c`; engine checked on host: `4.7.2.stable.official.ed1daf0bf`.

## Scope and baseline

This report targets 200–300 rendered FPS: 5.00–3.33 ms per rendered frame, with 4.17 ms at 240 FPS. It covers only gaps or corrections in:

- `skills/godot-optimization/SKILL.md` and its references;
- `skills/godot-perf-measure/SKILL.md` and its references;
- `skills/godot-multithreading/SKILL.md`;
- the behavior expected by `teammates/godot-performance-engineer.md`.

The broader `godot-library` shelves already contain much of the advanced material. This report cross-checked `cpu-and-frame-budget`, `servers-and-low-level`, `threads-and-concurrency`, `engine-internals`, `physics-advanced`, and `navigation-advanced` so a future teammate should link to those shelves rather than duplicate them.

Evidence labels use **verified** for behavior checked against Godot 4.7 official class/manual documentation generated from engine source, 4.7 source links, or the installed 4.7.2 binary. No local project benchmark was run, so expected gains below are rankings, not measured promises.

## Corrections to current skills

1. `skills/godot-optimization/references/physics-tuning.md:89-92` says `max_physics_steps_per_frame` prevents the spiral of death but omits its main failure mode. At 60 TPS and the default maximum of 8, rendering below 7.5 FPS makes game time slow down. Raising TPS without raising the cap moves that threshold upward. The teammate must treat the cap as a bounded catch-up/slow-motion tradeoff, not a free optimization.
2. `skills/godot-optimization/SKILL.md:106` and `references/physics-tuning.md:108` categorically prefer `Area2D/3D` to per-frame raycasts. That is too broad. An Area continuously maintains overlap state and pairs; it is a good fit for continuous enter/exit semantics. A bounded direct query is often cheaper for sparse, staggered, or on-demand questions. Measure both under the real entity count.
3. `skills/godot-optimization/references/physics-tuning.md:63-77` calls concave collision “extremely expensive” without the compound-shape crossover. Official guidance says concave is the slowest individual shape, but a decomposition into many convex hulls can become slower than one static concave shape. Prefer primitives for dynamic bodies; use a simplified concave mesh or height map for large static geometry.
4. `skills/godot-optimization/references/physics-tuning.md:81` gives the default tick rate but does not explain HFR cadence. At 240 rendered FPS and 60 TPS, most render frames have no physics tick. Raising physics to 240 TPS quadruples simulation and `_physics_process` work. Interpolation exists specifically to avoid that cost.
5. `skills/godot-multithreading/SKILL.md:18` is correct that physics and rendering server thread safety depends on their separate-thread settings, but it should also say that enabling them is not automatically faster. Physics API access becomes restricted to physics processing, Jolt separate-thread support remains experimental in 4.7, and the rendering Separate model has known bugs.
6. `skills/godot-multithreading/SKILL.md:22` should distinguish CPU preparation from GPU synchronization. Workers can fill plain `Image`/packed-array data; creating textures, retrieving GPU-backed image data, or uploading from workers can synchronize with `RenderingServer` and stall.

## Ranked techniques

Rank order is expected gain when the named bottleneck exists. “Thread” names where time is removed or overlapped.

### 1. Keep simulation at its required TPS, not the display FPS

- **What:** Run physics/gameplay at a fixed tested rate (usually 60 TPS for responsive action) while rendering at 200–300 FPS; use built-in interpolation for smooth visuals instead of raising physics to the render rate.
- **Why/thread:** At 240 FPS, 60 TPS means one physics tick per four rendered frames. Raising to 240 TPS can make physics-server and `_physics_process` CPU cost about 4 times as frequent. This saves main/physics-thread time.
- **Measure:** Compare `Performance.TIME_PHYSICS_PROCESS`, script profiler `_physics_process` totals, and p50/p99 frame time in paired release runs. Test motion at 10 TPS to expose interpolation mistakes, then restore the intended rate.
- **Version:** 4.7; 2D interpolation since 4.3 and 3D since 4.4.
- **Source:** [Physics interpolation introduction](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/physics_interpolation_introduction.html), [quick-start guide](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/physics_interpolation_quick_start_guide.html).
- **Evidence:** Verified against 4.7 docs and installed 4.7.2.
- **Trap:** Interpolation displays state one to two ticks in the past, adds input latency, and requires all interpolated movement—including parents, tweens, and animation—to occur on physics ticks. Call `reset_physics_interpolation()` after spawn or teleport.

### 2. Move a proven heavy physics step to its separate thread

- **What:** Enable `physics/2d/run_on_separate_thread` or `physics/3d/run_on_separate_thread` only after physics is a measured main-thread blocker and the project obeys the restricted access rules.
- **Why/thread:** The physics server can overlap its simulation with main/render preparation on another CPU core. It reduces main-thread critical-path time; it does not reduce total physics work.
- **Measure:** Paired runs with the setting off/on; record wall-frame p50/p99, `TIME_PHYSICS_PROCESS`, and a tracing profile. Confirm direct-space queries and physics API calls occur only during `_physics_process`.
- **Version:** 4.7; both settings default off.
- **Source:** [Thread-safe APIs](https://docs.godotengine.org/en/4.7/tutorials/performance/thread_safe_apis.html), [ProjectSettings physics thread setting](https://docs.godotengine.org/en/4.6/classes/class_projectsettings.html#class-projectsettings-property-physics-3d-run-on-separate-thread), [Jolt thread safety](https://docs.godotengine.org/en/4.7/tutorials/physics/using_jolt_physics.html#thread-safety).
- **Evidence:** Verified against 4.7 docs.
- **Trap:** Jolt support is explicitly experimental; errors lose node context and report `<unknown>`. A light physics step can lose to synchronization overhead.

### 3. Benchmark Jolt against Godot Physics for the actual scene

- **What:** For 3D, compare Jolt and Godot Physics with identical bodies, contacts, queries, and behavior checks; new 4.7 projects use Jolt, while upgraded projects can retain Godot Physics.
- **Why/thread:** Jolt is commonly stronger for large stacks, cylinders, soft bodies, and multicore workloads. A backend switch can reduce physics-thread time without changing gameplay code.
- **Measure:** Export two builds because changing `physics/3d/physics_engine` requires restart. Compare `TIME_PHYSICS_PROCESS`, active objects, collision pairs, island count, p99, and gameplay outcomes.
- **Version:** Jolt built in since 4.4; default for new projects since 4.6; 4.7 changes soft bodies and area overlap behavior.
- **Source:** [Using Jolt Physics](https://docs.godotengine.org/en/4.7/tutorials/physics/using_jolt_physics.html), [4.7 migration notes](https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html).
- **Evidence:** Verified against 4.7 docs.
- **Trap:** Backends are not behavior-identical. Jolt ignores some joint properties, ray `face_index` costs extra trimesh memory when enabled, and 4.7 requires soft-body retuning.

### 4. Remove collision pairs with directional layer/mask design

- **What:** Give every body and Area the smallest collision layer/mask combination; make one-way sensors detect only the needed category and disable unused Area directions with `monitoring` or `monitorable`.
- **Why/thread:** Pair filtering prevents unnecessary narrow-phase/contact/overlap work on the physics thread. It is more valuable than micro-optimizing callbacks after pairs already exist.
- **Measure:** Watch `Performance.PHYSICS_2D_COLLISION_PAIRS` or `PHYSICS_3D_COLLISION_PAIRS` while entity counts remain fixed; compare `TIME_PHYSICS_PROCESS` and validate required collisions.
- **Version:** 4.7; Jolt 4.7 additionally reports `Area3D`/`SoftBody3D` overlaps, making masks important after upgrade.
- **Source:** [Area3D overlap filtering](https://docs.godotengine.org/en/4.7/classes/class_area3d.html), [Performance monitors](https://docs.godotengine.org/en/4.6/classes/class_performance.html), [4.7 migration notes](https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html).
- **Evidence:** Verified against 4.7 class docs.
- **Trap:** A mask change can silently remove gameplay events. Test every intended interaction matrix, not only frame time.

### 5. Use primitives for moving bodies and purpose-built static shapes

- **What:** Prefer sphere/box/capsule for dynamic bodies; use a small compound only when needed; use simplified concave geometry or `HeightMapShape3D` for large static terrain.
- **Why/thread:** Cheaper shape tests reduce narrow-phase physics-thread time. A single non-transformed static shape also gives the broad phase its best discard path.
- **Measure:** Hold body count and motion fixed; compare `TIME_PHYSICS_PROCESS`, collision pairs, and a microbenchmark scene with the same contacts. Inspect the collision-shape debug view.
- **Version:** 4.7.
- **Source:** [Shape3D performance order](https://docs.godotengine.org/en/4.7/classes/class_shape3d.html), [3D collision shapes](https://docs.godotengine.org/en/4.5/tutorials/physics/collision_shapes_3d.html), [Capsule cost](https://docs.godotengine.org/en/stable/classes/class_capsuleshape3d.html).
- **Evidence:** Verified against class docs.
- **Trap:** Convex decomposition can create dozens of hulls and cross over into being slower than one static concave mesh. Never use concave shapes for moving bodies.

### 6. Replace persistent Areas with bounded direct queries when semantics allow

- **What:** For an occasional “who is here now?” check, reuse a `PhysicsShapeQueryParameters2D/3D` and call `intersect_shape` with the smallest useful `max_results` instead of keeping one Area per actor.
- **Why/thread:** This removes always-live Area overlap tracking, signals, and nodes; it spends physics-thread time only when queried.
- **Measure:** Compare fixed-count scenes with Areas versus staggered queries. Record collision pairs, object/node count, `TIME_PHYSICS_PROCESS`, and the query caller’s custom timer.
- **Version:** 4.7.
- **Source:** [PhysicsDirectSpaceState3D](https://docs.godotengine.org/en/4.7/classes/class_physicsdirectspacestate3d.html), [Area3D](https://docs.godotengine.org/en/4.7/classes/class_area3d.html).
- **Evidence:** Verified against 4.7 APIs; relative gain must be measured.
- **Trap:** Queries allocate result containers in the classic API and give snapshots, not enter/exit events. Persistent Areas can win when every overlap must be tracked every tick.

### 7. Batch and stagger ray/shape questions through direct space state

- **What:** Replace many always-enabled `RayCast2D/3D` nodes with direct `intersect_ray`, `cast_motion`, or `intersect_shape` calls only on the ticks that need answers; reuse parameter objects.
- **Why/thread:** A `RayCast3D` computes every physics frame. On-demand and phased queries reduce node callbacks and physics-thread query count.
- **Measure:** Count enabled RayCast/ShapeCast nodes, time the query loop with a custom time monitor, and compare `TIME_PHYSICS_PROCESS` and p99. Keep result count and gameplay outcomes equal.
- **Version:** 4.7.
- **Source:** [RayCast3D update behavior](https://docs.godotengine.org/en/4.4/classes/class_raycast3d.html), [PhysicsDirectSpaceState3D](https://docs.godotengine.org/en/4.7/classes/class_physicsdirectspacestate3d.html).
- **Evidence:** Verified against class docs.
- **Trap:** Direct space access belongs in `_physics_process`, especially with threaded physics. Spreading safety-critical rays over ticks changes collision response.

### 8. Let rigid bodies sleep and pay for contact reports only where consumed

- **What:** Keep `can_sleep` enabled, stop per-tick writes that wake idle bodies, and set `contact_monitor`/`max_contacts_reported` only on bodies whose code reads contacts.
- **Why/thread:** Sleeping removes inactive bodies from integration; contact logs add per-contact physics work and script-facing data.
- **Measure:** Watch `PHYSICS_2D/3D_ACTIVE_OBJECTS`, collision pairs, `TIME_PHYSICS_PROCESS`, and contact counts in the same idle/active scenario.
- **Version:** 4.7.
- **Source:** [PhysicsServer3D contact reporting and sleep state](https://docs.godotengine.org/en/4.7/classes/class_physicsserver3d.html), [Performance monitors](https://docs.godotengine.org/en/4.6/classes/class_performance.html).
- **Evidence:** Verified against 4.7 APIs.
- **Trap:** Sleeping is wrong for bodies that must respond immediately to non-physics state. Too-low contact caps silently omit contacts.

### 9. Use server-only physics bodies for thousands of simple entities

- **What:** Create bodies/shapes by RID through `PhysicsServer2D/3D`, store gameplay state in arrays, and receive active-body state with `body_set_state_sync_callback` rather than polling server getters.
- **Why/thread:** This removes SceneTree nodes, per-node callbacks, and node-to-server forwarding from the main thread while retaining physics-server collision.
- **Measure:** Compare node/object counts, process time, physics time, and frame p99 at equal body counts. Attribute a custom timer to the manager and callback.
- **Version:** 4.7.
- **Source:** [Optimization using Servers](https://docs.godotengine.org/en/4.6/tutorials/performance/using_servers.html), [PhysicsServer3D](https://docs.godotengine.org/en/4.7/classes/class_physicsserver3d.html).
- **Evidence:** Verified against 4.7 API and server docs.
- **Trap:** The owner must free every RID and keep shape resources alive. Server-only objects disappear from the remote scene tree, increasing debugging cost.

### 10. Treat servers as write-mostly and avoid getters in hot paths

- **What:** Cache authoritative gameplay state outside `RenderingServer`/`PhysicsServer`; send transforms and properties, but avoid per-frame getters such as body state or buffer reads.
- **Why/thread:** Servers often process asynchronously. A getter can force queued work to complete and synchronize the calling main/worker thread with the server.
- **Measure:** Capture a tracing profile around getter calls; remove or batch them and compare main-thread stalls and p99. Use custom timers because the regular script profiler may attribute the wait to the getter.
- **Version:** 4.7.
- **Source:** [Optimization using Servers](https://docs.godotengine.org/en/4.4/tutorials/performance/using_servers.html).
- **Evidence:** Verified against official server guidance.
- **Trap:** Duplicated state needs one clear owner. A stale cache is worse than a measured synchronization point when correctness requires current server data.

### 11. Account for 2D/3D interpolation differences with raw RIDs

- **What:** Low-level 2D canvas/physics objects can use server interpolation; low-level 3D instances/bodies need manual previous/current transform blending with `Engine.get_physics_interpolation_fraction()`.
- **Why/thread:** Correct manual interpolation allows low-rate physics plus node-free 3D rendering without increasing physics-thread frequency.
- **Measure:** Test at 10 TPS and capture motion cadence; then compare 60 TPS interpolated against a higher-TPS baseline using frame time and visual checks.
- **Version:** In 4.5, 3D interpolation moved from `RenderingServer` to `SceneTree`; applies to 4.7.
- **Source:** [2D and 3D interpolation differences](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/2d_and_3d_physics_interpolation.html), [physics interpolation introduction](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/physics_interpolation_introduction.html).
- **Evidence:** Verified against 4.7 docs.
- **Trap:** Applying both manual and engine interpolation double-smooths motion. Teleports need interpolation reset or previous=current.

### 12. Use asynchronous navigation sync; do not force immediate map updates

- **What:** Leave `navigation/world/map_use_async_iterations` and `region_use_async_iterations` enabled, queue changes, and wait for `map_changed` or a later physics frame instead of forcing synchronous expectations.
- **Why/thread:** Map and region synchronization can run in the background, trading update latency for lower main-thread spikes.
- **Measure:** Track `Performance.TIME_NAVIGATION_PROCESS`, p99 physics/frame time, map iteration IDs, and latency in physics ticks from mutation to usable map.
- **Version:** Async map sync since 4.4; region/link updates since 4.5; applies to 4.7.
- **Source:** [Using NavigationServer](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationservers.html), [4.7 version ledger basis](https://godotengine.org/releases/4.5/).
- **Evidence:** Verified against 4.7 navigation docs.
- **Trap:** Changes are not visible immediately. Logic that queries in `_ready()` or the same tick can see an empty or old map.

### 13. Bound path searches, especially for unreachable targets

- **What:** Reuse path query parameter/result objects, omit unused metadata, and set `path_search_max_polygons`, `path_search_max_distance`, or region filters for known search horizons.
- **Why/thread:** An unreachable target can search every connected polygon. Bounds reduce NavigationServer worker time and tail spikes.
- **Measure:** Create reachable and unreachable worst cases; time each query, record p99 and `TIME_NAVIGATION_PROCESS`, and validate path endpoints/quality.
- **Version:** Search limits and region filters since 4.5; applies to 4.7.
- **Source:** [Navigation path query limits](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationpathqueryobjects.html).
- **Evidence:** Verified against 4.7 docs.
- **Trap:** Limits that are too low can return partial paths in the wrong direction. Return clipping alone does not reduce search work.

### 14. Disable or partition avoidance before tuning its threads

- **What:** Enable RVO avoidance only for agents that need local crowd steering; lower `neighbor_distance` and `max_neighbors`, split avoidance layers/masks, and pause off-screen agents.
- **Why/thread:** Avoidance runs through the navigation thread pool but still consumes CPU and must dispatch callbacks in the physics frame. Reducing neighbor sets cuts navigation/physics critical-path work.
- **Measure:** Watch `TIME_NAVIGATION_PROCESS`, active agent/obstacle monitors, and p99 while sweeping agent count, neighbor distance, and max neighbors.
- **Version:** 4.7; avoidance uses multiple high-priority threads by default where supported.
- **Source:** [Using NavigationServer](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationservers.html), [NavigationAgent avoidance properties](https://docs.godotengine.org/en/4.4/tutorials/navigation/navigation_using_navigationagents.html), [navigation thread settings](https://docs.godotengine.org/en/4.6/classes/class_projectsettings.html).
- **Evidence:** Verified against official docs.
- **Trap:** Low neighbor caps allow agents to ignore nearby actors. Avoidance does not know the navmesh or physics and cannot replace collision/path logic.

### 15. Remove runtime navigation parse stalls before optimizing the bake

- **What:** Parse source geometry on the main thread once, prefer static colliders over visual meshes, then call `bake_from_source_geometry_data_async`; for generated terrain, fill source geometry directly from arrays.
- **Why/thread:** Scene-tree parsing and GPU mesh readback can stall the main/render thread; asynchronous baking moves the thread-safe build stage off it.
- **Measure:** Time parse, bake, and assignment separately with custom monitors; compare hitch count and p99 during a runtime rebuild.
- **Version:** 4.7.
- **Source:** [Navigation meshes](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationmeshes.html), [thread-safe APIs](https://docs.godotengine.org/en/4.7/tutorials/performance/thread_safe_apis.html).
- **Evidence:** Verified against 4.7 docs.
- **Trap:** `parse_source_geometry_data` reads the scene tree and stays on the main thread. Heavy `_ready()` work can merely move the hitch to attachment/assignment.

### 16. Chunk WorkerThreadPool group tasks and poll before joining

- **What:** Use `add_group_task` for expensive independent batches, with one group index processing a chunk of items; preallocate output, poll completion across frames, then call `wait_for_group_task_completion` once to release task resources.
- **Why/thread:** Coarse batches amortize GDScript dispatch and pool scheduling while using multiple worker cores. Polling prevents the main thread from blocking on unfinished work.
- **Measure:** Time single-thread, per-item group, and chunked group variants; record wall time, main-thread p99, worker-pool contention, and equal output hashes.
- **Version:** 4.7.
- **Source:** [WorkerThreadPool class](https://docs.godotengine.org/en/4.7/classes/class_workerthreadpool.html), [worker pool source](https://github.com/godotengine/godot/blob/4.7-stable/core/object/worker_thread_pool.h).
- **Evidence:** Verified against 4.7 API/source and installed binary.
- **Trap:** Cheap tasks get slower. The engine shares the pool, container resize needs locking, every task must be waited once, and a same-frame join simply moves the wait into the frame.

### 17. Use node process thread groups only for isolated subtrees

- **What:** Set an isolated subsystem root to `PROCESS_THREAD_GROUP_SUB_THREAD`; communicate with other groups using `call_deferred_thread_group`, `call_thread_safe`, or thread-safe data handoff.
- **Why/thread:** A self-contained subtree’s `_process`/`_physics_process` callbacks can overlap main-thread work without manually dispatching every node.
- **Measure:** Compare main-thread script time and p99 with group main/sub-thread modes; run a debug build to surface forbidden cross-group access.
- **Version:** Available in Godot 4.3+; applies to 4.7.
- **Source:** [Node process thread groups](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-property-process-thread-group).
- **Evidence:** Verified against stable/4.7 class API.
- **Trap:** Most node access outside the group is forbidden, many engine nodes assume main-thread behavior, and synchronization can erase the gain. Prefer pure data jobs first.

### 18. Respect Jolt capacity limits and optional contact features

- **What:** Size `physics/jolt_physics_3d/limits/max_bodies` and `max_body_pairs` for the real peak, and leave costly optional data such as ray face indices or all kinematic contacts disabled unless consumed.
- **Why/thread:** Avoiding unnecessary contact generation and trimesh metadata saves physics-thread CPU/memory; adequate hard limits prevent dropped/random behavior at scale.
- **Measure:** Stress beyond expected peak while watching active bodies, collision pairs, physics time, errors, and memory. A/B each optional setting separately.
- **Version:** Built-in Jolt in 4.7; defaults include 10,240 bodies and 65,536 body pairs.
- **Source:** [Using Jolt Physics](https://docs.godotengine.org/en/4.7/tutorials/physics/using_jolt_physics.html), [ProjectSettings Jolt limits](https://docs.godotengine.org/en/4.5/classes/class_projectsettings.html).
- **Evidence:** Verified against official docs.
- **Trap:** Increasing limits allocates more memory and does not fix a bad pair matrix. Exceeding documented limits can drop collisions or produce undefined behavior.

## Measurement notes for 200–300 FPS

- Measure release exports with V-Sync disabled and no frame cap, then report milliseconds and p50/p95/p99. At 300 FPS, a 0.2 ms change is already 6% of the budget.
- `Performance.TIME_PHYSICS_PROCESS` is one physics frame, not every rendered frame. Correlate it with the number of ticks that occurred and with wall-frame time.
- `Performance.TIME_NAVIGATION_PROCESS` includes navigation map updates and avoidance, but not arbitrary script-side timing around path-query callers. Add a custom time monitor for those callers.
- Compare equal play duration, not equal render-frame count. A 240 FPS run otherwise covers four times less simulated time than a 60 FPS run.
- Use `PHYSICS_*_ACTIVE_OBJECTS`, `PHYSICS_*_COLLISION_PAIRS`, and `PHYSICS_*_ISLAND_COUNT` together. A time drop with changed counts can mean the scenarios are not equivalent.

## Detection

Use this sequence in a real project to find where the highest-ranked techniques apply:

1. **Cadence and spiral risk:** `rg -n "physics_ticks_per_second|max_physics_steps_per_frame|physics_interpolation|_physics_process" --glob 'project.godot' --glob '*.gd' --glob '*.cs' .`. Flag TPS above 60 without a measured requirement, interpolation off at HFR, or a cap below the intended worst-case catch-up. In the profiler, compare `TIME_PHYSICS_PROCESS` to the 3.33–5.00 ms frame budget.
2. **Physics backend/thread:** `rg -n "physics/3d/physics_engine|run_on_separate_thread|jolt_physics_3d" project.godot`. If physics dominates the main-thread trace, benchmark Jolt/backend and separate-thread variants independently.
3. **Pair pruning:** `rg -n "collision_(layer|mask)|set_collision_(layer|mask)_value|monitoring|monitorable|max_contacts_reported|contact_monitor" --glob '*.{gd,cs,tscn}'`. A high or count-growing `PHYSICS_*_COLLISION_PAIRS` monitor with flat entity count points here.
4. **Shape choice:** `rg -n "ConcavePolygonShape3D|ConvexPolygonShape3D|CylinderShape3D|HeightMapShape3D|convex_decomposition" --glob '*.{gd,tscn,tres}'`. Inspect every concave shape on a moving body and every prop decomposed into many hulls.
5. **Areas and queries:** `rg -n "Area[23]D|get_overlapping_|intersect_(ray|shape|point)|RayCast[23]D|ShapeCast[23]D|force_raycast_update" --glob '*.{gd,cs,tscn}'`. Many always-enabled casts or one Area per agent merit an equal-work A/B.
6. **Sleeping/contact cost:** In Monitors, compare active bodies with total rigid bodies after the scene settles. Search `rg -n "can_sleep\s*=\s*false|sleeping\s*=\s*false|contact_monitor|max_contacts_reported" --glob '*.{gd,cs,tscn}'`.
7. **Server candidates and stalls:** `rg -n "RenderingServer|PhysicsServer[23]D|body_get_state|multimesh_get_buffer|instance_get" --glob '*.{gd,cs}'`. Thousands of simple nodes plus high SceneTree/process time suggest RIDs; getters inside frame callbacks suggest synchronization stalls.
8. **Navigation:** `rg -n "NavigationAgent|NavigationServer|target_position|query_path|avoidance_enabled|neighbor_distance|max_neighbors|bake_from_source" --glob '*.{gd,cs,tscn}'`. Spikes in `TIME_NAVIGATION_PROCESS`, all agents repathing together, or unreachable-target spikes indicate ranks 12–15.
9. **Worker jobs:** `rg -n "WorkerThreadPool|add_group_task|wait_for_.*completion|process_thread_group" --glob '*.{gd,cs,tscn}'`. A wait immediately after dispatch, one trivial item per group index, or tree access in the worker is a red flag.
10. **HFR acceptance:** Run one fixed gameplay duration at 200, 240, and uncapped FPS. A system whose call count follows rendered frames instead of physics ticks is a likely main-thread HFR hotspot; a system whose cost follows TPS belongs in physics/navigation tuning.
