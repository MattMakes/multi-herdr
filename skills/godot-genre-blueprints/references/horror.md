# Horror

Games that build dread: the player is weak, resources are scarce, and a
threat may be near. Alien: Isolation, Resident Evil and Amnesia are the
reference points.

## Core loop

Explore → sense a threat → hide, run or fight at a disadvantage → reach
safety → a short relief → tension builds again. Tension that never drops
stops working; the relief is part of the design.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Pacing director | tension value, build-up, peak, forced relief | `godot-state-machine` |
| Monster senses | sight by ray, hearing by noise events, suspicion meter | `godot-ai-navigation`, `godot-physics-system`, `stealth.md` |
| Monster behaviour | patrol, investigate, hunt, search, leave | `godot-state-machine`, `godot-limboai` or `godot-beehave` |
| Light and fog | flashlight, volumetric fog, flicker | `godot-3d-essentials`, `godot-shader-basics` |
| Sound | 3D cues, muffling under stress | `godot-audio-system` |
| Scarcity | few batteries, little ammo, slow healing | `godot-inventory-system` |
| First-person body | lean, crouch, shake | `godot-player-controller`, `godot-camera-system` |
| Safe-room saves | save points that never hitch | `godot-save-load`, `godot-multithreading` |

## Scene tree (4.7)

```text
Level (Node3D)
├── WorldEnvironment (volumetric fog on)
├── Director (Node; tension and hints)
├── Map (Node3D; NavigationRegion3D, rooms, HidingSpot nodes with metadata)
├── Monster (CharacterBody3D)
│   ├── NavigationAgent3D
│   ├── Eyes (Marker3D; ray origin)
│   ├── VisibleOnScreenNotifier3D
│   └── Brain (state machine)
├── Player (CharacterBody3D)
│   └── Head (Node3D) → Camera3D → Flashlight (SpotLight3D)
└── SafeRoom (Area3D; Director forces relief inside)
```

## Genre code

The director cheats; the monster does not. The director knows where the
player is and nudges the monster towards the area. The monster must still
see or hear the player before it attacks.

```gdscript
extends Node

signal hint_area(position: Vector3)

@export var build_rate := 0.04   # tension per second while exploring
@export var peak := 1.0
@export var relief_seconds := 25.0

var tension := 0.0
var relief_left := 0.0

func tick(delta: float, player_pos: Vector3, rng: RandomNumberGenerator) -> void:
	if relief_left > 0.0:
		relief_left -= delta
		tension = maxf(tension - delta * 0.1, 0.0)
		return
	tension = minf(tension + build_rate * delta, peak)
	if tension >= peak:
		relief_left = relief_seconds
	elif rng.randf() < tension * delta * 0.2:
		# A point near the player, never on the player.
		var offset := Vector3(rng.randf_range(-12, 12), 0, rng.randf_range(-12, 12))
		hint_area.emit(player_pos + offset)
```

Raise tension on sightings and chases, drop it on escapes; end a chase
by entering relief so the player gets a quiet stretch.

## Pitfalls

- Constant maximum tension. Players go numb. Use build-up, peak, relief.
- Instant detection. Fill a suspicion meter over 1 to 3 s, scaled by
  distance, light and movement, and show or play a cue while it fills.
- A monster on a fixed loop is a puzzle, not a threat. Let the director move
  its investigation target.
- Vision from `Area3D` overlaps sees through walls. Cast rays with
  `PhysicsDirectSpaceState3D.intersect_ray()` to head and torso, and exclude
  the monster's own RID.
- Darkness so deep the floor is invisible. Hide threats, not the path; use
  rim light and a battery-limited flashlight.
- Jump scares as the main tool. Build anticipation with sound first.
- Heavy scare assets loaded on the spot with `load()`. Request them early
  with `ResourceLoader.load_threaded_request()`.
- Monster logic at full rate when far and off-screen. Slow it down with
  `VisibleOnScreenNotifier3D` and distance.
- Screen shaders written for Godot 3. Read the screen with a `sampler2D`
  uniform that has `hint_screen_texture`.
- Several chasers that stack in a corridor. Turn on avoidance on their
  `NavigationAgent3D` nodes.
- Item Resources with ammo or charge shared by every copy. Duplicate them
  when they enter the inventory.
