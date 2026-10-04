Adds two engine-version facts for high-level multiplayer on Godot 4.7: every peer must run the same Godot version, and the RPC configuration getter was renamed in 4.5; read it when peers fail to talk after an engine upgrade or when code reads RPC configuration at runtime.

# Engine version notes for multiplayer

> ← Back to [SKILL.md](../SKILL.md)

Source: checked by the fleet's GW10 unit against the Godot 4.7.2 `--doctool`
dump and the godot-docs 4.7 branch (revision `9adca4c`).

## All peers run the same Godot version

The high-level multiplayer protocol (`SceneMultiplayer`: RPCs, spawns,
synchronizers) changed in Godot 4.3 (GH-90027), and it is not promised to
stay compatible between engine versions. Build the server and every client
with the same Godot version.

- Ship the dedicated server and the clients from one engine build.
- Put the engine version into the handshake, next to your own protocol
  number, so a mismatch is reported instead of failing silently. The
  handshake is in **godot-dedicated-server**, `references/host-security.md`.

```gdscript
# protocol_version.gd
extends RefCounted

const GAME_PROTOCOL: int = 3


## A string both sides compare during the handshake.
static func current() -> String:
	var v: Dictionary = Engine.get_version_info()
	return "%d.%d.%d-%d" % [v.major, v.minor, v.patch, GAME_PROTOCOL]
```

## `get_node_rpc_config` (renamed in 4.5)

`Node.rpc_config(method, config)` sets the RPC configuration of a method
from code. The getter for the whole node's RPC configuration is
`Node.get_node_rpc_config()` since Godot 4.5 (GH-106848). Code or answers
that use the older name `get_rpc_config` fail on 4.7.

```gdscript
# rpc_config_example.gd
extends Node


func _ready() -> void:
	rpc_config(&"sync_state", {
		"rpc_mode": MultiplayerAPI.RPC_MODE_AUTHORITY,
		"transfer_mode": MultiplayerPeer.TRANSFER_MODE_UNRELIABLE_ORDERED,
		"call_local": false,
		"channel": 1,
	})
	print(get_node_rpc_config())


func sync_state(_state: Dictionary) -> void:
	pass
```
