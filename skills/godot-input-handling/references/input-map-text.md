# The Input Map as text in project.godot

A headless worker has no editor Input Map panel. It writes the actions as
text in the `[input]` section of `project.godot`, then proves them with
`InputMap.action_get_events()`. Everything here was measured on Godot 4.7.2
(macOS, headless, 2026-10-07).

## Format

Each action is 1 key in `[input]`. Its value holds a `deadzone` and an
`events` array. Each event is an `Object(<class>, "<property>": <value>, ...)`
on its own line. This example has 2 actions: `jump` on a key (`keycode`, the
key the layout produces) and `move_left` on a physical key (`physical_keycode`,
the key at that position on a US QWERTY board) plus a second physical key:

```text
[input]

jump={
"deadzone": 0.2,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":32,"physical_keycode":0,"key_label":0,"unicode":0,"location":0,"echo":false,"script":null)
]
}
move_left={
"deadzone": 0.2,
"events": [Object(InputEventKey,"resource_local_to_scene":false,"resource_name":"","device":-1,"window_id":0,"alt_pressed":false,"shift_pressed":false,"ctrl_pressed":false,"meta_pressed":false,"pressed":false,"keycode":0,"physical_keycode":65,"key_label":0,"unicode":0,"location":0,"echo":false,"script":null)
, Object(InputEventKey,"keycode":0,"physical_keycode":4194319)
]
}
```

- The long form is what the editor writes. Measured: the short form
  `Object(InputEventKey,"keycode":0,"physical_keycode":4194319)` also loads;
  the properties it leaves out keep their defaults.
- Set either `keycode` or `physical_keycode`, and the other to `0`.
- The values are the `Key` enum: `KEY_SPACE` is `32`, `KEY_A` is `65`,
  `KEY_LEFT` is `4194319`, `KEY_RIGHT` is `4194321`, `KEY_UP` is `4194320`,
  `KEY_DOWN` is `4194322`. Print one with
  `print(KEY_LEFT)` in a `-s` script when you need another key.
- Prefer `physical_keycode` for movement keys (WASD stays in place on other
  layouts) and `keycode` for keys named by their letter.
- `--import` keeps the section as it is.

## Prove it

Save as `.godot/horch-home/input_check.gd`. It prints each event of each
action named after `--`, and exits 1 when an action is missing:

```gdscript
# Prints every event of each named action, to prove the [input] section.
extends SceneTree


func _initialize() -> void:
	var failed := false
	for action in OS.get_cmdline_user_args():
		if not InputMap.has_action(action):
			print("INPUT_CHECK FAIL ", action, " missing")
			failed = true
			continue
		for event in InputMap.action_get_events(action):
			var key := event as InputEventKey
			if key:
				print("INPUT_CHECK ", action, " keycode=", key.keycode,
						" physical=", key.physical_keycode, " ", key.as_text())
			else:
				print("INPUT_CHECK ", action, " ", event.as_text())
	quit(1 if failed else 0)
```

Run it with the `gd` helper from **godot-build-verify** (`references/commands.md`,
"Common setup"):

```bash
gd -s "$PWD/.godot/horch-home/input_check.gd" -- jump move_left dash
```

Output for the section above (exit 1, because `dash` is not defined):

```text
INPUT_CHECK jump keycode=32 physical=0 Space
INPUT_CHECK move_left keycode=0 physical=65 A - Physical
INPUT_CHECK move_left keycode=0 physical=4194319 Left
INPUT_CHECK FAIL dash missing
```

Check that each action has the number of events you wrote, and read every
line. A typo in a property name does not fail the load. Measured with
`"physcal_keycode":4194319`: no error, exit 0, and the event prints
`keycode=0 physical=0 (unset)`.
