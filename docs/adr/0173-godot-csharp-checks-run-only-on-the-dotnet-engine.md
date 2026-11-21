# Godot C# checks run only on the .NET engine

A Godot C# project is built and run only by the Godot .NET engine.
`skills/godot-build-verify/scripts/godot-run.sh` `find_engine` gives a C#
project (C# in `config/features` or a `*.csproj`) only the .NET engine:
`GODOT_MONO_PATH`, then the macOS Godot_mono app. It never falls back to
the standard engine, which cannot build or run C#. Any other project gets
`GODOT_PATH`, then `godot` on PATH, then the macOS app.
`scripts/godot/live_csharp.sh` checks C# tests on that engine.

## Consequences

- A second project runs C# tests through the `dotnet test` adapter of
  gdUnit4Net. It pins api 5.0.0 with adapter 3.0.0. gdUnit4Net states
  Godot 4.3 and 4.4 only and names GodotSharp 4.4.0. This run is the check
  on Godot .NET 4.7.2. These are the newest stable packages that resolve
  together: adapter 3.0.0 needs api 5.0.0, and adapter 3.1.x needs the
  prerelease api 5.1.0-rc5.
- The adapter rebuilds the Godot project before each run and kills the
  rebuild at its `CompileProcessTimeout`, a short default. Under CPU load
  that ends the run with exit 137. `live_csharp.sh` writes a `.runsettings`
  with `CompileProcessTimeout` 300000 ms (5 minutes), the documented key.
- The test host targets net8.0, and the host may have only the .NET 10
  runtime. `live_csharp.sh` exports `DOTNET_ROLL_FORWARD=Major`, without
  which the test host does not start.
- `scripts/godot/csharp_blocks_check.py` runs up to `MAX_PASSES` builds.
  csc stops at the first stage with errors (syntax, then declarations, then
  method bodies), so 1 build cannot report every code form. Each pass
  removes the forms that got errors, and the next pass goes deeper.
