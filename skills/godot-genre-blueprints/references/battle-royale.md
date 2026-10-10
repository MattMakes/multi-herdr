# Battle royale

Many players drop on one map, loot what they find, and a shrinking safe zone
forces fights until one player or team is left.

## Core loop

Drop → loot → move with the zone → fight → last one standing. Each match is
self-contained; progression, if any, is cosmetic and lives outside the match.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Dedicated server | headless build, server-only branches, many peers | `godot-dedicated-server` |
| Replication | transforms unreliable, state on change, interest by distance | `godot-multiplayer-sync`, `godot-multiplayer-basics` |
| Lag compensation | server rewinds hit targets to the shooter's view time | `godot-multiplayer-sync` |
| Large map | terrain, streaming, foliage in MultiMesh | `godot-3d-essentials`, `godot-optimization`, `open-world.md` |
| Weapons and hits | hitscan or projectile, server-validated | `shooter-fps.md`, `godot-combat-system` |
| Backpack and attachments | slot limits, server-owned contents | `godot-inventory-system` |
| Drop sequence | plane → freefall → glide → grounded states | `godot-state-machine` |
| Release builds | server and client export presets | `godot-export-pipeline` |

## Scene tree (4.7)

```text
Match (Node3D)
├── Map (Node3D; terrain chunks, MultiMeshInstance3D foliage)
├── Zone (Node3D; StormZone script, inverted sphere mesh for the wall)
├── LootSpawns (Node3D; Marker3D points, server fills them)
├── Players (Node3D; MultiplayerSpawner spawns one Player per peer)
│   └── Player (CharacterBody3D)
│       ├── MultiplayerSynchronizer (transform; replication mode Always)
│       └── MultiplayerSynchronizer (health, ammo; replication mode On Change)
└── HUD (CanvasLayer; client only)
```

## Genre code

The zone picks each next circle inside the current one, so the new circle is
always reachable. Damage uses the distance to the centre, not an Area3D, so
a fast player cannot skip the edge between physics ticks.

```gdscript
extends Node3D

@export var phase_radii: PackedFloat32Array = [800.0, 500.0, 300.0, 150.0, 60.0, 0.0]
@export var phase_dps: PackedFloat32Array = [1.0, 2.0, 4.0, 8.0, 12.0, 20.0]
var center := Vector2.ZERO
var radius := 800.0
var phase := 0

func next_circle(rng: RandomNumberGenerator) -> Vector2:
	var next_r: float = phase_radii[phase + 1]
	# Any point within (radius - next_r) of the centre keeps the new circle inside.
	var slack := maxf(radius - next_r, 0.0)
	var dist := slack * sqrt(rng.randf())
	return center + Vector2.from_angle(rng.randf() * TAU) * dist

func damage_for(pos: Vector3, delta: float) -> float:
	var flat := Vector2(pos.x, pos.z)
	if flat.distance_to(center) <= radius:
		return 0.0
	return phase_dps[phase] * delta
```

Run `next_circle` on the server only. Send the new centre and radius to
clients with one reliable RPC per phase; clients animate the wall locally.

## Pitfalls

- Reliable RPCs for movement queue up behind lost packets. Replicate
  transforms with mode Always (sent unreliable) and health, ammo and
  inventory with mode On Change (sent reliable).
- Every player replicated to every peer at full rate. Lower the rate for
  far players with `replication_interval`, or hide them with visibility
  filters on `MultiplayerSynchronizer`.
- The client says "I hit" or "I picked up". The server checks range, line of
  sight and item existence before it applies anything.
- A fully random next circle can leave the old one. Use the contained pick
  above.
- Thousands of loot pickups as full scenes. Pool them, and keep far ones as
  data until a player comes near.
- `print()` in the server tick. Console output blocks and costs frames at
  scale.
- RPCs sent before the peer is connected. Wait for `connected_to_server` on
  the client and `peer_connected` on the server.
- Never decode untrusted packets with objects allowed: no
  `bytes_to_var_with_objects`, no `get_var(true)`. Object decoding lets a
  peer run code on the server.
- A mobile client with no internet permission fails silently. Turn on the
  permission in the Android export preset.
