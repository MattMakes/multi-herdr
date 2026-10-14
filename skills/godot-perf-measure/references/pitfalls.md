# Pitfalls

| Symptom | Cause | Fix |
| --- | --- | --- |
| Frame time sits at 8.33 ms (or 16.67 ms) with vsync "off". | Saved user settings force vsync on, or the fps cap is on. | Vsync off and `--max-fps 0` in a scratch settings file for each run. |
| Every GPU timer reads 0. | The driver gives no GPU timestamps (for example Metal). | Use another driver, or compare wall time and render CPU time. |
| Render time is tiny next to the frame (for example 0.15 ms against 1.65 ms). | The measure reads only the root viewport. | Sum every SubViewport. |
| Numbers jump when you switch apps. | The OS throttles a background window. | Run in the foreground. |
| Memory and object counts grow in headless runs only. | Headless uses a dummy renderer. | Measure memory and render cost in a window. |
| Frame times fall by half between two runs of the same code. | Machine load changed. | Interleaved pairs, load per take, invalid flag above a limit. |
| A cost test fails at 0.1 ms under load and passes at 0.025 ms when idle. | One noisy run. | Best of 3 to 5 runs inside the test. |
| The first take is slow. | Cold caches. | One untimed warm-up run. |
| A and B differ by more than the change. | Another window run used the GPU, or a variant did less work. | Hold the window locks; give every variant equal work. |
| A probe slows its own early frames. | The probe does debug work (for example a hash) at run start. | Discard those frames; put the debug work behind a flag. |
| A player's spikes do not show in your tool. | Different settings, renderer, build type or machine load. | Match settings and renderer, add background CPU load, run the release export. |
| A long run does not fit in one tool call or session limit. | The command times out. | Single-run sessions; a sped-up run (for example `--time-scale 2`) for memory, a normal-speed run for timing. |
| Both batches ran on one renderer. | The shell did not split a flag string. | Use `sh` or a shell array for flag lists. |
| A script run breaks halfway. | You edited the shell script while it ran; `sh` reads it as it goes. | Do not edit a script during a run. |
| Headless frame counts do not match time. | Headless frames run faster than real time. | Wait on timers, not frame counts. |
| A hitch map logs no frames headless. | Headless does not draw. | Take frame logs in a window. |
| Draw calls grow over time. | One draw per item is created and never freed. | Batch and free. |
| Node count grows over a long run (for example 500 to 1,400). | Far objects never despawn. | Despawn far objects; check each pool has a cap. |
| A load stalls 16 to 24 ms per new view. | A synchronous resource load on the main thread. | Load on a worker thread ahead of the view. |
| A chunk restore costs hundreds of microseconds. | Array concatenation per chunk. | Write into a preallocated slot (for example `put_data`). |
| An effect reads data every frame. | A constant is recomputed per frame. | Compute it once, or on change. |
| A vertex shader runs twice on one renderer. | A depth prepass on that renderer. | Read the engine source to confirm the rule. |
| A 60 to 110 ms spike on the first use of an effect. | A shader compiles on first draw. | Add the material to the warm-up; prove with cold runs. |
| Warmed shaders compile again later. | The engine freed unheld shaders. | Keep a reference to each warmed material. |
| A new feature file stalls on first draw. | It builds a material and is not in the warm-up. | The guard test; one row per file. |
| A filter on `autostart` finds no timers. | The engine clears `autostart` after the first start. | Filter by type. |
| A first render frame still costs hundreds of ms after warm-up. | Not all of it is shader compile. | Measure it on its own before you decide. |
| A probe trail is corrupt around an event. | The player emits the same signal each tick. | Disconnect it in the probe. |
| A summary blames the wrong frame. | An off-by-one in the frame index around the event. | Test the index logic on a known trail. |
| `has_method` returns false for a real method. | The call checked the `Script` object, not the node. | Call `node.has_method`. |
| A high-refresh comparison covers too little play. | The frame count was fixed, not the play time. | Set the frame count from the play time you need. |
| Threads give no gain. | The dispatch costs as much as the loop. | Measure dispatch cost first. |
| Two people fix the same hitch. | No owner per fix. | One owner per fix, announced. |
| The split timer itself makes orphans. | The instrumentation leaks. | Take clean numbers without the split timer. |
| Single worst frames do not repeat. | Heavy machine load. | Use medians, not worst frames, under load. |
| A late event never fires in a stress run. | The bot circles a waypoint it cannot reach. | Scale the route to the bot speed. |
| A MultiMesh effect shows every instance at the node origin. | `MODEL_MATRIX` gives the node transform. | Pass instance values in `INSTANCE_CUSTOM`. |
| A shader fails to compile after a batching change. | A built-in that the stage does not have. | Check the stage in the engine docs. |
| A packed data layout breaks when a peer adds a state. | No spare channels. | Reserve channels for known future states. |
| A reviewer cannot accept a visual change. | Images with no numbers. | Record pixel counts and ratios. |
| Interpolation is on in a SubViewport you set off at the root. | The root setting does not reach the SubViewport. | Set the mode on each SubViewport before `add_child`. |
| A marker drifts from the drawn player. | It uses the physics position. | Use the interpolated transform. |
| Camera judder is hard to see. | Impact shake hides the follow cadence. | Clear shake in the drive tool. |
| A deferred camera update errors. | It runs after the node left the tree. | Guard with `is_inside_tree()`. |
| The view edge shows the clear colour after a camera change. | The change added a margin the test did not know. | Add it to the test and to the fill margin. |
| Seams at chunk borders. | Noise keyed on the chunk seed. | Key it on world position. |
| A placement test passes but the rule fails in play. | The test checked one chosen chunk. | Assert the rule over all loaded sites. |
| A tool script fails to compile on a global class. | Some tool runs do not load global class names. | Load classes at run time with `load()`. |
| A sketch from research does not parse. | A non-constant expression in a `const`, or a wrong API. | Parse-check every sketch in a scratch project. |
| Rounding differs between GPUs. | `round(-0.5)` is not portable; low-precision custom data. | Avoid the tie case; encode values in 0 to 1. |
| A blocked web page gives no text. | A login wall. | Try the RSS feed; else ask a human for a saved file. |
| A hung timing script blocks the machine. | No bounded wait. | Use a bounded poll; kill only your own process. |
| In-place `sed` fails on macOS. | BSD `sed` needs a suffix argument. | Use `perl -pi` or a Python edit, then grep the value. |
| GDScript cannot infer a type. | An untyped dictionary value. | Declare the type. |
| A prototype commit lands by accident. | Prototype-only code shares a branch. | Keep prototype branches separate and unmerged. |
| An untracked file blocks the base worktree checkout. | A generated file (for example a `.uid`) exists in the folder. | Delete the untracked copy, then check out. |
