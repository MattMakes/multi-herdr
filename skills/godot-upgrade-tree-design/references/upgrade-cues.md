# Stat-keyed upgrade feedback

This file belongs to the `godot-upgrade-tree-design` skill. The terms (diag tree, chapter, class, policy) are defined in its SKILL.md.

Use this when each upgrade must show on screen and in sound, and later classes must reuse the cues.

1. Map each tree node to the stat it feeds.
2. Write 1 table: stat key (or unique node id) to cue. Do not key cues by class; a class-keyed cue cannot be reused.
3. Give each class model 1 method that returns its anchor points.
4. Make each cue scale from level 1 to the max level.
5. Measure the cost of all cues at the max level in 1 frame against the frame budget. Add the cue materials to the
   shader warm-up, so the first cue does not hitch.
6. Capture each cue at level 1 and at the max level. Use a clip for a cue that needs motion to read.
