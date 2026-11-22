---
name: video-manim
description: Use when a video scene explains mathematics, an algorithm, a formula, a graph, a geometric construction or a data transformation and should be animated with Manim Community - formulas that write on and transform term by term, axes and plotted functions, value trackers, step-by-step derivations, timing that hits an exact frame count and the narration's word times, deterministic renders, and delivery as a frame-exact plate for the film timeline.
---

# Manim scenes: formulas into smooth animation

Manim Community is a Python library that renders mathematical animation frame by frame. It is exact where a hand-built scene is slow: typeset formulas, term-to-term transformations, graphs and geometry. It delivers a **plate**; the `video-plates` skill holds the plate contract and the checks.

Your knowledge of the Manim API may be older or newer than the installed version. Before you write a scene, run `manim --version` and `manim render --help`, and check each class you use against the docs of that version. A name that changed fails loudly; a default that changed fails quietly.

## When to use

- The storyboard has a scene of kind `diagram` or `motion` whose subject is a formula, a proof step, a plot, a geometric figure, a sorting or graph algorithm, or a matrix or vector operation.
- An HTML diagram scene would need hand-placed glyphs or hand-computed curves.

Do not use it for product interface, brand type or character scenes. Those are HTML scenes.

## Preflight

- `manim --version` (missing: `pip install manim` in a virtual environment; it needs Python 3.9 or later, `ffmpeg`, and the Cairo and Pango libraries)
- `latex --version`, only when the scene uses typeset mathematics (missing: a TeX distribution, for example `brew install --cask mactex-no-gui`)

Report `BLOCKED:` with the failed command. Do not install anything yourself. A scene that needs only plain text and shapes can use the `Text` class and no LaTeX: say so when LaTeX is missing.

## Fit the timeline

1. Read the scene row of `timeline.json` and the word times in `audio/sync-sheet.json`. Subtract the scene `start_sec` for scene time.
2. List the beats: each has a start time, and each animation gets a `run_time`. Waits fill the gaps.
3. Every `run_time` and every wait is a whole number of frames: a multiple of `1 / fps`. Then the total is exact.
4. The total equals `duration_sec` plus handles. Keep the list in a table at the top of the scene file, and compute the waits from it; do not scatter numbers through the code.

```python
from manim import *

FPS = 60
BEATS = {            # scene time in seconds, from the sync sheet
    "claim": 0.50,
    "expand": 2.20,
    "result": 4.40,
}
TOTAL = 6.00


def frames(seconds):
    return round(seconds * FPS) / FPS


class S04Identity(Scene):
    def construct(self):
        self.clock = 0.0
        lhs = MathTex("(a+b)^2")
        rhs = MathTex("a^2", "+", "2ab", "+", "b^2")

        self.hold_until(BEATS["claim"])
        self.act(Write(lhs), 0.80)

        self.hold_until(BEATS["expand"])
        self.act(TransformMatchingTex(lhs, rhs), 1.00)

        self.hold_until(BEATS["result"])
        self.act(Indicate(rhs[2], scale_factor=1.15), 0.60)

        self.hold_until(TOTAL)

    def act(self, animation, seconds, **kwargs):
        seconds = frames(seconds)
        self.play(animation, run_time=seconds, **kwargs)
        self.clock += seconds

    def hold_until(self, at):
        gap = frames(at - self.clock)
        if gap > 0:
            self.wait(gap)
            self.clock += gap
```

`act` and `hold_until` keep a running clock, so a beat that starts late shows as a negative gap in review, not as drift.

## Motion rules in Manim

- The default rate function is a smooth in-out. Use it for transformations. For an arrival use an ease-out rate function, for an exit an ease-in. Take the feel from `MOTION.md`; the `video-motion-craft` skill gives the durations.
- Write a formula on in 0.6 to 1.2 s. Transform between 2 formulas in 0.8 to 1.2 s. Hold the result at least 0.6 s plus the reading time of what changed.
- Transform matching parts, so that a term visibly travels to where it ends. Split the formula into the parts that move. A plain transform that morphs every glyph into another reads as noise.
- 1 new thing per step. Dim what is not in play to 30 to 40 % opacity and bring it back when it matters.
- Colour carries meaning: 1 colour per quantity through the whole film, from the brand tokens. Set the background from `DIRECTION.md`, not the default black.
- For a value that drives several objects, use 1 value tracker and objects that redraw from it. Then the graph, the label and the marker cannot disagree.
- Stagger a group with a lagged start: a lag ratio of 0.1 to 0.3, at most 5 items.
- The camera moves only to follow the subject or to reveal scale; use a moving-camera scene and animate the frame. No rotation of the frame for effect.
- Text that is not mathematics uses the brand font through the `Text` class, only when that font is installed on the host. Check the render for a fallback font.

## Render

Stills first: render the last frame of the scene, and frames at each beat, and look at them.

```sh
manim render -s --resolution 1920,1080 scene.py S04Identity
```

Then the plate, as a PNG sequence, with caching off and a fixed seed:

```sh
manim render --format png --fps 60 --resolution 1920,1080 \
  --disable_caching --seed 7 --media_dir media scene.py S04Identity
```

- A PNG sequence is the lossless intermediate. Encode it with the `ffmpeg` command of the plate contract, so the film has 1 encoder and 1 set of colour tags.
- For a plate that sits over another layer, add the transparent option and keep the PNG frames.
- A draft at a low quality preset is for timing only. Never upscale a draft into the plate.
- Find where the installed version wrote the frames from its own output lines; the folder layout differs by version. Move them to `plates/<clip>/`.

## Checks

1. The 5 plate checks of `video-plates`. The frame count equals `round(TOTAL * FPS)`.
2. Beat times: for each entry in `BEATS`, the frame at that time shows the start of its animation. Put these times in the sidecar's `key_actions_sec`.
3. A formula is correct. Read every rendered formula against the script and the source it came from. A wrong sign in a film is a claims fault, not a typo.
4. Glyph size: the smallest glyph that must be read is at least 4 % of the frame height.
5. 2 renders with the same seed give the same first, middle and last frame.

## Hand-off

- `plates/<clip>.mp4` or the frame folder, and the sidecar.
- The scene source `scenes/<scene-id>/scene.py`. It is text and is committed.
- In `DONE:`: the Manim version, the LaTeX version when used, the frame count and rate, each check with its result, and the render time.

## Common mistakes

| Mistake | Result | Fix |
|---|---|---|
| A `run_time` such as 0.33 at 60 fps | The total drifts by partial frames | Round every time to a frame |
| Caching left on while iterating | A stale partial clip in the output | `--disable_caching` for the plate |
| The default background | A black scene in a light film | Set it from `DIRECTION.md` |
| 1 big formula object | The whole thing morphs | Split it into the terms that move |
| Rendering straight to a video file and re-encoding | 2 lossy encodes | PNG frames, 1 encode |
| A random layout or jitter with no seed | 2 renders differ | `--seed`, and seeded generators in the scene |
