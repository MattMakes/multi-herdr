# Determinism and simulation

Target engine: Godot 4.7. How to build game simulations that repeat exactly, run faster than
real time, and can be tuned, replayed, tested and (if needed) rolled back.

## Use this shelf when

- You design a sim-heavy game: colony, RTS, auto-battler, city builder, roguelike, card game.
- You need replays, a "same seed, same run" promise, or golden-file tests of game logic.
- You want headless batch runs for balance, or fast-forward that gives the same result.
- You plan lockstep or rollback netcode and must know what the engine will not give you.

## How the engine works here

One iteration of the main loop (read from `main/main.cpp` and `scene/main/scene_tree.cpp`):

1. The timer converts real elapsed time into N physics ticks (an accumulator; the rest is
   carried over). `Engine.physics_jitter_fix` lets N drift from the real clock to keep N
   steady. N is capped by `Engine.max_physics_steps_per_frame` (default 8); ticks above the
   cap are dropped, so the game slows down instead of spiraling.
2. For each tick: physics servers deliver the previous step's results (`flush_queries`:
   body state, area signals) -> `SceneTree.physics_frame` signal -> every `_physics_process`,
   ordered by `process_physics_priority`, then tree order -> deferred calls -> physics-mode
   timers, then tweens, each in creation order -> navigation -> the servers step. Forces set
   in tick T are simulated at the end of T and seen in T+1.
3. Once per frame: `SceneTree.process_frame` -> `_process` -> timers -> tweens -> draw.

Every tick gets `delta = Engine.time_scale / Engine.physics_ticks_per_second`, a constant.
Only the number of ticks per rendered frame varies. A simulation written only in
`_physics_process` with that constant is frame-rate independent by construction.

`--fixed-fps <n>` replaces measured time with exactly `1/n` per frame, skips the tick cap and
the frame delay. With vsync off or `--headless`, the game then runs as fast as the CPU
allows and produces the same tick sequence every time. The engine team describes it as
non-real-time simulation: `delta` always has the same value however long a frame took.
Movie Maker forces this mode.

## What is and is not deterministic

| Thing | Repeatable? |
| --- | --- |
| Integer math, `Vector2i`, `Vector3i` | Yes, everywhere |
| `RandomNumberGenerator` with a set `seed` | Yes on one engine version; the algorithm (PCG32) is documented as an implementation detail |
| `Dictionary` iteration | Insertion order (documented). Stable if inserts happen in a stable order |
| `_physics_process` order | Priority, then tree order (engine source). Stable if the tree is built in a stable order |
| Signal callbacks | Connection order in practice (engine source); not a documented promise |
| `call_deferred` from one thread | Call order (documented) |
| Float math in script | Same binary, same machine: yes. Across CPUs, compilers, platforms: not promised (reported by rollback addon authors; measure first) |
| 2D/3D physics (Godot Physics and Jolt) | No. The docs state physics is not deterministic for any engine |
| Global `randf`, `randi`, `Array.shuffle`, `Array.pick_random` | One shared stream; any caller shifts it |
| Thread groups, `WorkerThreadPool` | Completion order is undefined |
| `_process`, `Time.get_ticks_msec`, frame count | Depend on the machine |
| Overlap lists (`Area3D.get_overlapping_bodies`) | Order not documented; sort by your own ID |

## Advanced techniques

### Split simulation from presentation

Keep the rules in plain `RefCounted` objects with no nodes, no physics bodies and no engine
time. Nodes are views: they read the sim state and interpolate. This one decision buys
headless batch runs, cheap snapshots, unit tests without a scene and thousands of ticks per
second. Cost: you write your own movement and collision (grid, circles, AABB) instead of
using physics bodies. For most strategy and management games that is small.

```gdscript
class_name SimCore
extends RefCounted

var tick: int = 0
var rng_loot: RandomNumberGenerator = RandomNumberGenerator.new()
var rng_ai: RandomNumberGenerator = RandomNumberGenerator.new()

func _init(master_seed: int) -> void:
	rng_loot.seed = hash([master_seed, "loot"])
	rng_ai.seed = hash([master_seed, "ai"])

func step(_inputs: Dictionary) -> void:
	tick += 1

func snapshot() -> Dictionary:
	return {"tick": tick, "loot": rng_loot.state, "ai": rng_ai.state}

func restore(data: Dictionary) -> void:
	tick = data["tick"]
	rng_loot.state = data["loot"]
	rng_ai.state = data["ai"]
```

### One seeded RNG stream per system

Give loot, AI, world generation and cosmetic effects their own `RandomNumberGenerator`,
each seeded from a master seed. A new particle effect then cannot change which item drops.
Hash the seed (the generator has no avalanche effect; close seeds give similar streams).
Save `state`, not `seed`, to resume mid-run; set `seed` before `state`, because setting the
seed resets the state. Never call global `randf()` from sim code.

### Own tick loop with fast-forward

Drive the core from an accumulator so sim rate is independent of the physics tick rate and
fast-forward is just "more ticks per frame". Cap ticks per frame by count, never by time.

```gdscript
extends Node

const TICK: float = 1.0 / 30.0
const MAX_TICKS_PER_FRAME: int = 5
var speed: int = 1
var _accum: float = 0.0

func _process(delta: float) -> void:
	_accum += delta * speed
	var budget: int = MAX_TICKS_PER_FRAME * speed
	while _accum >= TICK and budget > 0:
		_accum -= TICK
		budget -= 1
		_sim_tick()

func _sim_tick() -> void:
	pass
```

Views interpolate between the last two sim states with `_accum / TICK`.

### Low tick rate plus physics interpolation

If the sim lives in nodes, turn on `physics/common/physics_interpolation` (2D since 4.3, 3D
and MultiMesh since 4.4) and lower `Engine.physics_ticks_per_second`. The engine team's
stated reason for the feature: run the physics update at a low rate, save CPU, and still
look smooth on high refresh displays; mobile games gain most. Since 4.5 the 3D
interpolation lives in the SceneTree, not the RenderingServer, so
`Node3D.get_global_transform_interpolated` and node logic (cameras, attachments) can read
the in-between transform. Rules: move things only in `_physics_process`; call
`reset_physics_interpolation()` after a teleport; read the interpolated transform only for
presentation, never for rules. Raise `Engine.max_physics_steps_per_frame` when you raise the
tick rate or the time scale, or the game slows down. Set `Engine.physics_jitter_fix` to 0
when you interpolate yourself or run a network game (class reference advice).

### Headless batch runs

A script that extends `SceneTree` or `MainLoop` runs with `--headless -s`. Loop over seeds,
run each sim to the end, write one CSV row per run. A node-free core runs far faster than
real time (measure yours). See `godot-headless-runs` and `godot-bot-balance-loop`.

```gdscript
extends SceneTree

func _initialize() -> void:
	for run_seed: int in range(100):
		var rng: RandomNumberGenerator = RandomNumberGenerator.new()
		rng.seed = run_seed
		print(run_seed, ",", rng.randi_range(1, 6))
	quit()
```

For a sim that lives in the scene tree, run the real game with
`--headless --fixed-fps 60 --quit-after <frames>` and `Engine.physics_jitter_fix = 0`.

### Replays by input recording

Record `(tick, player, command)` tuples plus the master seed, the build version and a hash
of the balance data. Replay by feeding the same commands at the same ticks. Record commands
("move unit 12 to cell 4,7"), not raw `InputEvent`s: commands survive UI and resolution
changes. Store a state checksum every N ticks (`hash(sim.snapshot())`) so a diverging
replay reports the first bad tick. A replay is also the cheapest bug report format.

### Snapshot and restore

Keep sim state in typed arrays, packed arrays and dictionaries of plain values, keyed by
your own integer IDs (not node paths, not instance IDs). `duplicate(true)` on that data is a
snapshot; `var_to_bytes` makes it a save or a network packet. Uses: undo, "retry turn",
AI look-ahead that simulates on a copy, save games, rollback. Typed dictionaries (since
4.4, `Dictionary[int, int]`) catch a wrong key type at the write, not at the desync. If
state holds `Resource` objects, know that since 4.5 `duplicate(true)` copies only
sub-resources internal to that resource and `duplicate_deep` gives explicit control over
what is copied; a snapshot that silently shares a sub-resource is a classic rollback bug.

### Integer and fixed-point math where exactness matters

For lockstep or cross-platform replays, store positions as `int` in sub-units (1/1000 of a
cell) and use `Vector2i`. `int` is 64-bit. Replace `sin`, `cos`, `sqrt` with integer or
table versions. Cost: more code and manual overflow care. Skip it for single-machine
replays and same-binary tests.

### Lockstep versus rollback

- Lockstep sends only inputs and needs bit-exact simulation on all peers: integer math, no
  engine physics, fixed iteration order, checksums to detect desync. Input is delayed.
- Rollback needs the same plus cheap snapshot/restore and the ability to re-run several
  ticks inside one frame. Engine physics cannot be re-stepped from script, so rollback games
  bring their own collision (reported by community rollback addons; one ships fixed-point
  2D physics for this reason).
- Server-authoritative prediction with reconciliation does not need determinism; addons
  such as netfox take that route. See `networking-advanced` and `godot-multiplayer-sync`.

### Simulation level of detail

Tick far or off-screen entities less often and with a coarser model: every 4th tick with a
4x step, or a statistical update ("this farm yields 3 per minute"). Stagger by ID so the
load is flat: `(tick + id) % 4 == 0`. Keep the rule a function of sim state (distance to a
sim-side point of interest), never of the camera, or replays diverge.

### Tick budgets and time-slicing

Spread heavy jobs (pathfinding, AI planning) across ticks with a queue and a fixed count
per tick. A time budget (`Time.get_ticks_usec`) is smoother but makes results depend on
machine speed; use it only for presentation work.

## Niche but useful

- `AnimationMixer.callback_mode_process` set to manual plus `advance(delta)` drives
  animation from sim ticks, so hit frames land on the same tick every run.
- `GPUParticles3D.use_fixed_seed` and `seed` (since 4.4) make effects repeat for captures.
- `PhysicsServer3D.set_active(false)` stops physics stepping while a turn-based sim runs.
- The Game screen speed menu (since 4.6) also scales the tick rate; `--time-scale` does not.
- `IterateIK3D.deterministic` (since 4.6; `TwoBoneIK3D` and `SplineIK3D` always are): the
  pose depends only on the target and the input pose, not on the previous frame. Peers that
  share only the IK target then show the same pose. Pair it with
  `LimitAngularVelocityModifier3D` to smooth the jumps this mode can produce.
- `FileAccess.create_temp` (since 4.4) gives batch runs scratch files that delete
  themselves.
- A custom `Logger` (since 4.5) can collect errors per run in a batch, and script
  backtraces work in release builds when
  `debug/settings/gdscript/always_track_call_stacks` is on.

## Traps

- `AnimationMixer.deterministic` is about blend math (un-normalized weights and RESET
  values). It does not make playback repeatable in the sense of this shelf.
- `Engine.time_scale` above 1 stretches each physics tick; it does not add ticks.
- Erasing and re-adding a `Dictionary` key moves it to the end of iteration.
- Sub-thread process groups run in parallel; write results to per-index slots and merge in
  index order on the main thread.
- A replay recorded with Godot Physics is invalid under Jolt, and across engine versions.
- `randomize()` is called at startup since 4.0; unseeded code is never repeatable.
- Tweens and `SceneTreeTimer`s in sim logic tie rules to frame timing. Count ticks.

## Version notes

- 4.4: 3D and MultiMesh physics interpolation; particle fixed seeds; typed dictionaries;
  Jolt available as an experimental option.
- 4.5: 3D interpolation moved to the SceneTree (same API); `duplicate_deep`; `Logger` and
  script backtraces; `TileMapLayer` colliders merged into chunks (body-to-cell lookups
  became approximate).
- 4.6: new projects use Jolt for 3D, so golden values recorded with Godot Physics change;
  Game screen speed menu; deterministic IK option.
- 4.7: Jolt soft body and world boundary behavior changed again (see `version-ledger`);
  keyboard and mouse events carry their own device IDs, which matters if recorded input
  stores `InputEvent.device`.

## Related skills

- `godot-headless-runs` - command lines and CI for headless execution.
- `godot-bot-balance-loop`, `godot-balance-model-data` - bots and balance data.
- `godot-multiplayer-sync` - high-level state sync and prediction.
- `godot-testing`, `godot-test-loop` - unit and scene tests.
- `godot-save-load` - persisting snapshots.
- Sibling shelves: `physics-advanced`, `math-and-procedural`, `threads-and-concurrency`.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/physics/physics_introduction.html
- https://docs.godotengine.org/en/stable/tutorials/physics/interpolation/index.html
- https://docs.godotengine.org/en/stable/tutorials/math/random_number_generation.html
- https://docs.godotengine.org/en/stable/tutorials/scripting/idle_and_physics_processing.html
- https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
- https://raw.githubusercontent.com/godotengine/godot/4.7/main/main.cpp
- https://raw.githubusercontent.com/godotengine/godot/4.7/main/main_timer_sync.cpp
- https://raw.githubusercontent.com/godotengine/godot/4.7/scene/main/scene_tree.cpp
- https://raw.githubusercontent.com/godotengine/godot/4.7/core/object/object.cpp
- https://raw.githubusercontent.com/godotengine/godot/4.7/scene/main/node.h
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/article/movie-maker-mode-arrives-in-godot-4/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-1/
- https://godotengine.org/article/dev-snapshot-godot-4-5-dev-4/
- https://godotengine.org/article/inverse-kinematics-returns-to-godot-4-6/
- https://raw.githubusercontent.com/foxssake/netfox/main/README.md
- Third-party blog hosts (fixed-timestep and rollback articles) were not reachable.
