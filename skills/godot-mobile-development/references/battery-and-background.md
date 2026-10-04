Adds battery, heat and responsiveness rules for phones: what happens in the background, foreground frame budgets, adaptive 3D resolution, main-thread stalls and sensor settings; read it when a mobile build drains battery, overheats, stutters or gets ANR reports.

# Battery, heat and the main thread on mobile

> ← Back to [SKILL.md](../SKILL.md)

## Background is cheap; the foreground is where battery goes

When the app goes to the background, Android and iOS stop its rendering,
and Godot sends `NOTIFICATION_APPLICATION_PAUSED`. The game does not tick
there, so lowering the frame rate on pause saves nothing. Use the pause
notification to save (SKILL.md section 2) and to stop network sessions
that the OS will cut anyway.

The real drain is a foreground app that renders 60 frames per second while
nothing moves: a menu, a map screen, a turn-based board waiting for input.

| Screen | Setting |
|---|---|
| Static menu or board | `Engine.max_fps = 30`, or `OS.low_processor_usage_mode = true` for UI that redraws only on change |
| Action gameplay | Target 60, or 30 on low-end devices; never uncapped |
| Cutscene video | The video rate (often 30) |

`OS.low_processor_usage_mode` redraws only when the screen changes. It
suits UI-only apps and menus, not gameplay with continuous animation.

## Resume: the clock moved, the game did not

On `NOTIFICATION_APPLICATION_RESUMED`, compute offline progress from the
wall clock, not from accumulated `delta`. Re-check that network sessions
still exist and reconnect if not.

```gdscript
# lifecycle.gd - autoload named Lifecycle
extends Node

signal resumed_after(seconds_away: float)

var _paused_at_unix: float = 0.0


func _notification(what: int) -> void:
	match what:
		NOTIFICATION_APPLICATION_PAUSED:
			_paused_at_unix = Time.get_unix_time_from_system()
		NOTIFICATION_APPLICATION_RESUMED:
			if _paused_at_unix > 0.0:
				var away: float = maxf(0.0, Time.get_unix_time_from_system() - _paused_at_unix)
				resumed_after.emit(away)  # Timers, energy refills, reconnect.
```

The wall clock is player-controlled. For anything worth cheating on, ask
the server for the time.

## Hold the frame rate by lowering 3D resolution

Phones throttle when they heat up, and the frame rate drops minutes into a
session. Rendering the 3D scene at a lower internal resolution and keeping
the 2D UI sharp is the cheapest fix. `Viewport.scaling_3d_scale` does
this; the UI is not affected.

```gdscript
# adaptive_resolution.gd - add to the gameplay scene
extends Node

@export var target_fps: float = 60.0
@export var steps: Array[float] = [1.0, 0.85, 0.7, 0.55]
@export var check_interval_sec: float = 2.0

var _step: int = 0
var _timer: float = 0.0


func _process(delta: float) -> void:
	_timer += delta
	if _timer < check_interval_sec:
		return
	_timer = 0.0
	var fps: float = Engine.get_frames_per_second()
	if fps < target_fps - 8.0 and _step < steps.size() - 1:
		_step += 1
	elif fps > target_fps - 1.0 and _step > 0:
		_step -= 1  # Climb back slowly when there is headroom.
	else:
		return
	get_viewport().scaling_3d_scale = steps[_step]
```

Change one step per interval, as above. Jumping straight to the lowest
step on a single slow frame makes the image flicker between qualities.
FSR scaling (`Viewport.scaling_3d_mode`) improves the look of low scales
on the Forward+ and Mobile renderers.

## Never stall the main thread

Android reports "Application Not Responding" when the app does not handle
input for about 5 seconds, and players see a hitch long before that.

- Load scenes with `ResourceLoader.load_threaded_request` behind a loading
  screen (see **godot-scene-organization**).
- Write large saves on a worker thread (`WorkerThreadPool.add_task`), and
  write to a temporary file first (the atomic save in
  **godot-export-pipeline**, `references/console-targets.md`).
- Do not call `OS.delay_msec` or wait on a thread from the main thread.

First-use shader compilation also stalls. Enable the Shader Baker in the
export preset (**godot-export-pipeline**, section 7) before you write any
warm-up code.

## Sensors are off by default

The accelerometer, gravity, gyroscope and magnetometer project settings
(`input_devices/sensors/enable_accelerometer`, `enable_gravity`,
`enable_gyroscope`, `enable_magnetometer`) default to `false` in Godot 4.7.
`Input.get_accelerometer()` and the other sensor getters return zero until
the matching setting is on. Turn on only the sensors the game reads: each
one costs power.

## Haptics

`Input.vibrate_handheld(duration_ms, amplitude)` takes an amplitude from
0.0 to 1.0 (`-1.0` uses the device default). Android needs the `VIBRATE`
permission in the export preset. Keep pulses short (10 to 40 ms for UI
feedback) and give the player a setting to turn them off.
