---
name: video-capture
description: Use when a video project's timeline.json has capture scenes and needs deterministic browser or terminal footage. Drives headless Chrome over the DevTools Protocol at the target size, 60 fps and deviceScaleFactor 2 with a locked clock, seeded random, local fonts and mocked network; choreographs a spline cursor, click ripples and human-cadence typing; writes captures/<scene-id>.mp4 plus captures/<scene-id>.json telemetry (cursor, clicks, keys, zoom_keyframes, speed_ramps); speed-ramps dead time of 500 ms or more; proves each plate with ffprobe and ffmpeg gates; and captures a product page's colours, fonts, logos, screenshots and wording into materials/ with a source list.
---

# Deterministic browser and terminal capture

Record product UI and CLI walkthroughs as plates that are the same on every
run, and record every interaction as telemetry. The audio phase places click
and key sounds from the telemetry. The editor applies camera zoom from it.
This skill covers CDP, terminal, and desktop app or engine capture.
For app and engine staging, zoom plans, forced timers, sidecars, and freeze checks, read
`<skill-dir>/references/app-engine-capture.md`.

`<skill-dir>` means the directory where this SKILL.md sits. The harness and the
plate gate are in `<skill-dir>/scripts/`. Run them from there. Do not copy
them. `<skill-dir>/references/cdp-harness.md` explains the harness, the take
API (`h`) and the gotchas.

## Inputs and outputs

- Input: `timeline.json` (`meta.fps`, `meta.width`, `meta.height`, and per
  scene `id`, `kind`, `duration_sec`), the capture notes in `STORYBOARD.md`
  (URL or command, start state, actions), and the app or CLI under test.
- Output, per `capture` scene: `captures/<scene-id>.mp4`,
  `captures/<scene-id>.json`, and the take script
  `captures/src/<scene-id>.mjs`. The scene's `plate` in `timeline.json` is
  `captures/<scene-id>.mp4`.
- Brand materials, when the task asks for them: `materials/` (see below).
- Do not write outside `captures/` and `materials/`. Do not edit `timeline.json`. If a take
  needs a different duration, tell the director the measured length.

## Tools

Chrome at `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`
(`CHROME_PATH` overrides it), Node built-ins, `ffmpeg` and `ffprobe`. Do not
install packages. Do not import `playwright` or `playwright-core`: the
harness drives Chrome through the DevTools Protocol (CDP) directly.

## Workflow

1. Read the capture scenes in `timeline.json` and their notes in
   `STORYBOARD.md`. List the actions of each scene with a time budget.
2. Write `captures/src/<scene-id>.mjs`: a `config` object and 1 async
   function that calls the `h` helpers. Start from
   `<skill-dir>/scripts/example-take.mjs`. Record fixtures for every API call.
3. Run `node "<skill-dir>/scripts/capture.mjs" . <scene-id>` from the project
   root. It exits 1 on any uncaught page error, `console.error` or blocked
   request.
4. Run the plate gate below. Look at frame 0, the middle frame and the last
   frame as PNGs.
5. Fix and run again until the gate passes. Then report the plates.

If the harness fails for a reason that is not the take, run
`node "<skill-dir>/scripts/capture.mjs" --self-test` (about 10 s). Add
`--timings` to print the real time of each phase. A missing Chrome binary exits 2
with "Chrome not found at <path>. Set CHROME_PATH or install Google Chrome."

## Rule 1: size, rate and output

- Viewport is the timeline size: `1920x1080`, `1080x1920` or `1080x1080`.
  Use `deviceScaleFactor: 2` (default) so a 1920x1080 scene gives a
  3840x2160 plate. The editor can then zoom to 2x and stay sharp. Use
  `deviceScaleFactor: 1` only for full-frame terminal scenes with no zoom.
- Constant 60 fps (or `meta.fps`). The harness steps a virtual clock by
  exactly 1 frame per screenshot, so a slow machine never drops a frame.
- Output is H.264 `yuv420p`, CRF 14, constant frame rate, into
  `captures/<scene-id>.mp4`. No audio track.
- No browser chrome, no OS scrollbars, no native cursor in any frame.

## Rule 2: stabilize before frame 0

Every source of change that is not the take script is locked before the
first frame:

- Clock: `Date` is fixed at `config.clockIso`. `setTimeout`, `setInterval`,
  `requestAnimationFrame` and `performance.now` run on the virtual clock.
  This is the CDP equivalent of `page.clock.install({ time })`.
- Random: `Math.random` is a seeded PRNG (`config.seed`, default 1).
- Fonts: fonts load from local files only (`@font-face` with a local
  `url()`). `goto` awaits `document.fonts.ready` and decodes every image.
- Network: every request is intercepted. API calls get fixtures from
  `config.fixtures`. Only `file:`, `data:` and `config.allow` prefixes (a
  local dev server) pass. Any live request fails the take.
- CSS animations: the real animation clock is stopped, and each frame moves
  every running animation by exactly 1 frame.
- Caret: `caret-color: transparent` on every element.
- Scrollbars: hidden with `--hide-scrollbars` and CSS.
- Cursor: `cursor: none` on every element. A vector cursor overlay at the
  top z-index draws the pointer.
- Timezone `UTC` and locale `en-US`, unless the brief says otherwise.

## Rule 3: choreograph like a person

- Cursor moves follow a cubic Bezier path with a small seeded bow, eased in
  and out, 400-700 ms per move. Never teleport the cursor.
- Every click draws a ring that expands over 240 ms and fades out, then
  holds 240 ms so the viewer sees it.
- Typing runs at 35-55 ms per character with seeded variance. Pause 300-600
  ms (`h.hold`) before an important click or after a result appears.
- Keep 1 action in focus at a time. Move to the target, act, let the result
  land, then move on.
- Terminal walkthroughs: record the real command once with
  `<skill-dir>/scripts/rec-cast.mjs` into an asciicast v2 file, then replay
  it in `h.termUrl` (`term.html`) with `h.termType` and `h.termCast`.
  Print the prompt before the first frame so frame 0 is not blank. An
  existing `asciinema` `.cast` file replays the same way. A live `ttyd` page is not deterministic. Use it only
  when the operator accepts takes that differ between runs.
- All on-screen text is HTML in the page. Never burn text into a plate with
  ffmpeg: the local ffmpeg has no `drawtext`, `subtitles` or `ass` filter.

## Rule 4: zoom and pan keyframes

The plate is never zoomed. The take script records camera keyframes in
`zoom_keyframes`, and the compositor or scene renderer applies them to the
HiDPI plate. Zoom to the active input before typing and back to 1.0x after
the result:

- `h.zoomTo('#search', 1.6)` records 1.0x to 1.6x over 500 ms with
  `cubic-bezier(0.22, 1, 0.36, 1)`, centred on the element.
- Keep scale at 2.0x or less: the plate has 2x pixels.

## Rule 5: speed-ramp dead time

Any wait with no input of 500 ms or longer (a load, a build, a spinner) goes
through `h.wait(ms)` or `h.wait('<js predicate>')`. The harness speed-ramps
it in the plate and records it in `speed_ramps`:

| Dead span | Factor | Ease-in pad and ease-out pad |
|---|---|---|
| 500 ms to under 1 s | 2x | 150 ms |
| 1 s to under 2 s | 3x | 200 ms |
| 2 s or more | 4x | 250 ms |

The speed rises from 1x to the factor over the pad before the ramp, holds,
and falls back to 1x over the pad after it, so the interaction before and
after stays at real speed. Waits under 500 ms play at 1x. Never use
`h.hold` for dead time.

## Telemetry: `captures/<scene-id>.json`

The harness writes this file. `video-audio` and `video-editor` read it. All
times are seconds from the first frame of the plate, after speed ramps. All
`x` and `y` values are plate pixels (CSS px times `deviceScaleFactor`). `cx`
and `cy` are normalized: 0.0 is the left or top edge, 1.0 is the right or
bottom edge.

```json
{
  "scene_id": "s02-demo",
  "fps": 60,
  "width": 3840,
  "height": 2160,
  "duration_sec": 6.0,
  "cursor": [{ "t_sec": 0.4167, "x": 1840, "y": 990 }],
  "clicks": [{ "t_sec": 0.7167, "x": 1920, "y": 996, "button": "left" }],
  "keys": [{ "t_sec": 0.95, "key": "f" }],
  "zoom_keyframes": [
    { "t_sec": 0.0, "scale": 1.0, "cx": 0.5, "cy": 0.5, "ease": "linear" },
    { "t_sec": 1.45, "scale": 1.6, "cx": 0.5, "cy": 0.4611, "ease": "cubic-bezier(0.22, 1, 0.36, 1)" }
  ],
  "speed_ramps": [
    { "in_sec": 2.1167, "out_sec": 2.6667, "factor": 3, "pad_in_sec": 0.2, "pad_out_sec": 0.2 }
  ]
}
```

| Field | Type | Meaning |
|---|---|---|
| `scene_id`, `fps`, `width`, `height`, `duration_sec` | str, int, int, int, float | Plate identity. `duration_sec` is frames divided by `fps`. |
| `cursor[]` | `{ t_sec: float, x: int, y: int }` | 1 sample per frame while the cursor moves. |
| `clicks[]` | `{ t_sec: float, x: int, y: int, button: "left" \| "right" }` | Press time and position. |
| `keys[]` | `{ t_sec: float, key: str }` | 1 entry per character or named key (`Enter`). |
| `zoom_keyframes[]` | `{ t_sec, scale, cx, cy: float, ease: str }` | Camera state at `t_sec`. `ease` shapes the segment that ends at this keyframe. The first keyframe is at 0.0 s. |
| `speed_ramps[]` | `{ in_sec, out_sec, factor, pad_in_sec, pad_out_sec: float }` | Plate span that plays fast. The pads are inside `in_sec` to `out_sec`. |

## Plate gate

Run from the project root for each capture scene. It passes when the capture
command exited 0 and this command exits 0:

```sh
python3 "<skill-dir>/scripts/check_plate.py" <scene-id>
```

It prints 1 JSON line: `{"scene_id", "duration_sec", "fails": [...]}`.

| Gate | Pass rule |
|---|---|
| G1 stream | `ffprobe` finds a video stream at `meta.fps`, at 1x or 2x the timeline size, equal to the telemetry size. |
| G2 duration | Plate duration is the scene `duration_sec` within +-0.05 s. |
| G3 frame 0 | Luma range (`YMAX - YMIN`) of frame 0 is 24 or more: fonts and UI are drawn, not a blank page. |
| G4 errors | The capture command exits 0: no uncaught `pageerror`, no `console.error`, no blocked live request. |

Also run the take 2 times and compare `ffmpeg -i <plate> -f framemd5 -`
output. Equal hashes prove the take is deterministic.

## Brand and material capture

When the brief names a product site, capture its materials before you
write a take, so the interface is rebuilt from them and not invented:

```sh
node "<skill-dir>/scripts/materials.mjs" . <url>
node "<skill-dir>/scripts/materials.mjs" --check .
```

It writes `materials/` with `first-screen.png`, `full-page.png`,
`colors.json`, `fonts.json`, `wording.md`, `logo-<n>.<ext>` and
`materials.md`, which lists every file with its source URL. A local URL
(`file:`, `localhost`, `127.0.0.1`) needs no flag. Any other site needs
`--allow-network`: pass it only when the operator allows that site in the
task. Without the flag, requests to other hosts are blocked and listed.
`<skill-dir>/references/materials.md` explains each file and the gotchas.

## Hand-off

Report to the orchestrator: each plate path, its gate JSON line, the
telemetry counts (clicks, keys, zoom keyframes, speed ramps), and any scene
whose take does not fit its `duration_sec`, with the measured length. When
you captured materials, also report the `materials.md` path and the
`--check` JSON line.
