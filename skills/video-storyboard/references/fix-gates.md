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
