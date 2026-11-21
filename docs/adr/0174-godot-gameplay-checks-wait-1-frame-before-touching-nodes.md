# Godot gameplay checks wait 1 frame before touching nodes

The gameplay check scripts under `scripts/godot/tests/fixtures/gameplay/`
(`check.gd` of godot-combat-system, godot-economy-system,
godot-gameplay-loops and godot-quest-system) extend `SceneTree` and await
1 `process_frame` at the start of `_initialize`. The root enters the tree
only after `_initialize` starts, and the skills' `_ready` code needs nodes
inside the tree.

## Consequences

- A GDScript lambda captures a local int by value. The quest-system check
  counts signal callbacks by appending to an Array, which is captured by
  reference. A local int counter would silently stay 0.
