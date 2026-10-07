# Live check: Godot C#

A C# scene and a C# signal run headless on Godot .NET 4.7. Until this check
passes, every C# block in the Godot skills stays at `proof: parse-checked
only` (`skills/README.md`, Godot section). Item U-24.

## How to run

```bash
scripts/godot/live_csharp.sh
```

It needs Godot .NET 4.7 (`GODOT_MONO_PATH`, else
`/Applications/Godot_mono.app/Contents/MacOS/Godot`) and `dotnet` on PATH. A
missing tool is `SKIP`. It starts no model session, so it costs no tokens.

It writes a small project under `.worktrees/_scratch/live-godot-csharp/`,
runs `dotnet build`, imports the project, runs it headless, and removes the
build output at the end. It sets `HOME` and `NUGET_PACKAGES` to that folder.

Steps: `godot-dotnet`, `dotnet-build`, `csharp-scene`, `csharp-signal`,
`csharp-errors`, `mixed-gd-calls-cs`, `mixed-cs-signal-to-gd`,
`mixed-cs-calls-gd`, `mixed-gd-signal-to-cs`, `mixed-untyped-array`,
`mixed-errors`, `mixed-typed-array-rejected`. The `mixed-*` steps use the 3
code blocks of step 5 of `skills/godot-language-choice/SKILL.md` unchanged.

## 2026-10-06

| Step | Claim it proves | Tool version | Result | Evidence |
|------|-----------------|--------------|--------|----------|
| godot-dotnet | A Godot .NET 4.7 build is installed. | none | SKIP | This host has no Godot .NET. `/Applications/Godot_mono.app` is missing and `GODOT_MONO_PATH` is not set. No C# claim changed. |

The operator runs `scripts/godot/live_csharp.sh` on a host that has Godot
.NET 4.7 and `dotnet`, then appends the table of that run to this file.

## 2026-10-07

| Step | Claim it proves | Tool version | Result | Evidence |
|------|-----------------|--------------|--------|----------|
| godot-dotnet | A Godot .NET 4.7 build is installed. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `PASS godot-dotnet 4.7.2.stable.mono.official.ed1daf0bf` |
| dotnet-build | The C# project builds with `Godot.NET.Sdk/4.7.2`. | dotnet 10.0.101 | PASS | `dotnet build` exit 0. |
| csharp-scene | A C# scene runs headless. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `LIVE_CS_SCENE` line in the run log. |
| csharp-signal | A C# signal fires and the handler runs. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `LIVE_CS_SIGNAL 7` line in the run log. |
| csharp-errors | The run log has no script errors. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | No `SCRIPT ERROR` or `No loader found` line in the run log. The first run printed nothing for this step; the script now prints `PASS csharp-errors`, and a rerun printed it. |

Host: macOS 26.5.1, arm64, Godot .NET 4.7.2.stable.mono.official.ed1daf0bf,
dotnet SDK 10.0.101. SHA-512 of `Godot_v4.7.2-stable_mono_macos.universal.zip`
matched its line in `SHA512-SUMS.txt`. `codesign --verify --deep --strict`
passed, and `spctl --assess --type execute` accepted the app (Notarized
Developer ID, Prehensile Tales B.V.).

## 2026-10-07 (mixed GDScript and C#)

The script ran twice and exited 0 both times with the same result. The scene
`mixed.tscn` runs with `--quit-after 10` and its own log. The scene
`typed.tscn` has its own log, so its expected error does not fail
`mixed-errors`.

| Step | Claim it proves | Tool version | Result | Evidence |
|------|-----------------|--------------|--------|----------|
| mixed-gd-calls-cs | GDScript calls a C# method by its C# name (`FindPath`), and a C# `Vector2[]` arrives as a `PackedVector2Array`. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `LIVE_MIX_GD_CALLS_CS 2` |
| mixed-cs-signal-to-gd | A GDScript handler connected to a C# signal (`paths.PathReady.connect`) runs. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `LIVE_MIX_CS_SIGNAL_TO_GD 2` |
| mixed-cs-calls-gd | C# calls a GDScript method by its snake_case name (`Call("count_items")`). | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `LIVE_MIX_CS_CALLS_GD 3` |
| mixed-gd-signal-to-cs | C# connects to a GDScript signal by name (`Connect("item_added", Callable.From<string>(...))`), and the handler runs. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `LIVE_MIX_GD_SIGNAL_TO_CS sword` and the bare `sword` line of `Hud.OnItemAdded`. |
| mixed-untyped-array | A plain `Godot.Collections.Array` from C# reaches an untyped `Array` GDScript parameter. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `LIVE_MIX_UNTYPED_ARRAY 2` |
| mixed-errors | The mixed run log has no script error. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | No `SCRIPT ERROR` or `ERROR:` line in `mixed.log`. |
| mixed-typed-array-rejected | A typed GDScript parameter `Array[int]` rejects a plain `Godot.Collections.Array` from C#. | Godot 4.7.2.stable.mono.official.ed1daf0bf | PASS | `ERROR: Invalid type in function 'take' in base 'Godot.Node'. The array of argument 2 (Array) does not have the same element type as the expected typed array argument.` |

Not proved by this run: `Array[Item]` with a custom class (only `Array[int]`
ran); the type name `PathService` in GDScript (the block types the variable
`Node`); the `MethodName`, `PropertyName` and `SignalName` constants; the
rule that no class extends across languages; and the speed figures (third
party, Godot 4.2). The `Hud.cs` marker for the signal is a lambda, because the
block's `OnItemAdded` is one line that the proof does not change; the bare
`sword` line proves that `OnItemAdded` ran.
