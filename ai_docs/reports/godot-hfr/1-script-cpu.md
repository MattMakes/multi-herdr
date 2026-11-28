# Godot 4.7 script and CPU hot paths

## Scope and method

This report covers GDScript and main-thread CPU cost at 200–300 fps. The frame budget is 4.17 ms at 240 fps and 3.33 ms at 300 fps. It reports only gaps or corrections relative to `skills/godot-optimization`, `skills/godot-perf-measure`, `skills/godot-multithreading`, and `teammates/godot-performance-engineer.md`.

Evidence was checked against Godot `4.7.2-stable` at commit `ed1daf0bf001b61586d9930840f2f1394092c079`. A local headless debug-build microbenchmark ran 3 measured trials after warm-up with `godot --headless --path . --max-fps 0`. These numbers establish direction only; a release export and the real game scene remain the acceptance measurement.

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

## Corrections to current skills

1. `skills/godot-optimization/references/cpu-bottlenecks.md:13` and `:82-97`, repeated by `skills/godot-optimization/SKILL.md:145`, incorrectly say each temporary `Vector2`/`Vector3` constructor allocates. `Variant` stores these small values inline; the source lists only larger transforms, packed arrays, `Array`, and `Dictionary` as requiring extra memory. Reusing one member vector can also introduce shared mutable state. Keep vector expressions local unless a benchmark proves arithmetic itself is hot. [Variant storage source](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/variant.h#L162-L165)
2. `skills/godot-optimization/references/cpu-bottlenecks.md:176-185` incorrectly says typed arrays skip per-element type checks. Typed `Array` writes call the type validator; typing mainly gives safety and lets the compiler specialize surrounding expressions. Packed arrays provide the contiguous value storage advantage. [Array write source](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L517-L523)
3. `skills/godot-optimization/references/cpu-bottlenecks.md:119` incorrectly says comparing two `String` values allocates temporaries. The comparison scans spans without constructing another string. `StringName == StringName` is still cheaper because it compares interned pointers. [String comparison](https://github.com/godotengine/godot/blob/4.7.2-stable/core/string/ustring.cpp#L338-L340), [StringName comparison](https://github.com/godotengine/godot/blob/4.7.2-stable/core/string/string_name.h#L109-L112)
4. `skills/godot-optimization/SKILL.md:133` recommends moving heavy `_process()` work to `_physics_process()` because it runs less often. That is update throttling with a semantic change, not a general optimization. It can add stale input, uneven visual updates, or physics misuse. Keep simulation in physics; give delay-tolerant work an explicit cadence or budget.

## Ranked techniques

### 1. Remove dormant process callbacks

**What:** Call `set_process(false)` and `set_physics_process(false)` whenever a node has no work, then re-enable it on the state transition that needs updates.

**Why/thread:** This removes the node from the main-thread process group instead of paying a callback and early return every frame. SceneTree copies the process-node vector, checks every entry, and sends each enabled notification. At 240 fps, 1,000 idle callbacks become 240,000 dispatches per second. [Process loop](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/scene_tree.cpp#L1200-L1235), [set_process implementation](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/node.cpp#L1025-L1044)

**Measure:** Use Debugger > Profiler `Calls` for `_process`/`_physics_process`, plus `Performance.TIME_PROCESS`, `TIME_PHYSICS_PROCESS`, and `OBJECT_NODE_COUNT`. Compare the same scene with idle actors enabled and disabled.

**Version/source/evidence:** Godot 4.7; engine source read, **verified**.

**Trap:** A disabled node misses polling-based state changes. Re-enable it from an event, timer, visibility transition, or owner.

### 2. Batch homogeneous updates behind one callback

**What:** For hundreds of identical lightweight actors, keep state in typed or packed arrays and update them from one manager callback instead of one script callback per node.

**Why/thread:** This preserves the same main-thread work while removing repeated SceneTree notification and GDScript call boundaries. The process loop performs per-node eligibility checks and notifications before business logic begins.

**Measure:** Profiler `Calls` should collapse from N callbacks to 1; compare `TIME_PROCESS`, manager `Self`, and release-export p50/p99 frame time. Keep entity count and results identical.

**Version/source/evidence:** Godot 4.7; [process-loop source](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/scene_tree.cpp#L1203-L1233), **verified**.

**Trap:** Do not erase useful node ownership or lifecycle boundaries for tens of objects. The gain matters only when callback count is large and measured.

### 3. Use explicit update tiers instead of moving arbitrary work to physics

**What:** Run visual work every rendered frame, simulation at the physics tick, and delay-tolerant AI/UI/cache work at explicit lower cadences distributed across frames.

**Why/thread:** This reduces main-thread call count without coupling unrelated behavior to `physics_ticks_per_second`; it also avoids all agents spiking on one frame.

**Measure:** Record Profiler call counts by tier and `TIME_PROCESS`/`TIME_PHYSICS_PROCESS`; verify p99 and worst frames, not only averages. At 240 fps, confirm a 10 Hz task runs 10 times per second, not once every fixed number of render frames.

**Version/source/evidence:** Godot 4.7; SceneTree has separate process and physics notification paths in [scene_tree.cpp](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/scene_tree.cpp#L1218-L1231), **verified**.

**Trap:** Frame-modulo throttles change wall-clock frequency as FPS changes. Accumulate `delta`, use a timer, or schedule against absolute time when cadence matters.

### 4. Cache node references after scene wiring

**What:** Resolve stable dependencies once with `@onready`, an exported node reference, or dependency injection; do not repeat `$Path` or `get_node()` in a hot loop.

**Why/thread:** `$Path` compiles to node lookup, not a cached field. Each main-thread lookup walks each `NodePath` name and performs a child hash lookup; unique-name lookup is still a lookup. The local test made a 3-segment lookup 2.77× the cost of using a cached reference.

**Measure:** Microbenchmark the exact path, then compare the caller's Profiler `Self` and `TIME_PROCESS`. Grep repeated `$`/`get_node` expressions inside frame callbacks.

**Version/source/evidence:** Godot 4.7; [Node path traversal](https://github.com/godotengine/godot/blob/4.7.2-stable/scene/main/node.cpp#L1904-L1963) plus local 4.7.2 test, **verified**.

**Trap:** A cached reference becomes invalid when scenes replace or free that node. Rebind on the owning lifecycle event and use `is_instance_valid()` only when replacement is legitimate.

### 5. Type the complete hot expression and call chain

**What:** Type parameters, locals, members, return values, container variables, and native receivers used by hot arithmetic or method calls.

**Why/thread:** The GDScript VM has validated operator, property, method-bind, and iteration opcodes that call preselected functions instead of generic Variant dispatch on the main thread. The local `Variant` integer loop was 1.61× the typed loop.

**Measure:** Compare equivalent typed and dynamic functions with `Time.get_ticks_usec()` over many calls, then verify the real caller in the Profiler. Treat warnings that force a value back to `Variant` as a broken typed chain.

**Version/source/evidence:** Godot 4.7; [validated operator](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L848-L863), [validated native call](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L2273-L2324), and local test, **verified**.

**Trap:** A type annotation that requires conversions on every boundary can move rather than remove work. Measure end-to-end and do not cast every iteration.

### 6. Prefer value data over per-element `Object` or `RefCounted` records

**What:** Store hot numeric state in vectors, typed arrays, packed arrays, or parallel arrays; use `Object`/`RefCounted` records only when identity, signals, methods, or shared lifetime are required.

**Why/thread:** Every `Object` registers in the global ObjectDB under a spin lock; `RefCounted` adds atomic reference operations. Small Variant values such as integers and vectors are inline, while packed arrays hold contiguous native values. This saves main-thread allocation, indirection, and reference traffic.

**Measure:** Compare `OBJECT_COUNT`, `MEMORY_STATIC`, construction time, and hot-loop `Self`; the value-data version should preserve results with fewer objects.

**Version/source/evidence:** Godot 4.7; [Object registration](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/object.cpp#L2284-L2296), [ObjectDB lock](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/object.cpp#L2451-L2464), [RefCounted operations](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/ref_counted.cpp#L74-L126), **verified**.

**Trap:** Parallel arrays can become hard to keep synchronized. Prefer one packed representation only for proven dense hot data.

### 7. Use native bulk methods when no script callback is required

**What:** Prefer operations such as `fill`, `reverse`, `sort`, `min`, `max`, and `bsearch` over equivalent element-by-element GDScript loops when semantics match.

**Why/thread:** The loop stays in native code and crosses the VM boundary once. Local `Array.fill()` was 9.20× faster than 100,000 indexed script assignments.

**Measure:** Microbenchmark the exact element type and size, then inspect the caller's Profiler `Self`. Confirm the operation does not allocate an unwanted result.

**Version/source/evidence:** Godot 4.7; [Array bulk implementations](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L733-L771) and local test, **verified**.

**Trap:** `sort_custom`, `bsearch_custom`, and other callback variants cross back into script many times and do not have the same advantage.

### 8. Benchmark callback-based collection helpers against a typed loop

**What:** In hot paths, compare `map`, `filter`, `reduce`, `any`, `all`, and custom comparators with a pre-sized typed loop before choosing the shorter syntax.

**Why/thread:** These native methods still invoke a `Callable` for every element; `map` and `filter` also allocate result arrays. The local `map(lambda)` test was 1.41× slower than a pre-sized typed loop.

**Measure:** Time the whole transformation, including result allocation, and compare allocations through `MEMORY_STATIC` deltas or repeated-run stability.

**Version/source/evidence:** Godot 4.7; [Array callback loops](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L604-L718) and local test, **verified**.

**Trap:** Manual loops are not automatically faster. Early-exit `any`/`all` or a rarely run transform may be clearer and fast enough.

### 9. Do not create one `await` state per entity or per frame

**What:** Replace high-rate coroutine creation with one owner state machine, one reusable timer/event, or a batched scheduler.

**Why/thread:** Awaiting a signal allocates a `GDScriptFunctionState`, copies the function stack, registers it in pending-state lists under a mutex, creates a bound callable, and connects one-shot. All setup and resume dispatch affect the main thread.

**Measure:** Profiler `Calls` around the async function and signal callback; track `OBJECT_COUNT` during a sustained scenario and time creating/resuming N waits with `Time.get_ticks_usec()`.

**Version/source/evidence:** Godot 4.7; [await VM path](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L2563-L2655), **verified**.

**Trap:** A central scheduler adds complexity and cancellation rules. Keep `await` for sparse orchestration where readability dominates.

### 10. Hoist reusable callables and avoid capturing lambda churn

**What:** Create reusable named callables once; do not construct capturing lambdas or repeated `.bind()` values inside `_process`, sort comparators, or per-item loops.

**Why/thread:** Lambda creation copies captures into a vector and heap-allocates a `GDScriptLambdaCallable`; bound callables also assemble argument vectors on calls. Local lambda creation was 8.74× the cost of reusing a named callable in the test's construction-focused loops.

**Measure:** Time callable construction separately from invocation, then inspect the creator's Profiler `Self`. Grep `func(` and `.bind(` inside frame callbacks.

**Version/source/evidence:** Godot 4.7; [lambda creation](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L2673-L2699), [bound-call dispatch](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/callable_bind.cpp#L141-L150), and local test, **verified**.

**Trap:** Caching a lambda also keeps captured objects alive and preserves captured values. Prefer a named method when capture lifetime is unclear.

### 11. Use signals for state transitions, not high-frequency data transport

**What:** For per-frame or per-entity data, use a direct typed call, shared buffer, or one batched “changed” event; keep signals for decoupled, relatively sparse transitions.

**Why/thread:** Each main-thread emission locks the signal map, copies every connection, dispatches every callable, and allocates heap slot storage above 5 listeners. A one-listener signal was 2.17× a direct method call locally.

**Measure:** Use Profiler call counts for emitters/listeners, `get_signal_connection_list()` for fan-out, and a direct-versus-signal microbenchmark with the real payload.

**Version/source/evidence:** Godot 4.7; [signal emission](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/object.cpp#L1179-L1325) and local test, **verified**.

**Trap:** Replacing event boundaries with direct calls can create tight coupling. Optimize only high-rate measured signals.

### 12. Reserve `call_deferred` for ordering and thread-safety boundaries

**What:** Call directly when same-thread ordering permits; batch deferred work instead of enqueueing one deferred call per item.

**Why/thread:** Each deferred call locks a queue, stores a message, copies every Variant argument, and later rebuilds argument pointers and dispatches the callable. SceneTree also flushes each process-group queue after processing.

**Measure:** Profile both enqueue and eventual callee; time N direct calls versus N deferred calls through the next frame and inspect p99. Count `call_deferred` sites in frame callbacks.

**Version/source/evidence:** Godot 4.7; [queue insertion](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/message_queue.cpp#L84-L125), [queue dispatch](https://github.com/godotengine/godot/blob/4.7.2-stable/core/object/message_queue.cpp#L207-L220), **verified**.

**Trap:** Direct replacement is wrong during scene-tree mutation, recursive signal emission, or cross-thread handoff. Preserve required deferred semantics.

### 13. Prefer statically resolved access over dynamic `call`, `get`, and `set`

**What:** Use typed direct methods and properties in hot paths instead of method-name `call()`, property-name `get()`, and `set()`.

**Why/thread:** Dynamic VM calls look up a method name and dispatch through Variant/Object, while typed native calls use a cached `MethodBind` and validated call path. Validated named getters similarly call a preselected getter.

**Measure:** Compare the exact access forms over many iterations, then use Profiler `Self` on the caller. Grep `.call(`, `.get(`, and `.set(` within process methods, excluding necessary data-driven code.

**Version/source/evidence:** Godot 4.7; [generic call](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L1901-L1971), [validated property get](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L1290-L1302), **verified**.

**Trap:** Data-driven dispatch may legitimately need names. Replace it with a table of cached callables only when the profile justifies the extra structure.

### 14. Pre-size reusable output and avoid front mutation

**What:** Reuse arrays, call `resize()` once, write by index, and keep a head index for queues rather than `push_front`, `pop_front`, repeated `slice`, or growing output incrementally.

**Why/thread:** Front removal shifts elements, slices allocate another array and copy elements, and repeated growth can reallocate. This is main-thread memory movement that grows with collection size.

**Measure:** Profile the owning function and microbenchmark realistic maximum sizes; compare p99 and `MEMORY_STATIC`, not only a small average.

**Version/source/evidence:** Godot 4.7; [remove/slice source](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L504-L515), [slice copy](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/array.cpp#L568-L601), **verified**.

**Trap:** Retained capacity raises steady memory use, and a head-index queue needs periodic compaction. Compact outside a latency-sensitive frame or under a budget.

### 15. Perform one dictionary probe when one will do

**What:** Avoid `if dict.has(key): value = dict[key]` in dense hot loops; use `get(key, sentinel)` or redesign the data so one lookup determines presence and value.

**Why/thread:** `has()` and value access each validate/hash/probe the key. This doubles hash-table work on the main thread, especially with long `String` or composite Variant keys.

**Measure:** Microbenchmark the real key type and hit ratio; inspect the loop's Profiler `Self`. Grep adjacent `.has(` and bracket access.

**Version/source/evidence:** Godot 4.7; [Dictionary has](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/dictionary.cpp#L220-L224) and [Dictionary get](https://github.com/godotengine/godot/blob/4.7.2-stable/core/variant/dictionary.cpp#L167-L187), **verified**.

**Trap:** A sentinel must be impossible as a real value. If `null` is valid, use a unique sentinel object or change the representation.

### 16. Use integer iteration directly; do not build an index array

**What:** Write `for i in count` or `for i in range(...)`; do not create an Array of indices to iterate.

**Why/thread:** Godot 4.7 has specialized integer and range iteration opcodes that increment native integer storage. `range()` is not evidence of a heap Array in this loop path.

**Measure:** Disassemble or microbenchmark only if index-loop cost is material; normally inspect the enclosing algorithm first.

**Version/source/evidence:** Godot 4.7; [integer iteration begin](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L3065-L3090) and [integer iteration step](https://github.com/godotengine/godot/blob/4.7.2-stable/modules/gdscript/gdscript_vm.cpp#L3471-L3493), **verified**.

**Trap:** This does not make a large GDScript loop cheap. Native bulk operations or a better algorithm can still dominate this micro-optimization.

### 17. Build and format strings only when a consumer needs them

**What:** Move labels, debug text, formatting, and log messages from every frame to state changes or a throttled diagnostics cadence.

**Why/thread:** String equality itself does not allocate, but concatenation and formatting produce new string data and formatting arguments. At high refresh rates, even unchanged text repeats CPU and memory work on the main thread.

**Measure:** Profiler `Self` in UI/debug functions and `TIME_PROCESS`; disable the formatting path for an A/B capture. Search `_process` and `_physics_process` for `%`, `+`, `str`, `print`, and text assignments.

**Version/source/evidence:** Godot 4.7; [String append source](https://github.com/godotengine/godot/blob/4.7.2-stable/core/string/ustring.cpp#L308-L315), **verified**.

**Trap:** Throttling visible counters can make UI feel stale. Update immediately on meaningful changes and throttle only continuously changing diagnostics.

## Detection

Use a debug capture to locate candidates, then prove gains in an uncapped release export. At 240 fps, investigate a subsystem once it consistently consumes about 0.20 ms (5% of budget); at 300 fps, 0.17 ms is already 5%. These are triage thresholds, not universal acceptance limits.

1. **Dormant and numerous callbacks:** In Debugger > Profiler, sort by `Calls`; compare `_process` and `_physics_process` totals with `Performance.TIME_PROCESS`, `TIME_PHYSICS_PROCESS`, and `OBJECT_NODE_COUNT`. Grep: `rg -n "func _process|func _physics_process" --glob '*.gd'`.
2. **Repeated node lookup:** Inspect high-`Self` callbacks. Grep: `rg -n '\$[^ ]+|get_node\(|get_node_or_null\(' --glob '*.gd'` and check whether each expression executes per frame.
3. **Dynamic hot code:** Follow warnings and untyped values through the whole top-profiled function. Grep hot files for `: Variant`, `.call(`, `.get(`, and `.set(`.
4. **Async/deferred churn:** Grep: `rg -n 'await |call_deferred|\.bind\(|func\(' --glob '*.gd'`. In the profiler, match creation call counts to entity count and watch `OBJECT_COUNT` during sustained play.
5. **Signal fan-out:** Start from high-call emitters, call `get_signal_connection_list()` in a diagnostic build, and list every listener. Grep: `rg -n '\.emit\(|emit_signal\(' --glob '*.gd'`.
6. **Collection and allocation hot paths:** Inspect top `Self` loops for `map`, `filter`, `reduce`, custom sorts, `slice`, front mutation, dictionary double probes, string formatting, and per-frame output creation. Microbenchmark the exact container size and type.
7. **Object-heavy data:** Compare `OBJECT_COUNT`, `OBJECT_NODE_COUNT`, and `MEMORY_STATIC` against live gameplay entity counts. A much larger count suggests per-element `Object`/`RefCounted` records or hidden node fan-out.
8. **Acceptance:** Run 3–5 interleaved A/B pairs with VSync disabled and `--max-fps 0`. Report p50, p95, p99, worst frame, `TIME_PROCESS`, `TIME_PHYSICS_PROCESS`, and identical game-state results. A lower average with a worse p99 does not meet the high-refresh goal.
