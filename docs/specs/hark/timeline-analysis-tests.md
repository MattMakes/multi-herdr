# hark timeline: analysis acceptance tests

Part of the timeline unit spec. Index: `docs/specs/hark/timeline.md`. Section numbers in the tests refer to `docs/specs/hark/timeline-analysis.md`.

## Acceptance tests

Synthetic signals at 48 kHz. A "click track" is decaying noise bursts (50 ms, amplitude 0.5, decay time constant
8 ms) at given times over a noise bed of amplitude 0.01. Model steps may be replaced by stubs where a test says
"no models": a stub tagger that labels the most prominent events as HIT "punchy impact hit" and a stub sectioner
that returns one section over the whole window.

| id | input | expected properties |
|---|---|---|
| T1 | click track at 0.5, 1.25, 2.0, 2.6, 3.33 s, 4 s long | every click has a hit within 3 ms of its time; at most 6 hits; the 1.25 s hit has prominence > 20 dB, peak between -12 and +6 dBFS, length < 200 ms |
| T2 | loud click at 1.0 s plus a 10x quieter click at 1.5 s with no bed | prominence at 1.0 s exceeds that at 1.5 s by more than 10 dB |
| T3 | 3 s of a sine, 1 s of zeros, 2 s of the sine | exactly 1 gap, kind `digital silence`, start within 0.15 s of 3.0, end within 0.15 s of 4.0; 0 clipping runs. The same sine x5 clipped to plus or minus 1 for 6 s: more than 10 clipping runs |
| T4 | click track of beats every 0.5 s from 0.25 s for 12.5 s, amplitude 0.3, with a 60 Hz decaying thump on every 4th beat | tempo within 2 BPM of 120; confidence `clear`; median distance of detected beats (1-11.5 s) to the true beats < 15 ms; the second downbeat within 30 ms of an accented beat |
| T5 | 8 s of white noise (amplitude 0.1) | beat grid confidence `none` |
| T6 | cue file `{fps: 60, events: [{f: 120, kind: hit, id: a}, {f: 30, kind: tick}]}`; strings `1.5, 3`; `[{"t": 2.25}]`; list `[4, 1]`; `null`; `{"fps": 50, "events": [{"frame": 100}]}`; `[{"seconds": 1.25}]` | fps 60 and times [0.5, 2.0] with the 2.0 cue id `a`; [1.5, 3.0]; [2.25]; first time 1; ([], null); 2.0; 1.25. Frame cue without fps and a cue without time raise input errors |
| T7 | events (t, prominence) (1.000, 12), (2.030, 12), (3.300, 2), (5.0, 12); targets 1.004, 2.0, 3.25, 4.0; beats every 0.5 s from 1.0 to 4.0; frame 1/60 | statuses OK (-4.0 ms), near (+30.0 ms), near, no sound (null offset); `hit_within_1_frame` 0.25; `no_sound` [4.0]; `chance_within_1_frame` between 0 and 0.1; `on_beat` 0.75; `on_beat_chance` 0.2; `accent_min_db` 12.0; accent offsets [-4.0, 30.0, null, null]; `accent_within_1_frame` 0.25; `accent_within_100ms` 0.5; accent chance below plain chance |
| T8 | noise of amplitude `0.01 + 0.2*gauss(t; 3.0 s, 0.25 s)` for 6 s | level peak at 3.05 s found within 60 ms of 3.0 s with rise > 10 dB; none at 1.0 s; sync of [3.05, 1.0] with no events and these level envelopes gives `peak` then `no sound` |
| T9 | 6 s click track with clicks every 1 s from 0.5 s; cues [1.5, 2.52, 5.0]; no models | the summary starts the text; the text is ASCII; 6 hits with prominence > 10 dB; cue stats n 3 and `no_sound` [5.0]; the text contains `NO HIT` and `cue -20 ms OK`; the result is JSON-serialisable; a second identical call returns `cached: true`; a zoom 2-4 s with caching off has start 2.0, end 4.0 and strong events only at 2.5 and 3.5 |
| T10 | 12 s click track every 0.5 s; cues [2.0, 4.02, 6.5, 7.97]; full run and zoom 4-8 s; no models | zoom context [0.0, 12.0]; every zoom cue has the same status and offset as in the full run; every zoom event and beat lies inside 4-8 s |
| T11 | 10 events of band `high` 60 ms apart from 1.0 s with prominence 30 down to 29.1, plus 4 `full` events at 3, 5, 7, 9 s; budget 6 over 0-10 s | at most 2 chosen events before 2 s; all of 3, 5, 7, 9 s chosen |
| T12 | probe results: video stream at 90 000 fps in `song.mp3` and `song.mp4`; 30 fps in `film.mp4`; 30 fps in `song.m4a` | not a video, not a video, video, not a video |
| T13 | 5 s of stereo zeros | digital silence true; integrated and true peak null; one gap with `where: whole file`; minimum momentary value >= -70. Zeros 1 s, sine 3 s, zeros 1 s, sine 2 s, zeros 1 s: gap `where` values [lead-in, none, tail], first gap start 0.0; with both edge flags false, all three have no `where` |
| T14 | a 3 s mp4 (black 1.5 s then white 1.5 s, 30 fps) with clicks at 0.6, 1.5, 2.4 s; no models | `video` is the file itself; fps 30; cuts [1.5]; the cut's status is OK or near with offset under 50 ms |
| T15 | the same mp4 without an audio stream; no models | `has_audio` false; cuts [1.5]; the cut's status `no sound`; summary contains `NO AUDIO STREAM` |
| T16 | the phrase table | the 49 phrases are unique and equal the background table's phrases |
| T17 | text `t\xf6n.wav` through the ASCII rule | `t\xf6n.wav` (the literal backslash form) |
| T18 | CLI parse `timeline x.wav --cues 1,2 --from 1 --to 3 --detail low` | start 1.0, end 3.0, detail low, cues `1,2` |
| T19 (slow, models) | 6 s click track, detail low | at least one tagged event; every kind is a vocabulary kind; sections exist with a label; CLAP event windows >= tagged count |

**Evaluation targets** (the source's measured quality; reproduce on equivalent data, index data asset A5):

| measure | target |
|---|---|
| placed sound effects found within 30 ms on a finished film mix (147 placements) | about 93 % (96 % at SNR >= 0 dB, 65 % below -6 dB) |
| timing error vs the mixer's attack times | median |error| about 0.5 ms, p90 about 11 ms |
| detections that are placed effects / placed or bed onsets / any real onset | about 51 % / 81 % / 96 % |
| synthetic tuning mixes, recall at robust z 3.0 and contrast 3 dB | about 81 % (98 % at SNR >= 0) |
| cue offsets vs true placement offsets (140 cues, 60 fps) | 84 % within 5 ms, 96 % within 1 frame, 99 % within 40 ms |
| level peaks on whoosh peaks with no onset (synthetic) | 6 of 10 found; 13 of 56 empty grid times give a false level peak |
| section boundaries on synthetic bed switches | recall about 89 %, precision about 84 %, median error 0.04 s |
| pulse clarity >= 0.15 | 94 % of music excerpts, 0 % of ambience excerpts |
| music vs ambience bed decision | 26 of 26 beds |
| tags, right kind (test half of the background set) | about 30 % on one-shots in a mix; bed events called KEY or UI about 32 %; film events about 1 % |
| exposed events (>= 12 dB over bed) right kind | about 50 % vs 25 % below |
| wall clock, 60-93 s file at detail normal, 2 CPU threads, cold caches | under about 2 minutes |
