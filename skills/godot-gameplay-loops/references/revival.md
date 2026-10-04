# Revival loop

The player dies and comes back. The loop is: death, a short pause, a reset
of the player's state, a return at the right checkpoint, a short window of
safety, and a world that keeps the player's progress. Variants add a corpse
run (go back and pick up what you dropped) or a ghost form.

## Parts

| Part | Job |
| --- | --- |
| `RespawnDirector` (autoload) | Owns the active checkpoint and the death-to-respawn sequence. |
| Checkpoint (`Area3D` or `Area2D`) | Has a `checkpoint_id`, a `progress` number and a `Marker3D` spawn spot. Offers itself to the director. |
| World flags | Opened doors, beaten bosses, taken unique items. They stay set after a death. |
| Player | Has `die()`-style signals and a reset method. It is never freed on death. |

## Pick the checkpoint by progress, not by distance

A checkpoint has a `progress` number that grows along the critical path. The
director accepts a checkpoint only when its `progress` is equal to or higher
than the active one. A player who walks back to the first shrine does not
lose the later one. The nearest checkpoint by distance is often the wrong
one in a non-linear level.

## The director

```gdscript
class_name RespawnDirector
extends Node

signal player_died(cause: StringName)
signal player_respawned(checkpoint_id: StringName)
signal checkpoint_activated(checkpoint_id: StringName)

@export var respawn_delay_sec: float = 1.5
@export var safe_window_sec: float = 2.0

var checkpoint_id: StringName = &""
var checkpoint_progress: int = -1
var spawn_transform := Transform3D.IDENTITY
var world_flags: Dictionary[StringName, bool] = {}
var _busy: bool = false


func offer_checkpoint(id: StringName, progress: int, at: Transform3D) -> bool:
	if progress < checkpoint_progress:
		return false
	checkpoint_id = id
	checkpoint_progress = progress
	spawn_transform = at
	checkpoint_activated.emit(id)
	return true


func set_flag(flag: StringName) -> void:
	world_flags[flag] = true


func on_player_died(player: CharacterBody3D, cause: StringName) -> void:
	if _busy:
		return
	_busy = true
	player_died.emit(cause)
	player.set_physics_process(false)
	player.set_process_unhandled_input(false)
	# The death animation and a screen fade play during this pause.
	await get_tree().create_timer(respawn_delay_sec, false).timeout
	player.velocity = Vector3.ZERO
	player.global_transform = spawn_transform
	if player.has_method(&"reset_after_death"):
		player.call(&"reset_after_death", safe_window_sec)
	player.set_physics_process(true)
	player.set_process_unhandled_input(true)
	_busy = false
	player_respawned.emit(checkpoint_id)


func to_save() -> Dictionary:
	return {
		"checkpoint_id": String(checkpoint_id),
		"progress": checkpoint_progress,
		"spawn": var_to_str(spawn_transform),
		"flags": PackedStringArray(world_flags.keys()),
	}


func from_save(data: Dictionary) -> void:
	checkpoint_id = StringName(data.get("checkpoint_id", ""))
	checkpoint_progress = int(data.get("progress", -1))
	spawn_transform = str_to_var(data.get("spawn", var_to_str(Transform3D.IDENTITY)))
	world_flags.clear()
	for flag: String in data.get("flags", PackedStringArray()):
		world_flags[StringName(flag)] = true
```

The player's `reset_after_death(safe_sec)` method:

- refills health (the health component is in `godot-component-system`),
- clears status effects, held inputs, combo buffers and climb or grab locks,
- returns the state machine to its idle state (`godot-state-machine`),
- turns on invulnerability for `safe_sec` seconds, with a blink, so a
  respawn next to an enemy is not a second death.

`var_to_str` and `str_to_var` turn a `Transform3D` into text and back, so the
save stays JSON-safe.

## Rules

- **Keep the player node.** Hide it and stop its processing; do not
  `queue_free()` it. The camera, the HUD and enemies hold references to it.
- **Zero the velocity before the move.** A body that keeps its fall speed
  flies off the respawn spot.
- **Pause before the return.** A respawn in the same frame as the death is
  hard to follow. 1 to 2 seconds with a fade or a death animation is common.
- **Do not reset the world.** Read `world_flags` when a level loads: an
  opened door stays open, a taken unique item stays gone. Reset only what
  the design resets (common enemies, for example).
- **Save the checkpoint when it activates**, not only on quit, so a crash
  does not lose it. `godot-save-load` writes the file.

## Corpse run

On death, spawn a "grave" node at the death spot and move the dropped
resource (souls, coins, experience) from the player into the grave. The
grave is an `Area3D` that returns the amount on touch. Rules:

- Only one grave exists. A second death before the pickup removes the first
  grave and its amount. This is the risk that makes the loop work.
- Store the grave's position and amount in the save data.
- A death spot over a pit or in lava cannot be reached. Place the grave at
  the last safe floor position: record `global_position` while
  `is_on_floor()` is `true` and the floor is not a hazard.

## Ghost form

For a downed or spirit state, swap the player's collision layers instead of
removing collision. The ghost layer collides with the world but not with
enemies or hazards, and spirit-only objects sit on a layer only the ghost
mask sees. Store the normal `collision_layer` and `collision_mask` before the
swap, and restore them on revive. A shader with transparency or a grey
screen effect shows the state (`godot-shader-basics`).

## Death data

Record each death as `{cause, position, checkpoint_id, run_time}` in a list,
and write it in batches. A heat map of death positions shows where a level
is too hard. Use `print` lines or a file under `user://` in a debug build
only.

## Checks

- Offer checkpoints with progress 1, then 3, then 2: the active one stays 3.
- A death during the pause of another death is ignored.
- After the respawn, `velocity` is zero and the transform is the spawn
  transform.
- `to_save()` then `from_save()` on a new director keeps the checkpoint and
  the flags.
