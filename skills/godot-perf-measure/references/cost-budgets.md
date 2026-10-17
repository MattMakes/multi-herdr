# Per-feature cost budgets

Use this when a new effect, cue or system must stay under a budget per frame (for example 0.1 ms).

1. Agree on the budget in milliseconds per frame before you write the feature.
2. Write a headless test that drives the worst case (for example the most instances that can
   appear in one frame).
3. Time only your part of the frame, not the whole frame.
4. Take the best of 3 to 5 runs inside the test. One run on a loaded machine is noise.
5. Over budget, time each part apart. Then remove, cap, cache or slice the costly part over
   frames. Redraw only on a state change.
6. Wait on timers, not on frame counts, in headless tests. Headless frames run faster than real
   time.
7. Record the final cost in the notes.

For a draw-call budget on new models, record the draw-call and CPU delta per model against the
base scene. Merge static meshes into one batch when a model goes over, then take a second
measure. Write a before-and-after table.
