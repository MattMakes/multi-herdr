# Reading build and test output

## Build log

Read the file you redirected the build into. That is the only log you need; UBT also keeps its own log, but its location differs by platform and version.

Search, in this order:

1. `error` (case-insensitive), with the file and line it names. The first error usually causes the rest. Fix it first and build again.
2. `warning` in files you changed. Report the count. Do not fix warnings in files you did not change.
3. Lines that name UBT or the build as a whole (no file and line): a module that is not found, a target that is not found, a conflicting UBT instance, Live Coding.

| Text in the log | Meaning | Action |
|---|---|---|
| `Unable to build while Live Coding is active` | The editor holds Live Coding | `BLOCKED:`; do not close the editor |
| A conflicting instance of UnrealBuildTool | Another build is running | `BLOCKED:`; name it to the orchestrator |
| `error LNK2019`, `undefined symbol` | Linker: missing module dependency or missing `*_API` export | `ue-module-build-system`, `common-build-errors.md`, "Linker Errors" |
| `error C1083`, `file not found` (`#include`) | Include path or missing dependency | same file, "Compiler Errors" |
| An error that names a `.generated.h` or `GENERATED_BODY` | UnrealHeaderTool | same file, "UnrealHeaderTool Errors" |
| `Could not find definition for module` | UBT: `Build.cs` name or dependency spelling | same file, "UnrealBuildTool Errors" |
| `Unable to instantiate module 'UnrealEd'` | Runtime module depends on an editor module | same file, "Packaging and Cooking Failures" |
| Permission denied or access denied on a file | A read-only Git LFS lockable file that you have not locked, or a file held open by a running editor | `BLOCKED:` with the path and `git lfs locks --path="<path>"` (`git-lfs.md`) |

The Live Coding text is quoted from Epic forum reports, and the conflicting-instance text is from community reports. Both are unverified on 5.8. Search for "Live Coding" and "conflicting instance" as substrings, not for the full sentence.

On macOS and Linux the compiler is Clang. The error codes are different (no `LNK`, no `C1083`), but the messages name the same causes.

## Test output

The JSON report in the `-ReportExportPath` directory is the result. In UE 5.x the file is `index.json`, next to an HTML viewer; the name is unverified on 5.8, so list the directory. It holds totals (succeeded, failed, not run, warnings) and one entry per test with its full name, state and messages. Report the totals from this file.

If the report is missing, the run did not finish (a crash, a timeout, or a bad filter). Then:

- Search the log for `LogAutomationController`: it logs each test start and result, and every error with the test name.
- Search for `Fatal error` and `Assertion failed` for a crash. Run again with `-ResumeRunTest` and the same `-ReportExportPath` to finish the remaining tests.
- If no test started, the filter matched nothing. Run `Automation List` and correct the prefix.

The editor's exit code does not reliably show test failures. Never report a pass from the exit code alone.

Report each failure as: the full test name, the first error message, and the file and line if the message has one.

## Where logs live

- Your build log: the path you chose. Keep it until the orchestrator merges your work.
- Your test log: the `-abslog` path.
- The project log: `<Project>/Saved/Logs/<Project>.log`, overwritten by every editor run. Copy it if you need it later.
- Crash reports: `<Project>/Saved/Crashes/`.
