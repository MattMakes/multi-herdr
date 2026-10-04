# W2 ue-own: `ue-build-verify` and `ue-editor-scripting`

Branch `ds/ue-own`. Worker opus-41. Date 2026-10-03.

## Result

| Skill | SKILL.md | Directory | Files |
|---|---|---|---|
| `ue-build-verify` | 8,796 B | 18,126 B | `SKILL.md`, `references/commands.md`, `references/log-reading.md` |
| `ue-editor-scripting` | 8,163 B | 17,977 B | `SKILL.md`, `references/commands.md`, `references/python-recipes.md` |

Both are house skills: own text, `"sources": []`, `"vendored": false`. The `adaptation` text names the upstream skills consulted, with the pin `quodsoler/unreal-engine-skills` at `f3742d7b688690810df369802b90430324e380b9` (MIT). No upstream text is copied. Both are listed in `REPO_ORIGINAL` in `crates/horch-core/tests/skills_catalog.rs`; the orchestrator approved this one-line test change (option 1). Gate green at each commit.

## Sources

No engine is installed, and `EpicGames/UnrealEngine` on GitHub is not reachable from this account (HTTP 404), so no flag was checked against engine source. Every "yes" below is a UE 5.8 page on dev.epicgames.com (each page states "5.8").

1. Scripting the Unreal Editor Using Python (5.8): https://dev.epicgames.com/documentation/en-us/unreal-engine/scripting-the-unreal-editor-using-python
2. Run Automation Tests (5.8): https://dev.epicgames.com/documentation/en-us/unreal-engine/run-automation-tests-in-unreal-engine
3. Command-Line Arguments Reference (5.8): https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-engine-command-line-arguments-reference
4. Static Code Analysis (5.8, `RunUBT.bat` lines): https://dev.epicgames.com/documentation/unreal-engine/static-code-analysis-in-unreal-engine
5. Linux Development Quickstart (5.8, `RunUAT ... -Project=`): https://dev.epicgames.com/documentation/en-us/unreal-engine/linux-development-quickstart-for-unreal-engine
6. Build Configuration (5.8; `bWithLiveCoding`, no CLI flags): https://dev.epicgames.com/documentation/en-us/unreal-engine/build-configuration-for-unreal-engine
7. Unreal Python 5.8 reference, `unreal` module, and the `BlueprintEditorLibrary`, `BlueprintGraphPin`, `EditorAssetSubsystem`, `AssetTools`, `DataTableFunctionLibrary`, `EditorActorSubsystem`, `LevelEditorSubsystem`, `EdGraph` classes: https://dev.epicgames.com/documentation/en-us/unreal-engine/python-api/module/unreal?application_version=5.8 (class pages at `.../python-api/class/<Name>?application_version=5.8`)
8. Community, not 5.8: Build.bat argument order, https://github.com/Allar/compiling-unreal and https://craftedcart.gitlab.io/unrealbookoftips/building_and_testing/build_from_the_command_line.html; Live Coding message, https://forums.unrealengine.com/t/how-to-build-c-with-unreal-editor-open/545436; registry key, https://x157.github.io/UE5/Windows-Registry-Keys.html; `-ExecutePythonScript` exit behavior, https://forums.unrealengine.com/t/prevent-editor-from-exiting-when-running-from-command-line-with-executepythonscript/2108201
9. Upstream (pinned): `ue-testing-debugging` `SKILL.md` and `references/automation-test-patterns.md`; `ue-module-build-system` `SKILL.md`, `references/build-cs-reference.md`, `references/common-build-errors.md`.

## Flag table

| Flag or command | Purpose | Source | Verified on 5.8 |
|---|---|---|---|
| `Build.bat <Target> <Platform> <Config>` | Build one target (Windows) | 8, 9 | no |
| `Mac/Build.sh`, `Linux/Build.sh` | Build one target (macOS, Linux) | 8 | no |
| `RunUBT.bat <Target> <Platform> <Config>` | Run UBT directly | 4 | yes |
| `-Project=<uproject>` | Project for the target | 5 (RunUAT form) | partly: RunUAT yes, UBT no |
| `-WaitMutex` | Queue behind another UBT | 8 | no; deliberately not used |
| `UnrealEditor-Cmd.exe <uproject>` | Headless editor process | 1 | yes (Windows); Mac/Linux binary name no |
| `-ExecCmds="..."` | Console commands after start | 3 | yes |
| `Automation RunTest <Filter>;Quit` | Run tests, then exit | 2 | yes |
| `+` and `Group:` filter forms | Several tests, a group | 2 | yes |
| `Automation List` | List tests | 9 | no |
| `-ReportExportPath=<dir>` | JSON + HTML report | 2, 3 | yes |
| `-ResumeRunTest` | Resume after a crash | 2 | yes |
| `-ReportOutputPath` | Old name | 3 ("Deprecated") | yes; not used |
| `-unattended` | No dialogs | 3 | yes |
| `-nullrhi` | Headless rendering | 3 | yes |
| `-nosplash` | No splash | 3 | yes |
| `-stdout` | Log to stdout | 3 | yes |
| `-abslog=<file>` | Absolute log path | 3 | yes |
| `-testexit=<phrase>` | Exit on a log phrase | 9; 3 lists the keyword without text | partly; replaced by `;Quit` |
| `-nopause` | No pause at exit | 9 | no; not in 3, not used |
| `-run=pythonscript -script=<file>` | Python commandlet | 1 | yes |
| `-ExecutePythonScript=<file>` | Python after full editor start | 1 | yes |
| `index.json` report file name | Read results | none at 5.8 | no |
| "Unable to build while Live Coding is active" | Live Coding blocks UBT | 8 (forum) | no; skill searches the substring "Live Coding" |
| `HKCU\Software\Epic Games\Unreal Engine\Builds` | GUID to source-build path | 8 | no |
| Launcher default folders `UE_5.8` | Installed engine path | none | no; skill says check the folder exists |
| `PythonScriptPlugin`, `EditorScriptingUtilities` descriptor names | `.uproject` plugin entries | 1 gives display names only | no |

Python API names used in `ue-editor-scripting`: all are on the 5.8 class pages (source 7) except the ones marked "unverified on 5.8" in `python-recipes.md`: `unreal.load_class`, the `add_event_override` position type, `create_asset` with factory `None`, `AssetRenameData` fields, `set_actor_label`.

## Decisions

- **`;Quit` instead of `-testexit`.** Epic's 5.8 page uses `Automation RunTest <Filter>;Quit`. Upstream uses `RunTests` plus `-testexit="Automation Test Queue Empty"`. I chose Epic's documented form; `-testexit` is in the 5.8 reference only as a bare keyword.
- **No `-WaitMutex`.** The IDE passes it, but in a fleet a silent wait hides a second builder. Without it, a conflict fails fast and the worker reports `BLOCKED:`.
- **Redirect build output to a file** instead of the UBT log location or `-Log=`. Neither is documented for 5.8; redirection needs no flag.
- **Results from the JSON report, not the exit code.** No 5.8 page states the exit code for failed tests or a Python error. Both skills require a log marker or the report.
- **Node spawning.** The 5.8 Python reference has `BlueprintGraphPin.try_create_connection` and `set_pin_value`, overrides and variables, but no documented call to spawn an arbitrary node or list a graph's nodes. The skill says so and sends new-node work to one C++ `UFUNCTION` or to a human. `graph_add_node_call_function` from search results belongs to the third-party 20tab UnrealEnginePython plugin, not to Epic's API; it is not used.
- **Commandlet versus open editor.** The skill forbids saving from a commandlet while the operator's editor has the project open, and offers the script to the operator instead. This is a fleet-safety rule, not an Epic statement.
- **Perforce.** Both skills report a read-only file as `BLOCKED:` and forbid `chmod`, `attrib -r`, `p4 edit` and `checkout_asset` unless the orchestrator says so.

## Dropped

- Upstream test-writing patterns, profiling, logging macros: they stay in `ue-testing-debugging`. `ue-build-verify` points to it by name.
- Build error catalog: stays in `ue-module-build-system` `common-build-errors.md`; `log-reading.md` maps log text to its section names only.
- `-NoHotReloadFromIDE`, `-Mode=`, `-SingleFile`, static-analysis flags: not needed for the verify loop.
- Packaging and cooking (`BuildCookRun`): out of scope (the wave report lists `ue-packaging` as optional later).

## Example trigger lines

- `ue-build-verify`: "Add a stack limit to UInventoryComponent, then build and run the Inventory tests."
- `ue-build-verify`: "Fix the LNK2019 in MyGameEditor after the Build.cs change and confirm the build."
- `ue-build-verify`: "Before DONE, prove the save-game change compiles in Development and passes MyGame.Save."
- `ue-editor-scripting`: "Set BP_Coin's Value default to 10 and add a Score variable."
- `ue-editor-scripting`: "Rebuild DT_Items from Data/items.csv."
- `ue-editor-scripting`: "Place 5 BP_Coin actors in /Game/Maps/Arena at these positions."

## Gotchas and follow-ups

- Recheck the "no" rows when an engine or the 5.8 source is available, especially the Mac and Linux editor binary name, `index.json`, and the Live Coding message.
- `BlueprintEditorLibrary` is in its own module in the 5.8 reference. If a project lacks it, a plugin may need enabling; the skill makes the worker ask first.
- `skills/README.md`: this unit created the "Unreal Engine skills" table with columns Skill, Upstream source, Copy. W1 (`ue-vendor`) creates a table with the same name; merge the rows into one sorted table at rebase.
- `crates/horch-core/tests/skills_catalog.rs` `REPO_ORIGINAL` changed (approved). V0 also edits this file; the orchestrator resolves it at merge.
- `cmp_05_e2e_candidates_in_dataset_workspace` failed once in the first full gate and passed alone. Another unit owns that flake.
- P1 (`ue-teammates`): attach `ue-build-verify` to every implementation teammate and `ue-qa-engineer`, and `ue-editor-scripting` to `ue-tools-engineer` and `ue-technical-artist`. Persona rule 2 can now say: assets change only through `ue-editor-scripting`.
