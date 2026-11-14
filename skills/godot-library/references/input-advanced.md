# Input, advanced

Target engine: Godot 4.7. The full path of an input event through the engine, what the `Input` singleton stores, and the settings and patterns that decide latency, rebinding and device support.

## Use this shelf when

- An input reaches the wrong node, reaches two nodes, or never arrives.
- Controls feel late, miss fast taps, or differ between frame rates.
- You design rebinding, key prompts, or support for other keyboard layouts.
- You add controller features: rumble, LED, gyro, several pads, odd devices.
- A touch build also gets mouse events, or a desktop build must be tested for touch.

## How the engine works here

- The OS hands events to the `DisplayServer`, which feeds the root `Window`. Each viewport then offers an event in this fixed order, and stops when someone marks it handled:

| Step | Receiver | Notes |
|---|---|---|
| 1 | embedded windows | a focused embedded window takes the event |
| 2 | `_input()` | every node that overrides it, before the GUI |
| 3 | `Control._gui_input()` | mouse: control under the cursor, then ancestors. Keys, pads: focus owner |
| 4 | `_shortcut_input()` | key, shortcut and joypad button events only |
| 5 | `_unhandled_key_input()` | key events only |
| 6 | `_unhandled_input()` | the place for gameplay |
| 7 | physics picking | `_input_event()` on collision objects, if picking is on |

- Inside one step, nodes are called in reverse tree order: the bottom of the scene tree first, the root last. `Viewport.set_input_as_handled()` stops the event; in a Control use `accept_event()`. A viewport does not pass events into a `SubViewport`. Only a `SubViewportContainer` does.
- Events and state are two views. The `Input` singleton keeps the current state of keys, buttons, axes and actions. An `InputEventAction` fed through `Input.parse_input_event()` runs the full path above. `Input.action_press()` only changes the state and never calls `_input()`.
- `Input.is_action_just_pressed()` is true for one process frame or one physics tick, whichever asks. A press and release inside the same frame is still reported, unless the project setting `input_devices/compatibility/legacy_just_pressed_behavior` is on.
- Accumulation: with `Input.use_accumulated_input` (on by default) similar events of one frame are merged and delivered once per rendered frame. Mouse motion is summed. So the rate of `_input()` calls is capped by the render rate.
- `InputMap` is loaded from the project settings at start and is never saved by the engine. Runtime changes live in memory only.

## Advanced techniques

### Pick the callback by intent

- Gameplay: `_unhandled_input()`. The GUI gets its chance first, so a click on a button does not also fire the weapon.
- Global hotkeys that must work while a text field has focus: `_shortcut_input()`.
- Key-only handlers: `_unhandled_key_input()`. It skips all mouse motion events, which are the bulk of the traffic.
- `_input()` only for things that must see events before the GUI, such as an input recorder. Turn it off with `set_process_input(false)` when idle.
- Inside these callbacks test the event (`event.is_action_pressed()`), not the singleton. `Input.is_action_just_pressed_by_event()` (4.5) tells whether this event was the one that started the press.

### Three key codes, three jobs

| Property | Meaning | Use for |
|---|---|---|
| `physical_keycode` | position on a US QWERTY board | movement and action defaults (WASD on any layout) |
| `keycode` | Latin letter of the current layout | shortcuts named by letter, such as Ctrl+S |
| `key_label` | localized label printed on the key | nothing by default; prompts on non-Latin layouts |
| `unicode` | the typed character | text entry only |

For a prompt, convert the stored physical code with `DisplayServer.keyboard_get_label_from_physical()` or `keyboard_get_keycode_from_physical()` and pass it to `OS.get_keycode_string()`. These work on Windows, macOS and Linux. `location` tells left Shift from right Shift.

### Rebinding that survives a restart

Capture the next event in `_input()`, ignore echoes and bare modifiers, then `InputMap.action_erase_events()` and `action_add_event()`. Persist plain data per action (event kind plus `physical_keycode`, `button_index`, or axis and sign), not serialized objects, and rebuild the events on load. `InputMap.load_from_project_settings()` restores the defaults. Use `event.is_match()` to find conflicts. `godot-input-handling` has the full procedure.

### Analog input done right

- `Input.get_vector()` applies one circular deadzone and limits length to 1. Building the vector from two `get_axis()` calls gives a square-like deadzone and faster diagonals.
- The deadzone is per action. `get_vector()` uses the average of the four unless you pass a fifth argument. `InputMap.add_action()` defaults to 0.2; the docs state 0.5 as the Input Map default. A high value hides drift but removes slow walking. Expose it as a setting.
- `get_action_raw_strength()` ignores the deadzone. Use it for your own response curve.

### Low latency

Sources of delay and the control for each:

- V-Sync and buffering: `display/window/vsync/vsync_mode` off removes the wait and adds tearing. With V-Sync on, in Forward+ or Mobile, `rendering/rendering_device/vsync/swapchain_image_count` set to 2 removes up to one frame, but a missed frame then drops to half rate (60 to 30). Only use it when the target hardware holds the refresh rate.
- Variable refresh displays: keep V-Sync on and cap `Engine.max_fps` just below the refresh rate. The class reference gives the formula r - (r * r) / 3600.
- Physics tick: input read in `_physics_process()` waits for the next tick. Raise `Engine.physics_ticks_per_second` to 120, 180 or 240 (multiples of 60) and raise `physics/common/max_physics_steps_per_frame` with it. CPU cost grows about linearly. Do not let players change it in competitive games, because results differ per tick rate.
- Physics interpolation adds latency for tick-driven motion. A higher tick rate offsets it.
- Accumulation: `Input.use_accumulated_input = false` delivers every OS event at once. More callbacks, more CPU. Worth it for drawing tools and precise aim, as below. Know the call rate before you choose: with accumulation on, a motion handler runs up to once per rendered frame (480 times a second on a 480 Hz display); with it off, once per mouse report, up to 8,000 a second on an 8 kHz mouse (figures from the engine team's article). Keep the `InputEventMouseMotion` path of `_input()` and `_unhandled_input()` trivial: test the type first and return.
- Android: `input_devices/buffering/agile_event_flushing` flushes key, touch and pad events before every idle and physics step, which helps when several ticks run per frame. Android only.

```gdscript
extends Node3D

@export var sensitivity: float = 0.002

func _ready() -> void:
	Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
	Input.use_accumulated_input = false

func _unhandled_input(event: InputEvent) -> void:
	var motion := event as InputEventMouseMotion
	if motion != null:
		rotate_y(-motion.screen_relative.x * sensitivity)
```

`screen_relative` is not scaled by the stretch settings. `relative` is, so aim speed would change with window size. `velocity` is zero while captured.

### High polling rate mice on Windows

Mice that report at 2 kHz or more could stall the engine on Windows before 4.7.2. Windows sends both raw and legacy motion messages, and the engine handled each one as it came. The engine team measured, on a top desktop CPU, an 8 kHz mouse in captured mode dropping 4.7.1 below 1 FPS and a visible cursor to 173 FPS, against 1703 and 1103 FPS with the fix (4097 FPS with no movement). 4.7.2 reads raw motion in buffered batches and dispatches legacy motion at most once per frame. Keys, buttons and wheel are unchanged. Linux was not affected. Slower CPUs hit the limit earlier, between 1 and 2 kHz. Consequences: ship on 4.7.2 or later, and treat "the game lags only when I move the mouse" reports from older builds as this bug, not as your code.

### Buffering and coyote time

Store the physics tick of the last press and of the last grounded tick, and accept a jump when both are within a small window. Count in ticks, not seconds, so replays and tests are exact. `godot-input-handling` and `godot-player-controller` hold the code.

### Controllers

- Since 4.5 desktop platforms use SDL 3 for pads, and iOS since 4.7, so mappings and behavior match other SDL games. The engine team moved to SDL because its own driver had piled up issues and missed newer pad features. Android and Web still use the older engine code, so test pads there separately.
- Wrong mapping: test a mapping string with the `SDL_GAMECONTROLLERCONFIG` environment variable before shipping it. `Input.is_joy_known()` is false for an unmapped device.
- `Input.get_joy_info()` returns vendor and product ids and, where present, `steam_input_index`. Use it with `get_joy_name()` to choose button glyphs. It is empty on Android and Web.
- Pads send no echo events. For menu repeat, send your own `InputEventAction` from a timer.
- Pad input arrives while the window is unfocused, which can trigger actions (even a quit) while the player is in another window. Since 4.7 the project setting `input_devices/joypads/ignore_joypad_on_unfocused_application` blocks it. It is off by default to keep old behavior; turn it on unless you rely on background input.
- Rumble: `start_joy_vibration(device, weak, strong, duration)`. Duration 0 runs as long as the device allows. Always offer a strength setting.
- LED (4.6): `has_joy_light()` and `set_joy_light()`. The color cannot be read back.
- Motion sensors (4.7): `has_joy_motion_sensors()`, `set_joy_motion_sensors_enabled()`, then calibrate with the pad lying still (`start_joy_motion_sensors_calibration()` and its stop call). `get_joy_gyroscope()` gives gyro aim. For shakes use `get_joy_accelerometer()` minus `get_joy_gravity()` and ignore readings briefly after a hit: the sensor reports the opposite motion next. Sensors drain the pad battery. LED and motion: Windows, macOS, Linux, iOS only.
- Pause on `Input.joy_connection_changed` when the active pad disconnects.

### Touch and mouse together

`input_devices/pointing/emulate_mouse_from_touch` is on by default: each touch also produces a mouse event with `device` equal to `InputEvent.DEVICE_ID_EMULATION`. That makes Control nodes work on phones, but a script that handles both event types acts twice. Filter by device id or turn the emulation off and handle `InputEventScreenTouch` and `InputEventScreenDrag` by `index` (one index per finger). Handle `canceled`. `emulate_touch_from_mouse` lets you test touch code on desktop. On Android, pan and pinch gestures need `input_devices/pointing/android/enable_pan_and_scale_gestures`.

### Quit requests

Set `get_tree().auto_accept_quit = false`, then react to `NOTIFICATION_WM_CLOSE_REQUEST`: save, ask, and call `get_tree().quit()`. `quit()` itself sends no notification; to tell every node first, call `get_tree().root.propagate_notification(NOTIFICATION_WM_CLOSE_REQUEST)`. Mobile has no close request. Save on `NOTIFICATION_APPLICATION_PAUSED` (about 5 seconds on iOS). The Android back button sends `NOTIFICATION_WM_GO_BACK_REQUEST`, and quits unless `quit_on_go_back` is off.

## Niche but useful

- `Input.flush_buffered_events()` forces delivery mid-frame, before a long blocking step.
- `Input.set_custom_mouse_cursor()` sets a hardware cursor per shape. Max 256x256, 128x128 or less advised, 128x128 max on Web. A sprite cursor trails by at least one frame.
- `CanvasItem.make_input_local()` converts an event to a node's local space.
- `Window.set_ime_active()` and `set_ime_position()` drive IME text entry for a custom text field. `NOTIFICATION_OS_IME_UPDATE` reports composition changes.
- `VirtualJoystick` (4.7) is a built-in touch stick that drives four actions.

## Traps

- Since 4.7 the mouse has device id `InputEvent.DEVICE_ID_MOUSE` and the keyboard `DEVICE_ID_KEYBOARD`, no longer 0. Pad 0 can be device 0. Test by event type. The ids tell keyboard and mouse from pads; they do not tell two keyboards apart.
- Touch on Linux with Wayland exists since 4.7 and is basic: no pan or scale gestures yet.
- The `ui_*` actions drive GUI focus. Do not reuse them for gameplay.
- Android sensor events are off by default since 4.4. Enable them under the `input_devices/sensors` project settings.
- `Input.warp_mouse()` does nothing on Android, iOS and Web.

## Version notes

- 4.4: Android sensors off by default. Apple controller code reworked (discovery, rumble).
- 4.5: SDL 3 pad driver on Windows, macOS and Linux. `is_action_just_pressed_by_event()`.
- 4.6: joypad LED control, the first of the planned advanced pad features.
- 4.7: joypad gyroscope and accelerometer, ignore-unfocused-joypad setting, device ids for mouse and keyboard, `VirtualJoystick` (fixed, dynamic and following modes), SDL 3 on iOS, Wayland touch.
- 4.7.2: high polling rate mouse fix on Windows.

## Related skills

- `godot-input-handling`: action setup, rebinding UI, buffering code, injected input for tests.
- `godot-player-controller`: movement code that consumes these inputs.
- `godot-mobile-development`: touch layouts and mobile lifecycle.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/inputs/inputevent.html
- https://docs.godotengine.org/en/stable/tutorials/inputs/input_examples.html
- https://docs.godotengine.org/en/stable/tutorials/inputs/controllers_gamepads_joysticks.html
- https://docs.godotengine.org/en/stable/tutorials/inputs/controller_features.html
- https://docs.godotengine.org/en/stable/tutorials/inputs/mouse_and_input_coordinates.html
- https://docs.godotengine.org/en/stable/tutorials/inputs/custom_mouse_cursor.html
- https://docs.godotengine.org/en/stable/tutorials/inputs/handling_quit_requests.html
- https://docs.godotengine.org/en/stable/tutorials/rendering/jitter_stutter.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html
- https://godotengine.org/article/fixing-high-polling-rate-mice-on-windows/
- https://godotengine.org/releases/4.4/
- https://godotengine.org/releases/4.5/
- https://godotengine.org/releases/4.6/
- https://godotengine.org/releases/4.7/
