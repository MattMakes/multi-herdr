# Plan a short or a long film

`meta.format` is `short` or `long`. Copy it from the `Format:` line of
`BRIEF.md`. The contract is `docs/specs/video-formats.md`.

## short

- `meta.duration_sec` is 15 s to 90 s (+-0.05 s).
- `meta.aspect` is `16:9`, `9:16` or `1:1`.
- Do not write `meta.chapters`.
- Assume the viewer has no sound. Burned-in captions carry the whole story.
- Plan 1 hero moment.

## long

- `meta.duration_sec` is 180 s to 900 s.
- `meta.aspect` is `16:9`.
- Write `meta.chapters` and `meta.end_screen_sec`.
- Use captions only. Do not plan a voice-over.

### Plan chapter by chapter

1. Split the film into 3 chapters or more. Each chapter runs 30 s to 180 s.
2. Give each chapter 1 payoff. Put the main hero moment in the final chapter.
3. Open each chapter on its `hook` caption. The first scene of the chapter shows it.
4. Make the first 30 s prove the title and the thumbnail.
5. Split each chapter into scenes. Vary the shot length: fast montage, then room at the payoff.
6. Plan the last `meta.end_screen_sec` seconds as the end screen. Keep clear space for the end-screen elements.

### Captions carry the story

A `long` film has no narration. The caption read-hold rule (gate
`read_hold`) carries the story. Give each caption its full read hold.
Write 1 idea for each caption. Do not add a voice-over to fix pacing.

### meta.chapters

A list of `{"id": str, "title": str, "start_sec": number, "hook": str}`.

- The first `start_sec` is `0`.
- Each `start_sec` is greater than the one before.
- A chapter runs to the next `start_sec`, or to `meta.duration_sec`.

```json
"format": "long",
"duration_sec": 300.0,
"end_screen_sec": 10,
"chapters": [
  {"id": "c1", "title": "The empty map", "start_sec": 0, "hook": "This map was empty"},
  {"id": "c2", "title": "First monument", "start_sec": 60, "hook": "Then the first one rose"},
  {"id": "c3", "title": "The crash", "start_sec": 150, "hook": "It broke everything"},
  {"id": "c4", "title": "The fix", "start_sec": 240, "hook": "One line fixed it"}
]
```
