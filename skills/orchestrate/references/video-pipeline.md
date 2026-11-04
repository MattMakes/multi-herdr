# Video pipeline recipe

Use this recipe when the operator asks for a rendered video: a launch film,
a product demo, a CLI walkthrough, a concept explainer or a social cut. 6
`video-*` teammates make it in 6 waves. Each wave passes a numeric gate
before the next wave starts.

## Before wave 1

1. Pick the project folder, for example `video/<slug>/`. Every teammate runs
   its commands from that folder. Put its path in `CONTEXT` of every plan.
2. Put the operator's prompt, the audience, the length in seconds, the
   aspects (`16:9`, `9:16`, `1:1`) and any brand files in the wave 1 plan.
3. Each teammate checks its host tools first (`ffmpeg`, `ffprobe`, `node`,
   `python3`, Chrome, macOS `say`). A `BLOCKED:` line names the install
   step. Take it to the human. Do not install tools yourself.

`<skill-dir>` in a gate command is the folder of the named skill in the
teammate's skill bundle. The teammate finds it. Copy the command into the
plan as it is.

## Project layout

```text
<project>/
  BRIEF.md  DIRECTION.md  STORYBOARD.md  timeline.json  MOTION.md
  captures/   <scene-id>.mp4, <scene-id>.json, src/
  audio/      vo-<scene-id>.wav, beats.json, sfx-cues.json, mix.wav
  scenes/     <scene-id>/index.html
  reanimate/  <scene-id>/ (reference keyframes, motion-spec.json)
  render/     master.mp4, 16x9.mp4, 9x16.mp4, 1x1.mp4, captions.srt,
              captions.vtt, chapters.txt, cover.jpg, preview.gif
  qa/         plan-check.json, audio-check.json, render-check.json,
              contact-sheet-360p.png, hook-strip.png, probe/,
              critique-round-<N>.md
```

## File ownership

| Teammate | Writes | Never writes |
|---|---|---|
| `video-director` | `BRIEF.md`, `DIRECTION.md`, `STORYBOARD.md`, `timeline.json`, `qa/plan-check.json` | scenes, captures, audio, render |
| `video-capture` | `captures/` | `timeline.json` |
| `video-audio` | `audio/`, `qa/audio-check.json`; in `timeline.json` only `start_sec`, `duration_sec`, `meta.duration_sec`, `meta.allowed_silences`; `qa/plan-check.json` after a reconciliation | scenes, captures, render |
| `video-animator` | `scenes/<scene-id>/` of its task, `MOTION.md`, `reanimate/`, `qa/reanimate-*` | `timeline.json` |
| `video-editor` | `render/`, `qa/probe/` | `timeline.json`, scenes |
| `video-critic` | `qa/render-check.json`, `qa/contact-sheet-360p.png`, `qa/hook-strip.png`, `qa/critique-round-<N>.md` | every other file |

Give each unit `own` and `do not touch` from this table. 2 animators in
wave 4 split `scenes/` by scene id. Only 1 of them owns `MOTION.md`.

## Wave 1: Direction & Storyboard

- Teammate: 1 `video-director`.
- Inputs: the operator prompt, the product code, the brand files.
- Outputs: `BRIEF.md`, `DIRECTION.md`, `STORYBOARD.md`, `timeline.json`,
  `qa/plan-check.json`.
- Gate: `python3 "<skill-dir>/scripts/check_timeline.py" timeline.json`
  (`video-storyboard`).
- Pass rule: exit 0, and `qa/plan-check.json` has no errors. The 8 plan
  gates include a hook by 3.0 s, scenes of 2.0 s to 8.0 s and 2.7 words/s
  or less of narration.

## Wave 2: Capture & Voice-Over

Run the 2 teammates at the same time. Skip `video-capture` when no scene has
kind `capture`.

- Teammate: `video-capture`.
  - Inputs: `timeline.json`, `STORYBOARD.md`, the app or CLI under test.
  - Outputs: `captures/<scene-id>.mp4`, `captures/<scene-id>.json`.
  - Gate: the plate gate of the `video-capture` skill, for each capture
    scene.
  - Pass rule: every plate passes, at `meta.fps` and the timeline size,
    with no blank or error screen.
- Teammate: `video-audio`, voice-over only (step 1 of its skill).
  - Inputs: `timeline.json`, `DIRECTION.md`.
  - Outputs: `audio/vo-<scene-id>.wav`.
  - Gate: none in this wave. Wave 3 checks the voice.

## Wave 3: Beat Grid, Audio Mix & Reconciled Timing

- Teammate: `video-audio` (resume it, or spawn fresh with `PRIOR WORK`).
- Inputs: `timeline.json`, `audio/vo-*.wav`, `captures/*.json`, an optional
  music bed.
- Outputs: the reconciled `timeline.json`, `audio/beats.json`,
  `audio/sfx-cues.json`, `audio/mix.wav`, `qa/audio-check.json`.
- Gate 1: `python3 "<skill-dir>/../video-storyboard/scripts/check_timeline.py" timeline.json`.
  Pass rule: exit 0 after the reconciliation.
- Gate 2: `python3 "<skill-dir>/scripts/check_audio.py" audio/mix.wav --timeline timeline.json --beats audio/beats.json --out qa/audio-check.json`
  (`video-audio`).
- Pass rule: exit 0. Integrated loudness is -15.0 to -13.0 LUFS, true peak
  is -1.0 dBTP or lower, `48000` Hz stereo, the length is within 0.05 s of
  `meta.duration_sec`, and no unplanned silence is longer than 0.80 s.

If the reconciliation moves a scene by more than 1 beat, send the new
timing to `video-director` before wave 4.

## Wave 4: Parallel Scene Animation

- Teammate: 1 to 3 `video-animator`, each with its own scene ids.
- Inputs: `timeline.json`, `DIRECTION.md`, `STORYBOARD.md`, `MOTION.md`,
  `audio/beats.json`, reference clips for a reanimate scene.
- Outputs: `scenes/<scene-id>/index.html` for every non-capture scene,
  `MOTION.md`, `reanimate/`, `qa/reanimate-check.json`.
- Gate 1: each scene exposes `window.__seekToFrame(frameIndex, fps)`. 3
  sample frames rendered forward and then in reverse are byte-identical.
- Gate 2, for a reference scene only:
  `python3 "<skill-dir>/scripts/compare_frames.py" --ref reanimate/<scene-id>/ref/keyframes --rendered reanimate/<scene-id>/rendered --spec reanimate/<scene-id>/motion-spec.json --out qa/reanimate-check.json`
  (`video-reanimate`).
- Pass rule: both gates pass. No `Math.random()`, `Date.now()`,
  `performance.now()`, `setTimeout` or `setInterval` in render code.

## Wave 5: Frame-Stepping Render, Composite & Publish Pack

- Teammate: `video-editor`.
- Inputs: `timeline.json`, `scenes/`, `captures/`, `audio/mix.wav`,
  `audio/beats.json`.
- Outputs: `render/master.mp4`, the aspect variants, `captions.srt`,
  `captions.vtt`, `chapters.txt`, `cover.jpg`, the preview, and the
  determinism probe in `qa/probe/forward/` and `qa/probe/reverse/`.
- Steps: render each scene with `render_frames.mjs --scene`, render the
  text overlays in Chrome with `--transparent`, assemble the master with the
  master mode of `render_frames.mjs`, then make the pack with `--package`.
  Do not hand-build an `ffmpeg` command for the master.
- Probe: the editor renders frames `0`, `K` and `2K` of the hook scene cold
  forward to `qa/probe/forward/`, then the same frames with `--reverse` to
  `qa/probe/reverse/`. Without the probe, wave 6 skips the determinism
  check.
- Gate: `ffprobe` on `render/master.mp4`, as the `video-render` skill shows.
- Pass rule: `h264` High level `42`, `yuv420p`, CFR at `meta.fps`, frame
  count equal to `meta.duration_sec` x fps, AAC `48000` Hz stereo.

## Wave 6: Numeric QA + Fresh-Session Blind Critic Loop

- Teammate: a new `video-critic` for each round. Never resume one.
- Inputs: `BRIEF.md`, `DIRECTION.md`, `STORYBOARD.md`, `timeline.json`,
  `render/`, `scenes/`, `qa/`. Do not put builder notes, earlier critiques
  or the fix history in its plan.
- Outputs: `qa/render-check.json`, `qa/contact-sheet-360p.png`,
  `qa/hook-strip.png`, `qa/critique-round-<N>.md`.
- Gate: `python3 "<skill-dir>/scripts/qa_video.py" --video render/master.mp4 --timeline timeline.json --probe-forward qa/probe/forward --probe-reverse qa/probe/reverse`
  (`video-qa`).
- Pass rule: `VERDICT: PASS` in `qa/critique-round-<N>.md`. That needs
  `qa/render-check.json` with `passed: true` (all 11 numeric checks), an
  average of 4.0 or more over the 8 axes (1 to 5 each), and no axis below 3.

The loop:

1. Round `N` starts at 1. Spawn a fresh `video-critic` and give it `N`.
2. On `VERDICT: FAIL`, group the fix lines by owner teammate. Send each
   group to a fresh teammate of that kind as a fix plan.
3. Run the waves again from the earliest owner: a director fix runs waves 1
   to 5 for the changed scenes, an audio fix runs waves 3 and 5, an
   animator fix runs waves 4 and 5, an editor fix runs wave 5.
4. Spawn a new `video-critic` for round `N + 1`. Stop after round 3. If
   round 3 fails, report the 3 critiques to the human. Do not start round
   4.
