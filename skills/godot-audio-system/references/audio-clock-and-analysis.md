Adds timing and signal work: reading the true audio clock for beat-synced gameplay and subtitles, switching music on a beat, generating sound in code with AudioStreamGenerator, and driving visuals from a spectrum analyzer; read it for rhythm mechanics, lip or subtitle sync, procedural tones, or audio-reactive effects.

# The Audio Clock, Generated Sound and Analysis

> ← Back to [SKILL.md](../SKILL.md). Interactive music streams are in [interactive-music.md](interactive-music.md).

All code targets Godot 4.7.

---

## 1. The Audio Clock Is Not the Frame Clock

`get_playback_position()` only advances when the audio server mixes a chunk,
so read alone it jumps in steps. The sound also reaches the speakers later
than it was mixed. The position the player hears now is:

```
heard = get_playback_position()
      + AudioServer.get_time_since_last_mix()
      - AudioServer.get_output_latency()
```

Use this value, not a sum of `delta`, for anything that must match the music:
note highways, beat pulses, subtitles, lip sync. A sum of `delta` drifts from
the music within seconds.

```gdscript
extends AudioStreamPlayer

## Beat clock for a rhythm game. Emits `beat` once per beat of the song.
signal beat(index: int)

@export var bpm: float = 120.0
@export var first_beat_offset: float = 0.0

var _last_beat: int = -1


func heard_time() -> float:
	if not playing:
		return 0.0
	return get_playback_position() + AudioServer.get_time_since_last_mix() - AudioServer.get_output_latency()


func _process(_delta: float) -> void:
	if not playing:
		return
	var seconds_per_beat := 60.0 / bpm
	var index := floori((heard_time() - first_beat_offset) / seconds_per_beat)
	if index > _last_beat:
		_last_beat = index
		beat.emit(index)
```

`get_output_latency()` is an estimate from the driver. Offer a calibration
offset in the settings for rhythm games.

## 2. Subtitles on the Audio Clock

Store cue start times in seconds from the start of the voice line, and show
the cue whose time has passed on the heard clock. A timer started with the
line drifts when the frame rate drops; the audio clock does not.

```gdscript
extends AudioStreamPlayer

## cues: Array of {"t": float, "text": String}, sorted by "t".
signal subtitle_changed(text: String)

var cues: Array[Dictionary] = []
var _shown: int = -1


func play_line(line: AudioStream, line_cues: Array[Dictionary]) -> void:
	stream = line
	cues = line_cues
	_shown = -1
	play()


func _process(_delta: float) -> void:
	if not playing or cues.is_empty():
		return
	var t := get_playback_position() + AudioServer.get_time_since_last_mix() - AudioServer.get_output_latency()
	var reached := _shown
	while reached + 1 < cues.size() and float(cues[reached + 1]["t"]) <= t:
		reached += 1
	if reached != _shown:
		_shown = reached
		var text: String = cues[_shown]["text"]
		subtitle_changed.emit(text)
```

For frame-exact sync with an animation, an `AnimationPlayer` with an audio
track and a method track keeps both on one timeline instead.

## 3. Change Music on a Beat

`AudioStreamInteractive` (see [interactive-music.md](interactive-music.md))
is the first choice, because its transitions snap to beats and bars by
themselves. For two plain players, wait until the next beat on the heard
clock, then crossfade.

```gdscript
extends Node

@onready var current: AudioStreamPlayer = $MusicA
@onready var incoming: AudioStreamPlayer = $MusicB
@export var bpm: float = 120.0
@export var fade_time: float = 1.0


func switch_on_beat(next_song: AudioStream) -> void:
	var seconds_per_beat := 60.0 / bpm
	var heard := current.get_playback_position() + AudioServer.get_time_since_last_mix() - AudioServer.get_output_latency()
	var wait := seconds_per_beat - fmod(heard, seconds_per_beat)
	await get_tree().create_timer(wait).timeout
	incoming.stream = next_song
	incoming.volume_db = -60.0
	incoming.play()
	var t := create_tween().set_parallel(true)
	t.tween_property(current, "volume_db", -60.0, fade_time)
	t.tween_property(incoming, "volume_db", 0.0, fade_time)
	await t.finished
	current.stop()
	var old := current
	current = incoming
	incoming = old
```

## 4. Sound Made in Code: AudioStreamGenerator

An `AudioStreamGenerator` plays sample frames that your script pushes. Use it
for engine hums whose pitch follows speed, alarms, simple synth effects, or
streaming data. Fill only as many frames as the buffer has room for, each
frame.

```gdscript
extends AudioStreamPlayer

## A sine tone whose frequency can change while it plays.
@export var frequency_hz: float = 220.0
@export var amplitude: float = 0.2

var _playback: AudioStreamGeneratorPlayback
var _phase: float = 0.0
var _mix_rate: float = 44100.0


func _ready() -> void:
	var gen := AudioStreamGenerator.new()
	gen.mix_rate = _mix_rate
	gen.buffer_length = 0.1
	stream = gen
	play()
	_playback = get_stream_playback() as AudioStreamGeneratorPlayback
	_fill()


func _process(_delta: float) -> void:
	_fill()


func _fill() -> void:
	if _playback == null:
		return
	var step := frequency_hz / _mix_rate
	for i in _playback.get_frames_available():
		var s := sin(_phase * TAU) * amplitude
		_playback.push_frame(Vector2(s, s))
		_phase = fmod(_phase + step, 1.0)
```

Keep `buffer_length` short (around 0.1 s) for low latency, but long enough
that a slow frame does not empty it: an empty buffer clicks. GDScript is slow
for heavy synthesis; batch with `push_buffer()` or move the work to C# or
GDExtension when the tone needs more than a few voices.

## 5. Visuals from the Mix: Spectrum Analyzer

Add an `AudioEffectSpectrumAnalyzer` to the bus you want to read (for example
`Music`). Its instance returns the magnitude in a frequency range.

```gdscript
extends Node

## Drives a 0..1 "bass" value from the Music bus for VFX or UI.
signal bass_changed(level: float)

@export var bus_name: StringName = &"Music"
@export var min_db: float = -60.0
var _analyzer: AudioEffectSpectrumAnalyzerInstance


func _ready() -> void:
	var bus := AudioServer.get_bus_index(bus_name)
	var fx := AudioEffectSpectrumAnalyzer.new()
	AudioServer.add_bus_effect(bus, fx)
	var index := AudioServer.get_bus_effect_count(bus) - 1
	_analyzer = AudioServer.get_bus_effect_instance(bus, index) as AudioEffectSpectrumAnalyzerInstance


func _process(_delta: float) -> void:
	if _analyzer == null:
		return
	var mag := _analyzer.get_magnitude_for_frequency_range(20.0, 150.0)
	var db := linear_to_db(maxf(mag.x, mag.y))
	bass_changed.emit(clampf((db - min_db) / -min_db, 0.0, 1.0))
```

The analyzer does not change the sound. Smooth the value (for example with
`lerpf()` over a few frames) before it scales anything on screen, or the
visuals flicker.
