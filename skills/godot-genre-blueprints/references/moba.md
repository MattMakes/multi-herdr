# MOBA

Two teams of heroes push lanes of AI minions toward each other's base, with
towers between. League of Legends and Dota 2 are the reference points.

## Core loop

Farm minions in lane → trade damage with the enemy hero → roam and gank
other lanes → push towers → destroy the enemy core. A match lasts 20 to 40
minutes.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Authoritative server | server owns damage, mana, gold, cooldowns | `godot-dedicated-server`, `godot-multiplayer-basics` |
| Replication and prediction | low tick rate (10 to 20 Hz), interpolation, batched minion state | `godot-multiplayer-sync` |
| Click to move | ray from camera to ground, path, attack-move | `godot-input-handling`, `godot-ai-navigation` |
| Hero abilities | QWER, cooldowns, mana, skill-shot indicators | `godot-ability-system`, `godot-combat-system` |
| Status effects | stun, slow, silence as data with a duration | `godot-ability-system` |
| Minions and jungle | waves on a timer, lane paths, leashing camps | `godot-gameplay-loops` (waves), `godot-ai-navigation`, `godot-state-machine` |
| Fog of war | team vision, grid or texture mask | `godot-shader-basics` |
| Gold and items | shop, bounties | `godot-economy-system`, `godot-inventory-system` |

## Scene tree (4.7)

```text
Match (Node3D)
├── Map (Node3D; NavigationRegion3D baked from simple collision only)
│   ├── Lanes (Path3D per lane) + Towers + Cores
│   └── Jungle (camps with spawn markers)
├── Server (Node; waves, tower targeting, damage; server only)
├── Heroes (Node3D; MultiplayerSpawner)
│   └── Hero (CharacterBody3D) → NavigationAgent3D, Abilities, Synchronizer
├── Minions (Node3D; one manager owns all minion state)
├── FogOfWar (SubViewport mask + terrain shader; client only)
└── HUD (CanvasLayer)
```

## Genre code

Towers must protect their own heroes: when an enemy hero hits an allied hero
in range, the tower switches to that hero at once. Otherwise minions come
first.

```gdscript
extends Node3D

@export var range_sq := 36.0

var target: Node3D = null
var _attackers_of_allies: Dictionary[Node3D, int] = {}   # enemy hero -> last hit frame

func report_ally_hero_hit(attacker: Node3D) -> void:
	_attackers_of_allies[attacker] = Engine.get_physics_frames()

func pick(enemies_in_range: Array[Node3D]) -> Node3D:
	var now := Engine.get_physics_frames()
	var best: Node3D = null
	var best_rank := 99
	var best_d := INF
	for e in enemies_in_range:
		var rank := 2 if e.is_in_group(&"heroes") else 1
		if now - _attackers_of_allies.get(e, -1000) < 60:
			rank = 0   # dive protection: hero that hit an allied hero
		var d := global_position.distance_squared_to(e.global_position)
		if rank < best_rank or (rank == best_rank and d < best_d):
			best = e
			best_rank = rank
			best_d = d
	return best
```

Keep the current target while it stays in range and nothing with a better
rank appears; switching every tick looks erratic.

## Pitfalls

- The client sends "I did 300 damage". The client sends "cast Q toward
  this point"; the server checks mana, cooldown and range and computes the
  hit.
- Movement sent reliable at 60 Hz. Replicate at 10 to 20 Hz, unreliable, and
  interpolate on clients.
- One `MultiplayerSynchronizer` per minion. Hundreds of minions need one
  manager that packs their state into one array per tick.
- Every minion re-paths every frame. Spread path updates over frames, and
  query paths in `_physics_process()`.
- A navigation mesh baked from detailed visual meshes. Bake from simple
  collision shapes.
- `path_search_max_polygons` too low for a large map: agents stop short.
  Raise it.
- A shared ability Resource changed by a buff changes it for every hero.
  Duplicate per hero.
- No comeback rules. Add bounties on fed heroes and catch-up XP.
- Jungle camps that follow forever. Leash them: past a radius, they return
  home and reset health.
- Pathing right after spawn before the navigation map has synced. Wait one
  physics frame, or check `NavigationServer3D.map_get_iteration_id()` is
  above 0.
