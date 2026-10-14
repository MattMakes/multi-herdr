# Code health scan

Part of `godot-safe-refactor`. Target engine: **Godot 4.7**.

Use this for a measured, ranked list of safe fixes before a cleanup: warnings, dead code, unused
signals, large functions, module coupling.

1. Run the headless import and a parse check over every script.
2. Copy the project to a scratch folder without test output.
3. Turn on every GDScript warning kind in the copy's `project.godot`. Headless runs print no
   parser warnings, and the editor ignores `override.cfg` for warnings.
4. Start the Godot language server on the copy. Open each script and save each warning
   diagnostic as 1 JSON line. Group the warnings by kind and by scope (game, tests, tools).
   Separate the default-on warnings in game code from the rest.
5. Run a metrics script: lines and branch count per function, duplicate blocks, magic numbers.
   Sort functions by branches, then by lines.
6. For each dead-code candidate, search the whole repository for string calls and built names.
   A name that a string calls is live.
7. Run a signal audit: signals that code emits but nothing connects, and the reverse.
8. Write a layout evidence script: folder sizes, autoload references, calls between modules.
   Check each apparent cycle in the code. Drop matches in comments; most apparent cycles are
   comment matches.
9. Review each module: purpose, size, callers. Propose extractions (pure logic out, deeper
   interfaces) with size and risk. Add a deletion test (what breaks if the module goes) and a
   do-not-fix list.
10. Map each fix candidate to the tests that cover it. Rank fixes by size and risk. Size each
    fix for 1 worker. Mark extractions that must wait until busy branches merge.
11. Verify each claim before you send the report: API signatures, the cause of each warning
    kind, each line number.
