Adds three test kinds the base skill does not cover: performance budget tests, orphan and leak checks, and multiplayer tests that run server and client in one process; read it when a test must guard frame-time, memory or netcode behaviour.

# Budget, leak and network tests

> ← Back to [SKILL.md](../SKILL.md)

## Performance budgets

A budget test fails when a hot function gets much slower. Timing is noisy,
so measure many runs, take the median, and set the limit well above the
normal value (3 to 5 times). The goal is to catch an accidental O(n²), not
a 10 % change.

```gdscript
# test_support/bench.gd
class_name Bench
extends RefCounted


## Median time in microseconds of `runs` calls to `body`, after `warmup` calls.
static func median_usec(body: Callable, runs: int = 31, warmup: int = 3) -> int:
	for i: int in warmup:
		body.call()
	var samples: Array[int] = []
	for i: int in runs:
		var start: int = Time.get_ticks_usec()
		body.call()
		samples.append(Time.get_ticks_usec() - start)
	samples.sort()
	return samples[samples.size() / 2]


## "" when the median is within budget, else a failure message.
static func within(label: String, body: Callable, budget_usec: int, runs: int = 31) -> String:
	var median: int = median_usec(body, runs)
	if median <= budget_usec:
		return ""
	return "%s: median %d us > budget %d us" % [label, median, budget_usec]
```

Rules:
- Benchmark logic, not rendering: a headless run draws nothing.
- Build the input outside `body`, so the test measures the work, not
  the setup.
- Debug builds of the engine and GDScript in the editor are slower than an
  export. Budgets are relative guards, not shipping numbers.
- Put budget tests in their own directory, so CI can run them on a quiet
  runner or skip them when timing is unreliable.

## Orphans and leaks

A `Node` that is removed from the tree but never freed is an orphan. It
keeps its memory and its signal connections. Both test frameworks report
orphans per test (GUT in its summary, gdUnit4 per test case). For a check
around one scenario, compare the engine's orphan list before and after.

```gdscript
# test_support/leaks.gd
class_name Leaks
extends RefCounted


## Snapshot of current orphan node ids.
static func orphans() -> Array[int]:
	var ids: Array[int] = []
	ids.assign(Node.get_orphan_node_ids())
	return ids


## Orphans created since `before` was taken, as "Class name" strings.
static func new_orphans(before: Array[int]) -> PackedStringArray:
	var out: PackedStringArray = []
	for id: int in Node.get_orphan_node_ids():
		if id in before:
			continue
		var node: Object = instance_from_id(id)
		if node is Node:
			out.append("%s %s" % [node.get_class(), (node as Node).name])
	return out
```

Usage: take `Leaks.orphans()`, run the scenario (open and close a menu,
load and unload a level), wait one process frame so `queue_free` runs, then
assert that `Leaks.new_orphans(before)` is empty. The list names the
leaked nodes, which is the first thing the fix needs.

`Node.get_orphan_node_ids()` and `Node.print_orphan_nodes()` work in debug
builds (the editor and debug exports), where tests run.

For non-node leaks (resources, `RefCounted` cycles), watch
`Performance.get_monitor(Performance.OBJECT_COUNT)` before and after a
scenario repeated many times: a count that grows with every repetition is
a leak.

## Multiplayer tests in one process

Starting a server process and client processes in a unit test is slow and
fragile. Instead, give two branches of the test's tree their own
`MultiplayerAPI` with `SceneTree.set_multiplayer()`, connect them over
ENet on localhost, and call RPCs as in the real game. Tested on Godot
4.7.2: an RPC sent from the client branch reached the server branch in one
headless process, with the client's peer id as the sender.

```gdscript
# test_support/net_pair.gd
class_name NetPair
extends RefCounted

var server_root: Node
var client_root: Node
var server_api: MultiplayerAPI
var client_api: MultiplayerAPI


## Builds /root/<prefix>Server and /root/<prefix>Client from the same scene,
## connects them, and waits for the connection. Returns OK or an error.
func start(tree: SceneTree, scene: PackedScene, port: int = 17000, prefix: String = "Test") -> Error:
	server_root = scene.instantiate()
	server_root.name = prefix + "Server"
	client_root = scene.instantiate()
	client_root.name = prefix + "Client"
	tree.root.add_child(server_root)
	tree.root.add_child(client_root)
	server_api = MultiplayerAPI.create_default_interface()
	client_api = MultiplayerAPI.create_default_interface()
	tree.set_multiplayer(server_api, server_root.get_path())
	tree.set_multiplayer(client_api, client_root.get_path())

	var server_peer := ENetMultiplayerPeer.new()
	var err: Error = server_peer.create_server(port, 4) as Error
	if err != OK:
		return err
	server_api.multiplayer_peer = server_peer
	var client_peer := ENetMultiplayerPeer.new()
	err = client_peer.create_client("127.0.0.1", port) as Error
	if err != OK:
		return err
	client_api.multiplayer_peer = client_peer

	for i: int in 300:  # About 5 seconds at 60 frames per second.
		if client_peer.get_connection_status() == MultiplayerPeer.CONNECTION_CONNECTED:
			return OK
		await tree.process_frame
	return ERR_TIMEOUT


func stop() -> void:
	if server_api != null and server_api.multiplayer_peer != null:
		server_api.multiplayer_peer.close()
	if client_api != null and client_api.multiplayer_peer != null:
		client_api.multiplayer_peer.close()
	if is_instance_valid(server_root):
		server_root.queue_free()
	if is_instance_valid(client_root):
		client_root.queue_free()
```

Rules:
- Use a port range only tests use, and a different port per test file if
  the runner runs files in parallel.
- Both branches must have the same node layout below their roots, because
  RPC and synchronizer paths are relative to the branch root.
- Always call `stop()` in the test's teardown, also after a failure, or
  the next test cannot bind the port.
- For logic that only needs "am I the server", skip the network: the
  default `OfflineMultiplayerPeer` makes `multiplayer.is_server()` true
  and the unique id 1.
- Add latency with the relay in **godot-multiplayer-basics**
  (`references/local-testing-and-discovery.md`) for tests of prediction
  and interpolation.
