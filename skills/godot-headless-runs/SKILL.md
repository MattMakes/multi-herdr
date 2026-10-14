---
name: godot-headless-runs
description: >-
  Run Godot 4 tools, probes, bots and checks headless so that they compile, end by themselves,
  report errors honestly and never touch the player's save. It covers `godot -s` SceneTree tool
  scripts (no class names, autoloads by node path, a quit watchdog), the import and parse check
  after a merge or a new worktree, a throwaway check project when the real one cannot load,
  save and settings isolation for tool runs, a dev task runner, time limits, log scans and
  process cleanup. Use it whenever someone writes or runs a `godot --headless` or `godot -s`
  script, or asks why a tool says "Identifier not found", "Could not find type", "Nonexistent
  function 'new'" or "no resource loaders", why a headless run hangs or gives silent wrong
  numbers, why tests fail after a merge, or how to keep bots and tests off the real save, even
  if nobody says "headless".
---

# Headless Godot runs

Target engine: **Godot 4.7**. The GDScript blocks parse on 4.7.2. The shell blocks are POSIX `sh`.

A headless Godot tool fails in quiet ways. A `godot -s` script compiles before the autoloads
exist, so 1 class name in it can stop a script from compiling, and the run still exits 0 with
wrong numbers. A stale import cache, a hang after a script error or a shared save file gives
the same kind of quiet fault. The method is a short set of rules for the tool script, plus a
run wrapper that adds a hard time limit, a log file and a scan for error lines. It works
because each rule closes 1 failure that the engine does not turn into an exit code.

## When to use

- You write a SceneTree tool: a bot, a measure probe, a capture, a bench, a fixture maker, a
  data export or a parse check.
- A headless tool or test fails with a compile error, a parse error, a hang or a value that
  looks wrong.
- A new worktree, a merge, a branch switch, a new `class_name` or new art reaches a checkout.
- Tests, bots or captures start the real game on a machine where a person also plays it.
- Several headless runs share 1 machine or 1 `user://` folder.

## When not to use

- Window-only work: captures, real frame timings, shader looks. `frame_post_draw` never fires
  headless and the headless renderer is a dummy. Use `godot-visual-verification` or `godot-perf-measure`.
- The test loop itself (filters, test quality, the finish gate). Use `godot-test-loop`.
- Locks, slot pools and worktree landing across many agents. Use `godot-fleet-game-dev`.
- The verify loop before DONE (find the engine, build C#, smoke-run the main scene). Use
  `godot-build-verify`.

## Method: write and run a SceneTree tool

1. Start from the tool template in
   [references/script-tool-template.md](references/script-tool-template.md); copy the block into a file. Write the entry as `extends SceneTree`. Name no
   `class_name` script and no autoload in it, and keep game objects in untyped variables.
   Check: the file has no game class name and no autoload name outside a string.
   Why: a `-s` script compiles before the autoloads exist, so a name that needs them fails.
2. Do no game work in `_initialize()`. Defer it to the first frame. If the game reads a save
   path at start-up, set the scratch save slot before the run (see [references/tool-save-isolation.md](references/tool-save-isolation.md)).
   Check: the work starts from a `process_frame` callback or later.
3. Reach each autoload at run time with `root.get_node("<Name>")` and `call()`. Load game
   scripts with `load("res://...")` at run time, not with `preload()` or a class name. Use
   enum numbers, not `SomeClass.State.VALUE`.
   Check: grep the tool for `preload(` and for each autoload name followed by `.`.
   Why: a tool that names a game enum can stop the game script itself from compiling.
4. For a larger tool, split it. A thin `-s` loader instances a stage `Node` script after
   start-up, and the stage uses the game normally because it compiles after the autoloads.
   Check: the loader holds no game names; only the stage does.
5. Give the tool its own quit watchdog, and wrap the run in a hard alarm as well.
   Check: a run with a forced endless wait ends with `TOOL_TIMEOUT` and a nonzero code.
   Why: a script error stops the normal `quit()`, and the run hangs.
6. Keep tools in a tools or review folder, never as `test_*.gd` in the test folder. Run them
   with `-s <absolute path>`.
   Check: the test runner does not list the tool.
   Why: a test runner runs every `test_*.gd` file. A tool outside the project needs no `.uid`.
   If a tool must be under `res://`, copy it in for the run and remove it after.
7. Parse-check the tool before its first real run. Run it once on a tiny input, or run the
   project parse check
   ([references/script-parse-check.md](references/script-parse-check.md); copy the block into a file) when the tool is under `res://`.
   Check: the log has no `SCRIPT ERROR` line.
   Why: `--check-only` has no autoloads, reports false "Identifier not found" errors, and can
   exit 0 on a real parse error.
8. Run the tool through `sh godot-run.sh <seconds> <log> godot --headless ...`. Copy the block in
   [references/script-godot-run.md](references/script-godot-run.md) into a file. It writes
   the log, stops the run at the alarm, and fails on error lines.
   Check: the exit code is 0 and the log holds the summary line you set in
   `GODOT_RUN_REQUIRE`. A sweep or calibration marks a run invalid on any `SCRIPT ERROR`.
   Why: a tool that failed to compile still exits 0 and prints plausible but empty results.
9. Run the import after a new `class_name`, a merge, a branch switch or new art (see
   [references/import-and-parse.md](references/import-and-parse.md)). Then rerun the tool.
10. Send fake input with `Input.parse_input_event()` and a real event, not a direct method
    call. Await 1 or 2 frames after you move an object before a staged call reads its
    position. In a SceneTree script, connect `node_added` on `self`. Guard calls that need a
    real display server with a feature check.
    Check: the game reacts to the event in the log, the same as for a player.
11. Apply the GDScript type rules that bite tools most:
    - A typed constant like `Array[PackedStringArray]` does not convert inner literals. Use
      plain nested Arrays and an `Array` loop variable.
    - A `const` cannot hold a function call. Use a `var` or a static function.
    - A packed array inside a dictionary is a value copy. Write it back or return it.
    - Write an explicit type where `:=` cannot infer one, for example a value from an untyped
      dictionary.
    - `PackedStringArray` has no `map()`. Use a loop.
    Check: the parse check passes and the tool prints non-empty values.

## Import and parse hygiene

A new worktree, a merge or a new `class_name` leaves the `.godot` import cache missing or stale.
Read [references/import-and-parse.md](references/import-and-parse.md) for the 12 import steps
and for the isolated check project, the fallback when the real project cannot load.

## Tool save isolation

Autoloads read the save and the settings before a `-s` script runs. Read
[references/tool-save-isolation.md](references/tool-save-isolation.md) for the run-mode check,
the slot from an environment variable, and the save-hash proof.

## Run hygiene

Read [references/run-hygiene.md](references/run-hygiene.md) for the 13 rules on alarms, logs,
exit codes, test filters, determinism flags, priority and process cleanup.

## A task runner for dev commands

Read [references/task-runner.md](references/task-runner.md) to put every headless command in 1
`justfile` or `Makefile`, with the time limit and the log scan inside the recipes.

## Pitfalls

The 8 most common rows are below. The full table of 57 rows is in
[references/pitfalls.md](references/pitfalls.md).

| Symptom | Cause | Fix |
| --- | --- | --- |
| Bots play the whole game but a scripted event never happens; every result is wrong for a day | 1 class name in the tool script stopped a script from compiling; the run still exited 0 | Load scripts with `load()` at run time; mark a run invalid on any `SCRIPT ERROR` line |
| "Identifier not found: <Autoload>" in a `-s` script | Autoloads are not compile-time names in a SceneTree script | `root.get_node("<Name>")` and `call()` |
| A tool hangs until the alarm | A script error stopped `quit()` | A watchdog timer inside the tool, and a hard alarm outside it |
| A run hangs 25 minutes at about 1 % CPU | No `.godot` import cache in a new worktree | Stop your own process, import, rerun with narrow filters |
| About 50 to 70 tests fail on unknown classes after a merge | Stale class cache | Run the import, then the tests |
| `--check-only` exits 0 on a broken script | Check-only does not set the exit code | Use the parse check, which exits 1 |
| A capture wrote the player's save | It started the game without `-s` | Treat script mode, test mode and worktree paths as tool runs |
| The exit code says pass but the run failed | `$?` came from `tail` in a pipe | A log file or `pipefail` |

## Artifacts to build

- A SceneTree tool template: root lookups, run-time `load()`, a quit watchdog, a done line.
  [references/script-tool-template.md](references/script-tool-template.md) is a start.
- A project parse check: loads every `.gd`, `.tscn` and `.tres`; prints
  `checked=N failed=M`; exits 1 on a failure. [references/script-parse-check.md](references/script-parse-check.md) is a start.
- A run wrapper: hard alarm, log file, error-line scan, a required summary line.
  [references/script-godot-run.md](references/script-godot-run.md) is a start.
- A process lister that marks your own Godot processes by path.
  [references/script-godot-procs.md](references/script-godot-procs.md) is a start. Each
  script reference ends with a check of the copy.
- A run-mode helper in the game: script mode, test mode, worktree path, and the tool save and
  settings paths from 1 environment variable.
- A test path helper: a per-process scratch folder and save, deleted at exit.
- A save-hash check: hash the player's save before and after a batch.
- A list of invalid-run markers that every sweep shares (`SCRIPT ERROR`, `Parse Error`,
  "Identifier not found").
- A task runner file (`justfile` or `Makefile`) plus a preflight script.
- A checklist for new worktrees: import, parse check, baseline tests, `.uid` files.

## Related skills

- `godot-test-loop`: the headless test loop, flake isolation and the finish gate. This skill
  makes the runs safe; that one makes the tests good.
- `godot-fleet-game-dev`: the slot runner that caps parallel runs, the `mkdir` locks, and the rules
  for landing `.uid` files and merges.
- `godot-bot-balance-loop`: bot fights and measure probes, the main users of SceneTree tools and the
  determinism flags.
- `godot-visual-verification`: window captures and visual gates, which cannot run headless.
- `godot-perf-measure`: frame timings in a real window, and the shader warm-up list that a parse check
  can guard.
- `godot-feature-seams-and-saves`: save versions and save integrity in the shipped game.
- `godot-release-and-store`: export and demo build recipes for the task runner.
- `godot-build-verify`: the verify loop before DONE, with its own import and parse check steps.

## References

Read each file when its case applies.

- [references/import-and-parse.md](references/import-and-parse.md): after a merge or a new
  worktree, or when the real project cannot load.
- [references/tool-save-isolation.md](references/tool-save-isolation.md): a tool starts the
  real game.
- [references/run-hygiene.md](references/run-hygiene.md): before a batch and before done.
- [references/task-runner.md](references/task-runner.md): 1 runner file for dev commands.
- [references/pitfalls.md](references/pitfalls.md): search it for an error text or a symptom.
- [references/script-tool-template.md](references/script-tool-template.md): a new SceneTree
  tool.
- [references/script-parse-check.md](references/script-parse-check.md): the project parse
  check.
- [references/script-godot-run.md](references/script-godot-run.md): any headless command.
- [references/script-godot-procs.md](references/script-godot-procs.md): before you stop a
  process or report done.
