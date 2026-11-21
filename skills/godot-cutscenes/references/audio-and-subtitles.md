# Audio sync and subtitles

Every block here is `proof: not run (needs Godot)`.

## Which clock leads

| Scene | Leader | How |
|---|---|---|
| Under about 20 s, effects and short lines | the `AnimationPlayer` | audio tracks start each stream at its key |
| Music-led, or longer than about 20 s | the music | audio-master sync below |
| Voice-led dialogue | each voice line | the line's length sets the shot length; the next shot starts on the line's end |

The picture runs on frame time and sound runs on the audio device's time. They agree at the start and then drift by a few milliseconds per second. Past about 20 s a cut that should hit a beat misses it by a frame or more.

## Audio-master sync

The music plays on its own `AudioStreamPlayer`. The `AnimationPlayer` does not advance by itself. Each frame, the script advances it to the music's time. Method keys still fire, because `advance()` processes them.

```gdscript
extends Cutscene
## The music is the clock. Set the AnimationPlayer's callback mode to manual.

@export var music: AudioStreamPlayer


func _enter() -> void:
	player.callback_mode_process = AnimationMixer.ANIMATION_CALLBACK_MODE_PROCESS_MANUAL
	music.play()


func _process(_delta: float) -> void:
	if not running or not music.playing:
		return
	var heard := music.get_playback_position() \
		+ AudioServer.get_time_since_last_mix() \
		- AudioServer.get_output_latency()
	var step := heard - player.current_animation_position
	if step > 0.0:
		player.advance(step)


func _exit() -> void:
	music.stop()
```

- `get_playback_position()` moves in mix-size steps. The 2 `AudioServer` terms smooth it and remove the device delay.
- The time can step back by a few milliseconds. The `step > 0.0` test drops those frames.
- The music stream must not loop, or its position wraps. For a looping bed under a cutscene, the `AnimationPlayer` leads.
- Under Movie Maker the audio is mixed offline and both clocks are exact.

## Beats as markers

A beat grid from the film pipeline (`audio/beats.json` with `beats_sec` and `downbeats_sec`, see the `video-audio` skill) or from any beat tool becomes named markers on the cutscene animation. Snap cuts and hits to them in the editor.

```gdscript
@tool
extends EditorScript
## Run once from the script editor: writes beat markers on an animation.

const ANIMATION_PATH := "res://cutscenes/intro_cutscene.res"
const BEATS_PATH := "res://cutscenes/intro_beats.json"


func _run() -> void:
	var animation := load(ANIMATION_PATH) as Animation
	var data: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(BEATS_PATH))
	var downbeats: Array = data.get("downbeats_sec", [])
	var index := 0
	for time in data.get("beats_sec", []):
		var prefix := "bar" if downbeats.has(time) else "beat"
		animation.add_marker(StringName("%s_%03d" % [prefix, index]), float(time))
		index += 1
	ResourceSaver.save(animation, ANIMATION_PATH)
```

Rules for a music-led scene:

- A cut lands on a beat, within 1 frame. A big reveal lands on a downbeat.
- The picture hits the beat; the sound of an impact starts on the same frame. A move that ends on the beat starts before it.
- The biggest picture moment sits on the music's biggest change. If the track has none where you need it, move the picture, or ask for a music edit. Do not stretch the music.
- Leave 0.3 to 0.5 s of quiet before a large hit. Duck the music 6 to 9 dB under a voice line and release over 0.3 s. Bus rules are in **godot-audio-system**.

## Subtitles as a cue list

A subtitle is a fact about time: between `start_sec` and `end_sec`, this key is on screen. Poll the clock and show the cue that covers it. Then scrub, skip, pause and slow motion all show the right line.

```gdscript
class_name SubtitleTrack
extends Label
## Shows the cue that covers the cutscene time.

@export var cutscene: Cutscene
## Each cue: { "start_sec": float, "end_sec": float, "key": String }
@export var cues: Array[Dictionary] = []

var _shown: int = -2


func _process(_delta: float) -> void:
	var index := _cue_at(cutscene.position_sec()) if cutscene.running else -1
	if index == _shown:
		return
	_shown = index
	text = tr(cues[index]["key"]) if index >= 0 else ""
	visible = index >= 0


func _cue_at(time: float) -> int:
	for i in cues.size():
		if time >= cues[i]["start_sec"] and time < cues[i]["end_sec"]:
			return i
	return -1
```

The cue list comes from the voice files. A word-timing tool gives each word a start and an end; the film pipeline writes them to `audio/sync-sheet.json` (`vo_segments[].words[]` with `word`, `start_sec`, `end_sec`). Group words into cues by these rules, and store the result as JSON or as the exported array:

- 1 cue is 1 phrase: at most 2 lines of about 42 characters.
- A cue starts with its first word, or up to 0.1 s before it. It ends 0.2 to 0.5 s after its last word, and before the next cue starts.
- A cue stays on screen at least 1.0 s. Reading speed stays under 17 characters per second; 12 for a game aimed at children.
- Break a line at a clause, not in the middle of a name.
- The speaker's name, when shown, is a separate key and a separate label.

Display rules:

- Subtitles are on by default, with a setting for size and for a background plate.
- The label sits in the bottom 20 % of the frame, inside the safe area, with a dark plate at 50 to 70 % opacity or an outline.
- Use a font with the glyphs of every shipped language. Long languages run 30 % longer than English: give the label 2 lines of room and autowrap.
- Sounds that carry story get a caption in brackets when the captions setting is on: `[gate grinds open]`.
- Check every key against the translation files in a test (`verification.md`). Localization rules are in **godot-localization**.

## Word-by-word and typewriter text

- Typewriter: key `visible_ratio` on the label from 0 to 1 over the spoken length of the line. It reads the cutscene clock through the track, so a skip shows the full line.
- Karaoke highlight for a song or a chant: a `RichTextLabel` whose text is rebuilt when the current word index changes, with the spoken word in the accent colour. Take the index from the word times, not from a timer.
- Do not animate each letter of body text. Letter effects are for a title of 1 to 4 words.

## Voice

- 1 file per line, named by its translation key. Then a language swap is a folder swap.
- Start each line with an audio key; the line's file length sets the earliest time of the next line. Leave 0.25 to 0.6 s between speakers, less in an argument.
- A skip stops every voice player in `_exit()`. A voice that keeps talking after a skip is the most common cutscene bug.
- Lip flaps for stylized characters: switch between 2 to 4 mouth shapes from the loudness of the line, sampled into a track, not from a live analyzer.
