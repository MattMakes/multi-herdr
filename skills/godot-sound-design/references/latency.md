# Latency: from the event to the ear

> Back to [SKILL.md](../SKILL.md). The evidence is in
> [research.md](research.md) section 3.

A feedback sound helps only when it starts with the event that it reports.
Reynolds and Day (2007) found that a sound that starts with a visual event
speeds a fast visual correction by about 20 ms. That gain is smaller than
one frame at 30 fps, so a sound that is 1 frame late loses it. This file
gives a latency budget, the Godot settings, and 2 ways to measure.

## 1. The budget

The delay from a player's press to the sound in the ear has these parts:

| Part | Typical value | Who controls it |
| --- | --- | --- |
| Input to event | 0 to 1 frame (0 to 16.7 ms at 60 fps) | Game code |
| Event to `play()` call | 0 frames when the code plays on the same frame | Game code |
| `play()` to the next mix | 0 to 1 mix buffer (`AudioServer.get_time_to_next_mix()`) | Engine |
| Output latency | `audio/driver/output_latency`, 15 ms by default on desktop, 50 ms on web | Project setting and OS driver |
| Device | 0 ms wired; Bluetooth headsets often add 100 ms or more | Player hardware |
| Sound onset | Leading silence plus the attack of the sound | Sound asset |

**Pass marks** (practice, inside the ITU-R BT.1359-1 window):

- Event to `play()`: 0 frames. The sound plays on the frame of the event.
- `AudioServer.get_output_latency()` on the target desktop: at most 30 ms.
- Leading silence (start of file to the first sample above -60 dBFS): at
  most 1 ms.
- Onset (first audible sample to the first sample within 20 dB of the
  peak): at most 5 ms for a feedback sound. `sfx_report.py` in
  [synthesis.md](synthesis.md) measures both.
- Total, press to ear, wired output, 60 fps: at most 50 ms.

## 2. Godot settings

| Setting | Value | Why |
| --- | --- | --- |
| `audio/driver/output_latency` | 15 (default). Try 10 on desktop if the game has no crackle. | A smaller buffer means less delay, but too small a buffer crackles under load. |
| `audio/driver/output_latency.web` | 50 (default). Do not go lower without a test on target browsers. | Web audio needs a larger buffer. |
| `audio/driver/mix_rate` | 44100 or 48000, the same rate as the sound files | A match avoids resampling. |
| Short sound format | WAV (PCM or QOA) | Ogg and MP3 need decoding, and MP3 adds silence at the start. |
| `AudioStreamPlayer.playback_type` on web | `AudioServer.PLAYBACK_TYPE_SAMPLE` for short feedback sounds | Sample playback goes to the browser's audio engine directly. |
| Pitch | `pitch_scale` on the player, not `AudioEffectPitchShift` | The pitch-shift effect uses an FFT buffer and adds delay. |
| Bus effects on feedback buses | EQ, filters, compressor, limiter only | Reverb and delay add tails, not onset delay; FFT effects add delay. |

## 3. Play on the frame of the event

Connect the sound handler straight to the event signal. Do not use
`call_deferred`, `await`, a `Timer` or a queue that `_process` empties on the
next frame.

```gdscript
extends Node

## Plays the feedback sound in the same call stack as the event.
@export var hit_sound: AudioStream
@onready var _player: AudioStreamPlayer = $HitPlayer


func _ready() -> void:
	# Good: the handler runs inside the emit() call.
	get_parent().connect(&"hit", _on_hit)


func _on_hit(_strength: float) -> void:
	_player.stream = hit_sound
	_player.play()
	# Bad: _player.play.call_deferred() starts the sound 1 frame later.
	# Bad: await get_tree().process_frame before play() does the same.
```

A physics event (a collision signal) arrives in the physics step. Play the
sound there too. Do not store it for `_process`.

## 4. Measure in the engine

The headless driver is a dummy driver: it reports no real latency. Run this
probe in a windowed build on the target machine. It reports the mix rate,
the output latency and the frame gap between an event and its `play()`.

```gdscript
class_name LatencyProbe
extends Node

## Reports audio latency numbers. Call mark_event() where the event fires
## and mark_play() just before play(). Read report() after a test session.

var _event_frame: int = -1
var _gaps: PackedInt32Array = PackedInt32Array()


func mark_event() -> void:
	_event_frame = Engine.get_process_frames()


func mark_play() -> void:
	if _event_frame < 0:
		return
	_gaps.append(Engine.get_process_frames() - _event_frame)
	_event_frame = -1


func report() -> Dictionary:
	var worst_gap: int = 0
	for gap: int in _gaps:
		worst_gap = maxi(worst_gap, gap)
	# The headless audio driver reports 0 ms, so a headless run never passes.
	var real_driver: bool = DisplayServer.get_name() != "headless"
	return {
		"real_driver": real_driver,
		"mix_rate_hz": AudioServer.get_mix_rate(),
		"output_latency_ms": AudioServer.get_output_latency() * 1000.0,
		"time_to_next_mix_ms": AudioServer.get_time_to_next_mix() * 1000.0,
		"setting_output_latency_ms": ProjectSettings.get_setting("audio/driver/output_latency", 15),
		"samples": _gaps.size(),
		"worst_event_to_play_frames": worst_gap,
		"pass": real_driver and _gaps.size() > 0 and worst_gap == 0
				and AudioServer.get_output_latency() <= 0.030,
	}
```

## 5. Measure press to ear (flash and click)

The engine cannot see the driver and the device. Measure them from outside:

1. Build a test scene that shows a white `ColorRect` and plays a click on
   the same frame, once per second.
2. Film the screen and the speaker with a phone in slow motion (240 fps
   gives 4.2 ms per frame), or record the screen and the audio output with a
   capture tool that keeps them in sync.
3. Count the frames from the first white frame to the first click in the
   audio track. Subtract the display latency if you know it.
4. Repeat 10 times. Report the median and the worst value.

```gdscript
extends Node2D

## Flash-and-click test: a white flash and a click start on the same frame.
@export var click: AudioStream
@export var period_s: float = 1.0
@export var flash_s: float = 0.05

var _flash: ColorRect
var _player: AudioStreamPlayer
var _elapsed: float = 0.0


func _ready() -> void:
	_flash = ColorRect.new()
	_flash.color = Color.WHITE
	_flash.size = get_viewport_rect().size
	_flash.visible = false
	add_child(_flash)
	_player = AudioStreamPlayer.new()
	_player.stream = click
	add_child(_player)


func _process(delta: float) -> void:
	_elapsed += delta
	if _elapsed >= period_s:
		_elapsed -= period_s
		_flash.visible = true
		_player.play()
	elif _elapsed >= flash_s:
		_flash.visible = false
```

Use a click with a 1 ms attack and no leading silence, so the asset adds no
delay to the measurement.
