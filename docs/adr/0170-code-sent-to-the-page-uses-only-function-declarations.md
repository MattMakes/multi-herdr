# Code sent to the page uses only function declarations

The video scripts send functions to Chrome as source text, and their
self-tests run before the module-level constants are set. Such code is
written as named function declarations that use only browser globals and
each other. A reference to any module-level name breaks inside the page,
and a `const` reference throws in a self-test.

## Consequences

- `skills/video-capture/scripts/materials.mjs` `pageScript` serialises a
  fixed list of functions (`collectPage` and its helpers) with `toString`.
  Each may call only the others in that list.
- `skills/video-render/scripts/render_frames.mjs` `measureScript` sends the
  raster-probe functions. A sample is `[url, natural size, drawn CSS
  size]`, or null.
- The self-test of `skills/video-capture/scripts/capture.mjs` (a web scene
  and a terminal scene pass the plate gate, `console.error` fails, 3 runs
  match) runs before the module-level constants are set. Its captures run
  in parallel, each web run in its own project folder. Parallel runs also
  load the host, so the 3 identical runs check determinism under load.
- The self-test of `materials.mjs` uses a local fixture page and no
  network. The mode dispatch at the top runs before any constant further
  down is set.
