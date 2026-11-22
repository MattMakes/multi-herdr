---
name: video-motion-craft
description: Use when the motion of a video scene must look made by a trained animator, or when a render looks generic, robotic, cheap or machine-made - choosing a motion personality, durations by size and distance, an easing by role, springs from overshoot and settle time, anticipation, overlap, arcs, stagger and hold budgets, camera moves, transitions planned on both sides of a cut, blur grammar, and a tell-by-tell audit that replaces template motion with designed motion.
---

# Motion craft: timing and easing like a trained animator

The `video-motion` skill makes a scene deterministic. This skill makes it good. It gives the numbers a motion designer carries in the hands, and an audit that finds the marks of template motion.

A scene still obeys the seek contract of `video-motion`: every value is a function of `t`. A spring is a closed-form curve, not a simulation. A "random" offset comes from a seeded generator.

When `MOTION.md` or `BRAND.md` holds a token, the token wins. Use these numbers to write tokens that do not exist yet, and record each one in `MOTION.md` before you use it twice.

## Order of work

1. **Feeling.** Write 1 line: what the viewer should feel at this beat (trust, speed, delight, tension). Every later choice serves it.
2. **Personality.** Pick 1 row of the personality table for the whole film. About 90 % of moves use it. 1 signature easing carries about 80 % of moves.
3. **Hero.** Name the 1 element that leads each beat. It moves first or last, moves farthest, and nothing competes with it.
4. **Timing.** Give each move a duration by size and distance, then an easing by role. Tables: `references/timing-and-easing.md`.
5. **Layers.** Primary motion at full strength. A secondary layer at 30 to 50 % of its amplitude, 50 to 100 ms later, on a different curve. An ambient layer at 10 to 20 %. Not every beat needs all 3: a dense product shot needs the primary only.
6. **Budgets.** Check the stagger, overshoot, hold and "how much moves at once" budgets below.
7. **Cuts.** Plan each transition as 1 row with both sides: `references/transitions-and-camera.md`.
8. **Audit.** Run `references/slop-audit.md` on the contact sheet and on a half-speed play. Fix in this order: truth, idea, connections between shots, hierarchy, timing, finish, sound.

## Personality

| Personality | Duration of a standard move | Curve | Overshoot | Entrance |
|---|---|---|---|---|
| Calm, premium | 350 to 600 ms | `cubic-bezier(0.4, 0, 0.2, 1)` | 0 % | fade with scale 0.98 to 1 |
| Clear, corporate | 200 to 400 ms | `cubic-bezier(0.2, 0, 0, 1)` | 0 to 3 % | slide with opacity |
| Playful | 150 to 300 ms | `cubic-bezier(0.34, 1.56, 0.64, 1)` | 10 to 20 % | rise with a small bounce |
| Energetic | 100 to 250 ms | `cubic-bezier(0.16, 1, 0.3, 1)` | 15 to 30 % | snap in from an edge |

Each personality has 3 durations (quick, standard, slow) and 1 entrance pattern. Write them as tokens.

## The measured defaults

Counts from frame-by-frame studies of professional films disagree with what a template gives. Use these as the base and add character on top.

- Most moves are short: about 0.33 s is typical, with half of all moves between 0.2 s and 0.8 s. Opacity changes take about 0.2 s.
- A thing that arrives slows down. A thing that leaves speeds up and is gone on the cut. An exit takes 65 to 75 % of its entrance.
- A hero entrance covers more than half its travel in the first 0.13 s, then settles over 0.4 to 0.6 s.
- A hero arrives from larger than life and comes down: a logo from about 3.5 times its size, a calm mark from about 1.9 times. Growing from nothing is the weaker choice.
- Overshoot is rare: under 1 move in 10. When used it is late and small. Keep 1 visible spring per film.
- Transforms carry entrances. Opacity only helps for 4 to 6 frames. Whole scenes do not crossfade.
- Things enter as whole words and whole objects. Letter-by-letter motion is for a title of 1 to 4 words, if at all.
- A hard 1-frame step is a valid move: it reads as a change of state.

## Budgets

| Budget | Limit |
|---|---|
| Moving at once | At most 1/3 of the visible elements, and at most 2 or 3 that the eye must follow. Ambient motion does not count. |
| Travel | No move crosses more than 1/3 of the frame without a keyframe between. |
| Stagger total | Under 500 ms for a group. Under about 600 ms a wave reads as 1 gesture. |
| Stagger step | 20 to 40 ms tight, 50 to 100 ms standard, 100 to 200 ms dramatic. Uneven steps look hand-made. |
| Overlap | The next item starts when the one before is about half done. |
| Overshoot | Success 5 to 10 %. Feedback 2 to 5 %. Celebration 15 to 25 %. Error and premium 0 %. Over 25 % looks broken. |
| Reading hold | 0.45 s minimum. 0.2 s per word for 2 to 5 words. 1.7 to 2.0 s for 6 to 8 words on 2 lines. |
| Stillness | A hold longer than 3 frames keeps 1 thing alive: a 1 to 3 px drift per frame, a 0.5 to 1 % scale creep, a sweep or a caret. |
| Breath | 100 to 200 ms of calm after a group settles. A designed pause of 0.6 to 1.5 s only before a payoff. |
| End hold | 0.6 to 1.4 s, and the last shot is at most 1/4 of the film. |

## The principles, with amounts

- **Anticipation.** 100 to 200 ms, 10 to 20 % of the main move, the other way. Skip it on a move shorter than 150 ms.
- **Follow-through.** A child lags its parent by 50 to 150 ms. Trailing parts stop 100 to 200 ms later.
- **Arcs.** Offset the middle of a path by 10 to 20 px across the direction of travel. 5 px for a sober film.
- **Squash and stretch.** Scale to about 1.2 by 0.8 on impact for 2 to 4 frames and recover in 4 to 8. Keep the area constant.
- **Staging.** Dim the rest to 40 to 60 %. The hero lands 100 to 200 ms after its supports.
- **Counter-motion.** The background moves against the hero at 20 to 30 % of its speed.
- **Weight.** Heavy: 300 to 500 ms, no overshoot. Light: 80 to 200 ms, 5 to 15 %.
- **Depth.** Parallax speeds of 1.0, 0.5, 0.2 and 0.1 from front to back.

## Gates

1. `scan_linear_motion.py` of `video-motion` exits 0.
2. Count the distinct `(duration, ease)` pairs in the scene. 1 pair for everything fails the audit; so do more than about 6.
3. Count overshooting eases (`back`, `elastic`, a bezier with a control point above 1). More than 1 visible spring per film needs a reason in `MOTION.md`.
4. For every group, stagger step times item count is under the stagger total.
5. Render the contact sheet at the reading size. Write 1 sentence per frame that names its hero. A frame with no hero, or with 2, fails.
6. Every audit row of `references/slop-audit.md` is "clear" or has a line in `MOTION.md` that says why the film keeps it.

Report the gate results in `DONE:` with the counts.

## References

- `references/timing-and-easing.md`: durations, the easing catalogue, springs in closed form, words, counters, presses, ambient loops.
- `references/transitions-and-camera.md`: the 2-sided transition row, transition kinds, camera moves, blur grammar, frame clocks.
- `references/slop-audit.md`: the tells of template motion and layout, each with its fix, and the troubleshooting table.
