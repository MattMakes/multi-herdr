# Visual verification pitfalls

Read this table when a capture, a gate or a UI test does something you did not expect. Rows are
grouped by area. Each row is one real failure and the fix that worked.

## Window and capture mechanics

| Symptom | Cause | Fix |
| --- | --- | --- |
| Tests pass, but the real window shows a broken look (for example a mask erased by 2D alpha blend) | Headless Godot does not draw | Capture in a real window; add a test that renders the mask with blend off |
| Window shot is 1 frozen frame; 2 shots show the same clock | Another window covers the capture window, and the OS stops its redraw | Always on top, no focus, force a draw; check that the clock moves; trust the 1x viewport images |
| A gate flakes or hangs for many minutes | The capture window is covered | Window flags, readiness check, run limit, window lock; prove with 20 runs under cover |
| First shot after start or resize is blank or black | The window did not draw yet | Wait about 60 frames and await the rendered frame; use the viewport image meanwhile |
| Window shot is black and includes the title bar | Grab before the first draw; decorated window | Shoot after frame 60; set the window borderless for the shot, or crop the title bar |
| `get_texture().get_image()` returns null; a render wait hangs | Headless dummy renderer | Capture only windowed; sync logic tests on process frame counts |
| `--resolution` has no effect | Project settings apply a window scale | Set the window size from the tool after start |
| A window shot comes out fullscreen | Project window mode or a shared settings file sets fullscreen | Switch to a fixed windowed size after some frames |
| A "1x" file is the window size, or 2x shots come out at base size | The root viewport follows the window | Nearest-neighbour downscale; save the window texture separately |
| A "1280" capture is only the small internal viewport | The tool saved the viewport, not the window | Add a real window shot path |
| The screen grab caught another window | The grab used a screen region | Grab by your own window id, or use your own viewport image |
| Mouse hover captures show nothing | `push_input` takes window coordinates, and the window is at 2x scale | Pass `in_local_coords = true` |
| The mouse steers the actor in a real window | Real cursor input reaches the game | Hold the heading by script in the tool |
| A teleport draws a smear | Physics interpolation blends from the old spot | Reset physics interpolation after each scripted move |
| Window shots lag and show a later state | The game clock runs during the grab | Stop the director clock during a shot; hold about 4 frames |
| A capture shows the other renderer | The renderer flag differs from the gate | Copy the exact renderer flags of the gate; check the log |
| 2 flags passed as 1 argument, so a setting stayed off | Quoting joined the flags | Pass each flag as its own argument |
| No `timeout` command on the platform | macOS has none by default | Give the capture tool its own quit timer |
| Relative output paths write inside the project or outside the worktree | The Godot working directory is the project folder | Absolute output paths |
| A failed capture keeps its window open and holds the lock | No quit on error | Quit watchdog at tool start; the runner kills a run after a limit |
| Close events drop each other's shot requests | Shots requested from signal handlers | A shot queue inside the tool |
| A window run overwrote a tracked file | The tool wrote to a default path in the repo | Output to the review folder only; restore the file from the base commit |

## Staging the state

| Symptom | Cause | Fix |
| --- | --- | --- |
| Captures loaded the player's real save | Default save path | Scratch or preset saves only; hold captures until the tool cannot reach the real save |
| A queued toast shows in a shot where it must be hidden | The UI queue was not drained | Drain queues in the tool first |
| A toast is missing from a capture | Signals fired out of game order and reset the HUD | Emit signals in game order |
| The subject is off screen or covered by the player | The tool did not place the actors | Place the player or camera beside the target |
| A shot of an enemy attack warning always catches the hit flash | The scripted player does not dodge | Steer away during the warning, as a player does |
| The capture scene differs from the real scene | The real scene hides or adds nodes in setup | Mirror the real scene setup in the tool |
| A colour shift is the time-of-day grade, not an art fault | Dusk grade active | Check the grade; capture at a fixed time of day |
| A 1-frame state shows the wrong frame | Timing variance | Run the state 2 times and compare |
| A mode is shot before its fill settled | Short settle time | Longer settle time |
| A fade does not trigger in the shot | The player stands beside the trigger, not in it | Place the player for the fade |
| A "before" shot hides the change under another effect | Bad shot timing | Take the "before" shot earlier at the same camera spot |
| A return pass on the same line cannot show the effect | The path overlaps itself | Offset the second pass |
| Stale import of changed art; a script does not compile | Import not run after art changed | Run the import before the capture |
| A shot from the wrong level or a fallback scene got committed | Nobody checked the subject | Check each image's subject; list the folder before the commit |
| Stale shots from an older build sit in the folder | A run ended early | Clear the folder or check shot names per run |

## Measuring and diffing

| Symptom | Cause | Fix |
| --- | --- | --- |
| 2 captures of 1 build differ by 100000+ pixels | Shaders and wind use wall-clock time | Zero time inputs; pause the tree; count only pixels that differ from both base captures |
| A whole-frame diff shows noise from wind | Real-time effects in the frame | Compare gate region values, or count target pixels per cell |
| A thin box samples only 1 row | Box too small | A box over the full block |
| A blob shadow cannot match a speckled shadow pixel by pixel | Different render method by design | Judge that goal by eye with a zoom sheet |
| A visibility check fails cells where the subject is plain to see | The check uses luminance only | Add a unique colour marker or a colour distance |
| A flicker check flags most pixels | Grass or foliage animation | Block-average the frames or mask the animated area |
| A probe flags a repeat that is by design | Probe region too wide | Limit the probe region |
| A value differs from old notes by a small amount | The base already had it | Run the base worktree; it shows the same value |
| A diff of 0 between 2 empty frames | Nothing rendered | Check that the frames show the subject |
| A 50 % fade frame looks grey | The scaled view blends dither | Judge pixels at 1x |
| The headless renderer returns zero MultiMesh transforms | It does not keep them | Keep spots in the script and test those |
| A snapshot helper fails on unset shader parameters | No defaults | Defaults in the helper |
| A built-in shader variable is not valid in the stage you assumed | Wrong stage | Check the shader reference; move the code |
| Reflection omits some texture uniforms | Engine reflection limits | Read the shader declaration too |

## Gate and runner

| Symptom | Cause | Fix |
| --- | --- | --- |
| A gate time includes the lock wait | Timer around the whole wrapper | Time the Godot run alone |
| A heredoc inside `$(...)` with an apostrophe misparses in old bash | Shell parser bug | Write the check result to a file |
| `git checkout` of a file restored an old check | The committed file had the old version | Restore from the base branch, then apply the edit again |
| The runner refuses arguments | Shell characters in the request | Put every option after `--`; no shell characters |
| A reused request id never runs | The runner skips ids with a `.done` file | New id per round; delete stale `.done` files |
| A capture script fails on an autoload name | Global names do not resolve in a `-s` script | Get the node from the tree root at run time |
| A direct gate in a sandbox hits its alarm | No window in the sandbox | Run gate cases through the capture queue |
| Workers poll the queue with long `sleep` loops | No wait primitive | 1 bounded wait or a file monitor |
| A reviewer finds defects after the worker said done | The worker did not review the after set | Review every after capture; write a failing test per defect first |
| The shell does not split a variable into words | zsh word splitting differs from bash | Use an array or a function for a file list |

## UI and text

| Symptom | Cause | Fix |
| --- | --- | --- |
| A results card overflows only in a real window shot | Logs cannot show overflow | Read the window captures |
| Button text has a contrast near 1:1 | Text colour equals the face colour | Report as a blocker |
| A PanelContainer stacks all children on each other | It holds 1 child by design | Put a box container inside it |
| A badge overlaps a label | Shot taken during a count-up | Move the badge; capture after the count-up ends |
| A panel does not grow with wrapped text | The label reports a 1-line minimum height | Compute the height from the wrapped line count |
| A widget appears 2 times | A seam already adds it, and the test adds another | Remove the extra add |
| `%Name` lookup fails | The node has no unique name | Set the unique name or use a plain path |
| An edge tab repeats a label that is almost fully visible | No visibility rule | No tab when most of the label is in view |
| A design size clashes with the base resolution | 2 specs disagree | Ask 1 question with a recommended default |
| A layout test reads rects before a tween ends | No settle wait | Await a settle helper |
| A width cap is set by trial many times | No computed limit | Compute the free width from the longest real content and assert it |
| Focus on the same node again does not restore its margin | Untested repeat path | A test that focuses the same node 2 times |
| A label at the map edge covers a node | No free-spot search | Search free spots by distance up to a limit; drop a label with no free spot |
| A sprite sits a fraction of a pixel inside the HUD band | The camera snaps to whole pixels | Allow a small margin |
| A test reads a pip that flashes for 2 frames | Wrong sample element | Read a stable element |
| Lettering is blurry | A pixel font at a non-native size | Native sizes or whole multiples; a test on every label |
| New copy is too wide for its panel | Spec copy landed after the build | A text-fit test; shorten the text |
| Names in the text changed during the work | A peer renamed things | Ask for the final names; update the source file |
| A `sed` edit failed on a newline and the old text stayed | Line-based edit on multi-line text | Use a structured edit and assert that it applied |

## Motion and looks

| Symptom | Cause | Fix |
| --- | --- | --- |
| An effect has no reduced-motion form | The script never reads the setting | Grow it in 3 still steps so it still reads as a timer |
| A new marker animates in steps under reduced motion | No still form planned | A still form with an instant fade |
| Reduced motion erases a closed shape after some seconds | The still form reuses the timed fade | Draw it settled at once; test that it stays some seconds later |
| A run ends at dusk before the effect completes | The game clock runs | Pin the run clock |
| Look 3 equals look 2 in 1 area | No difference check | Change it so it is a real third option |
| One renderer draws 2 looks much darker | Renderer difference | Compare and tune on each renderer |
| A picked concept reads as the wrong object | Missing role cue | Add a part that states the role |
| A brief arrives while generation runs | Late input | Finish the current call; read the full brief before final work |
| A static texture shows as a leak at exit | Shared static resource | Keep it per instance |

## Storage

| Symptom | Cause | Fix |
| --- | --- | --- |
| A review folder grows to tens of MB | Raw window shots committed | Commit 1x shots and contact sheets; keep raw shots out of the repo |
| 2x copies are byte-identical to the 1x files | The scale step did nothing | `cmp` the files; delete duplicates |
| A full run log is hundreds of KB | Verbose logs in the repo | Write the log to a temp folder; keep event lines only |
| An image library call is deprecated or missing | Library version drift | Check the current API; prefer a standard-library script |
