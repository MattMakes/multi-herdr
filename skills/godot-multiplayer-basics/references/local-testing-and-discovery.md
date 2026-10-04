Adds local multi-instance testing, a latency and loss proxy for ENet, LAN server discovery and UPnP port mapping for listen servers; read it before you call any netcode "done", and when players must find or reach a host without a dedicated server.

# Local testing, latency and discovery

> ← Back to [SKILL.md](../SKILL.md)

## Run several instances

- In the editor: **Debug → Customize Run Instances**, enable multiple
  instances and give each one its own arguments.
- From the shell, pass role arguments after `--`; the game reads them with
  `OS.get_cmdline_user_args()`:

```bash
godot --path . -- --host --port=7000 &
godot --path . -- --join=127.0.0.1 --port=7000 &
godot --path . -- --join=127.0.0.1 --port=7000 &
```

```gdscript
# launch_args.gd - autoload; read the role once at startup
extends Node

var role: StringName = &"menu"
var address: String = "127.0.0.1"
var port: int = 7000


func _ready() -> void:
	for arg: String in OS.get_cmdline_user_args():
		if arg == "--host":
			role = &"host"
		elif arg.begins_with("--join="):
			role = &"join"
			address = arg.get_slice("=", 1)
		elif arg.begins_with("--port="):
			port = arg.get_slice("=", 1).to_int()
```

A fleet worker runs the instances `--headless` and checks the logs; it
cannot judge how the game feels. Say so in `DONE:`.

## Never trust 0 ms

Everything works on localhost. Test every netcode change at about 150 ms
round trip with 1 to 3 % loss before you tune it.

ENet runs over UDP, so a small UDP relay between client and server adds
delay, jitter and loss without changing the game and without root rights.
Clients connect to the relay's port instead of the server's.

```gdscript
# tools/lag_proxy.gd - a UDP relay that adds delay, jitter and loss.
# Run: godot --headless --path . -s res://tools/lag_proxy.gd -- 7001 127.0.0.1 7000 75 20 0.02
# Clients connect to port 7001; the proxy forwards to the server at 127.0.0.1:7000.
# Arguments: listen port, server host, server port, one-way delay ms, jitter ms, loss 0..1.
extends SceneTree

var listen_port: int = 7001
var server_host: String = "127.0.0.1"
var server_port: int = 7000
var delay_ms: int = 75
var jitter_ms: int = 20
var loss: float = 0.02

var _server := UDPServer.new()
var _links: Array[Dictionary] = []  # {client: PacketPeerUDP, upstream: PacketPeerUDP}
var _queue: Array[Dictionary] = []  # {due: int, peer: PacketPeerUDP, data: PackedByteArray}
var _rng := RandomNumberGenerator.new()


func _init() -> void:
	var args: PackedStringArray = OS.get_cmdline_user_args()
	if args.size() >= 6:
		listen_port = args[0].to_int()
		server_host = args[1]
		server_port = args[2].to_int()
		delay_ms = args[3].to_int()
		jitter_ms = args[4].to_int()
		loss = args[5].to_float()
	if _server.listen(listen_port) != OK:
		push_error("lag_proxy: cannot listen on %d" % listen_port)
		quit(1)
		return
	print("lag_proxy: :%d -> %s:%d delay=%dms jitter=%dms loss=%.2f" % [listen_port, server_host, server_port, delay_ms, jitter_ms, loss])


func _process(_delta: float) -> bool:
	_server.poll()
	while _server.is_connection_available():
		var client: PacketPeerUDP = _server.take_connection()
		var upstream := PacketPeerUDP.new()
		upstream.connect_to_host(server_host, server_port)
		_links.append({"client": client, "upstream": upstream})
	for link: Dictionary in _links:
		_pump(link.client, link.upstream)
		_pump(link.upstream, link.client)
	var now: int = Time.get_ticks_msec()
	while not _queue.is_empty() and _queue[0].due <= now:
		var item: Dictionary = _queue.pop_front()
		(item.peer as PacketPeerUDP).put_packet(item.data)
	return false  # Keep running until the process is killed.


func _pump(from: PacketPeerUDP, to: PacketPeerUDP) -> void:
	while from.get_available_packet_count() > 0:
		var data: PackedByteArray = from.get_packet()
		if _rng.randf() < loss:
			continue
		var due: int = Time.get_ticks_msec() + delay_ms + _rng.randi_range(-jitter_ms, jitter_ms)
		_queue.append({"due": due, "peer": to, "data": data})
	_queue.sort_custom(func(a: Dictionary, b: Dictionary) -> bool: return a.due < b.due)
```

```bash
# Server on 7000, relay on 7001 with 75 ms one way (about 150 ms round trip), 20 ms jitter, 2 % loss.
godot --headless --path . -- --host --port=7000 &
godot --headless --path . -s res://tools/lag_proxy.gd -- 7001 127.0.0.1 7000 75 20 0.02 &
godot --path . -- --join=127.0.0.1 --port=7001
```

Measured on Godot 4.7.2: an ENet client through the relay at 75 ms one
way and 10 ms jitter reported a round trip time of 172 ms
(`ENetPacketPeer.PEER_ROUND_TRIP_TIME`).

Pass and fail gates at 150 ms and 2 % loss:

| Gate | Pass |
|---|---|
| Local feel | Own movement responds at once (prediction) |
| Remote players | Move smoothly; no visible snapping (interpolation) |
| Authority | A forged RPC (wrong range, wrong owner) is rejected on the server |
| Late join | A client joining mid-match sees the current world |
| Disconnect | Killing a client frees its player on the server and other clients |

The relay is a test tool. Keep it under `tools/` and out of the export
with the preset's exclude filter.

## Find servers on the LAN

A host broadcasts a small UDP packet every second; clients listen on the
same port and list who answered. This works only inside one network
segment. Internet discovery needs a master server or a platform lobby.

```gdscript
# lan_discovery.gd
extends Node

signal server_found(address: String, info: Dictionary)

const DISCOVERY_PORT: int = 7999
const MAGIC: String = "mygame-v1"

var _socket := PacketPeerUDP.new()
var _announce_info: Dictionary = {}
var _timer: float = 0.0


func start_announcing(server_name: String, game_port: int) -> void:
	_announce_info = {"magic": MAGIC, "name": server_name, "port": game_port}
	_socket.close()
	_socket.set_broadcast_enabled(true)
	_socket.set_dest_address("255.255.255.255", DISCOVERY_PORT)


func start_listening() -> Error:
	_announce_info = {}
	_socket.close()
	return _socket.bind(DISCOVERY_PORT) as Error


func stop() -> void:
	_socket.close()
	_announce_info = {}


func _process(delta: float) -> void:
	if not _announce_info.is_empty():
		_timer += delta
		if _timer >= 1.0:
			_timer = 0.0
			_socket.put_packet(JSON.stringify(_announce_info).to_utf8_buffer())
		return
	while _socket.get_available_packet_count() > 0:
		var data: PackedByteArray = _socket.get_packet()
		var address: String = _socket.get_packet_ip()
		var info: Variant = JSON.parse_string(data.get_string_from_utf8())
		if info is Dictionary and info.get("magic", "") == MAGIC:
			server_found.emit(address, info)
```

JSON numbers arrive as floats: connect with `int(info["port"])`. Tested
on Godot 4.7.2 between two processes on one machine. Treat the packet as untrusted: it is a hint where to connect, nothing
more. The version string in `MAGIC` keeps old builds from listing
incompatible servers.

## Listen servers behind a home router: UPnP

When a player hosts from home, other players on the internet cannot reach
the port unless the router forwards it. Many routers accept a UPnP
request. `UPNP.discover()` blocks for up to its timeout (2000 ms by
default), so run it on a thread.

```gdscript
# upnp_mapper.gd
extends Node

signal finished(ok: bool, external_ip: String)

var _thread := Thread.new()
var _port: int = 0


func open_port(port: int) -> void:
	_port = port
	_thread.start(_map_port)


func _map_port() -> void:
	var upnp := UPNP.new()
	var ok: bool = false
	var ip: String = ""
	if upnp.discover() == UPNP.UPNP_RESULT_SUCCESS and upnp.get_gateway() != null and upnp.get_gateway().is_valid_gateway():
		ok = upnp.add_port_mapping(_port, _port, "mygame", "UDP") == UPNP.UPNP_RESULT_SUCCESS
		ip = upnp.query_external_address()
	finished.emit.call_deferred(ok, ip)


func close_port() -> void:
	# Call before quitting; a stale mapping stays on the router otherwise.
	var upnp := UPNP.new()
	if upnp.discover() == UPNP.UPNP_RESULT_SUCCESS and upnp.get_gateway() != null:
		upnp.delete_port_mapping(_port, "UDP")


func _exit_tree() -> void:
	if _thread.is_started():
		_thread.wait_to_finish()
```

UPnP is often disabled, and it does nothing behind carrier-grade NAT.
Always show the host a manual fallback (forward port N), or use a relay
or a platform networking layer (Steam, Epic) that handles NAT for you.
A fleet worker cannot test UPnP: it needs a real router. Mark it untested.
