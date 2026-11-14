# Networking, advanced

Target engine: Godot 4.7. This shelf covers the multiplayer stack below and above the RPC
annotation: transports, replication nodes, traffic shaping, security and what netcode the
engine leaves to you.

## Use this shelf when

- You choose a transport (ENet, WebSocket, WebRTC) or a topology (server, host, mesh, relay).
- Replication costs too much bandwidth or pops, and you must shape channels and intervals.
- You design server authority, RPC validation or a login step for a public game.
- You need prediction, interpolation, lag compensation or rollback and must know what to build.
- You talk to a non-Godot backend or ship a headless server.

## How the engine works here

- Three layers. `MultiplayerPeer` (a `PacketPeer`) moves packets and knows peer ids, transfer
  mode and channel. `MultiplayerAPI` gives RPC on top; its default implementation
  `SceneMultiplayer` adds spawn and sync replication (the `MultiplayerSpawner` and
  `MultiplayerSynchronizer` nodes), auth and relay.
- The wire protocol of `SceneMultiplayer` is an implementation detail and may change. It is
  not meant for non-Godot servers.
- The default peer is `OfflineMultiplayerPeer`: a server with no clients, so
  `multiplayer.is_server()` is true in single player. Assign it again to stop networking.
- The server id is 1. Client ids are random positive integers. Authority is a per-node peer id
  (default 1). `set_multiplayer_authority()` is local only: it is not replicated and does not
  reach children added later. Set it on every peer, for example in a spawn function.
- RPCs address nodes by path, so paths must match on both sides. All `@rpc` methods of a
  script are checksummed together (annotation, name, return type, node path). One mismatch
  breaks the script's RPCs, and the error may name another method. Argument lists are not in
  the checksum. RPCs do not carry Objects or Callables.
- `SceneTree.multiplayer_poll` (default true) polls once per process frame. Received RPCs run
  inside that poll, so with defaults they run at render rate, not at physics rate.
- Transfer modes: `unreliable` (may drop or reorder), `unreliable_ordered` (late packets are
  dropped), `reliable` (resent and ordered; one lost packet stalls everything behind it on the
  same channel). Channel 0 is really three channels, one per mode.
- Synchronizer modes per property (`SceneReplicationConfig`): spawn (sent once with the
  spawn), `REPLICATION_MODE_ALWAYS` (unreliable, every `replication_interval`) and
  `REPLICATION_MODE_ON_CHANGE` (reliable delta, checked every `delta_interval`). Interval 0
  means every network frame, not "off".

## Advanced techniques

### Pick the transport by platform and loss tolerance
| Peer | Transport | Fits | Limits |
|---|---|---|---|
| `ENetMultiplayerPeer` | UDP, channels, all three modes | native action games | no Web; UDP port must be open |
| `WebSocketMultiplayerPeer` | TCP | Web clients, turn-based, lobbies | all traffic behaves as reliable ordered |
| `WebRTCMultiplayerPeer` | UDP data channels, NAT traversal | Web real time, peer to peer | needs signaling; native needs an extension |

### Traffic shaping with channels
Give each traffic class (state, events, chat) its own channel so a lost reliable packet
stalls only its class. Pass the channel as the last `@rpc` argument and create the host with
enough channels (`max_channels` in `create_server()`, `channel_count` in `create_client()`). Keep packet sizes on an
unreliable-ordered channel similar: a small packet that overtakes a large one makes the
large one arrive "late" and be dropped (docs).

### Run the network on the physics tick
Set `get_tree().multiplayer_poll = false` and call `multiplayer.poll()` at the top of
`_physics_process()` in one autoload. RPCs and sync then land at a fixed step, before the
simulation step that uses them. This is the base for any tick-numbered netcode.

### Replication config that scales
- Put slow or rare state on `REPLICATION_MODE_ON_CHANGE` and only fast state (position) on
  `REPLICATION_MODE_ALWAYS` with a `replication_interval` above 0, then interpolate on the
  receiver.
- A sync packet is capped by `SceneMultiplayer.max_sync_packet_size` (1350 bytes). State that
  does not fit arrives over several frames, so keep per-node state small.
- Synced values cannot be Objects, Resources, RIDs or instance ids.
- Use a second synchronizer under the player with the owning client as authority to carry
  input to the server. The server keeps authority over the body.

### Interest management with visibility
`add_visibility_filter()` takes a callable `(peer_id) -> bool`; `set_visibility_for()` sets it
per peer; `public_visibility = false` makes the node hidden by default. If the synchronizer's
root was spawned by a `MultiplayerSpawner`, visibility also spawns and despawns the node on
that peer: per-peer relevance (fog of war, area of interest) with no custom protocol. Filters run every frame by default; set `visibility_update_mode` to
`VISIBILITY_PROCESS_NONE` and call `update_visibility()` from a slower grid pass when there
are many nodes times many peers.

### Custom spawns
`MultiplayerSpawner.spawn(data)` calls `spawn_function` on every peer with a small Variant
and adds the returned node itself (do not `add_child()` it). Use it to send "what and for
whom" (scene id, owner id, seed) so each peer builds the node and sets authority the same way.

### Authenticate before peers exist
With `SceneMultiplayer.auth_callback` set, a new connection stays in an authenticating state:
no `peer_connected`, no RPCs. Exchange tokens with `send_auth()`; each side calls
`complete_auth()`. `auth_timeout` (3 s) drops silent peers and `peer_authentication_failed`
reports them. Encrypt the
transport first: `ENetConnection.dtls_server_setup()` and `dtls_client_setup()` with
`TLSOptions`, or `wss://` for WebSocket.

### Validate every client RPC
```gdscript
extends Node

const MAX_SPEED: float = 8.0

@rpc("any_peer", "call_remote", "unreliable_ordered", 1)
func submit_move(tick: int, direction: Vector2) -> void:
	if not multiplayer.is_server():
		return
	var sender: int = multiplayer.get_remote_sender_id()
	if tick < 0 or not direction.is_finite():
		return
	var clamped: Vector2 = direction.limit_length(1.0) * MAX_SPEED
	_apply_move(sender, tick, clamped)

func _apply_move(_peer: int, _tick: int, _velocity: Vector2) -> void:
	pass
```

Rules: identify the player by `get_remote_sender_id()`, never by an id argument. Read it
before any `await`; it returns 0 outside the call. Send intent, not results. Check type,
range, finiteness, cooldown and rate. Keep `allow_object_decoding` false: a decoded object
can carry a script.

### What you must build: prediction, interpolation, lag compensation, rollback
The engine gives transport, RPC, spawn and sync. It gives none of these four.

- Snapshot interpolation: buffer received states with their tick and render remote entities
  a fixed delay in the past (commonly about two send intervals). Engine physics interpolation
  (`physics/common/physics_interpolation`) smooths between local ticks, not network states.
- Prediction and reconciliation: the client applies its own input at once and keeps inputs
  by tick. On a server state for tick N it resets to it and replays the inputs after N.
  Movement must be a pure function of state and input.
- Lag compensation: the server keeps a short history of hitbox transforms and tests a shot
  against the pose at the shooter's view tick. Clamp the rewind window.
- Rollback: save and load the whole simulated state and re-simulate several ticks in one
  frame. The class reference has no call that steps a physics space by hand, so rollback code
  uses its own collision queries or a script-driven simulation. Addons such as netfox
  implement tick loops, rollback and interpolation; they report rigid bodies as unsuited to
  rollback and rollback as CPU-heavy at high latency (reported by netfox docs; measure first).

### Measure bandwidth and link quality
- Debugger > Network Profiler lists per-node RPC and sync traffic. Its meter ignores ENet
  compression.
- `ENetConnection.pop_statistic()` (`HOST_TOTAL_SENT_DATA`, `HOST_TOTAL_RECEIVED_DATA`) gives
  real bytes; sample it once per second. Reach it through `ENetMultiplayerPeer.host`.
- `ENetPacketPeer.get_statistic()` gives `PEER_ROUND_TRIP_TIME` and `PEER_PACKET_LOSS` per
  client (`ENetMultiplayerPeer.get_peer(id)`): use them for a ping display and send-rate cuts.
- `ENetConnection.compress()`: `COMPRESS_RANGE_CODER` suits small packets; Zstandard is poor
  under 4 KB (class reference). A client with another mode than the server fails to connect.

### Dedicated server builds
Use an export template with the "Export as dedicated server" resource mode. It adds the
`dedicated_server` feature tag, forces headless and lets you mark resources Strip Visuals
(textures and materials become size-only placeholders), Keep or Remove. Detect the
role with `OS.has_feature("dedicated_server")`, `DisplayServer.get_name() == "headless"` or
`OS.get_cmdline_user_args()`. Set `application/run/flush_stdout_on_print` for service logs.

## Niche but useful

- A `MultiplayerAPI` per branch (`SceneTree.set_multiplayer()` with a node path) runs a server
  and a client in one process, for tests and listen servers.
- `SceneMultiplayer.server_relay` (default true) lets clients message each other through the
  server. Turn it off so clients do not learn about each other at all.
- `ENetConnection.socket_send()` sends a raw datagram from the bound port: the building block
  for UDP hole punching. `UPNP.add_port_mapping()` opens a router port; it blocks, so call
  it from a thread.
- `ENetPacketPeer.set_timeout()` and `throttle_configure()` tune disconnect detection and the
  unreliable-traffic throttle.
- `UDPServer`, `PacketPeerUDP`, `TCPServer`, `StreamPeerTCP` (`set_no_delay()`),
  `StreamPeerTLS`, `PacketPeerDTLS` and `WebSocketPeer` serve custom protocols.
- `HTTPRequest`: one request per node at a time. Set `timeout` (default 0 = never); use
  `download_file` and `body_size_limit` for large bodies.
- `network/tls/certificate_bundle_override` replaces the OS certificate bundle. The private
  key stays on the server. `TLSOptions.client_unsafe()` is for tests only.

## Traps

- `peer_connected` fires on a new client once per existing peer. Start gameplay after your own
  "loaded" handshake, not on the signal.
- Outdated belief: "delta interval 0 disables delta sync". 0 means every network frame.
- Web builds have no raw UDP or TCP and cannot host an ENet or WebSocket server.
- Secrets in the client can be extracted. Ship no backend credentials (docs).
- Port forwarding for ENet must be UDP. Android needs the INTERNET permission.

## Version notes

- 4.5: `Node.get_rpc_config()` was renamed `get_node_rpc_config()`.
- 4.6: `StreamPeerSocket` is the new base of `StreamPeerTCP`; `get_status()`, `poll()` and
  `disconnect_from_host()` moved there.
- 4.4 to 4.7: the upgrade notes list no change to `@rpc` or the replication nodes.

## Related skills

- `godot-multiplayer-basics` - ENet setup, RPC syntax, lobby flow.
- `godot-multiplayer-sync` - worked examples of interpolation, prediction and rollback.
- `godot-dedicated-server` - headless boot, tick loop, deployment.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/networking/high_level_multiplayer.html
- https://docs.godotengine.org/en/stable/tutorials/networking/webrtc.html
- https://docs.godotengine.org/en/stable/tutorials/networking/websocket.html
- https://docs.godotengine.org/en/stable/tutorials/networking/http_request_class.html
- https://docs.godotengine.org/en/stable/tutorials/networking/http_client_class.html
- https://docs.godotengine.org/en/stable/tutorials/networking/ssl_certificates.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_dedicated_servers.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/debug/debugger_panel.html
- https://foxssake.github.io/netfox/latest/netfox/tutorials/rollback-caveats/ (search summary)
