# Fix a failing plan gate

Each line names a `rule` of `check_timeline.py` and the usual fix.

- `hook_window`: cut the hook text to 3 to 5 words, or set
  `motion.reveal_sec` lower and make the motion land sooner.
- `scene_duration`: split a long scene at a beat, or merge a short one
  into its neighbor. Use an override reason only for a deliberate choice.
- `caption_density`: the caption is a headline, not the narration. Cut
  it to the 1 idea of the scene.
- `caption_place`: move `caption_box` into the safe band for the aspect.
- `speaking_rate`: cut words before you add seconds. 2.2 words/s sounds
  calm. 2.7 words/s is the limit, not the goal.
- `visual_variety`: change the framing (zoom, new plate, new template)
  or move the focal point to the other third of the frame.
- `safe_zone`: move content up, or for `9:16` move it left. Captions own
  the bottom 18%.
- `contiguous_timing`: recompute every `start_sec` from scene 1 in order.
  Do not edit 1 start by hand.
- `sync`: move the cut or locked action to the nearest beat in the grid.
- `read_hold`: cut words, land the motion sooner (`reveal_sec`), or give
  the scene more time.
- `terms`: say the term in the narration and put the same words on screen.
- `event_gap`: add an event (a reveal, a cut, a hit), or declare the
  stretch a `stop` when the stillness is the point.
- `hero`: move the payoff into the middle part of the film, and end the
  breakdown before the hero starts.
- `menu_time`: shorten UI and menu scenes, or replace them with product
  footage that proves the same point.
- `directions`: see "Three directions" in `video-direction`.
- `materials`: ask the orchestrator to run `materials.mjs` of
  `video-capture` on the product page, or fix the list with its `--check`.
  A capture scene shows the real product, so it is rebuilt from captured
  material, never invented.
- `schema`, `meta.format must be "short" or "long"`: copy the `Format:`
  line of `BRIEF.md` into `meta.format`. Ask the producer when it is missing.
- `format`, `short: meta.duration_sec ... 15 s to 90 s`: cut scenes to 90 s
  or less. A film of 3 min or more is `long`. Ask the producer.
- `format`, `short: meta.chapters must be absent`: delete `meta.chapters`.
- `format`, `long: meta.duration_sec ... 180 s to 900 s`: add chapters to
  reach 180 s, or ask the producer to change the film to `short`.
- `format`, `long: meta.aspect`: set `meta.aspect` to `16:9` and the frame
  to 1920x1080. Plan a vertical cut as a separate `short` film.
- `format`, `long: meta.end_screen_sec`: set it from 5 to 20. Keep the
  last scene clear for the end-screen elements.
- `format`, `long: meta.chapters must be a list`: give each chapter `id`,
  `title`, `hook` (non-empty strings) and a number `start_sec`.
- `format`, `long: meta.chapters has N chapters`: split the film into 3
  chapters or more.
- `format`, `long: meta.chapters[0].start_sec must be 0`: start the first
  chapter at 0.
- `format`, `long: meta.chapters[N].start_sec ... greater than`: sort the
  chapters by `start_sec` and remove duplicate starts.
- `format`, `long: chapter ... runs`: split a chapter over 180 s at a payoff,
  or merge a chapter under 30 s into its neighbor.
