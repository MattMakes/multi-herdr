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
`csharp-errors`.

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
