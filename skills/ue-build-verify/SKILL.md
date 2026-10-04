---
name: ue-build-verify
description: Use when you changed C++, Build.cs, Target.cs, .uproject or .uplugin files in an Unreal Engine project and must build it, run its automation tests, and report the result before you send DONE.
---

# UE Build and Verify

The loop every Unreal Engine implementer runs before `DONE:`: find the engine, build the one target that the change touches, run the automation tests that cover the change, read the logs, and report exact results. "It compiles" is not done. A `DONE:` names the target, the configuration, the automation filter, and the result.

Command lines and the evidence for each flag are in `references/commands.md`. A flag marked "unverified on 5.8" there came from upstream or community sources only. Use it, and if it fails, say so in your report.

## When to use

- You edited C++ source, a header, a `*.Build.cs`, a `*.Target.cs`, a `.uproject` or a `.uplugin`, and the orchestrator assigned you the build.
- You must prove that a fix or a feature works before `DONE:`.
- Not for: asset or Blueprint changes (use `ue-editor-scripting` if available), packaging and cooking, or writing new tests (use `ue-testing-debugging` if available for test patterns).

## Inputs

- `.agents/ue-project-context.md`: engine path, engine version, platform, target names, test prefix. If it is missing, send `QUESTION:` and ask for `ue-tech-lead` to run first. Do not write it from a guess.
- The `.uproject` file and `Source/*.Target.cs` (target names).
- The list of files you changed, and the module of each file.
- The build assignment from the orchestrator. It may name the target and the configuration.

## Workflow

1. **Get permission to build.** UnrealBuildTool (UBT) runs one instance at a time, so two panes that build the same working copy conflict. Build only when the orchestrator assigned the build to you. If not, send `QUESTION:` and ask for the build slot. Check: the assignment or the reply names you as the builder.

2. **Find the engine.** Use the engine path from the project context. If there is none, read `EngineAssociation` in the `.uproject`:
   - A version such as `"5.8"` is an installed (Launcher) engine.
   - A GUID is a source build that is registered on this machine (`references/commands.md`, "Find the engine").
   - An empty value means the project is inside the engine tree.

   Check: `<Engine>/Engine/Build/BatchFiles` exists. If you cannot resolve the path, send `QUESTION:`. Do not search the whole disk, and do not guess.

3. **Check for blockers before you build.**
   - If the editor for this project is running with Live Coding, UBT refuses to build. Report `BLOCKED:` and name the process. Do not close the editor and do not trigger a Live Coding compile. It is the operator's session.
   - If a file you must edit is read-only and the project uses Perforce (a `.p4config`, `P4CONFIG`, or `p4 info` succeeds), the file needs `p4 edit`. Report `BLOCKED:` with the path. Never `chmod` it, never `attrib -r` it.

   Check: no running `UnrealEditor` process for this project, or you reported `BLOCKED:`.

4. **Choose the target and the configuration.** Default: `<Project>Editor`, the host platform (`Win64`, `Mac` or `Linux`), `Development`. Read the target names from `Source/*.Target.cs`; do not assume them. Build the game target (`<Project>`) too only when the change touches the editor/runtime split: an `#if WITH_EDITOR` block, an editor-only dependency in a `Build.cs`, or a module `Type` in a descriptor. Build nothing else unless the orchestrator asks. Check: you can write the target, platform and configuration in one line.

5. **Build in the background, with a log file.** An editor build can take longer than a foreground shell call allows (Claude Code stops a foreground command after 10 minutes). Start the build as a background job if your harness supports it, redirect all output to a log file under `Saved/Logs/` or a temporary directory, and poll the log until the process exits. Without background jobs, use a shell `&` with `nohup`, and poll. Check: the process exited, and you have its exit code and the full log.

6. **Read the build log.** Exit code 0 is necessary, not sufficient. Search the log for `error` and `warning` lines from the compiler, the linker, UnrealHeaderTool (UHT) and UBT. For each error, find the cause in the file and line it names. If `ue-module-build-system` is available, its `references/common-build-errors.md` maps each error to its cause: linker (`LNK*`), compiler (`C*`), UHT (`.generated.h`, `GENERATED_BODY`), UBT (module not found, circular dependency), Live Coding, and cooking. Fix, then build again from step 5. Check: exit code 0, and 0 errors in the log. Count the warnings in files you changed.

7. **Choose the automation filter.** Test names are dotted paths, such as `MyGame.Inventory.AddItem`, and `Automation RunTest <prefix>` runs every test under a prefix. Pick the narrowest prefix that covers every module you changed. To see what exists, run `Automation List` (`references/commands.md`). If no test covers the change, say so in your report. If the task requires new tests, write them first (the `tdd` skill if available). Check: the filter matches at least 1 test, or your report says that no test exists.

8. **Run the tests headless.** Run `UnrealEditor-Cmd` with `-ExecCmds="Automation RunTest <Filter>;Quit"`, `-unattended`, `-nullrhi`, and `-ReportExportPath=<dir>` (exact line in `references/commands.md`). Run it in the background, the same as the build. Tests flagged `NonNullRHI` need a GPU. If they are in the filter, run once without `-nullrhi`, or report them as not run. Check: the process exited, and the report directory has a JSON report.

9. **Read the test results.** Do not trust the exit code alone. Read the JSON report in the `-ReportExportPath` directory and count passed, failed and not-run tests. For each failure, copy the test name and its first error message. Search the log for `Error:` lines from the automation controller. Fix and repeat from step 5, or report the failure. Check: you have exact counts, and every failure has its test name and message.

10. **Report.** Send one line to the orchestrator with the build, the tests, and the log paths. Use the format in "Rules". Check: a reader can run the same build and the same tests from your message alone.

## Rules

- Build only when the orchestrator assigns the build. One UBT per working copy.
- Live Coding active: report `BLOCKED:`. Do not close the editor. Do not kill a process you did not start.
- Perforce read-only file: report `BLOCKED:` with the path. Never `chmod`, never `attrib -r`, never `p4 edit` unless the orchestrator tells you to.
- Run builds and test runs in the background, with output in a log file. Never let a foreground call time out halfway through a build.
- Build the narrowest target that proves the change. Do not run `Clean`, `Rebuild`, or delete `Intermediate/` or `Binaries/` unless the log proves stale output and the orchestrator agrees.
- Never edit `.uasset` or `.umap` bytes. Asset changes go through `ue-editor-scripting` or go in your `DONE:` as a step for a human.
- Never edit engine source to make a project build. Report it.
- Use absolute paths for the `.uproject`, the log files and the report directory.
- A `DONE:` names: target, platform, configuration, build result (errors, warnings in changed files), automation filter, test counts (passed, failed, not run), and the log and report paths. Example:

  `[ue-gameplay-engineer] DONE: Inventory stacking works. Built MyGameEditor Win64 Development: 0 errors, 0 new warnings. Ran "Automation RunTest MyGame.Inventory": 7 of 7 passed. Logs: Saved/Logs/build-inventory.log, Saved/Automation/inventory/.`

- If you could not build or test, the report starts with `BLOCKED:`, not `DONE:`, and says what stopped you.

## Review checklist

- [ ] The orchestrator assigned this build to me.
- [ ] The engine path came from the project context or `EngineAssociation`, not a guess.
- [ ] No Live Coding session, no Perforce read-only file, or I reported `BLOCKED:`.
- [ ] Target names come from `Source/*.Target.cs`.
- [ ] The build ran in the background and wrote a log file. Exit code 0 and 0 errors.
- [ ] The automation filter covers every changed module, or I said that no test exists.
- [ ] I read the JSON report and have exact counts.
- [ ] The `DONE:` line has target, platform, configuration, filter, counts, and paths.

## References

- `references/commands.md`: load at step 2. Engine lookup, build and test command lines for Windows, macOS and Linux, and the flag table with sources and the 5.8 verification status.
- `references/log-reading.md`: load at step 6 or step 9. Where each log is, what to search for, and how to read the automation JSON report.
