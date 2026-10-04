# Godot commands (4.7)

Every line below was run on Godot 4.7.2.stable.official (macOS, 2026-10-04).
Run them from the project root, the directory that holds `project.godot`.
Quote paths: project paths often contain spaces.

## Find the engine

```bash
GODOT="${GODOT_PATH:-}"
if [ -z "$GODOT" ]; then GODOT="$(command -v godot || true)"; fi
if [ -z "$GODOT" ] && [ -x /Applications/Godot.app/Contents/MacOS/Godot ]; then
  GODOT=/Applications/Godot.app/Contents/MacOS/Godot
fi
[ -n "$GODOT" ] && [ -x "$GODOT" ] || { echo "NO_ENGINE: ${GODOT:-not found}"; exit 2; }
have="$("$GODOT" --version | cut -d. -f1-2)"
want="$(grep '^config/features=' project.godot | grep -oE '"[0-9]+\.[0-9]+"' | head -1 | tr -d '"')"
echo "engine=$GODOT have=$have want=${want:-none}"
if [ -n "$want" ] && [ "$(printf '%s\n%s\n' "$want" "$have" | sort -t. -k1,1n -k2,2n | tail -1)" != "$have" ]; then
  echo "REFUSE: project wants Godot $want, engine is $have"; exit 3
fi
```

- Exit 2: no engine. Send `QUESTION:` with what you found. Do not search the disk.
- Exit 3: the project is newer than the engine. Report `BLOCKED:`.
- `--version` prints `4.7.2.stable.official.ed1daf0bf`; the script compares
  major.minor only.
- A C# project needs the .NET build to run C#: on macOS,
  `/Applications/Godot_mono.app/Contents/MacOS/Godot` (not installed on the test
  host; not run). Measured: the standard build prints `No loader found for
  resource: res://Player.cs` for a scene with a C# script, and exits 0.

## Common setup

```bash
GH="$PWD/.godot/horch-home"
LOGS="$GH/logs"
mkdir -p "$LOGS"
gd() { HOME="$GH" "$GODOT" --headless --path "$PWD" "$@"; }
```

Measured on macOS: Godot ignores `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and
`XDG_CACHE_HOME`, and writes under `$HOME/Library/Application Support/Godot/`
(`editor_settings-4.7.tres`, `app_userdata/<project name>/`) and
`$HOME/Library/Caches/Godot/`. With `HOME="$GH"`, nothing is written to the
real user directory. Export templates live under the real `HOME`; an export
run (not covered here) needs them.

## Import

```bash
gd --import > "$LOGS/import.log" 2>&1; echo "import exit=$?"
```

- Exit 0 on success. A fresh 3-file project imports in about 2 s; a large
  project takes minutes: run it in the background and poll the log.
- Creates `.godot/` and a `<script>.gd.uid` sidecar next to each script that
  has none.
- It does not add a `uid=` to a `.tscn` header (see `godot-scene-files`).

## Parse check

```bash
# write references/parse-check.md's script to "$GH/parse_check.gd" first
gd -s "$GH/parse_check.gd" > "$LOGS/parse.log" 2>&1; echo "parse exit=$?"
gd -s "$GH/parse_check.gd" -- --skip=addons > "$LOGS/parse.log" 2>&1   # skip folders by name
```

Exit 1 with `PARSE_CHECK FAIL <path>` lines; exit 0 with
`PARSE_CHECK checked=<n> failed=0`.

Do not use `--check-only`. Measured: `gd --check-only --script res://bad.gd`
prints `SCRIPT ERROR: Parse Error: ...` and exits 0.

## C# build

```bash
dotnet build > "$LOGS/dotnet.log" 2>&1; echo "dotnet exit=$?"
```

- Exit 0 with `Build succeeded.` and `0 Error(s)`. Exit 1 on a compile error,
  with lines `Player.cs(7,18): error CS0103: ...`.
- The `.csproj` names `Sdk="Godot.NET.Sdk/4.7.2"`; the first build downloads it
  from NuGet (network needed). The output is `.godot/mono/temp/bin/Debug/`.
- Measured with .NET SDK 10.0.101 and `<TargetFramework>net8.0</TargetFramework>`.

## Tests

GUT 9.7.1 (the release for Godot 4.7.x):

```bash
gd -s res://addons/gut/gut_cmdln.gd -gdir=res://test -ginclude_subdirs -gexit -gdisable_colors > "$LOGS/gut.log" 2>&1; echo "gut exit=$?"
```

| result | exit | output |
|---|---|---|
| all pass | 0 | `---- All tests passed! ----` |
| a test fails | 1 | `[Failed]:` lines, `---- 1 failing tests ----` |
| no test found | 0 | `[GUT ERROR]:  Nothing was run.` |
| a test script does not parse | 0 | `SCRIPT ERROR: Parse Error`; the script is skipped and the others still report `All tests passed!` |

Without `-gexit`, GUT does not quit. Read the `Totals` block for counts.

gdUnit4 6.2.1 (supports Godot 4.5 to 4.7.1 by its README; run on 4.7.2):

```bash
gd -s res://addons/gdUnit4/bin/GdUnitCmdTool.gd -a res://test --ignoreHeadlessMode -rd res://.godot/horch-home/gdunit-reports > "$LOGS/gdunit.log" 2>&1; echo "gdunit exit=$?"
```

| result | exit | output |
|---|---|---|
| all pass | 0 | `Overall Summary: N test cases \| 0 errors \| 0 failures` |
| a test fails | 100 | `Exit code: 100` |
| tests pass but leave orphan nodes | 101 | `Exit code: 101`, `orphans` count above 0 |
| `--ignoreHeadlessMode` missing | 103 | `Abnormal exit with 103` |
| a test script does not parse | 105 | `Abnormal exit with 105` |
| no test found | 0 | `No test cases found, abort test run!` |

Older notes give 0/1/2 for gdUnit4; 6.2.1 does not use those codes. Treat 101
as a failure to fix (free the node, or `auto_free()` it), not as a pass.
`-a` takes a directory or a test script; repeat it for more paths. Without
`-rd`, gdUnit4 writes `reports/` into the project root: never commit that.
Give `-rd` a `res://` path. Measured: an absolute path such as `/Users/me/x`
is joined to the project root and creates `Users/me/x` inside the project.

## Smoke run

```bash
gd --quit-after 120 > "$LOGS/smoke.log" 2>&1; echo "smoke exit=$?"
gd --quit-after 120 --scene res://levels/level_1.tscn > "$LOGS/smoke.log" 2>&1
```

`--quit-after <n>` quits after n main-loop iterations; 120 took about 2 s
headless. Measured: a `_process` that calls a method on `null` prints
`SCRIPT ERROR: Cannot call method 'queue_free' on a null value.` and the run
still exits 0.

## Grep

```bash
grep -n -E 'SCRIPT ERROR|Parse Error|^ERROR:|^WARNING:|PARSE_CHECK FAIL' "$LOGS"/*.log
```

Strings seen on 4.7.2:

| string | meaning |
|---|---|
| `SCRIPT ERROR: Parse Error: <message>` then `at: GDScript::reload (res://x.gd:<line>)` | a script does not parse |
| `ERROR: Failed to load script "res://x.gd" with error "Parse error".` | the same, from the loader |
| `SCRIPT ERROR: <message>` then `at: <function> (res://x.gd:<line>)` | a runtime error |
| `ERROR: Parse Error: Parse error. [Resource file res://x.tscn:<line>]` | a `.tscn` or `.tres` does not parse |
| `ERROR: res://x.tscn:<line> - Parse Error: [ext_resource] referenced non-existent resource at: <path>` | a scene names a missing file; the scene still loads |
| `WARNING: Parent path './X' for node 'Y' has vanished when instantiating` | a node's `parent=` is wrong; the scene still loads |
| `[GUT ERROR]:  Nothing was run.` | GUT found no test |
| `ERROR: No loader found for resource: res://Player.cs (expected type: Script)` | the standard (non-.NET) build met a C# script; the run still exits 0 |

Godot prints colour codes when stdout is a terminal; redirecting to a file, as
above, removes most of them. gdUnit4 colours its log; strip with
`sed 's/\x1b\[[0-9;]*m//g'` before you grep.
