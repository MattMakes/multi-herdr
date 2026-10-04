# Simulation / tycoon / management

The player builds and runs a system (a park, a hospital, a city, a factory)
and optimises it from feedback. RollerCoaster Tycoon, Two Point Hospital,
SimCity and Factorio are the reference points.

## Core loop

Place or build → the simulation ticks → read income, expenses and needs →
unlock or expand → fix the bottleneck → repeat. Early decisions must come
fast; a slow start reads as a waiting game.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Simulation clock | fixed ticks, speed 1x/2x/4x, pause | this reference |
| Money and stocks | integer money, sources, sinks, storage caps | `godot-economy-system` |
| Agents | visitors, workers, needs, schedules | `godot-ai-navigation`, `godot-state-machine` |
| Placement | grid snap, ghost, cost preview | `godot-2d-essentials` or `godot-3d-essentials` |
| Unlocks | research, milestones | `godot-quest-system` |
| Dashboards | income vs expense graphs, overlays | `godot-ui`, `godot-hud-system` |
| Data | building and product definitions as Resources | `godot-resource-pattern` |
| Heavy ticks | worker tasks for large graphs | `godot-multithreading` |
| Saves | world, money, agents | `godot-save-load` |

## Scene tree (4.7)

```text
Game (Node)
├── SimClock (autoload; ticks, speed, pause)
├── Economy (autoload; integer cents, ledger per category)
├── World (Node2D or Node3D)
│   ├── Grid (TileMapLayer or GridMap)
│   ├── Buildings (each with a duplicated BuildingData Resource)
│   └── Agents (managed in batches, not one _process each)
└── UI (CanvasLayer; build menu, speed buttons, finance panel)
```

## Genre code

Run the simulation in fixed ticks, independent of frame rate and of the
speed setting. At 4x the clock runs four ticks per step; the result is the
same as 1x, only sooner.

```gdscript
extends Node

signal sim_tick(tick: int)

@export var tick_seconds := 0.25
@export var max_ticks_per_frame := 16
var speed := 1          # 0 = paused, 1, 2, 4
var tick := 0
var _acc := 0.0

func _physics_process(delta: float) -> void:
	if speed == 0:
		return
	_acc += delta * speed
	var ran := 0
	while _acc >= tick_seconds and ran < max_ticks_per_frame:
		_acc -= tick_seconds
		tick += 1
		ran += 1
		sim_tick.emit(tick)
	if ran == max_ticks_per_frame:
		_acc = 0.0   # drop the backlog rather than freeze
```

Agents and buildings connect to `sim_tick`. With many agents, update one
slice per tick (for example agents where `id % 4 == tick % 4`).

## Pitfalls

- Float money. Rounding errors build up. Store integer cents.
- Simulation in `_process()`. Results change with frame rate. Use fixed ticks.
- `Engine.time_scale` for game speed. It speeds up UI tweens and animations
  too. Run more ticks instead.
- One `_process()` per agent with thousands of agents. Update them in
  batches from a manager.
- Linear costs. Use growth curves so expansion stays a decision.
- Hidden numbers. Show a breakdown of income and expense by category.
- No storage limits. Caps create the logistics game.
- A shared building Resource. Upgrading one building upgrades all of that
  type. Duplicate per instance.
- Labels updated every frame. Update on signals.
- Heavy calculations on the main thread. Run them in a `WorkerThreadPool`
  task and apply results with `call_deferred`.
- A full CPU on a static screen. Turn on `OS.low_processor_usage_mode`
  where the game allows it.
- Save files parsed on the main thread. Use a binary format or a thread.
