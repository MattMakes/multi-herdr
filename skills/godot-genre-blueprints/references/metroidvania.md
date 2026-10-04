# Metroidvania

One large, connected world where new abilities open paths the player already
saw. Super Metroid, Hollow Knight and Ori are the reference points.

## Core loop

Explore → hit a lock (a gap, a wall, deep water) → find the key (an ability
or a boss) → return to old locks → new areas and shortcuts open.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Precise movement | the platformer base: coyote time, buffer, wall jump | `godot-player-controller`, `platformer.md` |
| Ability states | dash, double jump, wall cling as states that unlock | `godot-state-machine`, `godot-ability-system` |
| World state | abilities, opened doors, defeated bosses, collected items; global | `godot-save-load`, `godot-dependency-injection` (autoloads) |
| Rooms | one scene per room, threaded load, door-to-door spawn | `godot-scene-organization` |
| Map | grid of visited cells, fog until visited | `godot-ui`, `godot-2d-essentials` |
| Room camera | limits per room, smooth hand-off at doors | `godot-camera-system`, `godot-phantom-camera` |
| Combat | contact damage, i-frames, bosses | `godot-combat-system` |

## Scene tree (4.7)

```text
Game (Node)
├── WorldState (autoload; ability flags, room flags, map cells)
├── RoomHost (Node2D; holds the one live room)
│   └── Room_A3 (Node2D, its own .tscn)
│       ├── TileMapLayer (ground) + TileMapLayer (one-way)
│       ├── Doors (Area2D per exit; target room path + spawn id)
│       ├── Gates (nodes that check WorldState abilities)
│       └── Pickups (each with a stable id)
├── Player (CharacterBody2D; survives room swaps)
├── Camera2D (limits set from the room's bounds)
└── HUD (CanvasLayer; health, map overlay)
```

## Genre code

Rooms are freed when the player leaves, so they must not keep progress.
Each pickup and gate asks the world state by a stable id when it enters the
tree.

```gdscript
extends Node

signal ability_gained(id: StringName)

var abilities: Dictionary[StringName, bool] = {}
var room_flags: Dictionary[StringName, bool] = {}   # "A3/chest_1" -> true
var visited: Dictionary[Vector2i, bool] = {}

func has_ability(id: StringName) -> bool:
	return abilities.get(id, false)

func grant(id: StringName) -> void:
	abilities[id] = true
	ability_gained.emit(id)

func flag(room: StringName, thing: StringName) -> bool:
	return room_flags.get(StringName("%s/%s" % [room, thing]), false)

func set_flag(room: StringName, thing: StringName) -> void:
	room_flags[StringName("%s/%s" % [room, thing])] = true
```

A chest calls `WorldState.flag(room_id, name)` in `_ready()` and starts open
if the flag is set. Typed dictionaries need Godot 4.4 or later.

## Pitfalls

- A one-way path that traps the player with the current abilities. Every
  one-way drop needs an exit the player can use at that point. Test each
  with the abilities available when it is first reachable.
- Empty dead ends. Every remote corner gives something: an item, lore, a
  shortcut.
- Backtracking that stays slow. New movement should make old routes faster,
  and shortcuts should open behind the player.
- Progress kept in room scripts. It is lost when the room is freed. Keep it
  in the autoload.
- Room scenes loaded with `load()` at the door. Start
  `ResourceLoader.load_threaded_request()` when the player nears the door;
  swap with `call_deferred` when it reports loaded.
- Scene changes from a worker thread. Only the main thread touches the
  tree.
- Map cells as `Vector2`. Use `Vector2i`.
- One ability, one use. A dash that also dodges attacks earns its place;
  give abilities a traversal use and a combat use.
- Shared sub-resources inside an instanced room. Use
  `resource_local_to_scene` for per-room data.
- No landmarks. Give each area a look and a sound so the player can build a
  mental map.
