# The audit: template motion and how to replace it

Run this on the contact sheet, on the scene code, and on a play at half speed. Mark each row "clear" or "found". Fix every "found" row, or write in `MOTION.md` why the film keeps it.

A film can pass every numeric gate and still look machine-made. The marks below are what a viewer reads as "nobody designed this".

## Motion tells

| Tell | Fix |
|---|---|
| 1 duration and 1 ease on every move | Set each move by role from the tables. Keep 3 durations and 1 signature ease per film. |
| Everything fades in from opacity 0 with a small rise | Let transforms carry the entrance. Each element enters in a way that fits what it is. |
| A bounce or back curve on every pop | 1 visible spring per film. |
| The same stagger step everywhere (often 100 ms) | Tight, uneven steps; total under 500 ms. |
| A logo that grows from nothing | It arrives from larger and lands. |
| A slow zoom on every scene | Lock off most shots; move for a reason. |
| An exit that is the entrance played backwards | An exit is shorter, accelerates and ends on the cut. |
| Linear motion on visible objects | Ease it, or mark why it is constant. |
| Things start and stop as 1 block | Overlap parts by 50 to 150 ms. |
| Straight paths for living things | Add an arc. |
| Frozen holds | 1 living thing per hold. |
| A sound hit on every cut over a loop | 1 to 3 placed hits, with quiet before each. |
| Letters that type on with a caret for a headline | Whole words. A caret belongs in a terminal. |
| A showreel of techniques, 1 per scene | 1 motion idea per film, varied. |

## Layout and look tells

| Tell | Fix |
|---|---|
| Page furniture: a brand name in a corner, tracked capitals, a scene counter, a timecode, brackets, a progress bar | Remove it. A film is not a slide. |
| Slide layout: headline left, sub-line under it, a kicker above | Stage 1 thing per frame, large. |
| A big number over a small label, centred | Show the thing the number is about. Never invent a number. |
| A rounded card with a status dot that stands for "the product" | Use the real interface from `materials/`. |
| A dark ground with a radial glow, a gold spark and an italic serif accent | Take ground, light and type from the brand. |
| Glow as wallpaper | Light has a source and a reason. |
| The accent colour on everything | 1 hue with 1 meaning. It may tint an arrival for 1 to 6 frames, then settle to ink. |
| Bold for emphasis | Emphasis by position, size or timing. |
| Pure black | A dark from the brand. |
| A footer ending: logo, tagline, button and address in 1 frame, held for seconds | 1 thing, held 0.6 to 1.4 s. |
| A toolbar pinned across 3 or more shots | The frame changes with the story. |
| A small interface on an empty dark ground, called minimal | Fill the frame with the subject. |

## The second set

A builder who was told "be bold" leaves the first set and lands in a second one. These are also tells:

- 1 giant word cropped by the frame;
- a band of scrolling text;
- a texture or a video inside letters;
- a circle that wipes the scene;
- a dot that turns into the logo, or a chain of "A becomes B becomes the logo".

Each can be right once, when the brand's own shapes ask for it. As a default, each is a template.

## Checks that a script can make

- Count the distinct pairs of duration and ease in the scene code.
- Count overshooting eases.
- Find every `opacity` tween from 0 whose element has no transform tween in the same 100 ms.
- Find runs of more than 3 frames with no pixel change in the contact frames.
- Measure the end hold and the share of the last shot.
- Search the scene text for counters such as `01 /`, tracked capital labels and timecodes.

## The order of fixes

When several things are wrong, fix them in this order. A later fix does not repair an earlier fault.

1. **Truth.** The film shows the real product and makes claims that the claims ledger holds.
2. **Idea.** Cover the logo. If the film could be for any product, the idea is missing.
3. **Connections.** Each shot hands something to the next.
4. **Hierarchy.** 1 hero per frame.
5. **Timing.** Durations, eases, holds.
6. **Finish.** Blur, grain, light.
7. **Sound.** Hits on events.

## Troubleshooting by symptom

| It looks | Cause | Do |
|---|---|---|
| Robotic | Linear or uniform moves, straight paths, no offsets | Ease by role, add 10 to 20 px arcs, offset parts by 50 to 150 ms |
| Cheap or flat | Only a primary layer; no weight | Add a secondary layer, 3 to 10 % overshoot on 1 element, a shadow that reacts |
| Busy | Too much moves at once | Apply the 1/3 rule; add 100 to 200 ms breaths; dim the rest |
| Slow | In-out curves on arrivals; long anticipation | Ease out on arrivals; shorten or drop anticipation |
| Nervous | Short moves with no holds | Hold after each group; lengthen the hero's tail |
| Floaty | Long ease-outs with no clear landing | A faster start and a firm end; a small settle |
| Generic | The tells above | The audit |

## What a critic writes

A fix is useful when it holds 4 things: the timecode, what is seen, why it hurts the film, and how to change it in numbers (a curve, frames, pixels, a colour). "Make it bouncier" is not a fix. "At 3.2 s the card lands with no settle: add a 4 % overshoot that peaks at 70 % of 15 frames" is.
