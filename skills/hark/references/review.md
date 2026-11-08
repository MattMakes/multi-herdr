# Review a mix or a film, and A/B a change

The `timeline` unit owns `timeline`, `sheet` and `critique`. `measure` is model-free.

## Loop 2: review a mix or a film

| step | command | read |
|---|---|---|
| 1 | `hark timeline <film.mp4> --cues cues.json` (or `<soundtrack.wav> --video <film.mp4>`) | a text transcript: timed events with level and tags, sections, the loudness curve, picture cuts, sync per cut and cue |
| 2 | `hark sheet <film> -o sheet.png --cues cues.json [--from 10 --to 25] [--detail low]` | 1 annotated picture: frames, sections, labelled events, spectrogram, loudness, beats, cue markers by sync (green within 1 frame, amber within 100 ms, red no sound). Open the PNG with your image reader. |
| 3 | `hark critique <film> [--fast]` | a scorecard against 7 professional launch films: each metric ok, low or high with a weight, timed actions, a verdict (launch-grade, close, needs work, not launch-grade) |
| 4 | `hark measure <mix.wav> [--music]` | LUFS, true peak, LRA, noise floor, attack and decay, brightness, bands; with `--music` the tempo |

- `--detail low` runs fast. `--detail high` tags at most 96 events in order of importance; the summary gives the
  tagged count, so an event without tags is not a missing sound.
- `--fast` on critique skips the tagger and realness passes.
- `offset_ms` is sound time minus picture time: negative is early. `chance` is what random edit points would score.
  Sync means something only when it beats chance.
- A cut with no listed hit can still hold a quiet whoosh. Look at the sheet before you add anything.
- Fix the critical scorecard rows first. Render again and repeat steps 1-3.
- The "onset on visual events" share counts a new attack only within +-100 ms of the visual event. The reference films
  were scored the same way. Its fixes list cuts with no sound at all.
- When the input is 1 of the profile's reference films, critique says so and scores it against the rest.

## Loop 3: A/B a change

- `hark ab before.wav after.wav --query "<description>"`, or `--refs <pro refs>/` instead of a query. The result
  names the closer file and describes B against A in measured terms (louder, brighter, longer decay).
- When the sounds sit inside longer files, give the trim times (`--a-start --a-end --b-start --b-end`). 2 whole
  multi-take files compare all takes and the gaps together.
- Then run `hark timeline after.wav` and compare its events and sync figures with the earlier run.
- Make 1 change per comparison.

## Mixing to picture

The `video-audio` skill owns the video pipeline's mix: voice-over, ducking, the beat grid in `audio/beats.json` and
the -14 LUFS / -1 dBTP master. Use its beat grid to place hits and reveals; use hark to check the result (`timeline
--cues`, `sheet`, `critique`). For a soundtrack from real recordings, use the `sound-sourcing` skill.
