# Review checklist: audit the sound of a game

> Back to [SKILL.md](../SKILL.md).

An agent runs this review on a Godot game. Steps 1 to 4 need no ears: they
read code, measure files and run the game headless or windowed. Step 5 needs
a person who listens. Report each check as a row: check, value, pass mark,
result. Do not mark a check "pass" that you did not run; write "not run" and
the reason.

## 1. Inventory (code)

1. List the event-bus signals. List the sound files. List the map rows (or
   the handlers that play sounds).
2. Mark each signal: player action, reward, bad news, world, UI, or silent.

| Check | Pass mark | Basis |
| --- | --- | --- |
| Every player action that changes the world has a sound | 100 % | research: Haehn et al. 2024 |
| Every reward event has a reward sound | 100 % | practice |
| Gameplay code never plays audio directly | 0 `play()` calls outside the sound director | practice |
| Every player has a category bus, not `Master` | 100 % | practice |
| The contract test of [event-sound-map.md](event-sound-map.md) section 5 passes | exit 0 | practice |

## 2. Assets (measure)

Run `sfx_report.py` ([synthesis.md](synthesis.md) section 4) on every file.

| Check | Pass mark | Basis |
| --- | --- | --- |
| `leading_silence_ms` (feedback sounds) | at most 1 ms | research, indirect: Reynolds and Day 2007 |
| `onset_ms` (feedback sounds) | at most 5 ms | research, indirect |
| `peak_dbfs` | at most -1 dBFS | standard |
| `end_dbfs` (one-shot sounds) | at most -50 dBFS (no click at the end) | practice |
| `loop_seam_ratio` (loops) | at most 1.0 | practice |
| `loudness_lufs` per group | within 2 LU of the group target in [mixing.md](mixing.md) section 2 | practice |
| Reward tiers | loudness and length rise with the tier; at most 3 dB between tiers | research: Dixon et al. 2014 (honest proportion) |
| Bad-news sounds | quieter than the smallest reward | practice |
| `audible_ms` | inside the row for the event rate in [feedback-design.md](feedback-design.md) section 2 | practice |
| Same output from 2 runs of the synthesis script | byte-identical | practice |

## 3. Code rules (read)

| Check | Pass mark | Basis |
| --- | --- | --- |
| The sound plays in the event handler: no `call_deferred`, `await`, `Timer` or next-frame queue before `play()` | 0 cases | research: Reynolds and Day 2007 |
| No `AudioEffectPitchShift` on a feedback bus | 0 | practice (adds delay) |
| A reward sound plays only when the net gain is above 0 | 0 reward sounds on a net loss or zero | research: Dixon et al. 2014 |
| The reward tier comes from the net gain, not the gross payout | yes | research: Dixon et al. 2014 |
| A cue that carries a value (reward tier, combo step) has no random pitch | 0 cases | practice |
| A cue that plays more than once per second has pitch or variant variation | 100 % | practice |
| A cue that plays more than 8 times per second has a cooldown or a cap of at most 3 | 100 % | practice |
| Short feedback sounds are WAV, not Ogg or MP3 | 100 % | practice |
| `audio/driver/output_latency` | at most 15 on desktop | practice |

## 4. Runtime (run)

**Event rates, headless.** Run a scripted play session (an autoplay or a
bot) with this counter attached to the event bus. It gives the rate of each
event, which picks the length row in [feedback-design.md](feedback-design.md).

```gdscript
class_name EventRateCounter
extends Node

## Counts emits per event-bus signal. Attach it, run a session, read rates().
var _counts: Dictionary[StringName, int] = {}
var _start_ms: int = 0


func watch(bus: Object) -> void:
	_start_ms = Time.get_ticks_msec()
	var own_signals: Array[Dictionary] = (bus.get_script() as Script).get_script_signal_list()
	for info: Dictionary in own_signals:
		var signal_name: StringName = info["name"]
		var arg_count: int = (info["args"] as Array).size()
		_counts[signal_name] = 0
		var handler: Callable = _count.bind(signal_name)
		# unbind(0) is an error, so drop arguments only when the signal has some.
		bus.connect(signal_name, handler.unbind(arg_count) if arg_count > 0 else handler)


## Mean events per second over the session, per signal.
func rates() -> Dictionary[StringName, float]:
	var seconds: float = maxf(0.001, (Time.get_ticks_msec() - _start_ms) / 1000.0)
	var out: Dictionary[StringName, float] = {}
	for signal_name: StringName in _counts:
		out[signal_name] = _counts[signal_name] / seconds
	return out


func _count(signal_name: StringName) -> void:
	_counts[signal_name] += 1
```

**Windowed checks** (the headless audio driver mixes nothing real):

| Check | Pass mark | Tool |
| --- | --- | --- |
| Event frame to `play()` frame | 0 frames, worst case | `LatencyProbe`, [latency.md](latency.md) section 4 |
| `AudioServer.get_output_latency()` | at most 30 ms on desktop | `LatencyProbe` |
| Press to ear (flash and click) | at most 50 ms median, wired output | [latency.md](latency.md) section 5 |
| Session loudness (10 minutes, Master) | -24 or -18 LUFS +/- 2, per platform | record ([mixing.md](mixing.md) section 6) and a BS.1770 meter |
| True peak of the session | at most -1 dBTP | the same meter |
| Duck depth under a typical reward | 3 to 6 dB on Engine and Ambient | the recording, or `AudioServer.get_bus_peak_volume_left_db()` on the bus |
| Voices in use, peak | under the pool size | count playing players in the director |

## 5. Listening (a person)

An agent cannot listen. Write "proof: not run (needs a human listen)" for
this step, and give the person this list. Each item is pass or fail:

1. Each event is clear with the eyes closed: the player can name it.
2. The 3 most frequent sounds do not tire the ear after 5 minutes.
3. No reward sound plays on an outcome that lost something.
4. The biggest reward feels bigger than the smallest, and the order of the
   tiers is obvious.
5. No sound startles. No sound clicks, pops or crackles.
6. The engine and the ambient bed step back under a reward and return
   smoothly, with no pumping.
7. With the music on, every feedback sound is still audible.
8. With the sound off, the game is still playable (visual cues exist).

## 6. Report format

```
Sound review: <game> at <commit>
| Step | Check | Value | Pass mark | Result |
| 2 | onset_ms grass_cut.wav | 0.3 ms | <= 5 ms | pass |
| 3 | reward on net loss | 1 case: audio.gd:212 | 0 | FAIL |
| 5 | listening | - | - | not run (needs a human listen) |
Fixes, most important first: ...
```

Order the fixes: honesty first, then latency, then loudness and masking, then
repetition, then taste.
