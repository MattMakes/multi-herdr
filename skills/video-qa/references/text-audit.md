# Text audit over time

`scripts/text_audit.mjs` checks every text item that a scene shows, at every
`0.1 s` of the scene. It loads 1 HTML scene in headless Chrome, calls
`window.__seekToFrame` at each sample time, and reads each visible text item.
It needs Node and Google Chrome. It downloads nothing.

## Run it

Run it once for each HTML scene, from the video project root:

```bash
node "<skill-dir>/scripts/text_audit.mjs" \
  --scene scenes/s01-hook/index.html --out qa/text-audit-s01-hook.json
```

| Flag | Default | Use |
| :--- | :--- | :--- |
| `--scene` | required | Scene HTML file or URL |
| `--out` | not set | Write the JSON report to this path |
| `--duration` | `window.__videoMeta.duration` | Scene length in seconds |
| `--fps` | `window.__videoMeta.fps`, then `30` | Frame rate for `__seekToFrame` |
| `--width`, `--height` | `window.__videoMeta`, then `1920x1080` | Viewport size |
| `--step` | `0.1` | Seconds between samples |
| `--min-phone-px` | `8` | Smallest font size in px at 360 px phone width |

Exit codes: `0` no finding, `1` at least 1 finding or a scene failure, `2` a
usage error or no Chrome. Set `CHROME_PATH` when Chrome is not at
`/Applications/Google Chrome.app`.

Self-test (rules on synthetic samples, the exit code without Chrome, and 3
fixture scenes in `scripts/fixtures/text-audit/`):

```bash
node "<skill-dir>/scripts/text_audit.mjs" --self-test
```

## What counts as a text item

- A DOM text node. The box is the union of its line boxes from a `Range`. The
  font size is the computed `font-size` times the CSS scale of its element.
- Canvas text. A wrapper around `fillText` and `strokeText` is installed
  before the scene loads. It records the text, the box from `measureText` and
  the canvas transform, and the font size from `ctx.font`. A `clearRect` or
  `fillRect` that covers the box removes the text.

An item is visible when its opacity (times all ancestor opacities) is above
`0.05`, `visibility` is `visible`, and part of its box is inside the frame. An
ancestor with `overflow` other than `visible` clips the box, so a mask reveal
hides its text. An ancestor box that holds the whole frame does not clip.

An item keeps 1 identity while its digits change, so a counter from `0` to
`1,234` is 1 item.

## The 6 rules

| Rule | Fails when |
| :--- | :--- |
| `too_small` | Font size x `360 / width` is below `--min-phone-px`. At `1920` wide, `8 px` needs a `43 px` font. At `1080` wide, it needs `24 px`. |
| `too_short` | 1 continuous showing is shorter than `0.4 s + 0.2 s` per word. The showing time is the sample count x `--step`. |
| `overlap` | 2 items with different words cover at least `10 %` of the smaller line box. The same words on 2 layers (glow, outline) do not count. |
| `off_frame` | A settled box goes more than `1 px` past a frame edge. |
| `safe_zone` | A settled box goes below `82 %` of the height (the caption zone), or, in a portrait frame, right of `85 %` of the width (the app zone). These are the zones of `qa_video.py` check 11. |
| `placeholder` | The text shows `NaN`, `undefined` or `[object Object]`. |

Settled means the box moved at most `1 px` since the previous sample. A
slide-in that passes the edge or the caption zone does not fail. Text that
rests there fails.

Each rule reports an item once (`too_short` reports each short showing). The
first time it fails is `time_sec`.

## The report

`--out` writes `passed`, the frame size, `samples`, `text_items`,
`failed_rules` and `findings`. Each finding has `rule`, `text`, `time_sec`,
`detail` and `owner`. The owner is always `video-animator`.

Put each finding in the critique report as 1 fix line. Add the scene
`start_sec` to `time_sec` to get the time in the master.

## Limits

- Text inside an image, a video or a WebGL canvas is not seen.
- Canvas text is clipped to its canvas. A canvas text box that a
  semi-transparent `fillRect` covers is counted as hidden.
- A canvas that is cleared by a size change and not by `clearRect` keeps its
  old text. Scenes that follow the seek contract redraw with `clearRect`.
