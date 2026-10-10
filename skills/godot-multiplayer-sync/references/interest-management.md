Adds interest management for large worlds: a spatial grid on the server that decides which peers see which nodes, and how synchronizer visibility also controls spawning; read it when bandwidth or server CPU grows with the number of players times the number of objects.

# Interest management

> ← Back to [SKILL.md](../SKILL.md)

SKILL.md section 1 shows a per-object distance filter. That works for a
few dozen objects. With hundreds of objects and many players, the server
calls every filter for every peer, and objects far away still exist on
every client. Interest management fixes both: the server keeps a spatial
index and tells each synchronizer exactly which peers may see it.

## Visibility also controls spawning

When a node is spawned by a `MultiplayerSpawner`, its
`MultiplayerSynchronizer` visibility decides whether the node exists on a
peer, not only whether updates flow:

- A peer that cannot see the synchronizer does not get the node spawned.
- `set_visibility_for(peer, true)` later spawns it on that peer, with its
  current spawn properties.
- Hiding it again despawns it on that peer.

Tested on Godot 4.7.2 with a server and a client process: a node whose
synchronizer had `public_visibility = false` was not spawned on the client;
after `set_visibility_for(client_id, true)` it appeared, and after
`set_visibility_for(client_id, false)` it was removed again. A node with a
visibility filter that returned `false` was not spawned either.

So a far-away enemy costs a client nothing: no node, no updates. Keep
everything a node needs to look right in its spawn properties, because a
node can appear on a client at any time.

Visibility filters and `set_visibility_for` combine: a peer sees the node
only if every filter allows it **and** it is public or explicitly
visible. Pick one mechanism per object type; the grid below uses
`public_visibility = false` plus `set_visibility_for`.

## A grid on the server

Each object and each player is in one cell. A player sees the objects in
its own cell and the neighbouring cells. When an object or player changes
cell, only the visibility pairs that changed are updated.

```gdscript
# interest_grid.gd - server-only autoload named InterestGrid
extends Node

@export var cell_size: float = 64.0
@export var view_radius_cells: int = 2  # 5x5 cells around the player.

var _cell_of_object: Dictionary = {}   # MultiplayerSynchronizer -> Vector2i
var _objects_in_cell: Dictionary = {}  # Vector2i -> Array[MultiplayerSynchronizer]
var _cell_of_peer: Dictionary = {}     # int -> Vector2i


func cell_for(pos: Vector2) -> Vector2i:
	return Vector2i(floori(pos.x / cell_size), floori(pos.y / cell_size))


## Register a networked object. Its synchronizer starts hidden from everyone.
func add_object(sync: MultiplayerSynchronizer, pos: Vector2) -> void:
	sync.public_visibility = false
	_cell_of_object[sync] = Vector2i(2147483647, 0)  # Not in any cell yet.
	move_object(sync, pos)


func remove_object(sync: MultiplayerSynchronizer) -> void:
	var cell: Vector2i = _cell_of_object.get(sync, Vector2i.ZERO)
	if _objects_in_cell.has(cell):
		_objects_in_cell[cell].erase(sync)
	_cell_of_object.erase(sync)


## Call when an object moves (from its _physics_process on the server).
func move_object(sync: MultiplayerSynchronizer, pos: Vector2) -> void:
	var new_cell: Vector2i = cell_for(pos)
	var old_cell: Vector2i = _cell_of_object.get(sync, new_cell)
	if old_cell == new_cell and _objects_in_cell.has(new_cell) and sync in _objects_in_cell[new_cell]:
		return
	if _objects_in_cell.has(old_cell):
		_objects_in_cell[old_cell].erase(sync)
	if not _objects_in_cell.has(new_cell):
		_objects_in_cell[new_cell] = []
	_objects_in_cell[new_cell].append(sync)
	_cell_of_object[sync] = new_cell
	for peer: int in _cell_of_peer:
		sync.set_visibility_for(peer, _near(_cell_of_peer[peer], new_cell))


## Call when a player's avatar moves.
func move_peer(peer: int, pos: Vector2) -> void:
	var new_cell: Vector2i = cell_for(pos)
	if _cell_of_peer.get(peer) == new_cell:
		return
	var old_cell: Variant = _cell_of_peer.get(peer)
	_cell_of_peer[peer] = new_cell
	var r: int = view_radius_cells
	if old_cell != null:
		# Hide what left the view.
		for cell: Vector2i in _cells_around(old_cell, r):
			if not _near(new_cell, cell):
				for sync: MultiplayerSynchronizer in _objects_in_cell.get(cell, []):
					sync.set_visibility_for(peer, false)
	# Show what entered the view.
	for cell: Vector2i in _cells_around(new_cell, r):
		for sync: MultiplayerSynchronizer in _objects_in_cell.get(cell, []):
			sync.set_visibility_for(peer, true)


func remove_peer(peer: int) -> void:
	_cell_of_peer.erase(peer)


func _near(a: Vector2i, b: Vector2i) -> bool:
	return absi(a.x - b.x) <= view_radius_cells and absi(a.y - b.y) <= view_radius_cells


func _cells_around(center: Vector2i, r: int) -> Array[Vector2i]:
	var out: Array[Vector2i] = []
	for y: int in range(center.y - r, center.y + r + 1):
		for x: int in range(center.x - r, center.x + r + 1):
			out.append(Vector2i(x, y))
	return out
```

For a 3D world, use `Vector3.x` and `Vector3.z` for the cell.

Rules:
- Run the grid only on the server (`multiplayer.is_server()`). Clients
  never decide what they may see.
- A player always sees its own avatar: call
  `set_visibility_for(owner_peer, true)` on the avatar's synchronizer and
  never hide it from its owner.
- Use **hysteresis**: an object on a cell border can flicker in and out.
  Hide only when it is one cell beyond the show radius, or delay hiding
  by a second.
- Things every player needs (match state, scoreboard, global events) stay
  outside the grid: public synchronizers or reliable RPCs.
- RPCs are not filtered by visibility. Send object events with
  `rpc_id` to the peers that can see the object, or the client receives
  an RPC for a node it does not have and logs an error.

## Combine with update rates

Interest decides **who** gets an object. The bandwidth reference decides
**how often**: within the visible set, near objects can sync at 20 to 30
Hz and the outer ring at 5 Hz, by giving them separate synchronizers or
by changing `replication_interval` when they change ring.

## Measure

Log bytes per second per peer before and after, from the server:
`ENetConnection.pop_statistic(ENetConnection.HOST_TOTAL_SENT_DATA)` on
`(multiplayer.multiplayer_peer as ENetMultiplayerPeer).host` gives the
bytes sent since the last call. A `DONE:` for this work names the player
count, object count and the measured numbers.
