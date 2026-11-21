# Remotion compositions as plates

Remotion renders React components frame by frame. Its model is the same as the seek contract: a frame is a pure function of the frame number. Use it when the project already has Remotion code, or when the operator asks for it.

## Before you start: licence and downloads

1. **Licence.** Remotion is not under an open-source licence. Its terms make it free for some users (individuals and very small companies, at the time of writing) and paid for larger companies. You cannot decide this for the operator. Send 1 `QUESTION:` that asks whether the operator holds, or does not need, a company licence, and link `remotion.dev/docs/license`. Do not render until the answer is in `decisions.md`.
2. **Downloads.** The first render downloads a browser build. Captions by Whisper and background matting download models. Font packages fetch font files at render time. Each one is a download: it needs the operator's word, as the standing rules say. Prefer local font files loaded from the project's static folder.
3. **Version.** Read the installed version from `package.json` and check each API below against the docs of that version. Do not upgrade the project unasked.

## Rules of a Remotion scene

- All motion comes from the current frame through the interpolation helper. CSS transitions, CSS keyframes and utility animation classes do not render.
- Take the frame rate from the video config and write times as `seconds * fps`. Never write a bare frame number that assumes 30.
- Clamp interpolation at both ends unless you want it to run on: values are not clamped by default.
- Pick the easing from `MOTION.md`. A bezier token carries over as it is.
- For a scale move, interpolate on a perceptual (logarithmic) scale so the zoom looks even.
- A stepped look samples every 2nd or 3rd frame of a child sequence.
- Lay out scenes with sequences that carry `from` and `durationInFrames`. A child's local frame starts at 0.
- A transition between 2 sequences overlaps them and so shortens the total. Recount the frames after you add one.
- In a 3D canvas, animate only from the current frame. A per-render-tick callback of the 3D library runs on wall time and is forbidden.
- No `Math.random()`: use the seeded random helper with a fixed seed string.
- Load a Lottie file or any remote data inside the delay-render guard, so that no frame renders before the data is in.

## Fit the timeline

Set the composition's size, fps and `durationInFrames` from `timeline.json`: `round(duration_sec * fps)` plus handles. When the duration depends on data, compute it in the metadata callback, not by hand.

## Render

Read the composition ids first, then render the plate:

```sh
npx remotion compositions
npx remotion render <composition-id> plates/<clip>.mp4
npx remotion render <composition-id> --frames=0,180,359 --image-format=png
```

- Always pass the composition id.
- For an alpha plate, render a PNG sequence, or ProRes 4444 with a 4:4:4:4 pixel format.
- Sample stills first (`--frames`), look at them, then render the range.
- A company that holds a licence passes its key as the docs of the installed version say. Do not pass, print or store any key that the task did not give you.

Then run the 5 checks of the plate contract and write the sidecar. `build` holds the Remotion version and the source commit.

## Word-level captions

Remotion's caption format is a list of objects with `text`, `startMs`, `endMs`, `timestampMs` and `confidence`; a word carries its leading space. The film pipeline's own word times are in `audio/sync-sheet.json` (`vo_segments[].words[]` with `word`, `start_sec`, `end_sec`). Convert between the 2 with a few lines of code: multiply by 1000, and add the leading space to every word after the first of a phrase. Keep times relative to the audio file they came from; add the scene start when you place them.

The film's captions are made by the editor seat from the sync sheet. A Remotion plate carries captions only when the storyboard puts them inside that scene.

## When not to use it

- The scene can be an HTML scene. Then the animator builds it in the seek contract and the pipeline renders it with no licence question and no download.
- The task would add Remotion to a project that has none. Ask first.
