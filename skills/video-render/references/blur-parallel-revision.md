# Motion blur, parallel render and revision gates

Detail for `SKILL.md` Steps 1 and 5. Every command runs from the video project
directory.

## Subframe motion blur

`--subframes <n>` (1, 2, 4 or 8, default 1) renders `n` seeks per output frame.
For frame `k`, subframe `j` calls `window.__seekToFrame(k + j / n, fps)`. The
shutter opens at frame `k` and stays open for 1 frame. `ffmpeg tmix` averages
the `n` subframes into 1 PNG. The frame count and the CFR rate do not change.

- The scene must accept a fractional `frameIndex`. A scene that computes
  `t = frameIndex / fps` and sets every value from `t` does this already.
  A scene that rounds `frameIndex`, or steps a counter, gives `n` identical
  subframes and no blur.
- Use 1 subframe for every review render and for the determinism probe.
- Use 8 subframes for the final master when the direction has smooth motion
  (camera moves, eased slides, spring moves). Use 4 when render time matters.
  Use 1 for hard-cut, stepped or pixel styles, where blur softens the look.
- A render with 8 subframes takes about 8 times as long. Use parallel
  workers (below).
- The blur is deterministic: the same frames rendered forward and in
  `--reverse` are byte-identical.

## Parallel scene render

```sh
node "<skill-dir>/scripts/render_frames.mjs" --scenes timeline.json \
  --out-dir render/scenes [--workers 6] [--subframes 8]
```

- It renders every HTML scene to `render/scenes/<scene-id>.mp4`. A scene is
  HTML when its `plate` ends in `.html`, or when `scenes/<scene-id>/index.html`
  exists. Capture plates are not rendered.
- Frame count per scene: the frames up to the `start_sec` of the next scene, plus the tail
  handle of its `transition_out` (the same plan as `assemble_master.mjs`).
  Size and fps come from `meta`.
- Each worker is a child `node` process with its own Chrome. The default
  count is 60 % of the CPUs, rounded down, at least 1 and at most 8.
- Every scene runs to the end. When 1 or more scenes fail, the command exits
  `1` and names each failed scene with its error.
- It prints a JSON report: `workers`, `subframes`, and `id`, `out`, `frames`
  of each scene.

## Revision scope check

A fix round must change only the frames inside the fix. After each render
round, sample the scenes. In a fix round, compare the new samples with the
samples of the round before, then encode.

```sh
node "<skill-dir>/scripts/render_frames.mjs" --scenes timeline.json --sample --out-dir qa/scope/round-2
node "<skill-dir>/scripts/render_frames.mjs" --scope-check timeline.json \
  --before qa/scope/round-1 --after qa/scope/round-2 --fix s03-demo,s04-cta
```

- `--sample` writes `qa/scope/round-<n>/<scene-id>/`. An HTML scene gets 5
  frames: the first, the quarters and the last of its clip, at 1 subframe.
  A capture scene gets `plate.sha256`, the SHA-256 of its plate file.
- `--fix` lists the scene ids inside the fix. An unknown id is a usage error
  (exit `2`).
- Every scene outside `--fix` must have samples in both folders, the same
  file names and byte-identical files. Else the check exits `1` and
  `changed` names each scene id with the reason.
- A changed scene outside the fix is a regression of the fix. Send it back
  to the owner of the fix. Do not encode the master.
- The check covers scene pages and plates. It does not cover text overlays
  in `render/overlays/` or `audio/mix.wav`.

## Self-notes before the critic

Before the hand-off to `video-critic`, watch your own master and write
`qa/self-notes.md`. Write at least 3 notes that a demanding director would
give. Each note is 1 list item that starts with a film time:

```md
- 0:00.4 s01-hook: the title lands 2 frames before the downbeat. Fix: video-animator.
- [0:02] s02-cta: the button text is illegible at phone size. Fix: video-animator.
- 0:03.5 s02-cta: the fade ends on a flat frame. Fix: video-editor.
```

- The time is `M:SS` or `M:SS.s`, bare or in brackets.
- Name the scene, the defect, a measurable fix and the owner.
- Send all fixes in 1 message, 1 time. Fix your own notes, render again, then
  hand off. Do not start a second self-notes round. The critic loop does the
  next rounds.

Gate:

```sh
node "<skill-dir>/scripts/render_frames.mjs" --self-notes qa/self-notes.md --timeline timeline.json
```

It exits `1` with fewer than 3 timestamped notes, or with a note after
`meta.duration_sec`. List items without a time do not count.
