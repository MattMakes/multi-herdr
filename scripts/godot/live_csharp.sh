#!/usr/bin/env bash
# Live check: C# runs on Godot .NET (U-24). The gate never runs this script.
#
# The test host has no Godot .NET, so the Godot skills label every C# line
# "proof: parse-checked only" (skills/README.md, Godot section). On a host
# with Godot .NET 4.7, this script proves that a C# scene and a C# signal run
# headless. It prints one line per step: PASS <step>, FAIL <step>: <reason>
# or SKIP <step>: <reason>, and exits 1 when a step fails. A missing tool is
# SKIP. It writes only under .worktrees/_scratch/live-godot-csharp/ and
# removes the build output at the end. NuGet needs the network.
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

# Mixed GDScript and C# project: the code blocks of step 5 of
# skills/godot-language-choice/SKILL.md, character for character. Only the
# marker prints are extra lines.
cat > PathService.cs <<'EOF'
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
EOF
cat > paths_user.gd <<'EOF'
extends Node

@onready var paths: Node = $PathService


func _ready() -> void:
	paths.PathReady.connect(_on_path_ready)
	var points: PackedVector2Array = paths.FindPath(Vector2.ZERO, Vector2(64, 0))
	print(points.size())
	print("LIVE_MIX_GD_CALLS_CS ", points.size())


func _on_path_ready(points: PackedVector2Array) -> void:
	print("path with ", points.size(), " points")
	print("LIVE_MIX_CS_SIGNAL_TO_GD ", points.size())
EOF
cat > inventory.gd <<'EOF'
extends Node

signal item_added(id: String)


func _ready() -> void:
	item_added.emit.call_deferred("sword")


func count_items() -> int:
	return 3


func update(items: Array) -> void:
	print("LIVE_MIX_UNTYPED_ARRAY ", items.size())
EOF
cat > Hud.cs <<'EOF'
using Godot;

public partial class Hud : Control
{
    public override void _Ready()
    {
        Node inventory = GetNode("../Inventory"); // a GDScript node
        inventory.Connect("item_added", Callable.From<string>(OnItemAdded));
        int count = inventory.Call("count_items").AsInt32();
        GD.Print(count);
        GD.Print($"LIVE_MIX_CS_CALLS_GD {count}");
        inventory.Connect("item_added", Callable.From<string>(id => GD.Print($"LIVE_MIX_GD_SIGNAL_TO_CS {id}")));
        inventory.Call("update", new Godot.Collections.Array { 1, 2 });
    }

    private void OnItemAdded(string id) => GD.Print(id);
}
EOF
cat > mixed.tscn <<'EOF'
[gd_scene load_steps=5 format=3]

[ext_resource type="Script" path="res://paths_user.gd" id="1"]
[ext_resource type="Script" path="res://PathService.cs" id="2"]
[ext_resource type="Script" path="res://inventory.gd" id="3"]
[ext_resource type="Script" path="res://Hud.cs" id="4"]

[node name="Mixed" type="Node"]
script = ExtResource("1")

[node name="PathService" type="Node" parent="."]
script = ExtResource("2")

[node name="Inventory" type="Node" parent="."]
script = ExtResource("3")

[node name="Hud" type="Control" parent="."]
script = ExtResource("4")
EOF

# A typed GDScript parameter rejects a plain Godot.Collections.Array from C#.
cat > typed_sink.gd <<'EOF'
extends Node


func take(items: Array[int]) -> void:
	print("LIVE_MIX_TYPED_TAKEN ", items.size())
EOF
cat > TypedCaller.cs <<'EOF'
using Godot;

public partial class TypedCaller : Node
{
    public override void _Ready()
    {
        GetNode("../TypedSink").Call("take", new Godot.Collections.Array { 1, 2 });
    }
}
EOF
cat > typed.tscn <<'EOF'
[gd_scene load_steps=3 format=3]

[ext_resource type="Script" path="res://typed_sink.gd" id="1"]
[ext_resource type="Script" path="res://TypedCaller.cs" id="2"]

[node name="Typed" type="Node"]

[node name="TypedSink" type="Node" parent="."]
script = ExtResource("1")

[node name="TypedCaller" type="Node" parent="."]
script = ExtResource("2")
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
else
  pass "csharp-errors"
fi

# Mixed scene: GDScript and C# call each other and connect each other's signals.
if bounded 120 "$godot" --headless --path . --quit-after 10 res://mixed.tscn > "$work/mixed.log" 2>&1; then
  grep -q '^LIVE_MIX_GD_CALLS_CS 2$' "$work/mixed.log" && pass "mixed-gd-calls-cs" || fail "mixed-gd-calls-cs" "no LIVE_MIX_GD_CALLS_CS 2 line"
  grep -q '^LIVE_MIX_CS_SIGNAL_TO_GD 2$' "$work/mixed.log" && pass "mixed-cs-signal-to-gd" || fail "mixed-cs-signal-to-gd" "no LIVE_MIX_CS_SIGNAL_TO_GD 2 line"
  grep -q '^LIVE_MIX_CS_CALLS_GD 3$' "$work/mixed.log" && pass "mixed-cs-calls-gd" || fail "mixed-cs-calls-gd" "no LIVE_MIX_CS_CALLS_GD 3 line"
  # Both the Hud.OnItemAdded handler of the skill (prints the bare id) and the marker handler must run.
  if grep -q '^LIVE_MIX_GD_SIGNAL_TO_CS sword$' "$work/mixed.log" && grep -q '^sword$' "$work/mixed.log"; then
    pass "mixed-gd-signal-to-cs"
  else
    fail "mixed-gd-signal-to-cs" "no LIVE_MIX_GD_SIGNAL_TO_CS sword line or no bare sword line"
  fi
  grep -q '^LIVE_MIX_UNTYPED_ARRAY 2$' "$work/mixed.log" && pass "mixed-untyped-array" || fail "mixed-untyped-array" "no LIVE_MIX_UNTYPED_ARRAY 2 line"
else
  fail "mixed-gd-calls-cs" "exit $? (see $work/mixed.log)"
fi
if grep -q 'SCRIPT ERROR\|ERROR:' "$work/mixed.log"; then
  fail "mixed-errors" "error lines in $work/mixed.log"
else
  pass "mixed-errors"
fi

# Typed array: the expected error goes to its own log, not to mixed.log.
bounded 120 "$godot" --headless --path . --quit-after 10 res://typed.tscn > "$work/typed.log" 2>&1 || true
typed_err=$(grep -i 'element type\|typed array' "$work/typed.log" | head -1 || true)
if [ -n "$typed_err" ] && ! grep -q '^LIVE_MIX_TYPED_TAKEN' "$work/typed.log"; then
  pass "mixed-typed-array-rejected"
  echo "  $typed_err"
else
  fail "mixed-typed-array-rejected" "no element type error in $work/typed.log"
fi

# gdUnit4Net: a second project runs C# tests through the `dotnet test` adapter on
# the .NET engine. gdUnit4Net states Godot 4.3 and 4.4 only and names GodotSharp
# 4.4.0; this run is the check on Godot .NET 4.7.2. These are the newest stable
# packages that resolve together (adapter 3.0.0 needs api 5.0.0; adapter 3.1.x
# needs the prerelease api 5.1.0-rc5).
gdunit_api=5.0.0
gdunit_adapter=3.0.0
gdunit="$work/gdunit"
rm -rf "$gdunit"
mkdir -p "$gdunit"
cd "$gdunit"
cat > project.godot <<'EOF'
config_version=5

[application]
config/name="LiveGdUnit"
config/features=PackedStringArray("4.7", "C#")

[dotnet]
project/assembly_name="LiveGdUnit"
EOF
cat > LiveGdUnit.csproj <<EOF
<Project Sdk="Godot.NET.Sdk/$sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <EnableDynamicLoading>true</EnableDynamicLoading>
    <IsTestProject>true</IsTestProject>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Microsoft.NET.Test.Sdk" Version="18.0.1" />
    <PackageReference Include="gdUnit4.api" Version="$gdunit_api" />
    <PackageReference Include="gdUnit4.test.adapter" Version="$gdunit_adapter" />
  </ItemGroup>
</Project>
EOF
cat > Health.cs <<'EOF'
public class Health
{
    public int Value { get; private set; }

    public Health(int value) { Value = value; }

    public void Damage(int amount) { Value -= amount; }
}
EOF
# The passing test needs the Godot runtime and asserts the engine version, so a
# pass proves that the adapter started the 4.7.2 engine from GODOT_BIN.
cat > HealthTest.cs <<'EOF'
using System.Threading.Tasks;
using Godot;
using GdUnit4;
using static GdUnit4.Assertions;

[TestSuite]
public class HealthTest
{
    [TestCase]
    [RequireGodotRuntime]
    public void DamageReducesValue()
    {
        var health = new Health(10);
        health.Damage(3);
        AssertThat(health.Value).IsEqual(7);
        AssertThat((string)Engine.GetVersionInfo()["string"]).StartsWith("4.7.2");
    }

    [TestCase]
    public void FailsOnPurpose()
    {
        var health = new Health(10);
        health.Damage(3);
        AssertThat(health.Value).IsEqual(8);
    }

    // The patterns of the gdUnit4Net examples in the skills: AutoFree with AddNode,
    // and the scene runner with AssertSignal. A signal name is the C# name (PascalCase).
    [TestCase]
    [RequireGodotRuntime]
    public void AutoFreeAddsNodeToTree()
    {
        var counter = AutoFree(new Counter());
        AddNode(counter, autoFree: false);
        AssertThat(counter.IsInsideTree()).IsTrue();
        AssertThat(counter.Value).IsEqual(5);
    }

    [TestCase]
    [RequireGodotRuntime]
    public async Task SceneRunnerSeesSignal()
    {
        var runner = ISceneRunner.Load("res://counter.tscn");
        var counter = (Counter)runner.Scene();
        var signal = AssertSignal(counter).StartMonitoring();
        counter.Add(2);
        await signal.IsEmitted("Changed", 5, 7).WithTimeout(500);
        await signal.IsNotEmitted("Other").WithTimeout(500);
        await runner.AwaitIdleFrame();
        AssertThat(counter.Value).IsEqual(7);
    }
}
EOF
cat > Counter.cs <<'EOF'
using Godot;

public partial class Counter : Node
{
    [Signal]
    public delegate void ChangedEventHandler(int oldValue, int newValue);

    [Signal]
    public delegate void OtherEventHandler();

    public int Value { get; private set; }

    public override void _Ready() { Value = 5; }

    public void Add(int amount)
    {
        int old = Value;
        Value += amount;
        EmitSignal(SignalName.Changed, old, Value);
    }
}
EOF
cat > counter.tscn <<'EOF'
[gd_scene load_steps=2 format=3]

[ext_resource type="Script" path="res://Counter.cs" id="1"]

[node name="Counter" type="Node"]
script = ExtResource("1")
EOF
cat > .runsettings <<EOF
<?xml version="1.0" encoding="utf-8"?>
<RunSettings>
  <RunConfiguration>
    <MaxCpuCount>1</MaxCpuCount>
    <TreatNoTestsAsError>true</TreatNoTestsAsError>
    <EnvironmentVariables>
      <GODOT_BIN>$godot</GODOT_BIN>
    </EnvironmentVariables>
  </RunConfiguration>
</RunSettings>
EOF
# The test host targets net8.0 and this host has only the .NET 10 runtime.
export DOTNET_ROLL_FORWARD=Major
gdunit_built=0
if bounded 600 dotnet build > "$work/gdunit-build.log" 2>&1; then gdunit_built=1; fi
gdunit_code=0
if [ "$gdunit_built" = 1 ]; then
  bounded 300 dotnet test --no-build --settings .runsettings --filter "Name=DamageReducesValue" > "$work/gdunit-pass.log" 2>&1 || gdunit_code=$?
fi
if [ "$gdunit_built" = 0 ]; then
  fail "gdunit4net-pass" "dotnet build failed (see $work/gdunit-build.log)"
elif [ "$gdunit_code" = 0 ] && grep -Eq 'Passed!.*Failed: +0, Passed: +1,' "$work/gdunit-pass.log"; then
  pass "gdunit4net-pass (gdUnit4.api $gdunit_api, gdUnit4.test.adapter $gdunit_adapter)"
else
  fail "gdunit4net-pass" "exit $gdunit_code, want exit 0 and 1 passed (see $work/gdunit-pass.log)"
fi
gdunit_code=0
if [ "$gdunit_built" = 1 ]; then
  bounded 300 dotnet test --no-build --settings .runsettings --filter "Name=FailsOnPurpose" > "$work/gdunit-fail.log" 2>&1 || gdunit_code=$?
fi
if [ "$gdunit_built" = 0 ]; then
  fail "gdunit4net-fail" "dotnet build failed (see $work/gdunit-build.log)"
elif [ "$gdunit_code" != 0 ] && grep -Eq 'Failed!.*Failed: +1, Passed: +0,' "$work/gdunit-fail.log"; then
  pass "gdunit4net-fail (exit $gdunit_code, 1 failed)"
else
  fail "gdunit4net-fail" "exit $gdunit_code, want non-zero and 1 failed (see $work/gdunit-fail.log)"
fi
gdunit_code=0
if [ "$gdunit_built" = 1 ]; then
  bounded 300 dotnet test --no-build --settings .runsettings --filter "Name=AutoFreeAddsNodeToTree|Name=SceneRunnerSeesSignal" > "$work/gdunit-api.log" 2>&1 || gdunit_code=$?
fi
if [ "$gdunit_built" = 0 ]; then
  fail "gdunit4net-api" "dotnet build failed (see $work/gdunit-build.log)"
elif [ "$gdunit_code" = 0 ] && grep -Eq 'Passed!.*Failed: +0, Passed: +2,' "$work/gdunit-api.log"; then
  pass "gdunit4net-api (AutoFree, AddNode, scene runner, AssertSignal)"
else
  fail "gdunit4net-api" "exit $gdunit_code, want exit 0 and 2 passed (see $work/gdunit-api.log)"
fi

rm -rf "$work/project/.godot" "$work/project/bin" "$work/project/obj"
rm -rf "$gdunit/.godot" "$gdunit/bin" "$gdunit/obj" "$gdunit/TestResults"
exit "$failed"
