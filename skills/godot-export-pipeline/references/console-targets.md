Adds what a console port needs from the project before a porting partner takes it: how console export works for Godot, controller-first rules, disconnect handling, safe saves and fixed budgets; read it when a console release is planned or a "console-ready" review is asked for.

# Console targets: prepare the project

> ← Back to [SKILL.md](../SKILL.md)

## How a Godot game reaches a console

> proof: not run (needs a console dev kit and its NDA SDK).

Godot's official builds have **no** PlayStation, Xbox or Nintendo Switch
export templates. Console SDKs are under NDA, so console support comes
from a licensed porting partner or a studio with its own NDA'd port. A
fleet worker cannot build or test a console binary.

What a fleet worker can do: make the project console-ready, so the port is
cheap. The rules below are common certification themes. The exact
requirement lists (TRC, XR, Lotcheck) are under NDA; the porting partner
supplies them.

## Controller-first rules

- Every menu works with the D-pad and the face buttons, through focus
  (`Control.grab_focus()`, focus neighbours) and the built-in `ui_*`
  actions. Analog-stick-only menus fail.
- No mouse cursor is ever visible. Set `Input.mouse_mode` to
  `Input.MOUSE_MODE_HIDDEN` at boot for console builds.
- Button prompts come from the active device, never from fixed text such
  as "Press A". The confirm button differs between platforms and regions.
- Do not assume device 0 is player 1. Read `Input.get_connected_joypads()`
  and keep your own player-to-device map.
- Rumble is finite (`Input.start_joy_vibration(device, weak, strong, duration)`
  with a duration) and the settings menu can turn it off.
- Window APIs (`DisplayServer.window_set_mode`, resolution lists) mean
  nothing on a console. Hide those settings with a feature tag.

## Pause when a controller disconnects

```gdscript
# controller_watch.gd - autoload
extends Node

signal player_controller_lost(device: int)

## Devices that belong to an active player. Gameplay code fills this.
var active_devices: Array[int] = []


func _ready() -> void:
	Input.joy_connection_changed.connect(_on_joy_connection_changed)


func _on_joy_connection_changed(device: int, connected: bool) -> void:
	if connected or device not in active_devices:
		return
	get_tree().paused = true
	player_controller_lost.emit(device)  # The pause menu shows "Reconnect controller".


func prompt_family(device: int) -> StringName:
	# Pick a glyph set from the controller name. The port may replace this
	# with the platform's own API.
	var joy_name: String = Input.get_joy_name(device).to_lower()
	if "playstation" in joy_name or "dualsense" in joy_name or "ps4" in joy_name or "ps5" in joy_name:
		return &"playstation"
	if "nintendo" in joy_name or "switch" in joy_name or "joy-con" in joy_name:
		return &"nintendo"
	return &"xbox"
```

Pause also when the system menu opens. The porting layer reports it; on
desktop builds `NOTIFICATION_APPLICATION_FOCUS_OUT` is the stand-in.
Consoles suspend the game rather than close it, so do not put save code
only behind `NOTIFICATION_WM_CLOSE_REQUEST`.

## Saves that survive power loss

Write the new save to a temporary file, then rename it over the old one.
A crash during the write leaves the old save intact. The rename is one
step on POSIX file systems; on other targets, check at load time: if the
main file is missing and the `.tmp` file exists, load the `.tmp` file.
Large saves go to a worker thread so the frame does not stall.

```gdscript
# safe_save.gd
extends RefCounted


static func write_text_atomic(path: String, text: String) -> Error:
	var tmp_path: String = path + ".tmp"
	var file := FileAccess.open(tmp_path, FileAccess.WRITE)
	if file == null:
		return FileAccess.get_open_error()
	if not file.store_string(text):
		file.close()
		return ERR_FILE_CANT_WRITE
	file.close()  # Flushes and releases the handle before the rename.
	return DirAccess.rename_absolute(tmp_path, path)


static func write_text_atomic_async(path: String, text: String) -> int:
	# Returns a WorkerThreadPool task id. Poll is_task_completed(), then
	# call wait_for_task_completion() to release it.
	return WorkerThreadPool.add_task(write_text_atomic.bind(path, text), false, "save")
```

## Fixed budgets

- Frame rate: pick 30 or 60 per mode and hold it. Set `Engine.max_fps`
  and keep vsync on. A frame-time spike is a certification finding, not a
  polish item.
- Memory: console memory is fixed and smaller than a PC. Track
  `OS.get_static_memory_usage()` and the `Performance` monitors in a debug
  overlay on the dev kit, and free scenes on transitions.
- Loading: long loads need a progress screen; use threaded loading
  (`ResourceLoader.load_threaded_request`), see **godot-scene-organization**.

## Hand-off checklist for the porting partner

- [ ] Every menu navigable with D-pad and face buttons only
- [ ] No cursor in any state; no keyboard-only path
- [ ] Prompts follow the active controller type
- [ ] Disconnect pauses and shows a reconnect message
- [ ] Saves are atomic and never written from the main thread for large data
- [ ] Platform services (achievements, presence, cloud saves) sit behind one
      service autoload with a no-op implementation for desktop builds
- [ ] Frame cap and memory budget measured on the lowest target
