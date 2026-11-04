# Capture harness: how it works

The harness drives Chrome through the Chrome DevTools Protocol (CDP) with
Node built-ins only: no `playwright` import, no `npm install`. It ran on
Chrome 156, Node v26.12.1 and ffmpeg 8.1.

`<skill-dir>` is the folder that holds SKILL.md. The code is in
`<skill-dir>/scripts/`. Run the scripts from there. Do not copy them.

| File | Use |
|---|---|
| `capture.mjs` | The harness. `node "<skill-dir>/scripts/capture.mjs" <project-dir> <scene-id>`. |
| `example-take.mjs` | A web take. Copy it to `captures/src/<scene-id>.mjs` and edit it. |
| `rec-cast.mjs` | Records a real command as asciicast v2: `node "<skill-dir>/scripts/rec-cast.mjs" <out.cast> -- <command> [args...]`. |
| `term.html` | The terminal page that replays a cast. A take opens it with `h.goto(h.termUrl)`. |
| `check_plate.py` | The plate gate. `python3 "<skill-dir>/scripts/check_plate.py" <scene-id>` from the project root. |

`capture.mjs` reads `timeline.json` (`meta.fps`, `meta.width`,
`meta.height` and the scene's `duration_sec`) and the take script
`captures/src/<scene-id>.mjs`. It writes `captures/<scene-id>.mp4` and
`captures/<scene-id>.json`, and exits 1 on any page error. A 6 s take runs
in about 25 s.

## Self-tests

- `node "<skill-dir>/scripts/capture.mjs" --self-test` builds a web scene
  and a terminal scene in a temp folder. Each must pass `check_plate.py`. A
  page with `console.error` and a live request must make the capture exit
  1. 3 runs of the web scene must give identical decoded frames. A missing
  Chrome binary (`CHROME_PATH` set to a missing file) must exit 2. It runs in
  about 10 s. `--timings` prints the real time of each phase.
- `python3 "<skill-dir>/scripts/check_plate.py" --self-test` gates 6
  synthetic plates (pass at 1x and 2x, blank, short, wrong rate, wrong size)
  in under 2 s.

## How the harness works

- The prelude freezes `Date` at `clockIso` and runs `setTimeout`,
  `setInterval`, `requestAnimationFrame` and `performance.now` on a virtual
  clock. Each frame advances it by exactly `1000 / fps` ms. This replaces
  Playwright's `page.clock.install({ time })`.
- `Math.random` in the page is a seeded PRNG. The take gets the same PRNG
  as `h.rand`.
- `Fetch.enable` sends fixtures for `config.fixtures` URLs, passes `file:`,
  `data:`, `about:` and `config.allow` prefixes, and fails the take on any
  other request.
- `wait()` buffers dead-time frames, then keeps a subset on the speed curve.
  The plate is already ramped. After the take script ends, the last state
  holds until `round(duration_sec * fps)` frames.
- Frames go to ffmpeg as PNG on stdin and come out as constant-rate H.264
  `yuv420p`, CRF 14.

## Take script API (`h`)

| Call | Effect | Telemetry |
|---|---|---|
| `goto(url)` | Navigate, wait for load, `document.fonts.ready` and image decode. | - |
| `hold(ms)` | Interactive pause at 1x. | - |
| `move(x, y, ms?)` | Cubic Bezier path in CSS px, ease-in-out timing. Default `ms` is 400-700 by distance. | `cursor[]` per frame |
| `click(selector, button?)` | Move to the element centre, press and release, 240 ms ripple, 240 ms hold. | `cursor[]`, `clicks[]` |
| `type(text)` | Key events per character, 35-55 ms apart (seeded). | `keys[]` |
| `press(key)` | `Enter`, `Tab`, `Backspace` or `Escape`, then a 120 ms hold. | `keys[]` |
| `zoom(scale, cx?, cy?, ms?, ease?)` | Records a camera move. Default 500 ms `cubic-bezier(0.22, 1, 0.36, 1)`. The plate does not zoom. | `zoom_keyframes[]` |
| `zoomTo(selector, scale?)` | `zoom` centred on an element. Default 1.6x. | `zoom_keyframes[]` |
| `wait(ms \| jsExpression, timeoutMs?)` | Dead time with no input. 500 ms or longer is speed-ramped. | `speed_ramps[]` |
| `termUrl` | `file:` URL of `term.html`. | - |
| `term(data)` | Terminal page: print output now. | - |
| `termType(text)` | Terminal page: echo a typed command at 35-55 ms per character. | `keys[]` |
| `termCast(file)` | Terminal page: replay an asciicast v2 file. Gaps of 500 ms or longer go through `wait`. | `keys[]`, `speed_ramps[]` |

`config` keys: `dsf` (1 or 2, default 2), `seed` (default 1), `clockIso`
(default `2026-01-15T10:00:00Z`), `cursor` (default `true`; set `false` for
terminal scenes), `fixtures` (`{ url: { status?, json? | body?, type?, delay_ms? } }`),
`allow` (URL prefixes that may reach a local server, for example
`http://127.0.0.1:5173/`).

Speed ramp table (dead span `D` is virtual time):

| D | Factor | Pad in and pad out |
|---|---|---|
| 500 ms to under 1 s | 2x | 150 ms |
| 1 s to under 2 s | 3x | 200 ms |
| 2 s or more | 4x | 250 ms |

The speed rises from 1x to the factor over the pad in, holds, and falls back
to 1x over the pad out (smoothstep).

## Terminal scenes

Record the real command once, then replay it in `term.html`. The replay is
deterministic, and a long build becomes a ramped `wait`:

```sh
node "<skill-dir>/scripts/rec-cast.mjs" captures/src/s03-cli.cast -- sh -c 'npm test'
node "<skill-dir>/scripts/capture.mjs" . s03-cli
```

A terminal take: `goto(h.termUrl)`, `term('$ ')` before the first frame,
`termType('npm test')`, `term('\r\n')`, `termCast('captures/src/s03-cli.cast')`,
and `config.cursor: false`. `termCast` reads the path from the current
folder, so run the capture from the project root.

An existing `asciinema` `.cast` file (v2) replays the same way. A live
`ttyd` session in the browser is not deterministic: its output arrives in
real time, outside the virtual clock. Use it only when the operator accepts
a take that can differ between runs.

## Other recordings

Convert any other recording (a QuickTime screen recording) to a constant
60 fps plate before it goes into `captures/`:

```sh
ffmpeg -y -i in.mov -vsync cfr -r 60 -c:v libx264 -preset slow -crf 14 -pix_fmt yuv420p \
  -an captures/<scene-id>.mp4
```

## Gotchas

- `Animation.setPlaybackRate` 0 is required. It stops the real animation
  clock, and each frame then moves every running CSS animation by exactly 1
  frame. Without it, compositor animations (`transform`, `opacity`) drift
  by a few ms between runs and the frame hashes differ.
- Each `__cap.step` resolves only after 2 real `requestAnimationFrame`
  callbacks (the page sees only the virtual one). Without this wait, a
  screenshot under CPU load can show the state of the step before: 3 of 12
  parallel runs gave different spinner and text frames.
- A fixture request takes real time unless it has `delay_ms`. With
  `delay_ms` the harness holds the response until the page clock has moved
  that many ms, sends it, and waits until the page has taken it. A `wait()`
  predicate on the result then covers the same frame count on every run.
  `delay_ms` needs an async `fetch` or `XMLHttpRequest`: a synchronous
  request blocks the page, so keep it to a fixture with no `delay_ms`.
- A `<video>` element and a live `ttyd` page play in real time. Replace
  them with a still or a cast replay.
- ffmpeg 8.1 warns that `-vsync` is deprecated. `-fps_mode cfr` is equal.
- Do not burn text into a plate with `drawtext`: the local ffmpeg has no
  `drawtext`, `subtitles` or `ass` filter. Render text in Chrome as HTML.
- To prove a take is deterministic, run it 2 times and compare
  `ffmpeg -v error -i <plate> -f framemd5 - | grep -v '^#'`. A difference
  means a real-time source that the take does not lock.
