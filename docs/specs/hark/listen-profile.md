# hark listen: profile

Part of the listen spec. Index: `docs/specs/hark/listen.md`.

## Purpose

`profile` measures what professional reference films (launch films,
trailers) do with sound, the same way for every film, and stores the spread
of each metric. The timeline unit's critique compares a new soundtrack with
such a profile. The profile holds numbers only: no media, no file names.

## Interface

CLI and Python signatures are in the index.

### Per-film result (`film_metrics`)

| Field | Type | Meaning |
|---|---|---|
| `path` | str | the soundtrack or film |
| `video` | str or null | the picture source used (the `video` argument, else `path` when it has a video stream) |
| `duration` | float | seconds, 3 decimals |
| `fps` | float or null | picture frame rate |
| `codec` | str or null | audio codec (core-io probe) |
| `lossy` | bool | lossy audio codec |
| `mono` | bool | identical channels (and not silent) |
| `short` | bool | duration < 10 s |
| `metrics` | object | every metric key (table below) -> number or null |
| `chance` | object | sync key -> what chance scores (2 decimals) |
| `arc` | list of 11 or null | loudness arc |
| `events` | object | `{hits, visual, visual_strength, stopdowns, beats, music_windows, onsets_n}` |
| `seconds` | float | wall time |
| `timing` | object | seconds per pass: `audio`, `audioset`, `picture` |
| `summary` | str | one line |

`events`:

- `hits`: salient hit times (3 decimals).
- `visual`: visual event times; `visual_strength`: their frame-difference
  values (3 decimals).
- `stopdowns`: list of `[start, length, depth_db]`.
- `beats`: beat times (3 decimals).
- `music_windows`: list of `[t, p_music, p_speech]` per 5 s window, or null
  without AST.
- `onsets_n`: onset count.

### Profile file

| Key | Type | Meaning |
|---|---|---|
| `version` | int | 2 |
| `n` | int | number of films |
| `metrics` | object | metric key -> stats |
| `chance` | object | the 4 sync keys -> stats of their chance values |
| `arc` | object or null | `{median: [11], p10: [11], p90: [11]}` (1 decimal or null) |
| `films` | list | per film `{id, style, duration, has_video, metrics, chance, arc}` |
| `warnings` | list of str | |
| `summary` | str | |

Stats: `{n, median, p10, p90, min, max, values}` (3 decimals; `values` lists
every film's value in film order, null where a film has none). A metric no
film measured has stats `{n: 0}`.

Film ids are `ref_1`, `ref_2`, ... in the order of the films kept. `style`
comes from the styles option or is "".

### Metric table

| Key | Group | Unit | Decimals | Meaning |
|---|---|---|---|---|
| `lufs_i` | loudness | LUFS | 1 | integrated loudness (BS.1770) |
| `true_peak` | loudness | dBTP | 1 | true peak (4× oversampled, core-io) |
| `lra` | loudness | LU | 1 | loudness range |
| `crest_lu` | loudness | LU | 1 | loudest 3 s (short-term max) minus integrated |
| `intro_lu` | arc | LU | 1 | opening level: energy mean of the first 10 % against integrated |
| `end_lu` | arc | LU | 1 | closing level: the last 10 % |
| `peak_pos` | arc | 0-1 | 2 | where the loudest 3 s sit (0 = start, 1 = end) |
| `first_sound_s` | arc | s | 2 | time to the first sound |
| `max_gap_s` | arc | s | 2 | longest near-silent gap between first and last sound |
| `end_silence_s` | ending | s | 2 | dead air after the last sound |
| `fade_s` | ending | s | 2 | last moment at full level to the end of sound (< 0.5 s = hard out) |
| `final_lu` | ending | LU | 1 | loudest 0.4 s of the last 3 s of sound against integrated (is there a final hit?) |
| `tail_s` | ending | s | 2 | how long the final hit rings out; null when there is no final hit |
| `music_frac` | music | share | 2 | share of 5 s windows with AudioSet Music >= 0.3 |
| `speech_frac` | music | share | 2 | share of 5 s windows with Speech >= 0.5 |
| `tempo` | music | BPM | 0 | tracked tempo (integer) |
| `onset_rate` | density | /s | 2 | onsets per second |
| `hit_rate` | density | /s | 2 | salient hits per second |
| `stopdowns_min` | density | /min | 1 | stop-downs per minute |
| `cut_rate` | sync | /s | 2 | visual events (cuts, big picture changes) per second |
| `cuts_with_sound` | sync | share | 2 | visual events with an onset within ±100 ms |
| `cut_hit_frac` | sync | share | 2 | visual events with a salient hit within ±100 ms |
| `cut_frame_sync` | sync | share | 2 | visual events with an onset within ±1 frame |
| `cuts_on_beat` | sync | share | 2 | visual events within ±70 ms of a tracked beat |
| `band_sub` | spectrum | dB | 1 | energy share of 20-60 Hz |
| `band_low` | spectrum | dB | 1 | 60-250 Hz |
| `band_lowmid` | spectrum | dB | 1 | 250 Hz-1 kHz |
| `band_mid` | spectrum | dB | 1 | 1-4 kHz |
| `band_presence` | spectrum | dB | 1 | 4-8 kHz |
| `band_air` | spectrum | dB | 1 | 8-16 kHz (above 16 kHz is left out: web AAC encodes cut it) |
| `brightness_hz` | spectrum | Hz | 0 | median magnitude-weighted spectral centroid (integer) |
| `side_to_mid_db` | stereo | dB | 1 | stereo width; a mono mix counts as -40 |
| `correlation` | stereo | r | 2 | L/R correlation |

The table order is the output order. Text formatting of a value: "n.a."
for null; shares as integer percentages; units `0-1` and `r` without a unit;
others "<value with the stated decimals> <unit>".

### Text output

The summary; one line per film (id, duration, style or "-"); the median arc
as 11 signed values; then a table: metric label, key, median, "p10 .. p90",
"min .. max", for each metric with n > 0.

## Behaviour

### profile

1. Normalise paths to a list. A given path that yields no media gets a
   warning "skipped <path>: not found (or no media files in it)".
2. For each listed media file, in order (index i over all listed files):
   1. Media problem: not found, a folder, not a readable audio or video
      file, or no audio track -> skip with that reason.
   2. Unscorable: shorter than 3 s, or integrated loudness at or below
      -69.5 LUFS (silent) -> skip with that reason. (When the audio pass of
      the file is cached, use its duration and loudness; else decode and
      measure only the loudness.)
   3. A decode error -> skip "cannot decode (<first 160 characters>)".
   4. Else `film_metrics(path, use_ast)`. Set its id (`ref_<n>` for the n-th
      kept film) and its style: from a styles object by base name, from a
      styles list by index i, else "".
   5. Every skip prints a warning to stderr; every kept film prints its id,
      duration and analysis time to stderr.
3. No film kept: input error "no usable reference films: <reasons>" (or "no
   media files given").
4. Aggregate (P8). Warnings: the count of skipped files when any; when fewer
   than 3 films, a warning that the ranges are the min-max of so few values
   and that 5 or more references are needed (also printed).
5. Summary: the number of films; medians of integrated loudness (with its
   p10-p90), LRA, onset rate, salient hit rate, music presence (when
   measured) and visual event rate (when measured); then each warning.
6. With `out`: write the profile JSON (indent 1), creating parent folders.

CLI errors: a styles file that does not exist, an input error, a decode
error or invalid styles JSON -> one stderr line `hark profile: error:
<message>`, exit 1. Success exits 0.

### film_metrics

1. Audio pass (cached, P1).
2. Salient hits (P3) and stop-downs (P4).
3. Metrics (P6).
4. AudioSet pass when enabled (cached, P2).
5. Picture pass when a picture source exists: the timeline unit's frame
   difference detector gives the frame rate and a per-frame difference
   curve; its visual-event detector gives event frame indices. Event time =
   frame index / fps; keep events before the audio duration.
6. Sync metrics (P7).
7. Round each metric to its decimals; `tempo` and `brightness_hz` become
   integers; NaN and infinity become null.
8. Summary: name, duration, LUFS, true peak, LRA (when known), music share
   (when known), onset rate, salient hit rate, and the visual event count
   with the share that has an onset within ±100 ms (when known).

## Algorithms

### P1. Audio pass

1. Decode stereo. Light measurement (core-io): duration, LUFS, true peak,
   LRA, short-term max LUFS, band energies, side-to-mid, correlation,
   brightness.
2. Mono mean of the channels, resampled to 22 050 Hz.
3. Onset strength envelope (spectral flux, hop 256 samples), onset times
   from that envelope, beat tracking (tempo and beat times) on the same
   envelope.
4. Loudness curves (core-io, K-weighted blocks, LUFS): fast (0.1 s blocks,
   hop 0.02 s), momentary (0.4 s, hop 0.1 s), short-term (3.0 s, hop 0.1 s).
   Times are block centres.
5. e2 = 2 ms frame RMS of the mono mean, in dB (+1e-9).
6. Onset times are moved to their attacks (P5).
7. Cache everything (key: the core-io decode identity of the file and an audio
   pass version).

### P2. AudioSet pass

5 s windows of the mono decode, starting every 5 s; a window is kept when at
least 1 s of audio remains from its start (the first window is always kept).
AST probabilities per window (527 classes). Cache the window starts, the
probabilities (float16) and the labels (key: file identity, window length,
a version).

### P3. Salient hits

A salient hit: the 100 ms loudness jumps by at least 6 dB within 80 ms and
lands within 8 LU of the integrated level.

1. Needs at least 20 fast blocks. h = fast hop (0.02 s).
2. For each block i: pre = the minimum of the 10 blocks before i (0.2 s;
   before the start, the first block's value is used); post = the maximum of
   blocks i to i + 3 (0.08 s; after the end, the last block's value).
3. Candidate blocks: post - pre >= 6 and post >= integrated - 8.
4. Group candidate block indices into runs (a gap of more than 2 blocks
   starts a new run).
5. For each run: the hit time is the time of the strongest onset-envelope
   frame between the run's first block time - 0.05 s and its last block time
   + 0.1 s (frame time = index × 256 / 22050); when there is no envelope frame
   there, the first block time + 0.03 s.
6. Keep a hit when it is more than 0.1 s after the previous kept hit.
7. Move each hit to its attack (P5).

### P4. Stop-downs

A stop-down is a short dip before a hit: at least 10 dB below the median of
the preceding 2 s, for 0.15 to 1.5 s, back to within 3 dB of that median
within 0.3 s.

1. Fast blocks, h = 0.02 s, w = 100 blocks (2 s). Fewer than 3 blocks, or at
   most w blocks: none.
2. Reference of block i (i >= w) = median of blocks i - w .. i - 1.
3. Scan i from w. When block i < ref - 10: find the end j of the run of
   blocks below that same reference - 10. Length = (j - i) × h. Record
   `(time of block i - 0.05, length, ref - min level of the run)` when the
   length is 0.15 to 1.5 s, j is inside the curve, and the maximum of blocks
   j .. j + 15 (0.3 s) reaches ref - 3. Then continue at j + 25 (0.5 s
   later). Else continue at i + 1.
4. Rounding: start and length 3 decimals, depth 1 decimal.

### P5. Move to the attack

Spectral-flux peaks lag the attack by 10 to 30 ms; an editor aligns on the
attack. For each time t, look at 2 ms frames from 60 ms before to 20 ms after
t; the new time = the frame with the steepest rise (largest frame-to-frame
dB increase). When the range has fewer than 2 frames, keep t. Round to 4
decimals.

### P6. Metrics

I = the integrated loudness (-70 when unknown). d = duration.

- `lufs_i`, `true_peak`, `lra`, `correlation`, `brightness_hz`: from the
  measurement.
- `mono`: side-to-mid unknown (identical channels) and I > -69.5. Then
  `side_to_mid_db` = -40, else the measured value.
- `band_*`: the measurement's band energies.
- `crest_lu` = short-term max - I (when both known).
- With more than 2 momentary blocks: `intro_lu` = energy mean of the
  momentary blocks with time <= max(0.1 d, 0.5 s), minus I; `end_lu` = the
  same for blocks with time >= d - max(0.1 d, 0.5 s). Energy mean of levels
  L = 10 log10(mean(10^(L/10))).
- `peak_pos` = time of the loudest short-term block / d, when there are at
  least 2 short-term blocks and d >= 6 s.
- `arc` (more than 2 momentary blocks): for k = 0..10, the energy mean of the
  momentary blocks within ±max(1.5 s, 0.05 d) of k/10 × d, minus I, 1
  decimal; null when no block is in range.
- Loud blocks = fast blocks above I - 20; audible blocks = fast blocks above
  I - 30.
- `first_sound_s` = refine the first loud block's time with the start edge
  rule (P9).
- End of sound t_end = min(d, the end edge of the last audible block (P9));
  `end_silence_s` = max(0, d - t_end). (No audible block: t_end = d and
  the ending metrics stay null.)
- `fade_s` = max(0, t_end - (time of the last momentary block at >= I - 6
  with time <= t_end) - 0.2).
- `max_gap_s` = the longest run of fast blocks below I - 30 among blocks with
  time from `first_sound_s` (0 when unknown) to t_end, times h; 0 when that
  range holds at most 1 block.
- `final_lu` = the maximum momentary block with time in [t_end - 3, t_end],
  minus I.
- `tail_s`: onset strengths = the onset envelope at each onset's frame.
  Among onsets in [t_end - 4, t_end), take the strongest. When its strength
  is at least the 75th percentile of all onset strengths, `tail_s` = t_end -
  its time; else null (the film stops or fades without a final hit).
- `onset_rate` = onsets / d; `hit_rate` = hits / d.
- `stopdowns_min` = stop-downs / d × 60, when d >= 6 s.
- `tempo` = the tracked tempo, when more than 4 beats were tracked.
- When d < 10 s: `lra`, `crest_lu`, `intro_lu`, `end_lu`, `final_lu` are null
  (on a short clip they measure the clip, not a film).
- With AST: `music_frac` = share of windows with Music >= 0.3; `speech_frac`
  = share with Speech >= 0.5.

### P7. Sync metrics

With visual event times V (only when there are at least 3):

- near(V, targets, tol) = share of events with a target within ±tol.
  None when V is empty; 0 when targets is empty.
- `cuts_with_sound` = near(V, onsets, 0.1).
- `cut_hit_frac` = near(V, hits, 0.1).
- `cut_frame_sync` = near(V, onsets, 1 / fps + 0.006).
- `cuts_on_beat` = near(V, beats, 0.07).
- chance of each = the mean over 16 fixed offsets of near((V + offset) mod
  d, targets, tol). Offsets (s): 0.23, 0.31, 0.43, 0.57, 0.71, 0.87, 1.13,
  1.37, -0.29, -0.41, -0.53, -0.67, -0.79, -0.97, -1.19, -1.43.
- `cut_rate` = visual events / d (also with fewer than 3 events).

### P8. Aggregate

Per metric: stats over the films' values (nulls left out). Per sync key:
stats of the chance values. Arc: per index, the 50th, 10th and 90th
percentile of the films' arc values at that index (nulls left out; null when
all are null), 1 decimal.

### P9. Edge refinement

For a 100 ms block centred at t, with 2 ms frames e2:

- Frames a = max(0, floor((t - 0.05) / 0.002)) to b = min(count,
  floor((t + 0.05) / 0.002) + 1).
- Start edge: reference = the loudest 2 ms frame of the 150 ms from a; the
  edge = the first frame of [a, b) within 20 dB of the reference.
- End edge: reference = the loudest frame of the 150 ms before b; the edge =
  just after the last frame of [a, b) within 20 dB of it.
- No frame qualifies, or an empty range: max(0, t). Round to 3 decimals.

## Constants

| Name | Value |
|---|---|
| profile version | 2 |
| minimum reference length | 3 s |
| short film | < 10 s (shape metrics null) |
| silent | integrated <= -69.5 LUFS |
| analysis rate for onsets | 22 050 Hz, hop 256 |
| attack frames | 2 ms; search -60 ms to +20 ms |
| fast / momentary / short-term blocks | 0.1 s hop 0.02; 0.4 s hop 0.1; 3.0 s hop 0.1 |
| salient hit | jump >= 6 dB within 80 ms against the minimum of the prior 200 ms, landing >= I - 8 LU; runs split at gaps > 2 blocks; 0.1 s apart |
| stop-down | >= 10 dB under the 2 s median, 0.15 to 1.5 s, back within 3 dB in 0.3 s, then skip 0.5 s; start reported 0.05 s early |
| loud / audible | I - 20 / I - 30 |
| full level | I - 6 (fade), minus 0.2 s |
| gap level | I - 30 |
| final window | last 3 s (final_lu), last 4 s (tail_s), 75th percentile strength |
| arc | 11 points, window ±max(1.5 s, 5 % of duration) |
| intro / end span | max(10 %, 0.5 s) |
| peak_pos | needs d >= 6 s |
| AST window | 5 s; kept when >= 1 s remains |
| music presence | Music >= 0.3 |
| speech presence | Speech >= 0.5 |
| sync tolerances | ±100 ms (sound, hits), ±(1 frame + 6 ms), ±70 ms (beat) |
| sync minimum | 3 visual events |
| chance offsets | 16 (listed in P7) |
| mono width | -40 dB |
| few-films warning | < 3 films |

Visual events (timeline unit): grey frames of 96 × 54 pixels; a frame
difference peak at least 4× the median of the surrounding second, or a mean
grey change of at least 15 %, is a visual event. This catches cuts on
white-background motion graphics where a plain scene score finds almost
nothing.

## Data assets

The bundled launch profile (7 professional launch films) belongs to the
timeline unit's spec. Its format is the profile file above. Status
**NEEDS-DATA** (films we do not have). Fallback: no bundled profile; the
user builds one with `hark profile <their references> -o <file>` and passes
it to critique. A profile built from fewer than 5 films carries the
warning.

## Errors and edge cases

- Missing path, folder given as a file, no audio track, undecodable file,
  shorter than 3 s, silent: skipped with a warning; the run continues.
- Every file skipped: input error, exit 1.
- A styles file that does not exist or is not valid JSON: error, exit 1.
- A styles list shorter than the file list: the rest get "".
- An audio-only file: no picture metrics (`cut_rate` and the sync shares
  null, `fps` null).
- Fewer than 3 visual events: sync shares and chance null; `cut_rate` set.
- No beats (or at most 4): `tempo` null.
- `--no-ast`: `music_frac`, `speech_frac` null, `music_windows` null.
- A mono mix: `side_to_mid_db` = -40 and `mono` true.
- NaN or infinite metric values: null.

## Acceptance tests

Fixtures are generated: `film.wav` = 30 s stereo of pink noise at -30 dBFS
RMS with a hit (a 5 ms click into a 0.4 s decaying noise burst at -6 dBFS)
at 5, 10, 15, 20 and 25 s, a 0.5 s dip of -20 dB at 14.3 s, silence from
28 s to 30 s.

- **PRO-1** Salient hits. `film.wav`: hits found within 30 ms of 5, 10, 15,
  20, 25 s; `hit_rate` == 5 / 30 rounded to 2 decimals.
- **PRO-2** Stop-down. `film.wav`: one stop-down starting near 14.3 s
  (±0.1), length 0.4 to 0.6 s, depth >= 10 dB.
- **PRO-3** Ending. `film.wav`: `end_silence_s` in [1.8, 2.2]; `first_sound_s`
  < 0.1.
- **PRO-4** Arc. `film.wav`: `arc` has 11 values, none null; the value at
  100 % is the lowest of the 11 (the silent end pulls it down).
- **PRO-5** Short file. A 6 s file: `short` true; `lra`, `crest_lu`,
  `intro_lu`, `end_lu`, `final_lu` null; `peak_pos` set (d >= 6).
- **PRO-6** near and chance. near([1, 2, 3], [1.05, 2.5], 0.1) == 1/3;
  near([], [1], 0.1) is None; near([1], [], 0.1) == 0; chance uses the 16
  offsets modulo the duration.
- **PRO-7** Profile of 2 files. Two generated files of 12 s and 15 s:
  `n` == 2; every metric stats has `values` of length 2; the warning about
  fewer than 3 films is present; `films[i].id` == `ref_1`, `ref_2`; no path
  or file name appears anywhere in the profile JSON.
- **PRO-8** Skips. Inputs: a 2 s file, a silent 10 s file, a missing path and
  one good 12 s file -> 1 film, 3 skip warnings on stderr, a warning count
  in `warnings`.
- **PRO-9** Nothing usable. Only a 2 s file -> input error, CLI exit 1 with
  `hark profile: error: no usable reference films: ...`.
- **PRO-10** Styles. A styles object `{"b.wav": "kinetic type"}` with files
  a.wav and b.wav -> styles "" and "kinetic type"; a styles list ["x"] ->
  "x" and "".
- **PRO-11** Picture sync. A generated 12 s video with a hard cut every 2 s
  and a click on every cut: `cut_rate` ≈ 0.5; `cuts_with_sound` >= 0.8;
  its `chance` value is lower than the measured share.
