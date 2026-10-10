# Evidence for godot-language-choice

Every rule in `SKILL.md` and its source. Access date for every web source:
2026-10-07. Doc pages are the 4.7 branch (`stable` = 4.7 on that date).
"Source-derived" means read from Godot source at tag `4.7.2-stable`.
"Third-party" marks a source that is not the Godot project.

## Release

| Fact | Source |
|------|--------|
| Godot 4.7.2-stable is the latest stable release (2026-08-18). The .NET build ships in the same release. | https://github.com/godotengine/godot/releases/tag/4.7.2-stable |

## C# platform support in 4.7.2

| Platform | Status | Source |
|----------|--------|--------|
| Windows, macOS, Linux | Supported | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/index.html#c-platform-support |
| Android | Experimental; needs .NET 9 or later | https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_android.html |
| iOS | Experimental; NativeAOT; export only from macOS | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/index.html#c-platform-support |
| Web | Not supported: "Projects written in C# using Godot 4 currently cannot be exported to the web." The 4.7.2 export plugin throws "Target platform not yet implemented." for any platform other than Windows, Linux/BSD, macOS, Android and iOS. | https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html ; source-derived: modules/mono/editor/GodotTools/GodotTools/Export/ExportPlugin.cs line 182 |

## .NET

| Fact | Source |
|------|--------|
| .NET SDK 8 or later; Android export needs .NET 9 or later. | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_basics.html#prerequisites |
| A C# project needs the .NET edition of the editor and the .NET SDK. | same page; https://docs.godotengine.org/en/stable/getting_started/step_by_step/scripting_languages.html |
| The first C# script makes Godot generate `.sln` and `.csproj`. | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_basics.html#project-setup-and-workflow |
| Exports are self-contained; players need no installed .NET. | source-derived: modules/mono/mono_gd/gd_mono.cpp |
| Run: Godot .NET 4.7.2 with .NET SDK 10.0.101 built a C# project and ran a C# scene and signal headless (macOS arm64, 2026-10-07). | `docs/live-checks/godot-csharp.md` |

## Performance

| Fact | Source |
|------|--------|
| "The C# language itself tends to be faster than GDScript ... in situations with few calls to Godot engine code." "C# can be slower than GDScript when making many Godot API calls, due to the cost of marshalling." Garbage collection "occurs at random and unpredictable moments". | https://docs.godotengine.org/en/stable/about/faq.html#which-programming-language-is-fastest |
| "In many cases, writing gameplay logic in GDScript, C#, or C++ won't have a significant impact on performance." | https://docs.godotengine.org/en/stable/getting_started/step_by_step/scripting_languages.html |
| Godot 4.2 benchmark on export templates: loop of 10^7 iterations, GDScript 117 ms, C# 2 ms. C# `Call()` into GDScript 10^7 times: 6456 ms, GDScript to GDScript: 498 ms. Large Godot `Dictionary` iteration: GDScript 47 ms, C# 202 ms. Node creation 10^4: GDScript 13.2 ms, C# 21.8 ms. Old engine version; GDScript has had optimizations since. | https://github.com/dicarne/godot-benchmark-gdscript-csharp (third-party, 2023) |

## Workflow

| Fact | Source |
|------|--------|
| C#: "You need to (re)build the project assemblies whenever you want to see new exported variables or signals in the editor." | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_basics.html#general-differences-between-c-and-gdscript |
| C# hot reload: "State is currently not saved and restored when hot-reloading, with the exception of exported variables." | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_basics.html#current-gotchas-and-known-issues |
| C# editor plugins are possible "but it is currently quite convoluted"; tool scripts need a rebuild to apply changes. | same page; https://docs.godotengine.org/en/stable/tutorials/plugins/editor/making_plugins.html |
| GUT tests GDScript in GDScript. gdUnit4 tests GDScript and C# (C# through gdUnit4Net). | https://github.com/bitwes/Gut ; https://github.com/godot-gdunit-labs/gdUnit4 |
| NuGet packages work as in any C# project. | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_basics.html#using-nuget-packages-in-godot |
| GDScript typing is gradual: dynamic by default, type hints optional. | https://docs.godotengine.org/en/stable/tutorials/scripting/gdscript/static_typing.html |

## Mixed projects

| Fact | Source |
|------|--------|
| A project can define nodes in both C# and GDScript. | https://docs.godotengine.org/en/stable/tutorials/scripting/cross_language_scripting.html |
| GDScript accesses C# fields and methods directly. | same page, "Accessing C# fields from GDScript" |
| C# accesses GDScript only through `Get`, `Set`, `Call` and `Connect` by name, with types GDScript knows. | same page, "Accessing GDScript fields from C#" |
| C# connects to a GDScript signal only by name, "because no C# static types exist for signals defined by GDScript". | same page, "Connecting to GDScript signals from C#" |
| No inheritance across languages; "this limitation is unlikely to be lifted". | same page, "Inheritance"; https://github.com/godotengine/godot/issues/38352 |
| `[GlobalClass]` registers a C# type by name in the editor. | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_global_classes.html |
| A C# `Godot.Collections.Array` passed to a GDScript `Array[CustomClass]` parameter fails; an untyped `Array` works (Godot 4.6). | https://forum.godotengine.org/t/c-to-gdscript-typed-array-interop/135652 (third-party, 2026-03-18). Run on 4.7.2 (2026-10-07): `Array[int]` rejected a plain C# `Array` with "does not have the same element type as the expected typed array argument"; an untyped `Array` worked. `docs/live-checks/godot-csharp.md`, steps `mixed-untyped-array` and `mixed-typed-array-rejected`. |
| Name-based calls use the snake_case engine names; prefer the generated `MethodName`, `PropertyName`, `SignalName`. | https://docs.godotengine.org/en/stable/tutorials/scripting/c_sharp/c_sharp_basics.html#current-gotchas-and-known-issues |

## Not sourced

- Exported game size and start time, C# against GDScript, measured.
- A paired C# and GDScript benchmark on a Godot version newer than 4.2.
