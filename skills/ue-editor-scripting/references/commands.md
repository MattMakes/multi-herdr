# Running editor Python (UE 5.8)

Placeholders: `<Engine>` is the engine root, `<Proj>` the absolute `.uproject` path, `<script>` the absolute path of a `.py` file. Find the engine as in `ue-build-verify` (`references/commands.md`, "Find the engine"), or from `.agents/ue-project-context.md`.

## Plugins

The `.uproject` `Plugins` array enables plugins by name:

```json
{ "Name": "PythonScriptPlugin", "Enabled": true },
{ "Name": "EditorScriptingUtilities", "Enabled": true }
```

Epic's 5.8 page names them "Python Editor Script Plugin" and "Editor Scripting Utilities". The descriptor names above are the long-standing ones, unverified on 5.8: check `<Engine>/Engine/Plugins/**/PythonScriptPlugin.uplugin` and `EditorScriptingUtilities.uplugin` exist. `BlueprintEditorLibrary` is its own module in the 5.8 Python reference; if `unreal.BlueprintEditorLibrary` is missing at run time, find the plugin that contains `BlueprintEditorLibrary` under `<Engine>/Engine/Plugins` and ask the orchestrator before you enable it.

## Form 1: commandlet (default)

```
"<Engine>\Engine\Binaries\Win64\UnrealEditor-Cmd.exe" "<Proj>" -run=pythonscript -script="<script>" -unattended -nullrhi -nosplash -stdout -abslog="<log>"
```

- Fast; no editor UI. It does not load a level. Call `unreal.get_editor_subsystem(unreal.LevelEditorSubsystem).load_level("/Game/Maps/MyMap")` first when the script works on actors.
- `-script=` takes a file path, or Python statements with `\n` for line breaks. Use a file: it is reviewed and repeatable.
- On macOS and Linux, use the editor binary in `<Engine>/Engine/Binaries/Mac/` or `Linux/` (Epic's 5.8 Linux quickstart names `Engine/Binaries/Linux/UnrealEditor`; the `-Cmd` name is unverified on 5.8; list the directory).

## Form 2: full editor start

```
"<Engine>\Engine\Binaries\Win64\UnrealEditor-Cmd.exe" "<Proj>" -ExecutePythonScript="<script>"
```

- Starts the editor, loads the default startup level, then runs the script. Needs `EditorScriptingUtilities`. Slower. Use it only when the commandlet form lacks something the script needs (for example, an editor-only system that starts with the full editor).
- The editor exits after the script on some versions and stays open on others (forum reports). Check the process ends; if it stays open, end the script with a quit command and say so in your report.

## Exit status and logs

- Do not rely on the exit code to show a Python failure (unverified on 5.8). The proof is the log: your marker line, and 0 `LogPython: Error` lines.
- `unreal.log`, `unreal.log_warning`, `unreal.log_error` write to the `LogPython` category.
- A Python exception prints a traceback under `LogPython: Error`. Search for `Traceback` too.

## In the operator's editor

If the operator's editor is open on the project, the operator runs the same script: File > Execute Python Script, or type the script path into the editor console's Python mode. Send the script path and the expected marker line. Wrap interactive changes in `with unreal.ScopedEditorTransaction("<task>"):` so they undo as one step.

## Evidence

Rechecked on 2026-10-04. UE 5.8 (hotfix 5.8.3, 2026-09-22) is the latest release, so the 5.8 marks still apply.

| Flag, command or API | Purpose | Source | Verified on 5.8 |
|---|---|---|---|
| `UnrealEditor-Cmd.exe <Proj> -run=pythonscript -script=<file>` | Headless Python commandlet | Epic 5.8, Scripting the Unreal Editor Using Python | yes |
| "does not automatically load levels" | Commandlet limit | same page | yes |
| `-ExecutePythonScript=<file>` | Run a script after full editor start | same page | yes |
| Python Editor Script Plugin, Editor Scripting Utilities | Required plugins | same page | yes (display names) |
| `PythonScriptPlugin`, `EditorScriptingUtilities` descriptor names | `.uproject` entries | long-standing engine plugin names | no |
| `unreal.ScopedEditorTransaction` | One undo step | same page | yes |
| `unreal.log`, `log_warning`, `log_error` (`LogPython`) | Logging | Epic 5.8 Python API, module `unreal` | yes |
| `-unattended`, `-nullrhi`, `-nosplash`, `-stdout`, `-abslog=` | Headless, logging | Epic 5.8, Command-Line Arguments Reference | yes |
| `-run=` | Commandlet selector | Epic 5.8 Python page (as `-run=pythonscript`); not listed in the argument reference | yes, in that form |
| Exit code on a Python error | Failure signal | none found | no (not used) |
| `unreal.load_class` | Load a C++ class by path | Epic 5.8 Python API, module `unreal` | yes (rechecked 2026-10-04) |
| `BlueprintEditorLibrary.add_event_override(blueprint, event_name, position: IntPoint)` | Event override node | Unreal Python 5.8, `BlueprintEditorLibrary` | yes (rechecked 2026-10-04); the recipe used `Vector2D` before |
| `AssetRenameData(asset, new_package_path, new_name)` | Rename or move | Unreal Python 5.8, `AssetRenameData` | yes (rechecked 2026-10-04) |
| `AssetTools.create_asset(..., factory)` with `factory=None` | Create without a factory | Unreal Python 5.8, `AssetTools` types `factory` as `Factory` | no |
| `Actor.set_actor_label` | Name a placed actor | not on the Unreal Python 5.8 `Actor` page (only `get_actor_label`) | no |
