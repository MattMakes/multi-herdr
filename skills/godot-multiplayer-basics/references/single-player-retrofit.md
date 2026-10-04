Adds the order of work for turning a single-player game into a multiplayer one: architecture choice, splitting input from simulation, an offline path that keeps working, the late-join snapshot and persistent player identity; read it before adding networking to an existing project.

# Retrofitting multiplayer onto a single-player game

> ← Back to [SKILL.md](../SKILL.md)

## Choose the architecture first

| Factor | Dedicated or host-authoritative server | Peer-to-peer lockstep |
|---|---|---|
| Players | 2 to 100+ | 2 to 4 |
| Cheating matters | Yes (PvP, economy) | No (friends' co-op) |
| A server to host | Available, or one player hosts | Not available |
| Simulation | Only the server simulates shared state | Every peer simulates; must be deterministic |
| Godot fit | `MultiplayerSynchronizer`, `MultiplayerSpawner`, RPCs | RPCs carrying inputs; deterministic custom simulation |

Most retrofits take the first column with one player hosting (a listen
server). Lockstep needs a simulation that gives the same result on every
machine; Godot's physics engines do not promise that.

## Work order

1. **Split input from simulation.** Today the player script reads `Input`
   and moves the body in one function. Split it: one part produces an
   input struct, the other applies an input struct. Only the second part
   runs on the authority.
2. **Make the game run through the network layer with no network.** The
   default `multiplayer.multiplayer_peer` is an `OfflineMultiplayerPeer`:
   `is_server()` is true and the unique id is 1. Code that checks
   authority the right way already works offline. Keep it that way, so
   single-player stays a supported mode, not a broken one.
3. **Assign authority per node.** The server keeps authority over world
   state. A player's input node is owned by that player's peer.
4. **Replicate state** with `MultiplayerSynchronizer` and spawn with
   `MultiplayerSpawner` (SKILL.md sections 4 and 5).
5. **Turn gameplay events into server requests.** "Open chest" becomes an
   `any_peer` RPC to the server, which checks and then applies.
6. **Late join.** A player who joins mid-game gets the current world state.
7. **Test with latency** before you tune anything (see
   [local-testing-and-discovery.md](local-testing-and-discovery.md)).

## Input split: the player owns input, the server owns movement

```gdscript
# player_input.gd - child node "Input" of the player scene.
# Its MultiplayerSynchronizer replicates `move` from the owning client.
extends Node

@export var move: Vector2 = Vector2.ZERO
@export var jump_pressed: bool = false


func _physics_process(_delta: float) -> void:
	if not is_multiplayer_authority():
		return  # Only the owning peer reads its device.
	move = Input.get_vector(&"move_left", &"move_right", &"move_up", &"move_down")
	jump_pressed = Input.is_action_pressed(&"jump")
```

```gdscript
# player_body.gd - the player scene root, owned by the server (peer 1)
extends CharacterBody2D

const SPEED: float = 220.0

## Set by the spawner: the peer that controls this player.
@export var owner_peer: int = 1:
	set(value):
		owner_peer = value
		if is_node_ready():
			$Input.set_multiplayer_authority(value)

@onready var input_node: Node = $Input


func _ready() -> void:
	input_node.set_multiplayer_authority(owner_peer)


func _physics_process(_delta: float) -> void:
	if not multiplayer.is_server():
		return  # Clients receive position from the synchronizer.
	var move: Vector2 = input_node.get("move")
	velocity = move * SPEED
	move_and_slide()
```

The same two scripts run in single-player: the offline peer is the server
and owns both nodes.

For responsive movement on clients, add prediction from
**godot-multiplayer-sync** (client prediction reference) after this works.

## Every client request is checked on the server

```gdscript
# chest.gd
extends Node2D

signal opened(by_peer: int)

const MAX_REACH: float = 64.0

var is_open: bool = false


@rpc("any_peer", "call_local", "reliable")
func request_open() -> void:
	if not multiplayer.is_server():
		return
	var sender: int = multiplayer.get_remote_sender_id()
	if sender == 0:
		sender = multiplayer.get_unique_id()  # A local call on the server.
	var player: Node2D = get_tree().current_scene.get_node_or_null("Players/%d" % sender)
	if is_open or player == null or player.global_position.distance_to(global_position) > MAX_REACH:
		return  # Not allowed: ignore, and log if it repeats.
	_apply_open.rpc(sender)


@rpc("authority", "call_local", "reliable")
func _apply_open(by_peer: int) -> void:
	is_open = true
	opened.emit(by_peer)
```

The client calls `request_open.rpc_id(1)`. The server decides; every peer
then runs `_apply_open`. Never let a client send the result ("I opened the
chest and got 500 gold").

## Late join: send what the synchronizers do not

`MultiplayerSpawner` and synchronizer spawn properties bring a new peer the
nodes and their spawn state. World changes that live outside them (opened
doors, the score, the match timer) need one snapshot RPC.

```gdscript
# world_state.gd - autoload named WorldState
extends Node

var score: Dictionary = {}  # peer id -> points
var opened_doors: PackedStringArray = []
var match_time_left: float = 300.0


func _ready() -> void:
	multiplayer.peer_connected.connect(_on_peer_connected)


func _on_peer_connected(peer_id: int) -> void:
	if multiplayer.is_server():
		_receive_snapshot.rpc_id(peer_id, {
			"score": score,
			"opened_doors": opened_doors,
			"match_time_left": match_time_left,
		})


@rpc("authority", "call_remote", "reliable")
func _receive_snapshot(snapshot: Dictionary) -> void:
	score = snapshot.get("score", {})
	opened_doors = snapshot.get("opened_doors", PackedStringArray())
	match_time_left = snapshot.get("match_time_left", 0.0)
```

## Peer ids are not player identity

A peer id is new on every connection. A player who reconnects gets a new
id. Anything that must survive a reconnect (inventory, team, score) is
keyed by a stable player id from your login or save system, and the server
keeps a map from peer id to player id. Never show peer ids to players.

```gdscript
# identity.gd - server-side map, autoload named Identity
extends Node

var _player_by_peer: Dictionary = {}  # int -> String
var _peer_by_player: Dictionary = {}  # String -> int


func bind(peer_id: int, player_id: String) -> void:
	var old_peer: int = _peer_by_player.get(player_id, 0)
	if old_peer != 0:
		_player_by_peer.erase(old_peer)  # A reconnect replaces the old session.
	_player_by_peer[peer_id] = player_id
	_peer_by_player[player_id] = peer_id


func player_of(peer_id: int) -> String:
	return _player_by_peer.get(peer_id, "")


func unbind_peer(peer_id: int) -> void:
	var player_id: String = _player_by_peer.get(peer_id, "")
	_player_by_peer.erase(peer_id)
	if _peer_by_player.get(player_id, 0) == peer_id:
		_peer_by_player.erase(player_id)
```

How the client proves its player id (a token from your backend, checked
during the connection handshake) is in **godot-dedicated-server**,
`references/host-security.md`.
