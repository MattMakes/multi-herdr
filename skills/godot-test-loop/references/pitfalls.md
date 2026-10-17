# Pitfalls: the full table

| Symptom | Cause | Fix |
| --- | --- | --- |
| Hundreds of failures after a merge | Stale import or class cache | Import, then run again |
| Failures from conflict markers | Suite ran on an unresolved merge | Resolve the merge first; discard the result |
| Runner green with 0 tests | No skip state; no 0-test rule | Count skips; exit 1 on 0 tests |
| Filtered run ran the full suite, or 0 tests | Wrong flag or a comma list | Check the passed count before you report |
| 1 bad file fails dozens of tests | Typed var with an untyped value | Parse check names the file; add a type |
| Test file does not load | Class name not in scope | Add a `preload` const in the test |
| Whole test file fails to parse | Multi-line lambda, bad string escape | Plain helper function; plain string |
| Wrong helper name | Guessed API (`assert_equal`) or clash with base | Read the base class; unique helper names |
| Async helper did nothing | Called without `await` | `await` it |
| Node name compare fails | `name` is a `StringName` | Compare `String(name)` |
| Local var acts odd | It shadows a node property (`scale`) | Rename the local |
| Inner class cannot call outer statics | GDScript scope rule | Pass values in from the outer code |
| Packed array edit has no effect | You edited a copy | Assign the whole array back |
| `node.get("CONST")` returns null | `get` does not read script constants | Read the script's constant map |
| JSON number compare fails | JSON loads numbers as float | Compare as float or cast on load |
| Float compare fails | float32 loops, darkened colours | Tolerance about 1e-3, or 1/255 for colours |
| Angle test fails near a wrap | Angle wraps (for example -90 to 270) | Compare with `fposmod` |
| Recolour changed other sprites | `Texture.get_image()` shares the image | Recolour a copy |
| Message queue overflow | Thousands of steps with no idle frame | Let 1 frame pass per second of steps |
| HUD crash in a test | Huge test capacity built 1 tick per unit | Zero the drain instead |
| Scenario ends before the check | Test damage too large after a data change | Keep test inputs small |
| Test passes for the wrong reason | Wrong reference point | Step frame by frame; measure at the event |
| Rule never exercised | Inputs reach it rarely | Add inputs that hit the rule; prove with a mutant |
| Test stops before the event | Time cap on the loop | Cap the step count |
| Position wrong at frame 0 | Camera or spawn not settled | Wait for the event; add a 1-tick margin |
| 2 events in 1 frame | 1 replaced the other | Fire them in separate frames |
| Object always moving hides the effect | No control case | Compare a twin in and out of the effect |
| Capture hangs headless | Headless never draws | Skip headless; free the scene on wait |
| Dummy renderer returns no instance data | Logic lives in render code | Move it to a pure function; test that |
| Test reads stale log | File not flushed | `flush()` before the read |
| World vs local units mix | Test used world coordinates | A helper reads spots from world data |
| Test sends telemetry | Demo scene logs in tests | Turn telemetry off in test runs |
| Tool ran with the suite | Tool file had the `test_` prefix | Rename the tool |
| Base copy fails with missing file | Partial copy | Copy every folder the check reads |
| Test value looks like a code bug | Wrong expectation | Read the real rule; fix the test value |
| Suite prints nothing on macOS | `timeout` is not installed | `perl -e 'alarm N; exec @ARGV' ...` |
| Glob not expanded in grep | zsh glob in `--include=*.gd` | Quote the glob |
| Random test fails 1 in 5 | Unseeded random draw | Seed the draw |
| Check has a 1-frame margin | Timers run before tweens; fast headless frames | Wait on timers; wait for tween end |
| Next test sees old tuning | Static var leaked | Restore it in `after_each` |
| Save tests fail in every run | Another branch wrote a newer save version | Per-run scratch saves |
| Focus test flakes 11 of 20 | Selection by dictionary order | Pick by a stable rule; test the tie |
| Bugs reached the build | Lands ran only named folders | Full suite at verify |
| `/tmp` copy fails to start | No or stale imported cache | Import, then copy |
| Window gate fails once, passes next | Known gate flake | Log it; run twice; fix the gate later |
| Skips look like passes | Runner counts skips as passes | Count skips separately |
| Fail-safe path not checked | Runner ignores warnings | Count warnings in that test |
| Output folder ignored or unwritable | Ignore rule or sandbox | Write under the tests folder with `.gdignore` |
| Game loads a backup save | Test save missing | Write the test save in setup |
| Leak count did not change | Wrong suspect (free, type refs) | Self-typed static cycle; return untyped |
| Second tap lost | No frame between release and press | 1 frame after release |
| HUD tests fail in suite only | Pad test left device family set | Reset input state in `after_each` |
| Rewrite lost a real check | Audit dropped an assert | Add it back; prove it with a break |
| Data change breaks row tests | Exact row counts | Assert non-empty and unique |
| Rewrite fails on valid rows | Optional field empty by design | Read the data contract |
| Hidden changes after a mutant | Hand edit to restore | Restore from copy; compare a hash |
| Backup files left in tests | Save rotation in a test | Clean in `after_each`; check `git status` |
| Timing test fails under load only | Machine load | Record load; rerun alone |
| Test passes under 1 filter only | Order dependence | Run under 2 filters and on the base |
| Wrong screen state on open | Hover from a resting pointer | Take mouse focus only on real motion |
| Process check finds false hits | `pgrep -f godot` matched shells | Match the real binary and its folder |
| Base moved during the work | Long unit | Merge again; repeat the gate |
| Generator wrote outside scope | Model run side effect | `git checkout` that file |
| Merge blocked by a `.uid` file | Import made a local copy | Delete the local copy |
| Merge blocked by a rename | Staged rename not committed | Commit the rename first |
| Diff shows other work | Base tip moved after the merge | Diff against the commit you merged |
| New guard test fails on your files | Merge brought a new rule | Ask the owner before you edit outside scope |
| Test in another unit's file breaks | Change by design | Ask the owner before you edit it |
| Hung run at full CPU | 1 test never ends | Find it by process and last log line |
| Out-of-scope stale test found | Parse check reads everything | Report it; ask for ownership of that file |
