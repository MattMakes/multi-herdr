# Designing a feedback sound

> Back to [SKILL.md](../SKILL.md). Tags (research, standard, practice) are
> defined in [research.md](research.md).

A feedback sound tells the player 3 things: **when** the event happened,
**what** happened, and **how much** it was worth. Each part of the sound
carries one of them.

## 1. Transient, body, tail

| Part | Time | Carries | Rules |
| --- | --- | --- | --- |
| Transient | 0 to 10 ms | When | Attack 1 to 5 ms. A click, a noise burst or a pitched "tick" with a fast decay. No leading silence. (research, indirect: Reynolds and Day) |
| Body | 10 to 150 ms | What | The pitch, timbre and noise colour name the event. 2 events that mean different things get bodies that differ in pitch or colour, not only in loudness. (practice) |
| Tail | 50 ms to 2 s | How much | Length, brightness and chord size grow with the value of the event. A small event has almost no tail. (practice, bounded by Dixon: honest) |

Fade every sound out to zero over its last 2 to 5 ms, so that it does not
click at the end. (practice)

## 2. Length by event rate

A sound that repeats faster than it ends piles up into a wash. Set the
length from the event rate:

| Event rate | Example | Audible length | Extra rule |
| --- | --- | --- | --- |
| More than 8 per second | grass cut, footstep on a run, bullet hit | 30 to 90 ms | A cooldown of 50 to 80 ms, or a per-sound cap of 2 to 3 voices. Quiet: 10 dB or more under rewards. |
| 2 to 8 per second | step, small pickup, combo tick | 60 to 150 ms | Cap of 3 to 4 voices. |
| Every few seconds | hit taken, enemy cut, shot fired | 150 to 400 ms | Cap of 2 to 4 voices. |
| Rare (less than once in 10 s) | find, level-up, shape closed | 400 to 1000 ms | 1 voice. Duck the loops under it (see [mixing.md](mixing.md)). |
| Milestone (once a run) | run end, new record | up to 2 s | 1 voice. Silence before it gives contrast. |

All values are practice. Check them by the review in
[review-checklist.md](review-checklist.md).

## 3. Variation against repetition fatigue

The ear notices an identical sound that repeats, and it tires of it.
(practice)

- A sound that plays more than once per second gets pitch variation of
  +/- 0.5 to 1 semitone (about +/- 3 to 6 %) and volume variation of
  +/- 1 to 2 dB.
- A sound that plays more than 4 times per second gets 3 or more variants
  as well, in random order without an immediate repeat.
- Do not vary a sound that carries a value (a reward tier, a combo step).
  The variation then hides the value.

`AudioStreamRandomizer` does both without code:

```gdscript
extends Node

## Builds a randomized stream: 3 variants, no immediate repeat,
## +/- 0.7 semitone and +/- 1.5 dB.
@export var variants: Array[AudioStream] = []


func make_cut_stream() -> AudioStreamRandomizer:
	var random_stream := AudioStreamRandomizer.new()
	random_stream.playback_mode = AudioStreamRandomizer.PLAYBACK_RANDOM_NO_REPEATS
	random_stream.random_pitch_semitones = 0.7
	random_stream.random_volume_offset_db = 1.5
	for variant: AudioStream in variants:
		random_stream.add_stream(-1, variant)
	return random_stream
```

## 4. Honest reward size

**Rule (research: Dixon et al. 2014).** The size of a reward sound follows
the real net gain. A net loss or a zero net gain gets no reward sound.

Compute the net gain where the game knows the cost: the payout minus what
the action cost the player in the same transaction (fuel, ammo, a bet, a
broken streak). Choose the tier from the net gain, never from the gross
payout.

```gdscript
class_name RewardTiers
extends Resource

## Maps a real net gain to a reward-sound tier. Tier -1 means "no reward
## sound". thresholds[i] is the smallest net gain for tier i, ascending.
@export var thresholds: PackedInt32Array = PackedInt32Array([1, 10, 50, 200])
@export var streams: Array[AudioStream] = []


func tier_for(net_gain: int) -> int:
	if net_gain <= 0:
		return -1
	var tier: int = -1
	for i: int in thresholds.size():
		if net_gain >= thresholds[i]:
			tier = i
	return tier


func stream_for(net_gain: int) -> AudioStream:
	var tier: int = tier_for(net_gain)
	if tier < 0 or tier >= streams.size():
		return null
	return streams[tier]


## Self-check for a test: a loss is silent, and the tier never falls when
## the gain rises.
func is_honest(max_gain: int) -> bool:
	if tier_for(0) != -1 or tier_for(-1) != -1:
		return false
	var last: int = -1
	for gain: int in range(1, max_gain + 1):
		var tier: int = tier_for(gain)
		if tier < last:
			return false
		last = tier
	return true
```

A bigger tier gets a longer tail, a wider chord or a brighter top. It does
not get more than 3 dB more loudness than the tier below it, so that the mix
stays inside its loudness target.

## 5. Escalation with a combo

A combo (a streak of good actions) can raise the pitch of each step. The
player hears the streak grow. (practice)

- Step up on a musical scale, not in free cents, so that the steps sound
  like a melody. A major-pentatonic scale avoids sour intervals.
- Stop at a ceiling (5 steps of the pentatonic scale make 1 octave). Above the
  ceiling, hold the top step and add a brighter layer instead.
- Reset to the first step when the streak breaks. Play the break sound
  lower and duller than the first step, never louder than the steps.
- The step follows the real streak count. Do not step up on an action that
  did not extend the streak (honesty rule).

```gdscript
class_name ComboPitch
extends RefCounted

## Returns the pitch_scale for a combo step on a major-pentatonic scale.
## Step 0 is the root; step 5 is 1 octave up (pitch_scale 2.0).
const PENTATONIC: Array[int] = [0, 2, 4, 7, 9]

var max_steps: int = 5


func _init(steps: int = 5) -> void:
	max_steps = steps


func pitch_for(step: int) -> float:
	var clamped: int = clampi(step, 0, max_steps)
	var octave: int = floori(float(clamped) / PENTATONIC.size())
	var degree: int = clamped % PENTATONIC.size()
	var semitones: int = octave * 12 + PENTATONIC[degree]
	return pow(2.0, float(semitones) / 12.0)
```

## 6. Bad news

A loss, a hit or a broken streak needs a clear sound too. Make it lower,
duller (less high frequency) and shorter than the good-news sounds. Do not
make it louder than the rewards: a loud penalty startles and punishes, and
the player turns the sound off. (practice)

## 7. Silence as contrast

A big reward is louder by contrast. Before a rare reward, duck the loops by
3 to 6 dB, or leave 100 to 300 ms with no new small sounds. Do not stack
small sounds on top of the big one. (practice)
