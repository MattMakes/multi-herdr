Adds mix control beyond bus volumes: sidechain ducking of music under dialogue, a voice pool with priorities and stealing, caps on identical sounds, and built-in variation with AudioStreamRandomizer and AudioStreamPolyphonic; read it when many sounds compete, dialogue gets lost under music, or repeated sounds clip or sound mechanical.

# Mixing: Voices, Caps and Ducking

> ← Back to [SKILL.md](../SKILL.md). Bus layout and slider math are in SKILL.md section 3; the basic pool is in [sfx-pooling.md](sfx-pooling.md).

All code targets Godot 4.7.

---

## 1. Bus Roles

Route every player to a category bus, and keep `Master` for the final
limiter only:

```
Master        (Limiter only)
├── Music     (Compressor with sidechain = Voice, see section 2)
├── SFX
├── UI
└── Voice
```

A category bus lets the settings menu, ducking and mute work per category. A
sound played straight on `Master` escapes all of them.

## 2. Duck Music Under Dialogue with a Sidechain

An `AudioEffectCompressor` on the Music bus with `sidechain` set to the Voice
bus lowers the music while the Voice bus has signal, and lets it back up when
the line ends. No script has to fade volumes, and overlapping lines work.

| Property | Start value | Meaning |
|---|---|---|
| `sidechain` | `&"Voice"` | The bus whose level drives the compressor |
| `threshold` | -30 dB | Voice level at which ducking starts |
| `ratio` | 8 | How hard the music is pushed down |
| `attack_us` | 20000 (20 ms) | How fast ducking starts |
| `release_ms` | 400 | How fast the music comes back |

```gdscript
extends Node

## Adds a sidechain compressor to the Music bus at startup.
## Prefer setting this up once in the bus layout (default_bus_layout.tres).
func _ready() -> void:
	var music := AudioServer.get_bus_index(&"Music")
	if music < 0 or AudioServer.get_bus_index(&"Voice") < 0:
		push_error("Ducking: buses 'Music' and 'Voice' must exist")
		return
	var duck := AudioEffectCompressor.new()
	duck.sidechain = &"Voice"
	duck.threshold = -30.0
	duck.ratio = 8.0
	duck.attack_us = 20000.0
	duck.release_ms = 400.0
	AudioServer.add_bus_effect(music, duck)
```

Tune by ear with a real dialogue line over the loudest music track.

## 3. A Voice Pool with Priorities

A fixed pool of players avoids per-sound node churn. When the pool is full,
something must give. Let each sound carry a priority, and steal the oldest
voice with the lowest priority, but never one with a higher priority than the
new sound. A boss roar then never cuts a quest line, and the 30th footstep
takes the place of the oldest footstep.

```gdscript
extends Node

## Priority voice pool. Add as an autoload named "Sfx".
@export var voice_count: int = 24
@export var bus: StringName = &"SFX"

var _voices: Array[AudioStreamPlayer] = []
var _priority: PackedInt32Array = PackedInt32Array()
var _started_ms: PackedInt64Array = PackedInt64Array()


func _ready() -> void:
	_priority.resize(voice_count)
	_started_ms.resize(voice_count)
	for i in voice_count:
		var p := AudioStreamPlayer.new()
		p.bus = bus
		add_child(p)
		_voices.append(p)


## Plays `stream`. Returns false when every voice holds a higher priority.
func play(stream: AudioStream, priority: int = 0, volume_db: float = 0.0) -> bool:
	var slot := _pick_slot(priority)
	if slot < 0:
		return false
	var p := _voices[slot]
	p.stream = stream
	p.volume_db = volume_db
	p.play()
	_priority[slot] = priority
	_started_ms[slot] = Time.get_ticks_msec()
	return true


func _pick_slot(priority: int) -> int:
	var best := -1
	for i in _voices.size():
		if not _voices[i].playing:
			return i
		if _priority[i] > priority:
			continue
		if best < 0 or _priority[i] < _priority[best] \
				or (_priority[i] == _priority[best] and _started_ms[i] < _started_ms[best]):
			best = i
	return best
```

## 4. Cap Identical Sounds

Fifty explosions in one frame sum into a clipped wall of noise. Limit how many
copies of one sound may play at once:

- **One player per sound:** set `max_polyphony` on that player. When it plays
  again past the cap, the oldest instance stops. This is the simplest cap.
- **Shared pool:** count active voices per stream before you call `play()`.

```gdscript
extends Node

## Per-stream concurrency cap in front of a voice pool.
@export var default_cap: int = 4
var caps: Dictionary[AudioStream, int] = {}
var _active: Dictionary[AudioStream, int] = {}


func try_start(stream: AudioStream) -> bool:
	var cap: int = caps.get(stream, default_cap)
	var count: int = _active.get(stream, 0)
	if count >= cap:
		return false
	_active[stream] = count + 1
	return true


## Call when a voice that played `stream` finishes or is stolen.
func release(stream: AudioStream) -> void:
	var count: int = _active.get(stream, 0)
	if count <= 1:
		_active.erase(stream)
	else:
		_active[stream] = count - 1
```

## 5. Variation Without Code: AudioStreamRandomizer

The same footstep sample played 200 times sounds mechanical, and copies that
start together phase into a louder, hollow sound. An `AudioStreamRandomizer`
holds several streams and picks one per play, with random pitch and volume.

```gdscript
extends AudioStreamPlayer3D

## Builds a footstep randomizer from a few takes.
@export var takes: Array[AudioStream] = []


func _ready() -> void:
	var r := AudioStreamRandomizer.new()
	for i in takes.size():
		r.add_stream(i, takes[i])
	r.playback_mode = AudioStreamRandomizer.PLAYBACK_RANDOM_NO_REPEATS
	r.random_pitch = 1.08
	r.random_volume_offset_db = 2.0
	stream = r
	max_polyphony = 3
```

`random_pitch` is a scale: 1.08 picks a pitch between 1/1.08 and 1.08. It
can also be set in the inspector on a `.tres`, which is the usual workflow.

## 6. Many One-Shots from One Node: AudioStreamPolyphonic

When a single source (a gun, a UI panel) plays many short sounds, an
`AudioStreamPolyphonic` on one player can be cheaper than a node per voice.
Get its playback object and call `play_stream()` for each sound.

```gdscript
extends AudioStreamPlayer

var _poly: AudioStreamPlaybackPolyphonic


func _ready() -> void:
	var s := AudioStreamPolyphonic.new()
	s.polyphony = 16
	stream = s
	play()
	_poly = get_stream_playback() as AudioStreamPlaybackPolyphonic


func play_one(sfx: AudioStream, volume_db: float = 0.0, pitch: float = 1.0) -> void:
	if _poly:
		_poly.play_stream(sfx, 0.0, volume_db, pitch)
```

All sounds share the node's position and bus, so this suits UI and
sources that do not move apart.
