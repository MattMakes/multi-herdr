# Audio, advanced

Target engine: Godot 4.7. This shelf covers the mixer internals, stream types, adaptive music,
audio-accurate timing, procedural and captured audio, 3D audio and the Web sample path.

## Use this shelf when

- You design a bus layout, a ducking scheme or a per-room reverb setup.
- You pick a format or a stream type for many voices, long music or adaptive music.
- Gameplay must line up with the beat (rhythm game, beat-synced VFX).
- You generate, analyse or capture audio at runtime (synth, visualizer, voice chat, mic).
- A Web export has missing effects, late sound or crackle.

## How the engine works here

- A separate audio thread mixes in fixed chunks. `play()` does not start a sound at once: it
  starts at the next mix, and that mix reaches the speakers after the driver latency. The two
  delays are `AudioServer.get_time_to_next_mix()` and `AudioServer.get_output_latency()`.
- `audio/driver/output_latency` (default 15 ms, 50 on Web, ignored on Android) is a request.
  The OS may give another value. Lower values cost CPU and risk crackle.
- Buses form a chain that only sends "left" (to a lower index), so loops cannot exist. Players
  hold the bus by name. A renamed or missing bus falls back to `Master` without an error.
- A bus that stays under `audio/buses/channel_disable_threshold_db` (-60 dB) for
  `audio/buses/channel_disable_time` (2 s) is switched off with all its effects, so idle
  effect buses are free.
- Each `play()` creates an `AudioStreamPlayback`. The stream is shared data; the playback is
  the voice. `max_polyphony` on a player (default 1) is how many playbacks it keeps; one more
  `play()` cuts the oldest.
- Decode cost by format (docs): WAV (PCM, IMA ADPCM, or Quite OK Audio, the default import
  compression) is cheap enough for hundreds of voices. Ogg Vorbis is the smallest and the most
  expensive to decode. MP3 is larger than Ogg at equal quality and cheaper to decode.
- Two playback types exist (experimental API). `PLAYBACK_TYPE_STREAM` uses the engine mixer.
  `PLAYBACK_TYPE_SAMPLE` hands the sound to the platform (Web Audio). Samples skip the engine
  mixer, so bus effects, reverb buses, doppler and procedural streams do not work on them.
- `AudioStreamPlayer2D.play()` and `AudioStreamPlayer3D.play()` queue the sound for the next
  physics frame. Only the non-positional player starts at the next mix.

## Advanced techniques

### Sidechain ducking with a compressor
Put an `AudioEffectCompressor` on the Music bus and set `sidechain` to the Voice bus name. The
music level then follows the voice level with `attack_us` and `release_ms` smoothing, with no
script and no tween. It pays for dialogue and voice chat. Cost: one effect on one bus. Trap:
the sidechain bus must exist under that exact name.

### Master safety chain
End the Master bus with `AudioEffectHardLimiter` (look-ahead, `ceiling_db` -0.3 by default).
A compressor before it makes the limiting less audible. `AudioEffectLimiter` is the old
effect, kept for compatibility. On phones, an EQ that cuts lows the speaker cannot play stops
the limiter from pulling the mix down; toggle it with `AudioServer.set_bus_effect_enabled()`.

### One player, many voices: AudioStreamPolyphonic
Assign an `AudioStreamPolyphonic` (`polyphony` default 32) to one player, call `play()` once,
then start sounds through the playback. It replaces a pool of player nodes for non-positional
SFX; each voice gets its own volume, pitch and bus.

```gdscript
extends AudioStreamPlayer

var _voices: AudioStreamPlaybackPolyphonic

func _ready() -> void:
	stream = AudioStreamPolyphonic.new()
	play()
	_voices = get_stream_playback() as AudioStreamPlaybackPolyphonic

func play_sfx(sfx: AudioStream, volume_db: float = 0.0, pitch: float = 1.0) -> int:
	var id: int = _voices.play_stream(sfx, 0.0, volume_db, pitch)
	return id  # AudioStreamPlaybackPolyphonic.INVALID_ID when no voice is free
```

Trap: the playback is only valid while the player is playing. If the player stops, every id
is dead. Check `is_stream_playing(id)` before `set_stream_volume()` or `stop_stream()`.

### Variation without code: AudioStreamRandomizer
Wrap takes in an `AudioStreamRandomizer`: weighted pick (`PLAYBACK_RANDOM_NO_REPEATS` is the
default), `random_pitch` and `random_volume_offset_db`. It removes the "machine gun" effect of
a repeated sample, and it nests inside polyphonic and interactive streams.

### Adaptive music: horizontal, vertical, sequential
Three meta streams (`is_meta_stream()` is true) cover the usual adaptive patterns:

| Stream | Pattern | Control |
|---|---|---|
| `AudioStreamInteractive` | horizontal re-sequencing: clips plus a transition table | `switch_to_clip_by_name()` on the playback |
| `AudioStreamSynchronized` | vertical layers, started together (max 32) | `set_sync_stream_volume(index, db)` |
| `AudioStreamPlaylist` | ordered or shuffled sequence with `fade_time` (max 64) | properties |

A transition (`add_transition()`) says when to leave the current clip
(`TRANSITION_FROM_TIME_IMMEDIATE`, `_NEXT_BEAT`, `_NEXT_BAR`, `_END`), where to enter the next
(`TRANSITION_TO_TIME_START`, `_SAME_POSITION`, `_PREVIOUS_POSITION`), the fade mode and
`fade_beats`, an optional filler clip (a stinger or bridge), and `hold_previous` so a
temporary clip (a victory sting) returns to what played before via
`AUTO_ADVANCE_RETURN_TO_HOLD`. `CLIP_ANY` writes one rule for every source or target.
Beat and bar boundaries come from the import metadata: set BPM, Beat Count and Bar Beats in
the advanced import dialog of the Ogg or MP3 file (`bpm`, `beat_count`, `bar_beats`).
Streams nest, so a clip may be a
synchronized stream with its own layers. Trap: `_SAME_POSITION` needs clips of equal length.
Trap: layer volumes live on the resource, so all players that share it change together.

### Beat-accurate timing
- System clock (short songs, most precise): store `Time.get_ticks_usec()` when you call
  `play()`, and subtract `get_time_to_next_mix() + get_output_latency()` once. The sound card
  clock and the system clock drift apart over time.
- Audio clock (any length): `get_playback_position() + AudioServer.get_time_since_last_mix()
  - latency`. `get_playback_position()` alone moves in mix-chunk steps. The result can jitter
  backward by a little; keep the maximum seen so far.

`get_output_latency()` can be expensive: cache it, do not call it every frame. Add a player
calibration offset on top: the engine does not know the display lag (TV processing, GPU queue).

### Procedural audio with AudioStreamGenerator
The script fills a ring buffer; the mixer drains it. Fill `get_frames_available()` frames per
`_process`. `buffer_length` (default 0.5 s) is both the latency and the safety margin: it must
cover the longest frame hitch, or `get_skips()` rises and the sound cracks. In GDScript
`push_frame()` in a loop can beat `push_buffer()`; in C# and native code `push_buffer()` wins
(class reference). `mix_rate_mode` (since 4.4) can follow the output or input device rate
instead of a custom `mix_rate`. For anything heavier than a few
oscillators, render once into an `AudioStreamWAV` (`data`, `format`, `mix_rate`) at load time
and play that as a cheap PCM voice.

### Capture, analysis and microphone
- `AudioEffectCapture` copies a bus into a ring buffer (`buffer_length` 0.1 s). Drain it with
  `get_buffer()` every frame; `get_discarded_frames()` tells you that you were too slow. Use
  it for voice chat and oscilloscopes.
- `AudioEffectSpectrumAnalyzer` only configures. Query the instance:
  `AudioServer.get_bus_effect_instance(bus, idx)` cast to `AudioEffectSpectrumAnalyzerInstance`
  then `get_magnitude_for_frequency_range(from_hz, to_hz)`. It returns linear energy per
  channel; convert with `linear_to_db()`. Larger `fft_size` is steadier and later.
- Microphone: set `audio/driver/enable_input`, play an `AudioStreamMicrophone` on a bus that
  holds a capture or `AudioEffectRecord` effect, and mute that bus to avoid feedback. iOS also
  needs `audio/general/ios/session_category` set to a record category. A blocked microphone
  returns silence, not an error. Since 4.6 `AudioServer` also has an experimental direct
  path (`set_input_device_active()`, `get_input_frames_available()`, `get_input_frames()`)
  that needs no player or bus: the cheapest route for voice chat capture.

### 3D audio that reads the level
- `attenuation_model` picks inverse, inverse square, logarithmic or disabled. `unit_size`
  scales the falloff, `max_distance` is a hard cut, `max_db` caps loudness near the listener.
- A distance low-pass is on by default (`attenuation_filter_cutoff_hz` 5000); 20500 disables it.
- `Area3D` reverb: `reverb_bus_enabled`, `reverb_bus_name`, `reverb_bus_amount`,
  `reverb_bus_uniformity` send a wet copy of sounds inside the area to a bus that holds an
  `AudioEffectReverb`. `audio_bus_override` on `Area2D`/`Area3D` reroutes the dry sound
  (an "underwater" bus with a low-pass).
- Doppler needs `doppler_tracking` set on the player and on the `Camera3D` or
  `AudioListener3D`.

### Web: plan for the sample path
On Web the default is `PLAYBACK_TYPE_SAMPLE` (`audio/general/default_playback_type.web`).
Why it exists (Web export in 4.3 report): without threads the engine mixer runs on the main
loop, so mixing is tied to the frame rate. When frames drop, too few audio frames reach the
buffer and the sound garbles. A sample is handed to the browser instead: the engine skips
it in the mix phase and only tells the Web driver "play, pause, stop", and the driver keeps
a Web Audio node chain per sample and per bus. The sound then survives frame drops. Design
the Web mix so it does not depend on bus effects, or set single players to
`PLAYBACK_TYPE_STREAM` and accept more latency (less with thread support). A stream is
registered as a sample on first play, which can spike on low-end devices: call
`AudioServer.register_stream_as_sample()` while the level loads. Only static streams (WAV,
Ogg Vorbis, MP3) can be sampled (`AudioStream.can_be_sampled()`).

## Niche but useful

- `AudioServer.playback_speed_scale` slows or speeds all audio, independent of
  `Engine.time_scale`. Use it for slow motion that should also pitch the mix.
- `AudioStreamWAV.load_from_file()` and `AudioStreamMP3.load_from_file()` (both since 4.4)
  and `AudioStreamOggVorbis.load_from_file()` load user music or mod audio without import.
- `AudioStreamPlayback.mix_audio()` (since 4.4) pulls mixed frames from a playback by hand,
  for offline analysis such as beat or loudness detection at load time.
- WAV loop points are in samples (`loop_begin`, `loop_end`) and support ping-pong and backward.
  Ogg and MP3 only have `loop_offset` in seconds and forward looping.
- Text to speech lives on `DisplayServer` (`tts_speak()`, `tts_get_voices_for_language()`,
  `tts_set_utterance_callback()`).

## Traps

- The `finished` signal never fires for a looping stream.
- TTS: `tts_get_voices()` is empty on Linux without speech-dispatcher; `tts_speak()` and
  `tts_stop()` have a platform-dependent delay; enabling `audio/general/text_to_speech` at
  startup costs idle CPU.
- Dozens of concurrent Ogg voices cost real CPU on mobile and Web. Use WAV there for short
  effects and MP3 for long ambiences (docs advice).
- Outdated belief: "24-bit or 96 kHz assets sound better". The docs state no audible gain
  above 16-bit and 48 kHz; use Force Mono and Max Rate on WAV imports.

## Version notes

- 4.3: sample playback type, `AudioStreamInteractive`, `AudioStreamSynchronized`,
  `AudioStreamPlaylist` (present in the 4.3 class reference).
- 4.4: `AudioStreamGenerator.mix_rate_mode`; runtime WAV and MP3 loading; `mix_audio()`
  exposed; `volume_linear` on the players; Quite OK Audio became the default WAV import
  compression (4.4 dev 2 notes), so re-imported WAV files changed format.
- 4.5: `AudioListener3D` tracks velocity for doppler (fix in 4.5 beta 5); before that only
  the camera did.
- 4.6: direct microphone buffer methods on `AudioServer`.
- 4.7: `AudioEffectSpectrumAnalyzer.tap_back_pos` was removed together with a fix for
  jittered analyzer values; the audio bus panel was reworked (editor only).
- 4.7: default `area_mask` of `AudioStreamPlayer2D`/`AudioStreamPlayer3D` changed from 1 to 0.
  Projects that rely on `audio_bus_override` or reverb areas must set the mask layer again.

## Related skills

- `godot-audio-system` - bus setup, music manager, SFX pool and settings code; start there.
- `godot-sound-design` - loudness targets, latency budget, offline synthesis of effects.
- `godot-game-audio` - measuring the played mix and loop seams.

## Sources

- https://docs.godotengine.org/en/stable/tutorials/audio/audio_buses.html
- https://docs.godotengine.org/en/stable/tutorials/audio/audio_effects.html
- https://docs.godotengine.org/en/stable/tutorials/audio/audio_streams.html
- https://docs.godotengine.org/en/stable/tutorials/audio/sync_with_audio.html
- https://docs.godotengine.org/en/stable/tutorials/audio/recording_with_microphone.html
- https://docs.godotengine.org/en/stable/tutorials/audio/text_to_speech.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/importing_audio_samples.html
- https://docs.godotengine.org/en/stable/tutorials/export/exporting_for_web.html
- https://docs.godotengine.org/en/stable/tutorials/migrating/upgrading_to_godot_4.7.html
- https://godotengine.org/article/progress-report-web-export-in-4-3/
- https://godotengine.org/releases/4.4/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-2/
- https://godotengine.org/article/dev-snapshot-godot-4-4-dev-4/
- https://godotengine.org/article/dev-snapshot-godot-4-5-beta-5/
- https://godotengine.org/article/dev-snapshot-godot-4-6-dev-6/
- https://godotengine.org/article/dev-snapshot-godot-4-7-beta-1/
