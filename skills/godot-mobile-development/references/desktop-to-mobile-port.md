Adds the work order and checks for porting a desktop game to phones: an inventory of desktop-only input, touch target sizes, finger occlusion, control schemes per genre, the Android back stack and the on-screen keyboard; read it when a PC project gets an Android or iOS build.

# Porting a desktop game to mobile

> ← Back to [SKILL.md](../SKILL.md)

The opposite direction is in **godot-export-pipeline**
(`references/mobile-to-desktop-port.md`). Touch event handling and the
built-in `VirtualJoystick` are in **godot-input-handling**; safe areas and
stretch settings are in **godot-responsive-ui**.

## Work order

1. **Inventory desktop-only input.** List every use of hover, right click,
   mouse wheel, keyboard shortcuts, mouse position and text entry. Each one
   needs a touch replacement or a cut.
2. **Choose a control scheme** per the table below.
3. **Re-layout the UI** for thumbs: target sizes, occlusion, safe area.
4. **Lifecycle:** save on pause, handle the Android back button.
5. **Performance:** renderer, texture compression and the frame budget
   (SKILL.md section 6 and [battery-and-background.md](battery-and-background.md)).
6. **Store layer:** IAP and ads behind a service interface (SKILL.md, IAP reference).

## Control scheme by genre

| Genre | Scheme |
|---|---|
| Platformer, action | Virtual joystick left, action buttons right |
| Twin-stick shooter | Two virtual joysticks, each tracking its own touch `index` |
| Strategy, puzzle, card | Direct tap, drag and drop, pinch to zoom |
| Turn-based | Tap to select, tap to confirm; no time pressure on input |
| Racing | Tilt (enable the sensor setting) or left / right touch zones |

Track each finger by `InputEventScreenTouch.index`. Two sticks that read
"the current touch" steal each other's finger.

## Touch targets and occlusion

- Minimum target: 44 pt on iOS, 48 dp on Android. Spacing between targets
  matters as much as size.
- The thumb covers the area under and around it. Put health, timers and
  warnings in the top half, never under the controls.
- Ignore touches that start in a thin strip at the screen edge if palm
  touches trigger actions; test this on a real phone.
- Hover does not exist. Anything shown on hover (tooltips, item stats)
  needs a tap-and-hold or a details panel.
- Right click becomes a long press. Godot can do this for you on Android
  with the project setting
  `input_devices/pointing/android/enable_long_press_as_right_click`.
- `input_devices/pointing/emulate_mouse_from_touch` is on by default, so
  mouse-based UI code receives taps. Do not build gameplay on that: there
  is no hover and no second pointer.

## Android back button as a navigation stack

The back button should close the top panel, then the next, and only quit
(with a confirmation) from the main screen.

```gdscript
# ui_stack.gd - autoload named UiStack
extends Node

signal quit_requested

var _stack: Array[Control] = []


func _ready() -> void:
	get_tree().quit_on_go_back = false


func push(panel: Control) -> void:
	_stack.append(panel)
	panel.show()


func pop() -> bool:
	while not _stack.is_empty():
		var top: Control = _stack.pop_back()
		if is_instance_valid(top):
			top.hide()
			return true
	return false


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_GO_BACK_REQUEST and not pop():
		quit_requested.emit()  # The main screen asks "Quit?".
```

## The on-screen keyboard covers text fields

When a `LineEdit` gets focus on a phone, the system keyboard covers the
lower part of the screen. Move the form up by the keyboard height.

```gdscript
# keyboard_lift.gd - on the root Control of a form
extends Control

@export var focused_field: LineEdit

var _base_y: float = 0.0


func _ready() -> void:
	_base_y = position.y


func _process(_delta: float) -> void:
	var keyboard_px: int = DisplayServer.virtual_keyboard_get_height()
	if keyboard_px <= 0 or focused_field == null or not focused_field.has_focus():
		position.y = _base_y
		return
	# Keyboard height is in screen pixels; convert to canvas units.
	var scale_y: float = get_viewport().get_visible_rect().size.y / float(DisplayServer.window_get_size().y)
	var keyboard_top: float = get_viewport().get_visible_rect().size.y - keyboard_px * scale_y
	var field_bottom: float = focused_field.get_global_rect().end.y
	var overlap: float = field_bottom - keyboard_top + 16.0
	position.y = _base_y - maxf(0.0, overlap)
```

`virtual_keyboard_get_height()` returns 0 while the keyboard is hidden and
on platforms without one, so the script does nothing on desktop.

## Test on devices, not in the editor

The editor cannot show finger size, heat, thermal throttling or the real
keyboard. Before `DONE:`, a worker names what it could not test on a device
and lists it for the operator.

- [ ] Every desktop-only input from the inventory has a touch path or is cut
- [ ] Targets at least 44 pt / 48 dp on the smallest supported phone
- [ ] Critical HUD is not under the thumbs
- [ ] Back button pops panels and confirms quit
- [ ] Text fields stay visible above the keyboard
- [ ] Save on pause; correct state after a long background period
- [ ] Portrait and landscape safe areas, if both are allowed
- [ ] Frame rate holds after 10 minutes of play (heat)
