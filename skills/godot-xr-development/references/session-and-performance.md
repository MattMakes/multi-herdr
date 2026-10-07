Adds the OpenXR session lifecycle (focus, visibility, recenter, loss) and the frame-time levers for headsets: refresh rate, foveation, render scale and an effects budget; read it when XR pauses wrongly, recenters badly or misses frames.

# OpenXR session lifecycle and frame time

> proof: not run (needs an XR headset with an OpenXR runtime). The session events, foveation, refresh rates and frame times need the device.

> ← Back to [SKILL.md](../SKILL.md)

## Start the interface

SKILL.md section 1 checks `is_initialized()`. On some setups the
interface is found but not started yet; call `initialize()` once before
you give up.

```gdscript
# xr_boot.gd - on the main scene root
extends Node3D

signal xr_failed

var xr: OpenXRInterface = null


func _ready() -> void:
	xr = XRServer.find_interface("OpenXR") as OpenXRInterface
	if xr == null or not (xr.is_initialized() or xr.initialize()):
		push_error("xr_boot: OpenXR is not available")
		xr_failed.emit()  # Fall back to a flat-screen mode or a message.
		return
	# The runtime paces frames; vsync on the desktop window adds latency.
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	get_viewport().use_xr = true
	xr.session_visible.connect(_on_session_visible)
	xr.session_focussed.connect(_on_session_focussed)
	xr.session_stopping.connect(_on_session_stopping)
	xr.pose_recentered.connect(_on_pose_recentered)


func _on_session_visible() -> void:
	# Visible but not focused: a system menu or overlay has input. The app
	# still renders but must not react to input. Pause gameplay.
	get_tree().paused = true


func _on_session_focussed() -> void:
	get_tree().paused = false


func _on_session_stopping() -> void:
	# The runtime ends the session (headset off, app quitting). Save now.
	pass


func _on_pose_recentered() -> void:
	# The player held the system recenter button. Recenter the reference
	# space and keep the floor height.
	XRServer.center_on_hmd(XRServer.RESET_BUT_KEEP_TILT, true)
```

Session states, in the order a normal run sees them:

| Signal | Meaning | Do |
|---|---|---|
| `session_begun` | The runtime started the session | Load the first scene |
| `session_focussed` | The app has input | Unpause |
| `session_visible` | Rendered, but another layer has input (system menu) | Pause, ignore input |
| `session_stopping` | The session ends soon | Save |
| `session_loss_pending` | The runtime is losing the session (device lost) | Save, show a message on the desktop window |
| `instance_exiting` | The OpenXR instance shuts down | Quit cleanly |

Godot 4.7 adds `user_presence_changed` for headset-on detection (SKILL.md
section 10). Use the focus signals above as well: presence is not
supported on every runtime.

## Frame time levers, in order of cost

1. **Refresh rate.** `OpenXRInterface.get_available_display_refresh_rates()`
   lists what the headset offers; set `display_refresh_rate` to one of them.
   A steady 72 Hz is better than a 90 Hz target that misses frames.
2. **Foveation** (Mobile and Compatibility renderers on standalone
   headsets). `foveation_level` is 0 (off) to 3 (high);
   `foveation_dynamic = true` lets the runtime raise it under load. Check
   `is_foveation_supported()` first. The Godot 4.7 eye-tracked foveation
   settings are in SKILL.md section 7.
3. **Render scale.** `render_target_size_multiplier` scales the eye
   buffers (1.0 is the runtime's recommended size). Set it before
   `initialize()`; the eye buffers are created with the session. For a
   runtime change, lower `Viewport.scaling_3d_scale` in steps instead, the
   same way as the adaptive 3D scale in **godot-mobile-development**.
4. **Effects budget.** On standalone headsets, every full-screen effect is
   paid twice (two eyes). Turn off SSAO, SSR, SDFGI, glow and volumetric
   fog first; bake lighting; keep transparent overdraw low.
5. **Draw calls.** Use `MultiMeshInstance3D` for repeated props and keep
   materials shared (see **godot-optimization**).

```gdscript
# xr_performance.gd - apply after xr_boot.gd has started the session
extends Node

@export var preferred_rates: Array[float] = [90.0, 72.0]
@export var foveation: int = 2  # 0 off, 1 low, 2 medium, 3 high.


func apply(xr: OpenXRInterface) -> void:
	var offered: Array = xr.get_available_display_refresh_rates()
	for rate: float in preferred_rates:
		for available: Variant in offered:
			if is_equal_approx(float(available), rate):
				xr.display_refresh_rate = rate
				apply_foveation(xr)
				return
	apply_foveation(xr)


func apply_foveation(xr: OpenXRInterface) -> void:
	if xr.is_foveation_supported():
		xr.foveation_level = clampi(foveation, 0, 3)
		xr.foveation_dynamic = true


func set_render_scale_before_init(xr: OpenXRInterface, scale: float) -> void:
	# Call before xr.initialize(); the eye buffers are sized at session start.
	if not xr.is_initialized():
		xr.render_target_size_multiplier = clampf(scale, 0.5, 1.5)
```

## Measure on the headset

Desktop frame times say little about a standalone headset. A `DONE:` for
XR performance work names the headset, the refresh rate, and the frame
time measured on it (the runtime's performance overlay or the Godot
profiler over a remote debug session). If no headset was available, say so
and mark the result unverified.
