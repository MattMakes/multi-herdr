# Romance / dating sim

Relationship games: the player spends limited time and choices on
characters with their own moods and schedules, and earns a route and an
ending. Tokimeki Memorial, Monster Prom and the Persona social links are the
reference points.

## Core loop

Meet → plan the day → talk, give gifts, go on dates → reach a milestone →
commit to a route (or keep several open) → reach an ending that reflects the
relationship.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Relationship stats | several axes per character, visible to the player | `godot-resource-pattern` |
| Calendar and schedules | a clock autoload that signals hour and day changes | `godot-event-bus` |
| Dialogue and choices | branching lines with conditions on stats and flags | `godot-dialogue-system` or `godot-dialogue-manager` |
| Presentation | portraits, expressions, text reveal | `visual-novel.md`, `godot-ui` |
| Routes and endings | route lock, milestone flags, gallery unlocks | `godot-quest-system` |
| Saves | many slots, flags, gallery across saves | `godot-save-load` |
| Language | translation keys for every line | `godot-localization` |
| Feedback | heart icons, blush effects, stat change pop-ups | `godot-tween-animation` |

## Scene tree (4.7)

```text
Game (Node)
├── Clock (autoload; day, hour; signals day_changed, hour_changed)
├── Relationships (autoload; one profile per character)
├── Town (Control or Node2D; locations the player can visit)
├── DateScene (Control)
│   ├── Background (TextureRect)
│   ├── Portraits (Control; one per character on stage)
│   └── DialogueBox (RichTextLabel + choice buttons)
└── HUD (CanvasLayer; day, time left, stat hints)
```

## Genre code

One affection number makes a vending machine: insert gifts, receive love.
Use several axes, and let the character's state change how much an action
is worth.

```gdscript
extends Resource

@export var character_id: StringName
@export var liked_gifts: Array[StringName] = []
@export var attraction := 0
@export var trust := 0
@export var comfort := 0
var last_date_place: StringName = &""

func date_effect(place: StringName, mood: float) -> Dictionary:
	var gain := 3
	if place == last_date_place:
		gain = 1   # the same place twice in a row is worth less
	gain = roundi(gain * clampf(mood, 0.5, 1.5))
	last_date_place = place
	return {"attraction": gain, "comfort": 1}

func confession_result() -> StringName:
	if attraction < 40:
		return &"no_prompt"
	return &"accepted" if trust >= 30 else &"soft_rejection"
```

Here attraction opens a confession, trust decides it, and comfort can soften
timed choices. Show each change on screen, for example "+2 trust".

## Pitfalls

- One stat for everything. Use separate axes with separate effects.
- Hidden numbers. Give visible hints: hearts, expressions, a line of
  narration.
- The same date every time works. Reduce repeated actions and vary by mood.
- Characters who wait for the player. Give them schedules and the right to
  say no.
- Schedule checks in `_process()`. React to the clock's signals.
- Jealousy wired with hard references between characters. Send romance
  events to a group with `call_group()`; each character reacts.
- Typewriter text in `_process()`. Tween `visible_ratio` and set it to 1.0
  when the player skips.
- Timed choices on the OS clock. Use `get_tree().create_timer()`, which
  follows pause.
- An invisible overlay with `mouse_filter` Stop that blocks the "next"
  click. Set Ignore or Pass on non-interactive layers.
- Dialogue strings in code. Use data files with translation keys.
- Absolute positions for portraits. Use anchors.
- Affection compared with `==` on floats. Store integers.
- Missable events with no warning. Missing an event can matter, but tell
  the player what the calendar holds.
