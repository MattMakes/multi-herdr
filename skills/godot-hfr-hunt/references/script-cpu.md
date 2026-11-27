# Script CPU: GDScript and main-thread hot paths

Use this page when the hot spot is script CPU at 200 to 300 fps (4.17 ms at
240 fps, 3.33 ms at 300 fps). The source report was checked against Godot
`4.7.2-stable` (`ed1daf0bf`). Its local headless debug-build microbenchmark
ran 3 trials after warm-up with `godot --headless --path . --max-fps 0`. The
numbers show direction only. A release export of the real scene decides.

| Operation | Work | Median |
| --- | ---: | ---: |
| Typed integer loop | 1,000,000 iterations | 17.978 ms |
| `Variant` integer loop | 1,000,000 iterations | 29.016 ms |
| Cached node reference | 1,000,000 reads | 26.102 ms |
| `get_node(^"A/B/C")` | 1,000,000 lookups | 72.376 ms |
| Direct method call | 1,000,000 calls | 72.269 ms |
| One-listener signal emit | 1,000,000 emits | 156.618 ms |
| Reuse named `Callable` | 1,000,000 assignments | 13.742 ms |
| Create capturing lambda | 1,000,000 creations | 120.109 ms |
| `Array.fill()` | 100,000 elements | 0.198 ms |
| Indexed fill loop | 100,000 elements | 1.822 ms |
| `Array.map(lambda)` | 100,000 elements | 3.925 ms |
| Pre-sized indexed map loop | 100,000 elements | 2.788 ms |

## Ranked techniques

### 1. Remove dormant process callbacks

- **What:** Call `set_process(false)` and `set_physics_process(false)` when a node has no work. Enable it again on the state transition that needs updates.
- **Why/thread:** Main thread. The node leaves the process group, so it does not pay a callback and an early return every frame. SceneTree copies the process-node vector, checks every entry and sends each enabled notification. At 240 fps, 1,000 idle callbacks are 240,000 dispatches per second.
- **Measure:** Debugger > Profiler `Calls` for `_process` and `_physics_process`, plus `Performance.TIME_PROCESS`, `TIME_PHYSICS_PROCESS` and `OBJECT_NODE_COUNT`. Compare the same scene with idle actors enabled and disabled.
- **Version:** Godot 4.7.
- **Source:** [SceneTree process loop](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/scene_tree.cpp#L1200-L1235) and [`set_process`](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/node.cpp#L1025-L1044).
- **Evidence:** Verified from the engine source.
- **Trap:** A disabled node misses state changes that it polls for. Enable it again from an event, a timer, a visibility transition or its owner.

### 2. Batch homogeneous updates behind one callback

- **What:** For hundreds of identical light actors, keep their state in typed or packed arrays and update them from 1 manager callback, not 1 script callback per node.
- **Why/thread:** Main thread. The work stays the same, but the repeated SceneTree notifications and GDScript call boundaries go away. The process loop checks and notifies each node before any game logic runs.
- **Measure:** Profiler `Calls` drop from N callbacks to 1. Compare `TIME_PROCESS`, the manager `Self` and the release-export p50 and p99 frame time. Keep the entity count and the results the same.
- **Version:** Godot 4.7.
- **Source:** [SceneTree process loop](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/scene_tree.cpp#L1203-L1233).
- **Evidence:** Verified from the engine source.
- **Trap:** Do not remove useful node ownership or lifecycle boundaries for tens of objects. The gain matters only when the callback count is large and measured.

### 3. Use explicit update tiers instead of moving work to physics

- **What:** Run visual work every rendered frame, simulation at the physics tick, and delay-tolerant AI, UI and cache work at explicit lower rates, spread across frames.
- **Why/thread:** Main thread. The call count drops without coupling unrelated work to `physics_ticks_per_second`, and all agents do not spike on 1 frame.
- **Measure:** Profiler call counts per tier, `TIME_PROCESS` and `TIME_PHYSICS_PROCESS`. Check p99 and the worst frame, not only averages. At 240 fps, confirm that a 10 Hz task runs 10 times per second, not once every fixed number of rendered frames.
- **Version:** Godot 4.7.
- **Source:** [SceneTree process and physics paths](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/scene_tree.cpp#L1218-L1231).
- **Evidence:** Verified from the engine source.
- **Trap:** A frame-modulo throttle changes its wall-clock rate when the FPS changes. Accumulate `delta`, use a timer, or schedule against absolute time when the rate matters.

### 4. Cache node references after scene wiring

- **What:** Resolve stable dependencies once with `@onready`, an exported node reference or dependency injection. Do not repeat `$Path` or `get_node()` in a hot loop.
- **Why/thread:** Main thread. `$Path` compiles to a node lookup, not a cached field. Each lookup walks each `NodePath` name and does a child hash lookup. A unique-name lookup is still a lookup. In the local test, a 3-segment lookup cost 2.77 times a cached reference.
- **Measure:** Microbenchmark the exact path, then compare the caller's Profiler `Self` and `TIME_PROCESS`. Search frame callbacks for repeated `$` and `get_node` expressions.
- **Version:** Godot 4.7.
- **Source:** [Node path traversal](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/node.cpp#L1904-L1963) and the local 4.7.2 test.
- **Evidence:** Verified from the engine source and the local microbenchmark.
- **Trap:** A cached reference is invalid when the scene replaces or frees that node. Bind it again on the owner's lifecycle event, and use `is_instance_valid()` only when replacement is expected.

### 5. Type the complete hot expression and call chain

- **What:** Type the parameters, locals, members, return values, container variables and native receivers that hot arithmetic or method calls use.
- **Why/thread:** Main thread. The GDScript VM has validated operator, property, method-bind and iteration opcodes. They call preselected functions instead of generic `Variant` dispatch. The local `Variant` integer loop cost 1.61 times the typed loop.
- **Measure:** Time equivalent typed and untyped functions with `Time.get_ticks_usec()` over many calls, then check the real caller in the Profiler. A warning that turns a value back into `Variant` is a broken typed chain.
- **Version:** Godot 4.7.
- **Source:** [Validated operator](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L848-L863), [validated native call](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L2273-L2324) and the local test.
- **Evidence:** Verified from the engine source and the local microbenchmark.
- **Trap:** A type annotation that needs a conversion at every boundary can move the work, not remove it. Measure end to end, and do not cast in every iteration.

### 6. Prefer value data over per-element `Object` or `RefCounted` records

- **What:** Keep hot numeric state in vectors, typed arrays, packed arrays or parallel arrays. Use `Object` or `RefCounted` records only when you need identity, signals, methods or shared lifetime.
- **Why/thread:** Main thread. Every `Object` registers in the global ObjectDB under a spin lock, and `RefCounted` adds atomic reference operations. Small values are inline, and packed arrays hold contiguous native values. This removes allocation, indirection and reference traffic.
- **Measure:** Compare `OBJECT_COUNT`, `MEMORY_STATIC`, construction time and the hot loop's `Self`. The value-data version keeps the same results with fewer objects.
- **Version:** Godot 4.7.
- **Source:** [Object registration](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/object.cpp#L2284-L2296), [ObjectDB lock](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/object.cpp#L2451-L2464) and [RefCounted operations](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/ref_counted.cpp#L74-L126).
- **Evidence:** Verified from the engine source.
- **Trap:** Parallel arrays are hard to keep synchronized. Use 1 packed layout only for proven dense hot data.

### 7. Use native bulk methods when no script callback is needed

- **What:** Use `fill`, `reverse`, `sort`, `min`, `max` and `bsearch` instead of the same loop in GDScript when the behavior matches.
- **Why/thread:** Main thread. The loop stays in native code and crosses the VM boundary once. Locally, `Array.fill()` was 9.20 times faster than 100,000 indexed script assignments.
- **Measure:** Microbenchmark the exact element type and size, then read the caller's Profiler `Self`. Confirm that the operation does not allocate a result you do not want.
- **Version:** Godot 4.7.
- **Source:** [Array bulk methods](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L733-L771) and the local test.
- **Evidence:** Verified from the engine source and the local microbenchmark.
- **Trap:** `sort_custom`, `bsearch_custom` and the other callback forms call back into script many times. They do not have the same gain.

### 8. Benchmark callback collection helpers against a typed loop

- **What:** In hot paths, compare `map`, `filter`, `reduce`, `any`, `all` and custom comparators with a pre-sized typed loop before you choose the shorter syntax.
- **Why/thread:** Main thread. These native methods still call a `Callable` for every element, and `map` and `filter` also allocate result arrays. Locally, `map(lambda)` was 1.41 times slower than a pre-sized typed loop.
- **Measure:** Time the whole transformation, including the result allocation. Compare allocations through `MEMORY_STATIC` deltas or the stability of repeated runs.
- **Version:** Godot 4.7.
- **Source:** [Array callback loops](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L604-L718) and the local test.
- **Evidence:** Verified from the engine source and the local microbenchmark.
- **Trap:** A manual loop is not always faster. An early-exit `any` or `all`, or a transform that runs rarely, can be clearer and fast enough.

### 9. Do not create one `await` state per entity or per frame

- **What:** Replace high-rate coroutine creation with 1 owner state machine, 1 reusable timer or event, or a batched scheduler.
- **Why/thread:** Main thread. Each `await` on a signal allocates a `GDScriptFunctionState`, copies the function stack, registers it in pending-state lists under a mutex, creates a bound callable and connects it one-shot. The setup and the resume dispatch both cost main-thread time.
- **Measure:** Profiler `Calls` around the async function and the signal callback. Track `OBJECT_COUNT` during a sustained scenario, and time N waits created and resumed with `Time.get_ticks_usec()`.
- **Version:** Godot 4.7.
- **Source:** [`await` VM path](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L2563-L2655).
- **Evidence:** Verified from the engine source.
- **Trap:** A central scheduler adds complexity and cancellation rules. Keep `await` for sparse orchestration where readability matters most.

### 10. Hoist reusable callables and avoid capturing lambda churn

- **What:** Create reusable named callables once. Do not build capturing lambdas or repeated `.bind()` values inside `_process`, sort comparators or per-item loops.
- **Why/thread:** Main thread. A lambda copies its captures into a vector and heap-allocates a `GDScriptLambdaCallable`. A bound callable also builds an argument vector on each call. Locally, lambda creation cost 8.74 times the reuse of a named callable.
- **Measure:** Time callable construction apart from the call, then read the creator's Profiler `Self`. Search frame callbacks for `func(` and `.bind(`.
- **Version:** Godot 4.7.
- **Source:** [Lambda creation](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L2673-L2699), [bound-call dispatch](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/callable_bind.cpp#L141-L150) and the local test.
- **Evidence:** Verified from the engine source and the local microbenchmark.
- **Trap:** A cached lambda keeps its captured objects alive and keeps the captured values. Use a named method when the capture lifetime is not clear.

### 11. Use signals for state transitions, not high-rate data

- **What:** For per-frame or per-entity data, use a direct typed call, a shared buffer or 1 batched "changed" event. Keep signals for decoupled, sparse transitions.
- **Why/thread:** Main thread. Each emission locks the signal map, copies every connection, dispatches every callable, and allocates heap slot storage above 5 listeners. Locally, a one-listener signal cost 2.17 times a direct method call.
- **Measure:** Profiler call counts for emitters and listeners, `get_signal_connection_list()` for fan-out, and a direct-call against signal microbenchmark with the real payload.
- **Version:** Godot 4.7.
- **Source:** [Signal emission](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/object.cpp#L1179-L1325) and the local test.
- **Evidence:** Verified from the engine source and the local microbenchmark.
- **Trap:** A direct call in place of an event boundary can make tight coupling. Change only high-rate signals that you measured.

### 12. Keep `call_deferred` for ordering and thread-safety boundaries

- **What:** Call directly when same-thread ordering allows it. Batch deferred work, and do not queue 1 deferred call per item.
- **Why/thread:** Main thread. Each deferred call locks a queue, stores a message and copies every `Variant` argument. Later it builds the argument pointers again and dispatches the callable. SceneTree also flushes each process-group queue after processing.
- **Measure:** Profile both the enqueue and the callee. Time N direct calls against N deferred calls through the next frame, and read p99. Count `call_deferred` sites in frame callbacks.
- **Version:** Godot 4.7.
- **Source:** [Queue insertion](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/message_queue.cpp#L84-L125) and [queue dispatch](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/message_queue.cpp#L207-L220).
- **Evidence:** Verified from the engine source.
- **Trap:** A direct call is wrong during scene-tree mutation, recursive signal emission or a handoff across threads. Keep the deferred behavior where it is required.

### 13. Prefer statically resolved access over dynamic `call`, `get` and `set`

- **What:** Use typed direct methods and properties in hot paths, not method-name `call()` or property-name `get()` and `set()`.
- **Why/thread:** Main thread. A dynamic call looks up the method name and dispatches through `Variant` and `Object`. A typed native call uses a cached `MethodBind` and the validated call path. A validated named getter calls a preselected getter.
- **Measure:** Compare the exact access forms over many iterations, then read the caller's Profiler `Self`. Search process methods for `.call(`, `.get(` and `.set(`, and skip data-driven code that needs them.
- **Version:** Godot 4.7.
- **Source:** [Generic call](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L1901-L1971) and [validated property get](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L1290-L1302).
- **Evidence:** Verified from the engine source.
- **Trap:** Data-driven dispatch can need names. Replace it with a table of cached callables only when the profile justifies the extra structure.

### 14. Pre-size reusable output and avoid front mutation

- **What:** Reuse arrays, call `resize()` once, write by index, and keep a head index for queues. Do not use `push_front`, `pop_front`, repeated `slice` or step-by-step growth of the output.
- **Why/thread:** Main thread. Front removal shifts elements, a slice allocates a new array and copies elements, and repeated growth can reallocate. This memory movement grows with the collection size.
- **Measure:** Profile the owning function and microbenchmark realistic maximum sizes. Compare p99 and `MEMORY_STATIC`, not only a small average.
- **Version:** Godot 4.7.
- **Source:** [Array remove and slice](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L504-L515) and [slice copy](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L568-L601).
- **Evidence:** Verified from the engine source.
- **Trap:** Kept capacity raises steady memory use, and a head-index queue needs periodic compaction. Compact outside a latency-sensitive frame or under a budget.

### 15. Do one dictionary probe when one is enough

- **What:** In dense hot loops, do not write `if dict.has(key): value = dict[key]`. Use `get(key, sentinel)`, or change the data so 1 lookup gives both presence and value.
- **Why/thread:** Main thread. `has()` and the value access each validate, hash and probe the key. This doubles the hash-table work, most of all with long `String` keys or composite `Variant` keys.
- **Measure:** Microbenchmark the real key type and hit ratio, and read the loop's Profiler `Self`. Search for `.has(` next to a bracket access.
- **Version:** Godot 4.7.
- **Source:** [Dictionary `has`](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/dictionary.cpp#L220-L224) and [Dictionary `get`](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/dictionary.cpp#L167-L187).
- **Evidence:** Verified from the engine source.
- **Trap:** The sentinel must never be a real value. If `null` is a valid value, use a unique sentinel object or change the layout.

### 16. Use integer iteration directly, not an index array

- **What:** Write `for i in count` or `for i in range(...)`. Do not build an `Array` of indices to iterate.
- **Why/thread:** Main thread. Godot 4.7 has integer and range iteration opcodes that increment native integer storage. `range()` in this loop path does not build a heap `Array`.
- **Measure:** Disassemble or microbenchmark only when the index-loop cost is material. Normally, read the enclosing algorithm first.
- **Version:** Godot 4.7.
- **Source:** [Integer iteration begin](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L3065-L3090) and [integer iteration step](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L3471-L3493).
- **Evidence:** Verified from the engine source.
- **Trap:** This does not make a large GDScript loop cheap. A native bulk operation or a better algorithm still gives more.

### 17. Build and format strings only when a consumer needs them

- **What:** Move labels, debug text, formatting and log messages from every frame to state changes or a throttled diagnostics rate.
- **Why/thread:** Main thread. String equality does not allocate, but concatenation and formatting make new string data and formatting arguments. At high frame rates, unchanged text repeats this CPU and memory work every frame.
- **Measure:** Profiler `Self` in UI and debug functions, and `TIME_PROCESS`. Turn off the formatting path for an A/B capture. Search `_process` and `_physics_process` for `%`, `+`, `str`, `print` and text assignments.
- **Version:** Godot 4.7.
- **Source:** [String append](https://github.com/godotengine/godot/blob/4.7.2-stable/core/string/ustring.cpp#L308-L315).
- **Evidence:** Verified from the engine source.
- **Trap:** A throttled visible counter can make the UI feel stale. Update at once on a meaningful change, and throttle only diagnostics that change all the time.
