# Area costs, navigation links, moving obstacles and stuck recovery

Adds region costs so agents prefer roads, `NavigationLink2D/3D` for jumps, ladders and teleports, the two kinds of `NavigationObstacle`, unreachable-target handling, stuck detection, and leader-relative formations. Read it when agents take silly routes, need to cross gaps, stack up behind moving hazards, or stop moving with no error.

All code targets Godot 4.7. Examples are 2D; the 3D nodes have the same names and properties.

## Region costs: prefer roads over mud

The pathfinder finds the cheapest route, not the shortest. Each `NavigationRegion2D` has two costs:

| property | default | meaning |
|---|---|---|
| `travel_cost` | 1.0 | Multiplies the length of path inside this region. 3.0 makes mud count as three times as long. |
| `enter_cost` | 0.0 | Added once when a path enters this region from another. Use it for "avoid unless needed", such as a door or water. |

Split the level into regions by surface (road, grass, swamp) and set the costs on each. Agents then take a longer road instead of crossing a swamp, but still cross it when the detour is long enough.

- Costs affect route choice only. They do not slow the agent. Change the agent's speed in its own script when it is in a slow region.
- Regions must share edges, or be within the map's `edge_connection_margin`, for paths to cross between them.
- To keep a unit type out of a region entirely, use navigation layers (SKILL.md section 1), not a huge cost.

## Navigation links: jumps, ladders and teleports

A `NavigationLink2D` connects two points that the navigation mesh does not join: a gap to jump, a ladder, a teleporter. The pathfinder uses the link like a path segment with its own `enter_cost` and `travel_cost`. The agent reports when it reaches the start of a link, and your code performs the special move.

1. Add a `NavigationLink2D`. Set `start_position` and `end_position` (local to the link node). Each point must lie on or near a navigation region.
2. Set `bidirectional = false` for one-way links, such as a drop down a ledge.
3. Set `navigation_layers` so only units that can use it (jumpers, not tanks) path through it.

```gdscript
extends CharacterBody2D

@export var speed: float = 140.0

@onready var _agent: NavigationAgent2D = $NavigationAgent2D

var _link_target: Vector2 = Vector2.ZERO
var _on_link: bool = false


func _ready() -> void:
    _agent.link_reached.connect(_on_link_reached)


func _on_link_reached(details: Dictionary) -> void:
    # details holds "link_entry_position", "link_exit_position" and "owner" (the link node).
    _link_target = details["link_exit_position"]
    _on_link = true


func _physics_process(delta: float) -> void:
    if _on_link:
        # The special move: here a straight hop; a real game plays a jump or climb.
        global_position = global_position.move_toward(_link_target, speed * 1.5 * delta)
        if global_position.distance_to(_link_target) < 2.0:
            _on_link = false
        return
    if _agent.is_navigation_finished():
        velocity = Vector2.ZERO
        return
    var next: Vector2 = _agent.get_next_path_position()
    velocity = global_position.direction_to(next) * speed
    move_and_slide()
```

The `owner` entry is the link node. Store data on it (metadata or an exported property) to choose the move: `"jump"`, `"ladder"`, `"teleport"`.

## Moving obstacles: avoidance or carving

`NavigationObstacle2D` has two separate jobs. Choose per obstacle.

| goal | settings | cost |
|---|---|---|
| Agents steer around a moving hazard (a rolling boulder, a vehicle) | `avoidance_enabled = true`, a `radius`, and `velocity` set each tick | Cheap. Only agents with `avoidance_enabled` react. Paths do not change; agents dodge locally. |
| Paths go around a placed object (a crate, a turret, a closed gate) | `affect_navigation_mesh = true` with `vertices` for its outline; the region rebakes | A rebake. Do not use it for things that move every tick. |

`carve_navigation_mesh = true` makes the outline cut the mesh exactly, ignoring the agent radius offset that the bake normally adds. Use it for an object whose outline already includes the clearance you want.

Avoidance does not stop an agent from walking into a static wall; only the path does that. A hazard that blocks a corridor for a long time should change the mesh, not only the avoidance.

## Unreachable targets

When the target is off the mesh or on an island with no connection, the agent still builds a path: to the closest reachable point. It then reports `is_navigation_finished()` at that point, and a patrol that waits for "finished" can loop on it forever.

Check reachability after the path is built:

```gdscript
extends CharacterBody2D

signal target_unreachable(target: Vector2)

@onready var _agent: NavigationAgent2D = $NavigationAgent2D


func go_to(target: Vector2) -> void:
    _agent.target_position = target
    # The path is built on the next physics tick; check after it.
    await get_tree().physics_frame
    if not _agent.is_target_reachable():
        target_unreachable.emit(target)
```

`get_final_position()` returns where the path really ends. Compare it with the target for a softer test ("close enough"). On an unreachable patrol point, skip to the next point instead of waiting.

## Stuck detection

An agent can stop short of its goal with no error: pushed into a corner by others, caught on a collider that the mesh ignores, or blocked by a closed door. Measure progress over a time window. If the agent has a path and moved less than a set distance in that window, it is stuck.

```gdscript
extends Node

signal stuck

@export var body: CharacterBody2D
@export var agent: NavigationAgent2D
@export var window: float = 1.0       # seconds
@export var min_progress: float = 12.0  # distance that counts as moving
@export var max_retries: int = 2

var _timer: float = 0.0
var _last_pos: Vector2 = Vector2.ZERO
var _retries: int = 0


func _physics_process(delta: float) -> void:
    if agent.is_navigation_finished():
        _timer = 0.0
        _retries = 0
        _last_pos = body.global_position
        return
    _timer += delta
    if _timer < window:
        return
    var moved: float = body.global_position.distance_to(_last_pos)
    _timer = 0.0
    _last_pos = body.global_position
    if moved >= min_progress:
        _retries = 0
        return
    if _retries < max_retries:
        _retries += 1
        # Rebuild the path from where the agent stands now.
        agent.target_position = agent.target_position
    else:
        _retries = 0
        stuck.emit()
```

Respond in steps: first rebuild the path (assigning `target_position` again forces a new query), then nudge the body sideways, then give up and pick another goal. Let the AI's state machine handle `stuck`, for example by moving to a search state.

## Formations

Many agents with the same target clump into a line and push each other. Give each follower a fixed offset from a leader, rotated by the leader's heading, and path each follower to its own slot.

```gdscript
extends Node2D

@export var leader: Node2D
@export var followers: Array[NavigationAgent2D] = []
@export var spacing: float = 40.0
@export var retarget_distance: float = 24.0

var _last_leader_pos: Vector2 = Vector2.INF


func _physics_process(_delta: float) -> void:
    if leader.global_position.distance_to(_last_leader_pos) < retarget_distance:
        return
    _last_leader_pos = leader.global_position
    var heading: float = leader.global_rotation
    for i in followers.size():
        followers[i].target_position = leader.global_position + _slot(i).rotated(heading)


## A wedge: two followers per row, each row further back.
func _slot(i: int) -> Vector2:
    var row: int = i / 2 + 1
    var side: float = -1.0 if i % 2 == 0 else 1.0
    return Vector2(-row * spacing, side * row * spacing * 0.6)
```

Retarget the slots only when the leader has moved a set distance, like any chase target. When a slot lies off the mesh, the follower paths to the nearest reachable point, which is usually the right fallback. Keep avoidance on, so followers still dodge each other on the way.
