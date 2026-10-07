#!/usr/bin/env bash
# Live check: C# runs on Godot .NET (U-24). The gate never runs this script.
#
# The test host has no Godot .NET, so the Godot skills label every C# line
# "proof: parse-checked only" (skills/README.md, Godot section). On a host
# with Godot .NET 4.7, this script proves that a C# scene and a C# signal run
# headless. It prints one line per step: PASS <step>, FAIL <step>: <reason>
# or SKIP <step>: <reason>, and exits 1 when a step fails. A missing tool is
# SKIP. It writes only under .worktrees/_scratch/live-godot-csharp/ and
# removes the build output at the end.
#
# Godot .NET: GODOT_MONO_PATH, else /Applications/Godot_mono.app.
# Run: scripts/godot/live_csharp.sh
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
work="$repo/.worktrees/_scratch/live-godot-csharp"
godot=${GODOT_MONO_PATH-/Applications/Godot_mono.app/Contents/MacOS/Godot}
failed=0

pass() { echo "PASS $1"; }
fail() { echo "FAIL $1: $2"; failed=1; }
skip() { echo "SKIP $1: $2"; }

# Runs a command with a time bound (macOS has no `timeout`).
bounded() { local s=$1; shift; perl -e 'alarm shift; exec @ARGV' "$s" "$@"; }

if [ -z "$godot" ] || [ ! -x "$godot" ]; then
  skip godot-dotnet "no Godot .NET at '${godot}' (set GODOT_MONO_PATH)"
  exit 0
fi
if ! command -v dotnet >/dev/null 2>&1; then
  skip dotnet "no dotnet on PATH"
  exit 0
fi

version=$("$godot" --headless --version 2>/dev/null | tail -1)
case "$version" in
  4.7.*mono*) pass "godot-dotnet $version" ;;
  *) fail godot-dotnet "want a 4.7 mono build, got '$version'"; exit 1 ;;
esac
sdk=$(echo "$version" | cut -d. -f1-3)

rm -rf "$work/project" "$work/home"
mkdir -p "$work/project" "$work/home" "$work/nuget"
cd "$work/project"
cat > project.godot <<'EOF'
config_version=5

[application]
config/name="LiveCs"
run/main_scene="res://main.tscn"
config/features=PackedStringArray("4.7", "C#")

[dotnet]
project/assembly_name="LiveCs"
EOF
cat > LiveCs.csproj <<EOF
<Project Sdk="Godot.NET.Sdk/$sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <EnableDynamicLoading>true</EnableDynamicLoading>
  </PropertyGroup>
</Project>
EOF
cat > Main.cs <<'EOF'
using Godot;

public partial class Main : Node
{
    [Signal]
    public delegate void PingEventHandler(int value);

    public override void _Ready()
    {
        int got = 0;
        Ping += value => got = value;
        EmitSignal(SignalName.Ping, 7);
        GD.Print($"LIVE_CS_SCENE {Engine.GetVersionInfo()["string"]}");
        GD.Print($"LIVE_CS_SIGNAL {got}");
        GetTree().Quit(got == 7 ? 0 : 1);
    }
}
EOF
cat > main.tscn <<'EOF'
[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://Main.cs" id="1"]

[node name="Main" type="Node"]
script = ExtResource("1")
EOF

export HOME="$work/home" NUGET_PACKAGES="$work/nuget"
if bounded 600 dotnet build > "$work/build.log" 2>&1; then
  pass "dotnet-build"
else
  fail "dotnet-build" "see $work/build.log"
fi

bounded 300 "$godot" --headless --path . --import > "$work/import.log" 2>&1 || true
if bounded 120 "$godot" --headless --path . > "$work/run.log" 2>&1; then
  grep -q '^LIVE_CS_SCENE ' "$work/run.log" && pass "csharp-scene" || fail "csharp-scene" "no LIVE_CS_SCENE line"
  grep -q '^LIVE_CS_SIGNAL 7$' "$work/run.log" && pass "csharp-signal" || fail "csharp-signal" "no LIVE_CS_SIGNAL 7 line"
else
  fail "csharp-scene" "exit $? (see $work/run.log)"
fi
if grep -q 'SCRIPT ERROR\|No loader found' "$work/run.log"; then
  fail "csharp-errors" "error lines in $work/run.log"
fi

rm -rf "$work/project/.godot" "$work/project/bin" "$work/project/obj"
exit "$failed"
