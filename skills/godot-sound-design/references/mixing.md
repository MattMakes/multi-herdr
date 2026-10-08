# Mixing: buses, levels, slots, voices and ducking

> Back to [SKILL.md](../SKILL.md). The engine side (bus API, a priority voice
> pool with stealing, per-stream caps) is in `godot-audio-system`, file
> `references/mixing-voices-and-ducking.md`. This file sets the numbers.

All numbers here are tagged **practice** or **standard** (see
[research.md](research.md) section 5). Tune them by measurement and by ear.

## 1. Bus layout

```
Master    HardLimiter, ceiling -1 dB
├── Music     Compressor, sidechain = Reward   (optional)
├── Ambient   Compressor, sidechain = Reward; LowPass
├── Engine    Compressor, sidechain = Reward
├── SFX       HighPass 120 Hz
│   └── Reward    (sends to SFX)
└── UI        HighPass 300 Hz
```

- **Reward** holds the reward sounds only. Its signal drives the ducking of
  the loops, so a reward cuts through without a louder reward sound.
- **Engine** holds continuous player loops (an engine, a motor, a beam).
- **Ambient** holds the world bed.
- **UI** holds menu and HUD sounds. Settings sliders map to Master, Music,
  SFX (with Reward and Engine under it or beside it) and Ambient.

The script below builds this layout in code. Use it in a test, or call it
once at startup if the project has no `default_bus_layout.tres`. A layout
file is the usual place; the code shows the exact settings.

```gdscript
class_name SoundBuses
extends RefCounted

## Builds the bus layout of this skill. A bus that exists keeps its effects,
## so a second call changes nothing.

static func ensure_layout() -> void:
	var master: int = AudioServer.get_bus_index(&"Master")
	if AudioServer.get_bus_effect_count(master) == 0:
		var limiter := AudioEffectHardLimiter.new()
		limiter.ceiling_db = -1.0
		AudioServer.add_bus_effect(master, limiter)
	if _add_bus(&"SFX", &"Master"):
		_add_high_pass(&"SFX", 120.0)
	_add_bus(&"Reward", &"SFX")
	if _add_bus(&"UI", &"Master"):
		_add_high_pass(&"UI", 300.0)
	if _add_bus(&"Engine", &"Master"):
		_add_duck(&"Engine", 4.0)
	if _add_bus(&"Ambient", &"Master"):
		_add_duck(&"Ambient", 6.0)


## Adds bus_name with its send. Returns false when the bus exists already.
static func _add_bus(bus_name: StringName, send: StringName) -> bool:
	if AudioServer.get_bus_index(bus_name) >= 0:
		return false
	AudioServer.add_bus()
	var index: int = AudioServer.bus_count - 1
	AudioServer.set_bus_name(index, bus_name)
	AudioServer.set_bus_send(index, send)
	return true


static func _add_high_pass(bus_name: StringName, cutoff_hz: float) -> void:
	var index: int = AudioServer.get_bus_index(bus_name)
	var high_pass := AudioEffectHighPassFilter.new()
	high_pass.cutoff_hz = cutoff_hz
	AudioServer.add_bus_effect(index, high_pass)


## Ducks bus_name by about duck_db while the Reward bus plays.
static func _add_duck(bus_name: StringName, duck_db: float) -> void:
	var index: int = AudioServer.get_bus_index(bus_name)
	var duck := AudioEffectCompressor.new()
	duck.sidechain = &"Reward"
	duck.threshold = -30.0
	duck.ratio = clampf(duck_db, 2.0, 8.0)
	duck.attack_us = 10000.0
	duck.release_ms = 250.0
	AudioServer.add_bus_effect(index, duck)
```

The compressor ratio does not map to a fixed number of decibels: the duck
depth depends on how far the reward goes over the threshold. Measure the
depth (section 6) and change `threshold` until the loops drop 3 to 6 dB under
a typical reward.

## 2. Loudness targets

**Session target (standard, industry).** Measure the game's master output
over a 10-minute play session:

| Platform | Integrated loudness | True peak |
| --- | --- | --- |
| PC or console on a TV or speakers | -24 LUFS, +/- 2 | at most -1 dBTP |
| Handheld (Steam Deck, Switch handheld), mobile, laptop speakers | -18 LUFS, +/- 2 | at most -1 dBTP |

Source: Sony ASWG-R001; BS.1770 meter. A game for both picks 1 target and
gives a "dynamic range" setting, or targets -18 LUFS with gentle master
compression.

**Group targets (practice).** Momentary loudness of each file at player
volume 0 dB, measured with `sfx_report.py` ([synthesis.md](synthesis.md)).
These set the relative balance; the bus faders set the absolute level.

| Group | Momentary LUFS | Note |
| --- | --- | --- |
| Rare reward (top tier) | -18 | The loudest planned feedback |
| Reward (lower tiers) | -21 to -19 | 1 dB per tier, at most 3 dB between tiers |
| Impact, hit taken | -20 | Clear, not startling |
| Bad news (loss, break) | -24 | Quieter than every reward |
| Small gain, pickup | -22 | |
| Action (2 to 8 per second) | -24 to -26 | |
| Very frequent action (more than 8 per second) | -28 or lower | Its sum must not cover the rewards |
| UI click | -26 | |
| Engine loop | -24 at full throttle | Lower with the speed |
| Ambient bed | -32 | Felt more than heard |

Spread: keep the planned feedback inside about 10 LU (loudness units), from
-28 to -18. A wider spread makes the player turn the volume up for quiet
sounds and then get hit by loud ones.

## 3. Frequency slots (EQ)

2 sounds in the same frequency range at the same time mask each other: the
louder one hides the quieter one. Give each group a slot:

| Group | Main energy | Filter |
| --- | --- | --- |
| Engine loop | 50 to 300 Hz | Low-pass at about 2 kHz, so it leaves the top free |
| Ambient bed | 200 Hz to 2 kHz, soft | Low-pass at 2 to 4 kHz; high-pass at 150 Hz |
| Actions, cuts | 300 Hz to 6 kHz, noise-based | High-pass at 120 Hz on the SFX bus |
| Impacts | 60 to 400 Hz thump plus a 2 to 4 kHz click | The only feedback that owns the low end |
| Rewards | 500 Hz to 5 kHz, tonal | Bright top; a different timbre from actions |
| UI | 1 to 6 kHz, short | High-pass at 300 Hz |

Check a slot with `AudioEffectSpectrumAnalyzer` on the bus, or with a
spectrum view in an audio editor on a recording (section 6).

## 4. Voices and priorities

| Item | Value (practice) |
| --- | --- |
| SFX voice pool | 16 to 32 voices |
| Cap per sound | 2 to 4 voices; 1 for rare rewards and milestones |
| Cooldown for very frequent sounds | 50 to 80 ms, or a cap of 2 |
| Priority, high to low | danger cue, reward, hit taken, action, frequent action, UI |

When the pool is full, steal the oldest voice with the lowest priority. Never
steal a voice of higher priority than the new sound. Code:
`godot-audio-system`, `references/mixing-voices-and-ducking.md` section 3.

## 5. Ducking

| Trigger | Ducked buses | Depth | Attack | Release |
| --- | --- | --- | --- | --- |
| Reward bus | Engine, Ambient, Music | 3 to 6 dB | 5 to 20 ms | 150 to 300 ms |
| Voice or dialogue bus (if any) | Music, Ambient | 6 to 10 dB | 20 ms | 400 ms |

A duck that is too slow lets the first transient of the reward through
unducked; that is fine, because the transient carries the timing. A duck
that releases too fast "pumps" (the loop audibly jumps back).

## 6. Record the mix and measure it

Put an `AudioEffectRecord` on Master, play a scripted session in a windowed
build, and save the result. Then measure it with `sfx_report.py` or a BS.1770
meter.

```gdscript
extends Node

## Records the Master bus to a WAV file for loudness and spectrum checks.
@export var output_path: String = "user://mix_capture.wav"

var _record: AudioEffectRecord


func start_capture() -> void:
	var master: int = AudioServer.get_bus_index(&"Master")
	_record = AudioEffectRecord.new()
	AudioServer.add_bus_effect(master, _record)
	_record.set_recording_active(true)


func stop_capture() -> Error:
	if _record == null:
		return ERR_UNCONFIGURED
	_record.set_recording_active(false)
	var recording: AudioStreamWAV = _record.get_recording()
	if recording == null:
		return ERR_CANT_CREATE
	return recording.save_to_wav(output_path)
```

`sfx_report.py` reads mono and stereo 16-bit files. For an integrated
session number, use a full BS.1770 meter with gating; the kit reports the
maximum momentary loudness, which is higher than the integrated value.
