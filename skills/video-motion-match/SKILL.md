---
name: video-motion-match
description: Use when a video scene must recreate the motion of a reference clip or GIF - decompose it into keyframes, a numbered contact sheet and a motion-spec.json of layers, boxes, opacity, transforms and easing, then rebuild it as a deterministic scene and loop scripts/compare_frames.py until SSIM and box IoU pass.
---

# Reference reanimation

Rebuild the movement of a reference clip as a deterministic scene. First measure the reference. Then build a candidate, render the same moments, and compare numbers. Repeat until the numbers pass. Do not judge a match by eye alone.

`<skill-dir>` means the directory where this SKILL.md sits. Run every command from the video project folder.

## When to use

- The operator gives a reference video, GIF or frame folder and asks for "motion like this".
- A storyboard beat names a reference clip.

The rebuilt scene follows the seek contract of the `video-motion` skill. A character in the reference also uses the `video-character` skill. Recreate motion and layout ideas only. Use the project's own brand, copy and characters, not the reference's.

## Inputs

- The reference: `ref/clip.mp4`, a GIF, or a folder of PNG frames.
- The target `fps`, width and height from `timeline.json`.
- `MOTION.md` and `BRAND.md` tokens.

## 1. Decompose the reference

Work in `motion-match/<scene-id>/`.

1. Probe it: `ffprobe -v error -show_entries stream=width,height,r_frame_rate:format=duration -of json ref/clip.mp4`.
2. Extract all frames at the target size for inspection:
   `ffmpeg -i ref/clip.mp4 -vf "fps=<fps>,scale=<w>:<h>:flags=area" motion-match/<scene-id>/ref/all/f%05d.png`
3. Find the keyframes, where the motion changes:
   `ffmpeg -i ref/clip.mp4 -vf "select='gt(scene,0.08)',showinfo" -f null -` prints a `pts_time` for each change.
   Always add the first frame, the last frame, and the start and end of each visible move. Use 6-16 keyframes.
4. Copy each keyframe to `ref/keyframes/k<NN>.png` in time order.
5. Make `contact-sheet.png`: every keyframe in a grid, each labelled with its frame number, timestamp in seconds and `tau`.
   - Warning: the local `ffmpeg` has no `drawtext` filter. Do not label tiles with `ffmpeg`.
   - Write `contact-sheet.html`: a CSS grid of `<img>` tiles with an HTML caption under each tile.
   - Capture it with headless Chrome: `"<chrome>" --headless=new --screenshot=contact-sheet.png --window-size=1920,1080 file://$PWD/contact-sheet.html`.
6. Write `motion-spec.json` (format below).

`tau` is normalized time in [0, 1]: `tau = t / duration_sec` of the clip. A rebuilt scene of a different length maps its times by the same `tau`.

## 2. motion-spec.json

```json
{
  "schema_version": 1,
  "source": { "width": 1280, "height": 720, "fps": 30, "duration_sec": 2.4 },
  "keyframes": [
    { "tau": 0.0,  "frame": 0,  "t_sec": 0.0,  "foreground_bbox": [0.31, 0.38, 0.38, 0.24] },
    { "tau": 0.25, "frame": 18, "t_sec": 0.6,  "foreground_bbox": [0.22, 0.30, 0.56, 0.40] }
  ],
  "layers": [
    {
      "id": "card",
      "kind": "shape",
      "z": 2,
      "track": [
        { "tau": 0.0,  "bbox": [0.31, 0.38, 0.38, 0.24], "opacity": 0.0,
          "transform": { "x": 0, "y": 24, "scale": 0.96, "rotate_deg": 0 } },
        { "tau": 0.25, "bbox": [0.22, 0.30, 0.56, 0.40], "opacity": 1.0,
          "transform": { "x": 0, "y": 0, "scale": 1.0, "rotate_deg": 0 },
          "easing": "cubic-bezier(0.22, 1, 0.36, 1)" }
      ]
    }
  ]
}
```

- Every box is normalized `[x, y, w, h]`: left, top, width and height as fractions of the frame.
- `keyframes[].foreground_bbox` is the union box of all moving layers. `compare_frames.py` uses it as the reference box when it is present.
- `layers` is the inventory: 1 entry per visible element, with z order and a track of states.
- `easing` on a track entry describes the curve from the previous entry to this entry. Estimate it from the in-between frames: measure the progress at 25%, 50% and 75% of the move time and pick the closest curve.
- Measure boxes on the extracted frames. Do not guess them from the contact sheet.

## 3. Rebuild

1. Build `scenes/<scene-id>/index.html` with `window.__seekToFrame` and a paused GSAP timeline.
2. Place every tween at `tau * duration_sec` of the scene. Use the `motion-spec.json` boxes, opacity, transforms and easing.
3. Use the project's tokens for colour and type. Copy timing, layout, easing and choreography from the spec.

## 4. Compare and iterate

1. Render the candidate at each keyframe `tau` into `motion-match/<scene-id>/rendered/` with the render step of the pipeline. The frame index is `round(tau * (frame_count - 1))`.
2. Run the check:

```sh
python3 "<skill-dir>/scripts/compare_frames.py" \
  --ref motion-match/<scene-id>/ref/keyframes \
  --rendered motion-match/<scene-id>/rendered \
  --spec motion-match/<scene-id>/motion-spec.json \
  --out qa/motion-match-check.json \
  --diff-strip qa/motion-match-diff-strip.png
```

3. Read `qa/motion-match-check.json`. Fix the frame named by `worst_tau` first.
   - Low IoU means a wrong position or size. Fix the box or transform at that `tau`.
   - Good IoU with low SSIM means a wrong shape, colour, blur or an easing that lags. Compare the in-between frames.
4. Look at `qa/motion-match-diff-strip.png`. Each row shows reference, candidate and the absolute difference. Bright areas are wrong.
5. Change 1 cause per round. Run the check again. Stop when it passes, or after 6 rounds. Then report the numbers and the remaining defect.

## The script

`<skill-dir>/scripts/compare_frames.py` needs only Python 3 and `ffmpeg` / `ffprobe`.

- `--ref` and `--rendered` each take a frame folder (PNG or JPEG, name order) or a video file. Frames pair by `tau`.
- Without `--spec`, a folder reference uses all its frames. A video reference uses `--samples` keyframes (default 8).
- For each pair it reports SSIM (from `ffmpeg` `ssim`), pixel accuracy (`1 - mean |difference| / 255`), the share of pixels that differ by more than 25 of 255, both foreground boxes and their IoU.
- The foreground box comes from the alpha channel when the frame has transparency. Otherwise it is the set of pixels that differ from the most common border colour by more than 24 of 255.
- The gate: exit 1 when mean SSIM `< 0.85` or mean IoU `< 0.85`. Exit 0 on pass. Exit 2 on bad input.
- `--self-test` builds synthetic frames in a temp folder, checks pass and fail cases, and checks the size budgets of the 3 video motion skills.

The gate is the minimum. For a close recreation, also aim for `mean_mismatch_ratio <= 0.02` and `max_mismatch_ratio <= 0.05`.

## Output

- `motion-match/<scene-id>/ref/keyframes/`, `contact-sheet.png` and `motion-spec.json`.
- `scenes/<scene-id>/index.html`.
- `qa/motion-match-check.json` and `qa/motion-match-diff-strip.png` from the passing run.
