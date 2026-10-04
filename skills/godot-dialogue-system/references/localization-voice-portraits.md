Adds translated dialogue (translation keys, plurals, context), voice-over per line, text-to-speech, and portraits by speaker and mood. Read it when the game ships in more than one language, has recorded voice, or shows a portrait that changes with the line.

> ← Back to [SKILL.md](../SKILL.md)

# Localization, Voice and Portraits

## Store keys, not sentences

The `DialogueLine.text` in SKILL.md holds the sentence. For a translated game, hold a **translation key** and look it up when the line is shown. The key is stable; the sentence changes per locale and per rewrite.

| Field | Holds | Example |
|---|---|---|
| `text` | translation key | `BLACKSMITH_GREET_01` |
| choice `"text"` | translation key | `BLACKSMITH_CHOICE_BUY` |
| `speaker` | speaker id, not the display name | `blacksmith` |

Translators work in a CSV or a `.po` file. Import it in Project Settings > Localization > Translations. A CSV has a `keys` column and one column per locale:

```text
keys,en,de
BLACKSMITH_GREET_01,"Welcome, {player_name}.","Willkommen, {player_name}."
SPEAKER_BLACKSMITH,Blacksmith,Schmied
```

Translate first, then fill placeholders. If you format first, the translated table never matches the key.

```gdscript
# dialogue_text.gd
class_name DialogueText
extends RefCounted


## Turns a key into display text: translate, then fill {placeholders}.
static func render(owner: Object, key: String, values: Dictionary) -> String:
    return owner.tr(key).format(values)


## Plural-aware text. `key` is the singular form; `plural_key` the plural.
static func render_count(owner: Object, key: String, plural_key: String, count: int) -> String:
    return owner.tr_n(key, plural_key, count).format({"count": count})


## The same English word can need two translations ("Close" the door, "close" by).
static func render_in_context(owner: Object, key: String, context: StringName) -> String:
    return owner.tr(key, context)
```

- `tr()` and `tr_n()` are `Object` methods. Call them on a node in the tree, so the node's translation domain applies.
- A `Label` or `RichTextLabel` translates its own `text` when its `auto_translate_mode` is not `AUTO_TRANSLATE_MODE_DISABLED`. The dialogue box sets text from code after `tr()`, so set `auto_translate_mode = Node.AUTO_TRANSLATE_MODE_DISABLED` on those labels. Otherwise the label translates the already translated text a second time, and a sentence that matches another key changes.
- On a locale change (`TranslationServer.set_locale()`), the open line does not re-render by itself. Listen for `NOTIFICATION_TRANSLATION_CHANGED` in the dialogue UI and render the current line again.
- **BBCode survives translation** when the tags are in the translated string. Tell translators to keep `[b]`, `[color]` and placeholders as they are, and check that each translated row has the same placeholders as the source row in a test.
- Typewriter speed in characters per second reads well in Latin scripts. For Chinese, Japanese or Korean, use a lower rate or a fixed time per line.

## Voice-over per line

A recorded line is an `AudioStream` named after the line id, per locale. Do not build the path by string concatenation in the UI; resolve it in one place, and fall back to text only when no file exists.

```gdscript
# voice_line_player.gd
class_name VoiceLinePlayer
extends AudioStreamPlayer

signal voice_finished

## Path pattern; {locale} and {id} are filled in. Files are imported, so check the source path.
@export var path_pattern: String = "res://voice/{locale}/{id}.ogg"


func play_line(line_id: String) -> bool:
    stop()
    var lang := TranslationServer.get_locale().get_slice("_", 0)
    var path := path_pattern.format({"locale": lang, "id": line_id})
    if not ResourceLoader.exists(path):
        return false
    stream = load(path) as AudioStream
    play()
    return true


func skip() -> void:
    if playing:
        stop()
        voice_finished.emit()


func _ready() -> void:
    finished.connect(func() -> void: voice_finished.emit())
```

- **Skip still works with voice.** The first confirm press stops the voice and completes the text; the second advances. Never lock input until the clip ends.
- `ResourceLoader.exists()` checks the path that `load()` accepts. In an export, the `.ogg` source file is gone and only the imported data remains, so `FileAccess.file_exists()` on the source path is `false`. Use `ResourceLoader.exists()`.
- Put the voice player on a `Voice` audio bus, so the settings menu has its own volume slider and the music can duck under speech.
- When a line has voice, match the typewriter to the clip: tween `visible_ratio` over `stream.get_length()` seconds.
- Many voice files: preload the next line's stream with `ResourceLoader.load_threaded_request()` while the current line plays.

## Text-to-speech

`DisplayServer.tts_speak()` reads text with the OS voice. It is an accessibility feature (screen-reader-like narration), not a replacement for recorded voice.

```gdscript
# dialogue_tts.gd
class_name DialogueTTS
extends Node

var _voice_id: String = ""


func _ready() -> void:
    var voices := DisplayServer.tts_get_voices_for_language(TranslationServer.get_locale())
    if not voices.is_empty():
        _voice_id = voices[0]


func speak(text: String) -> void:
    if _voice_id.is_empty():
        return
    # interrupt = true: a new line cuts the old one, as a skip should.
    DisplayServer.tts_speak(text, _voice_id, 50, 1.0, 1.0, 0, true)


func stop() -> void:
    DisplayServer.tts_stop()
```

- Enable the project setting `audio/general/text_to_speech` first. Without it, TTS calls do nothing.
- Strip BBCode before you speak: read `RichTextLabel.get_parsed_text()` instead of the raw text.
- Platform voices differ. Test on each target, and keep the feature behind an option that is off by default unless accessibility needs it on.

## Portraits by speaker and mood

Keep portraits in data, keyed by speaker id and mood, so writers can change a face without code.

```gdscript
# portrait_set.gd
class_name PortraitSet
extends Resource

@export var speaker_id: StringName = &""
@export var display_name_key: String = ""
## mood -> texture. "neutral" is the fallback.
@export var moods: Dictionary[StringName, Texture2D] = {}


func get_portrait(mood: StringName) -> Texture2D:
    if moods.has(mood):
        return moods[mood]
    return moods.get(&"neutral")
```

- Add an optional `mood` field to the line data (`@export var mood: StringName = &"neutral"`). The UI asks the speaker's `PortraitSet` for that mood.
- Keep one `Dictionary[StringName, PortraitSet]` registry in the dialogue manager, loaded once. The UI never loads textures by path.
- A portrait change between lines of the same speaker should not replay the entry animation. Compare the speaker id with the previous line before you tween.
