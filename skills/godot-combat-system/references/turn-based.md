# Turn-based combat

Four turn models, one shared actor shape, and grid movement. Pick the model
with the table in `SKILL.md`, then read its section.

## The actor

Every model reads the same few values. Keep them in a small class (or in the
actor's stats resource):

```gdscript
class_name TurnActor
extends RefCounted

var id: int = 0           # stable: used to break ties
var name: String = ""
var speed: int = 10
var alive: bool = true


func _init(p_id: int = 0, p_name: String = "", p_speed: int = 10) -> void:
	id = p_id
	name = p_name
	speed = p_speed
```

`id` is a fixed number from the encounter data (spawn order, party slot). Do
not use `get_instance_id()` for ties: it changes between runs, so replays
differ.

## Round queue

Each living actor acts once per round, fastest first.

```gdscript
class_name RoundQueue
extends RefCounted

signal round_started(round_number: int)
signal turn_started(actor: TurnActor)

var round_number: int = 0
var _order: Array[TurnActor] = []
var _cursor: int = -1
var _actors: Array[TurnActor] = []


func setup(actors: Array[TurnActor]) -> void:
	_actors = actors.duplicate()
	round_number = 0
	_start_round()


## Ends the current turn and starts the next one. Returns the next actor,
## or null when one side is gone (the caller checks the win state).
func next_turn() -> TurnActor:
	while true:
		_cursor += 1
		if _cursor >= _order.size():
			if _actors.filter(func(a: TurnActor) -> bool: return a.alive).is_empty():
				return null
			_start_round()
			_cursor = 0
		var actor: TurnActor = _order[_cursor]
		if actor.alive:
			turn_started.emit(actor)
			return actor
	return null


## Call when a speed stat changes mid-round. The new order applies to
## the actors who have not acted yet.
func resort_remaining() -> void:
	var done: Array[TurnActor] = _order.slice(0, _cursor + 1)
	var rest: Array[TurnActor] = _order.slice(_cursor + 1)
	rest.sort_custom(_faster)
	_order = done + rest


func _start_round() -> void:
	round_number += 1
	_order = _actors.duplicate()
	_order.sort_custom(_faster)
	_cursor = -1
	round_started.emit(round_number)


static func _faster(a: TurnActor, b: TurnActor) -> bool:
	if a.speed != b.speed:
		return a.speed > b.speed
	return a.id < b.id
```

A dead actor stays in the arrays with `alive = false` and is skipped. Nothing
removes elements during the loop. Remove the dead at the round start if the
list grows large.

The battle controller drives it:

1. `var actor := queue.next_turn()`.
2. For a player actor, show the menu and `await` the menu's
   `command_chosen` signal. For an AI actor, call its decision function.
3. Run the command through the damage pipeline (`SKILL.md`).
4. Tick the actor's status effects and reset its per-turn values.
5. Emit `turn_ended`, check the win state, then go to step 1.

## Timeline (CTB)

Each actor gains `speed` points per tick and acts at 100. A fast actor acts
more often. The same code that runs the timeline predicts it, which gives the
turn-order bar for the UI.

```gdscript
class_name TimelineQueue
extends RefCounted

const THRESHOLD: int = 100

var actors: Array[TurnActor] = []
var charge: Dictionary[int, int] = {}  # actor id -> points


func add(actor: TurnActor) -> void:
	actors.append(actor)
	charge[actor.id] = 0


## Advances time until an actor reaches the threshold, and returns it.
func next_actor() -> TurnActor:
	return _advance(charge)


## The next count actors in order, from a copy of the state.
func preview(count: int) -> Array[TurnActor]:
	var sim: Dictionary[int, int] = charge.duplicate()
	var out: Array[TurnActor] = []
	for i: int in count:
		var actor: TurnActor = _advance(sim)
		if actor == null:
			break
		out.append(actor)
	return out


## Delays an actor (a "slow" or "knock back on the timeline" effect).
func delay(actor: TurnActor, points: int) -> void:
	charge[actor.id] -= points


func _advance(state: Dictionary[int, int]) -> TurnActor:
	var living: Array[TurnActor] = actors.filter(func(a: TurnActor) -> bool: return a.alive)
	if living.is_empty():
		return null
	while true:
		var ready: TurnActor = null
		for actor: TurnActor in living:
			if state[actor.id] < THRESHOLD:
				continue
			if ready == null or state[actor.id] > state[ready.id] \
					or (state[actor.id] == state[ready.id] and actor.id < ready.id):
				ready = actor
		if ready != null:
			state[ready.id] -= THRESHOLD
			return ready
		for actor: TurnActor in living:
			state[actor.id] += actor.speed
	return null
```

An interrupt (a counterattack, a reaction) is a turn outside the timeline:
the controller runs it, then continues with the same timeline. It does not
change `charge` unless the design says the reaction costs time.

## Gauges (ATB)

Gauges fill in real time. When one is full, the battle pauses for that
actor's command (or keeps running in an "active" mode).

```gdscript
extends Node

signal actor_ready(actor: TurnActor)

const FULL: float = 100.0

var actors: Array[TurnActor] = []
var gauges: Dictionary[int, float] = {}
var running: bool = true


func _physics_process(delta: float) -> void:
	if not running:
		return
	for actor: TurnActor in actors:
		if not actor.alive:
			continue
		var g: float = gauges.get(actor.id, 0.0) + actor.speed * delta
		gauges[actor.id] = minf(g, FULL)
		if g >= FULL:
			running = false  # wait mode: stop all gauges for the command
			actor_ready.emit(actor)
			return


func command_done(actor: TurnActor) -> void:
	gauges[actor.id] = 0.0
	running = true
```

Use `delta`, so the fill rate does not depend on the frame rate. For a turn
preview in ATB, simulate the fill in steps on a copy of `gauges`, the same way
`TimelineQueue.preview` does.

## Action points and phases

```gdscript
class_name TurnBudget
extends RefCounted

enum Phase { START, MAIN, END }

signal phase_changed(phase: Phase)

var max_points: int = 2
var points: int = 2
var phase: Phase = Phase.START


func can_afford(cost: int) -> bool:
	return phase == Phase.MAIN and points >= cost


func spend(cost: int) -> bool:
	if not can_afford(cost):
		return false
	points -= cost
	return true


func advance_phase() -> void:
	match phase:
		Phase.START:
			phase = Phase.MAIN
		Phase.MAIN:
			phase = Phase.END
		Phase.END:
			points = max_points  # reset before the next turn starts
			phase = Phase.START
	phase_changed.emit(phase)
```

Points are `int`. If the design needs fractions, scale them (for example 10
points per move) instead of a float.

## Grid tactics

`AStarGrid2D` is made for square grids:

```gdscript
extends Node

var grid := AStarGrid2D.new()


func build(size: Vector2i, walls: Array[Vector2i]) -> void:
	grid.region = Rect2i(Vector2i.ZERO, size)
	grid.cell_size = Vector2(1, 1)
	grid.diagonal_mode = AStarGrid2D.DIAGONAL_MODE_NEVER
	grid.update()  # after region and cell settings; a rebuild clears solid points
	for cell: Vector2i in walls:
		grid.set_point_solid(cell, true)


func path_cells(from: Vector2i, to: Vector2i) -> Array[Vector2i]:
	return grid.get_id_path(from, to)


func set_occupied(cell: Vector2i, occupied: bool) -> void:
	# Units block cells. No update() is needed for solid changes.
	grid.set_point_solid(cell, occupied)
```

- `get_id_path` returns cells; `get_point_path` returns positions scaled by
  `cell_size` plus `offset`. Use cells for game logic.
- Movement range: the cells whose path length minus 1 is at most the actor's
  move points. For a large range, a breadth-first search from the actor
  is cheaper than one path query per cell.
- Keep the board state (which unit is in which cell) in a
  `Dictionary[Vector2i, TurnActor]`, not in the scene tree. The nodes draw
  the board; the dictionary is the truth.
- Terrain cost: `grid.set_point_weight_scale(cell, 2.0)`.
- A change of `region` or `cell_size` needs `update()`, and that rebuild
  clears solid points and weights. Set them again after it.

## Undo

A "take back the move" button needs commands that can be reverted. A
`UndoRedo` object works at run time: wrap each move in
`create_action` / `add_do_method` / `add_undo_method` / `commit_action`, and
call `undo()`. Clear the history when the turn ends, so nothing undoes an
earlier turn.

## Online turns

- The server owns the queue and the turn timer. On `turn_started` it starts a
  `Timer`. On timeout it applies a default action (pass or defend) and moves
  on.
- A client sends a command by RPC. The server checks that it is that
  player's turn and that the command is legal, then applies it and sends the
  result to everyone.
- Clients draw the countdown from the server's start time. They never change
  the queue.
- To notify many units at the turn start, use
  `get_tree().call_group_flags(SceneTree.GROUP_CALL_DEFERRED, &"units", &"on_turn_started")`,
  so the calls run at the end of the frame and not inside the current
  handler.

## Checks

- Round queue with speeds 10, 30, 30 (ids 1, 2, 3): order 2, 3, 1.
- An actor that dies mid-round does not act; the round still ends.
- Timeline with speeds 50 and 25: the preview of 6 turns has the fast actor
  4 times.
- `spend` with too few points returns `false` and keeps the points.
- `AStarGrid2D`: a path around a wall avoids the wall cell.
