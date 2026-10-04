Adds host hardening for an authoritative server: a protocol and token handshake before a peer joins, no object decoding, no client-to-client relay, per-peer RPC rate limits, DTLS encryption and clean kicks; read it before a server is reachable from the internet.

# Host security

> ← Back to [SKILL.md](../SKILL.md)

Assume every packet a client sends is written by an attacker. The server
checks each request (see **godot-multiplayer-basics**,
`references/single-player-retrofit.md`), and the transport rules below
limit what a hostile client can do before your code even runs.

## 1. Handshake before the peer joins

`SceneMultiplayer` has an authentication step. While it runs, the new peer
is "authenticating": it cannot send RPCs, it gets no spawns, and
`peer_connected` has not fired. Use it to check the protocol version and
a login token. A peer that does not finish within `auth_timeout` seconds
is dropped.

```gdscript
# auth_gate.gd - autoload named AuthGate on server and client
extends Node

const PROTOCOL: int = 3  ## Bump on every network-incompatible change.

## Client: set before connecting. Server: checks it.
var token: String = ""

var _sm: SceneMultiplayer = null


func _ready() -> void:
	_sm = multiplayer as SceneMultiplayer
	_sm.auth_callback = _on_auth_data
	_sm.auth_timeout = 3.0
	_sm.peer_authenticating.connect(_on_peer_authenticating)
	_sm.peer_authentication_failed.connect(_on_auth_failed)


func _on_peer_authenticating(peer: int) -> void:
	if not _sm.is_server():
		# The client speaks first: send protocol and token to the server (peer 1).
		var hello: Dictionary = {"protocol": PROTOCOL, "token": token}
		_sm.send_auth(1, JSON.stringify(hello).to_utf8_buffer())
		_sm.complete_auth(1)  # The client accepts the server; the server decides about us.


func _on_auth_data(peer: int, data: PackedByteArray) -> void:
	if not _sm.is_server():
		return
	var hello: Variant = JSON.parse_string(data.get_string_from_utf8())
	if not (hello is Dictionary) or int(hello.get("protocol", -1)) != PROTOCOL or not _token_ok(str(hello.get("token", ""))):
		_sm.disconnect_peer(peer)
		return
	_sm.complete_auth(peer)


func _token_ok(candidate: String) -> bool:
	# Replace with a check against your backend (a signed token, not a password).
	return candidate.length() >= 8


func _on_auth_failed(peer: int) -> void:
	print("auth failed or timed out: ", peer)
```

Tested on Godot 4.7.2 with ENet: a client with a valid token connected;
a client with a short token was dropped, and the server never emitted
`peer_connected` for it.

Rules:
- The token comes from your login backend and is short-lived and signed.
  Never send a password here; ENet is not encrypted unless you enable
  DTLS (section 5).
- A protocol mismatch should reach the player as "update the game", not
  as a timeout. Send a reason before disconnecting if you need that (see
  section 6), or check versions over HTTP before connecting.

## 2. Never decode objects from the network

A `Variant` can carry an `Object`, and decoding one runs its script. Keep
these off on every path that reads client data:

| API | Safe setting |
|---|---|
| `SceneMultiplayer.allow_object_decoding` | `false` (the default). Never turn it on for a public server. |
| `PacketPeer.get_var(allow_objects)` | `get_var()` or `get_var(false)` |
| `bytes_to_var(bytes)` | Use it; never `bytes_to_var_with_objects` on client data |

Even without objects, a client controls every value: check types, sizes
and ranges. A `Dictionary` or `Array` argument can be huge; cap lengths
before you loop over it.

## 3. No relay between clients

By default `SceneMultiplayer.server_relay` is `true`: the server forwards
RPCs that one client addresses to another client. An authoritative server
should not forward anything it did not decide itself.

```gdscript
# server_boot.gd - on the dedicated server only
extends Node


func _ready() -> void:
	var sm := multiplayer as SceneMultiplayer
	sm.server_relay = false            # Clients talk only to the server.
	sm.allow_object_decoding = false   # Already the default; keep it explicit.
```

With the relay off, client code that calls `rpc()` expecting the other
clients to receive it must instead call the server, which broadcasts.

## 4. Rate-limit requests per peer

A client can call an `any_peer` RPC thousands of times per second. A token
bucket per peer and per action caps the rate; repeated abuse kicks.

```gdscript
# rate_limiter.gd - server-side; one instance per protected action group
extends RefCounted

var rate_per_sec: float
var burst: float
var kick_after_violations: int

var _tokens: Dictionary = {}     # peer -> float
var _last_ms: Dictionary = {}    # peer -> int
var _violations: Dictionary = {} # peer -> int


func _init(p_rate_per_sec: float = 10.0, p_burst: float = 20.0, p_kick_after: int = 50) -> void:
	rate_per_sec = p_rate_per_sec
	burst = p_burst
	kick_after_violations = p_kick_after


## Returns true if the request may run. Call first thing in the RPC.
func allow(peer: int) -> bool:
	var now: int = Time.get_ticks_msec()
	var tokens: float = _tokens.get(peer, burst)
	var elapsed: float = (now - int(_last_ms.get(peer, now))) / 1000.0
	tokens = minf(burst, tokens + elapsed * rate_per_sec)
	_last_ms[peer] = now
	if tokens < 1.0:
		_tokens[peer] = tokens
		_violations[peer] = int(_violations.get(peer, 0)) + 1
		return false
	_tokens[peer] = tokens - 1.0
	return true


func should_kick(peer: int) -> bool:
	return int(_violations.get(peer, 0)) >= kick_after_violations


func forget(peer: int) -> void:
	_tokens.erase(peer)
	_last_ms.erase(peer)
	_violations.erase(peer)
```

```gdscript
# weapon_server.gd - usage inside an RPC on the server
extends Node

var _fire_limit: RefCounted = (load("res://net/rate_limiter.gd") as GDScript).new(12.0, 4.0, 30)


@rpc("any_peer", "call_remote", "reliable")
func request_fire(aim: Vector2) -> void:
	var peer: int = multiplayer.get_remote_sender_id()
	if not multiplayer.is_server() or not _fire_limit.allow(peer):
		if _fire_limit.should_kick(peer):
			(multiplayer as SceneMultiplayer).disconnect_peer(peer)
		return
	if not aim.is_finite() or aim.length() > 1.01:
		return  # Malformed input.
	# ... apply the shot ...
```

Call `forget(peer)` from the `peer_disconnected` handler.

## 5. Encrypt with DTLS when it matters

ENet traffic is plain UDP. If clients send anything sensitive (tokens,
chat, purchases), enable DTLS on the ENet host right after creating it,
on both sides.

```gdscript
# dtls_setup.gd
extends RefCounted


static func secure_server(peer: ENetMultiplayerPeer, key: CryptoKey, cert: X509Certificate) -> Error:
	return peer.host.dtls_server_setup(TLSOptions.server(key, cert)) as Error


static func secure_client(peer: ENetMultiplayerPeer, server_hostname: String) -> Error:
	# TLSOptions.client() verifies the certificate against the system CA list.
	# For a self-signed test certificate, pass TLSOptions.client(trusted_cert).
	return peer.host.dtls_client_setup(server_hostname, TLSOptions.client()) as Error
```

Load the key and certificate from files outside the exported pack (server
side only). Never put a private key in the client build.

## 6. Kick with a reason

A disconnect gives the client no reason. Send one with a reliable RPC
first, then drop the peer after a short delay so the message can arrive.

```gdscript
# kicker.gd - autoload named Kicker on server and client
extends Node

signal kicked(reason: String)


func kick(peer: int, reason: String) -> void:
	if not multiplayer.is_server():
		return
	_notify_kicked.rpc_id(peer, reason)
	await get_tree().create_timer(0.3).timeout
	(multiplayer as SceneMultiplayer).disconnect_peer(peer)


@rpc("authority", "call_remote", "reliable")
func _notify_kicked(reason: String) -> void:
	kicked.emit(reason)  # The client shows the reason, then returns to the menu.
```

## 7. Limits and lockdown

- `ENetMultiplayerPeer.create_server(port, max_clients)`: set `max_clients`
  to what the match supports, not the default 32.
- `multiplayer.multiplayer_peer.refuse_new_connections = true` once a match
  is full or started, if late join is not allowed.
- Validate every string a client sends (names, chat) for length and
  content before you store or broadcast it.
