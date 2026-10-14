# MultiMesh batching

Use this when many similar billboards or sprites cost too many draw calls, or a spatial shader works
on single meshes but fails on MultiMesh instances.

1. Map every material parameter of the old per-node path.
2. Write the design before code: the group key (for example texture, size, frame count) and the use
   of each per-instance colour and custom-data channel. Reserve channels for states you know are
   coming.
3. Add the grouped renderer behind a switch that is off by default.
4. Write tests that compare the packed per-instance data with the old material values.
5. Pass per-instance values (for example the instance position) through `INSTANCE_CUSTOM`. In a
   MultiMesh, `MODEL_MATRIX` gives the node transform, not the instance transform. Read the value
   behind a shader flag, so single meshes keep the old path. Check which built-ins exist in each
   shader stage in the engine docs.
6. Run a headless probe that counts nodes and draw calls for both modes.
7. Check near, far and behind cases with `godot-visual-verification`. It gives the captures and the
   pixel counts. Check the visible pixels per instance against the plan, and write the numbers into
   the notes. Acceptance needs numbers, not only images.
8. Prove the gain with the [Method](../SKILL.md#method) pairs.
