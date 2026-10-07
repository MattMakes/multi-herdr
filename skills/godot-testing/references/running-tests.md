# Running Tests

Reference for `skills/godot-testing/SKILL.md` — GUT CLI, gdUnit4 CLI, GitHub Actions CI workflow.

> ← Back to [SKILL.md](../SKILL.md)

---
## Running Tests

### GUT CLI

Commands and exit codes below were measured with GUT 9.7.1 on Godot 4.7.2
(the same facts as **godot-build-verify**, `references/commands.md`).

`gd` runs Godot through `godot-build-verify`'s wrapper; see its `references/commands.md`, Common setup.

```bash
# Run all tests in res://tests and its subdirectories, then quit
gd -s addons/gut/gut_cmdln.gd -gdir=res://tests -ginclude_subdirs -gexit

# Run a specific directory
gd -s addons/gut/gut_cmdln.gd -gdir=res://tests/unit -gexit

# Run a specific file
gd -s addons/gut/gut_cmdln.gd -gtest=res://tests/unit/test_health_component.gd -gexit

# Verbose output with a JUnit XML report
gd -s addons/gut/gut_cmdln.gd -gdir=res://tests -gexit -glog=3 -gjunit_xml_file=res://test_results/gut.xml
```

- Without `-gexit`, GUT does not quit.
- Exit 0 when all tests pass, exit 1 when a test fails.
- Exit 0 when a test script does not parse: GUT prints
  `SCRIPT ERROR: Parse Error`, skips the script and still reports
  `All tests passed!` for the others. Exit 0 also when no test is found
  (`[GUT ERROR]:  Nothing was run.`). So the exit code alone does not
  prove the tests ran: grep the log for `SCRIPT ERROR` and
  `Nothing was run` too.

### gdUnit4 CLI

Commands and exit codes below were measured with gdUnit4 6.2.1 on Godot
4.7.2. The runner is `addons/gdUnit4/bin/GdUnitCmdTool.gd`; `-a` takes a
directory or a test-suite file.

```bash
# Run all tests
gd -s res://addons/gdUnit4/bin/GdUnitCmdTool.gd -a res://tests --ignoreHeadlessMode

# Run a specific directory
gd -s res://addons/gdUnit4/bin/GdUnitCmdTool.gd -a res://tests/unit --ignoreHeadlessMode

# Run a specific test file
gd -s res://addons/gdUnit4/bin/GdUnitCmdTool.gd -a res://tests/unit/test_health_component.gd --ignoreHeadlessMode

# Run every test after a failure (the default stops at the first failure), with reports
gd -s res://addons/gdUnit4/bin/GdUnitCmdTool.gd -a res://tests --ignoreHeadlessMode -c -rd res://reports
```

| Result | Exit |
|---|---|
| all tests pass | 0 |
| no test found | 0 |
| a test fails | 100 |
| tests pass but leave orphan nodes | 101 |
| `--ignoreHeadlessMode` missing under `--headless` | 103 |
| a test script does not parse | 105 |

- Older notes give 0/1/2 or 0/1 for gdUnit4; 6.2.1 does not use those
  codes. Treat any non-zero exit as a failed run, and 101 as a leak to fix.
- `-rd` needs a `res://` path. An absolute path creates folders inside the
  project. Without `-rd`, reports go to `res://reports/`: add it to
  `.gitignore`.
- An exit of 0 with no tests found is not a pass: check the log for the
  test count.
- C# test suites run through the gdUnit4Net `dotnet test` adapter, not
  through `GdUnitCmdTool.gd`. Pin `gdUnit4.api` 5.0.0 with
  `gdUnit4.test.adapter` 3.0.0 and set `GODOT_BIN` in a `.runsettings` file.
  Run `dotnet test --settings .runsettings`; a test that needs the engine
  carries `[RequireGodotRuntime]`. gdUnit4Net states no Godot 4.7 support
  (README: 4.3.0 to 4.4.1) and names GodotSharp 4.4.0.
  proof: run on 2026-10-07 on Godot .NET 4.7.2 with those 2 versions: 1 test
  passed (exit 0) and 1 test failed on purpose (exit 1)
  (`docs/live-checks/godot-csharp.md`, steps `gdunit4net-pass` and
  `gdunit4net-fail`).
  A host whose only .NET runtime is newer than the test target (for example
  .NET 10 with `net8.0`) needs `DOTNET_ROLL_FORWARD=Major` for `dotnet test`,
  or the test host stops; proof: `scripts/godot/live_csharp.sh` exports it
  and its 3 gdUnit4Net steps passed on .NET 10.0.101.
- gdUnit4 `-c` and GUT `-gjunit_xml_file` come from the addon help text.
  proof: not run (needs the addons in a project).

### GitHub Actions CI

```yaml
# .github/workflows/tests.yml
name: Tests

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

jobs:
  test-gut:
    name: GUT Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Godot
        uses: chickensoft-games/setup-godot@v2
        with:
          version: 4.7.2  # GUT 9.7.1 is the release for Godot 4.7.x; gdUnit4 6.2.1 is for 4.5 or later
          use-dotnet: false

      - name: Import project
        run: godot --headless --import 2>&1 | tail -5

      - name: Run GUT tests
        # GUT exits 0 when a test script does not parse; fail on the log too.
        # No pipe and no PIPESTATUS (bash only, empty in zsh); `if` because
        # `! grep` does not stop a `-e` shell.
        run: |
          code=0
          godot --headless -s addons/gut/gut_cmdln.gd -gdir=res://tests -ginclude_subdirs -gexit -glog=2 -gdisable_colors > gut.log 2>&1 || code=$?
          cat gut.log
          if grep -E 'SCRIPT ERROR|Parse Error|Nothing was run' gut.log; then exit 1; fi
          test "$code" -eq 0

  test-gdunit4:
    name: gdUnit4 Tests (GDScript + C#)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Godot with .NET
        uses: chickensoft-games/setup-godot@v2
        with:
          version: 4.7.2  # GUT 9.7.1 is the release for Godot 4.7.x; gdUnit4 6.2.1 is for 4.5 or later
          use-dotnet: true

      - name: Restore NuGet packages
        run: dotnet restore

      - name: Import project
        run: godot --headless --import 2>&1 | tail -5

      - name: Run gdUnit4 tests
        # Exit 0 pass, 100 fail, 101 orphans, 103 headless refusal, 105 broken script.
        run: >
          godot --headless
          -s res://addons/gdUnit4/bin/GdUnitCmdTool.gd
          -a res://tests
          --ignoreHeadlessMode
          -c
          -rd res://reports

      - name: Upload test report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: test-report
          path: reports/
```

---

