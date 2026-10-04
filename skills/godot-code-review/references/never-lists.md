Adds domain red-flag lists for a Godot review beyond the base checklist: security, signals and lifetimes, typing settings, rendering cost, paths and exports, threads, networking, saves and physics; read it when a review covers one of those areas, and load only the sections the change touches.

# Never-lists by domain

> ← Back to [SKILL.md](../SKILL.md)

Each item is a pattern to flag, why it hurts, and what to ask for. Report
findings in the SKILL.md section 9 format. Cite the line and quote the
code; do not report a rule without the evidence that the code breaks it.

## Security

| Flag | Why | Ask for |
|---|---|---|
| `Expression.parse()` / `execute()` on text a player or a mod wrote | Runs arbitrary method calls on the base instance | A whitelist parser, or `execute` with a base object that exposes only safe methods |
| `bytes_to_var_with_objects`, `get_var(true)`, `SceneMultiplayer.allow_object_decoding = true` on network or file data | Decoding an `Object` runs its script | `bytes_to_var`, `get_var()`, object decoding off |
| Loading `.tres` / `.res` / `.tscn` files from `user://` or downloads | Resources can embed scripts, which run on load | Saves in JSON or `ConfigFile`; check and sign any downloaded pack |
| `OS.execute` / `OS.create_process` with player text in the arguments | Command injection | Fixed argument lists; validate every value |
| Secrets (API keys, store keys) in scripts or `project.godot` | The `.pck` is readable by anyone | A server-side call; the key never ships |

## Signals and lifetimes

| Flag | Why | Ask for |
|---|---|---|
| `connect("timeout", _on_timeout)` with a string name | No compile-time check; a rename breaks silently at runtime | `timeout.connect(_on_timeout)` |
| A lambda connected to a long-lived signal (an autoload, a global bus) from a short-lived node | The lambda is not disconnected when the node frees; it keeps running or errors | A method callable (auto-disconnects when the target frees), `CONNECT_ONE_SHOT`, or an explicit disconnect in `_exit_tree()` |
| `connect` in `_enter_tree()` of a node that is removed and re-added | `_ready()` runs once per node, but `_enter_tree()` runs on each re-entry: connecting there errors with "already connected" for a method, and a new lambda each time runs the handler twice | Connect in `_init()`, or check `is_connected()` first |
| `await` on a signal of another node that may be freed first | The coroutine never resumes | Await something owned by the same node, or check validity and use a timeout |

Evidence for the lambda row, tested on Godot 4.7.2: after the connecting
node was freed, its method connection was removed, but its lambda
connection stayed and ran on `emit()`.

## Typing settings

The base skill covers typed declarations. Ask also for the project to
enforce them:

- `debug/gdscript/warnings/untyped_declaration` set to Error (value 2) in
  new code, Warn (1) when adopting it in an old project.
- `debug/gdscript/warnings/unsafe_method_access` and
  `unsafe_property_access` at Warn: they mark calls the analyzer cannot
  check.
- Typed collections: `Array[Enemy]`, and since Godot 4.4 typed
  dictionaries such as `Dictionary[StringName, int]`.

## Rendering cost

| Flag | Why | Ask for |
|---|---|---|
| `material.duplicate()` per instance to change one color or value | Each copy is a separate material; batching and the shader cache suffer | An `instance uniform` in the shader and `set_instance_shader_parameter()` on the `GeometryInstance3D` (or `CanvasItem` in 2D) |
| Hundreds of identical `MeshInstance3D` nodes | One draw call and one node each | `MultiMeshInstance3D` |
| `_process` that rebuilds UI text or meshes every frame without a change | Constant CPU cost | Update on the signal that changes the value |

## Paths and exports

| Flag | Why | Ask for |
|---|---|---|
| A `res://` path whose letter case differs from the file on disk | Works on Windows and macOS during development, fails in the export, where paths are case-sensitive | Exact case; lowercase snake_case file names |
| Absolute node paths (`/root/Main/Player`) in gameplay code | Breaks when the scene tree changes, and in tests | Groups, `%UniqueName`, exported node references |
| `@export var target: Node` | Any node can be assigned; errors appear at runtime | The specific class: `@export var target: Player` |
| Writes to `res://` at runtime | Read-only in an export | `user://` |

## Threads

| Flag | Why | Ask for |
|---|---|---|
| Adding, removing or changing nodes in the active tree from a `Thread` or `WorkerThreadPool` task | The scene tree is not thread-safe; crashes are random | Build the data on the thread, then `call_deferred()` to apply it on the main thread |
| A `Thread` started without `wait_to_finish()` | Leaks, and an error at exit | Join in `_exit_tree()` |

## Networking

| Flag | Why | Ask for |
|---|---|---|
| `@rpc("any_peer")` handler that applies the change without checking `multiplayer.get_remote_sender_id()` | Any client can act for any player | Check the sender owns the object and the request is legal |
| `"reliable"` on per-tick position or input streams | A lost packet delays every later one | `"unreliable_ordered"` on its own channel |
| The client sends results ("I hit for 50") instead of intents ("I fired here") | Trivial cheating | The server computes results |
| No rate limit on `any_peer` RPCs on a public server | Flooding | A per-peer limit (**godot-dedicated-server**, `references/host-security.md`) |

## Saves and economy

| Flag | Why | Ask for |
|---|---|---|
| Saving whole node trees (`PackedScene.pack` of the live level) as the save format | Breaks on every scene change; loads scripts | A versioned data dictionary |
| No version field in the save data | Old saves cannot be migrated | `"version": N` and a migration step per version |
| Floats for currency | Rounding errors accumulate | Integers in the smallest unit |
| Writing the save file in place | A crash mid-write corrupts it | Write a temporary file, then rename |

## Physics

| Flag | Why | Ask for |
|---|---|---|
| Moving physics bodies in `_process` | Out of step with the physics tick; jitter | `_physics_process`, plus physics interpolation for smooth drawing |
| Non-uniform scale on a `CollisionShape2D/3D` or its parents | Unreliable collision results | Change the shape's size, keep scale at 1 |
| Raycasts through `PhysicsDirectSpaceState2D` or `PhysicsDirectSpaceState3D` outside the physics tick | The space may be locked or stale | Query in `_physics_process` |
