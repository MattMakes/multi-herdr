#!/usr/bin/env bash
# Live check: Unreal Engine 5.8 on this Mac (U-19, U-56). Builds a blank C++
# project with UnrealBuildTool, runs 1 automation test and 1 Python commandlet,
# and checks the Git LFS lockable behaviour the ue-* skills describe.
# Result table: docs/live-checks/unreal.md. The gate never runs this.
#
# Usage: scripts/live/unreal.sh            (engine: /Users/Shared/Epic Games/UE_5.8)
#        UE_ENGINE=<engine root> scripts/live/unreal.sh
#        UE_KEEP=1 scripts/live/unreal.sh  (keep Binaries/ and Intermediate/)
# A cold editor-target build can take up to 60 minutes (UE_BUILD_TIMEOUT, s).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ENGINE="${UE_ENGINE:-/Users/Shared/Epic Games/UE_5.8}"
SCR="$ROOT/.worktrees/_scratch/live-unreal"
PROJ_DIR="$SCR/LiveUE"
PROJ="$PROJ_DIR/LiveUE.uproject"
LOGS="$SCR/logs"
BUILD_TIMEOUT="${UE_BUILD_TIMEOUT:-3600}"
EDITOR_TIMEOUT="${UE_EDITOR_TIMEOUT:-1200}"
FAILS=0

pass() { echo "PASS $1${2:+: $2}"; }
fail() { echo "FAIL $1: $2"; FAILS=$((FAILS + 1)); }
skip() { echo "SKIP $1: $2"; }

# run_bg <timeout s> <log> <cmd...>: run in the background, poll, return its exit code (124 on timeout).
run_bg() {
  local limit="$1" log="$2"; shift 2
  "$@" >"$log" 2>&1 &
  local pid=$! waited=0
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$waited" -ge "$limit" ]; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
      return 124
    fi
    sleep 5
    waited=$((waited + 5))
  done
  local rc=0
  wait "$pid" || rc=$?
  return "$rc"
}

cleanup() {
  if [ "${UE_KEEP:-0}" != "1" ] && [ -d "$PROJ_DIR" ]; then
    rm -rf "$PROJ_DIR/Binaries" "$PROJ_DIR/Intermediate" "$PROJ_DIR/DerivedDataCache" "$PROJ_DIR/Saved/Autosaves"
  fi
}
trap cleanup EXIT

mkdir -p "$LOGS"
CMD="$ENGINE/Engine/Binaries/Mac/UnrealEditor-Cmd"
EDITOR_FLAGS=(-unattended -nullrhi -nosplash -stdout)

# 1. Engine version.
VERSION_FILE="$ENGINE/Engine/Build/Build.version"
if [ ! -f "$VERSION_FILE" ]; then
  skip engine-version "no engine at $ENGINE"
  echo "SKIP all: Unreal Engine is not installed"
  exit 0
fi
VERSION="$(python3 -c 'import json,sys; v=json.load(open(sys.argv[1])); print("%d.%d.%d CL %d" % (v["MajorVersion"], v["MinorVersion"], v["PatchVersion"], v["Changelist"]))' "$VERSION_FILE")"
case "$VERSION" in
  5.8.*) pass engine-version "$VERSION" ;;
  *) fail engine-version "expected 5.8.x, found $VERSION" ;;
esac
for p in "$ENGINE/Engine/Build/BatchFiles" "$ENGINE/Engine/Binaries/Mac" "$ENGINE/Engine/Build/BatchFiles/Mac/Build.sh" "$CMD"; do
  [ -e "$p" ] || fail engine-layout "missing $p"
done
[ "$FAILS" -eq 0 ] && pass engine-layout "BatchFiles/Mac/Build.sh and Binaries/Mac/UnrealEditor-Cmd exist"

# 2. A minimal C++ project, written by hand (no template download).
rm -rf "$PROJ_DIR/Content/Live" "$PROJ_DIR/Saved"
mkdir -p "$PROJ_DIR/Source/LiveUE/Private" "$PROJ_DIR/Config" "$PROJ_DIR/Content" "$SCR/scripts"
cat >"$PROJ" <<'EOF'
{
	"FileVersion": 3,
	"EngineAssociation": "5.8",
	"Category": "",
	"Description": "Live check project for scripts/live/unreal.sh",
	"Modules": [
		{ "Name": "LiveUE", "Type": "Runtime", "LoadingPhase": "Default" }
	],
	"Plugins": [
		{ "Name": "PythonScriptPlugin", "Enabled": true },
		{ "Name": "EditorScriptingUtilities", "Enabled": true }
	]
}
EOF
cat >"$PROJ_DIR/Config/DefaultEngine.ini" <<'EOF'
[/Script/EngineSettings.GeneralProjectSettings]
ProjectID=4C495645554531000000000000000001
EOF
cat >"$PROJ_DIR/Source/LiveUE.Target.cs" <<'EOF'
using UnrealBuildTool;

public class LiveUETarget : TargetRules
{
	public LiveUETarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Game;
		DefaultBuildSettings = BuildSettingsVersion.Latest;
		IncludeOrderVersion = EngineIncludeOrderVersion.Latest;
		ExtraModuleNames.Add("LiveUE");
	}
}
EOF
cat >"$PROJ_DIR/Source/LiveUEEditor.Target.cs" <<'EOF'
using UnrealBuildTool;

public class LiveUEEditorTarget : TargetRules
{
	public LiveUEEditorTarget(TargetInfo Target) : base(Target)
	{
		Type = TargetType.Editor;
		DefaultBuildSettings = BuildSettingsVersion.Latest;
		IncludeOrderVersion = EngineIncludeOrderVersion.Latest;
		ExtraModuleNames.Add("LiveUE");
	}
}
EOF
cat >"$PROJ_DIR/Source/LiveUE/LiveUE.Build.cs" <<'EOF'
using UnrealBuildTool;

public class LiveUE : ModuleRules
{
	public LiveUE(ReadOnlyTargetRules Target) : base(Target)
	{
		PCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;
		PublicDependencyModuleNames.AddRange(new string[] { "Core", "CoreUObject", "Engine" });
	}
}
EOF
cat >"$PROJ_DIR/Source/LiveUE/LiveUEItemData.h" <<'EOF'
#pragma once

#include "CoreMinimal.h"
#include "Engine/DataAsset.h"
#include "LiveUEItemData.generated.h"

UCLASS(BlueprintType)
class LIVEUE_API ULiveUEItemData : public UDataAsset
{
	GENERATED_BODY()

public:
	UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "LiveUE")
	int32 Value = 0;
};
EOF
cat >"$PROJ_DIR/Source/LiveUE/Private/LiveUE.cpp" <<'EOF'
#include "Modules/ModuleManager.h"

IMPLEMENT_PRIMARY_GAME_MODULE(FDefaultGameModuleImpl, LiveUE, "LiveUE");
EOF
cat >"$PROJ_DIR/Source/LiveUE/Private/LiveUESmokeTest.cpp" <<'EOF'
#include "Misc/AutomationTest.h"

#if WITH_DEV_AUTOMATION_TESTS

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FLiveUESmokeAddTest, "LiveUE.Smoke.Add",
	EAutomationTestFlags::EditorContext | EAutomationTestFlags::ProductFilter)

bool FLiveUESmokeAddTest::RunTest(const FString& Parameters)
{
	TestEqual(TEXT("2 + 2"), 2 + 2, 4);
	return true;
}

#endif
EOF
pass project "LiveUE.uproject, 1 module, LiveUE + LiveUEEditor targets under $PROJ_DIR"

# 3. Build the editor target with UBT (skills/ue-build-verify/references/commands.md, "Build").
BUILD_LOG="$LOGS/build-editor.log"
start=$(date +%s)
rc=0
run_bg "$BUILD_TIMEOUT" "$BUILD_LOG" \
  "$ENGINE/Engine/Build/BatchFiles/Mac/Build.sh" LiveUEEditor Mac Development -Project="$PROJ" || rc=$?
secs=$(( $(date +%s) - start ))
build_errors=$(grep -cE '(^|[^A-Za-z])error( [A-Z]+[0-9]+)?:' "$BUILD_LOG" || true)
if [ "$rc" -eq 124 ]; then
  fail build "timeout after ${BUILD_TIMEOUT} s; log $BUILD_LOG"
elif [ "$rc" -ne 0 ] || [ "$build_errors" -ne 0 ]; then
  fail build "exit $rc, $build_errors error lines in ${secs} s; log $BUILD_LOG"
elif ! ls "$PROJ_DIR/Binaries/Mac/"*LiveUE*.dylib >/dev/null 2>&1; then
  fail build "exit 0 but no LiveUE dylib in Binaries/Mac"
else
  pass build "LiveUEEditor Mac Development: exit 0, 0 errors, ${secs} s"
fi
BUILT=$([ "$rc" -eq 0 ] && echo 1 || echo 0)

# 3b. The conflicting-instance message, checked by a second UBT while one runs, is not
# reproduced here; the engine source holds the text (docs/live-checks/unreal.md).

if [ "$BUILT" -ne 1 ]; then
  skip automation-list "build failed"
  skip automation-run "build failed"
  skip python-commandlet "build failed"
  skip python-error-exit "build failed"
  skip lfs-lockable "build failed"
  echo "FAILS: $FAILS"
  exit 1
fi

# 4a. Automation List (skills/ue-build-verify step 7).
LIST_LOG="$LOGS/automation-list.log"
rc=0
run_bg "$EDITOR_TIMEOUT" "$LIST_LOG" "$CMD" "$PROJ" -ExecCmds="Automation List;Quit" \
  "${EDITOR_FLAGS[@]}" -abslog="$LOGS/automation-list.abs.log" || rc=$?
if grep -q "LiveUE.Smoke.Add" "$LIST_LOG"; then
  pass automation-list "exit $rc; log lists LiveUE.Smoke.Add"
else
  fail automation-list "exit $rc; LiveUE.Smoke.Add not in $LIST_LOG"
fi

# 4b. Automation RunTest headless with a JSON report (skills/ue-build-verify steps 8-9).
REPORT="$SCR/report"
RUN_LOG="$LOGS/automation-run.log"
rm -rf "$REPORT"
rc=0
run_bg "$EDITOR_TIMEOUT" "$RUN_LOG" "$CMD" "$PROJ" -ExecCmds="Automation RunTest LiveUE.Smoke;Quit" \
  "${EDITOR_FLAGS[@]}" -ReportExportPath="$REPORT" -abslog="$LOGS/automation-run.abs.log" || rc=$?
report_files="$(ls "$REPORT" 2>/dev/null | tr '\n' ' ')"
if [ -f "$REPORT/index.json" ]; then
  counts="$(python3 -c '
import json, sys
raw = open(sys.argv[1], "rb").read()
for enc in ("utf-8-sig", "utf-16"):
    try:
        d = json.loads(raw.decode(enc)); break
    except Exception:
        continue
print("succeeded=%s failed=%s notRun=%s" % (d.get("succeeded"), d.get("failed"), d.get("notRun")))
' "$REPORT/index.json")"
  if [ "$counts" = "succeeded=1 failed=0 notRun=0" ]; then
    pass automation-run "exit $rc; index.json $counts; files: $report_files"
  else
    fail automation-run "exit $rc; index.json $counts"
  fi
else
  fail automation-run "exit $rc; no index.json in $REPORT (files: $report_files)"
fi
grep -E "LogAutomationController" "$RUN_LOG" | grep -E "Test Completed|Result=|Succeeded|Error" | head -5 || true

# 5. Python commandlet (skills/ue-editor-scripting step 5, Form 1).
PY_OK="$SCR/scripts/live_list_assets.py"
cat >"$PY_OK" <<'EOF'
import unreal

assets = unreal.get_editor_subsystem(unreal.EditorAssetSubsystem)
names = assets.list_assets("/Engine/BasicShapes", recursive=False)
unreal.log(f"LIVE-UE assets in /Engine/BasicShapes: {len(names)}")
unreal.log(f"LIVE-UE BlueprintEditorLibrary: {hasattr(unreal, 'BlueprintEditorLibrary')}")
unreal.log(f"LIVE-UE Actor.set_actor_label: {hasattr(unreal.Actor, 'set_actor_label')}")
tools = unreal.AssetToolsHelpers.get_asset_tools()
path = "/Game/Live/DA_LiveItem"
if not assets.does_asset_exist(path):
    new = tools.create_asset("DA_LiveItem", "/Game/Live", unreal.LiveUEItemData, None)
    if new is None:
        unreal.log_error("LIVE-UE create_asset with factory None returned None")
        raise RuntimeError("create_asset failed")
    new.set_editor_property("value", 10)
    if not assets.save_asset(path, only_if_is_dirty=True):
        raise RuntimeError("save failed")
unreal.log(f"LIVE-UE create_asset factory None: ok, value={assets.load_asset(path).get_editor_property('value')}")
levels = unreal.get_editor_subsystem(unreal.LevelEditorSubsystem)
if not levels.new_level("/Game/Live/L_Live"):
    raise RuntimeError("new_level failed")
actors = unreal.get_editor_subsystem(unreal.EditorActorSubsystem)
actor = actors.spawn_actor_from_class(unreal.StaticMeshActor, unreal.Vector(0, 0, 100))
actor.set_actor_label("Live_01")
unreal.log(f"LIVE-UE set_actor_label: get_actor_label={actor.get_actor_label()}")
unreal.log("EDITOR-SCRIPT-OK live-unreal")
EOF
PY_LOG="$LOGS/python-commandlet.log"
rc=0
run_bg "$EDITOR_TIMEOUT" "$PY_LOG" "$CMD" "$PROJ" -run=pythonscript -script="$PY_OK" \
  "${EDITOR_FLAGS[@]}" -abslog="$LOGS/python-commandlet.abs.log" || rc=$?
# unreal.log() writes at Log verbosity: it reaches the -abslog file, not -stdout.
PY_ABS="$LOGS/python-commandlet.abs.log"
py_errors=$(grep -c "LogPython: Error" "$PY_ABS" || true)
if grep -q "EDITOR-SCRIPT-OK live-unreal" "$PY_ABS" && [ "$py_errors" -eq 0 ]; then
  stdout_marker=$(grep -c "EDITOR-SCRIPT-OK" "$PY_LOG" || true)
  pass python-commandlet "exit $rc; marker in -abslog (stdout copies: $stdout_marker); 0 LogPython: Error lines"
else
  fail python-commandlet "exit $rc; $py_errors LogPython: Error lines; log $PY_ABS"
fi
grep -E "LIVE-UE" "$PY_ABS" | sed 's/^.*LogPython: //' || true

# 5b. Exit code of a commandlet whose Python raises (ue-editor-scripting, "Exit status and logs").
PY_BAD="$SCR/scripts/live_raise.py"
printf 'import unreal\nraise RuntimeError("LIVE-UE deliberate failure")\n' >"$PY_BAD"
BAD_LOG="$LOGS/python-error.log"
rc=0
run_bg "$EDITOR_TIMEOUT" "$BAD_LOG" "$CMD" "$PROJ" -run=pythonscript -script="$PY_BAD" \
  "${EDITOR_FLAGS[@]}" -abslog="$LOGS/python-error.abs.log" || rc=$?
if grep -q "LogPython: Error" "$BAD_LOG" && grep -q "Traceback" "$BAD_LOG"; then
  pass python-error-exit "exit $rc on a raised exception; LogPython: Error and Traceback in the log"
else
  fail python-error-exit "exit $rc; no LogPython: Error with Traceback in $BAD_LOG"
fi

# 6. Git LFS lockable behaviour (skills/ue-build-verify/references/git-lfs.md).
if ! command -v git-lfs >/dev/null 2>&1; then
  skip lfs-lockable "git-lfs is not installed"
else
  LFS="$SCR/lfs-repo"
  rm -rf "$LFS"
  mkdir -p "$LFS/Content/Live"
  cp "$PROJ_DIR/Content/Live/DA_LiveItem.uasset" "$LFS/Content/Live/"
  (
    cd "$LFS"
    git init -q
    git lfs install --local >/dev/null
    printf '*.uasset filter=lfs diff=lfs merge=lfs -text lockable\n*.umap   filter=lfs diff=lfs merge=lfs -text lockable\n' >.gitattributes
    git add .gitattributes Content/Live/DA_LiveItem.uasset
    git -c user.name=live -c user.email=live@example.invalid commit -qm "live" >/dev/null
  )
  attr="$(git -C "$LFS" check-attr filter lockable -- Content/Live/DA_LiveItem.uasset | tr '\n' ' ')"
  lsf="$(git -C "$LFS" lfs ls-files | tr '\n' ' ')"
  rm "$LFS/Content/Live/DA_LiveItem.uasset"
  git -C "$LFS" checkout -q -- Content/Live/DA_LiveItem.uasset
  if [ -w "$LFS/Content/Live/DA_LiveItem.uasset" ]; then ro="writable"; else ro="read-only"; fi
  lock_out="$(cd "$LFS" && git lfs lock Content/Live/DA_LiveItem.uasset 2>&1 | head -1 || true)"
  if echo "$attr" | grep -q "filter: lfs" && echo "$attr" | grep -q "lockable: set" \
     && echo "$lsf" | grep -q "DA_LiveItem.uasset" && [ "$ro" = "read-only" ]; then
    pass lfs-lockable "check-attr: $attr| ls-files: $lsf| checkout: $ro | lock with no remote: $lock_out"
  else
    fail lfs-lockable "check-attr: $attr| ls-files: $lsf| checkout: $ro"
  fi
fi

echo "FAILS: $FAILS"
[ "$FAILS" -eq 0 ]
