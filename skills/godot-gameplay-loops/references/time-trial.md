# Time-trial loop

The player races the clock through checkpoints, completes laps, and tries to
beat a best time or a ghost of an earlier run.

## Parts

| Part | Job |
| --- | --- |
| `RaceClock` | Owns the run: start time, the next checkpoint, laps, splits, the best time. |
| Checkpoint (`Area3D`) | Has an exported `index`. Reports the racer to the clock. |
| `GhostRecorder` | Samples the racer's transform at a fixed rate during the run. |
| `GhostPlayer` | Plays a saved run on a mesh with no collision. |

## The clock

Time a race with `Time.get_ticks_usec()`. It is monotonic and has microsecond
resolution. Store times as `int` microseconds; convert only for display.

The clock accepts checkpoints only in order. A racer who skips a checkpoint
does not complete the lap. This stops shortcut cheats.

```gdscript
class_name RaceClock
extends Node

signal checkpoint_passed(index: int, split_usec: int)
signal lap_completed(lap: int, lap_usec: int)
signal race_finished(total_usec: int, is_best: bool)

## Checkpoints between the start and the finish line, numbered 1 to N.
@export var checkpoint_count: int = 3
@export var lap_count: int = 3

var best_usec: int = 0  # 0 means no record yet
var _start_usec: int = 0
var _lap_start_usec: int = 0
var _next_index: int = 1
var _lap: int = 0
var _running: bool = false


func start(now_usec: int = Time.get_ticks_usec()) -> void:
	_start_usec = now_usec
	_lap_start_usec = now_usec
	_next_index = 1
	_lap = 0
	_running = true


func pass_checkpoint(index: int, now_usec: int = Time.get_ticks_usec()) -> bool:
	if not _running or index != _next_index:
		return false
	_next_index += 1
	checkpoint_passed.emit(index, now_usec - _start_usec)
	return true


func cross_finish_line(now_usec: int = Time.get_ticks_usec()) -> bool:
	if not _running or _next_index <= checkpoint_count:
		return false
	_lap += 1
	lap_completed.emit(_lap, now_usec - _lap_start_usec)
	_lap_start_usec = now_usec
	_next_index = 1
	if _lap < lap_count:
		return true
	_running = false
	var total: int = now_usec - _start_usec
	var is_best: bool = best_usec == 0 or total < best_usec
	if is_best:
		best_usec = total
	race_finished.emit(total, is_best)
	return true
```

Each method takes an optional `now_usec`. Gameplay omits it. A headless
check passes fixed values, so the check does not depend on real time.

Wiring: each checkpoint `Area3D` calls `pass_checkpoint(index)` from
`body_entered`. The finish area calls `cross_finish_line()`. A crossing out
of order returns `false` and changes nothing, so a racer who drives back
over an old checkpoint does no harm.

## Triggers

- Detect crossings with an `Area3D` signal, which runs in the physics step.
  A check in `_process` can miss a thin trigger at low frame rates.
- A fast racer can pass through a thin trigger between two physics steps.
  Make triggers deep along the track, or cast a ray from the previous to the
  current racer position each step (`godot-physics-system`).
- Scale the shape resource (`BoxShape3D.size`), not the `CollisionShape3D`
  node.
- A checkpoint's mask sees only the racer's layer.

## Ghosts

Record position and rotation at a fixed rate (for example 20 samples a
second) in packed arrays. Record the rotation as a `Quaternion`. Between two
samples, `lerp` the position and `slerp` the quaternion. Euler angles snap
when they wrap.

```gdscript
class_name GhostTrack
extends Resource

@export var sample_interval: float = 0.05
@export var positions := PackedVector3Array()
@export var rotations: Array[Quaternion] = []


func add_sample(xform: Transform3D) -> void:
	positions.append(xform.origin)
	rotations.append(xform.basis.get_rotation_quaternion())


func sample_at(time_sec: float) -> Transform3D:
	if positions.is_empty():
		return Transform3D.IDENTITY
	var f: float = time_sec / sample_interval
	var i: int = clampi(floori(f), 0, positions.size() - 1)
	var j: int = mini(i + 1, positions.size() - 1)
	var w: float = clampf(f - i, 0.0, 1.0)
	var pos: Vector3 = positions[i].lerp(positions[j], w)
	var rot: Quaternion = rotations[i].slerp(rotations[j], w)
	return Transform3D(Basis(rot), pos)
```

Recorder: in `_physics_process`, add `delta` to a float accumulator, and
call `add_sample(racer.global_transform)` each time it passes
`sample_interval`. Player: add `delta` to a play time, and set
`ghost.global_transform = track.sample_at(play_time)`.

The ghost is a mesh with a transparent material and no collision body. Do
not record the racer node or its children; record only the transform.

Save a ghost with `ResourceSaver.save(track, "user://ghosts/track_01.res")`
for a binary file. Or write the arrays with
`FileAccess.open_compressed(path, FileAccess.WRITE, FileAccess.COMPRESSION_ZSTD)`
and `store_var`. Keep only the best run's ghost per track.

To shrink a track, drop a sample when the racer moved less than a small
distance and turned less than a small angle since the last kept sample.
Then store the time of each sample too, because the interval is no longer
fixed.

## Display

Format `int` microseconds as minutes, seconds and milliseconds:

```gdscript
static func format_race_time(usec: int) -> String:
	var ms: int = usec / 1000
	return "%02d:%02d.%03d" % [ms / 60000, (ms / 1000) % 60, ms % 1000]
```

Compare records as `int`. Never compare float times with `==`.

## Online races

The server decides checkpoint and finish events
(`multiplayer.is_server()`). A client sends its input or position; the
server runs the triggers. Send positions with an unreliable transfer mode.
`godot-multiplayer-sync` covers this.

## Checks

- Checkpoints 1, 2, 3, then the finish line, with fixed times: one lap with the
  expected `lap_usec`.
- Checkpoint 2 before 1: `pass_checkpoint` returns `false`.
- The finish line before all checkpoints: no lap.
- `sample_at` halfway between two samples returns the middle position.
