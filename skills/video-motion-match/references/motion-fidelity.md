# Motion fidelity: match the movement, not the pixels

`compare_frames.py` compares pictures. A rebuild that uses the project's own brand, copy and characters differs in every pixel and can still move exactly like the reference. This page is how to prove that.

## Choose the fidelity first

Write 3 lines at the top of `motion-match/<scene-id>/spec.md` before you measure anything:

- **Target:** the clip and the time range, or the element inside it.
- **Destination:** the scene id and the stack it uses. Match the stack that the project already has; do not add a library to copy a move.
- **Fidelity:** 1 of the 2 levels.

| Level | What must match | Gate |
|---|---|---|
| Motion-faithful (the default) | what triggers each move, the order, the stagger, the timing, the easing family, the travel relative to the element's size, the layout proportions | the pass numbers below |
| Pixel-faithful | all of the above, and the picture | `compare_frames.py`, with mean mismatch 2 % or less, no frame over 5 %, and rest frames 0.5 % or less |

## The motion table

`motion-spec.json` holds boxes per keyframe. Next to it, write a table with 1 row per element per property. A cell with a question mark means the measurement is not done.

| Element | Property | From, to | Start (ms) | Duration (ms) | Easing | Stagger | Source |
|---|---|---|---|---|---|---|---|
| card | translateY | 24 px, 0 | 120 | 640 | expo out | 80 ms per card | frames 4 to 23 |
| card | opacity | 0, 1 | 120 | 200 | linear | same | frames 4 to 10 |

Where the numbers come from, best first:

1. The source itself, when you have it: an animation file or the page's own keyframes and timings.
2. A fit to positions that you tracked on every frame.
3. Reading frames by eye.

Reading frames:

- Extract at 25 to 30 fps for measuring. Start of a move = the first changed frame over the frame rate. Duration = (last changed frame minus first, plus 1) over the frame rate.
- A GIF is exact only to about 1 frame.
- Scale a measured pixel distance to the scene: scene pixels = clip pixels times scene width over clip width.
- Read the middle frames of a move, not only the first and the last. The middle is where the easing shows.
- Stagger is the gap between the start frames of siblings. Note whether it is constant, growing, or per word or character.

## Fit the easing

Measure the share of the travel that is done at 1/4, 1/2 and 3/4 of the move's time. Then choose:

| Observation | Family |
|---|---|
| More than half the travel done at 1/3 of the time | ease out |
| Less than a quarter done at 1/3 of the time | ease in |
| Symmetric about the middle | ease in-out |
| Equal steps | linear |
| Passes the end and returns | back or spring |

Candidates to fit against, as bezier control points:

| Family | Curves |
|---|---|
| Out | expo `(0.16, 1, 0.3, 1)`, quint `(0.22, 1, 0.36, 1)`, quart `(0.25, 1, 0.5, 1)`, cubic `(0.33, 1, 0.68, 1)`, quad `(0.5, 1, 0.89, 1)` |
| In-out | quad `(0.45, 0, 0.55, 1)`, cubic `(0.65, 0, 0.35, 1)`, quart `(0.76, 0, 0.24, 1)`, quint `(0.83, 0, 0.17, 1)`, expo `(0.87, 0, 0.13, 1)` |
| In | cubic `(0.32, 0, 0.67, 0)`, expo `(0.7, 0, 0.84, 0)` |
| Overshoot | back out `(0.34, 1.56, 0.64, 1)`, back in-out `(0.68, -0.6, 0.32, 1.6)` |
| Interface | standard `(0.4, 0, 0.2, 1)`, emphasized `(0.2, 0, 0, 1)` |

Compute each candidate at the measured times and keep the one with the smallest root-mean-square error against the measured progress. An overshoot above 2 % of the travel means a spring or a back curve, not an ease out.

A spring from 2 measurements: the overshoot gives the damping ratio (20 % gives 0.46, 10 % gives 0.59, 5 % gives 0.69), and the settle time gives the frequency: `omega = 4 / (zeta * settle_sec)`. The closed-form spring in `timing-and-easing.md` of the `video-motion-craft` skill takes those 2 numbers.

## The activity curve

An activity curve is the amount of change between each pair of neighbouring frames, over time. It shows when things move and how the speed rises and falls, and it does not depend on colour, copy or art. 2 clips with the same motion have the same curve shape.

Make it for the reference and for your render, at the same frame rate and size:

```sh
ffmpeg -i ref/clip.mp4 -vf "fps=30,scale=320:-2,format=gray,tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=ref-activity.txt" -f null -
```

Run the same command on the rendered scene. Each file holds 1 mean value per frame pair.

Compare:

- The motion window: the first and the last frame where the value is above the noise floor (take the floor from a still passage). Start, end and duration must agree.
- The peak: the frame of the largest value. An ease out peaks early, an ease in peaks late, an in-out peaks in the middle.
- The shape: plot both, or compare the normalized curves. Bars of the same shape mean the same easing.
- Shift the candidate by up to 15 frames to find the best alignment before you compare. A constant offset is a start-time error, not an easing error.
- For particles and grain, blur both clips by 4 to 8 px first.

## Pass numbers, motion-faithful

| Check | Limit |
|---|---|
| Start, end and duration of each move | within 0.08 s (2 frames at 25 fps) |
| Easing | the same family; error of the fitted curve 0.05 or less, or a mean difference of 5 % of the travel or less |
| Stagger gaps | within 0.04 s |
| Travel | within 10 % |
| Stillness | nothing moves in 1 clip while it is still in the other |

Report the numbers of the last round as a table: timing error, mismatch, easing match, pass or fail. After 5 rounds, report what remains, with numbers and the reason. Never loosen a limit quietly.

## Typical values, as a first guess

Use these to check that a measurement is plausible, not in place of one.

| Pattern | Typical |
|---|---|
| Rise with stagger | 16 to 48 px, 500 to 800 ms, expo or quint out, 60 to 150 ms steps |
| Text reveal by line | 600 to 1000 ms, 20 to 50 ms per piece, from below a clipping wrapper |
| Clip reveal | 800 to 1200 ms, in-out |
| Scale in | from 0.9 to 0.96, 150 to 250 ms, ease out, from the anchor point |
| Looping band of text | linear, 10 to 40 s per cycle |
| Parallax | speed ratios 0.2, 0.5, 1 |
| Scene change | 300 to 600 ms, the exit shorter than the entrance |

## Details that decide a match

- The transform origin. A scale from the centre and a scale from a corner are different moves.
- A clipping wrapper around text that rises into view.
- The order of transforms: move then scale differs from scale then move.
- The state before the motion starts. Frame 0 of the rebuild must already match frame 0 of the reference.
- A follower that lags a target frame by frame is stateful. In a seek-safe scene, rebuild it as a closed-form curve fitted to the tracked positions.
