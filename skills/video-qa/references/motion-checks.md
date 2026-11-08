# Motion, cut and frame checks (checks 12 to 20)

`qa_video.py` decodes every frame of the master once more, in grey, scaled so
the long side is `96 px` (`96x54` for a 16:9 master). A frame step is the mean
absolute pixel difference between 2 adjacent frames, from `0` (same) to `1`
(black to white). Motion per second is the sum of the frame steps in 1 whole
second. All constants are at the top of `scripts/qa_video.py`.

## Check 12: frame_pops

Frame `i` is a pop when both steps around it (`i-1` to `i` and `i` to `i+1`)
are `0.08` or more, and frame `i-1` and frame `i+1` differ by at most half of
the smaller step. So the picture jumps away for exactly 1 frame and comes back.
When the mean luma of frame `i` differs from frame `i-1` by more than `0.20`,
the report calls it a flash. Owner: the scene owner (`video-animator` or
`video-capture`), or `video-editor` when the pop is at a join.

## Check 13: cuts_vs_plan

A frame step is a cut when it is `0.10` or more and at least `3x` the median
step of the 8 frames on each side. Pop frames from check 12 are not cuts. The
plan puts a transition at every scene boundary (the end of each scene, where
`transition_out` applies). A cut within `0.5 s` of a boundary is planned. Any
other cut fails. `planned_cuts_not_seen` lists boundaries with
`transition_out: "cut"` where no cut was found. It is information only,
because a scene can continue the same picture across a cut.

## Check 14: dead_time

A run of frame steps below `0.002` longer than `2.0 s` is dead time. A run
inside a `static_hold: true` scene (1 frame of tolerance) is planned. Check 7
(`freeze_frames`) uses ffmpeg `freezedetect` at `1.5 s` and finds only exact
freezes. Check 14 also finds a slow drift, where the mean change is under
`0.5` grey levels per pixel per frame, but it allows a longer run.

## Check 15: flat_opening

The mean motion per second of `0-5 s` must be at least `0.5x` the mean of the
rest. Films shorter than `7 s` skip the check, because the rest is too short
to compare.

## Check 16: motion_contrast

The 90th percentile of motion per second must be at least `1.5x` the 10th
percentile. A film where every second moves the same amount has no calm or busy
stretch. Films shorter than `6 s` skip the check.

## Check 17: loop_seam

Runs only when `meta.loop` is `true`. The step from the last frame to frame 0
must be at most `max(0.02, 3x the median step of the first and last second)`.

## Check 18: raster_upscale and raster-sizes.json

A raster image (PNG, JPEG, WebP) drawn larger than its natural size looks soft.
The scene knows both sizes. The video file does not, so the gate reads a size
report that the render writes next to the master.

Decision on the data path: the render step that drives Chrome writes
`render/raster-sizes.json`. At each sample time it calls `__seekToFrame`, then
reads, for every visible `<img>` and SVG `<image>` with a raster source:

- `natural_px`: `naturalWidth` and `naturalHeight` (for SVG `<image>`, the
  size of the decoded image).
- `drawn_px`: `getBoundingClientRect()` width and height times
  `devicePixelRatio`, so a CSS `scale()` and the render size both count.

Format:

```json
{
  "step_sec": 0.1,
  "samples": [
    {"t_sec": 1.2, "src": "materials/dashboard.png", "natural_px": [1600, 1000], "drawn_px": [1840, 1150]}
  ]
}
```

`step_sec` is the time between samples (default `0.1`). The scale of a sample
is the larger of the 2 axis ratios `drawn / natural`. A canvas `drawImage` of a
`<video>` is a sample too (`"source": "video"`, natural = `videoWidth` x
`videoHeight`). Pixel art passes: a sample with `"sampling": "nearest"` at the
same whole-number scale on both axes (`3x`, within `0.02`) is not an upscale.
`1.2x` stays the limit for smoothed scaling. Per `src`:

- fail when 1 sample has a scale of `1.2` or more;
- warn when consecutive samples above `1.12` last `0.5 s` or more
  (`count x step_sec`). A warning goes to `warnings` in the report and does
  not fail the gate.

`--raster-sizes` overrides the path. When the file is absent, the check fails
if a scene source draws a raster (`<img>`, `<video>`, `drawImage`, a `.png`,
`.jpg`, `.webp` or `.mp4` path) or `captures/` holds a plate, and skips
otherwise. A report with `0` samples fails: an empty report proves nothing.
A file that does not parse, or a natural size of `0`,
is a bad input (exit `2`). Owner of a fix: `video-capture` (capture at a larger
size) or `video-animator` (draw it smaller).

## Check 19: cut_flash

For every scene in `timeline-master.json`, or `timeline.json` when the master
timeline is absent, the gate compares the mean luma of the final 2 frames before
the scene end. A difference above `6%` of full scale fails and reports the scene
ID, both frame numbers, and both luma values. A scene is exempt only when
`static_hold` is `true` and `static_hold_reason` contains `fade`.

## Check 20: gate_results

The default expected results are `qa/audio-check.json`,
`audio/music-continuity.json`, `qa/timeline-check.json`, and
`qa/plate-check.json`. Repeat `--expect <path>` to replace that list. Each file
must exist, contain a true `pass` or `passed` field, and be at least as new as
the source it checks. Audio and music results check `audio/mix.wav`; timeline
and plate results check `render/master.mp4`. A missing source also fails.

## Phone sheet

`qa/phone-sheet-360w.png` has the contact sheet tiles (`2 fps`, 6 per row) at
`360 px` wide, the width of a phone screen. A 16:9 master gives `360x202`
tiles, a 9:16 master `360x640` tiles.

`--phone-frames <dir>` writes a separate `hero.png` at the midpoint of
`meta.hero_sec` (or the film midpoint) and every scene's key-action frames at
`360 px` high. `locked_actions_sec` values are scene-relative. A scene without
locked actions contributes its midpoint. Review these frames with the phone-size
self-review in `rubric.md`.
