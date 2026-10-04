---
name: ue-editor-scripting
description: Use when an Unreal Engine task must create or change assets or Blueprints - set defaults, add variables, wire an exposed C++ function, fill a data table, place actors - without hand-editing .uasset or .umap files.
---

# UE Editor Scripting

Change Unreal assets the only safe way an agent can: through the editor's own Python API, run headless by a commandlet, then verify the change in a fresh process. `.uasset` and `.umap` files are binary and cannot be merged. Never edit their bytes, never write them with a text tool, never copy one over another. With this skill, an asset change is code you can review: a Python script in the repository, a log that proves it ran, and a check that proves the result.

Command lines, flags and API facts with their 5.8 evidence are in `references/commands.md`. API recipes are in `references/python-recipes.md`.

## When to use

- A task needs an asset change: Blueprint defaults, a Blueprint variable or override, a pin value or a connection between existing nodes, a new Blueprint from a C++ parent, a data table or data asset, actors in a level, a rename or a move with redirectors.
- You exposed C++ to Blueprint and the result must be connected in an asset.
- Not for: C++ changes and builds (use `ue-build-verify` if available), editor tool C++ (detail customizations, editor modules), or art work that needs a human eye (materials by look, animation timing, level layout by feel). Hand those to a human in your `DONE:`.

## Inputs

- `.agents/ue-project-context.md`: engine path, enabled plugins, content layout. If it is missing, send `QUESTION:` and ask for `ue-tech-lead` to run first.
- The asset paths (`/Game/...`) to change, and the exact end state.
- The current state of each asset. Read it with a script first; do not assume it.
- Whether the operator's editor is open on this project.

## Workflow

1. **Check the plugins.** The `.uproject` must enable `PythonScriptPlugin`, and for the editor-startup form also `EditorScriptingUtilities`. If one is missing, adding it is a `.uproject` change: ask the orchestrator first, because it changes the project for everyone. Check: both appear with `"Enabled": true`, or the orchestrator answered.

2. **Get the slot.** A commandlet loads the whole project. Run one only when the orchestrator assigned it, never at the same time as a build or another commandlet on the same working copy. If the operator's editor is open on this project, do not save assets from a second process: the editor holds them in memory and overwrites your change on its next save. Report `BLOCKED:` and offer the script for the operator to run with File > Execute Python Script. Check: the assignment names you, and no editor has this project open.

3. **Read before you write.** Write a read-only script that loads each target asset and logs its current state: class, parent class, the properties you will change, the variables, graphs and pins you will touch. Run it (step 5 form). Check: the log shows every value you plan to change, with its current value.

4. **Write the change script.** Put it in the repository, for example `Scripts/Editor/<task>.py`, so it is reviewed and can run again. Rules for the script:
   - Use the editor API only: `EditorAssetSubsystem`, `AssetTools`, `BlueprintEditorLibrary`, `DataTableFunctionLibrary`, `LevelEditorSubsystem`, `EditorActorSubsystem` (`references/python-recipes.md`).
   - Make it idempotent: check the current value and skip what is already right. A second run changes nothing.
   - Fail loudly: on any unexpected state, call `unreal.log_error(...)` and `raise`. Never continue past a failed load or a failed compile.
   - Compile every changed Blueprint with `BlueprintEditorLibrary.compile_blueprint` and fail if it returns `False`.
   - Save only the assets you changed: `save_asset(path, only_if_is_dirty=True)`.
   - The last line logs a unique marker, for example `unreal.log("EDITOR-SCRIPT-OK <task> <n> assets saved")`.

   Check: the script names every asset path it saves, and it has the marker line.

5. **Run it headless.** Use the commandlet form: `UnrealEditor-Cmd <Proj> -run=pythonscript -script="<abs script>"` with `-unattended -nullrhi -nosplash -stdout -abslog=<file>` (`references/commands.md`). The commandlet does not load a level. A script that works on actors must load the level first with `LevelEditorSubsystem.load_level`. Run it in the background with the output in a log file, the same as a build. Check: the process exited, the log has your marker, and the log has 0 `LogPython: Error` lines.

6. **Check what changed on disk.** List the changed files with the version control tool (`git status --porcelain`, or `p4 opened` and `p4 diff -sa`). Check: exactly the `.uasset` and `.umap` files you meant to change, plus your script. Nothing else. An extra file means the script touched something you did not plan; find out why before you continue.

7. **Verify in a fresh process.** Run a second, read-only script in a new commandlet. It loads each changed asset from disk and asserts the end state: property values, variable names and types, pin connections and values, row names, the Blueprint compiles. It logs one line per check and a final marker. If an automation test covers the behavior, run it too (`ue-build-verify`, step 8). Check: every assertion passed in the new process, and the marker is in the log.

8. **Report.** The `DONE:` names the script, every asset changed, the verification script and its result, and the logs. Check: a reviewer can run both scripts again and get the same result.

## Rules

- Never edit `.uasset` or `.umap` bytes. Assets change only through editor Python in a commandlet, or the editor with the operator's consent. If the API cannot make a change, the change goes in your `DONE:` as a step for a human.
- One commandlet or build per working copy at a time, and only when assigned.
- Never save assets from a commandlet while the operator's editor has the project open.
- Perforce: a save fails on a read-only asset that is not checked out. Report `BLOCKED:` with the asset path. Never `chmod` it, never `attrib -r` it, never run `checkout_asset` or `p4 edit` unless the orchestrator says so.
- Prefer `EditorAssetSubsystem` over the older `EditorAssetLibrary`. Use `AssetTools.rename_assets` for renames and moves, so references are fixed and redirectors are written. Never move asset files with `mv` or `git mv`.
- Never use `delete_asset` without a reference check first (`find_package_referencers_for_asset`); it deletes even when other assets still refer to it. `consolidate_assets` deletes its inputs too.
- Change only what the task names. Do not run "fix up" or "resave all" passes over the project.
- The 5.8 Python API can set pin values and connect existing pins (`BlueprintGraphPin.try_create_connection`), add variables, add event and function overrides, and compile. It has no documented call to spawn an arbitrary function-call node. If the change needs new nodes, expose the logic as one C++ `UFUNCTION` that an override calls, or hand the node work to a human.
- Python editor scripts run only in the editor. Never put `import unreal` code on a runtime path.

## Review checklist

- [ ] The project enables `PythonScriptPlugin`.
- [ ] I had the slot, and no editor had the project open.
- [ ] A read-only script logged the state before the change.
- [ ] The change script is in the repository, is idempotent, fails loudly, compiles Blueprints, saves only changed assets, and logs a marker.
- [ ] The run log has the marker and 0 `LogPython: Error` lines.
- [ ] Version control shows only the planned files changed.
- [ ] A fresh-process verification script passed, or an automation test passed.
- [ ] The `DONE:` names the scripts, the assets, the result, and the logs.

## References

- `references/commands.md`: load at step 5. Both command-line forms, flags, plugin names, and the evidence table.
- `references/python-recipes.md`: load at step 3. Short recipes for loading, saving, Blueprint defaults, variables, overrides, pins, data tables, levels, renames, and the verification script pattern.
