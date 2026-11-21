# Video frames and audio are computed from exact time

A video frame is a pure function of its time, so frames can be rendered
in any order and every run gives the same pixels.
`skills/video-motion/scripts/seek-timeline.js` is a paused, seekable
timeline with no imports, no network and no clock. It is loaded with a
plain `<script>` tag and sets the browser global `SeekTimeline`.
`seek(t)` computes every animated property from `t` alone and writes it.

## Consequences

- `seek-timeline.js` has spring presets by role: ease `spring-ui`,
  `spring-camera`, `spring-heavy` or `spring-playful`. A tween with a
  preset and no duration takes the preset's duration.
  `SeekTimeline.springs` lists the preset numbers. A scene sets
  `window.__seekToFrame = function (frameIndex, fps) { tl.seek(frameIndex /
  fps); }`.
- `skills/video-capture/scripts/capture.mjs` sets the CDP Animation
  playback rate to 0 (`Animation.setPlaybackRate`). Only the injected
  virtual page clock moves CSS animations. Chrome's real clock would make
  frames differ between runs.
- In `skills/video-render/scripts/render_frames.mjs` the shutter of frame
  k opens at k and spans 1 frame: subframe j seeks k + j / n. n is a power
  of 2 (`SUBFRAME_COUNTS` 1, 2, 4, 8), so every seek time is exact in
  binary.
- `render_frames.mjs` `padAudio` pads or cuts the audio to the exact video
  length (aresample 48000, apad, atrim to the sample count). A mux that
  stops at the first stream end cut 3 frames off the film.
- Native ffmpeg aac raised the true peak by up to 4 dB. AudioToolbox
  `aac_at` (macOS) did not. `aacArgs` uses `aac_at` when this ffmpeg lists
  it, else native aac, and loudness is gated on the encoded MP4, not on the
  source WAV.
