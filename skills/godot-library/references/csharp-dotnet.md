# C# and .NET

Target engine: Godot 4.7. This shelf covers how C# code meets the engine: the interop
boundary, what it costs, the source generators, async, platform limits and mixed projects.

## Use this shelf when

- A C# system is slower than expected, or stutters, and you must find out why.
- You decide where the line between C# and GDScript goes in a mixed project.
- You plan an export to Android, iOS or Web for a project that holds C# code.
- A build fails with a `GD0xxx` diagnostic.
- You write async code that waits for signals, timers or tasks.

## How the engine works here

- C# runs on the modern .NET runtime, in a separate .NET build of the editor. The engine
  ships what is needed to run compiled code; the .NET SDK (8 or later) is a
  separate install. Android export needs .NET 9 or later.
- Every member of a type derived from `GodotObject` is a native interop call into C++. A
  read of `Position` and a write of `Position` are 2 calls, and the write also does the
  engine work (for a `Node3D`, a transform update).
- Math types (`Vector2`, `Vector3`, `Basis`, `Transform3D`, `Quaternion`, `Color`, `Aabb`)
  are C# structs. Math on them is plain managed code and crosses nothing.
- Marshalling is the copy and convert step at the boundary. `string` (UTF-16 in .NET,
  UTF-32 in the engine) and raw arrays such as `byte[]` are converted on each call. An
  implicit `string` to `StringName` or `NodePath` conversion pays interop plus marshalling.
- `Godot.Collections.Array` and `Godot.Collections.Dictionary` wrap C++ containers. Each
  element access is an interop call. Packed arrays have no wrapper: they map to C# arrays
  (`PackedVector3Array` is `Vector3[]`) and are copied when they cross.
- Roslyn source generators run at build time. They need the class to be `partial`, and they
  produce the nested `PropertyName`, `MethodName` and `SignalName` classes, the C# events
  for signals and the export metadata. The editor sees new exports and signals only after
  a build.
- Managed objects are garbage collected. The docs name GC pauses as a cause of stutter in
  complex projects and object pooling as the usual answer.
- The docs put GDScript and C# in the same order of magnitude. C# wins when most time is
  spent in its own code; it can lose when most time is spent in engine API calls.

## Advanced techniques

### Read once, compute locally, write once

Copy an engine property to a local, do all the math on the struct, assign once at the end.
The docs example turns 10 reads and 10 writes of `Position` into 1 and 1. Structs are
copied on read, so `Position.X = 1` does not compile (CS1612); use a local or
`Position = Position with { X = 1 }`.

### Use the generated names, cache your own

Pass `Node.MethodName.AddChild`, `Timer.SignalName.Timeout` or `PropertyName.Position`
where an API takes a `StringName`. They are cached, and they hide the snake_case rule:
`Call`, `CallDeferred`, `Get`, `Set` and `Connect` want engine names such as `add_child`,
not `AddChild`. Names of your own C# members are used as written. For action names, groups
and node paths, keep `static readonly StringName` and `NodePath` fields instead of string
literals in `_Process`.

### Keep hot data in .NET types

Use `List<T>`, `Dictionary<TKey, TValue>`, arrays and spans for data the engine never sees.
Convert once at the boundary. Godot collections are for exports, signal arguments and API
calls. LINQ over a Godot collection marshals every element on every pass; the collection's
own instance methods (`Sort`, `Reverse`) are one interop call. `ToList` and `ToArray` copy
and marshal the whole collection.

### Cut allocations in per-frame code

Sources of garbage: string concatenation (use `StringBuilder`), lambdas that capture,
LINQ, new arrays passed to the engine each frame, and `Variant.Obj`, which boxes value
types. Reuse buffers and pool objects. Measure with a managed profiler (dotTrace, dotMemory,
Visual Studio); the docs note that profiling managed and native code together works only on
Windows.

### Variant discipline

`Godot.Variant` is a struct and cannot be null; `default` is the nil value. It stores
integers and floats as 64-bit, so an `int` or `float` round trip can lose precision if the
wrong type is asked for. Enums have no implicit conversion: cast to the integer type or use
`Variant.From<T>` and `Variant.As<T>`. A failed conversion may return a default, a similar
value (`"42a"` becomes `42`) or throw, depending on the types. For generic code, mark the
type parameter with `[MustBeVariant]`.

### Signals as events, and when they do not disconnect

Declare a signal with `[Signal]` on a delegate whose name ends in `EventHandler`; emit with
`EmitSignal(SignalName.X, ...)`, never with `Invoke`. Arguments must be Variant-compatible,
so a plain C# class cannot be a signal argument. Freed objects normally disconnect, with 2
exceptions on the receiver side: a lambda that captures a variable, and a custom signal
connected with `+=`. Both keep calling into a freed object and raise
`ObjectDisposedException`. Fix: keep the delegate and use `-=` in `_ExitTree`, or use
`Connect`, which disconnects automatically for custom signals. `Connect` is also the only
way to pass `ConnectFlags` or to reach a signal that a GDScript declares.

### Async with signals

`await ToSignal(source, SignalName.X)` suspends until the signal fires; this is the C# form
of the GDScript `await`. Use it for timers, animation ends and frame waits. It works with
`Task` code, so one method can await a signal and then a task. A node can be freed while a
method is suspended: check `IsInstanceValid(this)` after each await. Where a continuation
resumes after a background task is not stated in the 4.7 docs; hand results to the scene
with `CallDeferred` (see the `threads-and-concurrency` shelf).

### Mixed projects: one direction, coarse calls

From GDScript, a C# script loads and instantiates like any script, and its fields and
methods are reached directly. Arguments are converted where possible; a mismatch reports a
nonexistent function. From C#, a GDScript object is reached only through `Call`, `Get`,
`Set` and `Connect` with string names, so nothing is checked at compile time. Neither
language can inherit from a script of the other. A `Callable` made with `bind` or `unbind`,
or by a GDExtension, is not supported on the C# side; `Bind` does not exist there, so
capture with a lambda. Rule: let GDScript call into C# systems through a few coarse
methods, and pass built-in types or `GodotObject` types only.

### Global classes and tool code

`[GlobalClass]` registers a type for the Add Node and Create Resource dialogs and narrows
exported slots to that type; `[Icon]` sets its icon. The file name must match the class
name, case included. A global class may not be generic. Names that start with `Editor` are
hidden from the dialogs. `[Tool]` code needs a rebuild to change. Hot reload keeps only
exported values. `[ExportToolButton]` (since 4.4) needs a `[Tool]` class and an
expression-bodied `Callable` property.

### Compile-time switches

The SDK defines symbols for `#if`: `GODOT`, `TOOLS` (Debug configuration), one platform
symbol (`GODOT_WINDOWS`, `GODOT_LINUXBSD`, `GODOT_MACOS`, `GODOT_ANDROID`, `GODOT_IOS`,
`GODOT_WEB`), a class symbol on export (`GODOT_PC`, `GODOT_MOBILE`, `GODOT_WEB`), version
symbols such as `GODOT4_5_OR_GREATER`, and `GODOT_REAL_T_IS_DOUBLE` for double-precision
builds. Use them to keep one library working across engine versions and platforms.

## Platform state in 4.7

| Target | State in the 4.7 docs |
| --- | --- |
| Windows, Linux, macOS | Supported |
| Android | Experimental; .NET 9 or later |
| iOS | Experimental; export only from macOS; simulator templates are x64 only |
| Web | Not possible for C# projects |

The 4.7 docs say nothing on NativeAOT or trimming. The iOS path is reported to compile
ahead of time, which limits reflection-heavy libraries (reported by the Godot blog post on
C# platform state for 4.2; not read in this session; test on a device). A Web prototype was
shown in 2025 (reported by search results on a Godot blog post; not in 4.7).

## Diagnostics map

| Range | Topic | Typical cause |
| --- | --- | --- |
| GD0001 to GD0003 | Class shape | Missing `partial`; 2 classes with one name in a file |
| GD0101 to GD0107 | Exports | Static, read-only, write-only or unsupported type; a `Node` export in a non-`Node` type |
| GD0108 to GD0111 | Tool buttons | Not a `[Tool]` class; mixed with `[Export]`; not a `Callable` |
| GD0201 to GD0203 | Signals | Delegate name, parameter type, non-void return |
| GD0301 to GD0303 | Generics | Type argument is not Variant-compatible |
| GD0401, GD0402 | Global classes | Not a `GodotObject`; generic class |

## Niche but useful

- `new Basis()`, `new Transform3D()` and `new Quaternion()` are all zeros, not identity. Use
  `Basis.Identity` and the like.
- `Vector2.Right` is a read-only property, not a constant: it cannot be a `switch` label.
- Singletons are static classes. `Input.Singleton` gives the instance when a `GodotObject`
  member such as `Connect` is needed.
- An export default is read by source analysis, not by running the getter. A computed
  default may not show in the inspector.
- `GetNodeOrNull<T>` returns null on a type mismatch; `GetNode<T>` throws.
- NuGet packages work through the `.csproj`. Commit the `.sln` and `.csproj`; ignore
  `.godot`. Deleting `.godot/mono` fixes many stale-build problems.
- The docs suggest `System.Random` over `GD.Randf` and friends for C#-side randomness.

## Traps

- The built-in script editor has no C# completion; use an external IDE.
- Editor plugins in C# are possible but awkward; C# `EditorScript` cannot run from the
  script editor.
- `AddUserSignal` signals do not appear in `SignalName`.
- Outdated: "C# is always faster". Chatty engine calls can make it slower than GDScript.
- Outdated: "Godot 4 C# exports to Web". It does not in 4.7; Godot 3 did.

## Version notes

- 4.4: `[ExportToolButton]`.
- 4.5: Android export needs .NET 9 or later; other platforms stay on .NET 8 or later.
- 4.6: some `AnimationPlayer` properties became `StringName` (source break in C#).
- 4.7: `Animation.length` is `double` in C#. The SDK accepts `EnableGodotDotNetPreview` to
  opt in to prerelease packages of the new .NET bindings (GitHub PR 118001; not in the
  docs; keep it off for production).

## Related skills

- `godot-csharp-godot` - setup, conventions and the day-to-day API differences.
- `godot-csharp-signals` - signal declaration and connection procedures.
- `godot-language-choice` - choosing C# or GDScript per system, and the boundary rules.
- `godot-export-pipeline` - export presets and per-platform builds.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_basics.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_features.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_differences.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_collections.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_variant.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_signals.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_exports.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_global_classes.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/diagnostics/index.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/cross_language_scripting.html
- https://docs.godotengine.org/en/stable/about/faq.html
- https://github.com/godotengine/godot/pull/118001
