---
name: video-qa
description: "Use when a rendered video (render/master.mp4) must pass the final quality gate - runs scripts/qa_video.py for 18 numeric render checks (determinism, stream integrity, frame 0, hook motion, black and freeze frames, -14 LUFS loudness and true peak, cut pops, aspect variants and captions, safe zones and flashes, 1-frame pops, unplanned cuts, dead time, flat opening, motion contrast, loop seam, raster upscale), draws a 360 px contact sheet, a 360 px wide phone sheet and a 0-3 s hook strip, then scores the video blind on 8 axes and writes qa/critique-round-<N>.md with timestamped fixes per owner."
---

# Video QA: numeric gate and blind critic

`<skill-dir>` is the folder that holds this SKILL.md. Run every command from
the video project root (the folder that holds `timeline.json`).

The gate has 2 parts. Part 1 is numeric and decides itself: a failed check is
a failed video. Part 2 is a blind visual review on 8 axes. The video passes
only when both parts pass.

## Blind review rules

- Read only these inputs: `BRIEF.md`, `DIRECTION.md`, `STORYBOARD.md`,
  `timeline.json`, `render/`, `scenes/` and `qa/`.
- Do not read builder notes, chat history, commit messages or earlier
  `qa/critique-round-*.md` files. Each round is a fresh session.
- Do not edit project files. Write only to `qa/`.
- Judge what the video shows, not what the plan says it should show.
- A round limit of 3 applies. After round 3 fails, report to the
  orchestrator. Do not start round 4.

## Step 1: run the numeric gate

```bash
python3 "<skill-dir>/scripts/qa_video.py" \
  --video render/master.mp4 --timeline timeline.json
```

Optional flags:

| Flag | Default | Use |
| :--- | :--- | :--- |
| `--scenes-dir` | `scenes/` next to `timeline.json` | Scene sources for the determinism scan |
| `--render-dir` | folder of `--video` | Holds `16x9.mp4`, `9x16.mp4`, `1x1.mp4`, `captions.srt`, `captions.vtt` |
| `--out` | `qa/render-check.json` | Report path |
| `--contact-sheet` | `qa/contact-sheet-360p.png` | Contact sheet path |
| `--hook-strip` | `qa/hook-strip.png` | Hook strip path |
| `--phone-sheet` | `qa/phone-sheet-360w.png` | Phone sheet path |
| `--raster-sizes` | `raster-sizes.json` in the render dir | Image natural vs drawn sizes for check 18 |
| `--probe-forward`, `--probe-reverse` | not set | Probe PNG folders for the render determinism check |

Exit codes: `0` all 18 checks pass, `1` at least 1 check fails, `2` the input
is bad (missing file, unreadable `timeline.json`, missing `ffmpeg`).

The probe folders hold the same file names, for frames `0`, `K` and `2K`. The
video-editor renders them with the video-render skill: a cold forward pass to
`probe/forward/` and a warm reverse pass to `probe/reverse/`. Without probe
folders the render determinism check reports `skipped` and passes.

Self-test (synthesizes all fixtures in a temp folder, about 10 s):

```bash
python3 "<skill-dir>/scripts/qa_video.py" --self-test
```

## Step 1b: run the text audit on each HTML scene

```bash
node "<skill-dir>/scripts/text_audit.mjs" \
  --scene scenes/<id>/index.html --out qa/text-audit-<id>.json
```

It steps the scene every `0.1 s` in headless Chrome and fails text that is
too small at 360 px phone width, shown too short, overlapping, off the frame,
in the caption or app zone, or showing `NaN`, `undefined` or
`[object Object]`. Exit `1` means findings, `2` means no Chrome. Each finding
is a fix line for `video-animator`. Rules, flags and limits are in
`<skill-dir>/references/text-audit.md`.

## Step 2: read qa/render-check.json

The report has `passed` (bool), `checks` (1 object per check, each with
`passed` and its measured values), `errors` (list of `check`, `message`,
and `timestamp` when the defect has a time) and `warnings` (same form; a
warning does not fail the gate, but put it in the critique report).

| # | Check | Fails when |
| :--- | :--- | :--- |
| 1 | `scene_determinism` | Scene `.html/.js/.mjs/.ts` calls `requestAnimationFrame`, `setInterval`, `setTimeout`, `Date.now`, `performance.now` or `Math.random`; HTML CSS has `animation:` or `transition:` other than `none`; a scene folder does not define `window.__seekToFrame` |
| 2 | `render_determinism` | Cold-forward vs warm-reverse probe frame `psnr_avg < 45.0 dB`, fewer than 3 probe frames, or the 2 folders hold different names |
| 3 | `stream_integrity` | No video or audio stream; `r_frame_rate`, `avg_frame_rate` and `meta.fps` differ; `pix_fmt` is not `yuv420p`; size is not `meta.width x meta.height`; a stream duration is more than `0.05 s` from `meta.duration_sec`; `nb_frames` is not `round(duration_sec * fps) +-1` |
| 4 | `frame0_readiness` | Frame 0 `YAVG` outside `[18, 240]` (unless `meta.allow_dark_intro` is `true`), or `YMIN == YMAX` |
| 5 | `hook_motion` | No pair of adjacent `0.5 s` samples in `0.0-3.0 s` has a mean absolute pixel difference `>= 0.03` |
| 6 | `black_frames` | `blackdetect=d=0.05:pic_th=0.98:pix_th=0.03` finds black outside a declared fade; a dark brand background such as `#0B1020` is not black |
| 7 | `freeze_frames` | `freezedetect=n=-60dB:d=1.5` finds a freeze longer than `1.5 s` outside a `static_hold` scene |
| 8 | `audio_loudness` | Integrated loudness is outside `meta.lufs_target` (default `-14.0 LUFS`) `+-1.0 LU`, or true peak is above `-1.0 dBTP` |
| 9 | `audio_boundary_pops` | Audio peak within `20 ms` of a scene cut is above `-3.0 dBFS` |
| 10 | `variants_captions` | A present aspect variant has the wrong size or a duration more than `0.05 s` off; a present caption file does not parse, has an empty cue, overlapping cues, or a cue outside `[0, meta.duration_sec]` |
| 11 | `safe_zone_flash` | A `critical_regions` box leaves `[0, 1]`, crosses the caption zone (`y + h > 0.82`) or, for `9:16`, the right UI zone (`x + w > 0.85`); more than 3 full-frame luminance transitions above `20%` occur in 1 s |
| 12 | `frame_pops` | 1 frame jumps away from its neighbours and the next frame comes back (a pop, or a flash when mean luma changes by more than `20%`) |
| 13 | `cuts_vs_plan` | A hard cut is more than `0.5 s` from every scene boundary (`transition_out` time) |
| 14 | `dead_time` | No motion for more than `2.0 s` outside a `static_hold` scene |
| 15 | `flat_opening` | Motion per second in `0-5 s` is under `0.5x` the rest of the film (films of `7 s` or more) |
| 16 | `motion_contrast` | The busiest seconds have under `1.5x` the motion of the calmest (90th vs 10th percentile, films of `6 s` or more) |
| 17 | `loop_seam` | `meta.loop` is `true` and the last frame to frame 0 step is above `max(0.02, 3x` the typical step near the ends`)` |
| 18 | `raster_upscale` | A raster image is drawn at `1.2x` its natural size or more; above `1.12x` for `0.5 s` or more is a warning |

Variant sizes follow the master's short side `S` and long side `L`:
`16x9.mp4` is `L x S`, `9x16.mp4` is `S x L`, `1x1.mp4` is `S x S`. For a
`1920x1080` master these are `1920x1080`, `1080x1920` and `1080x1080`.

Scan scope for check 1: files under `node_modules/`, `vendor/` or `lib/`
folders and `*.min.js` files are vendored libraries and are not scanned.
Comments are not scanned.

Checks 12 to 17 read every frame in grey at `96 px` on the long side. The measures, the
thresholds and the `raster-sizes.json` format for check 18 are in
`<skill-dir>/references/motion-checks.md`. Without that file check 18
reports `skipped` and passes.

`timeline.json` fields the gate reads:

- `meta`: `fps`, `width`, `height`, `duration_sec` (required);
  `lufs_target`, `aspect`, `allow_dark_intro`, `loop` (optional).
- `scenes[]`: `id`, `start_sec`, `duration_sec` (required);
  `transition_out` (string or `{type, duration_sec}`; a type that contains
  `fade` permits black within `duration_sec` of the cut, default `0.4 s`),
  `static_hold` (`true` permits a freeze inside the scene),
  `critical_regions` (list of normalized `[x, y, w, h]`).

When check results fail, the review still runs. Put every numeric error in
the critique report with its timestamp and owner.

## Step 3: inspect the sheets

- `qa/contact-sheet-360p.png`: 1 tile every `0.5 s` (`2 fps`) of the whole
  video, `360 px` high, 6 tiles per row. Each tile shows its time
  (`MM:SS.mmm`) and scene ID in the top left corner.
- `qa/hook-strip.png`: 12 tiles at `4 fps` for `0.0-2.75 s`, same layout.
- `qa/phone-sheet-360w.png`: the contact sheet tiles at `360 px` wide, the
  width of a phone screen. Use it to judge text size and detail on a phone.

Read both images at full size. At `360 px` high, text that you cannot read is
too small for a phone. Use the hook strip for axis 1 and the contact sheet for
pacing, layout drift, safe zones and visual cohesion. To see 1 moment in
detail, extract 1 frame:

```bash
ffmpeg -v error -ss 12.500 -i render/master.mp4 -frames:v 1 qa/frame-12.500.png
```

## Step 4: score 8 axes

Give each axis an integer score from `1` to `5`. The score anchors, the
evidence for each axis and the owner of each defect type are in
`<skill-dir>/references/rubric.md`. Read it before you score.

1. Hook & First-Frame Read
2. Narrative & Pacing
3. Typography & Safe Zones
4. Motion Craft & Physics
5. Product/Demo Clarity
6. Character/Illustration Cohesion
7. Audio-Visual Sync
8. Brand & Aesthetic Cohesion

Verdict rule. All 3 conditions must be true for `VERDICT: PASS`:

- `qa/render-check.json` has `passed: true`.
- The average of the 8 scores is `>= 4.0`.
- No axis scores below `3`.

Otherwise the verdict is `VERDICT: FAIL`.

## Step 5: write qa/critique-round-<N>.md

`<N>` is the round number that the orchestrator gives you (1, 2 or 3). Use the
template in `references/rubric.md`. Each fix line has this form:

```text
[MM:SS.mmm] Scene <id>: <issue> -> <owner>: <fix>
```

- `MM:SS.mmm` is the start of the defect. Use the tile label, the
  `timestamp` from `render-check.json`, or the frame that you extracted.
- `<owner>` is 1 of `video-director`, `video-capture`, `video-audio`,
  `video-animator`, `video-editor`.
- `<issue>` names what is wrong, with a number when there is one.
- `<fix>` is 1 concrete action that the owner can do without asking.
- Write 1 line for each defect. Sort lines by time.

Send the verdict line and the report path to the orchestrator. Stop after the
report. Do not fix the defects.
