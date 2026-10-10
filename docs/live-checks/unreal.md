# Live check: Unreal Engine

The installed UE 5.8 builds a hand-written blank C++ project with
UnrealBuildTool, runs 1 automation test and 1 Python commandlet headless, and
the Git LFS lockable setup behaves as the UE skills say. Items U-19, U-56.

## How to run

```bash
scripts/live/unreal.sh
```

It needs UE 5.8 at `/Users/Shared/Epic Games/UE_5.8` (set `UE_ENGINE` for
another root), Xcode (UBT uses its Clang), `python3`, and `git-lfs` for the
last step. No engine is `SKIP`. It starts no model session, so it costs no
tokens. A cold editor-target build can take up to 60 minutes on a slow host;
the script waits up to `UE_BUILD_TIMEOUT` seconds (default 3600). Each editor
run waits up to `UE_EDITOR_TIMEOUT` seconds (default 1200). On this host the
whole run takes about 5 minutes.

The script writes the project `LiveUE` under
`.worktrees/_scratch/live-unreal/` by hand: `LiveUE.uproject` (1 `Runtime`
module, `PythonScriptPlugin` and `EditorScriptingUtilities` enabled),
`LiveUE.Target.cs` and `LiveUEEditor.Target.cs`, `LiveUE.Build.cs`, a
`UDataAsset` subclass and 1 automation test `LiveUE.Smoke.Add`. Logs go to
`.worktrees/_scratch/live-unreal/logs/`. At the end it deletes `Binaries/`,
`Intermediate/` and `DerivedDataCache/` (`UE_KEEP=1` keeps them), so every run
is a cold project build.

Steps:

- `engine-version`: `Engine/Build/Build.version`.
- `engine-layout`: `Engine/Build/BatchFiles/Mac/Build.sh` and
  `Engine/Binaries/Mac/UnrealEditor-Cmd` exist.
- `project`: the project files above.
- `build`: `Build.sh LiveUEEditor Mac Development -Project="<Proj>"`, in the
  background with a log file (`skills/ue-build-verify` step 5); exit 0, 0
  compiler `error:` lines, and a `LiveUE` dylib in `Binaries/Mac`.
- `automation-list`: `-ExecCmds="Automation List;Quit"`; the log lists the
  test.
- `automation-run`: `-ExecCmds="Automation RunTest LiveUE.Smoke;Quit"` with
  `-unattended -nullrhi -nosplash -stdout -ReportExportPath -abslog`; reads
  `index.json` totals.
- `python-commandlet`: `-run=pythonscript -script=<file>`; the script lists
  `/Engine/BasicShapes`, checks `unreal.BlueprintEditorLibrary`, creates and
  saves a data asset with `create_asset(..., None)`, makes a new level, spawns
  an actor and calls `set_actor_label`, then logs a marker.
- `python-error-exit`: a script that raises; records the exit code and the
  `LogPython: Error` and `Traceback` lines.
- `lfs-lockable`: a scratch git repo with the `.gitattributes` from
  `skills/ue-build-verify/references/git-lfs.md` and the saved `.uasset`;
  `check-attr`, `lfs ls-files`, a fresh checkout, and `git lfs lock` with no
  remote.

Engine source claims: an installed engine ships the UBT C# source
(`Engine/Source/Programs/UnrealBuildTool`) but no engine `.cpp` files. The
fixes below cite the UBT source lines where a live run cannot show a claim.

## 2026-10-06

Host: macOS (Darwin 25.5.0), 18 cores, Xcode 26.6 (Mac SDK 26.5, Clang
21.1.6), git-lfs 3.4.0.

| Step | Claim it proves | Tool version | Result | Evidence |
|------|-----------------|--------------|--------|----------|
| engine-version | The engine is UE 5.8 (`skills/ue-build-verify/SKILL.md`). | UE 5.8.1 | PASS | `5.8.1 CL 56057345`, branch `++UE5+Release-5.8`. |
| engine-layout | The Launcher install folder on macOS is `/Users/Shared/Epic Games/UE_5.8`; the headless binary on macOS is `Engine/Binaries/Mac/UnrealEditor-Cmd` (`ue-build-verify/references/commands.md`). | UE 5.8.1 | PASS, 2 marks cleared | Both paths exist. `UnrealEditor-Cmd` is a universal Mach-O executable next to `UnrealEditor.app`. |
| project | A blank C++ project needs no template: `.uproject`, 1 module, 1 `Target.cs` pair. | UE 5.8.1 | PASS | `BuildSettingsVersion.Latest` (= `V7`) and `EngineIncludeOrderVersion.Latest` (= `Unreal5_8`) from `Configuration/Rules/TargetRules.cs`. |
| build | `Mac/Build.sh <Target> Mac Development -Project="<Proj>"` builds the editor target (`ue-build-verify/references/commands.md`, "Build"). | UE 5.8.1, Clang 21.1.6 | PASS, 2 marks cleared | Exit 0, `Result: Succeeded`, 0 errors, 0 warnings. Cold build 66 s to 192 s over 4 runs (141 s on the first run). `-Project=` is read in `Configuration/Descriptors/TargetDescriptor.cs:411`. |
| automation-list | `Automation List` lists the registered tests (`ue-build-verify` step 7). | UE 5.8.1 | PASS, 1 mark cleared | The log names `LiveUE.Smoke.Add`. Exit 0. |
| automation-run | `UnrealEditor-Cmd <Proj> -ExecCmds="Automation RunTest <Filter>;Quit" -unattended -nullrhi ... -ReportExportPath` runs headless and writes a JSON report (`ue-build-verify` steps 8-9, `log-reading.md`). | UE 5.8.1 | PASS, 1 mark cleared | Exit 0. The report directory has `index.json` and `index.html`. `index.json`: `succeeded=1 failed=0 notRun=0`; keys `succeeded`, `succeededWithWarnings`, `failed`, `notRun`, `inProcess`, `tests[]` (`fullTestPath`, `state`, `errors`, `warnings`, `entries`). Log: `LogAutomationController: Display: Test Completed. Result={Success} Name={Add} Path={LiveUE.Smoke.Add}`. |
| python-commandlet | `-run=pythonscript -script=<file>` runs editor Python headless; `PythonScriptPlugin` and `EditorScriptingUtilities` are the descriptor names; `create_asset` with factory `None` and `Actor.set_actor_label` work (`ue-editor-scripting`). | UE 5.8.1 | PASS, 4 marks cleared, 2 claims fixed | Exit 0. 0 `LogPython: Error` lines. `LogPluginManager: Mounting Engine plugin PythonScriptPlugin` (`Plugins/Experimental/`) and `EditorScriptingUtilities` (`Plugins/Editor/`). 6 assets in `/Engine/BasicShapes`. `create_asset` with `None` saved `DA_LiveItem` with `value=10`. `get_actor_label` returned `Live_01`. Fix 1: the `unreal.log` marker is in the `-abslog` file only, 0 copies on `-stdout`; the skill now says to read `-abslog`. Fix 2: `BlueprintEditorLibrary` is the engine module `Engine/Source/Editor/BlueprintEditorLibrary`, not a plugin; the skill told the agent to find a plugin. |
| python-error-exit | The commandlet exit code on a Python error (`ue-editor-scripting/references/commands.md`, "Exit status and logs"). | UE 5.8.1 | PASS, 1 mark cleared | An uncaught `RuntimeError` gives exit 255, with `LogPython: Error` and `Traceback` lines in both logs. The skill now treats non-zero as failure and still needs the log as proof. |
| lfs-lockable | `.gitattributes` with `lockable` puts `.uasset` in LFS and checks it out read-only; `git lfs lock` needs a remote (`ue-build-verify/references/git-lfs.md`). | git-lfs 3.4.0 | PASS | `filter: lfs`, `lockable: set`; `git lfs ls-files` marks the file `*`; a fresh checkout is read-only; `git lfs lock` with no remote: `Locking Content/Live/DA_LiveItem.uasset failed: missing protocol: ""`. |
| ubt-messages (source) | The texts `Unable to build while Live Coding is active` and `A conflicting instance of ... is already running.`, and `-WaitMutex` (`log-reading.md`, `commands.md`). | UE 5.8.1 UBT source | PASS (source, not a live run), 3 marks cleared | `UnrealBuildTool/System/HotReload.cs:277`; `Shared/EpicGames.Build/System/GlobalSingleInstanceMutex.cs:45`; `UnrealBuildTool/GlobalOptions.cs:75`. A live run would need a second UBT or a Live Coding editor; the script does not start them. |

Marks this host cannot clear (they stay "unverified on 5.8"):

- The Windows default folder `C:\Program Files\Epic Games\UE_5.8`: needs a Windows host.
- `Linux/Build.sh` and the Linux `-Cmd` binary name: needs a Linux host.
- The macOS and Linux `Install.ini` location for a registered source build:
  needs a source build; the installed engine has no `DesktopPlatformMac.cpp`.
