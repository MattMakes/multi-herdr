Adds 3D acoustics on top of positional players: occlusion behind walls, reverb and bus-override zones with Area3D (including the 4.7 `area_mask` default), Doppler setup on both ends, surface-dependent footsteps and distance culling; read it when 3D sound must react to the level geometry, not only to distance.

# Spatial Acoustics in 3D

> ← Back to [SKILL.md](../SKILL.md). Player node basics are in SKILL.md section 4.

All code targets Godot 4.7.

---

## 1. Defaults Worth Knowing

Read from the 4.7.2 class reference:

| `AudioStreamPlayer3D` property | Default | Note |
|---|---|---|
| `attenuation_model` | `ATTENUATION_INVERSE_DISTANCE` | Volume falls with distance by default. |
| `max_distance` | `0.0` | 0 means no cut-off distance. Set it so far sounds stop mixing. |
| `unit_size` | `10.0` | Larger values carry the sound farther. |
| `attenuation_filter_cutoff_hz` | `5000.0` | A low-pass that grows with distance; also the occlusion tool below. |
| `area_mask` | `0` (since 4.7; was 1) | No `Area3D` audio effects until you set it. |
| `doppler_tracking` | `DOPPLER_TRACKING_DISABLED` | Doppler is off until enabled on the player **and** the camera. |

## 2. Occlusion Behind Walls

Sound behind a wall should be quieter and duller. Cast a ray from the sound to
the listener on a physics tick. If geometry blocks it, tween the
distance filter down and the volume a little.

```gdscript
extends AudioStreamPlayer3D

## Muffles this source when level geometry blocks the line to the listener.
@export_flags_3d_physics var occluder_mask: int = 1
@export var open_cutoff_hz: float = 20500.0
@export var blocked_cutoff_hz: float = 800.0
@export var blocked_volume_db: float = -6.0
@export var check_interval: float = 0.1

var _timer: float = 0.0
var _blocked: bool = false
var _tween: Tween


func _physics_process(delta: float) -> void:
	_timer -= delta
	if _timer > 0.0 or not playing:
		return
	_timer = check_interval
	var camera := get_viewport().get_camera_3d()
	if camera == null:
		return
	var query := PhysicsRayQueryParameters3D.create(global_position, camera.global_position, occluder_mask)
	var hit := get_world_3d().direct_space_state.intersect_ray(query)
	var blocked := not hit.is_empty()
	if blocked != _blocked:
		_blocked = blocked
		_apply(blocked)


func _apply(blocked: bool) -> void:
	if _tween and _tween.is_valid():
		_tween.kill()
	_tween = create_tween().set_parallel(true)
	_tween.tween_property(self, "attenuation_filter_cutoff_hz", blocked_cutoff_hz if blocked else open_cutoff_hz, 0.25)
	_tween.tween_property(self, "volume_db", blocked_volume_db if blocked else 0.0, 0.25)
```

The ray uses the camera position because the current camera is the default
listener. If the game uses an `AudioListener3D`, cast to that node instead.
Keep the occluder mask to walls and large props, not to characters.

## 3. Reverb and Bus Override Zones

An `Area3D` can change how sound inside it is heard:

- **Reverb send:** `reverb_bus_enabled = true`, `reverb_bus_name` = a bus that
  holds an `AudioEffectReverb` (for example `"CaveReverb"`), and
  `reverb_bus_amount` for how much is sent. `reverb_bus_uniformity` blends
  between a positional send (0) and an even one (1).
- **Bus override:** `audio_bus_override = true` with `audio_bus_name` sends
  the player's whole output to another bus (for example an underwater bus
  with a low-pass filter).

Since Godot 4.7, a 3D or 2D player finds these areas only when its
`area_mask` overlaps the area's collision layer, and `area_mask` now defaults
to 0. Set it on every player that should react to zones.

```gdscript
extends Area3D

## A cave zone. Players that should react need area_mask to include this layer.
func _ready() -> void:
	reverb_bus_enabled = true
	reverb_bus_name = &"CaveReverb"
	reverb_bus_amount = 0.6
	reverb_bus_uniformity = 0.3
```

```gdscript
extends AudioStreamPlayer3D

## Layer 8 holds the acoustic zones in this project.
func _ready() -> void:
	area_mask = 1 << 7
```

Create the `CaveReverb` bus in the bus layout and route it to `Master` (or an
environment bus) so that the reverb tail is not ducked with SFX.

## 4. Doppler

Doppler needs both ends:

- On the source: `doppler_tracking = DOPPLER_TRACKING_PHYSICS_STEP` for a
  body moved in `_physics_process()`, or `DOPPLER_TRACKING_IDLE_STEP` for a
  node moved in `_process()`.
- On the listening camera: `Camera3D.doppler_tracking` set to the matching
  mode.

Use it on fast sources (cars, projectiles, fly-bys). On slow sources it costs
work and is not heard.

## 5. Footsteps by Surface

Pick the footstep bank from what the character stands on. A downward ray
from the feet returns the collider; tag ground bodies with metadata (or
groups) that name their surface.

```gdscript
extends Node3D

## Plays a surface-specific footstep. Ground colliders carry meta "surface".
@export var banks: Dictionary[StringName, AudioStream] = {}
@export var fallback: AudioStream
@export_flags_3d_physics var ground_mask: int = 1

@onready var player: AudioStreamPlayer3D = $FootstepPlayer


func step() -> void:
	var from := global_position + Vector3.UP * 0.2
	var query := PhysicsRayQueryParameters3D.create(from, from + Vector3.DOWN * 1.0, ground_mask)
	var hit := get_world_3d().direct_space_state.intersect_ray(query)
	var surface := &""
	if not hit.is_empty():
		var collider: Object = hit["collider"]
		if collider and collider.has_meta(&"surface"):
			surface = StringName(collider.get_meta(&"surface"))
	var stream: AudioStream = banks.get(surface, fallback)
	if stream:
		player.stream = stream
		player.play()
```

Make each bank an `AudioStreamRandomizer` (see
[mixing-voices-and-ducking.md](mixing-voices-and-ducking.md)) so that steps
vary. Call `step()` from an animation method track at the frames where the
foot lands.

## 6. Stop What Cannot Be Heard

A playing `AudioStreamPlayer3D` is mixed even when it is too far away to hear.
Set `max_distance` on looping sources (machines, rivers, crowds), and for
many of them stop and restart playback by distance from the listener, with
some slack so that they do not flicker at the edge.

```gdscript
extends AudioStreamPlayer3D

## Pauses this looping source when the camera is far away.
@export var resume_from_start: bool = false


func _ready() -> void:
	if max_distance <= 0.0:
		max_distance = 40.0


func _on_cull_timer_timeout() -> void:
	var camera := get_viewport().get_camera_3d()
	if camera == null:
		return
	var d := global_position.distance_to(camera.global_position)
	if d > max_distance * 1.2 and playing:
		stream_paused = true
	elif d < max_distance and stream_paused:
		stream_paused = false
		if resume_from_start:
			play()
```

Connect a `Timer` (for example 0.5 s, autostart) to `_on_cull_timer_timeout`.
