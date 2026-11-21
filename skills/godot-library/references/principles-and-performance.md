# Principles and performance

Target engine: Godot 4.7. The Godot techniques where the fast choice is also the clean choice by SRP, DRY, KISS or YAGNI, and the rule to follow where speed and a principle pull apart.

## Use this shelf when

- You are about to add a manager, a pool, an event bus, a thread or an abstraction layer "for performance" or "for later".
- You review a change and must judge both its structure and its cost.
- You have a measured hot spot and must speed it up without spreading complexity.
- You choose between hand-written script code and an engine feature that does the same job.
- You plan a system and must decide what is fixed now and what can wait.

## The rule that joins them

- The manual says scripts and scenes should follow single responsibility, DRY, KISS and YAGNI by name (scene organization page).
- Script work is the expensive part. Every script access to an engine object walks a query chain (script, then each class up the line); the manual names this chain as the reason script code is slow. A `PackedScene` is instanced in engine code, while the same tree built in script pays one API crossing per statement. Built-in functions run at the same speed from any language, and the GDScript VM author notes that most of what a game script does already runs in engine C++.
- So the engine-side path (built-in nodes, `Resource` files, signals, notifications, typed code, servers behind nodes) is usually both the least code and the fastest.
- The manual's speed rule is YAGNI applied to speed: profile, fix the largest bottleneck, profile again. It warns that optimized code is harder to read and debug, and that a small gain can be worth less than its complexity. The renderer team agrees: optimizing means not doing what need not be done, guesses are often wrong, and without a test case there is no optimization.
- Bottleneck math (manual): A = 9 ms of a 10 ms frame, made 9x faster, gives a 5x faster frame. The same fix in a 59 ms frame gives 51 ms. If the GPU needs 50 ms, CPU work gains nothing.
- The limit of YAGNI: the manual also says speed comes from design. Data layout and algorithm are chosen before the first line and cannot be polished in later.

## SRP

1. **One node, one job, callbacks off until needed.** A script that defines `_process` or `_physics_process` is called every frame or tick; the manual says these callbacks do not rest. Call `set_process(false)` / `set_physics_process(false)` in `_ready` and switch on only while the job is active; freeze a whole subsystem with `process_mode` on its root. Responsibility and cost are then both local. Limit: an early `return` inside a running callback still pays the call. Details: `cpu-and-frame-budget`.
2. **Data in a `Resource`, behaviour in a node, view reads the data.** Design values and rules then change apart. Views connect to `changed` (sent by `emit_changed()`) and do no polling. The manual ranks `Resource` as nearly as light as `Object` while keeping Inspector editing, and names it as the cheaper stand-in for sub-nodes. Limit: a loaded resource is shared, so run-time state needs its own copy (see DRY 1).
3. **Calls go down, signals go up, the common owner wires siblings.** A child that never looks outward runs alone. The servers page notes that most engine work is signal-driven, so nothing polls. Limit: a signal emitted per entity per frame is polling with extra cost; use a direct call there.
4. **Each scene owns the nodes it needs.** The manual's own example: a global sound-pool autoload gives global state, untraceable callers and a pool that is either too small or wasted memory; an `AudioStreamPlayer` inside each scene removes all three.
5. **One owner for one hot loop.** When hundreds of nodes each run the same few lines per frame, move the loop into one system node that owns that data and only that rule. This keeps SRP: the rule has one place. It is faster because every node has a tree cost and each callback is a separate call into the VM; the engine lead (2023) describes nodes as made for complex behaviour on hundreds of objects at most. It breaks SRP once the manager changes other systems' data. Shape and numbers: `cpu-and-frame-budget`, `servers-and-low-level`.
6. **Split simulation from view.** Rules run in `_physics_process` on plain data; view nodes copy state for drawing. Tests run headless and the tick rate can drop without touching the view (KISS 6). See `determinism-and-simulation`.
7. **Single ownership is what makes threads possible.** The active tree is not thread-safe. A subtree that talks to the rest only through deferred calls can process in a sub-thread: `process_thread_group = PROCESS_THREAD_GROUP_SUB_THREAD` plus `call_deferred_thread_group()`. A subsystem with one owner needs one property change; a tangled one needs a rewrite. Limit: the core team lists nodes that assume one thread as open work, so measure. Rules: `threads-and-concurrency`.
8. **Input in input callbacks.** `_unhandled_input` runs only on frames with an event; a poll in `_process` runs every frame (manual).

## DRY

1. **Share `Resource` instances.** The engine loads a path once and hands the same instance to every user: one copy in memory, one file to edit. Use `resource_local_to_scene` or `duplicate()` only for state that differs per instance; each copy gives up the sharing. Copy rules: `data-and-memory`.
2. **Instance and inherit scenes; do not copy node trees or build them in script.** One scene file is the single definition, and instancing runs in engine code, which the manual calls much faster than the same construction in script. Since 4.6 per-node ids keep overrides alive when the base scene is restructured. See `scene-architecture`.
3. **One material, many looks.** Use `instance uniform` with `set_instance_shader_parameter()`, global uniforms (`RenderingServer.global_shader_parameter_set()`) for values many shaders read, and `#include` of a `.gdshaderinc` file for shared functions. The manual says one unique material per object costs performance; the 4.4 release page says 2D instance uniforms do not break batching. Limit: no textures or arrays per instance. See `shaders-advanced`.
4. **One mesh, one `MultiMesh`.** Thousands of copies become one draw primitive (manual). Limit: the whole set is culled as one.
5. **`class_name` plus `static func` instead of helper autoloads or pasted helpers.** The manual recommends it for shared functions and `static var` for shared state. No node, no load order. Limit: a call into another script function is not free in a hot loop (`gdscript-performance`).
6. **One `Theme` with type variations (`theme_type_variation`) instead of `add_theme_*_override` on each control.** The docs give no speed figure; this is a structure gain. See `ui-advanced`.
7. **Project settings with feature-tag overrides instead of platform branches in code.** One value, read with `ProjectSettings.get_setting_with_override()`; the branch never runs. See `scene-architecture`.
8. **Named input actions and one name table.** Gameplay reads actions (`Input.get_vector()`), never raw keys, and action names live as `StringName` constants in one script. The gain is rebinding and typo safety, not speed: the compiler often converts constant strings already (`gdscript-performance`).
9. **State the contract once, in code.** `@tool` plus `_get_configuration_warnings()` turns a documented rule into a check the editor runs; the manual prefers it to documentation that duplicates the scene.

## KISS

1. **A built-in node before a script.** `Timer` for work that need not run every frame (the manual's answer), `Tween` and `AnimationPlayer` for value changes over time, `VisibleOnScreenEnabler2D` / `VisibleOnScreenEnabler3D` to stop off-screen work, `RemoteTransform2D` to follow without reparenting.
2. **An engine call before a script loop.** `AStarGrid2D`, `Geometry2D`, packed-array methods such as `sort()`, `has()`, `fill()`. List and limits: `gdscript-performance`.
3. **Pick the container by the access you need.** `Array` to iterate and index, `Dictionary` for keyed lookup, a class when the API matters more than raw access (manual, data preferences; that page is flagged outdated, so measure). Table: `data-and-memory`.
4. **Primitive collision shapes.** The manual recommends them for moving bodies as the most reliable and often the fastest, and a simplified mesh for static collision.
5. **An enum and `match` before a state framework.** Fewer nodes, fewer callbacks; no doc figure, so measure. Move up when states need their own child nodes: `godot-state-machine`.
6. **Default tick, or a lower tick plus built-in interpolation.** Turn on `physics/common/physics_interpolation` instead of writing a smoother. The manual: interpolation is orders of magnitude cheaper than a tick, a lower tick adds input lag, and 60 Hz stays the advice for real-time player movement.
7. **Set properties, then `add_child`.** Setters of a node inside the tree can trigger updates; the manual says this can slow procedural generation to a crawl. Exception: values that need the tree, such as `global_position`.
8. **A direct call or a local signal before an event bus.** A bus is global access; keep it for events that many unrelated scenes hear. See `godot-event-bus`.

## YAGNI

1. **Measure before you optimize.** Profile first; time a suspect with `Time.get_ticks_usec()` over 1,000 or more runs and average (manual). No number, no change.
2. **No pool until a profile shows spawn cost.** GDScript frees objects at once; the CPU page names pooling for C# collection pauses. See `data-and-memory`.
3. **No servers, threads or native code before the simpler steps.** The manual places servers after other avenues are exhausted, for thousands of objects updated every frame, and `MultiMesh` for hundreds of thousands; it warns that threads make debugging much harder.
4. **No speculative autoload or layer.** An autoload needs three things at once: it owns all its data, must be global, works in isolation. The engine team holds itself to this: a feature is refused until a real project needs it.
5. **Load late.** Use `load()` where the value is needed; give exports no preloaded default (the Inspector value replaces it); mark rarely opened parts as `InstancePlaceholder`. A `preload` constant is freed only when its script is. Start with a static level and stream "when the performance requires it" (manual). Limit: `load()` in play can hitch.
6. **Leave defaults alone until a number says otherwise.** Tick rate, thread model and render settings fit the common case.
7. **Delete what does nothing.** Every node has a cost; the renderer team names leftover debug code and needless calls as usual finds. Removing them is the cheapest optimization.
8. **Build profiles only when size matters.** Project > Tools > Engine Compilation Configuration Editor shrinks nothing by itself; it needs custom export templates. Do it for a web or mobile size budget only. See `gdextension-and-engine-builds`.

```gdscript
func usec_per_call(work: Callable, runs: int = 1000) -> float:
	var start: int = Time.get_ticks_usec()
	for i: int in runs:
		work.call()
	return float(Time.get_ticks_usec() - start) / runs
```

## Where speed and a principle pull apart

| Conflict | Rule | Evidence needed |
|---|---|---|
| DRY vs a hot inner loop | Inline the helper only inside the measured loop; keep it for all other callers. Test that both give the same result. | Timing with and without the call |
| DRY vs per-instance copies | Try instance uniforms or exported values first; copy a resource only for real per-instance state. | A value that cannot be a uniform |
| SRP vs one batch system | Allowed when node or callback count is the measured cost. The system owns one loop and its data, nothing else. | Script or tree time grows with count |
| KISS vs servers, `MultiMesh`, threads, native code | Stay with nodes below the manual's scale (thousands updated per frame). Above it, go one step at a time. | Frame time per step, in ms |
| KISS vs a lower tick rate | Keep 60 Hz for direct player control; lower it only with interpolation on. | CPU time of the physics step |
| YAGNI vs lazy loading | Lazy by default; preload or load in a thread what a profile shows as a hitch. | A hitch at a `load()` call |
| YAGNI vs costly retrofit | Decide early; see the list below. | None: cheap now, a rewrite later |

Decide early, even with no present need (the first two follow from the docs; the rest are team judgement):

- Data layout and algorithm of any system that may reach thousands of items (manual: speed comes from design).
- Static typing with the `debug/gdscript/warnings/untyped_declaration` warning raised. It changes the opcodes (`gdscript-performance`) and is slow to add late.
- Input actions instead of key checks; `tr()` keys instead of literal text (`i18n-and-accessibility`).
- A version field in every save file (`io-and-saves`); `.uid` files committed (`engine-internals`).
- The sim and view split, a fixed tick and seeded random numbers if replays or lockstep are possible (`determinism-and-simulation`); who has authority, if multiplayer is possible (`networking-advanced`).

Defer everything else: pools, tiers, servers, threads, LOD systems, streaming, build profiles, an event bus, a plugin.

**Isolate the ugly part.** When a measurement justifies fast but ugly code, keep it behind one small typed API. Callers hold no packed array, `RID` or index, so the rest of the game stays simple and the inside can change again. Here `multimesh.instance_count` is set once in the scene as the cap:

```gdscript
class_name EmberSwarm
extends MultiMeshInstance2D

var _pos: PackedVector2Array = PackedVector2Array()
var _vel: PackedVector2Array = PackedVector2Array()

func add_ember(at: Vector2, velocity: Vector2) -> void:
	_pos.append(at)
	_vel.append(velocity)

func _physics_process(delta: float) -> void:
	multimesh.visible_instance_count = _pos.size()
	for i: int in _pos.size():
		_pos[i] += _vel[i] * delta
		multimesh.set_instance_transform_2d(i, Transform2D(0.0, _pos[i]))
```

## Review checklist

1. SRP: each `_process` / `_physics_process` must run every frame; if not, it is off or replaced by a signal or `Timer` (idle cost).
2. SRP: no reusable scene reaches outward with `get_parent()` or `../` paths (reuse; no failed lookups).
3. SRP: a new manager owns one loop and its own data (fewer callbacks, no hidden coupling).
4. DRY: no pasted node subtree or script; it is an instanced scene or a `static func` (engine-side instancing).
5. DRY: no material or resource duplicated only to change one value (batching, memory).
6. KISS: no script loop that an engine call or built-in node already does (interpreted steps removed).
7. KISS: properties are set before `add_child` (no repeated setter updates).
8. KISS: collision uses primitive shapes unless a reason is written down (physics cost).
9. YAGNI: each pool, thread, server call, cache or tier names its measurement (ms before and after).
10. YAGNI: no new autoload, bus signal or base class with a single user (load time, global state).
11. YAGNI: dead nodes, unused exports and empty callbacks are deleted in the same change (per-node cost).
12. Isolation: optimized code sits behind one typed API; no caller sees its storage.

## Traps

- Reading "premature optimization" as "design can wait". The manual says the opposite for data layout and algorithms.
- Calling a batch system an SRP violation by reflex, or a do-everything autoload "a manager for performance".
- Timing in the editor, with V-Sync on, or once. Use a release export, ms, and averages (`cpu-and-frame-budget`).
- A cache added without a test: the renderer team names caching as a common change that can make code slower.

## Version notes

Features that delete boilerplate (dated by the class reference of 4.3 to 4.7 or a release page):

- 4.4: typed `Dictionary[K, V]`; `@export_tool_button`; instance uniforms in 2D shaders; `.uid` files; translation domains (`Object.set_translation_domain()`); 3D physics interpolation.
- 4.5: `@abstract`; variadic functions; `Variant` exports; `duplicate_deep()`; `SceneTree.scene_changed`; `Logger` with `OS.add_logger()` (one sink for all engine messages); `Control.mouse_behavior_recursive` (no per-child input flags); `CONNECT_APPEND_SOURCE_OBJECT`.
- 4.6: unique node ids in scene files; `SceneTree.change_scene_to_node()`.
- 4.7: `Tween.tween_await()`.

## Related skills

- `godot-code-review`: the full review procedure; the checklist here adds the cost view.
- `godot-safe-refactor`: proving that a cleanup or a speed-up kept behaviour.
- `godot-component-system`: small single-job component nodes.
- `godot-resource-pattern`: data classes as resources; shared or unique.
- `godot-event-bus`: when a global signal hub is justified.
- `godot-optimization`, `godot-perf-measure`: profiler steps and trusted A/B numbers.
- `godot-state-machine`: from enum to node states.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/best_practices/scene_organization.html and, in the same folder, scenes_versus_scripts, autoloads_versus_regular_nodes, node_alternatives, godot_notifications, data_preferences, logic_preferences
- https://docs.godotengine.org/en/stable/tutorials/performance/general_optimization.html and, in the same folder, cpu_optimization, using_servers, using_multimesh, thread_safe_apis
- https://docs.godotengine.org/en/stable/tutorials/scripting/resources.html
- https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/shading_language.html and shader_preprocessor.html
- https://docs.godotengine.org/en/stable/tutorials/physics/collision_shapes_3d.html
- https://docs.godotengine.org/en/stable/tutorials/editor/using_engine_compilation_configuration_editor.html
- https://godotengine.org/article/rendering-cpu-optimizations/
- https://godotengine.org/article/whats-missing-in-godot-for-aaa/
- https://godotengine.org/article/gdscript-progress-report-typed-instructions/
- https://godotengine.org/article/will-your-contribution-be-merged-heres-how-tell/
- https://godotengine.org/releases/4.4/ , https://godotengine.org/releases/4.5/ , https://godotengine.org/releases/4.6/ , https://godotengine.org/releases/4.7/
- https://godotengine.org/priorities/
- Class reference XML of 4.3 to 4.7, used to date the APIs
