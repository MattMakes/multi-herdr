---
name: video-storyboard
description: "Use when a video project has BRIEF.md and DIRECTION.md and needs a beat-timed plan before capture, animation, audio or render start. Writes STORYBOARD.md and the canonical timeline.json (meta plus scenes with timing, beats, narration, captions, plates, motion, safe-zone regions and focal points), then proves it with scripts/check_timeline.py: 8 numeric gates for the 3 s hook, 2-8 s scenes, caption density, 2.7 words/s narration, visual variety, safe zones, contiguous timing and schema."
---

# Video Storyboard

The storyboard turns the direction into a plan every later phase can run
without a question. `timeline.json` is the single source of truth for
scene ids, order and timing. Capture, audio, animation, render and QA all
read it. Only this phase and the audio timing reconciliation write it.

Inputs: `BRIEF.md`, `DIRECTION.md` (from `video-direction`).
Outputs: `STORYBOARD.md`, `timeline.json`, `qa/plan-check.json`.

`<skill-dir>` is the folder that holds this SKILL.md. Run every command from
the video project folder.

## Order of work

1. Read `BRIEF.md` and `DIRECTION.md`. Take the archetype beat list and
   scale it to `meta.duration_sec`.
2. Split each beat into scenes of 2 s to 8 s. Snap every cut to the music
   beat grid: at `bpm` B, 1 beat is `60 / B` s.
3. Write narration and the caption for each scene. Count words.
4. Write `STORYBOARD.md`, then `timeline.json` from it.
5. Run the checker. Fix every error. Run it again until it passes.

```bash
python3 "<skill-dir>/scripts/check_timeline.py" timeline.json
# writes qa/plan-check.json next to timeline.json; exit 0 = pass
python3 "<skill-dir>/scripts/check_timeline.py" timeline.json --out qa/plan-check.json
python3 "<skill-dir>/scripts/check_timeline.py" --self-test
```

Do not hand off with a failing `qa/plan-check.json`.

## STORYBOARD.md

1 section per scene, in order. Each section has:

- Heading: scene id, kind, start and end in seconds, beats.
- Frame: what the viewer sees, in 1 to 3 sentences. Name the plate (the
  layer under the motion: a capture file, an HTML scene, a still).
- Narration: the exact words, or "none".
- Caption: the exact on-screen text, or "none".
- Motion: the template and the 1 thing that moves first.
- Critical regions: what must stay visible, as boxes.
- Transition out: the type and its length (250 ms to 450 ms).
- Capture notes, for `capture` scenes: URL or command, start state, the
  clicks and keys in order.

Add a sketch per scene when the framing is not obvious: an ASCII box with
the focal point and the regions marked.

## timeline.json schema

1 file, 1 JSON object. Times are seconds as numbers. Boxes and points
are normalized: `0.0` is the left or top edge and `1.0` is the right or
bottom edge.

```json
{
  "meta": {
    "id": "acme-launch",
    "title": "Acme launch film",
    "aspect": "16:9",
    "fps": 60,
    "width": 1920,
    "height": 1080,
    "duration_sec": 30.0,
    "lufs_target": -14,
    "allow_dark_intro": false
  },
  "scenes": [
    {
      "id": "s01-hook",
      "kind": "hook",
      "start_sec": 0.0,
      "duration_sec": 2.5,
      "beat_in": 0,
      "beat_out": 5,
      "narration": "Your build broke again.",
      "caption": "Your build broke again",
      "plate": "scenes/s01-hook/index.html",
      "motion": {"template": "slam-in", "reveal_sec": 1.2},
      "transition_out": "cut",
      "critical_regions": [[0.12, 0.3, 0.76, 0.3]],
      "focal_point": [0.5, 0.45]
    }
  ]
}
```

### meta

| Field | Type | Rule |
|---|---|---|
| `id` | string | Non-empty. The project slug. |
| `title` | string | Non-empty. |
| `aspect` | string | `16:9`, `9:16` or `1:1`. |
| `fps` | integer | `30` or `60`. |
| `width`, `height` | integer | Must match the aspect: `1920x1080`, `1080x1920`, `1080x1080`. |
| `duration_sec` | number | Greater than 0. The sum of scene durations, within 0.05 s. |
| `lufs_target` | number | `-14` unless the brief says otherwise. |
| `allow_dark_intro` | boolean | Optional. `true` only when the direction asks for a dark open. Render QA reads it. |
| `allowed_silences` | list | Optional. Planned silent ranges as `[start_sec, end_sec]` pairs, `0 <= start_sec < end_sec <= duration_sec`. `check_audio.py` does not flag a silence inside one. |

### scenes[]

| Field | Type | Rule |
|---|---|---|
| `id` | string | Non-empty, unique. Use `sNN-slug`. Other phases name files by it: `captures/<id>.mp4`, `audio/vo-<id>.wav`, `scenes/<id>/`. |
| `kind` | string | `hook`, `capture`, `motion`, `character`, `diagram`, `callout` or `cta`. Scene 1 is `hook`. |
| `start_sec` | number | 0.0 for scene 1. Then the end of the scene before. |
| `duration_sec` | number | 2.0 s to 8.0 s. |
| `duration_override_reason` | string | Optional. When non-empty, the bounds widen to 1.0 s to 12.0 s. Say why (a beat-matched stinger, a long live run). |
| `static_hold` | boolean | Optional, default `false`. `true` marks a planned freeze (a held still, a paused terminal), so render QA does not flag it as a frozen frame. |
| `beat_in`, `beat_out` | integer | Music beat index at the scene start and end. `beat_in <= beat_out`. |
| `narration` | string | Spoken words. Empty string for none. |
| `caption` | string | On-screen text. Empty string for none. `\n` splits 2 lines. |
| `plate` | string | Non-empty path of the base layer, relative to the project root. |
| `motion` | object | `template` (non-empty string) names the motion recipe. Optional `reveal_sec`: seconds from scene start until motion and headline have landed. |
| `transition_out` | string | Non-empty. `cut`, `crossfade`, `push-left`, `wipe`, or a type named in `DIRECTION.md`. |
| `critical_regions` | list | `[x, y, w, h]` boxes that must stay visible in every crop and above the caption band. May be empty. |
| `focal_point` | list | `[x, y]`, each 0.0 to 1.0. The reframe for other aspects centers on it. |

Extra fields are allowed. The checker ignores them. Add them only when a
later phase reads them, and write them into `STORYBOARD.md` too.

## The 8 plan gates

`check_timeline.py` checks these. Each error in `qa/plan-check.json` has a
`rule`, a `message` and, when it applies, a `scene` id.

| # | `rule` | Pass rule |
|---|---|---|
| 1 | `hook_window` | Scene 1 starts at 0.0 s, has kind `hook`, has a caption (the headline), and `start_sec + motion.reveal_sec` is 3.0 s or less. Without `reveal_sec`, the whole scene must end by 3.0 s. |
| 2 | `scene_duration` | 2.0 s to 8.0 s, or 1.0 s to 12.0 s with a non-empty `duration_override_reason`. |
| 3 | `caption_density` | 1 line: 7 words or fewer. 2 lines: 14 words or fewer. Each line 42 characters or fewer. 3 or more lines fail. |
| 4 | `speaking_rate` | Narration words divided by `(duration_sec - 0.25)` is 2.7 words/s or less (160 WPM). The 0.25 s is the breath pad at the scene edge. |
| 5 | `visual_variety` | At most 2 scenes in a row share a kind. Adjacent scenes differ in kind, plate, `motion.template`, or a focal point shift of 0.25 or more (distance in normalized units). |
| 6 | `safe_zone` | Every region is inside the frame with area. Bottom edge `y + h` is 0.82 or less (caption band). In `9:16`, right edge `x + w` is 0.85 or less (platform buttons). |
| 7 | `contiguous_timing` | Scene 1 starts at 0.0 s. Each start equals the previous start plus duration, within 0.001 s. Durations sum to `meta.duration_sec` within 0.05 s. |
| 8 | `schema` | Every field in the tables above has the right type. IDs are non-empty and unique. A schema error stops the other gates, so fix it first. |

## Fix a failing gate

- `hook_window`: cut the hook text to 3 to 5 words, or set
  `motion.reveal_sec` lower and make the motion land sooner.
- `scene_duration`: split a long scene at a beat, or merge a short one
  into its neighbor. Use an override reason only for a deliberate choice.
- `caption_density`: the caption is a headline, not the narration. Cut
  it to the 1 idea of the scene.
- `speaking_rate`: cut words before you add seconds. 2.2 words/s sounds
  calm. 2.7 words/s is the limit, not the goal.
- `visual_variety`: change the framing (zoom, new plate, new template)
  or move the focal point to the other third of the frame.
- `safe_zone`: move content up, or for `9:16` move it left. Captions own
  the bottom 18%.
- `contiguous_timing`: recompute every `start_sec` from scene 1 in order.
  Do not edit 1 start by hand.

## Timing after audio

The audio phase measures each voice-over file. When a file plus the 0.25 s
pad is longer than its scene, it sets
`duration_sec = max(planned, vo + 0.25)`, recomputes every later
`start_sec` and `meta.duration_sec`, and runs the checker again. Plan
narration at 2.2 to 2.5 words/s so this change stays small.

## Hand-off

Send the capture, audio and animation phases the path of `timeline.json`
and the passing `qa/plan-check.json`. List any scene with an override
reason and any scene with a capture risk.
