# Long-form audio rules

Apply these rules when `meta.format` is `long`. A `short` film does not use them.

## Voice-over (step 1 and step 2)

A `long` film has no voice-over by default. Captions carry the story.
When no scene has `narration`, skip step 1 and step 2.
Then write no `audio/vo-*.wav` file and run no `reconcile_timing.py`.
Run the plan checker from step 2 once, then go to step 3.

## Beat grid and music beds (step 3)

- Use 1 music bed for each chapter or group of chapters in `meta.chapters`.
- Never loop 1 song for the whole film.
- Crossfade 2 beds at a chapter `start_sec`. Put the crossfade on a beat.
- Write 1 grid for each bed. Join the grids in `audio/beats.json`.
- Keep a music tail in the end screen. The end screen is the last `meta.end_screen_sec` seconds.
- End the last bed on its real final phrase at `meta.duration_sec`.

## Ducking and master (step 5)

- With no voice-over, skip the voice ducking. Keep the music under the game or product audio.
- Crossfade beds at each chapter start on a beat. Write the joined beds to `audio/bed.wav`.
- Keep the music tail on the end screen. Fade the last bed to the final frame.
- A `long` film requires 2 `loudnorm` passes too.
- Master the whole film in 1 pair of `loudnorm` passes.
