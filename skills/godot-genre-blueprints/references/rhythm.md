# Rhythm

The player acts on the beat of a song and is judged by timing. Guitar Hero,
osu!, Beat Saber and Crypt of the NecroDancer are the reference points.

## Core loop

Calibrate latency → the song plays → notes approach → the player hits →
the game judges the timing → combo and score → results → next song or a
harder chart.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Song clock | position from the audio output, not from frames | this reference |
| Input timing | read the time in the input callback | `godot-input-handling` |
| Charts | note times, lanes, holds, tempo map; loaded from files | `godot-resource-pattern`, `godot-save-load` |
| Note highway | pooled note nodes, or a scrolling shader | `godot-shader-basics`, `godot-optimization` |
| Judgement and score | timing windows, combo, multiplier | this reference |
| Calibration | a tap test that measures the player's offset | `godot-audio-system` |
| Hit feedback | flashes, particles, hit sounds | `godot-particles-vfx`, `godot-tween-animation` |
| Song select and results | lists, grades | `godot-ui` |

## Scene tree (4.7)

```text
Song (Node)
├── Conductor (Node; owns the AudioStreamPlayer, gives song time)
│   └── Music (AudioStreamPlayer)
├── Chart (Node; note list, spawns from a pool)
├── Highway (Node2D or Node3D)
│   ├── Lanes (one receptor per lane)
│   └── NotePool (pre-made note nodes, hidden when idle)
├── Judge (Node; windows, combo, score)
└── HUD (CanvasLayer; combo, score, judgement text)
```

## Genre code

The audio thread plays sound in chunks, so the playback position jumps.
Add the time since the last mix and remove the output latency, then the
player's calibration offset.

```gdscript
extends Node

@export var music: AudioStreamPlayer
@export var bpm := 120.0
var user_offset := 0.0   # seconds, from the calibration screen
var _latency := 0.0

func start() -> void:
	# The docs warn this call can be expensive: read it once per song.
	_latency = AudioServer.get_output_latency()
	music.play()

func song_time() -> float:
	if not music.playing:
		return music.get_playback_position()
	return music.get_playback_position() \
		+ AudioServer.get_time_since_last_mix() \
		- _latency \
		- user_offset

func beat() -> float:
	return song_time() * bpm / 60.0

const WINDOWS := {&"perfect": 0.035, &"great": 0.07, &"good": 0.11}

func judge(note_time: float, hit_time: float) -> StringName:
	var err := absf(hit_time - note_time)
	for grade in WINDOWS:
		if err <= WINDOWS[grade]:
			return grade
	return &"miss"
```

Read `song_time()` inside `_input()` or `_unhandled_input()` when the press
arrives, and judge against the nearest unjudged note in that lane. Place
each note on screen from `note_time - song_time()`, never by adding
`delta` to a position.

## Pitfalls

- `Time.get_ticks_msec()` or summed `delta` as the song clock. It drifts
  from the audio. Use the formula above.
- `AudioServer.get_output_latency()` every frame. The class docs say it can
  be expensive; read it once when the song starts.
- Judging inputs in `_process()`. A frame later is tens of milliseconds
  later. Judge in the input callback.
- No calibration. Bluetooth headphones can add 100 ms or more; offer a tap test
  and an offset setting.
- `Engine.time_scale` for a song speed mod. It changes physics too; change
  `pitch_scale` on the player and scale the chart times instead.
- One constant BPM. Many songs change tempo. Store a tempo map: a list of
  (time, bpm) segments.
- Notes created and freed per beat. Pool them.
- Narrow windows for everyone. Offer wider windows on easy levels.
- Mashing that never breaks a combo. A press with no note in range counts
  as a miss, or breaks the combo.
- The highway keeps moving on pause. Pause the conductor and the chart with
  the music.
- Hit sounds at one pitch sound mechanical. Vary pitch by a few percent.
- Spectrum visuals by FFT in GDScript. Use an `AudioEffectSpectrumAnalyzer`
  on a bus and read it with `AudioServer.get_bus_effect_instance()`.
