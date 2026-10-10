# Visual novel

Story told through text, portraits and backgrounds, with choices that
change lines, flags and endings. Steins;Gate, Doki Doki Literature Club and
Ace Attorney are the reference points.

## Core loop

Read → choose → the story branches or a flag changes → see the
consequence (now or later) → reach one of several endings → replay for the
others.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Script format | lines, labels, jumps, conditions; outside GDScript | `godot-dialogue-system`, `godot-dialogue-manager` |
| Flags | story variables, typed by name | `godot-resource-pattern` |
| Presentation | text box, typewriter, portraits, backgrounds, music | `godot-ui`, `godot-audio-system` |
| Rollback and backlog | step back through lines, reread history | this reference |
| Auto, skip, save and load | standard reader controls, many slots | `godot-save-load` |
| Effects | BBCode effects, screen shake, fades | `godot-tween-animation` |
| Language | translation of every line | `godot-localization` |
| Point-and-click scenes | investigation rooms, if any | `godot-popochiu` |

Pick one runner. A dialogue addon (for example Dialogue Manager, covered by
`godot-dialogue-manager`) owns the script; do not also write your own story
manager.

## Scene tree (4.7)

```text
Novel (Node)
├── Story (autoload; position in the script, flags, history)
├── Stage (Control)
│   ├── Background (TextureRect, full rect anchors)
│   ├── Actors (Control; one TextureRect per speaker, anchors in percent)
│   └── TextBox (PanelContainer)
│       ├── Name (Label)
│       └── Text (RichTextLabel, BBCode on)
├── Choices (VBoxContainer; buttons made per choice)
└── Menu (CanvasLayer; auto, skip, log, save, load)
```

## Genre code

Rollback works only if each step is saved before it changes anything. Push a
snapshot first, then apply the line or the choice.

```gdscript
extends Node

var line_index := 0
var flags: Dictionary[StringName, int] = {}
var background := ""
var history: Array[Dictionary] = []
const MAX_HISTORY := 500

func snapshot() -> void:
	history.push_back({
		"line": line_index,
		"flags": flags.duplicate(),
		"background": background,
	})
	if history.size() > MAX_HISTORY:
		history.pop_front()

func choose(flag: StringName, delta: int, jump_to: int) -> void:
	snapshot()   # before any change
	flags[flag] = flags.get(flag, 0) + delta
	line_index = jump_to

func roll_back() -> bool:
	if history.is_empty():
		return false
	var s: Dictionary = history.pop_back()
	line_index = s["line"]
	flags = s["flags"]
	background = s["background"]
	return true
```

Store anything the screen shows in the snapshot (background, music,
actors), so one rollback restores the whole scene.

## Pitfalls

- Choices that change nothing. Even a converging choice should change the
  next lines or a flag.
- No auto, skip, backlog or rollback. Readers expect all four.
- Walls of text. Keep each box to 3 or 4 lines.
- Story text in GDScript. Keep it in script files that writers can edit,
  with translation keys.
- Flags changed before the snapshot. Rollback then restores the wrong state.
- Typewriter in `_process()`. Tween `visible_ratio` from 0 to 1. On a click,
  kill the tween and set `visible_ratio` to 1.0; on the next click, advance.
- Flat text at emotional beats. Use BBCode effects such as `[shake]` and
  `[wave]`.
- Draw order by `z_index` on Controls. Draw order and input order then
  disagree; call `move_to_front()` on the speaker.
- Pixel positions for actors. Use anchors so portraits fit every screen.
- Large backgrounds loaded with `load()` at the cut. Request them ahead
  with `ResourceLoader.load_threaded_request()`.
- Actors left in the tree after they exit. Free them, or reuse a fixed set.
