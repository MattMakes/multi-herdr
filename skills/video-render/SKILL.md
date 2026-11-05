---
name: video-render
description: "Use when a video project pack has scenes, captures and a mastered audio mix and you must produce the final files - steps headless Chrome frame by frame through window.__seekToFrame into exact PNG frames and a CFR H.264 MP4, composites capture plates, HTML scene clips, Chrome-rendered caption overlays and audio/mix.wav into render/master.mp4 with beat-snapped transitions, then packages 16x9 / 9x16 / 1x1 exports reframed on focal_point and critical_regions, captions.srt, captions.vtt, chapters.txt, cover.jpg, preview.gif and preview.mp4."
---

# Video Render, Composite and Publish

Phase 5 of the video pipeline. Inputs come from the earlier phases. Outputs go
to `render/` and the critic (video-qa) reads them next.

`<skill-dir>` is the folder that holds this SKILL.md. Every command below runs
from the video project directory, the folder that holds `timeline.json`.

Tools: Node 22 or later (built-ins only), `ffmpeg` and `ffprobe` on `PATH`,
Google Chrome. The script looks for Chrome at
`/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`. Set
`CHROME_PATH` to use another binary. Nothing is downloaded or installed.

## Inputs and outputs

| Reads | Writes |
|---|---|
| `timeline.json` (`meta`, `scenes[]` with `start_sec`, `duration_sec`, `transition_out`, `focal_point`, `critical_regions`, `narration`, `caption`, optional `title`) | `render/scenes/<scene-id>.mp4` (one clip per HTML scene) |
| `scenes/<scene-id>/index.html` (exposes `window.__seekToFrame`) | `render/overlays/<name>/frame_%06d.png` (transparent text layers) |
| `captures/<scene-id>.mp4` (browser plates) | `render/master.mp4` |
| `audio/mix.wav`, `audio/beats.json` (`bpm`, `beats_sec`, `downbeats_sec`) | `render/16x9.mp4`, `render/9x16.mp4`, `render/1x1.mp4` |
| | `render/captions.srt`, `render/captions.vtt`, `render/chapters.txt`, `render/cover.jpg`, `render/preview.gif`, `render/preview.mp4` |

Do not edit `timeline.json` timing here. When a cut misses its beat, send the
fix back to the timeline owner and re-run the plan checker.

## Step 1 - render each HTML scene frame by frame

```sh
node "<skill-dir>/scripts/render_frames.mjs" --scene scenes/s01-hook/index.html \
  --out render/scenes/s01-hook.mp4 --fps 60 --width 1920 --height 1080 --duration 3.30
```

What the script does, in order:

1. Starts Chrome with `--headless=new`, software raster (`--disable-gpu`),
   `--force-device-scale-factor=1`, `--force-color-profile=srgb`,
   `--font-render-hinting=none`, `--disable-lcd-text` and no background
   throttling. It drives Chrome over the DevTools Protocol.
2. Sets the viewport to exactly `--width` x `--height` at scale 1.
3. Waits for the load event, `document.fonts.ready` and `decode()` on every
   `<img>`. A broken image fails the render.
4. Fails unless `typeof window.__seekToFrame === "function"`.
5. For each frame `k`, awaits `window.__seekToFrame(k, fps)`, then captures a
   PNG of the viewport. Wall-clock time never enters the frame.
6. For an `.mp4` out, encodes the PNGs with
   `-framerate <fps> -c:v libx264 -preset slow -crf 16 -pix_fmt yuv420p
   -profile:v high -level:v 4.2 -g <fps> -bf 2 -movflags +faststart`
   (constant frame rate). For a directory out, keeps `frame_%06d.png` files,
   named by frame index.

Frame count comes from `--frames`, else `--duration` x `--fps`, else
`window.__videoMeta.duration`. Width and height must be even.

| Flag | Meaning |
|---|---|
| `--frames 120` | frames 0 to 119 |
| `--frames 0,15,30` | only those frames (write one index as `30,`) |
| `--reverse` | capture in descending order, same file names |
| `--transparent` | clear page background, RGBA PNGs (frames directory only) |
| `--subframes 8` | motion blur: average 8 seeks per frame (1, 2, 4 or 8) |
| `--raster-sizes f.json` | also write image natural vs drawn sizes |

Render all HTML scenes in parallel workers (60 % of CPUs, at most 8), each
with its tail handle:
`render_frames.mjs --scenes timeline.json --out-dir render/scenes [--subframes 8]`.
It also writes `render/raster-sizes.json` for the video-qa `raster_upscale`
check: `references/raster-sizes.md`. Review renders and probes use 1 subframe. The final master uses 8 for smooth
motion. Rules: `references/blur-parallel-revision.md`.

Tail handle: render each clip `transition_out` seconds longer than
`duration_sec`, so the next transition has frames to blend (Step 3).

Determinism probe for video-qa, cold forward and then warm reverse:

```sh
node "<skill-dir>/scripts/render_frames.mjs" --scene scenes/s01-hook/index.html --out qa/fwd --frames 0,15,30,45,59
node "<skill-dir>/scripts/render_frames.mjs" --scene scenes/s01-hook/index.html --out qa/rev --frames 0,15,30,45,59 --reverse
```

Same-named files in `qa/fwd` and `qa/rev` must be byte-identical. If they are
not, the scene keeps state between seeks. Fix the scene, not the renderer.

## Step 2 - render all on-screen text in Chrome

The local `ffmpeg` has no `drawtext`, `subtitles` or `ass` filter. Every
caption, label, lower third and title card is HTML:

1. Make `scenes/overlay-captions/index.html` with a transparent `body` and a
   `__seekToFrame` that shows the kinetic caption for time `k / fps`.
2. Render it for the full film with `--transparent --out render/overlays/captions`.
3. Composite the PNG sequence with the `overlay` filter (Step 3).

Keep caption text inside the safe zone that the plan checker enforces (bottom
`18%` stays clear of `critical_regions`, right `15%` on `9:16`).

## Step 3 - assemble render/master.mp4

```sh
node "<skill-dir>/scripts/assemble_master.mjs" --master timeline.json --out render/master.mp4
```

Every input path is relative to the folder of `timeline.json`:

1. Scene clip, first match wins: `render/scenes/<scene-id>.mp4` (the Step 1
   render), then `captures/<scene-id>.mp4`. Each clip is normalized to
   `meta.fps`, fitted and padded to `meta.width` x `meta.height`, and trimmed
   by frame count. A short clip holds its last frame.
2. Transitions from each scene's `transition_out`, a string or
   `{"type": ..., "duration_sec": ...}`. The duration is clamped to
   `0.25-0.40 s`, default `0.30 s`. The last scene's value is ignored.

   | `transition_out` | ffmpeg |
   |---|---|
   | `cut` | `concat` |
   | `fade`, `crossfade` | `xfade` `fade` |
   | `wipeleft`, `wipe` | `xfade` `wipeleft` |
   | `smoothleft`, `push-left` | `xfade` `smoothleft` |
   | `circleopen` | `xfade` `circleopen` |

   Any other name fails. The transition starts at the next scene's
   `start_sec`, so render each clip with its tail handle (Step 1).
3. Beat check: when `audio/beats.json` exists, every scene start after the
   first must be within `1` frame of a value in `beats_sec` or
   `downbeats_sec`. A miss fails. The script never shifts a cut.
4. Text layers: every `render/overlays/<name>/frame_%06d.png` sequence (Step
   2), frame 0 at film time 0, overlaid back to front in name order.
5. `audio/mix.wav`, padded or cut to the film length.

Output: CFR H.264 High@L4.2, `-crf 16`, `yuv420p`, AAC `320 kbps`,
`48 kHz`, stereo, `round(meta.duration_sec x meta.fps)` frames. A missing
clip or `audio/mix.wav` exits `1` with a list of each missing path. The
script prints a JSON report: `frames`, `duration_sec`, `beat_checked`, the
source and transition of each scene, and the overlays used.

The filter graph is explained in `references/ffmpeg-compositing.md`.

Verify before you hand off:

```sh
ffprobe -v error -show_entries stream=codec_name,profile,level,pix_fmt,width,height,r_frame_rate,avg_frame_rate,nb_frames,sample_rate,channels -of json render/master.mp4
```

Pass rule: `h264`, `High`, level `42`, `yuv420p`, `1920x1080`,
`r_frame_rate == avg_frame_rate == 60/1`, `nb_frames == round(meta.duration_sec x 60)`,
audio `aac`, `48000`, `2` channels.

## Step 4 - publish pack

```sh
node "<skill-dir>/scripts/render_frames.mjs" --package timeline.json \
  --master render/master.mp4 --out-dir render [--aspects 16x9,9x16,1x1] \
  [--cover-sec 2.1] [--scene-clips render/aspect]
```

It writes, with the same inputs giving the same text files every run:

| File | Rule |
|---|---|
| `captions.srt`, `captions.vtt` | 1 cue group per scene from `narration` (else `caption`). At most 2 lines of 42 chars per cue. A long scene splits into cues sized by word count. |
| `chapters.txt` | `M:SS Title` lines. Scene 0 at `0:00`, then every scene with a `title`. With no titles, every scene. |
| `cover.jpg` | `1920x1080` frame at `--cover-sec`, default 75% into the first `hook` scene |
| `preview.mp4` | `5 s` from the hook start, muted, `1280 px` wide |
| `preview.gif` | the same `5 s`, `15 fps`, `480 px` wide, looping |
| `16x9.mp4`, `9x16.mp4`, `1x1.mp4` | `1920x1080`, `1080x1920`, `1080x1080`, master codec settings |

Reframing, per scene and per aspect:

1. If `--scene-clips <dir>/<scene-id>.<aspect>.mp4` exists, use it. This is
   the path for HTML scenes: re-render them with
   `--scene "scenes/<id>/index.html?aspect=9x16" --width 1080 --height 1920`.
   The scene reads `aspect` from `location.search` and lays itself out again.
2. Else crop the master: the largest window of the target aspect, centred on
   `focal_point`, then shifted to hold the union of `critical_regions`.
3. When the union does not fit in that window, the scene is letterboxed and
   the report prints a warning. Re-render that scene (path 1). Never ship a
   blind centre crop that cuts a critical region.

The script prints a JSON report: `outputs`, `warnings` and the `reframe` mode
of each scene (`crop` with `x`, `y`, `w`, `h`, `fit` or `clip`).

Chapter warnings repeat the YouTube rules: 3 or more chapters, each 10 s or
longer. Scenes are 2-8 s, so give `title` only to the scene that opens each
chapter.

## Step 5 - self-notes and revision scope

Rules and formats: `references/blur-parallel-revision.md`.

1. Every round: `--scenes timeline.json --sample --out-dir qa/scope/round-<n>`.
2. Fix round, before encoding: `--scope-check timeline.json --before
   qa/scope/round-<n-1> --after qa/scope/round-<n> --fix <ids>`. A changed
   scene outside `--fix` fails with its id.
3. Before the critic: write `qa/self-notes.md`, 3 or more timestamped
   notes. Route the fixes 1 time. Gate: `--self-notes qa/self-notes.md
   --timeline timeline.json`.

## Self-test

```sh
node --check "<skill-dir>/scripts/render_frames.mjs"
node "<skill-dir>/scripts/render_frames.mjs" --self-test
node --check "<skill-dir>/scripts/assemble_master.mjs"
node "<skill-dir>/scripts/assemble_master.mjs" --self-test
```

It runs in a temporary directory and removes it after. It checks exact `.srt`,
`.vtt` and `chapters.txt` text, crop math, frame parsing, CFR H.264 encoding,
a full `--package` run on a synthetic master, and, when Chrome is present,
forward/reverse byte-identical frames, `.mp4` scene output, transparent
frames, the missing `__seekToFrame` failure, blur width at 1 and 8
subframes, parallel scenes, the scope check and the self-notes gate.

The `assemble_master.mjs` self-test builds a 3-scene project from colour
plates, 1 transparent text layer, a sine `mix.wav` and a `beats.json`. It
assembles the master and probes stream specs, frame count, duration, the
xfade blend, the overlay and the clip lookup order. It also checks the
off-beat failure and the exit `1` on missing inputs.

## Gotchas

- `requestAnimationFrame` does not fire for the renderer's page. A scene that
  waits on it inside `__seekToFrame` hangs the render until the `60 s` CDP
  timeout. Make `__seekToFrame` synchronous or await only fonts and images.
- `yuv420p` needs even width and height. Crop windows are rounded to even.
- `xfade` needs both inputs at the same size, frame rate, pixel format and
  timebase. `assemble_master.mjs` normalizes every clip first (Step 3, item 1).
- An `.mp4` from `--scene` needs frames `0..N-1`. Sparse `--frames` lists
  write a directory.
- Exit codes: `0` success, `1` render or package failure, `2` usage error.
