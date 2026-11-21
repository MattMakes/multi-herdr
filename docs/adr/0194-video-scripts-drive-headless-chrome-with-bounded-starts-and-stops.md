# Video scripts drive headless Chrome with bounded starts and stops

The video skills (`skills/video-capture/scripts/capture.mjs`,
`skills/video-capture/scripts/materials.mjs`,
`skills/video-render/scripts/render_frames.mjs`,
`skills/video-qa/scripts/text_audit.mjs`) and
`skills/one-page-design/scripts/check_page.py` drive headless Chrome over
CDP with Node built-ins. Every start and stop is bounded and retried,
because Chrome can hang in each of them, and the launch flags remove the
known sources of pixel drift between runs.

## Consequences

- Chrome can deadlock in its own start-up fork, before it prints the
  DevTools URL. `capture.mjs` gives each start 15 s, then kills that Chrome
  and its child processes and starts again, 3 tries in total.
  `materials.mjs` `retryChrome` does the same with 15 s per start.
  `render_frames.mjs` gives each start `CHROME_START_MS`, `CHROME_TRIES`
  times in all.
- Chrome's helpers (network service, GPU, renderers) outlive the main
  process for some ms, and the network service writes its cache index into
  the user-data dir as it exits. `capture.mjs` and `render_frames.mjs`
  `stopChrome` and `text_audit.mjs` poll up to 5 s for them, then SIGKILL
  the rest, before the dir is removed.
- `render_frames.mjs` `chromeArgs` sets software raster, a fixed device
  scale, sRGB output, grayscale font antialiasing and no background
  throttling.
- A window below Chrome's minimum size can stall `Page.captureScreenshot`
  for good. The window never goes below `WINDOW_MIN` (800x600), and
  `Emulation.setDeviceMetricsOverride` sets the viewport.
- `--allow-file-access-from-files` lets a `file://` scene read its own
  image pixels. Without it the canvas is tainted and the pre-upscale check
  cannot run. It has no effect on http pages.
- On macOS, Chrome copies its app into a `code_sign_clone` folder at each
  start. A Chrome killed during its start leaves that copy on the disk,
  and macOS records each copy as 1 more "Google Chrome" in its privacy
  lists. `render_frames.mjs` sets `NO_CODE_SIGN_CLONE`, and the
  `check_page.py` preview passes
  `--disable-features=MacAppCodeSignClone`, to stop the copy.
- `render_frames.mjs` `openPage` uses the tab Chrome opened at launch. A
  tab from `Target.createTarget` can stay in the background, where it stops
  producing frames and `Page.captureScreenshot` never returns.
- `materials.mjs` caps full-page screenshots at `MAX_SHOT_HEIGHT` = 16384
  px, because Chrome cannot capture a taller image.
- Reading `cssRules` of a cross-origin style sheet throws in the browser.
  `materials.mjs` `readRules` catches it and treats the sheet as having no
  readable rules.
