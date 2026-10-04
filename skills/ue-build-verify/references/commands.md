# Build and test commands (UE 5.8)

Placeholders: `<Engine>` is the engine root (the directory that contains `Engine/`). `<Proj>` is the absolute path of the `.uproject`. `<Target>` is a class name from `Source/*.Target.cs` without the `Target` suffix (`MyGameEditor`, `MyGame`). Quote every path; engine and project paths often contain spaces.

## Find the engine

1. `.agents/ue-project-context.md` gives the engine path. Use it.
2. Else read `EngineAssociation` in the `.uproject` (a JSON file):
   - A version string (`"5.8"`): an installed engine. The Epic Games Launcher records its installs. The usual default folders are `C:\Program Files\Epic Games\UE_5.8` (Windows) and `/Users/Shared/Epic Games/UE_5.8` (macOS). These defaults are unverified on 5.8: Epic's 5.8 "Install Unreal Engine" page says only "the default installation location for your operating system". Check that the folder exists; the operator can install elsewhere.
   - A GUID (`"{8C2A...}"`): a source build registered on this machine. On Windows, the registry key `HKEY_CURRENT_USER\Software\Epic Games\Unreal Engine\Builds` maps the GUID to a path: `reg query "HKCU\Software\Epic Games\Unreal Engine\Builds"`. On macOS and Linux, an `Install.ini` file under the user's Epic config directory holds the same map (location unverified on 5.8).
   - Empty or missing: the project lives inside an engine tree. Walk up from the project until you find `Engine/Build/BatchFiles`.
3. Verify: `<Engine>/Engine/Build/BatchFiles` and `<Engine>/Engine/Binaries/<Platform>` exist. If not, send `QUESTION:` with what you found.

## Build

Windows (cmd.exe; in PowerShell, prefix with `&`):

```
"<Engine>\Engine\Build\BatchFiles\Build.bat" <Target> Win64 Development -Project="<Proj>" > "<log>" 2>&1
```

macOS and Linux:

```
"<Engine>/Engine/Build/BatchFiles/Mac/Build.sh" <Target> Mac Development -Project="<Proj>" > "<log>" 2>&1
"<Engine>/Engine/Build/BatchFiles/Linux/Build.sh" <Target> Linux Development -Project="<Proj>" > "<log>" 2>&1
```

`RunUBT.bat` (Windows) takes the same arguments and is the form Epic's 5.8 static-analysis page uses: `Engine\Build\BatchFiles\RunUBT.bat <Target> Win64 Development ...`. Argument order: target, platform, configuration, then flags.

Configurations: `Debug`, `DebugGame`, `Development`, `Test`, `Shipping`. Automation tests are not compiled in `Test` and `Shipping` by default, so verify in `Development` or `DebugGame`.

Do not add `-WaitMutex`. Without it, a second UBT fails at once with a "conflicting instance" error, which you report. With it, the build waits for the other pane without a sign. Do not add `-Clean` or `-Rebuild`.

## Test

Windows:

```
"<Engine>\Engine\Binaries\Win64\UnrealEditor-Cmd.exe" "<Proj>" -ExecCmds="Automation RunTest <Filter>;Quit" -unattended -nullrhi -nosplash -stdout -ReportExportPath="<abs report dir>" -abslog="<abs log file>"
```

macOS and Linux: the same arguments. The executable is under `<Engine>/Engine/Binaries/Mac/` or `<Engine>/Engine/Binaries/Linux/`. Epic's 5.8 Linux quickstart names `Engine/Binaries/Linux/UnrealEditor`. No 5.8 page names the `-Cmd` binary on macOS or Linux (unverified on 5.8): list the directory and use the `UnrealEditor-Cmd` (or `UnrealEditor`) binary you find.

Filter forms (`Automation RunTest ...`, from Epic's 5.8 "Run Automation Tests" page):

- `MyGame.Inventory`: every test under this prefix.
- `MyGame.Inventory.AddItem+MyGame.Save`: several, joined with `+`.
- `Group:MyGroup`: a group defined in the project's automation settings.

List the tests that exist: `-ExecCmds="Automation List;Quit"`, then search the log.

`-ResumeRunTest` with the same `-ReportExportPath` continues after a crash, from the first incomplete test, and marks the test in progress as failed.

## Flag table

Rechecked on 2026-10-04 against Epic's 5.8 pages. UE 5.8 (hotfix 5.8.3, 2026-09-22) is the latest release, so the marks still apply. Rows still marked "no" have no 5.8 page that states them.

| Flag or command | Purpose | Source | Verified on 5.8 |
|---|---|---|---|
| `Build.bat <Target> <Platform> <Config>` | Build one target | Epic 5.8, Create an Installed Build (`Engine\Build\BatchFiles\Build.bat ShaderCompileWorker Win64 Development`) | yes (rechecked 2026-10-04) |
| `Mac/Build.sh`, `Linux/Build.sh` | Build on macOS and Linux | Community guides | no |
| `RunUBT.bat <Target> <Platform> <Config>` | Run UBT directly | Epic 5.8, Static Code Analysis | yes |
| `-Project="<Proj>"` | Name the project for a project target | Epic 5.8 (RunUAT pages); community UBT guides | yes, in RunUAT; UBT use unverified on 5.8 |
| `-WaitMutex` | Wait for another UBT instead of failing | Community guides | no (not used here on purpose) |
| `UnrealEditor-Cmd.exe <Proj>` | Headless editor process | Epic 5.8, Scripting the Editor using Python | yes |
| `Engine/Binaries/Linux/UnrealEditor` | Editor binary on Linux | Epic 5.8, Linux Development Quickstart | yes (rechecked 2026-10-04); the `-Cmd` name on macOS and Linux: no |
| `-ExecCmds="..."` | Run console commands after start | Epic 5.8, Command-Line Arguments Reference | yes |
| `Automation RunTest <Filter>;Quit` | Run tests, then exit | Epic 5.8, Run Automation Tests | yes |
| `Automation List` | List the registered tests | Skill `ue-testing-debugging` | no |
| `-unattended` | No dialogs, no user input | Epic 5.8, Command-Line Arguments Reference | yes |
| `-nullrhi` | No rendering, headless | Epic 5.8, Command-Line Arguments Reference | yes |
| `-nosplash` | No splash screen | Epic 5.8, Command-Line Arguments Reference | yes |
| `-stdout` | Log to standard output | Epic 5.8, Command-Line Arguments Reference | yes |
| `-abslog=<file>` | Log file at an absolute path | Epic 5.8, Command-Line Arguments Reference | yes |
| `-ReportExportPath=<dir>` | Write the JSON and HTML test report | Epic 5.8, Run Automation Tests; Command-Line Arguments Reference | yes |
| `-ResumeRunTest` | Resume a crashed run | Epic 5.8, Run Automation Tests | yes |
| `-testexit="Automation Test Queue Empty"` | Exit when the phrase is logged | Skill `ue-testing-debugging`; listed without text in Epic 5.8 reference | partly; not used here, `;Quit` replaces it |
| `-nopause` | No "press a key" at exit | Skill `ue-testing-debugging` | no; not in Epic 5.8 reference, not used here |
| `-ReportOutputPath` | Old name of `-ReportExportPath` | Epic 5.8 reference: "Deprecated" | yes (do not use) |
