# Physics, servers and threads

Use this page when the hot spot is physics, servers, navigation or worker
threads at 200 to 300 rendered fps (5.00, 4.17 and 3.33 ms budgets). The
source report was checked against the Godot 4.7 documentation and source and
the installed `4.7.2.stable.official.ed1daf0bf` binary. No project benchmark
was run. "Why/thread" names the thread where time is removed or overlapped.

## Measurement notes

- `Performance.TIME_PHYSICS_PROCESS` is 1 physics frame, not every rendered frame. Correlate it with the number of ticks and with the wall-frame time.
- `Performance.TIME_NAVIGATION_PROCESS` includes map updates and avoidance, but not script time around path-query callers. Add a custom time monitor for those callers.
- Read `PHYSICS_*_ACTIVE_OBJECTS`, `PHYSICS_*_COLLISION_PAIRS` and `PHYSICS_*_ISLAND_COUNT` together. A time drop with changed counts can mean the scenarios are not equal.

## Ranked techniques

### 1. Keep simulation at its required TPS, not the display FPS

- **What:** Run physics and gameplay at a fixed tested rate, usually 60 TPS for responsive action, and render at 200 to 300 fps. Use built-in physics interpolation for smooth visuals. Do not raise physics to the render rate.
- **Why/thread:** Main and physics thread. At 240 fps, 60 TPS is 1 physics tick per 4 rendered frames. 240 TPS runs the physics server and `_physics_process` about 4 times as often.
- **Measure:** Compare `Performance.TIME_PHYSICS_PROCESS`, the Profiler `_physics_process` totals and p50 and p99 frame time in paired release runs. Test motion at 10 TPS to expose interpolation mistakes, then restore the intended rate.
- **Version:** Godot 4.7. 2D interpolation exists since 4.3 and 3D since 4.4.
- **Source:** [Physics interpolation introduction](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/physics_interpolation_introduction.html) and [quick-start guide](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/physics_interpolation_quick_start_guide.html).
- **Evidence:** Verified against the 4.7 documentation and the installed 4.7.2 binary.
- **Trap:** Interpolation shows state 1 to 2 ticks in the past and adds input latency. All interpolated movement, including parents, tweens and animation, must happen on physics ticks. Call `reset_physics_interpolation()` after a spawn or a teleport.

### 2. Move a proven heavy physics step to its own thread

- **What:** Enable `physics/2d/run_on_separate_thread` or `physics/3d/run_on_separate_thread` only when physics is a measured main-thread blocker and the project obeys the access rules.
- **Why/thread:** Main thread. The physics server overlaps its step with main and render preparation on another core. This shortens the main-thread critical path. It does not lower the total physics work.
- **Measure:** Paired runs with the setting off and on. Record wall-frame p50 and p99, `TIME_PHYSICS_PROCESS` and a tracing profile. Confirm that direct-space queries and physics API calls happen only in `_physics_process`.
- **Version:** Godot 4.7. Both settings are off by default.
- **Source:** [Thread-safe APIs](https://docs.godotengine.org/en/4.7/tutorials/performance/thread_safe_apis.html), [physics thread setting](https://docs.godotengine.org/en/4.6/classes/class_projectsettings.html#class-projectsettings-property-physics-3d-run-on-separate-thread) and [Jolt thread safety](https://docs.godotengine.org/en/4.7/tutorials/physics/using_jolt_physics.html#thread-safety).
- **Evidence:** Verified against the 4.7 documentation.
- **Trap:** Jolt support for it is experimental, and its errors lose node context and report `<unknown>`. A light physics step can lose to the synchronization cost.

### 3. Benchmark Jolt against Godot Physics for the real scene

- **What:** In 3D, compare Jolt and Godot Physics with the same bodies, contacts, queries and behavior checks. New 4.7 projects use Jolt. Upgraded projects can keep Godot Physics.
- **Why/thread:** Physics thread. Jolt is often stronger for large stacks, cylinders, soft bodies and multicore loads. A backend switch can lower physics time without gameplay code changes.
- **Measure:** Export 2 builds, because a change to `physics/3d/physics_engine` needs a restart. Compare `TIME_PHYSICS_PROCESS`, active objects, collision pairs, island count, p99 and gameplay outcomes.
- **Version:** Jolt is built in since 4.4 and the default for new projects since 4.6. 4.7 changes soft bodies and area overlap behavior.
- **Source:** [Using Jolt Physics](https://docs.godotengine.org/en/4.7/tutorials/physics/using_jolt_physics.html) and [4.7 migration notes](https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html).
- **Evidence:** Verified against the 4.7 documentation.
- **Trap:** The backends do not behave the same. Jolt ignores some joint properties, ray `face_index` costs extra trimesh memory when enabled, and 4.7 needs soft bodies tuned again.

### 4. Remove collision pairs with directional layer and mask design

- **What:** Give every body and Area the smallest layer and mask set. Make one-way sensors detect only the needed category, and turn off unused Area directions with `monitoring` or `monitorable`.
- **Why/thread:** Physics thread. Pair filtering prevents narrow-phase, contact and overlap work. It gives more than callback micro-optimization after the pairs exist.
- **Measure:** Watch `Performance.PHYSICS_2D_COLLISION_PAIRS` or `PHYSICS_3D_COLLISION_PAIRS` while the entity count stays fixed. Compare `TIME_PHYSICS_PROCESS` and check the required collisions.
- **Version:** Godot 4.7. Jolt in 4.7 also reports `Area3D` and `SoftBody3D` overlaps, so masks matter more after an upgrade.
- **Source:** [Area3D](https://docs.godotengine.org/en/4.7/classes/class_area3d.html), [Performance monitors](https://docs.godotengine.org/en/4.6/classes/class_performance.html) and [4.7 migration notes](https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html).
- **Evidence:** Verified against the 4.7 class reference.
- **Trap:** A mask change can silently remove gameplay events. Test the full interaction matrix, not only the frame time.

### 5. Use primitives for moving bodies and purpose-built static shapes

- **What:** Use sphere, box or capsule shapes for dynamic bodies, and a small compound only when needed. Use simplified concave geometry or `HeightMapShape3D` for large static terrain.
- **Why/thread:** Physics thread. Cheaper shape tests lower narrow-phase time. 1 static shape without a transform gives the broad phase its best discard path.
- **Measure:** Keep the body count and motion fixed. Compare `TIME_PHYSICS_PROCESS`, collision pairs and a microbenchmark scene with the same contacts. Inspect the collision-shape debug view.
- **Version:** Godot 4.7.
- **Source:** [Shape3D performance order](https://docs.godotengine.org/en/4.7/classes/class_shape3d.html), [3D collision shapes](https://docs.godotengine.org/en/4.5/tutorials/physics/collision_shapes_3d.html) and [capsule cost](https://docs.godotengine.org/en/stable/classes/class_capsuleshape3d.html).
- **Evidence:** Verified against the class reference.
- **Trap:** Convex decomposition can make dozens of hulls and become slower than 1 static concave mesh. Never use a concave shape on a moving body.

### 6. Replace persistent Areas with bounded direct queries when behavior allows

- **What:** For an occasional "who is here now?" check, reuse a `PhysicsShapeQueryParameters2D` or `PhysicsShapeQueryParameters3D` and call `intersect_shape` with the smallest useful `max_results`. Do not keep 1 Area per actor.
- **Why/thread:** Physics and main thread. This removes always-live Area overlap tracking, signals and nodes, and spends physics time only when you query.
- **Measure:** Compare fixed-count scenes with Areas against staggered queries. Record collision pairs, object and node count, `TIME_PHYSICS_PROCESS` and a custom timer on the query caller.
- **Version:** Godot 4.7.
- **Source:** [PhysicsDirectSpaceState3D](https://docs.godotengine.org/en/4.7/classes/class_physicsdirectspacestate3d.html) and [Area3D](https://docs.godotengine.org/en/4.7/classes/class_area3d.html).
- **Evidence:** Verified against the 4.7 API. Measure the gain in your scene.
- **Trap:** The classic query API allocates result containers and gives snapshots, not enter and exit events. Persistent Areas can win when every overlap must be tracked every tick.

### 7. Batch and stagger ray and shape questions through direct space state

- **What:** Replace many always-enabled `RayCast2D` and `RayCast3D` nodes with direct `intersect_ray`, `cast_motion` or `intersect_shape` calls, only on the ticks that need answers. Reuse the parameter objects.
- **Why/thread:** Physics thread. A `RayCast3D` runs every physics frame. On-demand and phased queries cut node callbacks and the query count.
- **Measure:** Count enabled RayCast and ShapeCast nodes, time the query loop with a custom time monitor, and compare `TIME_PHYSICS_PROCESS` and p99. Keep the result count and gameplay outcomes the same.
- **Version:** Godot 4.7.
- **Source:** [RayCast3D update behavior](https://docs.godotengine.org/en/4.4/classes/class_raycast3d.html) and [PhysicsDirectSpaceState3D](https://docs.godotengine.org/en/4.7/classes/class_physicsdirectspacestate3d.html).
- **Evidence:** Verified against the class reference.
- **Trap:** Direct space access belongs in `_physics_process`, most of all with threaded physics. Rays that guard safety spread over ticks change the collision response.

### 8. Let rigid bodies sleep and pay for contact reports only where read

- **What:** Keep `can_sleep` on, stop per-tick writes that wake idle bodies, and set `contact_monitor` and `max_contacts_reported` only on bodies whose code reads contacts.
- **Why/thread:** Physics thread. A sleeping body leaves integration. Contact logs add per-contact physics work and script-facing data.
- **Measure:** Watch `PHYSICS_2D_ACTIVE_OBJECTS` or `PHYSICS_3D_ACTIVE_OBJECTS`, collision pairs, `TIME_PHYSICS_PROCESS` and contact counts in the same idle and active scenario.
- **Version:** Godot 4.7.
- **Source:** [PhysicsServer3D contacts and sleep](https://docs.godotengine.org/en/4.7/classes/class_physicsserver3d.html) and [Performance monitors](https://docs.godotengine.org/en/4.6/classes/class_performance.html).
- **Evidence:** Verified against the 4.7 API.
- **Trap:** Sleep is wrong for a body that must respond at once to non-physics state. A contact cap that is too low silently drops contacts.

### 9. Use server-only physics bodies for thousands of simple entities

- **What:** Create bodies and shapes by RID through `PhysicsServer2D` or `PhysicsServer3D`, keep game state in arrays, and receive active-body state with `body_set_state_sync_callback`. Do not poll server getters.
- **Why/thread:** Main thread. This removes SceneTree nodes, per-node callbacks and node-to-server forwarding, and keeps physics-server collision.
- **Measure:** Compare node and object counts, process time, physics time and p99 frame time at equal body counts. Put a custom timer on the manager and the callback.
- **Version:** Godot 4.7.
- **Source:** [Optimization using Servers](https://docs.godotengine.org/en/4.6/tutorials/performance/using_servers.html) and [PhysicsServer3D](https://docs.godotengine.org/en/4.7/classes/class_physicsserver3d.html).
- **Evidence:** Verified against the 4.7 API and the server documentation.
- **Trap:** The owner must free every RID and keep the shape resources alive. Server-only objects do not show in the remote scene tree, so debugging costs more.

### 10. Treat servers as write-mostly and avoid getters in hot paths

- **What:** Keep the authoritative game state outside `RenderingServer` and `PhysicsServer3D`. Send transforms and properties, but do not call per-frame getters such as body state or buffer reads.
- **Why/thread:** Main and worker threads. Servers often process asynchronously. A getter can force queued work to finish and synchronize the caller with the server.
- **Measure:** Capture a tracing profile around the getter calls. Remove or batch them, and compare main-thread stalls and p99. Use custom timers, because the script Profiler can charge the wait to the getter.
- **Version:** Godot 4.7.
- **Source:** [Optimization using Servers](https://docs.godotengine.org/en/4.4/tutorials/performance/using_servers.html).
- **Evidence:** Verified against the official server guidance.
- **Trap:** Duplicated state needs 1 clear owner. A stale cache is worse than a measured synchronization point when correctness needs current server data.

### 11. Account for 2D and 3D interpolation differences with raw RIDs

- **What:** Low-level 2D canvas and physics objects can use server interpolation. Low-level 3D instances and bodies need manual blending of the previous and current transforms with `Engine.get_physics_interpolation_fraction()`.
- **Why/thread:** Physics thread. Correct manual interpolation allows low-rate physics with node-free 3D rendering, without a higher physics rate.
- **Measure:** Test at 10 TPS and capture the motion cadence. Then compare 60 TPS interpolated against a higher-TPS baseline with frame time and visual checks.
- **Version:** In 4.5, 3D interpolation moved from `RenderingServer` to `SceneTree`. This applies to 4.7.
- **Source:** [2D and 3D interpolation differences](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/2d_and_3d_physics_interpolation.html) and [physics interpolation introduction](https://docs.godotengine.org/en/4.7/tutorials/physics/interpolation/physics_interpolation_introduction.html).
- **Evidence:** Verified against the 4.7 documentation.
- **Trap:** Manual and engine interpolation together smooth the motion twice. A teleport needs an interpolation reset, or previous set equal to current.

### 12. Use asynchronous navigation sync and do not force map updates

- **What:** Keep `navigation/world/map_use_async_iterations` and `region_use_async_iterations` on, queue changes, and wait for `map_changed` or a later physics frame.
- **Why/thread:** Main thread. Map and region synchronization runs in the background. It trades update latency for smaller main-thread spikes.
- **Measure:** Track `Performance.TIME_NAVIGATION_PROCESS`, p99 physics and frame time, map iteration IDs, and the latency in physics ticks from a change to a usable map.
- **Version:** Async map sync since 4.4, and region and link updates since 4.5. This applies to 4.7.
- **Source:** [Using NavigationServer](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationservers.html) and the [4.5 release notes](https://godotengine.org/releases/4.5/).
- **Evidence:** Verified against the 4.7 navigation documentation.
- **Trap:** Changes are not visible at once. Logic that queries in `_ready()` or in the same tick can see an empty or old map.

### 13. Bound path searches, most of all for unreachable targets

- **What:** Reuse path query parameter and result objects, omit unused metadata, and set `path_search_max_polygons`, `path_search_max_distance` or region filters for known search ranges.
- **Why/thread:** NavigationServer worker. An unreachable target can search every connected polygon. Bounds lower worker time and tail spikes.
- **Measure:** Make reachable and unreachable worst cases. Time each query, record p99 and `TIME_NAVIGATION_PROCESS`, and check path endpoints and quality.
- **Version:** Search limits and region filters since 4.5. This applies to 4.7.
- **Source:** [Navigation path query objects](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationpathqueryobjects.html).
- **Evidence:** Verified against the 4.7 documentation.
- **Trap:** Limits that are too low can return partial paths in the wrong direction. Clipping the returned path alone does not lower the search work.

### 14. Disable or partition avoidance before you tune its threads

- **What:** Enable RVO avoidance only for agents that need local crowd steering. Lower `neighbor_distance` and `max_neighbors`, split avoidance layers and masks, and pause off-screen agents.
- **Why/thread:** Navigation and physics thread. Avoidance runs in the navigation thread pool, but it still uses CPU and dispatches callbacks in the physics frame. Smaller neighbor sets cut critical-path work.
- **Measure:** Watch `TIME_NAVIGATION_PROCESS`, the active agent and obstacle monitors and p99 while you sweep the agent count, neighbor distance and max neighbors.
- **Version:** Godot 4.7. Avoidance uses several high-priority threads by default where the platform supports them.
- **Source:** [Using NavigationServer](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationservers.html), [NavigationAgent avoidance](https://docs.godotengine.org/en/4.4/tutorials/navigation/navigation_using_navigationagents.html) and [navigation thread settings](https://docs.godotengine.org/en/4.6/classes/class_projectsettings.html).
- **Evidence:** Verified against the official documentation.
- **Trap:** A low neighbor cap lets agents ignore nearby actors. Avoidance does not know the navmesh or physics and cannot replace collision or path logic.

### 15. Remove run-time navigation parse stalls before you optimize the bake

- **What:** Parse source geometry on the main thread once, use static colliders instead of visual meshes, then call `bake_from_source_geometry_data_async`. For generated terrain, fill the source geometry directly from arrays.
- **Why/thread:** Main and render thread. Scene-tree parsing and GPU mesh readback can stall them. The async bake moves the thread-safe build stage off the main thread.
- **Measure:** Time the parse, the bake and the assignment apart with custom monitors. Compare the hitch count and p99 during a run-time rebuild.
- **Version:** Godot 4.7.
- **Source:** [Navigation meshes](https://docs.godotengine.org/en/4.7/tutorials/navigation/navigation_using_navigationmeshes.html) and [thread-safe APIs](https://docs.godotengine.org/en/4.7/tutorials/performance/thread_safe_apis.html).
- **Evidence:** Verified against the 4.7 documentation.
- **Trap:** `parse_source_geometry_data` reads the scene tree and stays on the main thread. Heavy `_ready()` work can only move the hitch to attachment or assignment.

### 16. Chunk WorkerThreadPool group tasks and poll before joining

- **What:** Use `add_group_task` for expensive independent batches, with 1 group index per chunk of items. Preallocate the output, poll for completion across frames, then call `wait_for_group_task_completion` once to release the task.
- **Why/thread:** Worker threads. Coarse batches spread the GDScript dispatch and pool scheduling cost over many items and use several cores. Polling keeps the main thread from blocking on unfinished work.
- **Measure:** Time single-thread, per-item group and chunked group variants. Record wall time, main-thread p99, worker-pool contention and equal output hashes.
- **Version:** Godot 4.7.
- **Source:** [WorkerThreadPool class](https://docs.godotengine.org/en/4.7/classes/class_workerthreadpool.html) and [worker pool source](https://github.com/godotengine/godot/blob/4.7-stable/core/object/worker_thread_pool.h).
- **Evidence:** Verified against the 4.7 API, the source and the installed binary.
- **Trap:** Cheap tasks get slower. The engine shares the pool, a container resize needs a lock, every task must be waited on once, and a same-frame join only moves the wait into the frame.

### 17. Use node process thread groups only for isolated subtrees

- **What:** Set the root of an isolated subsystem to `PROCESS_THREAD_GROUP_SUB_THREAD`. Talk to other groups with `call_deferred_thread_group`, `call_thread_safe` or a thread-safe data handoff.
- **Why/thread:** Main thread. The `_process` and `_physics_process` callbacks of a self-contained subtree can overlap main-thread work without manual dispatch per node.
- **Measure:** Compare main-thread script time and p99 with the group in main-thread and sub-thread modes. Run a debug build to surface forbidden cross-group access.
- **Version:** Godot 4.3 and later. This applies to 4.7.
- **Source:** [Node process thread groups](https://docs.godotengine.org/en/stable/classes/class_node.html#class-node-property-process-thread-group).
- **Evidence:** Verified against the stable and 4.7 class API.
- **Trap:** Most node access outside the group is forbidden, many engine nodes expect the main thread, and synchronization can erase the gain. Try pure data jobs first.

### 18. Respect Jolt capacity limits and optional contact features

- **What:** Size `physics/jolt_physics_3d/limits/max_bodies` and `max_body_pairs` for the real peak. Keep costly optional data, such as ray face indices or all kinematic contacts, off unless code reads it.
- **Why/thread:** Physics thread. Less contact generation and trimesh metadata saves CPU and memory. Enough headroom in the hard limits prevents dropped or random behavior at scale.
- **Measure:** Stress past the expected peak and watch active bodies, collision pairs, physics time, errors and memory. A/B each optional setting alone.
- **Version:** Built-in Jolt in 4.7. The defaults are 10,240 bodies and 65,536 body pairs.
- **Source:** [Using Jolt Physics](https://docs.godotengine.org/en/4.7/tutorials/physics/using_jolt_physics.html) and [Jolt limit settings](https://docs.godotengine.org/en/4.5/classes/class_projectsettings.html).
- **Evidence:** Verified against the official documentation.
- **Trap:** Higher limits allocate more memory and do not fix a bad pair matrix. Going past the documented limits can drop collisions or give undefined behavior.
