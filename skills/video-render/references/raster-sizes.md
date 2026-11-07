# Raster size report

video-qa check 18 (`raster_upscale`) fails an image drawn 1.2x or more larger
than its natural size. The video file does not hold image sizes, so the render
writes them to `render/raster-sizes.json`. The file format and the pass rules
are in `skills/video-qa/references/motion-checks.md`, check 18.

## Who writes it

- `--scenes timeline.json --out-dir render/scenes` (no `--sample`) writes
  `render/raster-sizes.json` next to `timeline.json`. It moves each scene's
  samples to film time (`start_sec + t`) and drops samples in the tail handle,
  because the next scene covers that time.
- `--scene <page> --out <clip> --raster-sizes <file>` writes the report of 1
  scene, with scene times.
- `--sample` and the determinism probes write no report.

## How it samples

The QA step is `0.1 s`, so the report uses the same step. After the frame
capture, the same Chrome page seeks the frame nearest each `0.1 s` point
again and reads sizes. It takes no screenshot, so the cost is about 1 seek per
`0.1 s` of scene. The 5 frames of `--sample` are too far apart for the
`0.5 s` warning rule, so the report does not use them.

At each point it reads every visible raster image:

| Source | Natural size | Drawn size |
|---|---|---|
| `<img>` | decoded image | box after `object-fit` |
| CSS `background-image: url()` | decoded image | `background-size` in the padding box, times the CSS transform scale |
| `<canvas>` `drawImage` of an `<img>` | source rectangle | destination size times the context transform and the canvas CSS scale |
| `<video poster>` | decoded poster | box after `object-fit`, while the poster shows |
| SVG `<image>` | decoded image | box after `preserveAspectRatio` |

Drawn sizes are in device pixels (`devicePixelRatio`, 1 in the render).
Visible means a non-zero box that touches the viewport and passes
`checkVisibility` with opacity and visibility. SVG files and `data:image/svg`
are vector, so they are skipped. 2 copies of 1 image at 1 time give 1 sample,
the larger one, so a copy does not lengthen a warning run.

## Pre-upscaled plates

The render also checks whether a raster was enlarged before the 1.2x gate.
It downscales the decoded pixels by the claimed 2x export factor, enlarges
them back with nearest-neighbour sampling, and compares the RGB pixels with
the decoded source. A round-trip PSNR above 40 dB means the file contains no
meaningful detail above half its dimensions. The report then adds `real_px`,
`detail_scale`, and `detail_psnr_db` to the sample. The gate uses `real_px`,
reports that measured resolution, and therefore rejects a pre-upscaled plate
even when its file dimensions equal its drawn dimensions.

Limits:

- A canvas keeps its image draws until the next seek calls `drawImage` or
  `clearRect` on it. A canvas that only draws shapes after a seek keeps the
  draws of the seek before.
- A rotated element reports its bounding box, which is larger than the image.
- Canvas draws of a canvas, a video or an `ImageBitmap` are not counted.
