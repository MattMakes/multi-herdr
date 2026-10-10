# Party

Local multiplayer collections: 2 to 4 players on one screen (or split
screen) play short minigames and score across rounds. Mario Party,
WarioWare and Overcooked are the reference points.

## Core loop

Players join → a board or menu picks the next minigame → a 3-second
instruction → play → score → back to the board. Repeat until the final
round.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Device-to-player routing | join by pressing a button, per-device actions | `godot-input-handling` |
| Minigame definitions | one Resource per minigame: scene, rules text, player count | `godot-resource-pattern` |
| Scene cycling | hub ↔ minigame, prefetch during instructions | `godot-scene-organization` |
| Tournament state | scores and round count outside any minigame | `godot-dependency-injection` (autoload) |
| Split screen or shared camera | SubViewport per player, or a zoom-to-fit camera | `godot-camera-system` |
| Menus with a gamepad | focus on every control | `godot-ui` |
| Fair play | handicaps, asymmetric 1-vs-3 balance | this reference |

## Scene tree (4.7)

```text
Party (Node)
├── Tournament (autoload; players, device ids, scores, round)
├── InputRouter (autoload; device -> player, joins, drops)
├── Board (Node2D; or a menu)
└── MinigameHost (Node)
    └── Minigame_X (Node; its own .tscn)
        └── GridContainer (2x2)
            └── SubViewportContainer (stretch on, mouse_filter Pass) x N
                └── SubViewport → Camera2D + its own CanvasLayer HUD
```

All SubViewports in a 2D minigame share one world: set their `world_2d` to
the main viewport's `world_2d`.

## Genre code

Bind each joining device to a player slot. Read the device from the event;
an action name alone does not say which pad pressed it.

```gdscript
extends Node

signal player_joined(slot: int, device: int)
signal player_dropped(slot: int)

const MAX_PLAYERS := 4
var device_to_slot: Dictionary[int, int] = {}

func _ready() -> void:
	Input.joy_connection_changed.connect(_on_joy_changed)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventJoypadButton and event.pressed \
			and event.button_index == JOY_BUTTON_START:
		if not device_to_slot.has(event.device) and device_to_slot.size() < MAX_PLAYERS:
			var slot := device_to_slot.size()
			device_to_slot[event.device] = slot
			player_joined.emit(slot, event.device)

func slot_for(event: InputEvent) -> int:
	return device_to_slot.get(event.device, -1)

func _on_joy_changed(device: int, connected: bool) -> void:
	if not connected and device_to_slot.has(device):
		get_tree().paused = true   # show a "reconnect" overlay that runs while paused
		player_dropped.emit(device_to_slot[device])
```

On reconnect, give the new device id the old slot and unpause; do not
reshuffle other players. Players move with per-slot reads such as
`Input.get_joy_axis(device, JOY_AXIS_LEFT_X)`, or with actions created per
player at runtime with `InputMap.add_action()` and `InputMap.action_add_event()`.

## Pitfalls

- Pad ids hard-coded as 0 and 1. Ids change with plug order. Bind at join.
- Actions such as `p1_jump` baked in the project. Create per-player actions
  at runtime, or read the device from each event.
- A disconnected pad removes the player. Pause and wait for a reconnect.
- Different buttons for "confirm" in each minigame. Keep one scheme
  everywhere: A confirms, B backs out, left stick moves.
- Long rule text. Show a short looping demo and one sentence.
- One CanvasLayer HUD over all split views. Give each SubViewport its own.
- Overlapping SubViewportContainers with `mouse_filter` Stop block input to
  the ones below. Set Pass.
- Scores stored in the minigame scene. They vanish when the scene is freed.
- A 1-vs-3 minigame with equal stats. Give the single player more health,
  speed or a unique tool.
- A static camera in a shared-screen game. Zoom and pan to fit all players.
- Menu controls with focus mode None. Pads cannot reach them.
