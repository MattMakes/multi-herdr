# GDScript performance

Target engine: Godot 4.7. This shelf explains how the GDScript VM runs code, which language
features make it faster or slower, and the less-known annotations that speed up development.

## Use this shelf when

- A script function shows up in the profiler and you must decide what to change.
- You set the typing and warning policy of a project.
- You choose between `Array`, a typed array and a packed array for hot data.
- You design an inspector surface with exports, tool buttons or custom property lists.
- You must judge whether a loop belongs in GDScript, an engine call, C# or C++.

## How the engine works here

- A script is parsed, analyzed (types and warnings) and compiled to bytecode. The VM runs one
  opcode after another on `Variant` slots. There is no JIT or AOT in 4.7. The team lists
  runtime speed as an active priority: GDScript is fast enough as glue between nodes, weak at
  pure data crunching, and JIT or AOT are still only under investigation.
- The compiler emits a specialized opcode when it knows the types at compile time. The 4.7
  opcode list (engine source) shows the pairs: `OPCODE_OPERATOR` and
  `OPCODE_OPERATOR_VALIDATED`, `OPCODE_GET_NAMED` and `OPCODE_GET_NAMED_VALIDATED`,
  `OPCODE_CALL` and `OPCODE_CALL_METHOD_BIND_VALIDATED_RETURN`, plus
  `OPCODE_CALL_BUILTIN_TYPE_VALIDATED` and `OPCODE_CALL_UTILITY_VALIDATED`. The plain form
  looks things up by name and checks types at run time. The validated form holds a direct
  pointer to the operator, built-in method or engine method. The author of the VM gives the
  reason: for 2 known integers the VM takes the raw values out of the `Variant` and adds
  them in C++; the untyped path must first find the types and then the operation.
- Loops have typed forms too: one iterate opcode per source type (`int`, `float`, vectors,
  `Array`, `Dictionary`, each packed array, and a range form). The generic form asks the
  `Variant` what it is on every step.
- The list has no validated form for a call into another script function. A script-to-script
  call stays the costly kind even when typed (read from the opcode list; measure first).
- Every engine object access passes through the `Object` query chain (script, then each
  class up the inheritance line). The docs name this chain as the main cost of script code.
- Debug builds carry extra checks, line tracking and call-stack tracking. A release export
  removes `assert` (the expression is not evaluated) and turns script backtraces off unless
  `debug/settings/gdscript/always_track_call_stacks` is on.

Reported numbers, per instruction, typed against untyped (GDScript progress report "Typed
instructions", synthetic loops, debug build, 2020; measure first):

| Instruction | Typed is faster by |
| --- | --- |
| Engine class method call, arguments already of the right type | 120-150% |
| Engine class method call, arguments need a check | 70-80% |
| Built-in type method call (`Vector2`, `String`, ...) | 30-70% |
| Operators, utility functions such as `sin()` | 25-50% |
| `for` iteration (less for dictionaries and strings) | 10-50% |
| Subscript and attribute access | 5-7% |

So typing pays most on calls into the engine, and an argument that needs a conversion may
lose the fast path: pass a `float` where a `float` is expected. A whole-function benchmark
(GitHub PR 70838, Godot 4.0, 2023; measure first) reports release typed 3.3 ms, release
untyped 6.4 ms, debug typed 16.0 ms, debug untyped 29.2 ms. Debug timings are not release
timings.

## Advanced techniques

### Type everything the hot path touches

Typed locals, parameters, return values and members let whole expressions use validated
opcodes. One untyped value in a chain makes the rest generic. `:=` infers the type, but
inference from a `Variant` gives nothing, which is why `inference_on_variant` is an error by
default.

```gdscript
extends Node2D

@onready var _sprite: Sprite2D = $Sprite2D
var _speeds: PackedFloat32Array = PackedFloat32Array()

func _process(delta: float) -> void:
	var total: float = 0.0
	for speed: float in _speeds:
		total += speed * delta
	_sprite.position.x += total
```

### Know what typed containers do and do not type

`Array[T]` and `Dictionary[K, V]` (since 4.4) type the loop variable and the `[]` operator.
Methods such as `front`, `back` and `pop_back` still return `Variant`, so assign the result
to a typed variable. Each write is checked against the element type. Nested element types
are not allowed. `Array[Node2D]` cannot be assigned to an `Array[Node]` variable; convert
with `Array.assign`, which copies.

### Packed arrays for large flat data

The docs rank them: packed arrays iterate and modify faster and use less memory than typed
arrays, typed arrays beat untyped ones. The threshold they give: it starts to matter when an
array reaches tens of thousands of elements. Below that, keep `Array[T]` for `map`,
`filter` and `sort_custom`. Packed arrays are values with copy-on-write: passing one is
cheap, the first write through a second reference copies everything.

### Push the loop into the engine

One engine call that loops in C++ beats a script loop: `fill`, `append_array`, `slice`,
`find`, `has`, `sort`, `bsearch`, `max`, `min`, `PackedByteArray` encode and decode methods,
`Geometry2D` helpers. `range(n)` and `for i in n` do not allocate an array.
`for i in array.size()` is the form to use when the loop writes back to the array.

### Lambdas and Callables in hot code

A lambda expression creates a new `Callable` every time the line runs and copies its
captured locals by value. Create it once outside the loop. `Array.map`, `filter`, `reduce`,
`any` and `all` do one `Callable` call per element; a typed `for` loop does none. A
`Callable` must be invoked with `call`; the docs say the `()` form was left out to keep
direct function calls fast.

### StringName and NodePath literals

`&"name"` is a `StringName` literal and `^"A/B"` is a `NodePath` literal; both are built at
compile time. A `String` literal passed to a `StringName` parameter is often converted at
compile time too, so the literal mostly pays for variables, dictionary keys, `match`
patterns and names built at run time. A `StringName` is slow to create and fast to compare.
Calling `String` methods on one converts it first; the class reference calls that highly
inefficient.

### Cache node lookups

The docs rank them: `get_node("Child")` with a string is slow, `$Child` is faster (cached
path), an `@onready` variable or an `@export var child: Node` is fastest. `%Name` lookups
are cached by the scene owner. `find_child` is slow. `@onready` with `@export` on one
variable is an error by default (`onready_with_export`): the default overwrites the value
from the scene.

### Coroutines

`await` on a signal or a coroutine saves the function state and returns to the caller. The
caller must `await` too if it needs the result. `await` on a plain value returns at once.
Turn on `missing_await` (since 4.6, off by default) to catch a coroutine called without
`await`. A
function that awaits a signal that never fires never resumes: give long waits a timeout.

### Properties with setters

Every assignment from outside runs the setter as a function call. Inside its own setter or
getter the name reaches the raw member, but not in a helper the setter calls (that
recurses). Initializers bypass the setter, also with `@export` and `@onready`. For per-frame
data use a plain typed member plus one explicit update function.

### Statics

`static var` belongs to the script, keeps it loaded and survives scene changes: a cheap
registry or cache without an autoload. `static func _static_init()` runs once when the
script loads. `@static_unload` is meant to let such a script unload, but the 4.7 docs say
scripts are never freed at present because of a bug. Clear big static data by hand.

### Inspector surface without a plugin

```gdscript
@tool
extends Node3D

@export_custom(PROPERTY_HINT_NONE, "suffix:m") var altitude: float = 0.0
@export_storage var baked_seed: int = 0
@export_tool_button("Rebuild", "Callable") var rebuild_action = _rebuild

func _rebuild() -> void:
	baked_seed = randi()
	notify_property_list_changed()
```

`@export_tool_button` (since 4.4) gives designers a one-click action; the callable can be a
method, a lambda or a bound call such as `print.bind("x")`. `@export_storage` saves a value
and copies it on `duplicate` without showing it. `@export var v: Variant` (since 4.5) shows a
type selector, so one slot can hold a number, a string or a color. `@export_custom` passes any
hint, hint string and usage flags with no validation. `@export_file` stores a UID when one
exists; `@export_file_path` keeps the raw path. For properties that depend on other values,
override `_get_property_list` (add entries) and `_validate_property` (change or hide
existing ones) in a `@tool` script and call `notify_property_list_changed`.

### Warnings as a build gate

Each warning is a project setting under `debug/gdscript/warnings/` with 3 levels: ignore,
warn, error. For a typed codebase raise `untyped_declaration`, `unsafe_method_access`,
`unsafe_property_access`, `unsafe_cast` and `unsafe_call_argument` (all off by default).
`return_value_discarded` (off) catches ignored `Error` results. `directory_rules` (since
4.6; it replaced the `exclude_addons` switch) excludes `res://addons` by default and takes
one include or exclude rule per folder, so you can gate your own addon and skip vendored
ones. Silence one line with `@warning_ignore("name")` and a region with
`@warning_ignore_start` and `@warning_ignore_restore` (both since 4.4).

### Abstract classes and variadic functions (since 4.5)

`@abstract` on a class forbids `new()`; on a function it forces subclasses to implement it.
The check happens at load time, not when the missing method is first called. An abstract
script cannot be attached to a node. A rest parameter (`...args: Array`) collects extra
arguments in a new array on every call and cannot be a typed array. Spread syntax does not
exist; forward with `Callable.callv`.

## Niche but useful

- `match` tests patterns from top to bottom and is stricter than `==` (`1` does not match
  `1.0`). Put the common case first.
- `as` returns `null` on a failed object cast; `var x: T = value` fails loudly. The docs
  call the second form more reliable even though the editor shows it as an unsafe line.
- Custom iterators (`_iter_init`, `_iter_next`, `_iter_get`) cost script calls per element.
- Lambdas can be named (`func my_name():`); the name shows in the debugger.
- `##` comments feed the built-in help and inspector tooltips. `@deprecated` and
  `@experimental` tags mark API for teammates.
- Inner classes are reached as `Outer.Inner` and add no global name; `class_name` does.
- `print` flushes standard output on every call in debug builds, only at exit in release
  (`application/run/flush_stdout_on_print` changes that). Do not time code that prints.
- Format with `%` or `String.format` outside hot loops; `+` needs `str()` per value.
- `Expression` runs designer formulas at run time; parse once, execute many times.
- Since 4.6 a custom engine build can feed script functions to a tracing profiler (Tracy;
  the release page also names Perfetto and Instruments). It shows single slow calls on a
  timeline, which the built-in profiler averages away. See `debugging-and-profiling-tools`.
- `Array.duplicate_deep` and `Dictionary.duplicate_deep` (since 4.5) state exactly what a
  deep copy clones. Plain `duplicate(true)` does not copy resources nested in containers.

## Traps

- Outdated: "typing is only for readability". It changes the opcodes.
- Outdated: "always write `&\"action\"` for speed". Constant strings are often converted at
  compile time already.
- Timing in the editor and reporting it as shipped speed.
- A typed array variable does not make `pop_back()` typed.
- `native_method_override` and `get_node_default_without_onready` are errors by default; do
  not lower them.
- Members initialize top to bottom before `_init`; exported values arrive after `_init`.

## Version notes

- 4.4: typed dictionaries, `@export_tool_button`, `@warning_ignore_start` and
  `@warning_ignore_restore`, UID storage for `@export_file`, faster `StringName` dictionary
  keys (4.4 dev 2 notes).
- 4.5: `@abstract`, variadic functions, script backtraces with
  `debug/settings/gdscript/always_track_call_stacks`, `@export_file_path`, `Variant`
  exports, `duplicate_deep`.
- 4.6: `missing_await` warning, `directory_rules`, tracing profiler support, fewer copies
  when typed arrays and dictionaries are constructed (4.6 dev 1 notes).
- 4.7: writing one element of a packed array property no longer calls that property's
  setter. An override of a method with a typed return now inherits the return type and needs
  an explicit `return`.

## Related skills

- `godot-gdscript-advanced` - idioms, metaprogramming and `@tool` recipes.
- `godot-gdscript-patterns` - structure of typed gameplay code.
- `godot-optimization` - the wider profiling and tuning pass.
- `godot-language-choice` - when to leave GDScript for C# or C++.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_basics.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/static_typing.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_exports.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/warning_system.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/gdscript_advanced.html
- https://docs.godotengine.org/en/stable/tutorials/best_practices/data_preferences.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/logging.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html
- https://godotengine.org/article/gdscript-progress-report-typed-instructions/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-2/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-3/
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-1/
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-6/
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/priorities/
- https://github.com/godotengine/godot/blob/4.7-stable/modules/gdscript/gdscript_function.h
- https://github.com/godotengine/godot/pull/70838
