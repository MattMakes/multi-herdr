---
name: video-character
description: Use when a video scene needs a character, mascot or expressive icon that moves - layered SVG or Canvas rigs with named pivots, MOTION.md identity tokens, breathing and blinking idle life, anticipation, squash and stretch, follow-through, reaction states and lip-sync driven by the voice-over, all computed from the frame index.
---

# Video character rigs

A character in a video scene is a rig: a tree of named layers, each with a pivot. All motion is a formula of `t = frameIndex / fps`. Nothing runs on a clock. The rig follows the seek contract of the `video-motion` skill: `window.__seekToFrame(frameIndex, fps)` sets every layer on every call.

## When to use

- A scene in `timeline.json` has a character, mascot or expressive icon layer.
- `MOTION.md` asks for a character, or the archetype is `character-spot`.
- A character looks stiff, twitchy, or out of sync with the voice-over.

Do not use it for UI motion or type. Use `video-motion` for those.

## Inputs

- `MOTION.md`: character identity tokens (palette, proportions, personality, motion tokens).
- `timeline.json`: the scenes where the character appears and its reaction cues.
- `audio/sync-sheet.json` (`vo_segments[].words[]` with `word`, `start_sec`, `end_sec`, in project time) and the voice-over WAV files: word timings and the amplitude envelope.

Read `references/rig-and-visemes.md` for the full layer tree, the formulas, the viseme table format and a rig check.

Read `references/idle-life.md` for idle motion recipes (breath, blink, sway, flicker, ripple) for things other than a character rig.

## 1. Rig hierarchy

Build the character as nested SVG groups (or a Canvas draw tree with the same names):

```text
rig-root
  rig-torso
    rig-head
      rig-eyes   (rig-eye-l, rig-eye-r, each with rig-lid-*)
      rig-brows  (rig-brow-l, rig-brow-r)
      rig-mouth  (1 child per viseme shape)
    rig-limbs    (rig-arm-l, rig-arm-r, rig-leg-l, rig-leg-r)
  rig-props      (badges, tools, hair, ears)
```

- Every group id starts with `rig-`. Names say what the part is, not how it looks.
- Every group that rotates or scales sets an explicit pivot: `transform-box: fill-box` and `transform-origin` in pixels or percent (for example `rig-head` at the neck, `rig-arm-l` at the shoulder).
- A child moves with its parent. Put motion on the highest group that should move.
- Read colours, line weights, proportions and the personality from `MOTION.md` identity tokens. Do not invent a new look per scene. The same character looks the same in every scene.
- Design an original character for each project. Do not copy an existing mascot.

## 2. Micro-motion from t

Compute every value from `t` in `__seekToFrame`. Default ranges:

1. Idle breathing.
   - A sine wave on `rig-torso` at 0.25-0.4 Hz (2.5-4 s per breath).
   - Scale Y by 1-2% and translate Y by 1-3 px at 1080p. Pivot at the base of the torso.
2. Eye blink.
   - Blink every 2.5-5.0 s. Draw the intervals once at load from `mulberry32(seed)`.
   - A blink closes in 60 ms and opens in 60 ms (120 ms total, about 7 frames at 60 fps).
   - Scale the lids, not the whole eye.
3. Anticipation.
   - Before a primary action, move 2-4 frames in the opposite direction.
   - Keep the counter-move small: 10-20% of the main move.
4. Squash and stretch.
   - On an impact or a jump, deform at most 12%.
   - Preserve volume: when scale Y is `1 + s`, scale X is `1 / (1 + s)`.
5. Follow-through.
   - Hair, ears, badges and secondary limbs lag the main part by 3-6 frames.
   - Use the main part's motion at `t - lag`, damped by 30-60%.
6. Minimum move. No pose change lasts less than 100 ms (6 frames at 60 fps).

## 3. Reaction states

The character has a fixed set of states. A state sets the target pose of each layer.

| State | Use for | Pose notes |
| --- | --- | --- |
| `idle` | Default | Breathing and blinks only |
| `point` | Direct the eye to a focal point | Arm and gaze toward `focal_point` |
| `celebrate` | Success, a reveal | Arms up, squash on landing |
| `think` | A question, loading | Head tilt 6-10 degrees, eyes up |
| `warn` | An error, a risk | Brows down, lean back |
| `talk` | Voice-over plays | Mouth follows visemes |

- Put state changes in a cue list with times in seconds: `[{ "t": 1.2, "state": "point" }]`.
- Blend between states over 150-300 ms with the enter ease. Add anticipation before the blend.
- Idle life (breathing, blinks) runs in every state.

## 4. Lip-sync

The mouth is driven by data that exists before the render. Never analyse audio in the browser.

- Option A, amplitude envelope. Precompute the RMS amplitude of the voice-over per frame with `ffmpeg` into a JSON array indexed by `frameIndex`. Map amplitude to `rest`, `open` and `wide` by 2 thresholds.
- Option B, viseme table. Write a table of `{frame, viseme}` from word timings, with visemes `rest`, `open`, `wide`, `round`, `closed`.
- In `__seekToFrame`, look up the entry for `frameIndex`. Show 1 mouth shape. Hold each shape for at least 2 frames to remove flicker.
- Close the mouth (`closed` or `rest`) within 3 frames after a word ends.

## Workflow

1. Read `MOTION.md`. Add the character identity tokens if they are missing.
2. Draw the rig as SVG with the `rig-` hierarchy. Set every pivot.
3. Write `renderCharacter(t, frameIndex)` with idle life, state blends and the mouth lookup. Call it from `__seekToFrame`.
4. Precompute the lip-sync data and the cue list into JSON files beside the scene.
5. Run the rig check in `references/rig-and-visemes.md`.
6. Render 3 frames forward and in reverse. They must match.

## Output

- The rig inside `scenes/<scene-id>/index.html`, or a shared `scenes/<scene-id>/character.svg`.
- `scenes/<scene-id>/visemes.json` or `envelope.json`, and `cues.json`.
- Updated character tokens in `MOTION.md`.
