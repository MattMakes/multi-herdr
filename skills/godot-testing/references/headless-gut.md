# GUT for a headless worker

Install GUT without the editor, configure it, run it with a script that
trusts the log and not only the exit code, and reach autoloads from tests.
Everything here was measured with GUT 9.7.1 on Godot 4.7.2 (macOS, headless,
2026-10-07), on a new project and on the Coin Dash trial project.

## Install without the editor

GUT 9.7.1 is the release for Godot 4.7.x. Download the tag tarball into an
empty directory of its own, and copy only `addons/gut/` into the project:

```bash
mkdir -p /tmp/gut-dl
curl -sSL -o /tmp/gut-dl/gut.tar.gz https://github.com/bitwes/Gut/archive/refs/tags/v9.7.1.tar.gz
mkdir -p addons
tar -xzf /tmp/gut-dl/gut.tar.gz -C addons --strip-components=2 Gut-9.7.1/addons/gut
grep '^version=' addons/gut/plugin.cfg   # version="9.7.1"
```

- The command line runner `addons/gut/gut_cmdln.gd` does not need the
  plugin. Do not enable it in `project.godot`: measured, a project with no
  `[editor_plugins]` section runs the suite. The "enable in Project Settings
  > Plugins" step in `gut-reference.md` is for the editor panel only.
- The tarball holds the `.uid` sidecars of the addon scripts. Run `--import`
  before the first test run: `GutTest` is a `class_name`, and the import
  writes the global class cache.

## Config file

GUT reads `res://.gutconfig.json` when no `-gconfig` is given (measured: the
same run with and without `-gconfig=res://.gutconfig.json` found the same 4
tests). The QA seat's file, `.gutconfig.json` in the project root:

```json
{
  "dirs": ["res://tests/unit", "res://tests/integration"],
  "include_subdirs": true,
  "prefix": "test_",
  "suffix": ".gd",
  "should_exit": true,
  "should_exit_on_success": true,
  "log_level": 1,
  "disable_colors": true
}
```

`should_exit` is the file form of `-gexit`: without it, GUT does not quit.
Create every directory in `dirs`. Measured: a missing one prints
`[GUT ERROR]:  The path [res://tests/integration] does not exist.`, and the run
goes on and exits 0. `run_gut.sh` below does not catch that line.

GUT prints ANSI colour codes in `GUT ERROR` lines even with
`"disable_colors": true` (measured: the log holds `ESC[31m[GUT ERROR]:  ESC[0m`
before the text). A log check must strip them first, for example
`sed 's/\x1b\[[0-9;]*m//g'`, or match a plain substring such as `does not exist`.

## Run script

`gd` runs Godot through `godot-build-verify`'s wrapper; see its `references/commands.md`, Common setup.

GUT exits 0 when a test script does not parse (it skips the script) and when
no test is found. Save this as `tests/run_gut.sh` and run it from any shell
(it runs under bash through its first line; it uses no pipe, so it needs no
`PIPESTATUS`):

```bash
#!/usr/bin/env bash
# Runs the GUT suite headless. Exit 0 only when tests ran and all passed.
# GUT exits 0 when a test script does not parse or no test is found, so the
# script also greps the log. Run it with bash, from any shell (zsh included).
set -u
cd "$(dirname "$0")/.." || exit 2
RUN="<skill dir>/scripts/godot-run.sh"
gd() { bash "$RUN" --path "$PWD" "$@"; }
GH="$PWD/.godot/horch-home"
LOG="$GH/logs/gut.log"
mkdir -p "$GH/logs"

gd --import > "$GH/logs/import.log" 2>&1
gd -s res://addons/gut/gut_cmdln.gd \
  -gconfig=res://.gutconfig.json > "$LOG" 2>&1
code=$?
if grep -Eq 'SCRIPT ERROR|Parse Error|Nothing was run' "$LOG"; then
  grep -nE 'SCRIPT ERROR|Parse Error|Nothing was run' "$LOG"
  echo "GUT_RUN FAIL: script error or no test ran (gut exit=$code); log: $LOG"
  exit 3
fi
grep -E '^(Scripts|Tests|Passing|Failing|Asserts)' "$LOG"
echo "GUT_RUN exit=$code log: $LOG"
exit "$code"
```

Measured, called from zsh and from bash:

| case | GUT exit | script exit | last line |
|---|---|---|---|
| 4 tests pass | 0 | 0 | `GUT_RUN exit=0 log: ...` |
| 1 test fails | 1 | 1 | `GUT_RUN exit=1 log: ...` |
| a test script does not parse | 0 | 3 | `GUT_RUN FAIL: script error or no test ran (gut exit=0); ...` |
| no test in the directories | 0 | 3 | the same, after `[GUT ERROR]:  Nothing was run.` |

The import line keeps the `.uid` files and the class cache current. The
private `HOME` comes from **godot-build-verify** (step 3). A CI job that
cannot run the script: see the GUT job in `references/running-tests.md`.

## Reach an autoload from a test

Get the autoload by its node path with `get_node_or_null()`. Do not name the
autoload identifier in the test. The block needs GUT, so the skill's
GDScript check skips it; it ran as written with GUT 9.7.1 (1 test, 3 asserts,
passed):

<!-- gdscript-check: skip -->
```gdscript
extends GutTest
# Reaches the autoload by its node path. A test that names GameState directly
# does not parse when the autoload is missing; this one fails with an assert.

var gs: Node


func before_each() -> void:
	gs = get_node_or_null("/root/GameState")
	assert_not_null(gs, "GameState autoload is registered")
	if gs:
		gs.score = 0


func test_add_score_adds_and_emits_once() -> void:
	watch_signals(gs)
	gs.add_score(5)
	assert_eq(gs.score, 5)
	assert_signal_emit_count(gs, "score_changed", 1)
```

Measured with the `[autoload]` line removed from `project.godot`:

- this test fails with `[Failed]:  Expected [<null>] to be anything but NULL:
  GameState autoload is registered`, and GUT exits 1. The calls on `null`
  after the assert also print `SCRIPT ERROR` lines, so `run_gut.sh` exits 3;
- a test that names `GameState.score` prints `SCRIPT ERROR: Parse Error:
  Identifier "GameState" not declared in the current scope.`, and GUT skips
  the whole script.

GUT runs as a `-s` script, but the autoloads exist when the tests run: with
the autoload present, `get_node_or_null("/root/GameState")` returns the node
and the tests pass. Reset the autoload's state in `before_each()`: the same
instance lives for the whole run.

## Wait for frames

`wait_frames()` is deprecated in GUT 9.7.1. It prints `[DEPRECATED]:
wait_frames has been replaced with wait_physics_frames which is counted in
_physics_process.  wait_process_frames has also been added which is counted
in _process.` Use `await wait_physics_frames(n)` for physics and
`await wait_process_frames(n)` for `_process` work. Measured: after
`await wait_physics_frames(5)`, `Engine.get_physics_frames()` grew by at
least 5; the same holds for `wait_process_frames(5)` and
`Engine.get_process_frames()`.
