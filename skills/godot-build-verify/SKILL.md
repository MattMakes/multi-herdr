---
name: godot-build-verify
description: "Use when you changed GDScript, C#, scenes, resources or project.godot in a Godot 4 project and must prove the change before you send DONE - finds the engine, refuses a project newer than the engine, imports, runs a project-wide parse check (--check-only exits 0 on errors), builds C# with dotnet, runs GUT or gdUnit4 tests, smoke-runs the main scene, and greps the output. Targets Godot 4.7; every command was run on 4.7.2."
---

# Godot Build and Verify

The loop every Godot implementer runs before `DONE:`: find the engine, import,
parse-check every script, build C#, run the tests, smoke-run the main scene,
read the output, and report exact results. "It parses" is not done.

Every command here was run on Godot 4.7.2 (macOS). Exact command lines, exit
codes and output strings are in `references/commands.md`. The parse checker is
in `references/parse-check.md`.

Godot exits 0 after most failures. A parse error under `--check-only`, a
runtime `SCRIPT ERROR` in a smoke run, and a GUT test script that does not
parse all exit 0. Read the output, not only the exit code.

## Inputs

- `.agents/godot-project-context.md`: engine version, C#, test framework, main
  scene. If it is missing, send `QUESTION:` and ask for `godot-tech-lead` to run
  `godot-project-context`. Do not write it from a guess.
- A new project (no `project.godot` before your task) has no context file
  yet. Use the engine version and the language that the brief names, and say
  in your `DONE:` that they came from the brief. You do not write the context
  file. If the brief names neither, send `QUESTION:`.
- `project.godot`, and the list of files you changed.

## Workflow

1. **Check for the operator's editor.** The editor rewrites scenes it has open
   and shares `.godot/` with your import. List Godot processes by process name:
   `pgrep -x Godot | while read -r pid; do ps -o pid=,args= -p "$pid"; done`.
   A process without `--headless` whose arguments name this project's path is
   an open editor: report `BLOCKED:` and name the PID. Never kill a process you
   did not start. Measured on macOS: the list holds only processes whose
   binary is named `Godot`. A `ps | grep -i godot` filter also lists your own
   shell, `claude` and `horch` processes when their arguments hold a path with
   "godot" in it. On Linux, use the binary's file name:
   `pgrep -x "$(basename "$GODOT" | cut -c1-15)"` (it found a headless Godot
   on macOS; not run on Linux). (proof for the editor case: not run (needs a
   live editor): no fleet run may open the editor GUI.)
   Check: no editor process for this project, or you reported `BLOCKED:`.

2. **Find the engine and check its version.** Use `GODOT_PATH`, then `godot` on
   `PATH`, then `/Applications/Godot.app/Contents/MacOS/Godot`. Read the version
   in `config/features` of `project.godot` (for example `"4.7"`). If the project
   version is newer than the engine (`--version`), stop and report `BLOCKED:`
   with both versions. Do not open a newer project with an older engine.
   Script: `references/commands.md`, "Find the engine".
   Check: you can write the engine path and both versions in one line.

3. **Set a private HOME for every Godot call.** On macOS, Godot ignores
   `XDG_DATA_HOME` and `XDG_CONFIG_HOME`. It writes
   `~/Library/Application Support/Godot/editor_settings-4.7.tres` on every
   `--import`, and `app_userdata/<project name>/` on every run. Set
   `HOME="$PWD/.godot/horch-home"` for each Godot command. That keeps two
   instances, and the operator's editor, out of each other's files. `.godot/` is
   git-ignored and never scanned for resources. On Linux, Godot reads
   `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` instead; set all 3
   under the same directory (proof: run on 2026-10-06, Godot 4.7.2 on aarch64
   Linux: `--import` and a headless script wrote only under those 3 and
   nothing under `HOME`; `docs/live-checks/linux.md`).
   Check: `.godot/horch-home/` exists after the first call.

4. **Import when `.godot/` is missing or files were added.**
   `--headless --path . --import` exits 0 and writes a `.uid` sidecar next to
   every new script. One import at a time per working copy: `.godot/` is
   shared. Run a long import in the background with a log file, and poll it.
   New project: import, build the scenes (`godot-scene-files`), import again,
   then run steps 5 to 9. When `run/main_scene` names a scene that does not
   exist yet, the first import prints
   `ERROR: Cannot open file 'res://scenes/main.tscn'.` and exits 0. That 1
   error is expected in the first import only; the second import must have 0.
   Check: exit 0, `.godot/imported/` exists, and `git status` shows the new
   `.uid` files. Commit them with their scripts. In a git-ignored scratch
   project, there is nothing to commit: say so in the report.

5. **Run the parse check.** `--check-only --script` prints `Parse Error` and
   exits 0, so never gate on it. Write the checker from
   `references/parse-check.md` to `.godot/horch-home/parse_check.gd` and run it:
   it loads every `.gd`, `.tscn` and `.tres`, prints `PARSE_CHECK FAIL <path>`
   for each file that does not load, and exits 1. Run it after the import:
   a `class_name` from another file needs the import's global class cache.
   The checker does its work in `_initialize()`, not `_init()`: the autoloads
   do not exist in `_init()`, so every script that names one fails there
   (`references/parse-check.md`).
   Check: exit 0 and `PARSE_CHECK checked=<n> failed=0`.

6. **Build C#** when the context says C# (`"C#"` in `config/features`, or a
   `*.csproj`): `dotnet build` in the project root. It exits 1 on a compile
   error and prints `error CS<code>`. The output goes to
   `.godot/mono/temp/bin/Debug/`. Running C# scenes and tests needs the .NET
   build of Godot (`Godot_mono.app`); the standard build cannot run C#. If only
   the standard build is installed, say "C# built, not run" in the report.
   The C# lines of the Godot skills are proof: parse-checked only (compiled
   with dotnet; running them needs Godot .NET).
   Check: `Build succeeded.` and `0 Error(s)`.

7. **Run the tests** with the project's framework (`addons/gut` or
   `addons/gdUnit4`). Exact lines in `references/commands.md`, "Tests".
   - GUT: `-s res://addons/gut/gut_cmdln.gd -gdir=res://test -ginclude_subdirs -gexit -gdisable_colors`.
     Exit 0 pass, 1 fail. A test script that does not parse is skipped, and GUT
     still exits 0. Step 5 catches it; also grep for `SCRIPT ERROR`.
   - gdUnit4: `-s res://addons/gdUnit4/bin/GdUnitCmdTool.gd -a res://test --ignoreHeadlessMode -rd <res:// dir>`.
     Exit 0 pass, 100 fail, 101 orphan nodes only, 103 missing
     `--ignoreHeadlessMode`, 105 a test script does not parse. No test found
     exits 0. Pass `-rd res://.godot/horch-home/gdunit-reports`. Without it,
     gdUnit4 writes `reports/` into the project; an absolute path is made
     relative to the project.
   - Run the narrowest directory that covers your change, then the full suite
     once before `DONE:`. If no test covers the change, say so. Write new tests
     first with `tdd` when the task needs them.
   Check: you have the counts: run, passed, failed.

8. **Smoke-run the main scene.** `--headless --path . --quit-after 120` runs
   `run/main_scene` for 120 frames (about 2 s) and exits 0 even after a runtime
   error. For another scene, add `--scene res://path.tscn`.
   Check: the grep in step 9 finds nothing.

9. **Grep every log.** Search the import, parse check, test and smoke logs for
   `SCRIPT ERROR`, `Parse Error`, `ERROR:` and `WARNING:`. Each hit in a file you
   changed is a failure to fix. A hit in a file you did not change goes in the
   report as "pre-existing". Fix and repeat from step 4.
   Check: 0 hits in changed files.

10. **Report.** One line to the orchestrator with the engine version, each
    step's result and the log paths. A reader must be able to run the same
    checks from your message alone.

## Rules

- Headless only: pass `--headless` to every Godot call. Never open the editor
  GUI. Never pass `-e` without `--headless`.
- Stop only a Godot process that you started, by its PID (`kill "$pid"`, with
  `pid=$!` saved when you start it). Never use `pkill` or `killall` with a
  name: other workers and the operator's editor run Godot on the same Mac.
- In a `-s` (`SceneTree`) script, do the work in `_initialize()`, not
  `_init()`: the autoloads do not exist in `_init()`. The `-s` script itself
  cannot name an autoload, even in `_initialize()`: it compiles before the
  autoloads exist, and Godot prints
  `SCRIPT ERROR: Compile Error: Identifier not found: <Name>` and exits 0.
  Use `root.get_node("<Name>")`. Scripts that the `-s` script loads in
  `_initialize()` can name autoloads.
- One Godot import at a time per working copy. Parse checks, tests and smoke
  runs may run after the import finishes.
- Set `HOME="$PWD/.godot/horch-home"` for every Godot call.
- The operator's editor has the project open: `BLOCKED:`. Do not close it.
- Never edit `.godot/` contents (except `.godot/horch-home/`), `.import` files,
  or binary `.res` and `.scn` files. Never delete `.godot/` unless the
  orchestrator agrees: the next import rebuilds every asset.
- Commit the `.uid` sidecars that the import writes, with their scripts. A
  git-ignored scratch project has nothing to commit.
- Never trust an exit code alone. Read the output.
- A `DONE:` names the Godot version, the parse check result, the C# build
  result (C# projects), the test framework with counts, the smoke run, and the
  grep result. Example:

  `[godot-gameplay-programmer] DONE: Double jump works. Godot 4.7.2. Parse check: 41 files, 0 failed. GUT res://test/player: 12 of 12 passed; full suite 88 of 88. Smoke run 120 frames: 0 SCRIPT ERROR, 0 ERROR. Logs: .godot/horch-home/logs/.`

- If you could not verify, the report starts with `BLOCKED:`, not `DONE:`, and
  says what stopped you.

## Review checklist

- [ ] No open editor for this project, or I reported `BLOCKED:`.
- [ ] The engine is at least the project's `config/features` version.
- [ ] Every Godot call had `--headless` and the private `HOME`.
- [ ] Import exit 0; new `.uid` files are in my commit (none for a
      git-ignored scratch project).
- [ ] Parse check exit 0 with `failed=0`. I did not rely on `--check-only`.
- [ ] C# project: `dotnet build` with 0 errors.
- [ ] Tests ran with exact counts; gdUnit4 exit code read with its table.
- [ ] Smoke run and the grep show 0 hits in changed files.
- [ ] The `DONE:` line has the version, each result and the log paths.

## References

- `references/commands.md`: load at step 2. Engine lookup script, every
  command line, the exit code tables, and the output strings to grep.
- `references/parse-check.md`: load at step 5. The parse checker source and
  how to run it.
- `godot-scene-files`: rules for `.tscn` and `.tres` edits, and the headless
  scene load check.
