Adds rollback netcode (predict every remote input, rewind and re-simulate when the real input differs): when it fits, what the simulation must guarantee, a tested rollback core and desync checks; read it for fighting games and other small-lobby games where every frame of input lag matters.

# Rollback netcode

> ← Back to [SKILL.md](../SKILL.md)

## How it differs from client prediction

The client prediction reference corrects **one** object, the local player,
from server snapshots. Rollback runs the **whole game** on every peer from
inputs only:

1. Each peer simulates every tick at once, using the real local input and
   a **predicted** input for each remote player (usually "same as last
   tick").
2. When a remote input arrives and differs from the prediction, the peer
   loads the state saved before that tick and re-simulates up to the
   present with the corrected inputs.
3. A small **input delay** (2 to 3 ticks) applies the local input slightly
   later. It hides most of the network delay, so fewer rollbacks happen.

| Fits | Does not fit |
|---|---|
| 2 to 4 players, peer-to-peer or relayed | Dozens of players or a large world |
| Fighting, sports, small-arena action | Games that cannot re-run 10 ticks in one frame |
| A simulation you control, in fixed ticks | Gameplay that depends on the physics engine |

## What the simulation must guarantee

- **Determinism.** The same state plus the same inputs gives the same
  next state, on every peer and every platform. Use integers or
  fixed-point numbers for positions and velocities; float results can
  differ between CPUs and compilers. Seed every random generator from the
  session and keep its state inside the saved state.
- **No physics engine in the loop.** Godot Physics and Jolt are not
  deterministic across machines and cannot rewind a whole space cheaply.
  Write the movement and hit tests in your own code (axis-aligned boxes,
  circles), or use an addon built for rollback.
- **Fixed ticks.** Simulate in `_physics_process` with a fixed
  `Engine.physics_ticks_per_second`; never read `delta` inside the step.
- **Cheap save and load.** The whole game state fits in a `Dictionary` or
  a small packed array that you can copy every tick.
- **Presentation outside the state.** Particles, sounds and camera shake
  are not rolled back. Trigger them from confirmed ticks, or make them
  tolerate a replay (do not play the hit sound twice).

## A rollback core

The class below holds the history and does the rewind. It knows nothing
about the network or the game: the game passes three callables, and the
network layer passes remote inputs. Inputs here are `int` bit masks
(bit 0 left, bit 1 right, ...), which are cheap to send and compare.

```gdscript
# rollback_session.gd - deterministic rollback core, no networking inside.
# The game supplies three callables; the network layer feeds remote inputs.
class_name RollbackSession
extends RefCounted

## save_state() -> Dictionary, load_state(state: Dictionary), step(inputs: Dictionary)
var save_state: Callable
var load_state: Callable
var step: Callable

var local_player: int
var players: PackedInt32Array
var input_delay: int = 2      ## Local input applies this many ticks later.
var max_rollback: int = 12    ## Ticks of history kept.

var tick: int = 0             ## Next tick to simulate.
var _states: Dictionary = {}  ## tick -> state saved BEFORE that tick ran.
var _inputs: Dictionary = {}  ## tick -> {player: input}; confirmed inputs only.
var _used: Dictionary = {}    ## tick -> {player: input} actually used (some predicted).
var _rollback_from: int = -1


func _init(p_players: PackedInt32Array, p_local: int, p_save: Callable, p_load: Callable, p_step: Callable) -> void:
	players = p_players
	local_player = p_local
	save_state = p_save
	load_state = p_load
	step = p_step


## Record the local input now; it is used at tick + input_delay. Returns that tick, to send.
func add_local_input(input: int) -> int:
	var at: int = tick + input_delay
	_confirm(at, local_player, input)
	return at


## Called when a remote player's input for `at` arrives.
func add_remote_input(at: int, player: int, input: int) -> void:
	_confirm(at, player, input)


func _confirm(at: int, player: int, input: int) -> void:
	if not _inputs.has(at):
		_inputs[at] = {}
	_inputs[at][player] = input
	# If that tick already ran with a different (predicted) input, roll back to it.
	if at < tick and _used.has(at) and _used[at].get(player) != input:
		_rollback_from = at if _rollback_from < 0 else mini(_rollback_from, at)


## Advance one tick. Call from _physics_process.
func advance() -> void:
	if _rollback_from >= 0:
		if _rollback_from < tick - max_rollback or not _states.has(_rollback_from):
			push_error("rollback: input for tick %d is too old; desync" % _rollback_from)
		else:
			load_state.call(_states[_rollback_from])
			var target: int = tick
			tick = _rollback_from
			while tick < target:
				_run_tick()
		_rollback_from = -1
	_run_tick()
	_states.erase(tick - max_rollback - 1)
	_inputs.erase(tick - max_rollback - 1)
	_used.erase(tick - max_rollback - 1)


func _run_tick() -> void:
	_states[tick] = save_state.call()
	var used: Dictionary = {}
	for p: int in players:
		used[p] = _input_for(tick, p)
	_used[tick] = used
	step.call(used)
	tick += 1


func _input_for(at: int, player: int) -> int:
	var t: int = at
	while t >= 0 and t > at - max_rollback:
		if _inputs.has(t) and _inputs[t].has(player):
			return _inputs[t][player]  # Exact, or the last known input as the prediction.
		t -= 1
	return 0
```

Use it from the game node:

```gdscript
# match.gd - the gameplay root on every peer
extends Node

const PLAYERS: PackedInt32Array = [1, 2]

## The whole simulation state: integer positions in 1/100 pixel.
var pos: Dictionary = {1: 0, 2: 100000}
var session: RefCounted = null


func _ready() -> void:
	var local_id: int = multiplayer.get_unique_id()
	var core: GDScript = load("res://net/rollback_session.gd")
	session = core.new(PLAYERS, local_id, _save, _load, _step)


func _physics_process(_delta: float) -> void:
	var mask: int = int(Input.is_action_pressed(&"move_left")) | (int(Input.is_action_pressed(&"move_right")) << 1)
	var at: int = session.add_local_input(mask)
	_send_input.rpc(at, mask)  # Unreliable is fine if you resend the last few inputs.
	session.advance()


@rpc("any_peer", "call_remote", "unreliable_ordered")
func _send_input(at: int, mask: int) -> void:
	session.add_remote_input(at, multiplayer.get_remote_sender_id(), mask)


func _save() -> Dictionary:
	return pos.duplicate()


func _load(state: Dictionary) -> void:
	pos = state.duplicate()


func _step(inputs: Dictionary) -> void:
	for p: int in inputs:
		var mask: int = inputs[p]
		pos[p] += ((mask >> 1) & 1) * 300 - (mask & 1) * 300
```

Tested on Godot 4.7.2: two sessions in one process, each receiving the
other's input 4 ticks late with an input delay of 2, ran 200 ticks of
random input. Both ended in the same state, equal to a run with every
input known in advance.

Production details the sketch leaves out:
- **Resend.** Send the last N local inputs in every packet, so one lost
  packet does not stall the other peer.
- **Too old.** If an input arrives for a tick older than `max_rollback`,
  the peers have drifted too far: pause and resynchronize from a full
  state, do not continue.
- **Time sync.** If one peer runs ahead, it rolls back constantly. Compare
  the tick numbers in received packets and slow the leading peer by a
  fraction of a tick until they match.
- **Visual smoothing.** After a rollback, a character may jump. Lerp the
  drawn position toward the simulated one over a few frames, and call
  `reset_physics_interpolation()` on nodes you teleport.

## Detect desyncs early

A desync is silent: both peers keep running different games. Every 30
ticks or so, hash a confirmed state (a tick older than `max_rollback`, so
it can no longer change) and send the hash. Unequal hashes mean a
determinism bug.

```gdscript
# state_checksum.gd
extends RefCounted


static func of(state: Dictionary) -> int:
	# var_to_bytes gives the same bytes for the same Dictionary built in the
	# same key order; build state dictionaries in a fixed order.
	return hash(var_to_bytes(state))
```

Log both states when the hashes differ, then diff them. The usual causes
are a float in the state, an unseeded random number, iteration over a
`Dictionary` built in a different order, or game code that reads `delta`
or the wall clock inside the step.

## Addons

The community addon **netfox** (`foxssake/netfox`) provides rollback,
time synchronization and input handling for Godot 4. If the project uses
it, follow its documentation for the installed version; do not mix it with
a hand-written core like the one above.
