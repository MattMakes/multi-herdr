---
name: godot-language-choice
description: "Use when a Godot 4 project or one of its systems must pick GDScript or C# - a new project, a new system (pathfinding, procedural generation, simulation, AI search, save data, a .NET library), a slow script, a Web or mobile export target, or a call across a GDScript and C# boundary. Gives the default, the hard limits of C# in Godot 4.7, the rule per part of a game, the boundary rules for a mixed project, and when the choice is the orchestrator's to make. Targets Godot 4.7.2."
---

# Godot Language Choice

The fleet writes Godot games in GDScript and in C#. Each language goes where it
is best. This skill gives the rule, the reason for each part of it, and the
limits that make some choices impossible. Every fact has a source in
`references/evidence.md`.

## The default

**A new project is GDScript only, on the standard Godot editor.** It exports to
every platform including Web, it needs no .NET editor and no .NET SDK, it has
no build step, and the official docs recommend it for most gameplay. C# comes
in only when a row of the table below names it.

The reason is cost. The first C# file changes the whole project, not 1 system:

- every person and every worker must use the Godot .NET editor and a .NET SDK;
- Godot adds a `.sln` and a `.csproj`, and every change needs `dotnet build`
  before the editor sees new exports and signals;
- the project can no longer export to Web;
- Android and iOS exports become experimental;
- the .NET garbage collector can pause a frame at unpredictable moments.

So a C# system must earn more than it costs.

## Step 1: Read the project first

Read `.agents/godot-project-context.md` (`godot-project-context`). The fields
that decide this skill are the language, the target platforms and the
decision records. A project that already has C# is a `"C#"` entry in
`config/features` of `project.godot`, or a `*.csproj` in the root.

Check: you can name the current language and every export target, or you
wrote `[unknown]` for the field you could not prove.

## Step 2: Apply the hard limits

These limits come from the engine, not from taste. Check them before you
weigh anything else. A team's preference for a language does not change them.

| Target | Rule |
|--------|------|
| Web export | No C# anywhere in the project. Any C# blocks Web export in 4.7. |
| Android or iOS first | GDScript. C# on mobile is experimental in 4.7 (Android needs .NET 9+; iOS uses NativeAOT and exports only from macOS). Use C# there only with an orchestrator decision. |
| Desktop only (Windows, macOS, Linux) | Both languages are supported. Go to step 3. |
| `@tool` scripts and editor plugins | GDScript. A C# plugin needs a rebuild for every change, and the docs call C# plugins "quite convoluted". |

## Step 3: Choose per system

Decide for each system, not once for the whole game. The question is: does
this code spend its time inside its own loops, or in calls to the engine?

| Part of a game | Language | Why |
|----------------|----------|-----|
| Gameplay scripts, node behaviour, UI, scene glue, input | GDScript | Most of the work is engine calls, where C# gains nothing and pays marshalling. No build step, and hot reload keeps state. |
| Pure-compute systems: pathfinding on a custom graph, procedural generation, simulation ticks, AI search, large data processing | C# | Little engine traffic per item. The C# language is much faster in tight loops (third-party, Godot 4.2: a 10,000,000-iteration loop took 117 ms in GDScript and 2 ms in C#). |
| A system that needs a .NET library (a JSON schema validator, a networking or platform SDK, a database, crypto) | C# | NuGet works as in any C# project. |
| The core model of a large, long-lived game with several programmers | C# | Typing is enforced and IDE refactoring works across files. GDScript typing is optional per file. |
| Tests | Same as the code under test | GDScript code: GUT or gdUnit4. C# code: gdUnit4 (gdUnit4Net). GUT runs GDScript only. |
| Hot paths that touch many nodes or a big Godot `Dictionary` | GDScript | Every engine call from C# crosses the native boundary. Iterating a large Godot `Dictionary` was 4 times slower in C# (third-party, 4.2). |

Measure before you move a slow GDScript system to C#. Use the profiler
(`godot-optimization`) to show that the time is in the script's own loops.
If the time is in engine calls, C# will not help. If C# is not enough,
the next step is GDExtension (`godot-gdextension`), not more C#.

## Step 4: Who decides

- **The first C# file in a GDScript-only project is a project decision.** It
  changes the editor, the build and the export targets for everyone. A
  builder never adds it inside a task. Send the orchestrator 1 `QUESTION:`
  that names the system, the measured reason, and the cost list from "The
  default". Wait for the answer.
- **A new C# or GDScript system in a project that already uses both** follows
  the table in step 3. Write the choice and its reason in the system's
  decision record (`godot-grill`, else `docs/decisions/`).
- **Moving an existing system to the other language** is a decision record
  plus a measurement, for the same reason as the first C# file.

Check: every system you add has its language in a decision record, or the
orchestrator answered your `QUESTION:`.

## Step 5: Build the boundary in 1 direction

In a mixed project, C# systems serve and GDScript gameplay calls them.

- **C# exposes, GDScript calls.** Give the C# class `[GlobalClass]` so GDScript
  and the editor see its name. Expose coarse methods (1 call does a lot of
  work), signals, and exported properties. A GDScript call into C# is typed
  and cheap.
- **C# calls GDScript only by name.** C# has no static type for a GDScript
  class, so it uses `Call`, `Get`, `Set` and `Connect` with the snake_case
  name. Each such call is slow (third-party, 4.2: about 13 times slower than a
  GDScript-to-GDScript call) and a typo fails at run time. Keep it off hot
  paths, and prefer a signal that GDScript connects.
- **No inheritance across languages.** A GDScript file cannot extend a C#
  class, and a C# class cannot extend a GDScript file. Use composition: a
  child node, or a resource.
- **Use Godot types at the boundary.** Pass `Godot.Collections.Array`,
  `Godot.Collections.Dictionary`, packed arrays, built-in types and
  `GodotObject` subclasses. `System.Collections.Generic` types do not cross.
  A typed GDScript parameter such as `Array[Item]` rejects an array from C#;
  declare it as untyped `Array` on the GDScript side.
- **Names.** GDScript sees C# members by their C# names (`FindPath`). C#
  sees GDScript members by their snake_case names (`"count_items"`). In C#,
  prefer the generated `MethodName`, `PropertyName` and `SignalName`
  constants for C# members.

A C# service that GDScript calls:

```csharp
using Godot;

[GlobalClass]
public partial class PathService : Node
{
    [Signal]
    public delegate void PathReadyEventHandler(Vector2[] points);

    public Vector2[] FindPath(Vector2 from, Vector2 to)
    {
        // The heavy search runs here, inside C#.
        var points = new[] { from, to };
        EmitSignal(SignalName.PathReady, points);
        return points;
    }
}
```

The GDScript side. It calls the C# method by its C# name. A C# `Vector2[]`
arrives as a `PackedVector2Array`:

```gdscript
extends Node

@onready var paths: Node = $PathService


func _ready() -> void:
	paths.PathReady.connect(_on_path_ready)
	var points: PackedVector2Array = paths.FindPath(Vector2.ZERO, Vector2(64, 0))
	print(points.size())


func _on_path_ready(points: PackedVector2Array) -> void:
	print("path with ", points.size(), " points")
```

The variable is typed `Node` so that this block also parses on the standard
editor. In the .NET editor, `PathService` is a valid type name because of
`[GlobalClass]`.

When C# must listen to GDScript, connect by name:

```csharp
using Godot;

public partial class Hud : Control
{
    public override void _Ready()
    {
        Node inventory = GetNode("../Inventory"); // a GDScript node
        inventory.Connect("item_added", Callable.From<string>(OnItemAdded));
        int count = inventory.Call("count_items").AsInt32();
        GD.Print(count);
    }

    private void OnItemAdded(string id) => GD.Print(id);
}
```

(proof: on 2026-10-07 the 3 blocks above ran unchanged in 1 mixed project on
Godot .NET 4.7.2. GDScript called C# and got its signal, C# called GDScript
and got its signal, an untyped `Array` crossed, and a typed `Array[int]`
parameter rejected a plain C# `Array`; `docs/live-checks/godot-csharp.md`,
steps `mixed-*`. The rules on inheritance, `System.Collections.Generic` and
the generated name constants come from the docs and did not run.)

## Step 6: Build and verify each language

`godot-build-verify` runs the loop. For a project with C#:

- use the Godot .NET editor binary: `GODOT_MONO_PATH`, else
  `/Applications/Godot_mono.app/Contents/MacOS/Godot` on macOS;
- run `dotnet build` before the import and before the tests;
- run the GDScript parse check as well: a mixed project has both kinds of
  files.

(proof: on 2026-10-07, Godot .NET 4.7.2 with .NET SDK 10.0.101 built a C#
project and ran a C# scene and a C# signal headless on macOS arm64;
`docs/live-checks/godot-csharp.md`.)

## Report

When you made or proposed a language choice, put it in your `DONE:` or
`QUESTION:` line in 1 sentence: the system, the language, and the reason from
the table. Example:

`[godot-tech-lead] QUESTION: Add C# for the flow-field pathfinding? The profiler shows 9 ms per frame in its own loops. The cost: the .NET editor for every seat, a dotnet build step, and no Web export. The project targets Windows and Linux only.`

## Related skills

- `godot-project-context`: the project's language, platforms and decisions.
- `godot-grill`: the decision record.
- `godot-csharp-godot`, `godot-csharp-signals`: how to write the C# side.
- `godot-gdscript-patterns`: how to write the GDScript side.
- `godot-build-verify`: build, test and smoke-run both languages.
- `godot-optimization`: measure before you move code.
- `godot-gdextension`: native code when C# is not enough.
