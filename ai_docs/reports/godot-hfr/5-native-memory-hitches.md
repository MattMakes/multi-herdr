# Native code, memory, and hitches at 200–300 FPS

## Scope and evidence

This report targets Godot 4.7. The local engine is `4.7.2.stable.official.ed1daf0bf`. At 240 FPS the whole frame is 4.17 ms; at 300 FPS it is 3.33 ms. A 10 ms frame can therefore damage the 1% low even when average FPS remains high.

The ranking is by likely reduction in the slowest 1% of frames, not by average throughput. “Verified” below means the behavior was checked in Godot 4.7 documentation, engine source, binding documentation, or the installed engine version. It does not mean a gain was benchmarked in an unknown game. No universal GDScript/native crossing cost is reported: it depends on the binding, signature, argument types, build, CPU, and safeguards. Measure it in the target project.

Existing fleet guidance already covers percentile capture, generic pooling, generic shader warm-up, packed arrays, and basic threaded loading. The techniques below give missing decisions, 4.7-specific behavior, or corrections.

## Corrections to existing guidance

- `skills/godot-optimization/references/cpu-bottlenecks.md:13` incorrectly groups GDScript `Vector2`/`Vector3` construction with garbage-collected heap objects. Godot vectors are value types, and GDScript does not use the .NET garbage collector. Temporary vectors still execute work, but they are not evidence of a GC pause. Arrays, dictionaries, strings, and managed C# objects have different ownership and allocation behavior and must not be described as one mechanism.
- `skills/godot-optimization/references/memory-management.md:136` says every instantiate/free cycle reruns `_ready()`. `_ready()` runs when a node and its children first enter the tree; removing and re-adding the same node does not request another `_ready()` automatically. `Node.request_ready()` is required if another ready callback is wanted. Pooling can also make performance worse by keeping nodes, callbacks, collision objects, and resources alive. Measure the complete acquire/reset/release path.
- `skills/godot-perf-measure/references/shader-warm-up.md:6-24` is too broad for Godot 4.7. Forward+ and Mobile have automatic ubershader and pipeline precompilation since 4.4. Use the four pipeline-compilation monitors to identify the missing path. Compatibility still needs first-draw warm-up. In Forward+/Mobile, loading and instantiating hidden dynamic effects can be sufficient; blindly drawing every variant adds load time and memory.
- `skills/godot-multithreading/SKILL.md:192-210` correctly warns that `load_threaded_get()` can block, but the example immediately instantiates and adds the scene after retrieval. Loading is only one part of a hitch. `PackedScene.instantiate()`, `_enter_tree`, child propagation, `_ready`, server registration, and pipeline surface preparation still run when the scene joins the tree. Stage or time-slice that work too.

## Ranked techniques

### 1. Stage scene integration after threaded resource loading

- **What:** Start `ResourceLoader.load_threaded_request()` early, poll once per frame, retrieve only after `THREAD_LOAD_LOADED`, then spread `instantiate()` and `add_child()` work across later frames or a loading screen.
- **Why/thread:** Disk parsing and supported dependency loading move off the main thread. Scene construction and scene-tree entry remain on the main thread, so staging prevents a single integration frame from exceeding 3.33–4.17 ms.
- **Measure:** Record wall-frame p50/p99/worst plus `TIME_PROCESS`, node/object counts, and the event phase (`request`, `loaded`, `instantiate`, `add_child`, first rendered frame). Use the script profiler around the integration queue. Count pipeline `Surface` compilations too.
- **Version:** Godot 4.7. `use_sub_threads=true` may load faster but can compete with the main thread; benchmark it against the default `false`.
- **Source:** [ResourceLoader 4.7 API](https://docs.godotengine.org/en/4.7/classes/class_resourceloader.html), [background loading](https://docs.godotengine.org/en/4.7/tutorials/io/background_loading.html), [Node callback order](https://docs.godotengine.org/en/4.7/classes/class_node.html).
- **Evidence:** **Verified** from official API documentation. Not project-benchmarked.
- **Trap:** Calling `load_threaded_get()` early blocks like synchronous `load()`. Loading many dependencies with subthreads can steal enough CPU to lower foreground FPS.

### 2. Drive Godot 4.7 pipeline precompilation from evidence

- **What:** Watch `RENDERING_INFO_PIPELINE_COMPILATIONS_CANVAS`, `MESH`, `SURFACE`, and `DRAW`; load meshes before play, instantiate dynamic 3D effects once while hidden, and expose rendering features before bulk asset load.
- **Why/thread:** Mesh pipeline work can occur during loading, and specialization pipelines compile in the background. Preventing `Surface`/`Draw` creation during play removes render/main-thread first-use spikes.
- **Measure:** Sample the four cumulative pipeline counters every frame beside wall time. A slow frame with a counter delta identifies its source. Test with a cold pipeline cache and report frames over the 4.17 ms and 3.33 ms budgets.
- **Version:** Automatic ubershader/pipeline precompilation applies to Forward+ and Mobile from 4.4 through 4.7. Compatibility still needs first visible draw. Shader baking, available since 4.5, reduces source compilation/startup but cannot bake hardware-specific final pipelines.
- **Source:** [pipeline compilation guide](https://docs.godotengine.org/en/4.7/tutorials/performance/pipeline_compilations.html), [RenderingServer pipeline monitors](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html).
- **Evidence:** **Verified** from official 4.x documentation and 4.7 class APIs.
- **Trap:** A giant hidden warm-up scene raises startup time and memory. Runtime MSAA, GI, shadow-mode, or renderer feature changes can still create new pipelines.

### 3. Batch GDScript-to-native work into coarse calls

- **What:** Replace per-element GDScript calls such as `native.step_one(id)` with one typed call such as `native.step_all(positions, velocities, delta)` that performs the whole loop in Rust/C++.
- **Why/thread:** Each script/native crossing performs method dispatch and argument/return conversion. A coarse call pays that fixed cost once and keeps the hot loop, branches, and memory traversal in native code on the calling thread.
- **Measure:** In a release export, time warmed loops with `Time.get_ticks_usec()`: pure GDScript, N tiny native calls, and one batched native call. Use at least 100,000 operations per round, retain a checksum, and report median nanoseconds per crossing plus total frame p99.
- **Version:** Godot 4.7 GDExtension. Applies to both godot-rust `gdext` and `godot-cpp`.
- **Source:** Godot’s [`GDExtensionMethodBind::validated_call`](https://github.com/godotengine/godot/blob/4.7/core/extension/gdextension.cpp#L2351-L2414) converts arguments before `ptrcall`; the engine explicitly notes that `ptrcall` is faster than regular `call`. Godot’s [C# performance guide](https://docs.godotengine.org/en/4.7/tutorials/scripting/c_sharp/c_sharp_basics.html#performance-of-c-in-godot) documents the analogous interop rule.
- **Evidence:** **Verified** from engine source. Exact cost is intentionally left for a target-hardware benchmark.
- **Trap:** One huge call can become an uninterruptible frame spike. Chunk it or run pure-data work on a worker when its worst-case duration exceeds the budget.

### 4. Move only self-contained, measured compute loops to GDExtension or C#

- **What:** First improve the algorithm and type GDScript. Move a loop when its script self-time remains material and most of the loop is arithmetic/data access rather than calls back into Godot. Prefer C# for lower integration cost; prefer Rust/C++ GDExtension for maximum control and a stable, coarse API.
- **Why/thread:** Compiled code accelerates project calculations on the same CPU thread. It does not accelerate physics, rendering, navigation, or other engine calls that already execute in C++.
- **Measure:** Profiler `Self` time identifies the candidate. A/B the same input in a release export, with equal outputs and call counts. The native version must improve total frame p99, not only a microbenchmark.
- **Version:** Godot 4.7. Build `godot-cpp` with `api_version=4.7`; select a godot-rust release that supports the target engine API.
- **Source:** [CPU optimization language guidance](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#languages), [language performance FAQ](https://docs.godotengine.org/en/4.7/about/faq.html#which-programming-language-is-fastest), [godot-cpp compatibility](https://github.com/godotengine/godot-cpp#compatibility), [godot-rust book](https://godot-rust.github.io/book/).
- **Evidence:** **Verified** as official architecture guidance; candidate gains are not verified without a project benchmark.
- **Trap:** Native build, packaging, debugging, platform support, and ABI/API maintenance can cost more than the saved time. Native code that repeatedly calls Nodes can be slower than a compact GDScript algorithm.

### 5. Keep GDExtension signatures typed so calls use `ptrcall`

- **What:** Bind fixed-arity methods with concrete parameter and return types. Avoid hot-path varargs, dynamic `Object.call()`, reflective property access, and string method lookup.
- **Why/thread:** Godot can pass opaque typed pointers through `ptrcall`; regular calls marshal arrays of `Variant` and return a `Variant`. This lowers CPU cost on the caller’s thread but does not remove the boundary.
- **Measure:** Benchmark the typed method against a deliberately dynamic wrapper with identical native work. Record call count, median total duration, and frame p99 in a release build.
- **Version:** Godot 4.7 GDExtension; binding support depends on the godot-rust/godot-cpp version.
- **Source:** [`gdextension.cpp` call and ptrcall implementations](https://github.com/godotengine/godot/blob/4.7/core/extension/gdextension.cpp#L2327-L2414), [GDExtension system](https://docs.godotengine.org/en/4.7/engine_details/engine_api/gdextension/index.html).
- **Evidence:** **Verified** from engine source.
- **Trap:** Replacing a genuinely polymorphic API with many narrow bindings can increase complexity. Batch first; signature tuning is secondary.

### 6. Exchange packed buffers, not object graphs

- **What:** Pass `PackedFloat32Array`, `PackedVector3Array`, or other packed arrays to native code. In godot-rust, operate through `as_slice()`/`as_mut_slice()`; in C++, use packed-array read/write access. Return one output buffer instead of thousands of Objects or Dictionaries.
- **Why/thread:** Packed arrays store homogeneous elements contiguously, reduce memory and cache misses, and avoid one Variant/object conversion per element. Native work stays data-oriented on the caller or worker thread.
- **Measure:** Compare memory, loop duration, and boundary duration for `Array[Vector3]` versus `PackedVector3Array`. Use `MEMORY_STATIC`, process RSS, and a warmed release microbenchmark; verify equal checksums.
- **Version:** Godot 4.7. godot-rust packed arrays use copy-on-write and expose slices.
- **Source:** [PackedVector3Array 4.7](https://docs.godotengine.org/en/4.7/classes/class_packedvector3array.html), [godot-rust packed arrays](https://godot-rust.github.io/book/godot-api/builtins.html#packed-arrays).
- **Evidence:** **Verified** from official Godot and binding documentation.
- **Trap:** Mutating a shared copy triggers copy-on-write and can copy the entire buffer. A packed property getter may return a copy that must be assigned back. Resizing during a hot loop reallocates.

### 7. Use struct-of-arrays layouts for wide, uniform hot loops

- **What:** Store positions, velocities, flags, and timers in separate pre-sized packed arrays when systems scan one or a few fields across many entities.
- **Why/thread:** Linear field access improves CPU cache use and SIMD/vectorization opportunities in native code. It also makes a single batched boundary call practical.
- **Measure:** Use the same entity count and algorithm for node/object, array-of-struct, and struct-of-array variants. Report bytes/entity, cache-sensitive loop time, and total p99. On macOS, Instruments’ Time Profiler can identify the loop even without hardware counters.
- **Version:** Godot 4.7; language-independent.
- **Source:** [Godot CPU cache guidance](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html), [PackedVector3Array 4.7](https://docs.godotengine.org/en/4.7/classes/class_packedvector3array.html).
- **Evidence:** **Verified** as engine guidance; layout gain is workload-dependent.
- **Trap:** SoA makes per-entity mutation and ownership harder. Sparse removals can require swaps and ID remapping; do not impose it on small or irregular systems.

### 8. Remove C# allocations from per-frame and burst paths

- **What:** Reuse `List<T>` and arrays, pre-size capacity, avoid LINQ/iterator closures/string interpolation in `_Process`, cache delegates and `StringName`s, and pool only managed objects proven to churn. Prefer value types for small records.
- **Why/thread:** Lower managed allocation rate reduces Gen 0 frequency and rare longer GC pauses that stop managed gameplay threads. This primarily protects main-thread frame tails.
- **Measure:** Use `GC.GetAllocatedBytesForCurrentThread()` around the system, `dotnet-counters`/profiler allocation rate, Gen 0/1/2 counts, and total GC pause time. Correlate those timestamps with the frame log.
- **Version:** Godot 4.7 .NET edition, using its supported .NET runtime.
- **Source:** [Godot language FAQ](https://docs.godotengine.org/en/4.7/about/faq.html#which-programming-language-is-fastest), [GC allocation API](https://learn.microsoft.com/en-us/dotnet/api/system.gc.getallocatedbytesforcurrentthread), [.NET runtime counters](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/available-counters).
- **Evidence:** **Verified** from Godot and .NET documentation.
- **Trap:** Retaining oversized buffers or pools trades pauses for permanent memory and longer full-GC scans. Do not call `GC.Collect()` during gameplay as a routine fix.

### 9. Batch C# native-property reads and writes

- **What:** Read a Godot property once into a local value, update it in managed code, and assign it once. Cache generated `MethodName`, `PropertyName`, and `SignalName` constants. Avoid passing strings and raw arrays across interop repeatedly.
- **Why/thread:** Most `GodotObject` properties call native C++. Repeated property access adds interop/marshalling and may trigger engine-side work for every write, all on the main thread.
- **Measure:** Count and time interop calls with a small A/B method, then verify the change in script self-time and p99. Use `GC.GetAllocatedBytesForCurrentThread()` to catch hidden string/array conversions.
- **Version:** Godot 4.7 .NET.
- **Source:** [Performance of C# in Godot](https://docs.godotengine.org/en/4.7/tutorials/scripting/c_sharp/c_sharp_basics.html#performance-of-c-in-godot), [C# collections](https://docs.godotengine.org/en/4.7/tutorials/scripting/c_sharp/c_sharp_collections.html).
- **Evidence:** **Verified** from official documentation.
- **Trap:** Local caching can become stale if native state changes during callbacks. Keep the read/modify/write interval narrow and preserve required notifications.

### 10. Pre-size and reuse hot buffers

- **What:** Call `resize()` or set capacity before filling known-size arrays. Reuse a ring/double buffer instead of append/clear/grow every frame. For native output, let the caller provide the destination buffer when ownership remains clear.
- **Why/thread:** This removes allocator work and copies from main- or worker-thread bursts. Stable buffers also make frame cost predictable.
- **Measure:** Track allocation bytes/calls and microbenchmark the complete fill operation, not only element access. Report p99 with the maximum gameplay count.
- **Version:** Godot 4.7; applies to packed arrays, C# arrays/lists, Rust `Vec`, and C++ containers.
- **Source:** [`PackedVector3Array.resize()` 4.7](https://docs.godotengine.org/en/4.7/classes/class_packedvector3array.html#class-packedvector3array-method-resize), [Godot CPU cache guidance](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html).
- **Evidence:** **Verified** from official documentation.
- **Trap:** Clearing logical length may still retain references in managed or Variant containers. A worst-case preallocation can waste large amounts of RAM.

### 11. Time-slice scene-tree insertion and removal storms

- **What:** Queue node additions/removals and admit a measured number or microsecond budget per frame. Add complete subtrees at controlled points instead of interleaving thousands of individual mutations with gameplay.
- **Why/thread:** `add_child()` recursively triggers tree entry, readiness on first entry, notifications, groups, and server registration. Removal reverses much of this on the main thread. Slicing caps the slow-frame cost.
- **Measure:** Place timestamps around `instantiate`, `add_child`, first `_ready`, `remove_child`, and queued deletion. Log node count and pipeline `Surface` deltas. Sweep batch size until p99 stays below budget.
- **Version:** Godot 4.7.
- **Source:** [Node 4.7 callback order](https://docs.godotengine.org/en/4.7/classes/class_node.html), [CPU optimization SceneTree section](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#scenetree), [pipeline surface compilation](https://docs.godotengine.org/en/4.7/tutorials/performance/pipeline_compilations.html#pipeline-precompilation-monitors).
- **Evidence:** **Verified** from official documentation.
- **Trap:** Deferred availability changes gameplay ordering. A partial subtree can observe missing siblings, and deferred calls can all execute in the same idle cycle rather than automatically spreading across frames.

### 12. Prefer PackedScene deserialization over scripted hierarchy construction

- **What:** Author stable node hierarchies as scenes and instantiate the `PackedScene` instead of issuing long sequences of `Node.new()`, property assignment, script attachment, and `add_child()` from script.
- **Why/thread:** The engine can deserialize scene data in a batch. Script construction makes many scripting-API lookups and boundary calls on the main thread.
- **Measure:** Compare identical hierarchies from a packed scene and script factory. Time load, instantiate, add-to-tree, first ready, and peak memory separately.
- **Version:** Godot 4.7.
- **Source:** [Performance of Script vs PackedScene](https://docs.godotengine.org/en/4.7/tutorials/best_practices/scenes_versus_scripts.html#performance-of-script-vs-packedscene).
- **Evidence:** **Verified** from official documentation.
- **Trap:** A monolithic scene can make loading and editing worse. Dynamic structures with little repeated setup may not benefit.

### 13. Pool only expensive, measured churn—and choose where inactive nodes live

- **What:** Pool short-lived scenes only after an event probe shows `instantiate`/tree-entry/free spikes. Reset all gameplay, signal, animation, particle, physics, and ownership state. For large inactive sets, compare disabled-in-tree nodes with retained nodes detached by `remove_child()`.
- **Why/thread:** Reuse avoids allocator, construction, and first-entry costs on the main thread. Detaching can also remove SceneTree housekeeping that hiding alone does not.
- **Measure:** A/B spawn/despawn bursts. Record acquire/reset/release time, node/object/active-body counts, static memory, p99, and pool miss/growth count. Test a full level reload for stale state.
- **Version:** Godot 4.7.
- **Source:** [CPU optimization SceneTree and reuse guidance](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#scenetree), [Node.request_ready](https://docs.godotengine.org/en/4.7/classes/class_node.html#class-node-method-request-ready).
- **Evidence:** **Verified** from official docs; the decision threshold is project-specific.
- **Trap:** A pool can keep thousands of nodes and server objects alive, increasing steady traversal and memory. Reparenting still incurs tree notifications; pooling does not make `add_child()` free.

### 14. Bypass Nodes for very large homogeneous populations

- **What:** Use `MultiMesh` or direct RenderingServer/PhysicsServer RIDs when entities do not need independent Node behavior. Keep gameplay state in arrays and submit bulk transforms/buffers.
- **Why/thread:** This removes per-node SceneTree memory, callbacks, notifications, and one-by-one server submissions. Rendering remains on render/GPU threads; preparation usually starts on the main thread unless the API is explicitly thread-safe.
- **Measure:** Compare node count, static memory, process time, render CPU, draw calls, and p99 at the same visible count. Include the cost of writing the whole transform buffer.
- **Version:** Godot 4.7.
- **Source:** [Optimization using Servers](https://docs.godotengine.org/en/4.7/tutorials/performance/using_servers.html), [CPU optimization SceneTree](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#scenetree), [MultiMesh](https://docs.godotengine.org/en/4.7/classes/class_multimesh.html).
- **Evidence:** **Verified** from official documentation.
- **Trap:** RIDs require manual lifetime management; MultiMesh culls as a unit and lacks per-instance scripts/collision. Rebuilding a huge buffer each frame can become the new spike.

### 15. Make resource-cache policy explicit for streaming

- **What:** Use the default `CACHE_MODE_REUSE` for repeatedly visited assets, release strong references when an area leaves, and use `IGNORE`/`REPLACE` modes only when the content workflow requires fresh data. Keep an explicit bounded residency set for streamed regions.
- **Why/thread:** Reuse avoids repeated parsing and GPU/resource setup; bounded residency prevents memory growth and later OS pressure. Loading runs on loader threads, while final resource use can touch main/render threads.
- **Measure:** Log request cache mode, load status duration, static/texture/buffer memory, resident asset count, and return-visit hitch frames. Run a repeated area churn test.
- **Version:** Godot 4.7; deep cache modes exist in current 4.x APIs.
- **Source:** [ResourceLoader cache modes](https://docs.godotengine.org/en/4.7/classes/class_resourceloader.html#enum-resourceloader-cachemode), [ResourceLoader.exists/cache caveat](https://docs.godotengine.org/en/4.7/classes/class_resourceloader.html).
- **Evidence:** **Verified** from official API documentation.
- **Trap:** `REPLACE` can mutate cached instances used elsewhere. Unbounded “preload everything” replaces hitches with excessive RAM/VRAM and startup time.

### 16. Correlate 1% lows with discrete engine events

- **What:** Keep a frame-event ring buffer containing resource transitions, instantiate/add/remove counts, managed allocation/GC deltas, and pipeline-counter deltas. Dump the surrounding frames whenever wall time exceeds the 240/300 FPS budget.
- **Why/thread:** Average FPS cannot identify rare main, render, GPU, loader, or GC stalls. Event correlation turns a slow frame into an actionable cause.
- **Measure:** Report median, p95, p99, 1% low FPS derived from p99 frame time, worst frame, and budget-miss count. Attribute each top spike to the main/render/physics/GPU split and nearby event; leave unattributed spikes explicit.
- **Version:** Godot 4.7; C# counters require the .NET build.
- **Source:** [Performance monitors](https://docs.godotengine.org/en/4.7/classes/class_performance.html), [RenderingServer monitors](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html), [.NET runtime counters](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/available-counters).
- **Evidence:** **Verified** monitor availability. Attribution still requires a project capture.
- **Trap:** Logging or sorting every frame can create the hitch being measured. Store numeric data in a preallocated ring and format/write after capture.

## Selection rules

Use this decision order:

1. Find repeatable slow frames and attribute their thread/event.
2. Remove synchronous loading, pipeline first-use, GC, or tree-mutation spikes before changing language.
3. Improve algorithms and data layout before porting code.
4. Port only a measured self-contained loop.
5. Keep the loop native and cross the boundary once with packed buffers.
6. Re-run the real scenario and accept only a p99/1% low improvement with equal behavior.

Choose C# when the team already ships the .NET build, the loop benefits from JIT-compiled managed code, and GC/interop can be controlled. Choose godot-rust or godot-cpp when the workload needs native libraries, tighter memory control, or maximum compute throughput and the deployment cost is justified. Both Rust `gdext` and `godot-cpp` ultimately use GDExtension; neither makes fine-grained calls free.

## Detection

In a real project, use these checks in rank order:

| Suspected technique | Detection in project | Profiler/monitor signature |
|---|---|---|
| Threaded loading and staged integration | `rg -n '\b(load|ResourceLoader\.load|GD\.Load|LoadThreadedGet|load_threaded_get|instantiate|Instantiate|add_child|AddChild)\b' --glob '*.{gd,cs}'` | One spike at request/get/instantiate/add; process time and node count jump |
| Pipeline precompilation | `rg -n '\.(gdshader|material|tres)|ShaderMaterial|Particle|material_override|MSAA|VoxelGI|SDFGI'` | Pipeline `Surface` or `Draw` counter rises on the hitch frame |
| Fine native calls | `rg -n 'GDExtension|gdextension|godot::|Gd<|ClassDB::bind_method|#\[func\]|\.call\('` | Hot script function has a very high call count and low work per call |
| Native-port candidates | Sort the script profiler by `Self`, then inspect only top arithmetic/data loops | Script self-time is material; engine/GPU time is not the bottleneck |
| Packed buffers / SoA | `rg -n 'Array\[(float|int|Vector)|Godot\.Collections\.Array|Dictionary|Variant|Vec<' --glob '*.{gd,cs,rs,cpp,hpp}'` | Container iteration/conversion dominates; memory rises with entity count |
| C# GC | `rg -n '(_Process|_PhysicsProcess|foreach|Select\(|Where\(|ToList\(|new |\$\")' --glob '*.cs'` | Allocation rate/Gen 0 count or GC pause rises beside p99 spikes |
| C# interop batching | `rg -n 'Position|Transform|GetNode|GD\.Call|CallDeferred|new StringName|new NodePath' --glob '*.cs'` inside loops | Managed method self-time grows with native property call count |
| Buffer reuse | `rg -n '\.(append|push_back|Add|ToArray|ToList|resize|Resize)\(' --glob '*.{gd,cs,rs,cpp,hpp}'` | Periodic capacity growth/copy or allocation bursts |
| Scene-tree churn | `rg -n '(add_child|remove_child|queue_free|AddChild|RemoveChild|QueueFree|instantiate|Instantiate)' --glob '*.{gd,cs}'` | Node/object/active-body jumps; `_ready` or notification-heavy frame |
| Script-built scenes | `rg -n '(Node|Node2D|Node3D|Control)\.new\(|new (Node|Node2D|Node3D|Control)' --glob '*.{gd,cs}'` | Many script/native calls during construction |
| Pooling | `rg -n '(pool|Pool|queue_free|QueueFree|instantiate|Instantiate)' --glob '*.{gd,cs}'` | Repeating spawn/despawn spike; compare with steady pooled node cost |
| Server/MultiMesh path | Count homogeneous nodes/scenes; `rg -n 'MultiMesh|RenderingServer|PhysicsServer'` | Thousands of nodes, high process/render CPU, many draw calls |
| Resource residency | `rg -n 'CACHE_MODE|CacheMode|preload\(|GD\.Load|ResourceLoader' --glob '*.{gd,cs}'` | Return visits reload; memory has an unbounded slope or OS pressure |

Thresholds are evidence triggers, not automatic fixes: investigate any repeated frame above 4.17 ms for 240 FPS or 3.33 ms for 300 FPS, any gameplay-time `Draw` pipeline compilation, any managed allocation/GC event aligned with a slow frame, and any boundary loop making thousands of calls per frame. Validate every selected change with interleaved release A/B runs on target hardware.
