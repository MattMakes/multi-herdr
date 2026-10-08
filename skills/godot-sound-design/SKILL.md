---
name: godot-sound-design
description: "Use when you design, synthesize, mix, tune or review game sound effects in Godot 4.7 so that actions and rewards feel immediate and honest: research rules with their limits, a latency budget, offline procedural synthesis, buses, ducking, loudness targets, an event-to-sound map, an audit."
---

# Godot sound design for game feedback

Target engine: **Godot 4.7**. Every GDScript block is typed and parses on
4.7.2. The Python blocks use the standard library only.

This skill decides **what** a feedback sound is and **how loud, how long and
how fast** it plays. `godot-audio-system` owns the engine side: players, the
bus API, pools, music. Read both when you build the sound of a game.

## What the research shows

Each rule carries a tag: **research** (a study supports it), **standard** (a
published specification sets the number) or **practice** (common game-audio
practice, not tested by these studies). Never cite a study for a claim in its
"does not show" column. Full citations, methods and numbers:
[references/research.md](references/research.md).

| Study | Shows | Does not show | Rule |
| --- | --- | --- | --- |
| Koepp et al. 1998, *Nature* 393:266 (PET) | Striatal dopamine release during a goal-directed video game; larger with better performance | Any effect of sound; that more feedback means more dopamine | Feedback follows what the player did, at once and in proportion |
| Dixon et al. 2014, *J Gambl Stud* 30:913 (N = 96) | Win sounds raise skin conductance, players prefer them, and players overestimate wins more with sound (+24 % against +15 % without) | Effects in skill games; which sound features matter; a heart-rate effect of sound | Reward sounds are honest: size follows real net gain; no reward sound on a net loss |
| Reynolds and Day 2007, *J Physiol* 583:1107 | A sound that starts with a visual event cuts a fast visual correction (134 ms) by about 20 ms | A latency threshold; the same effect at normal game loudness or in a game | Play the sound on the event frame, with a sharp onset |
| Haehn et al. 2024, *Appl Sci* 14:583 (2 x N = 32) | Character (action) sounds raise immersion, avatar identification and fun; ambient sound interacts with them on fun | An effect on perceived competence; a significant flow effect of ambient sound | Every player action gets a sound; tune the ambient bed together with it |

## The rules

Each rule has a pass mark that [the review](references/review-checklist.md)
checks.

1. **Same frame** (research). The sound plays inside the event handler. No
   `call_deferred`, `await` or `Timer` before `play()`. Pass: 0 frames from
   event to `play()`.
2. **Sharp onset** (research, indirect). Leading silence at most 1 ms; the
   sound reaches 20 dB under its peak within 5 ms.
3. **Low output latency** (practice). `audio/driver/output_latency` at most
   15 ms on desktop; `AudioServer.get_output_latency()` at most 30 ms; press
   to ear at most 50 ms on wired output.
4. **Honest rewards** (research). The reward tier comes from the net gain,
   never the gross payout. A net loss or zero gain plays no reward sound. A
   higher tier is never quieter or shorter than a lower tier.
5. **Every action speaks** (research). Every player action that changes the
   world has a character sound.
6. **Length follows rate** (practice). More than 8 events per second: 30 to
   90 ms. 2 to 8 per second: at most 150 ms. Rare rewards: 400 to 1000 ms.
7. **Variation against fatigue** (practice). A cue that plays more than once
   per second varies by +/- 0.5 to 1 semitone and +/- 1 to 2 dB. A cue that
   carries a value (tier, combo step) does not vary.
8. **Escalate on a scale, then cap** (practice). A combo steps up a
   pentatonic scale, stops at 1 octave (5 steps) and resets on a break.
9. **Bad news is quieter** (practice). Losses and breaks are lower, duller and
   quieter than the smallest reward. Nothing startles.
10. **Buses and slots** (practice). Every player uses a category bus. Each
    group has a frequency slot. The Master bus has a hard limiter at -1 dB.
11. **Loudness** (standard). Session: -24 LUFS +/- 2 (TV, speakers) or -18
    LUFS +/- 2 (handheld, laptop); true peak at most -1 dBTP. Planned
    feedback stays inside about 10 LU, from -28 to -18 momentary LUFS.
12. **Voices and ducking** (practice). Cap each cue at 2 to 4 voices (1 for
    rare rewards). Rewards duck the loops by 3 to 6 dB with a sidechain
    compressor. Silence before a big reward gives contrast.

## Pick the reference

| Task | Read |
| --- | --- |
| Cite a study, check a claim, explain a rule | [references/research.md](references/research.md) |
| Make sounds start on time; measure latency | [references/latency.md](references/latency.md) |
| Design one feedback sound: shape, length, variation, rewards, combos | [references/feedback-design.md](references/feedback-design.md) |
| Make the sounds with a script; measure the files | [references/synthesis.md](references/synthesis.md) |
| Buses, loudness targets, EQ slots, voices, ducking, mix capture | [references/mixing.md](references/mixing.md) |
| Connect the event bus to sounds through a data table | [references/event-sound-map.md](references/event-sound-map.md) |
| Audit a game's sound and report pass or fail | [references/review-checklist.md](references/review-checklist.md) |

## Workflow

**New sound set.** Read research, then feedback-design. List the events and
their rates (review-checklist section 4). Write the synthesis script, make the
files, measure them. Build the bus layout and the event-to-sound map. Run the
review.

**Tune an existing game.** Run the review first. Fix in this order: honesty,
latency, loudness and masking, repetition, taste. Measure again after each
change, and record the before and after values.

## Other skills own these parts

| Need | Skill |
| --- | --- |
| Players, bus API, voice pool code, music, spatial audio, settings sliders | `godot-audio-system` |
| The event bus that the sound director listens to | `godot-event-bus` |
| `SoundCue`, `SoundMap` and `RewardTiers` as data resources | `godot-resource-pattern` |
| Visual feedback that matches the sound (flash, shake, particles) | `godot-particles-vfx`, `godot-tween-animation` |
| Rewards, currency and payouts that define the net gain | `godot-economy-system` |
| Headless test runs and the check sequence | `godot-build-verify`, `godot-testing` |

## Prove it

> proof: headless-run: the map contract test (event-sound-map section 5) and
> the asset measurement (synthesis section 4) run without an audio device.
> Latency, session loudness and duck depth: proof: windowed run on the target
> machine. Feel: proof: not run (needs a human listen, review-checklist
> section 5).

## Report

Report each rule as a row with the measured value and the pass mark. Write
"not run" with the reason for a check you did not run. When a task does not
say the platform, use the -18 LUFS handheld target, and send 1 `QUESTION:` to
the orchestrator that names the -24 LUFS option. Do not wait for the answer
before you continue.
