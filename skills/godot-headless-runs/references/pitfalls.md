# Pitfalls of headless runs

The full table of symptoms, causes and fixes. Search it for the error text or the symptom that you see.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Bots play the whole game but a scripted event never happens; every result is wrong for a day | 1 class name in the tool script stopped a script from compiling; the run still exited 0 | Load scripts with `load()` at run time; mark a run invalid on any `SCRIPT ERROR` line |
| "Identifier not found: <Autoload>" in a `-s` script | Autoloads are not compile-time names in a SceneTree script | `root.get_node("<Name>")` and `call()` |
| A game script stops compiling after a tool change | The tool named a game enum (`SomeClass.State.X`) | Use the enum numbers in the tool |
| A tool hangs until the alarm | A script error stopped `quit()` | A watchdog timer inside the tool, and a hard alarm outside it |
| A run hangs 25 minutes at about 1 % CPU | No `.godot` import cache in a new worktree | Stop your own process, import, rerun with narrow filters |
| About 50 to 70 tests fail on unknown classes after a merge | Stale class cache | Run the import, then the tests |
| "Could not find type" after a branch switch or a new `class_name` | The class is not registered yet | Run the import |
| "no resource loaders" for new audio or images | Scripts loaded before the new files imported | Run the import a second time |
| A batch gives empty logs after a merge | The import did not run before the batch | Import inside the batch script |
| Textures load at the old size after an art change | Stale import cache | Run the import |
| `--check-only` says "Identifier not found" on a correct script | Check-only has no autoloads | Use the project parse check or the related tests |
| `--check-only` exits 0 on a broken script | Check-only does not set the exit code | Use the parse check, which exits 1 |
| A test script's tests silently do not run | The script failed to compile: `:=` could not infer a type | Run the script alone; add an explicit type |
| The parse check misses a tool | The tool lives outside `res://` | Run the moved tool once on its own |
| An autoload is null in a test | The autoload failed to parse | Read the first `SCRIPT ERROR` line |
| A headless capture hangs | `frame_post_draw` never fires headless | Run captures in a real window |
| Headless frame timings look odd | The headless renderer is a dummy | Measure performance in a window |
| A display-server call fails headless | The headless server lacks it | Guard it with a feature check |
| A fake input does nothing | The tool called a handler directly | `Input.parse_input_event()` with a real event |
| A staged call reads an old position | The object moved in the same frame | Await 1 or 2 frames |
| `set_process(false)` has no effect | It ran before `add_child()` | Call it after `add_child()` |
| A parse error on a correct-looking line | A local name shadows a parameter or a signal (`finished`), or an override does not match the parent signature | Rename the local or the helper |
| A typed loop over a const of arrays gives empty values | A typed constant does not convert inner literals | Plain nested Arrays and an `Array` loop variable |
| "not a constant expression" | A function call in a `const` | Use a `var` or a static function |
| A change to a packed array is lost | A packed array in a dictionary is a value copy | Write it back or return it |
| A shader reads null | A uniform is not set in every build | Set every uniform |
| A tween test misses the first callback | The first step runs 1 frame late | Expect it 1 frame late, or set the first step in code |
| An animated child jumps back | Its container resets the offset | Put it inside a plain holder node |
| A colour test fails on a texture pixel | The atlas reads in linear space | Test a pixel that works in linear and sRGB |
| The project file loses lines after a generator tool | `ProjectSettings.save()` drops default-valued lines | Diff the project file and restore lost lines |
| An import changed a file you do not own | The import rewrites some resources | Restore that file; save your own edits in it first |
| A merge stops on an untracked `.uid` | 2 branches made different uids for 1 script | Compare, keep the owner's copy, merge again |
| A commit holds other work's `.uid` files | `git add` of a whole folder | Add only your own `.uid` files |
| New test files fail on `get_tree()` | The test base class exposes another helper | Use the test base's helpers |
| A capture wrote the player's save | It started the game without `-s` | Treat script mode, test mode and worktree paths as tool runs |
| A tool run read the player's settings | Autoloads read settings before the `-s` script ran | Set the slot from an environment variable |
| A saved setting leaked into a test | The helper set the path after `_ready` loaded settings | Set the path first, or reload defaults |
| Tests pass alone and fail in parallel | 2 runs shared 1 tool save or fixed file names | A slot per run; per-process scratch folders |
| A shared tool save from a newer version set a read-only flag | Runs shared 1 tool save | A scratch save per runner and per test |
| A capture used another run's reduced-motion setting | The capture queue started Godot without a slot | Pass the requester's slot to the queue |
| Event ticks shifted by many frames in a tool | Shared settings turned on reduced motion and hit-stop | Pin timing settings in the tool's own settings file |
| The user data folder grows by tens of GB | Thousands of tool saves | Clean tool saves by name; never near the player's save |
| Save tests fail in a sandbox | The sandbox blocks `user://` writes and `nice` | Run them where writes work; list it as a known failure class |
| A guard test runs its setup twice | `before_each()` called by hand | Put setup in 1 helper |
| Exit 127 on macOS | No `timeout` command | The perl alarm, `gtimeout`, or `--quit-after` |
| A call waits for hours | The slot wait sat inside or around the alarm | Put the alarm on the Godot command only |
| The parent stopped but Godot still runs | The alarm wrapped the wrapper, not Godot | Find and stop the child |
| The exit code says pass but the run failed | `$?` came from `tail` in a pipe | A log file or `pipefail` |
| No log after a loop | A relative log path inside `cd` | Absolute log paths |
| A test filter ran 0 tests and looked green | A comma list in 1 filter | 1 filter per call; check the count |
| A run header says the fixed rate is off | The engine consumes `--fixed-fps` | Prove the rate from ticks per frame |
| A process list shows your own command | `pgrep -f` matched the shell line or the grep | Match the binary name and the worktree path |
| A run started with `&` held a slot after the call ended | The run was not tracked | A tracked background task or the foreground |
| A batch still ran after the shell stopped it | The child survived the stop | Check for leftover processes before a new run |
| A zsh loop passed 1 word instead of several flags | zsh does not split variables | An array, `$=VAR`, or a shell function |
| The `play` recipe never ends in a test | A play run has no end | Start it, wait, stop it |
| Hung windows from old runs load the machine | Runs did not end by themselves | A frame limit or an alarm on every run; a process check before done |
