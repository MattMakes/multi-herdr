---
name: godot-gameplay-loops
description: "Use when building a repeatable gameplay loop in Godot 4.7 with GDScript: a collect-them-all hunt, a gather-and-respawn (harvest, mining, idle) loop, a timed race with checkpoints, laps and ghosts, a wave or horde survival mode, a death-and-respawn (revival, checkpoint, corpse run) loop, or hidden secrets (cheat codes, look-at reveals, meta unlocks). A router: pick the loop, then read one reference. Each reference has typed 4.7 GDScript for the authoritative manager, its signals, and its save data. Inventory, save files, state machines, data resources and the event bus belong to their own godot-* skills."
---

# Godot gameplay loops

Target engine: **Godot 4.7**. Every code block is typed GDScript that parses
on 4.7.2. Every engine API it names is in the 4.7.2 `--doctool` dump.

A gameplay loop is a rule the player repeats: find, gather, race, survive,
die and return, discover. Each loop in this skill has the same parts:

1. **One authoritative manager.** A node (often an autoload) owns the loop
   state. Pickups, triggers and UI never own state. They send events to the
   manager and draw what it reports.
2. **Stable ids.** The manager stores `StringName` ids. It never stores a
   `NodePath` or a node reference as identity. A scene that moves keeps its ids.
3. **Signals out.** The manager emits typed signals. The HUD, audio and VFX
   connect to them. The manager does not know the UI.
4. **Plain save data.** The manager returns its state as a `Dictionary` of
   ids, counts and timestamps, and accepts the same `Dictionary` back.
   `godot-save-load` writes it to disk.

## Pick the loop

Read only the reference that matches the task.

| The player... | Loop | Read |
| --- | --- | --- |
| finds every item of a set (eggs, coins, lore pages, 100% runs) | collection | [references/collection.md](references/collection.md) |
| hits a node with a tool, takes a yield, waits for a respawn; idle or offline gains | harvest | [references/harvest.md](references/harvest.md) |
| races the clock through ordered checkpoints, laps and ghosts | time trial | [references/time-trial.md](references/time-trial.md) |
| survives waves of enemies, with a break between waves | waves | [references/waves.md](references/waves.md) |
| dies and returns at a checkpoint, maybe with a corpse run or a ghost form | revival | [references/revival.md](references/revival.md) |
| enters a code, looks at a wall, or finishes 100% to open hidden content | secrets | [references/secrets.md](references/secrets.md) |

Loops combine. A wave game often adds revival. A metroidvania adds
collection and secrets. Read each reference that applies, and keep one
manager per loop.

## Rules every loop follows

- **Count once.** `Area2D.body_entered` and `Area3D.body_entered` fire again
  when a body leaves and comes back. The manager rejects an id it already
  has. The pickup also turns its own `monitoring` off with
  `set_deferred(&"monitoring", false)`, because a physics callback must not
  change physics state directly.
- **Commit, then show, then free.** Tell the manager first. Then play the
  sound or particles. Then hide the node and free it when the effect ends.
  If you `queue_free()` first, the effect and the commit can both be lost.
- **Physics triggers run in the physics step.** Overlap signals arrive in the
  physics step. A newly added `Area` reports overlaps only after one physics
  frame: `await get_tree().physics_frame` before you ask it.
- **Spawn deferred.** Add nodes from a physics callback with
  `add_child.call_deferred(node)`.
- **Duplicate shared data.** A `.tres` loaded by many nodes is one object.
  Before a node changes it at run time, call `duplicate_deep()` (4.5 and
  later; it copies nested resources too) or `duplicate()` for a flat one.
- **Pick the right clock.**
  - `Time.get_ticks_usec()`: race timing. It is monotonic, in microseconds.
  - `Time.get_unix_time_from_system()`: offline progress, respawn timers that
    survive a quit. It is wall-clock seconds and the player can change it, so
    clamp it.
  - `delta` in `_physics_process`: rates that run while the game runs.
  - `Engine.get_physics_frames()`: a frame-exact count for replays.
- **Integers for counts.** Store item counts, currency and race times as
  `int` (GDScript `int` is 64-bit). Store float only for rates.
- **`StringName` for ids and groups.** Write `&"enemies"`, not `"enemies"`,
  in hot paths. Typed collections make the key type explicit:
  `Dictionary[StringName, int]` (4.4 and later).
- **Collision layers filter, groups label.** Put pickups and triggers on a
  layer that only the player's mask sees. Use groups to find nodes, not to
  filter hits.

## Other skills own these parts

Do not repeat their content here. Read them when the loop needs them.

| Need | Skill |
| --- | --- |
| Items the player keeps after a pickup | `godot-inventory-system` |
| Writing the manager's `Dictionary` to `user://`, save slots, versions | `godot-save-load` |
| Player or enemy states (alive, downed, ghost), phases | `godot-state-machine` |
| Data resources (wave tables, loot tables, tool tiers) | `godot-resource-pattern` |
| Global signals between loop managers and UI | `godot-event-bus` |
| Hitbox, hurtbox and health components | `godot-component-system` |
| Damage, attacks and turn order | `godot-combat-system` |
| Currency, shops and loot rewards | `godot-economy-system` |
| Objectives that wrap a loop ("collect 10 pages") | `godot-quest-system` |
| Enemy pathing in a wave | `godot-ai-navigation` |
| Many-enemy performance, pools, MultiMesh | `godot-optimization` |
| Counters, timers and progress bars on screen | `godot-hud-system` |
| The genre around the loop: core loop, systems, scene tree | `godot-genre-blueprints` |

## Prove the loop

> proof: headless-run: `scripts/godot/gameplay_scenarios.py` runs the checks of every reference in the gate, the 3D pickup, harvest node and ghost included. Feel (ghost smoothness, respawn pacing): proof: not run (needs a human play test).

A loop is done when a headless script drives it and checks the result. The
manager is a plain `Node`, so a `SceneTree` script can create it, call its
methods, and read its signals without a scene:

```gdscript
extends SceneTree

func _initialize() -> void:
	var failures: int = 0
	# Build the manager under test here, call its methods, and check the
	# results. Count each failed check in failures.
	if failures > 0:
		push_error("loop check: %d failure(s)" % failures)
	quit(1 if failures > 0 else 0)
```

Run it with
`<godot> --headless --path <project> --script res://tests/loop_check.gd`.
The exit code is the script's `quit()` value. Run `--import` first on a fresh
checkout, so that `class_name` scripts are registered. `godot-build-verify`
has the full check sequence.

## Report

A loop is a design choice. When the task does not say which loop variant to
build (for example, a corpse run or a plain checkpoint respawn), choose the
simpler variant, build it, and send one `QUESTION:` to the orchestrator that
names the other variant. Do not wait for the answer before you build the
simpler one.
