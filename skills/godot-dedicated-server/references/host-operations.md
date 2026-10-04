Adds the operating side of a server process: controlled packet polling, server and client in one process, WebSocket hosting for browser clients, a health line for log scrapers and a matchmaker hand-off; read it when the server runs unattended, serves web players, or must be tested without a second process.

# Host operations

> ← Back to [SKILL.md](../SKILL.md)

## Poll packets at a fixed point in the tick

By default the `SceneTree` polls the multiplayer peer once per frame, and
RPCs run at that moment. For a server that simulates in fixed ticks, poll
yourself at the start of each physics tick, so every tick sees the inputs
that arrived before it and none arrive in the middle of it.

```gdscript
# server_tick.gd - autoload on the dedicated server
extends Node

var tick: int = 0


func _ready() -> void:
	get_tree().multiplayer_poll = false  # From now on, nothing polls unless we do.


func _physics_process(_delta: float) -> void:
	if multiplayer.has_multiplayer_peer():
		multiplayer.poll()  # RPCs and synchronizer updates run here.
	tick += 1
	# ... simulate the tick ...
```

When `multiplayer_poll` is `false` and nothing calls `poll()`, the server
stops receiving packets and peers time out. Every custom `MultiplayerAPI`
set with `SceneTree.set_multiplayer()` needs its own `poll()` call too.

## Server and client in one process

`SceneTree.set_multiplayer(api, root_path)` gives a branch of the tree its
own `MultiplayerAPI`. A listen-server host can run the authoritative
server in one branch and its own client in another, with the same network
code as a remote client. Integration tests use the same trick (see
**godot-testing**, `references/budgets-leaks-and-network-tests.md`).

```gdscript
# split_host.gd - builds /root/Server and /root/Client branches
extends Node

const PORT: int = 7000

var server_api: MultiplayerAPI = MultiplayerAPI.create_default_interface()
var client_api: MultiplayerAPI = MultiplayerAPI.create_default_interface()


func start(server_scene: PackedScene, client_scene: PackedScene) -> Error:
	var server_root: Node = server_scene.instantiate()
	server_root.name = "Server"
	var client_root: Node = client_scene.instantiate()
	client_root.name = "Client"
	get_tree().root.add_child(server_root)
	get_tree().root.add_child(client_root)
	get_tree().set_multiplayer(server_api, ^"/root/Server")
	get_tree().set_multiplayer(client_api, ^"/root/Client")

	var server_peer := ENetMultiplayerPeer.new()
	var err: Error = server_peer.create_server(PORT, 8) as Error
	if err != OK:
		return err
	server_api.multiplayer_peer = server_peer

	var client_peer := ENetMultiplayerPeer.new()
	err = client_peer.create_client("127.0.0.1", PORT) as Error
	if err != OK:
		return err
	client_api.multiplayer_peer = client_peer
	return OK
```

RPC paths are relative to each branch root, so the server branch and the
client branch must have the same node layout below their roots. Tested on
Godot 4.7.2: a client branch sent an RPC to the server branch over ENet on
localhost in one headless process; the server branch saw the client's
peer id as the sender.

## Browser clients need WebSocket

Browsers cannot open UDP sockets, so a web build cannot use ENet. Run a
`WebSocketMultiplayerPeer` server for web players. Behind HTTPS, browsers
also require `wss://`, so pass TLS options (or terminate TLS at a reverse
proxy and run plain `ws://` behind it).

```gdscript
# ws_host.gd - dedicated server that accepts browser clients
extends Node

@export var port: int = 9080
@export var cert_path: String = ""  # Empty: plain ws:// (behind a TLS proxy).
@export var key_path: String = ""


func _ready() -> void:
	var peer := WebSocketMultiplayerPeer.new()
	var tls: TLSOptions = null
	if not cert_path.is_empty():
		var cert := X509Certificate.new()
		var key := CryptoKey.new()
		if cert.load(cert_path) != OK or key.load(key_path) != OK:
			push_error("ws_host: cannot load TLS files")
			get_tree().quit(1)
			return
		tls = TLSOptions.server(key, cert)
	var err: Error = peer.create_server(port, "*", tls) as Error
	if err != OK:
		push_error("ws_host: cannot listen on %d (%s)" % [port, error_string(err)])
		get_tree().quit(1)
		return
	multiplayer.multiplayer_peer = peer
```

The client connects with `create_client("wss://game.example.com:9080")`.
WebSocket runs over TCP: a lost packet delays every later one. Expect worse
behaviour under loss than ENet, and sync less often for web players.

## One health line per interval

Log scrapers (Loki, Elastic, CloudWatch) read standard output. Print one
JSON line with a fixed prefix every few seconds.

```gdscript
# health_log.gd - autoload on the dedicated server
extends Node

@export var interval_sec: float = 10.0

var _elapsed: float = 0.0


func _process(delta: float) -> void:
	_elapsed += delta
	if _elapsed < interval_sec:
		return
	_elapsed = 0.0
	var line: Dictionary = {
		"t": int(Time.get_unix_time_from_system()),
		"fps": Performance.get_monitor(Performance.TIME_FPS),
		"physics_ms": Performance.get_monitor(Performance.TIME_PHYSICS_PROCESS) * 1000.0,
		"static_mb": Performance.get_monitor(Performance.MEMORY_STATIC) / 1048576.0,
		"objects": Performance.get_monitor(Performance.OBJECT_COUNT),
		"nodes": Performance.get_monitor(Performance.OBJECT_NODE_COUNT),
		"orphans": Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT),
		"peers": multiplayer.get_peers().size() if multiplayer.has_multiplayer_peer() else 0,
	}
	var enet := multiplayer.multiplayer_peer as ENetMultiplayerPeer
	if enet != null and enet.host != null:
		line["sent_bytes"] = enet.host.pop_statistic(ENetConnection.HOST_TOTAL_SENT_DATA)
		line["recv_bytes"] = enet.host.pop_statistic(ENetConnection.HOST_TOTAL_RECEIVED_DATA)
	print("HEALTH " + JSON.stringify(line))
```

`pop_statistic` returns the bytes since the previous call and resets the
counter, so each line shows one interval. A rising `objects` or `orphans`
count over hours is a leak: free nodes when matches end.

## Matchmaker hand-off

A client asks a matchmaking service for a server, gets an address and a
one-time token, connects, and presents the token in the handshake
(`references/host-security.md`). The service is yours; the client side is
an `HTTPRequest`.

```gdscript
# matchmaker_client.gd
extends Node

signal match_found(address: String, port: int, token: String)
signal match_failed(reason: String)

@export var service_url: String = "https://matchmaker.example.com/v1/match"

var _http := HTTPRequest.new()


func _ready() -> void:
	add_child(_http)
	_http.request_completed.connect(_on_completed)


func find_match(region: String, session_token: String) -> void:
	var headers: PackedStringArray = ["Content-Type: application/json", "Authorization: Bearer " + session_token]
	var err: Error = _http.request(service_url, headers, HTTPClient.METHOD_POST, JSON.stringify({"region": region})) as Error
	if err != OK:
		match_failed.emit("request error %s" % error_string(err))


func _on_completed(result: int, code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	if result != HTTPRequest.RESULT_SUCCESS or code != 200:
		match_failed.emit("http %d / result %d" % [code, result])
		return
	var data: Variant = JSON.parse_string(body.get_string_from_utf8())
	if not (data is Dictionary) or not data.has("address") or not data.has("port") or not data.has("token"):
		match_failed.emit("malformed response")
		return
	match_found.emit(str(data.address), int(data.port), str(data.token))
```

## Many simulated objects

If profiling shows that nodes cost too much (hundreds of projectiles or
units on the server), move them to direct `PhysicsServer2D` /
`PhysicsServer3D` bodies or plain data arrays. Free every RID you create
(`PhysicsServer3D.free_rid`); a long-running server leaks otherwise. See
**godot-optimization** for the server APIs. Profile first: most servers
with fewer than a few hundred active bodies do not need this.
