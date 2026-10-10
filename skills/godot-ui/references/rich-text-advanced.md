Adds RichTextLabel work beyond plain BBCode: safe display of player text, clickable and hoverable links, custom effect tags, reveal timing and auto-scrolling logs; read it before you put dynamic or player-written text into a RichTextLabel.

# RichTextLabel: Advanced Use

> ← Back to [SKILL.md](../SKILL.md). The 4.7 image sizing change (`width_unit`, `height_unit`, `ImageUnit`) is in SKILL.md section 6.

All code targets Godot 4.7. Typewriter reveal for dialogue lines is in
**godot-dialogue-system**; this file covers the label itself.

---

## 1. Player-Written Text Is Data, Not Markup

With `bbcode_enabled = true`, any `[` in a chat line or player name is parsed.
A player can then insert `[img]`, huge `[font_size]` values or a
`[color=#0000]` that hides the line. Use one of two safe paths:

- **Escape:** replace every `[` with `[lb]`. The label shows a literal `[`.
- **Build with the push API:** `add_text()` never parses BBCode, so you can
  mix your own formatting (`push_color()`, `push_bold()`) with raw player text.

```gdscript
extends RichTextLabel

## Appends one chat line. Only the speaker name gets formatting.
func add_chat_line(speaker: String, message: String, speaker_color: Color) -> void:
	push_color(speaker_color)
	push_bold()
	add_text(speaker)
	pop()
	pop()
	add_text(": " + message)
	newline()


## Use when you must build one BBCode string that contains player text.
static func escape_bbcode(raw: String) -> String:
	return raw.replace("[", "[lb]")
```

Each `push_*()` call opens a tag. Call `pop()` once for each push, or use
`pop_all()` to close every open tag.

## 2. Links: Click, Hover and Cursor

`[url=<payload>]text[/url]` makes a span clickable. The label emits
`meta_clicked(meta)` with the payload. Use a prefix in the payload to route
it, for example `item:42` or `npc:smith`.

Players need to see that a span is a link. Keep `meta_underlined` on, and
react to `meta_hover_started` and `meta_hover_ended` (cursor shape, tooltip,
sound). Keep the `meta_clicked` handler short: emit a signal or queue the
action, and do the work elsewhere.

```gdscript
extends RichTextLabel

signal item_link_clicked(item_id: int)
signal npc_link_clicked(npc_id: StringName)


func _ready() -> void:
	bbcode_enabled = true
	meta_underlined = true
	meta_clicked.connect(_on_meta_clicked)
	meta_hover_started.connect(_on_meta_hover_started)
	meta_hover_ended.connect(_on_meta_hover_ended)


func _on_meta_clicked(meta: Variant) -> void:
	var payload := str(meta)
	var sep := payload.find(":")
	if sep < 0:
		push_warning("Links: payload '%s' has no prefix" % payload)
		return
	var kind := payload.left(sep)
	var value := payload.substr(sep + 1)
	match kind:
		"item":
			item_link_clicked.emit(value.to_int())
		"npc":
			npc_link_clicked.emit(StringName(value))
		_:
			push_warning("Links: unknown link kind '%s'" % kind)


func _on_meta_hover_started(_meta: Variant) -> void:
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND


func _on_meta_hover_ended(_meta: Variant) -> void:
	mouse_default_cursor_shape = Control.CURSOR_ARROW
```

A string such as `"Take the [url=item:42]rusty key[/url] to [url=npc:smith]the smith[/url]."`
then routes each link to the correct signal.

## 3. Custom Effect Tags (RichTextEffect)

A `RichTextEffect` script adds a new tag. The script sets a `bbcode` member to
the tag name and implements `_process_custom_fx()`, which runs for each glyph
inside the tag every frame. Tag parameters arrive in `char_fx.env`.

Writing the script is not enough: register an instance on each label, in the
inspector under **Markup → Custom Effects** or with `install_effect()`. An
unregistered tag shows as plain text.

```gdscript
@tool
extends RichTextEffect

## Usage: [hover_bob amp=3.0 speed=4.0]Relic[/hover_bob]
var bbcode := "hover_bob"


func _process_custom_fx(char_fx: CharFXTransform) -> bool:
	var amp := float(char_fx.env.get("amp", 2.0))
	var speed := float(char_fx.env.get("speed", 5.0))
	var phase := char_fx.elapsed_time * speed + char_fx.relative_index * 0.6
	char_fx.offset.y += sin(phase) * amp
	return true
```

```gdscript
extends RichTextLabel

## Assign the hover_bob effect resource (or a script instance) in the inspector.
@export var hover_bob: RichTextEffect


func _ready() -> void:
	bbcode_enabled = true
	if hover_bob:
		install_effect(hover_bob)
	text = "You found the [hover_bob amp=3.0]Moon Relic[/hover_bob]!"
```

Return `true` from `_process_custom_fx()` to draw the glyph. `char_fx.color`,
`char_fx.offset`, `char_fx.transform` and `char_fx.visible` are the usual
things to change. The `@tool` line makes the effect show in the editor.

## 4. Reveal Timing

| Need | Use |
|---|---|
| Fade or reveal a whole line evenly | Tween `visible_ratio` from `0.0` to `1.0` |
| Per-character control (pauses, speed changes, a sound per letter) | Step `visible_characters` yourself and compare it with `get_total_character_count()` |

Both keep BBCode effects running. Never rebuild `text` letter by letter: each
assignment re-parses the whole string. Set `visible_characters_behavior` to
`TextServer.VC_CHARS_AFTER_SHAPING` (the default) so that reveal counts
characters, not glyphs.

```gdscript
extends RichTextLabel

## Reveals the current text over `duration` seconds.
func reveal(duration: float) -> void:
	visible_ratio = 0.0
	var tween := create_tween()
	tween.tween_property(self, "visible_ratio", 1.0, duration)
```

## 5. Logs, Feeds and Credits

- **Follow new lines:** set `scroll_following = true`. The label scrolls to
  the bottom when text is added. With a reveal in progress, the label also has
  `scroll_following_visible_characters`, which follows the last visible
  character instead.
- **Cap the length:** a log that grows for an hour gets slow. Remove old
  paragraphs with `remove_paragraph(0)` when `get_paragraph_count()` passes a
  limit.
- **Big text:** set `threaded = true` so that layout runs on a worker thread.
  Wait for the `finished` signal before you read sizes.
- **Size to content:** `fit_content = true` makes the label as tall as its
  text, for use inside a container.

```gdscript
extends RichTextLabel

@export var max_lines: int = 200


func _ready() -> void:
	scroll_following = true


func log_line(line: String) -> void:
	add_text(line)
	newline()
	while get_paragraph_count() > max_lines:
		remove_paragraph(0)
```

## 6. Performance Notes

- Build a formatted string once and cache it. Do not re-assign a long BBCode
  string every frame.
- Custom effects run per glyph per frame. Keep the math in
  `_process_custom_fx()` small, and use them on short spans.
- For crisp large titles, enable multichannel signed distance field
  rendering on the font import (`multichannel_signed_distance_field`). The
  outline then scales without blur.
