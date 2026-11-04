# ffmpeg compositing and publish reference

Commands for Steps 3 and 4 of the video-render skill. Every command runs from
the video project directory. The example film is `60 fps`, `1920x1080`,
3 scenes:

| id | kind | start_sec | duration_sec | transition_out |
|---|---|---|---|---|
| `s01-hook` | hook (HTML scene) | 0.00 | 3.00 | `fade`, 0.30 s |
| `s02-demo` | capture (browser plate) | 3.00 | 6.00 | `cut` |
| `s03-cta` | cta (HTML scene) | 9.00 | 4.00 | none (last scene) |

`meta.duration_sec` is `13.00`, so the master has `780` frames.

The local `ffmpeg` has `overlay`, `xfade`, `concat`, `loudnorm` and `ebur128`.
It has no `drawtext`, `subtitles` or `ass`. All text arrives as Chrome-rendered
PNG frames.

## 1. Frame arithmetic

- Frame index of a time: `k = round(t x fps)`. Every scene boundary must be a
  whole frame: `start_sec x fps` is an integer.
- Clip length in frames: `n = round(duration_sec x fps) + round(handle x fps)`,
  where `handle` is the scene's `transition_out` duration (`0` for `cut` and
  for the last scene).
- Trim by frame count (`trim=end_frame=n`), not by seconds. Seconds round
  differently in each filter and drift by 1 frame.

## 2. Beat snap check

Read `audio/beats.json`. For every boundary `B = scenes[i].start_sec`,
`i >= 1`:

```text
err = min(|B - b| for b in beats_sec + downbeats_sec)
pass when err <= 1 / fps          (16.7 ms at 60 fps, 33.3 ms at 30 fps)
```

Run it before the first encode:

```sh
node -e '
const t = require("./timeline.json"), g = require("./audio/beats.json");
const fps = t.meta.fps, grid = [...g.beats_sec, ...g.downbeats_sec];
let bad = 0;
for (const s of t.scenes.slice(1)) {
  const err = Math.min(...grid.map((b) => Math.abs(b - s.start_sec)));
  if (err > 1 / fps) { bad++; console.log(`${s.id} start_sec ${s.start_sec}: ${(err * 1000).toFixed(1)} ms off the beat grid`); }
}
process.exit(bad ? 1 : 0);'
```

A miss goes back to the timeline owner. Do not shift a cut inside the
compositor. That moves every later scene off its voice-over.

The transition starts at the boundary. A `0.30 s` fade from `B = 3.00`
blends `3.00` to `3.30`. A hero reveal lands on the downbeat at `B`.

## 3. Normalize every layer

Every input to `xfade` and `concat` needs the same size, frame rate, pixel
format, sample aspect ratio and timebase. Normalize each clip to an
intermediate file first. The intermediates use `-crf 12` so a second encode
does not lose detail.

HTML scene clips come from the renderer at the right size and rate. Render
them with the tail handle:

```sh
node "<skill-dir>/scripts/render_frames.mjs" --scene scenes/s01-hook/index.html \
  --out render/scenes/s01-hook.mp4 --fps 60 --duration 3.30
node "<skill-dir>/scripts/render_frames.mjs" --scene scenes/s03-cta/index.html \
  --out render/scenes/s03-cta.mp4 --fps 60 --duration 4.00
```

Capture plates can have any size and a variable frame rate. Convert them to
CFR, fit them into the frame, then trim to the exact frame count. `tpad` holds
the last frame when a plate is short:

```sh
ffmpeg -y -i captures/s02-demo.mp4 -an -vf "fps=60,\
scale=1920:1080:force_original_aspect_ratio=decrease:flags=lanczos,\
pad=1920:1080:(ow-iw)/2:(oh-ih)/2:color=0x101820,setsar=1,format=yuv420p,\
tpad=stop_mode=clone:stop=-1,trim=end_frame=360,setpts=PTS-STARTPTS" \
  -c:v libx264 -preset slow -crf 12 -r 60 -fps_mode cfr render/scenes/s02-demo.mp4
```

Use the scene background colour from `direction.md` for `pad`, not black, so
a letterboxed plate does not flash.

Check every intermediate before you join:

```sh
for f in render/scenes/*.mp4; do
  ffprobe -v error -select_streams v:0 -count_frames \
    -show_entries stream=width,height,r_frame_rate,nb_read_frames,pix_fmt -of csv=p=0 "$f"
done
```

Expected: `1920,1080,yuv420p,60/1,198` (`s01-hook`), `...,360` (`s02-demo`),
`...,240` (`s03-cta`).

## 4. Join scenes with cuts and transitions

Build the chain left to right. Each join is one of 2 forms:

- `cut`: `[a][b]concat=n=2:v=1:a=0[ab]`
- `fade`, `wipeleft`, `smoothleft`, `circleopen`:
  `[a][b]xfade=transition=<type>:duration=<d>:offset=<B>[ab]`

`offset` is the time in the joined output where the transition begins. With
tail handles, that is the absolute boundary `B` = the next scene's
`start_sec`. The joined length stays at `meta.duration_sec`.

The timeline gives the transition as a string (`"fade"`) or an object
(`{"type": "fade", "duration_sec": 0.3}`). Clamp `duration_sec` to
`0.25-0.40 s`. Use `0.30 s` when it is absent.

For the example:

```text
[0:v][1:v]xfade=transition=fade:duration=0.30:offset=3.00[ab];
[ab][2:v]concat=n=2:v=1:a=0[base]
```

No trim is needed. The `xfade` output is `offset` plus the length of the
right input (`3.00 + 6.00 = 9.00 s`, `540` frames). The left input's frames
after `offset + duration` are dropped. A `cut` scene has no handle, so a
`concat` after it starts exactly at the next `start_sec`.

## 5. Overlay Chrome-rendered text

Render each text layer once for the whole film as a transparent PNG
sequence:

```sh
node "<skill-dir>/scripts/render_frames.mjs" --scene scenes/overlay-captions/index.html \
  --out render/overlays/captions --fps 60 --duration 13.00 --transparent
```

The overlay page must have `html, body { background: transparent }` and no
full-frame backdrop. Its `__seekToFrame(k, fps)` reads the caption cues for
`k / fps` from data built into the page. Do not fetch at render time.

Add it as an image-sequence input with the film frame rate, then overlay:

```text
-framerate 60 -start_number 0 -i render/overlays/captions/frame_%06d.png
...
[base][3:v]overlay=0:0:format=auto:eof_action=pass,format=yuv420p[v]
```

More text layers (lower thirds, keystroke badges, cursor ripples) are more
inputs and more `overlay` steps, in back-to-front order. Keep one layer per
role, so a fix re-renders only that layer.

For a short layer, render only its frames and place it with `itsoffset` and
`overlay=enable`:

```text
-itsoffset 4.50 -framerate 60 -i render/overlays/badge/frame_%06d.png
[v1][4:v]overlay=x=120:y=860:enable='between(t,4.50,6.00)'[v2]
```

## 6. Full master command

```sh
ffmpeg -y \
  -i render/scenes/s01-hook.mp4 \
  -i render/scenes/s02-demo.mp4 \
  -i render/scenes/s03-cta.mp4 \
  -framerate 60 -start_number 0 -i render/overlays/captions/frame_%06d.png \
  -i audio/mix.wav \
  -filter_complex "\
[0:v][1:v]xfade=transition=fade:duration=0.30:offset=3.00[ab];\
[ab][2:v]concat=n=2:v=1:a=0[base];\
[base][3:v]overlay=0:0:format=auto:eof_action=pass,format=yuv420p,trim=end_frame=780[v]" \
  -map "[v]" -map 4:a:0 \
  -c:v libx264 -preset slow -crf 16 -pix_fmt yuv420p -profile:v high -level:v 4.2 \
  -g 60 -bf 2 -r 60 -fps_mode cfr \
  -c:a aac -b:a 320k -ar 48000 -ac 2 -t 13.00 \
  -movflags +faststart render/master.mp4
```

Notes:

- `-t 13.00` makes the audio and video streams end together. A `mix.wav`
  that is shorter than `meta.duration_sec` is an audio defect. Send it back
  to video-audio. Do not pad it here.
- Do not run `loudnorm` on the master. `audio/mix.wav` is already mastered to
  `-14 LUFS` and `-1.0 dBTP`. AAC encoding can raise the true peak by up to
  about `0.5 dB`. Check the result with `ebur128` (Section 7).
- Level 4.2 covers `1920x1080` and `1080x1920` at `60 fps`.

## 7. Verify the master

```sh
ffprobe -v error -show_entries \
  stream=codec_name,profile,level,pix_fmt,width,height,r_frame_rate,avg_frame_rate,nb_frames,sample_rate,channels,bit_rate \
  -of json render/master.mp4
ffmpeg -hide_banner -nostats -i render/master.mp4 -map 0:a -af ebur128=peak=true -f null - 2>&1 | tail -n 12
```

| Field | Pass |
|---|---|
| video `codec_name`, `profile`, `level` | `h264`, `High`, `42` |
| `pix_fmt`, size | `yuv420p`, `1920x1080` |
| `r_frame_rate`, `avg_frame_rate` | both `60/1` |
| `nb_frames` | `round(meta.duration_sec x 60)` |
| audio | `aac`, `48000`, `2`, about `320000` bit/s |
| `ebur128` integrated / true peak | `-14.0 +-1.0 LUFS`, `<= -1.0 dBTP` |

Then hand `render/master.mp4` to video-qa.

## 8. Aspect exports

`render_frames.mjs --package` builds each aspect as 1 filter graph:

```text
[0:v]split=3[m0][m1][m2];
[m0]trim=start_frame=0:end_frame=180,setpts=PTS-STARTPTS,crop=606:1080:656:0,scale=1080:1920:flags=lanczos,setsar=1[v0];
[m1]trim=start_frame=180:end_frame=540,setpts=PTS-STARTPTS,crop=606:1080:1314:0,scale=1080:1920:flags=lanczos,setsar=1[v1];
[2:v]fps=60,scale=1080:1920:force_original_aspect_ratio=decrease:flags=lanczos,pad=1080:1920:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,tpad=stop_mode=clone:stop=-1,trim=end_frame=240,setpts=PTS-STARTPTS[v2];
[v0][v1][v2]concat=n=3:v=1:a=0[v]
```

- Scenes `0` and `1` are crops of the master. Scene `2` is an HTML scene
  re-rendered at `1080x1920` and passed through `--scene-clips`.
- The crop window is the largest window of the target aspect inside
  `1920x1080`: `606x1080` for `9:16`, `1080x1080` for `1:1`. Its centre
  starts at `focal_point`. It then moves the least distance that holds the
  union of `critical_regions`. Last, it is clamped to the frame and rounded
  to even pixels.
- A crop cuts across a transition. Scene boundaries in the aspect export are
  hard cuts at the master frames, so the master's transition frames appear
  inside the crop of the outgoing scene. Re-render both scenes at the target
  aspect when a transition shows a jump.
- `1080x1920` from a `606 px` wide crop is a `1.78x` upscale. Text rendered
  from HTML is soft at that scale. Re-render every text-heavy scene and every
  overlay at the target aspect for a 9:16 export that ships.

Re-render an HTML scene for an aspect:

```sh
node "<skill-dir>/scripts/render_frames.mjs" --scene "scenes/s03-cta/index.html?aspect=9x16" \
  --out render/aspect/s03-cta.9x16.mp4 --width 1080 --height 1920 --fps 60 --duration 4.00
```

The scene reads `new URLSearchParams(location.search).get("aspect")` and sets
its layout before the first `__seekToFrame` call. The clip name is
`<scene-id>.<aspect>.mp4`, the form `--scene-clips` looks for.

## 9. Publish text files

`.srt` cue:

```text
1
00:00:00,000 --> 00:00:03,000
Stop debugging flaky browser tests by hand
every single morning before standup.
```

`.vtt` is the same cues with a `WEBVTT` header line, a blank line, and `.`
in place of `,` before the milliseconds. Both files are UTF-8 with `\n`
line ends.

`chapters.txt`:

```text
0:00 Demo Launch
0:12 The demo
0:41 Get started
```

The first chapter is always `0:00`. Its title is the scene `title`, else
`meta.title`. A film of 1 h or more uses `H:MM:SS`.

## 10. Cover and previews

```sh
ffmpeg -y -ss 2.25 -i render/master.mp4 -frames:v 1 \
  -vf "scale=1920:1080:force_original_aspect_ratio=decrease:flags=lanczos,pad=1920:1080:(ow-iw)/2:(oh-ih)/2" \
  -q:v 2 render/cover.jpg
ffmpeg -y -ss 0 -t 5 -i render/master.mp4 -an -vf "scale=1280:-2:flags=lanczos" \
  -c:v libx264 -preset slow -crf 20 -pix_fmt yuv420p -movflags +faststart render/preview.mp4
ffmpeg -y -ss 0 -t 5 -i render/master.mp4 -an -filter_complex \
  "fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5" \
  -loop 0 render/preview.gif
```

- Pick `--cover-sec` on a frame where the headline is fully on screen and no
  transition is in progress. The default (75% into the hook scene) is a
  start point. Look at the frame before you ship it.
- A cover is a frame of the film. For a designed thumbnail with extra text,
  make an HTML page, render 1 frame with `--frames 1`, and convert it with
  `ffmpeg -i frame_000000.png -q:v 2 render/cover.jpg`.
- Keep `preview.gif` under about `8 MB`. Lower `fps` to `12` or width to
  `400 px` when it is larger.

## 11. Failure causes

| Symptom | Cause | Fix |
|---|---|---|
| `xfade` error "First input link ... parameters do not match" | inputs differ in size, rate, `pix_fmt` or SAR | normalize (Section 3) |
| master is `1-2` frames short | trim by seconds or a missing tail handle | trim by `end_frame`, add the handle |
| audio ends before video | `mix.wav` too short | return to video-audio |
| text edges are dark | overlay PNGs with premultiplied edges on a non-transparent body | set `background: transparent`, render with `--transparent` |
| cut lands between beats | boundary not on the grid | fix `timeline.json`, re-run the plan checker |
| `9x16` scene is letterboxed | `critical_regions` too wide for a `606 px` crop | re-render the scene at `9x16` |
