---
name: video-plates
description: Use when a video scene comes from a renderer other than the HTML seek contract and must enter the timeline as a frame-exact plate - a Blender shot, a Manim animation, a WebGL or Remotion composition, a game-engine recording or a composited layer. Covers the plate contract (size, constant frame rate, exact frame count, colour tags, alpha), the sidecar file, lossless intermediates and the final encode with ffmpeg, wrapping a plate in an HTML scene for overlays, compositing with ffmpeg or the EffectCraft command line, and the checks before hand-off.
---

# Plates: frames from any renderer, exact in the timeline

The film pipeline renders HTML scenes itself. Everything else arrives as a **plate**: a video file or a frame folder that some other tool rendered. A plate that is 1 frame short, at a different frame rate or in a different colour range breaks the master in ways that show only at the end. This skill is the contract that prevents it.

`timeline.json` stays the single source of truth. A scene names its plate in `plate`. You never edit `timeline.json`; you build a plate that fits it.

## The contract

For a scene with `duration_sec` D in a timeline with `meta.fps` F and size W by H:

| Property | Rule |
|---|---|
| Frame count | `round(D * F)` plus the handles the plan asks for. Count frames; do not trust the duration field. |
| Frame rate | constant, exactly F. No variable frame rate, no duplicated or dropped frames. |
| Size | W by H, or larger by the scene's zoom need (see scale). Square pixels. |
| Scale | A plate is never drawn above 1.2 times its natural size. A scene that zooms needs a plate rendered at 2 times or more. |
| Colour | BT.709, limited range, tagged. 8-bit `yuv420p` for a delivery plate. |
| Alpha | When the plate sits over other layers: a PNG sequence with straight alpha, or ProRes 4444. Say which. |
| Audio | none. Sound goes to the audio seat as files with cue times. |
| Determinism | 2 renders of the same source give the same frames. Fix every seed. |
| Place | `plates/<clip>.mp4` (or `plates/<clip>/f%05d.png`) with `plates/<clip>.json` beside it. Working media is not committed. |

Render to a lossless intermediate first: a PNG sequence, or FFV1 in a Matroska file. Encode once at the end. A crash in the middle of a long render then loses nothing, and a fix re-renders only the frames that changed.

## The sidecar

Write `plates/<clip>.json` in this shape. It is the same shape that recorded plates use, so 1 reader serves both.

```json
{
  "clip": "plates/<clip>.mp4",
  "fps": 60,
  "frame_count": 360,
  "seed": 7,
  "build": "<tool and version>; <source commit>",
  "key_actions_sec": {"hero_lands": 1.67, "cut_point": 4.20},
  "pixel_facts": {"subject_width_px": 640, "nearest_distance_px": 96},
  "max_frozen_sec": 0.0,
  "render_scale": 1.0,
  "timers_forced": []
}
```

- Measure `fps` and `frame_count` from the written file.
- `key_actions_sec` lists every moment that sound or a cut must hit, from frame 0 of the clip. The director and the audio seat sync to these.
- `max_frozen_sec` is the longest run of identical frames that you intend. A plate animated on 2s at 24 fps has `0.042`. Write the real value: the editor reads it to tell a designed hold from a fault.
- `render_scale` is the plate width over the timeline width.

The plate gate of the capture skill (`check_plate.py`) is for recordings. It fails any run of identical frames longer than 1 frame, so a stepped or held render fails it by design. Use the checks below for a rendered plate.

## Encode and check

From a PNG sequence to a delivery plate:

```sh
ffmpeg -y -framerate 60 -i plates/<clip>/f%05d.png \
  -vf "scale=1920:1080:flags=lanczos,format=yuv420p" \
  -c:v libx264 -preset slow -crf 14 -profile:v high -r 60 -fps_mode cfr \
  -color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv \
  -movflags +faststart -an plates/<clip>.mp4
```

Checks, all required:

1. Count: `ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames,r_frame_rate,avg_frame_rate,width,height,pix_fmt,color_space,color_range -of json plates/<clip>.mp4`. `nb_read_frames` equals the planned count. `r_frame_rate` equals `avg_frame_rate` equals F.
2. Freezes: `ffmpeg -i plates/<clip>.mp4 -vf freezedetect=n=0.001:d=<max_frozen_sec + 1 frame> -f null -` prints no `freeze_start` that you did not plan. A small mover on a still ground can read as frozen at this threshold: then lower `n`, or compare frame hashes.
3. Black and blank: `ffmpeg -i plates/<clip>.mp4 -vf blackdetect=d=0.05:pix_th=0.02 -f null -` prints nothing unplanned.
4. Determinism: render 3 frames again from a cold start (first, middle, last) and compare them with the first render: `ffmpeg -i a.png -i b.png -lavfi ssim -f null -` gives `All:1.000000`. A GPU renderer that cannot reach that reports the measured value; do not mix GPU and software renders in 1 plate.
5. Look at frame 0, the middle frame and the last frame at full size.

A plate that is short gets fixed at the source. Do not pad a short plate with a held frame unless `MOTION.md` records the hold.

## Wrap a plate for overlays

When the scene needs type, callouts or a camera move over the plate, wrap it in an HTML scene. Then the animator's tools apply and the renderer treats it as any other scene.

- Use a frame folder, not a video element, when you can: load `f%05d.png` by index. Pick the index with `SeekTimeline.plateFrame(t, fps, lastFrame)` from the `video-motion` scene library, and wait for the image to decode before the seek resolves.
- A video element also works: set `currentTime` from `t` and resolve on `seeked`. Encode that plate with every frame as a keyframe (`-g 1`), or seeks land on the wrong frame.
- Keep the plate under the 1.2 times scale limit through the whole camera move.

## Renderers

| Source | Skill or page | Notes |
|---|---|---|
| Blender | `references/blender-shots.md`, with the `blender-rendering` skill | frames first, then encode |
| Manim | the `video-manim` skill | exact frame counts from run times |
| three.js, WebGL | the `video-webgl` skill | usually a normal HTML scene; a plate only when it is too slow to seek |
| Remotion | `references/remotion.md` | licence check first |
| Godot or another engine | the capture seat, or the `godot-cutscenes` skill | Movie Maker frames |
| Layered composite | `references/compositing.md` | ffmpeg first; EffectCraft when installed |

## Standing rules

- Work offline. A renderer that wants to download a browser, a model, a font or a sample needs the operator's word in the task. Report `BLOCKED:` with the exact item.
- Do not install a renderer yourself. The preflight of your seat names each tool and its install step.
- Report in `DONE:`: the plate path, the tool and version, the frame count and rate from `ffprobe`, each check with its result line, and the render time per frame.
