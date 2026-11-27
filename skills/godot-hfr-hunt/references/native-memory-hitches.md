# Native code, memory and hitches

Use this page when the hot spot is spikes at 200 to 300 fps, or a measured
script loop is a candidate for native code. 1 10 ms frame damages the 1% low
even when the average FPS stays high. The rank is the likely cut in the
slowest 1% of frames. The source report was checked against the Godot 4.7
documentation and source, the binding documentation and the installed
`4.7.2.stable.official.ed1daf0bf` engine. No gain was benchmarked in a game,
and the cost of a GDScript to native call depends on the project. Measure it.

## Selection rules

1. Find repeatable slow frames and name their thread and event.
2. Remove synchronous loading, pipeline first use, GC and tree-mutation spikes before you change the language.
3. Improve the algorithm and the data layout before you port code.
4. Port only a measured self-contained loop.
5. Keep the loop native and cross the boundary once with packed buffers.
6. Run the real scenario again. Accept only a p99 or 1% low gain with the same behavior.

Choose C# when the team already ships the .NET build, the loop gains from JIT-compiled managed code, and you can control GC and interop. Choose godot-rust or godot-cpp when the work needs native libraries, tighter memory control or maximum compute throughput, and the deployment cost is justified. Both use GDExtension, and neither makes fine-grained calls free.

## Ranked techniques

### 1. Stage scene integration after threaded resource loading

- **What:** Start `ResourceLoader.load_threaded_request()` early, poll once per frame, and get the resource only after `THREAD_LOAD_LOADED`. Then spread `instantiate()` and `add_child()` work across later frames or a loading screen.
- **Why/thread:** Disk parsing and supported dependency loading move off the main thread. Scene construction and scene-tree entry stay on the main thread, so staging keeps 1 integration frame under 3.33 to 4.17 ms.
- **Measure:** Record wall-frame p50, p99 and worst, plus `TIME_PROCESS`, node and object counts and the event phase (`request`, `loaded`, `instantiate`, `add_child`, first rendered frame). Profile the integration queue, and count pipeline `Surface` compilations.
- **Version:** Godot 4.7. `use_sub_threads=true` can load faster but competes with the main thread. Benchmark it against the default `false`.
- **Source:** [ResourceLoader](https://docs.godotengine.org/en/4.7/classes/class_resourceloader.html), [background loading](https://docs.godotengine.org/en/4.7/tutorials/io/background_loading.html) and [Node callback order](https://docs.godotengine.org/en/4.7/classes/class_node.html).
- **Evidence:** Verified from the official API documentation. Not benchmarked in a project.
- **Trap:** An early `load_threaded_get()` blocks like a synchronous `load()`. Many dependencies loaded with sub-threads can take enough CPU to lower the foreground FPS.

### 2. Drive pipeline precompilation from evidence

- **What:** Watch `RENDERING_INFO_PIPELINE_COMPILATIONS_CANVAS`, `MESH`, `SURFACE` and `DRAW`. Load meshes before play, instantiate each dynamic 3D effect once while it is hidden, and expose rendering features before bulk asset loading.
- **Why/thread:** Render and main thread. Mesh pipeline work can happen during loading, and specialization pipelines compile in the background. No `Surface` or `Draw` creation during play means no first-use spikes.
- **Measure:** Sample the 4 cumulative pipeline counters every frame next to the wall time. A slow frame with a counter delta names its source. Test with a cold pipeline cache, and report frames over the 4.17 ms and 3.33 ms budgets.
- **Version:** Automatic ubershader and pipeline precompilation applies to Forward+ and Mobile from 4.4 to 4.7. Compatibility still needs a first visible draw. Shader baking, since 4.5, cuts source compilation and startup but cannot bake the hardware-specific final pipelines.
- **Source:** [Pipeline compilations](https://docs.godotengine.org/en/4.7/tutorials/performance/pipeline_compilations.html) and [RenderingServer pipeline monitors](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html).
- **Evidence:** Verified from the official 4.x documentation and the 4.7 class API.
- **Trap:** A giant hidden warm-up scene adds startup time and memory. A run-time change to MSAA, GI, the shadow mode or a renderer feature can still create new pipelines.

### 3. Batch GDScript-to-native work into coarse calls

- **What:** Replace per-element calls such as `native.step_one(id)` with 1 typed call such as `native.step_all(positions, velocities, delta)` that runs the whole loop in Rust or C++.
- **Why/thread:** Calling thread. Each script-to-native crossing does method dispatch and argument and return conversion. A coarse call pays that fixed cost once and keeps the loop, its branches and its memory traversal in native code.
- **Measure:** In a release export, time warmed loops with `Time.get_ticks_usec()`: pure GDScript, N tiny native calls and 1 batched native call. Use at least 100,000 operations per round, keep a checksum, and report the median nanoseconds per crossing and the total p99 frame time.
- **Version:** Godot 4.7 GDExtension. This applies to godot-rust `gdext` and to godot-cpp.
- **Source:** [`GDExtensionMethodBind::validated_call`](https://github.com/godotengine/godot/blob/4.7/core/extension/gdextension.cpp#L2351-L2414) converts arguments before `ptrcall`, and the engine notes that `ptrcall` is faster than a regular `call`. The [C# performance guide](https://docs.godotengine.org/en/4.7/tutorials/scripting/c_sharp/c_sharp_basics.html#performance-of-c-in-godot) gives the same interop rule.
- **Evidence:** Verified from the engine source. The exact cost needs a benchmark on the target hardware.
- **Trap:** 1 huge call can become a frame spike that cannot be interrupted. Chunk it, or run pure-data work on a worker when its worst case is above the budget.

### 4. Move only self-contained, measured compute loops to GDExtension or C#

- **What:** First improve the algorithm and type the GDScript. Move a loop when its script `Self` time stays material and most of it is arithmetic and data access, not calls back into Godot. C# costs less to integrate. A Rust or C++ GDExtension gives the most control and a stable, coarse API.
- **Why/thread:** Same CPU thread. Compiled code speeds up project calculations. It does not speed up physics, rendering, navigation or other engine calls, which already run in C++.
- **Measure:** Profiler `Self` time names the candidate. A/B the same input in a release export with equal outputs and call counts. The native version must improve the total p99 frame time, not only a microbenchmark.
- **Version:** Godot 4.7. Build godot-cpp with `api_version=4.7`, and choose a godot-rust release that supports the target engine API.
- **Source:** [CPU optimization: languages](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#languages), [language performance FAQ](https://docs.godotengine.org/en/4.7/about/faq.html#which-programming-language-is-fastest), [godot-cpp compatibility](https://github.com/godotengine/godot-cpp#compatibility) and the [godot-rust book](https://godot-rust.github.io/book/).
- **Evidence:** Verified as official architecture guidance. The gain for a candidate is not verified without a project benchmark.
- **Trap:** Native build, packaging, debugging, platform support and API maintenance can cost more than the saved time. Native code that calls Nodes again and again can be slower than a compact GDScript algorithm.

### 5. Keep GDExtension signatures typed so calls use `ptrcall`

- **What:** Bind fixed-arity methods with concrete parameter and return types. In hot paths, avoid varargs, dynamic `Object.call()`, reflective property access and method lookup by string.
- **Why/thread:** Calling thread. With `ptrcall`, Godot passes opaque typed pointers. A regular call marshals an array of `Variant` and returns a `Variant`. This lowers CPU cost, but the boundary stays.
- **Measure:** Benchmark the typed method against a dynamic wrapper with the same native work. Record the call count, the median total duration and the p99 frame time in a release build.
- **Version:** Godot 4.7 GDExtension. Binding support depends on the godot-rust or godot-cpp version.
- **Source:** [`gdextension.cpp` call and ptrcall](https://github.com/godotengine/godot/blob/4.7/core/extension/gdextension.cpp#L2327-L2414) and the [GDExtension system](https://docs.godotengine.org/en/4.7/engine_details/engine_api/gdextension/index.html).
- **Evidence:** Verified from the engine source.
- **Trap:** Many narrow bindings in place of a truly polymorphic API add complexity. Batch first. Signature tuning comes second.

### 6. Exchange packed buffers, not object graphs

- **What:** Pass `PackedFloat32Array`, `PackedVector3Array` or other packed arrays to native code. In godot-rust, work through `as_slice()` and `as_mut_slice()`. In C++, use packed-array read and write access. Return 1 output buffer, not thousands of Objects or Dictionaries.
- **Why/thread:** Calling or worker thread. Packed arrays store elements of 1 type contiguously, use less memory, miss the cache less and need no `Variant` or object conversion per element.
- **Measure:** Compare memory, loop time and boundary time for `Array[Vector3]` against `PackedVector3Array`. Use `MEMORY_STATIC`, the process RSS and a warmed release microbenchmark, and check equal checksums.
- **Version:** Godot 4.7. godot-rust packed arrays use copy-on-write and expose slices.
- **Source:** [PackedVector3Array](https://docs.godotengine.org/en/4.7/classes/class_packedvector3array.html) and [godot-rust packed arrays](https://godot-rust.github.io/book/godot-api/builtins.html#packed-arrays).
- **Evidence:** Verified from the official Godot and binding documentation.
- **Trap:** A write to a shared copy triggers copy-on-write and can copy the whole buffer. A packed property getter can return a copy that you must assign back. A resize inside a hot loop reallocates.

### 7. Use struct-of-arrays layouts for wide, uniform hot loops

- **What:** Keep positions, velocities, flags and timers in separate pre-sized packed arrays when systems scan 1 or a few fields across many entities.
- **Why/thread:** Calling thread. Linear field access uses the CPU cache better and gives native code SIMD and vectorization chances. It also makes 1 batched boundary call practical.
- **Measure:** Use the same entity count and algorithm for node or object, array-of-struct and struct-of-array variants. Report bytes per entity, loop time and total p99. On macOS, the Instruments Time Profiler finds the loop without hardware counters.
- **Version:** Godot 4.7. The layout does not depend on the language.
- **Source:** [CPU cache guidance](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html) and [PackedVector3Array](https://docs.godotengine.org/en/4.7/classes/class_packedvector3array.html).
- **Evidence:** Verified as engine guidance. The layout gain depends on the workload.
- **Trap:** A struct-of-arrays layout makes per-entity changes and ownership harder. Sparse removals can need swaps and ID remapping. Do not use it on small or irregular systems.

### 8. Remove C# allocations from per-frame and burst paths

- **What:** Reuse `List<T>` and arrays, pre-size capacity, and avoid LINQ, iterator closures and string interpolation in `_Process`. Cache delegates and `StringName` values, and pool only managed objects that you proved churn. Use value types for small records.
- **Why/thread:** Main thread. A lower managed allocation rate means fewer Gen 0 collections and fewer rare long GC pauses that stop managed gameplay threads. This protects the frame tail.
- **Measure:** Use `GC.GetAllocatedBytesForCurrentThread()` around the system, the `dotnet-counters` or profiler allocation rate, Gen 0, 1 and 2 counts and total GC pause time. Correlate those timestamps with the frame log.
- **Version:** Godot 4.7 .NET edition on its supported .NET runtime.
- **Source:** [Language performance FAQ](https://docs.godotengine.org/en/4.7/about/faq.html#which-programming-language-is-fastest), [GC allocation API](https://learn.microsoft.com/en-us/dotnet/api/system.gc.getallocatedbytesforcurrentthread) and [.NET runtime counters](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/available-counters).
- **Evidence:** Verified from the Godot and .NET documentation.
- **Trap:** Oversized kept buffers or pools trade pauses for permanent memory and longer full-GC scans. Do not call `GC.Collect()` during gameplay as a routine fix.

### 9. Batch C# native-property reads and writes

- **What:** Read a Godot property once into a local value, change it in managed code and assign it once. Cache the generated `MethodName`, `PropertyName` and `SignalName` constants. Do not pass strings and raw arrays across interop again and again.
- **Why/thread:** Main thread. Most `GodotObject` properties call native C++. Each access adds interop and marshalling, and a write can trigger engine work.
- **Measure:** Count and time interop calls with a small A/B method, then check the change in script `Self` time and p99. Use `GC.GetAllocatedBytesForCurrentThread()` to catch hidden string and array conversions.
- **Version:** Godot 4.7 .NET.
- **Source:** [Performance of C# in Godot](https://docs.godotengine.org/en/4.7/tutorials/scripting/c_sharp/c_sharp_basics.html#performance-of-c-in-godot) and [C# collections](https://docs.godotengine.org/en/4.7/tutorials/scripting/c_sharp/c_sharp_collections.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** A local copy goes stale if native state changes during callbacks. Keep the read, change and write interval short, and keep the required notifications.

### 10. Pre-size and reuse hot buffers

- **What:** Call `resize()` or set the capacity before you fill a known-size array. Reuse a ring or double buffer instead of append, clear and grow every frame. For native output, let the caller give the destination buffer when ownership stays clear.
- **Why/thread:** Main or worker thread. This removes allocator work and copies from bursts, and stable buffers make the frame cost predictable.
- **Measure:** Track allocated bytes and calls, and microbenchmark the complete fill, not only element access. Report p99 at the maximum gameplay count.
- **Version:** Godot 4.7. This applies to packed arrays, C# arrays and lists, Rust `Vec` and C++ containers.
- **Source:** [`PackedVector3Array.resize()`](https://docs.godotengine.org/en/4.7/classes/class_packedvector3array.html#class-packedvector3array-method-resize) and [CPU cache guidance](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** A cleared logical length can still keep references in managed or `Variant` containers. A worst-case preallocation can waste much RAM.

### 11. Time-slice scene-tree insertion and removal storms

- **What:** Queue node additions and removals, and admit a measured count or microsecond budget per frame. Add complete subtrees at controlled points, not thousands of single mutations mixed with gameplay.
- **Why/thread:** Main thread. `add_child()` recursively triggers tree entry, ready on first entry, notifications, groups and server registration. Removal reverses much of this. Slicing caps the cost of a slow frame.
- **Measure:** Put timestamps around `instantiate`, `add_child`, the first `_ready`, `remove_child` and queued deletion. Log the node count and pipeline `Surface` deltas. Sweep the batch size until p99 stays under the budget.
- **Version:** Godot 4.7.
- **Source:** [Node callback order](https://docs.godotengine.org/en/4.7/classes/class_node.html), [CPU optimization: SceneTree](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#scenetree) and [pipeline precompilation monitors](https://docs.godotengine.org/en/4.7/tutorials/performance/pipeline_compilations.html#pipeline-precompilation-monitors).
- **Evidence:** Verified from the official documentation.
- **Trap:** Deferred availability changes the gameplay order. A partial subtree can see missing siblings, and deferred calls can all run in the same idle cycle, not spread across frames.

### 12. Prefer PackedScene deserialization over scripted hierarchy construction

- **What:** Author stable node hierarchies as scenes and instantiate the `PackedScene`. Do not build them with long sequences of `Node.new()`, property assignment, script attachment and `add_child()` from script.
- **Why/thread:** Main thread. The engine deserializes scene data in a batch. Script construction makes many scripting-API lookups and boundary calls.
- **Measure:** Compare the same hierarchy from a packed scene and from a script factory. Time load, instantiate, add to tree, first ready and peak memory apart.
- **Version:** Godot 4.7.
- **Source:** [Performance of Script vs PackedScene](https://docs.godotengine.org/en/4.7/tutorials/best_practices/scenes_versus_scripts.html#performance-of-script-vs-packedscene).
- **Evidence:** Verified from the official documentation.
- **Trap:** A monolithic scene can make loading and editing worse. Dynamic structures with little repeated setup can gain nothing.

### 13. Pool only expensive, measured churn, and choose where inactive nodes live

- **What:** Pool short-lived scenes only after an event probe shows `instantiate`, tree-entry or free spikes. Reset all gameplay, signal, animation, particle, physics and ownership state. For large inactive sets, compare disabled nodes in the tree with kept nodes detached by `remove_child()`.
- **Why/thread:** Main thread. Reuse avoids allocator, construction and first-entry costs. Detaching can also remove SceneTree housekeeping that hiding alone keeps.
- **Measure:** A/B spawn and despawn bursts. Record acquire, reset and release time, node, object and active-body counts, static memory, p99 and the pool miss and growth count. Test a full level reload for stale state.
- **Version:** Godot 4.7.
- **Source:** [CPU optimization: SceneTree](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#scenetree) and [`Node.request_ready`](https://docs.godotengine.org/en/4.7/classes/class_node.html#class-node-method-request-ready).
- **Evidence:** Verified from the official documentation. The decision threshold depends on the project.
- **Trap:** A pool can keep thousands of nodes and server objects alive, which adds steady traversal and memory. Reparenting still sends tree notifications, so pooling does not make `add_child()` free.

### 14. Bypass Nodes for very large homogeneous populations

- **What:** Use `MultiMesh` or direct `RenderingServer` and `PhysicsServer3D` RIDs when entities need no independent Node behavior. Keep game state in arrays and submit bulk transforms and buffers.
- **Why/thread:** Main thread. This removes per-node SceneTree memory, callbacks, notifications and one-by-one server submissions. Rendering stays on the render thread and GPU. Preparation usually starts on the main thread unless the API is explicitly thread-safe.
- **Measure:** Compare node count, static memory, process time, render CPU, draw calls and p99 at the same visible count. Include the cost of the write of the whole transform buffer.
- **Version:** Godot 4.7.
- **Source:** [Optimization using Servers](https://docs.godotengine.org/en/4.7/tutorials/performance/using_servers.html), [CPU optimization: SceneTree](https://docs.godotengine.org/en/4.7/tutorials/performance/cpu_optimization.html#scenetree) and [MultiMesh](https://docs.godotengine.org/en/4.7/classes/class_multimesh.html).
- **Evidence:** Verified from the official documentation.
- **Trap:** RIDs need manual lifetime management. A MultiMesh is culled as 1 unit and has no per-instance scripts or collision. A rebuild of a huge buffer every frame can become the new spike.

### 15. Make the resource-cache policy explicit for streaming

- **What:** Use the default `CACHE_MODE_REUSE` for assets that the player visits again, release strong references when an area leaves, and use the `IGNORE` or `REPLACE` modes only when the content workflow needs fresh data. Keep an explicit bounded residency set for streamed regions.
- **Why/thread:** Loader, main and render threads. Reuse avoids repeated parsing and GPU and resource setup. Bounded residency stops memory growth and later OS pressure. Loading runs on loader threads, and the final resource use can touch the main and render threads.
- **Measure:** Log the request cache mode, the load status duration, static, texture and buffer memory, the resident asset count and the hitch frames on a return visit. Run a repeated area churn test.
- **Version:** Godot 4.7. The deep cache modes exist in current 4.x APIs.
- **Source:** [ResourceLoader cache modes](https://docs.godotengine.org/en/4.7/classes/class_resourceloader.html#enum-resourceloader-cachemode) and [ResourceLoader](https://docs.godotengine.org/en/4.7/classes/class_resourceloader.html).
- **Evidence:** Verified from the official API documentation.
- **Trap:** `REPLACE` can change cached instances that other code uses. An unbounded "preload everything" trades hitches for too much RAM, VRAM and startup time.

### 16. Correlate 1% lows with discrete engine events

- **What:** Keep a frame-event ring buffer with resource transitions, instantiate, add and remove counts, managed allocation and GC deltas, and pipeline-counter deltas. Dump the nearby frames when the wall time is over the 240 or 300 fps budget.
- **Why/thread:** All threads. Average FPS cannot name rare main, render, GPU, loader or GC stalls. Event correlation turns a slow frame into a cause you can act on.
- **Measure:** Report median, p95, p99, 1% low FPS from the p99 frame time, worst frame and budget-miss count. Assign each top spike to the main, render, physics or GPU split and the nearby event. List spikes without a cause as unassigned.
- **Version:** Godot 4.7. C# counters need the .NET build.
- **Source:** [Performance monitors](https://docs.godotengine.org/en/4.7/classes/class_performance.html), [RenderingServer monitors](https://docs.godotengine.org/en/4.7/classes/class_renderingserver.html) and [.NET runtime counters](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/available-counters).
- **Evidence:** Verified for monitor availability. Attribution still needs a project capture.
- **Trap:** Logging or sorting every frame can create the hitch you measure. Store numbers in a preallocated ring, and format and write them after the capture.
