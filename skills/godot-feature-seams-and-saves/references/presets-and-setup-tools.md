# Graphics presets and idempotent project setup tools

This file holds 2 settings methods of `godot-feature-seams-and-saves`: quality presets with a first-launch Auto pick, and tool scripts that write the same `project.godot` on a second run.

## Graphics presets

Use this when the game needs quality presets, a first-launch Auto pick and a frame cap.

1. Add the settings keys, a preset table (for example Low, Medium, High, Custom) and 1
   effective-value function. It returns the preset value, or the Custom value when the
   player picks Custom. Every consumer reads through this function.
2. Add a migration from old settings files to the new keys (see Save version migration in `save-integrity-and-migration.md`).
3. Add the Auto picker for the first launch. Save in the no-file branch, so the pick
   persists. Without that save, Auto runs again on every launch.
4. Build the settings rows. Dim the rows that the active preset controls until the player
   picks Custom.
5. Wire the values to the scene, for example light shadows and the post-process mode.
6. Measure the cost of each option with an A/B frame-time run (see `godot-perf-measure`).
7. In UI tests, loop over the row count. Do not press "down" a fixed number of times.

## Idempotent project setup tools

Use this when a tool script writes input maps or other settings into `project.godot`.

1. Probe unknown engine facts in a minimal scratch project first, not in the real one.
2. Put the defaults in 1 table, for example an input-defaults script.
3. Write plain arrays, as the editor does, not typed arrays. A typed array saves in another
   text form, and the next editor save rewrites every line.
4. Copy the project file. Run the tool. Diff against the copy. Only the intended lines change.
5. Run the tool again. The file must not change. The script in `script-check_idempotent.md` does steps 4 and 5. Copy the block into a file.
