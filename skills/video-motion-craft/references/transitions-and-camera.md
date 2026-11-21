# Transitions, camera, blur and frame clocks

## Plan both sides of a cut

A transition is designed as 1 row, not as an effect added later. Write the row in `MOTION.md` before you build either scene:

| Field | Last frame of A | First frame of B |
|---|---|---|
| Object the eye is on | | |
| Position (normalized) | | |
| Scale | | |
| Colour of object and ground | | |
| Blur | | |
| Velocity (direction and speed) | | |

The cut works when at least 2 fields match across the row, or when 1 field changes on purpose and everything else matches. When 2 animators own A and B, the one that owns `MOTION.md` writes the row and both build to it.

## Transition kinds

| Kind | How | Use |
|---|---|---|
| Hard cut on motion | Cut while both sides move the same way | the default |
| Carried object | 1 object keeps its place and size; the world changes around it | a feature leads to the next |
| Effect becomes ground | A shape grows until it is B's background | change of topic |
| Container swap | The frame of a card or window stays; its content changes in 1 frame | product walkthrough |
| Short dip | 4 frames through a neutral grey | a pause in the argument |
| Blur-matched cut | Both sides at the same blur at the cut | soft change of place |
| Feathered wipe | An edge in B's ground colour crosses in about 0.5 s; feather 1/3 of the axis | calm films |
| Clip wipe | An edge crosses through the glyphs in about 5 frames | type-led films |
| Stepped dissolve | Blocks switch in 0.3 s | pixel or retro styles |
| Dive | Zoom into a detail of A that is B | reveal of depth |
| Pull back with snap | 1-frame scale jump, then a soft landing | reveal of scale |
| Occlusion | Something crosses the lens; B is behind it | busy scenes |
| Word swap | 1 word replaces another in 1 frame | a claim changes |

Rules:

- Build a transition from the brand's own shapes, not from a stock wipe.
- No fade to black by default. No scene crossfade.
- Cut on the beat grid of `audio/beats.json`, within 2 frames at 60 fps.
- Scenes may shorten toward the climax, then the last one lingers.
- End a scene in motion when the next one continues it.

## Camera

- The camera is 1 transform on 1 group. Text is never zoomed by itself.
- A camera never overshoots. Use `travel` or sine in-out; see the easing catalogue.
- A reveal: a 1-frame scale snap to 0.56 to 0.77, then a 9 to 18 frame soft landing.
- A push scales by duration: about 1.25 times over 0.2 s, 1.4 times over 0.5 s, 1.5 times over 1 s. A slow drift may reach 2.4 times over a long shot.
- A dive zooms at a constant ratio per second: interpolate the logarithm of the scale, not the scale.
- A whip pan takes about 8 frames at 30 fps, peaks at 1/4 of its time, and cuts at peak speed.
- Parallax: near layers move 2 to 2.4 times as fast as far ones.
- A locked-off shot still drifts at 1 px per frame or more, or it reads as a still image.
- 1 continuous camera path with long eases can carry a whole explainer. Then every sentence brings 1 new thing into view.

## Blur grammar

Choose 1 per film and write it in `MOTION.md`.

| Grammar | How | Fits |
|---|---|---|
| Crisp steps | No blur; hard 1-frame changes | type-led, pixel, Swiss styles |
| Smear on the mover | Directional blur on the first frames of a fast move, fading over 3 frames, with 2 or 3 trail copies at falling alpha; length half to 1 times that frame's travel | graphic styles |
| Shutter | Sub-frame samples averaged at render time (the `--subframes` option of `video-render`) | photographic styles |

- Do not mix shutter blur and smears in 1 film.
- A colour split is for 1 moment, for under 10 frames.
- A focus pull behind a headline stays under about 11 px of blur at 1080p.

## Frame clocks

A film may hold more than 1 frame clock, but each moment has 1.

| Clock | How to seek | Fits |
|---|---|---|
| Smooth | every frame | most films |
| On 2s | compute from `Math.floor(frame / 2) * 2` | drawn animation at 24 fps |
| On 3s | `Math.floor(frame / 3) * 3` | held poses, collage, paper |
| Drawn layer over a smooth camera | the drawn layer uses a stepped frame, the camera the real one | hand-drawn characters in a moving world |

A stepped clock is still a function of the frame index, so it is seek-safe.

## Sound meets picture

The `video-audio` skill owns the mix. The animator owns the times that sound must hit, and reports them.

- An impact sound syncs on its first transient. A whoosh syncs on the loudest point of its pass. A riser syncs on its end.
- 1 to 3 hand-placed hits per film beat a hit on every cut. Leave 0.2 to 0.3 s of lower music before a hit.
- The hero moment sits at 60 to 75 % of the film, on the music's largest change.
