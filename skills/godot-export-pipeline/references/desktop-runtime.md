Adds the desktop runtime behaviour an exported PC build needs: window modes, multi-monitor and DPI, a persisted settings file, safe quit, focus loss and store SDK guards; read it before shipping a Windows, macOS or Linux build.

# Desktop runtime: what the PC build must do

> ← Back to [SKILL.md](../SKILL.md)

Related: **godot-responsive-ui** owns stretch modes and DPI scaling of the
UI, and **godot-input-handling** owns action rebinding. This file covers
the platform behaviour around them.

## Window modes

| Player choice | `DisplayServer` mode | Note |
|---|---|---|
| Windowed | `WINDOW_MODE_WINDOWED` | Restore the saved size and position |
| Borderless fullscreen | `WINDOW_MODE_FULLSCREEN` | Fast alt-tab; the usual default |
| Exclusive fullscreen | `WINDOW_MODE_EXCLUSIVE_FULLSCREEN` | Lowest latency on Windows; alt-tab is slower |
| Maximized | `WINDOW_MODE_MAXIMIZED` | Windowed, fills the usable screen area |

Offer at least windowed and fullscreen, and persist the choice.

## A settings file under `user://`

```gdscript
# display_settings.gd - autoload named DisplaySettings
extends Node

const PATH: String = "user://settings.cfg"

var window_mode: int = DisplayServer.WINDOW_MODE_FULLSCREEN
var vsync: bool = true
var screen: int = -1  # -1 means "the screen the OS picked".


func _ready() -> void:
	var cfg := ConfigFile.new()
	if cfg.load(PATH) == OK:
		window_mode = cfg.get_value("display", "window_mode", window_mode)
		vsync = cfg.get_value("display", "vsync", vsync)
		screen = cfg.get_value("display", "screen", screen)
	apply()


func apply() -> void:
	if screen >= 0 and screen < DisplayServer.get_screen_count():
		DisplayServer.window_set_current_screen(screen)
	DisplayServer.window_set_mode(window_mode)
	var mode: int = DisplayServer.VSYNC_ENABLED if vsync else DisplayServer.VSYNC_DISABLED
	DisplayServer.window_set_vsync_mode(mode)
	if not vsync:
		# Without vsync, cap near the monitor rate so the GPU does not run flat out.
		Engine.max_fps = int(DisplayServer.screen_get_refresh_rate()) if DisplayServer.screen_get_refresh_rate() > 0.0 else 0


func save() -> void:
	var cfg := ConfigFile.new()
	cfg.load(PATH)  # Keep keys other systems wrote.
	cfg.set_value("display", "window_mode", window_mode)
	cfg.set_value("display", "vsync", vsync)
	cfg.set_value("display", "screen", DisplayServer.window_get_current_screen())
	var err: int = cfg.save(PATH)
	if err != OK:
		push_error("DisplaySettings: save failed (%d)" % err)
```

Rules:
- Never write settings to `res://`; it is read-only in an export.
- Rebinds use `InputEventKey.physical_keycode`, so WASD stays in the same
  place on AZERTY and Dvorak layouts.
- To center a window on a monitor, use `DisplayServer.screen_get_usable_rect(screen)`
  (it excludes the taskbar and dock), not the full screen size.
- `DisplayServer.screen_get_scale()` returns the OS scale factor on macOS,
  Wayland and mobile. On Windows and X11 it returns 1.0; read
  `DisplayServer.screen_get_dpi()` there.

## Quit safely

By default the engine quits on the window close button before your save
code runs. Turn that off and handle the request yourself.

```gdscript
# quit_guard.gd - autoload
extends Node

signal quit_requested


func _ready() -> void:
	get_tree().auto_accept_quit = false


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST:
		quit_requested.emit()  # A menu can ask "save first?" and then call confirm_quit().


func confirm_quit() -> void:
	var settings: Node = get_node_or_null(^"/root/DisplaySettings")
	if settings != null:
		settings.call("save")
	get_tree().quit()
```

macOS sends the same notification for Cmd+Q. Mobile platforms do not send
it: see **godot-mobile-development** for the pause notification.

## Focus loss and stuck keys

When the window loses focus, the OS can keep the key-up event, and a held
movement action stays pressed. On focus out, release the held actions and
pause if the game is real-time.

```gdscript
# focus_guard.gd - autoload
extends Node

const HELD_ACTIONS: Array[StringName] = [&"move_left", &"move_right", &"move_up", &"move_down", &"sprint"]

@export var pause_on_focus_loss: bool = true


func _notification(what: int) -> void:
	match what:
		NOTIFICATION_APPLICATION_FOCUS_OUT:
			for action: StringName in HELD_ACTIONS:
				if InputMap.has_action(action):
					Input.action_release(action)
			if pause_on_focus_loss:
				get_tree().paused = true
		NOTIFICATION_APPLICATION_FOCUS_IN:
			if pause_on_focus_loss:
				get_tree().paused = false
```

A pause menu that the player opened must not be closed by focus-in; track
who paused the tree before you unpause.

## Tools and launchers: save power

An editor-like tool or a menu-only launcher does not need to redraw every
frame. Set `OS.low_processor_usage_mode = true` (project setting
`application/run/low_processor_mode`): the engine redraws only when
something changes.

## Store SDKs behind a guard

> proof: not run (needs the Steam client and GodotSteam) for the Steam calls. A test reaches only the path with no singleton.

Steam (GodotSteam), Epic and Discord integrations are GDExtensions or
modules that exist only in some builds. Never call them directly from
gameplay code. Wrap them in one autoload that checks for the singleton and
a feature tag, and does nothing otherwise.

```gdscript
# store_service.gd - autoload named StoreService
extends Node

var _steam: Object = null


func _ready() -> void:
	if OS.has_feature("steam") and Engine.has_singleton("Steam"):
		_steam = Engine.get_singleton("Steam")


func unlock_achievement(id: String) -> void:
	if _steam == null:
		return  # Editor runs, itch builds and tests take this path.
	_steam.call("setAchievement", id)
	_steam.call("storeStats")
```

The method names on the store singleton belong to that addon and its
version. Check them in the addon's documentation; they are not in the
Godot API dump.
