# Stealth

The player avoids detection: moves through shadow and cover, reads guard
routes, and recovers when spotted. Thief, Hitman, Dishonored and Mark of the
Ninja are the reference points.

## Core loop

Observe → plan a route → move through light, shadow and sound → guards grow
suspicious → hide, distract or slip past → reach the goal. Being seen must
be recoverable, but costly.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Vision | cone, distance, light level, several sample points | `godot-physics-system` |
| Hearing | noise events with loudness; distance along the nav path | `godot-ai-navigation`, `godot-event-bus` |
| Awareness | detection meter with decay; alert levels | `godot-state-machine` |
| Guard behaviour | patrol, investigate, search, alarm, return | `godot-limboai` or `godot-beehave` |
| Player visibility | light level at the player, crouch, movement speed | `godot-3d-essentials` |
| Feedback | "?" and "!" icons, barks, a visibility gem | `godot-hud-system`, `godot-audio-system` |
| Tools | distractions, takedowns, bodies | `godot-ability-system` |

## Scene tree (4.7)

```text
Level (Node3D)
├── NavigationRegion3D
├── Guards (Node3D; group "guards")
│   └── Guard (CharacterBody3D)
│       ├── Eyes (Marker3D)
│       ├── NavigationAgent3D
│       ├── Awareness (Node; meter, alert level)
│       ├── AlertIcon (Sprite3D, billboard)
│       └── VisibleOnScreenNotifier3D
├── Player (CharacterBody3D; Head, Torso, Feet markers for sampling)
├── NoiseBus (autoload; emits noise(position, loudness))
└── HUD (CanvasLayer; light gem, alert state)
```

## Genre code

Vision is a meter, not a yes or no. Sample head, torso and feet; each
visible point adds to the fill rate, scaled by distance and light. The meter
drains when nothing is seen.

```gdscript
extends Node3D

@export var eyes: Node3D
@export var view_distance := 18.0
@export var half_angle_deg := 50.0
@export var fill_per_second := 1.2
@export var drain_per_second := 0.4
var meter := 0.0   # 0..1; 1 = detected

func tick(delta: float, points: Array[Vector3], light: float, self_rid: RID, player: Node) -> void:
	var space := get_world_3d().direct_space_state
	var seen := 0
	for p in points:
		var to_p := p - eyes.global_position
		var dist := to_p.length()
		if dist > view_distance:
			continue
		var fwd := -eyes.global_basis.z
		if rad_to_deg(fwd.angle_to(to_p)) > half_angle_deg:
			continue
		var q := PhysicsRayQueryParameters3D.create(eyes.global_position, p)
		q.exclude = [self_rid]
		var hit := space.intersect_ray(q)
		if hit.is_empty() or hit.get("collider") == player:
			seen += 1
	if seen == 0:
		meter = maxf(meter - drain_per_second * delta, 0.0)
		return
	var closeness := 1.0 - (eyes.global_position.distance_to(points[0]) / view_distance)
	var rate := fill_per_second * (seen / float(points.size())) * light * (0.3 + closeness)
	meter = minf(meter + rate * delta, 1.0)
```

`light` is 0 to 1 from the player's light probe. A ray that reaches the
point, or hits the player's own body, counts as seen. Raise the alert level
at thresholds of the meter (for example 0.3 suspicious, 1.0 detected).

## Pitfalls

- Binary detection. A meter with decay gives the player time to react.
- One sample point. A player half behind cover is seen or missed wrongly;
  sample at least three points.
- The ray hits the guard itself. Exclude the guard's RID.
- Not checking the query result. `intersect_ray()` returns an empty
  Dictionary on no hit.
- Hearing by straight-line distance through walls. Measure along the
  navigation path, and reduce loudness per wall or door.
- Fixed guard routes that never change. Guards leave the route to check a
  noise, search, then return.
- No feedback on why the player was seen. Show "?" and "!" states and play
  barks.
- Fighting that beats sneaking. Going loud must bring reinforcements or
  higher danger.
- A `RayCast3D` node per guard per point. Use direct space queries for many
  checks.
- Full AI for off-screen guards far away. Slow them with
  `VisibleOnScreenNotifier3D` and distance.
- Alarms sent to a hard-coded list. Use `get_tree().call_group(&"guards", ...)`.
- Bodies that still block paths. Disable their collision or move them to a
  corpse layer.
- Cover chosen at a random nav point. Verify it with a ray toward the
  threat.
