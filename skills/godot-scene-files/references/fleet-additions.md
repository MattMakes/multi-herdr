# Fleet additions

Facts that the GodotPrompter v1.14.0 skills lack, verified by unit GW10
(worker opus-83) against the Godot 4.7.2 `--doctool` dump and godot-docs 4.7
(commit `9adca4c`). Written in own words.

## Scene format since 4.6

- Since Godot 4.6, a `.tscn` file stores a `unique_id` on each `[node]` line
  and has no `load_steps` in its header (godotengine/godot GH-106837 and
  GH-103352).
- Godot 4.5 and 4.6 read each other's scenes.
- Measured in this skill on 4.7.2: a script re-save writes new `unique_id`
  values on every node, and a node added by text without `unique_id` loads
  (`worked-example.md`, sections 1 and 2).
