---
name: video-motion-styles
description: Use when a video needs a named motion-graphics style or a launch-film structure - picking 1 of 15 styles (frame-by-frame line boil, isometric, flat vector, line art, 3D render, shape morph, sticker explainer, HUD, collage, aurora glass, Bauhaus, synthwave, pixel art, liquid, variety captions) with its palette, type, frame rate, motion signature and testable features, and laying out a launch film as acts, type beats and product beats on a bar grid with 1 element that carries every scene change.
---

# Motion styles and the launch-film form

A style is a set of rules that can be tested, not a mood word. "Make it bouncy" cannot be checked. "Entrances use `cubic-bezier(0.34, 1.56, 0.64, 1)` with a stagger of 2 to 4 frames" can.

This skill gives the director a catalogue to choose from and gives the animator the rules of the choice. The scene still follows the seek contract of `video-motion`. The numbers of the chosen style go into `MOTION.md` as tokens.

## When to use

- The operator names a style ("isometric", "like a HUD", "paper collage"), or asks for "something with character".
- A direction in `DIRECTION.md` needs a style that differs in kind from the other 2 directions.
- The film is a launch, a teaser or an update film and needs its act structure.

## How the director uses it

1. Read the brand first. A style that fights the brand's own shapes, colours and type loses. A brand colour replaces a catalogue colour of the same role.
2. For each of the 3 directions in `DIRECTION.md`, name 1 style from `references/styles.md`, or "house" for the brand's own look. The 3 directions differ in kind: not 3 flat-vector films in 3 palettes.
3. Write the style block of each direction in this form:

   ```text
   Style: <catalogue name>
   Frame rate: <fps> (and the stepped rate, when the style animates on 2s or 3s)
   Palette: 3 to 6 hex colours, each with its role
   Type: <face>, <weight>, <case>
   Signature features: 4 to 6 lines, each one testable
   Renderer: html | webgl | blender | manim   (who builds the scenes)
   ```

4. Give each style 1 key frame in `qa/directions/` as usual. A style is judged on a still before anyone animates it.
5. After the pick, copy the chosen block into the top of `MOTION.md`, or ask the animator that owns `MOTION.md` to do so.

The `Renderer` line tells the orchestrator which seat builds the scenes: `video-animator` for `html`, `video-3d-animator` for `webgl`, `blender-cinematographer` for `blender`, `video-math-animator` for `manim`, `video-illustrator` for the painterly and hand-drawn styles.

## How the animator uses it

1. Read the style block in `MOTION.md`. Each signature feature becomes a check in the "checks" list of `MOTION.md`.
2. Render poster-worthy stills at 6 to 10 key times before you animate. Keep about 5 % of the frame as a safe margin.
3. Build to the features. Then check each one on rendered frames and write "holds" or "broken at <time>" next to it.
4. Do not mix 2 styles in 1 film unless the story is the change of style.

## Rules that hold for every style

- A hook within 0.5 s. Never more than 0.3 s of empty or black at the start.
- 1 escalation, and 1 hero moment at 60 to 75 % of the runtime.
- The end frame holds 0.8 to 1.2 s with a small living motion.
- 1 cue sheet feeds picture and sound: `timeline.json` with `audio/beats.json` and `audio/sync-sheet.json`.
- Final-frame checks: no fallback font, nothing clipped at an edge, no shimmer on thin lines, no banding in gradients (add 3 to 5 % noise), no black frame.
- Assets are the brand's own, or free to use with their terms recorded. Invented brands only in a sample.

## The launch-film form

`references/launch-film.md` holds the form: the spine sentence, acts, type beats and product beats, the bar grid, the carried element with its handover kinds, the reading holds and the pacing limits. Use it for a launch film, a teaser, a feature update and a logo sting. The storyboard rules and plan gates of `video-storyboard` still apply; where a number here differs from a plan gate, the plan gate wins.

## Scoring a style

The critic's rubric in `video-qa` owns the verdict. For a style, add these 3 questions to a self-check before hand-off:

1. Style fidelity: would a person who knows the style name it from 1 still?
2. Finish: grain, edges, light and type are those of the style, not of a template under a filter.
3. Is anything generic? A part that could sit in any film is a fault. Name it and replace it.

A score scale for self-notes: 5 is student work, 7 is professional, 8 is agency work, 9 is award level. Aim for 8. Write every fix as timecode, what is seen, why it hurts, and how to change it in numbers.
