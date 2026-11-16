# Scene architecture

Target engine: Godot 4.7. How to structure scenes, scripts, autoloads and the project so that a large game stays changeable: dependency direction, lifetime, lookup, process order, scene swaps.

## Use this shelf when

- You decide where a system lives: scene, script class, autoload, static class or Resource.
- A scene breaks when it is reused somewhere else, or when a node is moved in the tree.
- You plan level changes, persistent actors, loading stages or a demo / server variant.
- Two systems need a fixed update order, or part of the game must freeze while another runs.
- Several people (or agents) edit the same scenes and merges hurt.

## How the engine works here

A scene file is a `PackedScene`: a table of nodes, property overrides and connections. An instanced scene in another scene stores only the path plus the properties that differ. `owner` records which scene root a node was saved with. It drives saving, `%Name` lookup and what `find_child` sees by default. `scene_file_path` is set only on the root of an instanced scene.

Groups are stored in the `SceneTree` as name -> node list. A node outside the tree is in no list. Scene groups and global groups share one namespace.

Autoloads are plain nodes added under `root` before the main scene, in the order of the Autoload list. Nothing makes them unique: the same script can be instanced again elsewhere.

Instancing a `PackedScene` runs in engine code. Building the same subtree in script crosses the script API once per statement and gets slower as the tree grows.

## Advanced techniques

### Scenes without outward dependencies

A scene should run alone (F6). What it needs from outside is injected by its owner. Five channels:

| Channel | Use | Note |
|---|---|---|
| Signal the child emits | Report that something happened | Past-tense names; never to start behaviour |
| `Callable` property | Let the owner supply behaviour | Target does not need to own the method |
| Object or Node reference (`@export var target: Node`) | Give a collaborator | Typed export checks in editor |
| `NodePath` property | Late lookup | Breaks on moves unless set in the editor |
| Method name string plus `call()` | Dynamic dispatch | Weakest; no static check |

Siblings never look each other up. Their common ancestor wires them. The rule in short: call down, signal up, and let the nearest common owner connect sideways.

### Parent by lifetime, not by position

Make B a child of A only when freeing A should free B. If B merely follows A, keep B elsewhere and use `RemoteTransform2D` / `RemoteTransform3D` (push A's transform to a `remote_path`). To stop transform inheritance without moving the node, set `top_level = true` (`CanvasItem`, `Node3D`), or put a plain `Node` between them: it has no transform, so nothing passes through. Pays off for the player, the camera and UI that must survive a room swap: no "rescue the player first" special case in the swap code.

### One entry point, swap below it

`Main` (controller script) with `World` and `GUI` children. Level changes replace children of `World`; `GUI` and session state stay. `change_scene_to_file` replaces everything: small games only.

| Way to leave a scene | Memory | Processing | Data |
|---|---|---|---|
| Free it | Released | Stops | Lost; reload costs time |
| Hide it (`hide()`, disable collision) | Kept | Continues | Live |
| `remove_child` and keep the reference | Kept | Stops; leaves groups | Stale but intact |

A removed subtree is an orphan: free it or re-add it, or it leaks. `change_scene_to_node(node)` (since 4.6) takes a scene you already built and configured (level seed, save data). The old scene leaves the tree at once; the new one is added at the end of the frame, so `current_scene` is `null` in between. `await get_tree().scene_changed` before you touch the new scene.

### Scene unique names as the internal API

`%Name` is resolved through a table on the owner, so it is a cached lookup, not a search. `find_child` walks every descendant on each call. Rules: names are per owner (a parent scene cannot see `%Hilt` of an instanced child, but `get_node("Hand/Sword/%Hilt")` and `%Sword/%Hilt` work); a node created at run time needs `owner` set and `unique_name_in_owner = true` to join the table; two nodes with the same unique name under one owner hide each other. Use it for internal references in UI scenes: layout changes then cost no script edits.

### Inheritance, composition, editable children

| Tool | Stores | Good for | Limit |
|---|---|---|---|
| Inherited scene | Differences from the base | Variants; imported 3D edits | Base nodes cannot be removed |
| Instanced child scene | Path plus root overrides | Reuse by composition | Parent sees only the root |
| Editable Children | Inner-node overrides, in the parent | One-off tweaks | Parent depends on inner layout |

Prefer exported properties on the child root over Editable Children. Sub-resources of the base are shared, not copied. Since 4.6 scene files store a `unique_id` per node, so overrides in inherited and instancing scenes survive a rename or move of the base node. Before 4.6 they were matched by path, and restructuring a base scene could silently drop them. Scenes saved by older versions have no ids: run Project > Tools > Upgrade Project Files once to re-save every scene.

### InstancePlaceholder for staged loading

Mark an instanced scene "Load As Placeholder". At run time the node is an `InstancePlaceholder` that holds only the path and the overridden properties. `create_instance(replace, custom_scene)` loads and inserts the real scene; `get_instance_path()` gives the path for a threaded preload first. Pays off for rooms behind doors and rarely opened UI: the parent scene loads fast. Traps: a placeholder has no transform, so its children sit at the viewport origin until replaced; `create_instance` loads on the calling thread and is not thread-safe.

### Groups as query sets and tags

`get_nodes_in_group` builds a new `Array` in tree order on every call: fine per event, wasteful per frame per node. Use `get_first_node_in_group` for a single target and `get_node_count_in_group` for "any left?". Groups also work as interface tags: `is_in_group(&"damageable")` is a cheap capability test across scene borders. For hot sets, a typed static registry avoids the copy:

```gdscript
class_name Raider
extends CharacterBody2D

static var all: Array[Raider] = []

func _enter_tree() -> void:
	all.append(self)

func _exit_tree() -> void:
	all.erase(self)
```

### Process mode and priority as structure

`process_mode` is inherited down the tree, so one property on a subsystem root controls the whole subsystem. `PROCESS_MODE_DISABLED` freezes a subtree without removing it (processing and input stop, signals still arrive). `PROCESS_MODE_WHEN_PAUSED` on the menu root and `PROCESS_MODE_ALWAYS` on audio and network roots give pause without flags in every script. `SceneTree.paused` also stops both physics servers; nodes that keep processing get no physics unless you call `PhysicsServer3D.set_active(true)`. Order: `process_priority` and `process_physics_priority` (lower runs first) beat tree order. Give fixed numbers to stages, for example input -100, simulation 0, camera 100, so a camera never reads a target position from the last frame. For heavy independent subtrees see `process_thread_group` in the shelf `threads-and-concurrency`; the core team lists "many nodes still assume a single thread" as an open priority, so test each node type before you rely on it.

### Autoload only for isolated systems

An autoload fits when the system owns all its data, must be reachable from everywhere, and works in isolation (save service, scene loader, audio bus control). It does not fit when it edits other systems' data. Lighter options:

- Stateless helpers: `class_name` plus `static func`. No node, no load order.
- Shared state without a node: `static var`. Cleared only when the script unloads.
- Shared tunable data: one `Resource` file that several scenes export.

### Scenes as named types

A script class gives a type name; a scene gives fast, editable construction. Join them with a namespace script, so callers never write paths (the 2 scene files are from your project):

<!-- gdscript-check: skip -->
```gdscript
class_name Scenes
extends RefCounted

const ENEMY: PackedScene = preload("res://enemies/enemy.tscn")
const BULLET: PackedScene = preload("res://weapons/bullet.tscn")
```

Trap: a script that preloads the scene it is attached to forms a load cycle. Keep the constants in a separate script, or use `load()` inside a static factory.

### Contracts the editor can check

- `@tool` plus `_get_configuration_warnings()`: a scene states what it needs (a child of some type, a non-empty export) and the Scene dock shows the warning icon.
- Typed `@export var hurtbox: Area2D`: the Inspector accepts only matching nodes.
- `@abstract` on classes and methods (since 4.5): a base "interface" that cannot be instanced; a subclass that misses an abstract method is an error at parse time.
- A signal name with a leading underscore (hidden from completion and docs since 4.6) marks a signal as internal to the scene.

### Feature tags as structural switches

`OS.has_feature("demo")` reads a custom tag from the export preset. A project setting can have a per-tag override (`application/run/main_scene.demo`), so a demo, a `dedicated_server` build or a platform can start from another root scene with no code branch. Read such values in code with `ProjectSettings.get_setting_with_override`. Custom tags exist only in exported builds, not when running from the editor. To run a variant root from the editor or CI, start it directly: `--scene <path or uid>` (since 4.5).

## Niche but useful

- `add_child(node, false, Node.INTERNAL_MODE_BACK)`: helper children hidden from `get_children()` and from index math of user code.
- `Node.replace_by(node, true)`: swap a stand-in for the real node and keep groups and children.
- `PackedScene.get_state()` (`SceneState`): read node types, groups and connections of a scene without instancing it. Good for validation tools and level catalogs.
- Set properties before `add_child`: in the tree many setters trigger updates (transforms, server calls, layout). Matters in spawners. Exception: values that need the tree (`global_position`).
- `.gdignore` in a folder: the editor skips it (raw art sources, docs). Faster first import.
- A custom `MainLoop` or a `SceneTree` subclass: see the shelf `engine-internals`.
- Keep `rpc` calls and `multiplayer` checks in a thin adapter node so gameplay scenes also run with no peer.

## Traps

- `get_node("../../Something")` in a reusable scene: an outward dependency in disguise.
- A global sound-pool autoload: global state, untraceable callers. Let scenes own their players.
- Belief "an autoload is a singleton". It is only an auto-added node.
- `current_scene` right after a scene change call: it is still `null` or the old one.
- Mixed-case file names work on Windows and fail on case-sensitive systems. Use snake_case.
- Deep chains of inherited scenes: every level is one more file an override can hide in. Rule of thumb (not from the docs): stop at two levels and compose beyond that.
- Version control: commit `.uid` and `.import` files, ignore `.godot/`. Set `* text=auto eol=lf`. Move a `.uid` file together with its script. A missing one only warns on your machine and breaks references on the next clone.
- Keep scenes small and single-purpose so two workers rarely edit the same `.tscn`.

## Version notes

- 4.4: `.uid` files for scripts and shaders; references survive moves done outside the editor.
- 4.5: `@abstract`; `SceneTree.scene_changed`; `--scene`; custom feature tags can be picked in the Project Settings override dialog; `Node.duplicate` no longer copies internal nodes.
- 4.6: per-node `unique_id` in scene files; `SceneTree.change_scene_to_node`; autoload entries can be stored as UIDs; underscore signals hidden.
- 4.7: no structural change in the release page or upgrade notes for this area.

## Related skills

- `godot-scene-organization`: step-by-step scene splitting and threaded level loading.
- `godot-component-system`: building component nodes; this shelf gives the dependency rules.
- `godot-dependency-injection`: autoload start-up order and injection patterns in detail.
- `godot-event-bus`: when a global signal hub is justified.
- `godot-safe-refactor`: moving and renaming scenes without breaking references.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/best_practices/scene_organization.html
- https://docs.godotengine.org/en/stable/tutorials/best_practices/scenes_versus_scripts.html
- https://docs.godotengine.org/en/stable/tutorials/best_practices/autoloads_versus_regular_nodes.html
- https://docs.godotengine.org/en/stable/tutorials/best_practices/project_organization.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/scene_unique_nodes.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/change_scenes_manually.html
- https://docs.godotengine.org/en/stable/tutorials/export/feature_tags.html
- https://godotengine.org/article/uid-changes-coming-to-godot-4-4/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-1/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-3/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-5/
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-2/
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/priorities/
