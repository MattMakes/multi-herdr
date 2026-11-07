# hark timeline: critique part (launch-video sound scorecard and the launch profile)

Part of the timeline unit spec. Index: `docs/specs/hark/timeline.md`. This part covers `hark critique`, the
bundled launch profile asset and the launch-film playbook facts that the scorecard encodes.

## 1. Behaviour

`hark critique FILE` compares a soundtrack or a film with a profile of professional launch films (by default the
bundled profile of 7 films) and says what to fix.

- With a video FILE, its own picture gives the visual events. `--video V` checks a separate soundtrack against
  its picture. `--profile P.json` scores against the user's own references (made by the listen unit's profile
  command). `--fast` skips the AudioSet pass (music and speech presence) and the realness pass; the verdict is
  then marked partial.
- Each metric row has: value, the ok range, the typical band (p10-p90 of the references), the reference median, a
  status (`ok`, `low`, `high`, `n.a.`), a weight (`critical`, `major`, `minor`; `info` rows are style choices that
  are shown but not scored), why it matters, and concrete actions with timestamps.
- Ranges come from the profile: the min-max of the reference films widened by a per-metric tolerance (the
  measurement precision). Loudness and true peak instead follow web-delivery practice and show the references for
  context only.
- Realness (`p_real`) comes from the listen unit's realness model when it is installed, with that model's verdict
  thresholds.
- The verdict weighs critical 3, major 2, minor 1.
- "Sound on cuts": the scored share counts visual events with an onset (a new attack) within plus or minus 100 ms,
  the definition the reference ranges were measured with. A whoosh swelling through a cut, or music already
  playing, is not an onset. The fixes and tips instead name the visual events that get no sound at all by the
  timeline matcher (analysis part, section 2.7: no onset within 100 ms, no whoosh or riser peak within 300 ms, no
  level peak within 150 ms), so `critique` and `timeline` list the same cuts.
- A file that is one of the profile's own reference films is scored leave-one-out against the other references,
  and the output says so.
- A file shorter than 3 s, or one with nothing above the -70 LUFS gate, gets no score. A file shorter than 10 s
  gets `n.a.` on the loudness-shape rows.
- Input problems raise the listen unit's input error with a plain message.

What the numbers cannot judge, and the text must not claim: taste (does the music fit the brand), the sync of
single layers inside a dense mix, over-compression (two references are as dense as a crushed mix), clips under
10 s for the shape rows, and styles outside the profile (build a profile of that style).

## 2. Algorithms

### 2.1 Inputs and measurement

The per-film measurement is the listen unit's `film_metrics(path, video, use_ast)` and its cached audio pass
(index, dependency on listen). The critique uses from it:

- `metrics`: the metric values (keys in table 3.1);
- `chance`: chance values of the 4 sync shares;
- `arc`: 11 loudness-arc points;
- `events`: `visual` (visual event times), `visual_strength` (their frame differences), `hits` (salient hit
  times), `stopdowns`, `beats`, `music_windows` (`[t, music p, speech p]` per 5 s window, or null);
- `duration`, `fps`, `lossy`, `mono`, `short`, `video`;
- from the audio pass: `onsets` (attack-aligned onset times); `tf, Lf` (100 ms loudness, 20 ms hop); `ts, Ls`
  (3 s loudness, 0.1 s hop).

The metric definitions (listen unit; summarised here so the fix rules read clearly):

| key | meaning |
|---|---|
| `lufs_i` | integrated loudness (BS.1770) |
| `true_peak` | true peak, 4x oversampled |
| `lra` | loudness range |
| `crest_lu` | loudest 3 s short-term minus integrated |
| `intro_lu`, `end_lu` | energy mean of the momentary loudness in the first / last 10 % (at least 0.5 s) minus integrated |
| `peak_pos` | position of the loudest 3 s, 0 = start, 1 = end (files of at least 6 s) |
| `first_sound_s` | first 100 ms block within 20 LU of integrated, refined to a 2 ms edge |
| `max_gap_s` | longest stretch between first and last sound where the 100 ms level stays 30 LU under integrated |
| `end_silence_s` | dead air after the last block within 30 LU of integrated |
| `fade_s` | last moment at full level (momentary within 6 LU of integrated) to the end of sound, minus 0.2 s |
| `final_lu` | loudest momentary value of the last 3 s of sound minus integrated |
| `tail_s` | strongest onset of the last 4 s to the end of sound, when it is in the top quarter of onset strengths |
| `music_frac`, `speech_frac` | share of 5 s windows with AudioSet Music >= 0.3 / Speech >= 0.5 |
| `tempo` | BPM of a second beat tracker |
| `onset_rate` | onsets per second |
| `hit_rate` | salient hits per second (100 ms level jumps >= 6 dB within 80 ms, landing within 8 LU of integrated) |
| `stopdowns_min` | dips >= 10 dB under the preceding 2 s median for 0.15-1.5 s that come back within 0.3 s, per minute |
| `cut_rate` | visual events per second |
| `cuts_with_sound` | share of visual events with an onset within 100 ms (needs at least 3 visual events) |
| `cut_hit_frac` | share with a salient hit within 100 ms |
| `cut_frame_sync` | share with an onset within 1 frame plus 6 ms |
| `cuts_on_beat` | share within 70 ms of a tracked beat |
| `band_sub` .. `band_air` | energy share in dB per band: sub 20-60, low 60-250, lowmid 250-1k, mid 1-4k, presence 4-8k, air 8-16k Hz |
| `brightness_hz` | median magnitude-weighted spectral centroid |
| `side_to_mid_db` | side vs mid energy in dB; a mono mix (identical channels) counts as -40 dB |
| `correlation` | L/R correlation |

Files shorter than 10 s get `null` for `lra`, `crest_lu`, `intro_lu`, `end_lu`, `final_lu` (the shape set).

Each metric has a group, a label, a unit and a display precision (listen unit). The units used by the range rule
below: `s` for `first_sound_s`, `max_gap_s`, `end_silence_s`, `fade_s`, `tail_s`; `/s` for `onset_rate`,
`hit_rate`, `cut_rate`; `/min` for `stopdowns_min`; `share` for `music_frac`, `speech_frac` and the 4 sync
shares. Value formatting (listen unit): a share prints as a whole percent; units `0-1` and `r` print without a
unit; others print the value with the metric's decimals and its unit; `null` prints `n.a.`.

### 2.2 Run order

1. When `--video` is the same file as FILE (absolute paths) and FILE has no picture, drop the video.
2. Check FILE for an audio stream and the video (when given) for a picture (listen unit `media_problem`). A
   problem raises the input error with its message: file not found; a folder, not a media file; not a readable
   audio or video file; no audio track; no picture (no video stream).
3. Load the profile (section 2.3).
4. Unscorable check (listen unit): duration under 3 s, or integrated loudness at most -69.5 (silent). Then return
   the unscored result (section 2.10) at once, before any heavy pass.
5. Measure the film (`film_metrics`) and read the audio pass.
6. Reference match (section 2.4). When it matches, replace the profile by the leave-one-out profile.
7. Cut sync (section 2.5).
8. Values = the film metrics plus `p_real` = null. When realness is on: run the listen unit's realness on FILE and
   keep `{p_real, verdict, synth_windows, segments, summary}`; set `p_real`. When realness fails for any reason,
   keep `{error: "<type>: <message>"}` and leave `p_real` null.
9. Scorecard (section 2.6).
10. Actions: for each row with status `low` or `high`, compute its actions (section 2.7) and attach them to the row
    as `actions`. Each action becomes `{metric, weight, w, dist, action}` where `w` is the row's final weight
    number and `dist = |value - edge| / max(tolerance, 0.001)`, with `edge` the violated range end. Sort all
    actions by larger `w` first, then larger `dist`.
11. Partial list: `realness` when `p_real` is null; `music presence` when `music_frac` is null.
12. Tips (section 2.8), with the set of metrics that produced at least one action.
13. Not measured: for each `n.a.` row whose weight is not `info`, group the metric under its reason (section 2.9).
14. Summary and text (section 2.11).

### 2.3 Profile loading

- A profile object passed in by code is used as is.
- A path that is not a file: input error `profile not found: <path>`.
- No path: the bundled profile (data asset A1 in the index).
- A file that does not parse as JSON (or is not text): input error that says it is not a profile JSON and how to
  make one, with the parse reason.
- Parsed JSON that is not an object or has no `metrics` object: the same input error, saying `metrics` is missing.

### 2.4 Reference match and leave-one-out

**Match**: for each film in the profile's `films`: skip it when its duration differs from FILE's by more than
0.05 s. Compare these metrics where both sides have a value: `lufs_i` (tolerance 0.15), `true_peak` (0.15),
`brightness_hz` (15), `band_mid` (0.15). When at least 3 are compared and all are within tolerance, the match is
that film's `id` (or the text "a reference" when it has none). Else no match.

**Leave-one-out profile**: every profile film except the match (each with `video` set from its `has_video`, true
when absent, and `style` defaulting to empty), aggregated by the listen unit's profile aggregation. It needs at
least 2 other films; else keep the profile unchanged. Add `loo_of: <id>` and `n_full: <old n>`.

### 2.5 Cut sync by the timeline matcher

When the film has visual events: decode FILE, detect events on the whole file (analysis 2.2), make level envelopes
(analysis 2.3), and run sync (analysis 2.7) with the visual event times, no beats, frame duration `1/fps` (when
fps is known) and the envelopes. Any failure gives `null`; the onset-based lists are then the fallback. Without
visual events, `null`.

From it: **no sound at all** = times of items with status `no sound` or `off`; **level peak only** = times of
items with status `peak`. Both are `null` when cut sync is `null`.

### 2.6 Scorecard

For each metric of table 3.1, in table order:

1. `stats` = the profile's entry for the metric (may be empty).
2. Window `(lo, hi, basis)`:
   - `p_real`: `(realness file hi threshold, null, "realness model")`.
   - `true_peak` on lossy decoded audio: `(null, 0.0, "decoded lossy")`.
   - A metric with a practice window: that window, basis `web` (`lufs_i` -16..-9; `true_peak` upper -1.0).
   - No stats or `n` = 0: `(null, null, "no reference data")`.
   - Else `lo = min - tol`, `hi = max + tol`. When `lo <= 0` and the unit is `s`, `/s`, `/min` or `share`: `lo`
     becomes `round(min/2, 3)` when the unit is `/s` and `min > 0` (a density floor at half the sparsest
     reference), else `null`. Then keep `lo` only when the sides are `both` or `lower`, `hi` only when `both` or
     `upper`. Basis `references`.
3. Status: `n.a.` when the basis is "no reference data". Else for a scored metric (weight > 0): `n.a.` when the
   value is null, `low` when below `lo`, `high` when above `hi`, else `ok`. For an info metric: `n.a.` when null,
   else `info`.
4. Weight adjustment:
   - `p_real` with status `low`: critical (3) when the value is below 0.5, else minor (1). No near-miss rule.
   - Else a critical metric that is `low` or `high` by at most 2 tolerances from the violated edge counts as major.
5. Row: `{metric, label ("realness (p_real)" for p_real, else the listen label), group ("realness" for p_real),
   value, unit ("p" for p_real), range: [lo, hi], basis, median, typical: [p10, p90] when stats has n else null,
   status, weight (word), why}` plus `chance` when the film's chance map has the metric.

Score and verdict:

- Scored rows: weight not `info` and status not `n.a.`.
- `score = sum(weights of ok scored rows) / sum(weights of scored rows)`, 3 decimals; `null` with no scored row.
- `critical_issues` = scored rows of weight 3 that are not `ok`; `major_issues` = weight 2 not `ok`.
- Verdict: `n.a.` when score is null; `launch-grade` when score >= 0.9 and no critical issue; `close` when
  score >= 0.75 and no critical issue; `needs work` when score >= 0.5; else `not launch-grade`.
- Also `in_range` (ok scored rows) and `scored` (scored row count).

The scorecard function takes `(values, profile, chance, lossy)` and has no side effects, so the leave-one-out
evaluation can call it.

### 2.7 Fix text rules (actions)

Every action is one ASCII sentence written in your own words. Each must contain the numbers named below. `rng`
is the text `refs <min> .. <max>` with the profile's min and max formatted for the metric, or empty when the
profile has no values. Times use `fmt_t`. A time list shows the first 8 times and then `(+N more)`.

| metric, status | the action must state |
|---|---|
| `lufs_i` low | raise the master by `(-14 - value)` dB (signed, 1 decimal) to about -14 LUFS with a true-peak limiter at a -1 dBTP ceiling; compress the music bus first when the limiter would work harder than about 3 dB; `rng` |
| `lufs_i` high | turn the master down by `(value + 12)` dB to about -12 LUFS, because above -9 LUFS normalising platforms turn it down anyway and the limiter flattens every hit; `rng` |
| `true_peak` high | the true peak (signed, 1 decimal); for lossy decoded audio, that it already clips on playback; end the chain with a true-peak limiter at -1.0 dBTP and re-encode |
| `p_real` low | rules below |
| `music_frac` low | the music share in percent; the time ranges with no music (each 5 s window with Music < 0.3 as `[t, min(dur, t + 5)]`, touching ranges merged, first 6 listed); run one continuous track under the whole film and duck it under voice instead of stopping it; `rng` |
| `cuts_with_sound` low, cut sync available | how many visual events of the total have no onset within 100 ms; `rng`; then either the no-sound times (advice: a whoosh into the transition peaking on the cut, a hit or a UI click there) or that every one has some sound under it; then, when `missing - no sound > 0`, that many have sound playing through them (a swell, music or a level peak) but no new attack, and to add an accent on the ones that matter |
| `cuts_with_sound` low, no cut sync | the same counts and `rng`, the same advice, and the missing times |
| `lra` high | the swing in LU; the quietest body stretch and the loudest 3 s (below); lift the quiet parts or compress the music bus 2-4 dB |
| `crest_lu` high | the loudest 3 s (time and level) standing `value` LU above integrated; the rest plays quiet next to it; lift the body (quietest stretch) or tame that peak |
| `lra` low | the LRA value; the level never moves; give the film sections (a calmer voice or UI passage, a lift into the reveal) or ease the limiter |
| `crest_lu` low | the value; no moment stands out; let the main reveal or logo hit rise 2-3 LU above the body or ease the limiter |
| `first_sound_s` high | when the energy mean of the 100 ms levels before the first sound is above -60: nothing reaches body level until the time, the level before it (0 decimals) and how many LU under the body; raise the opening or start the music at 0:00. Else: silence until the time; start music, ambience or a riser at 0:00. Both with `rng` |
| `end_silence_s` high | the dead air in seconds after the sound ends at `dur - value`; trim the picture or let the final sting ring out over the end card; `rng` |
| `max_gap_s` high | the longest hole; the holes (below), first 6; keep a music bed or room tone under them, or make each a deliberate stop-down that ends on a hit |
| `final_lu` low | the final moment sits `-value` LU under the body; end on a logo sting or hit at the last visual event (its time, when one exists) instead of fading away |
| `final_lu` high | the final hit is `value` LU above the body; pull it down 2-3 dB |
| `onset_rate` low | sparse, the rate; music with more drive, or UI foley (clicks, typing, ticks) on the on-screen actions |
| `onset_rate` high | busy, the rate; thin out the SFX layer and keep accents for the important moments |
| `hit_rate` low | "no" (rate 0) or "few" salient hits, the rate (2 decimals); add impacts or accents on the key moments; the biggest visual events without a hit (visual events by strength, largest first, without a salient hit within 0.1 s, first 6, listed by time) |
| `hit_rate` high | too many; keep the biggest for reveals, soften the rest by 3-6 dB |
| `intro_lu` low / high | the opening span `0:00-fmt_t(0.1*dur)`; low: how far under the body, start closer to full level or shorten the build-up; high: how far louder, bring it down or lift the body |
| `end_lu` low / high | the ending span `fmt_t(0.9*dur)-fmt_t(dur)`; low: keep the energy up into the end card and finish on a sting; high: bring it down or lift the body |
| `fade_s` high | the fade length; end on a sting with a short ring-out, or cut hard on the last picture cut |
| `fade_s` low | the end is cut hard; let the last hit ring out briefly |
| band rows | the band advice of table 3.3 (low or high), then the formatted value and `rng` |
| `side_to_mid_db` low | for a mono mix: identical left and right channels; else nearly mono with the side value; use stereo music and ambience, pan SFX with the motion |
| `side_to_mid_db` high | very wide with the side value; check the mono fold-down, narrow wide ambience or reverbs |
| any other metric | no action |

Every row's action also ends with `rng` in parentheses where the table says so.

**Quietest body stretch and loudest 3 s** (for `lra` and `crest_lu` high): on the 3 s loudness curve `(ts, Ls)`,
the body is `0.1*dur < ts < 0.9*dur`. The quietest stretch is `[max(0, ts[q] - 1.5), ts[q] + 1.5]` with level
`Ls[q]` (1 decimal) where `q` is the body minimum; the loudest is the global maximum `ts[l]`, `Ls[l]`. When the
body is empty, a `high` status falls back to the `low` sentence of the same metric (only reachable on very short
files, which have no shape metrics).

**Holes** (for `max_gap_s`): walk the 100 ms curve `(tf, Lf)` with integrated `I`. A run of frames with `Lf < I -
30` closes at the first frame that is not below. Keep a closed run when its length times the hop is at least 0.5 s,
it starts after `first_sound_s` (0 when null), and the closing frame time is before `dur - end_silence_s` (0 when
null). Report `[run start - 0.05, run end + 0.05]`. A run still open at the end is not reported.

**Realness rules** (`p_real` low):

1. Thresholds `t` = the realness file thresholds when realness returned 4 s window segments, else the clip
   thresholds. Window thresholds `tw` = the clip thresholds (each 4 s window is judged like a clip).
2. Segments = the 4 s windows with a `p_real`. Flagged = segments with `p_real <= tw.lo`, lowest first.
3. **Quiet miss**: when `p_real >= 0.5` and no segment is below 0.5, there is no action (a tip instead).
4. Head: when `p_real <= t.lo`, "likely code-synthesized" with the value and the `lo` threshold; else when below
   0.5, "leans code-synthesized" and that the realness model calls `lo..hi` uncertain; else "uncertain", and that
   real or pro-produced sound scores at least `hi`.
5. Pick windows: go through the flagged list (or, when nothing is flagged, all segments lowest first) and keep a
   window when its start is at least 2.0 s from every kept start. Keep the first 6 (flagged) or 3 (not flagged).
   List them by start as `start-end (p)` (p with 2 decimals), plus `(+N more)` when more flagged windows exist.
6. Where: "4 s windows at or below the window threshold `tw.lo`" with the list when flagged; "lowest-scoring 4 s
   windows" with the list when not flagged; nothing when there are no segments.
7. Fix: below 0.5, replace oscillator or noise layers with real recordings and produced music, and point to the
   `sound-sourcing` skill for sourcing and licences; at 0.5 or above, find and replace the layer that sounds
   synthetic. Both only "if the film should sound recorded or produced". Always add: judge each stem with
   `hark realness stem.wav`, because a finished mix is harder to judge than its sources.

**p_real number format**: 2 decimals, but 3 decimals when the 2-decimal text equals a threshold (`lo` or `hi`)
that the value is not actually equal to (so 0.398 does not print as 0.40 when 0.40 is the threshold).

**Realness thresholds**: the listen unit's realness model thresholds `{lo, hi}` for files (applied to a long
file's mean `p_real`) and for clips. When the model is not installed or fails, both are `{lo: 0.5, hi: 0.5}`.
Cache the lookup per process.

### 2.8 Tips (not scored)

In this order:

1. **Unsounded visual events**: when the no-sound list is not empty, `cuts_with_sound` has a value and
   `cuts_with_sound` produced no action: how many of the visual events get no sound at all, the 3 matcher criteria,
   the times; give the ones that matter a whoosh into the cut, a hit or a UI click, or keep one as a deliberate
   stop-down; plus, when the level-peak-only list is not empty, its count and up to 4 times to check on the sheet.
2. **Realness not confirmed**: when `p_real` has a value, produced no action, is below the file `hi` threshold,
   and is a quiet miss: the value, the `hi` line, that nothing synthetic was found (with the lowest window value
   when there are windows), no layer to replace on this evidence, and to check the stems with `hark realness`.
3. **No stop-downs**: when `stopdowns_min` is 0 and there are visual events: 4 of 7 references drop the sound out
   for 0.2-1 s right before a big hit; try one before the biggest visual change (largest `visual_strength`), with
   its time.
4. **Voice**: when `speech_frac >= 0.5`: the voice share in percent; duck the music 3-6 dB under the voice and keep
   SFX out of 1-4 kHz while it speaks.

### 2.9 Not-measured reasons

| metric | reason |
|---|---|
| `p_real` | skipped by `--fast`; else realness unavailable |
| `music_frac` | skipped by `--fast`; else no AudioSet windows |
| `cuts_with_sound` | no picture (pass `--video film.mp4`); else fewer than 3 visual events |
| a shape metric on a file under 10 s | clip under 10 s; the references are 20-93 s films |
| any other | not measurable on this file |

### 2.10 Unscored result

For a file under 3 s or silent: `{path, video, duration (3 decimals), verdict: "n.a.", score: null, in_range: 0,
scored: 0, critical_issues: 0, major_issues: 0, partial: [], actions: [], tips: [], metrics: [], summary, text}`.
The summary (equal to the text) names the file and the reason (shorter than 3 s with its length; or silent,
nothing above -70 LUFS), says there is nothing to score against launch films, and points to the single-sound
commands (describe, realness). The listen unit's unscorable check must not run the heavy audio pass.

### 2.11 Result, summary and text

**Result fields**: `path`, `video` (the picture actually used), `duration`, `profile_n` (the profile's `n` after
leave-one-out), `lossy`, `profile` (`bundled launch films` when none was given, the path string when a path was
given, `custom` for an object), `verdict`, `score`, `in_range`, `scored`, `critical_issues`, `major_issues`,
`partial`, `actions` (`[{weight, metric, action}]`), `tips`, `metrics` (rows), `reference_match` (`{id,
scored_against}` or null), `events` (`visual`, `visual_without_onset` = visual events with no onset within
0.1 s, `visual_without_any_sound` = the no-sound list (or null), `visual_without_hit` = visual events with no
salient hit within 0.1 s, `hits`, `stopdowns`), `arc` (`{film, refs_median, refs_p10, refs_p90}` from the film and
the profile arc), `realness`, `seconds` (wall time, 1 decimal), `not_measured` (`{reason: [metrics]}`), `summary`,
`text`.

**Summary** (one ASCII paragraph): the file name, "vs N reference films (profile)"; the leave-one-out note when it
matched (the file is reference X of the profile, so it is scored against the other N references, and its own
values do not set the ranges); the verdict line; "Fix first:" with the first action (end with a period), or a
sentence that nothing needs fixing against the references; and "Not measured:" with each reason and its metrics
when any.

**Verdict line**: the verdict; `(partial: <names> not checked)` when partial; "- N of M scored metrics in range
(weighted P%)" and the critical and major counts.

**Text** (`critique_text`), lines:

1. A title: the command, the file name, its duration (1 decimal) and "vs N reference films (profile)".
2. The leave-one-out note when matched.
3. `VERDICT: <verdict line>`.
4. When partial with both names: an indented note to run without `--fast` before shipping (realness and music
   presence are critical checks); with one name: that it could not be measured.
5. When there are actions: `FIX FIRST (most important first):` then numbered lines `N. [weight] action`.
6. Each tip as an indented `tip: ...` line.
7. `NOT MEASURED: ...` when any.
8. A `SCORECARD` header with columns value, ok range, ref median, status; then one line per non-info row: group
   (8 characters), label (30), value (formatted; `p_real` by the p_real format; `n.a.` when null), the range text
   (30), the median, and the status with ` (weight)` added when the status is low or high.
9. When the film arc and the profile median arc exist: a `LOUDNESS ARC` header (LU vs integrated at 0 %, 10 % ..
   100 %; not scored), a line for this film and a line for the reference median (`+6.1f` per point, `-` for null).
10. A `NOT SCORED` header for style choices, then each info row with a value: label, formatted value, `(chance X)`
    when it has a chance, and its range text.
11. `realness: <summary>` when realness gave one, else `realness: not available (<error, first 120 characters>)`
    when it failed.

**Range text**: for info rows, `refs p10-p90 a .. b` when the typical band exists, else empty. For scored rows:
the basis when both ends are null; `>= lo`, `<= hi` or `lo .. hi`; plus ` (<basis>)` when the basis is not
`references`.

### 2.12 CLI

`hark critique FILE [--video V] [--profile P] [--fast] [--json] [--out O] [--threads N]`. An input error or a core
decode error prints one line on stderr, `<program> critique: error: <message>` (core-io error-line format), and
exits 1. Else print the text (or JSON with `--json`), write JSON to `--out` when given, exit 0.

## 3. Constants

### 3.1 Metric specification (weight, tolerance, sides, practice window)

Weights: 3 critical, 2 major, 1 minor, 0 info.

| key | weight | tol | sides | practice | why (paraphrase) |
|---|---|---|---|---|---|
| `lufs_i` | 3 | 1.0 | both | -16 .. -9 LUFS | social and site players do not normalise; a major video site turns masters above -14 down |
| `true_peak` | 2 | 0.5 | upper | <= -1 dBTP | above -1 clips once encoded; decoded lossy above 0 clips on playback |
| `p_real` | 3 | 0 | lower | realness model | references use real recordings and produced music; synthesized sound reads cheap next to them |
| `music_frac` | 3 | 0.05 | lower | - | references are music-led; one track carries the energy and hides seams |
| `cuts_with_sound` | 2 | 0.05 | lower | - | viewers expect sound on motion; an unsounded edit point reads as lifeless |
| `lra` | 2 | 1.0 | both | - | launch films keep the body consistently strong |
| `crest_lu` | 2 | 0.5 | both | - | how far the loudest 3 s stick out |
| `first_sound_s` | 2 | 0.1 | upper | - | silence at the start loses autoplay viewers |
| `end_silence_s` | 2 | 0.25 | upper | - | dead air makes the ending feel unfinished |
| `max_gap_s` | 2 | 0.25 | upper | - | a silent hole mid-film reads as a mistake unless it is a deliberate stop-down |
| `final_lu` | 2 | 1.5 | both | - | the logo moment should land at body level or above |
| `onset_rate` | 2 | 0.3 | both | - | too sparse feels empty, too dense cluttered |
| `hit_rate` | 2 | 0.2 | both | - | salient hits mark the key moments |
| `intro_lu` | 1 | 1.5 | both | - | references open close to body level |
| `end_lu` | 1 | 1.5 | both | - | the last 10 % against the body |
| `fade_s` | 1 | 0.5 | both | - | long fade-outs lose the button |
| `band_sub` | 1 | 1.5 | both | - | sub weight |
| `band_low` | 1 | 1.5 | both | - | low-end body |
| `band_lowmid` | 1 | 1.5 | both | - | muddy vs hollow |
| `band_mid` | 1 | 1.5 | both | - | clicks and voice through phone speakers |
| `band_presence` | 1 | 1.5 | both | - | transient bite |
| `band_air` | 1 | 1.5 | both | - | openness; missing air sounds muffled |
| `side_to_mid_db` | 1 | 1.0 | both | - | near-mono sounds small, too wide collapses on phones |
| `speech_frac` | 0 | 0 | both | - | voice share (4 of 7 references have voice-over) |
| `tempo` | 0 | 0 | both | - | music tempo |
| `peak_pos` | 0 | 0 | both | - | where the loudest moment sits |
| `tail_s` | 0 | 0 | both | - | final hit ring-out |
| `stopdowns_min` | 0 | 0 | both | - | short drop-outs before hits |
| `cut_rate` | 0 | 0 | both | - | edit pace |
| `cut_hit_frac` | 0 | 0 | both | - | visual events with a salient hit (references near chance) |
| `cut_frame_sync` | 0 | 0 | both | - | visual events with an onset within a frame (near chance) |
| `cuts_on_beat` | 0 | 0 | both | - | visual events on the beat grid (near chance) |
| `brightness_hz` | 0 | 0 | both | - | perceived brightness |
| `correlation` | 0 | 0 | both | - | L/R correlation (1 = mono) |

Every key except `p_real` must exist in the listen unit's metric table.

### 3.2 Other numbers

| name | value |
|---|---|
| near miss of a critical rule | within 2 tolerances: counts as major |
| p_real critical line | below 0.5 |
| verdict lines | 0.90 launch-grade, 0.75 close, 0.50 needs work (no critical issue for the first two) |
| scorable | duration >= 3 s and integrated > -69.5 LUFS |
| shape metrics | need >= 10 s |
| reference match | duration within 0.05 s; lufs_i 0.15, true_peak 0.15, brightness_hz 15, band_mid 0.15; >= 3 compared |
| leave-one-out | needs >= 2 other films |
| onset / hit match radius | 0.1 s |
| holes | 30 LU under integrated, >= 0.5 s, edges 0.05 s |
| realness picks | 2.0 s apart; 6 flagged or 3 lowest |
| time lists | 8 times in actions and tip 1, 4 level-peak times |
| realness fallback thresholds | lo 0.5, hi 0.5 |

### 3.3 Band advice (paraphrase; write your own wording)

| band | low | high |
|---|---|---|
| `band_sub` | thin bottom: add a sub layer (a sub drop or kick) under reveals, or music with real low end | too much 20-60 Hz rumble: high-pass SFX and ambience at 30-40 Hz, cut the music's sub by 2-4 dB |
| `band_low` | weak low end: a fuller track or a low layer (bass, low hits) | boomy: cut 100-200 Hz on the music and heavy SFX by 2-3 dB |
| `band_lowmid` | hollow: lift 250 Hz-1 kHz on the music or use fuller recordings | muddy or boxy: cut 300-500 Hz on the music bed and roomy SFX by 2-3 dB |
| `band_mid` | recessed: lift 1-3 kHz on SFX or voice so they cut through phone speakers | honky or harsh: cut 1-3 kHz on the loudest layer by 2 dB |
| `band_presence` | dull: brighten SFX or use brighter recordings | harsh or sibilant: cut 5-7 kHz or de-ess the brightest layer |
| `band_air` | no air: use full-band recordings (44.1 or 48 kHz, no low-pass) | hissy or fizzy: low-pass or de-noise the noisiest layer |

### 3.4 Seed reference values (summary of the bundled profile; 7 films unless noted)

Facts measured on the source's 7 reference launch films (web MP4s with 128 kbps AAC audio). Ranges for scoring use
min and max; the median and p10-p90 are shown.

| key | median | p10 | p90 | min | max |
|---|---|---|---|---|---|
| `lufs_i` | -14.8 | -19.32 | -9.16 | -22.2 | -7.3 |
| `true_peak` | 0.6 | -0.26 | 1.52 | -0.8 | 2.0 |
| `lra` | 5.5 | 2.66 | 9.08 | 1.7 | 12.2 |
| `crest_lu` | 2.3 | 1.16 | 3.32 | 1.1 | 3.5 |
| `intro_lu` | -1.1 | -8.08 | 0.58 | -17.5 | 2.2 |
| `end_lu` | -3.0 | -11.96 | -0.66 | -14.3 | -0.3 |
| `peak_pos` | 0.75 | 0.096 | 0.826 | 0.03 | 0.88 |
| `first_sound_s` | 0.13 | 0.052 | 0.402 | 0.04 | 0.48 |
| `max_gap_s` | 0.02 | 0.0 | 0.688 | 0.0 | 1.66 |
| `end_silence_s` | 0.04 | 0.02 | 1.162 | 0.02 | 1.9 |
| `fade_s` | 0.27 | 0.092 | 2.288 | 0.08 | 2.54 |
| `final_lu` | 0.2 | -1.42 | 5.18 | -2.2 | 5.6 |
| `tail_s` (6 films) | 2.99 | 1.065 | 3.58 | 0.13 | 3.58 |
| `music_frac` | 1.0 | 0.802 | 1.0 | 0.67 | 1.0 |
| `speech_frac` | 0.83 | 0.0 | 1.0 | 0.0 | 1.0 |
| `tempo` | 123 | 115.2 | 136 | 108 | 136 |
| `onset_rate` | 5.66 | 3.27 | 6.876 | 2.46 | 8.22 |
| `hit_rate` | 1.25 | 0.748 | 1.6 | 0.1 | 1.66 |
| `stopdowns_min` | 1.2 | 0.0 | 9.06 | 0.0 | 18.3 |
| `cut_rate` | 0.63 | 0.306 | 1.42 | 0.3 | 1.6 |
| `cuts_with_sound` | 0.81 | 0.56 | 0.934 | 0.5 | 1.0 |
| `cut_hit_frac` | 0.29 | 0.078 | 0.488 | 0.03 | 0.53 |
| `cut_frame_sync` | 0.44 | 0.34 | 0.592 | 0.28 | 0.67 |
| `cuts_on_beat` | 0.22 | 0.198 | 0.448 | 0.18 | 0.52 |
| `band_sub` | -6.6 | -12.36 | -4.58 | -16.2 | -4.1 |
| `band_low` | -4.2 | -5.92 | -2.34 | -6.7 | -2.1 |
| `band_lowmid` | -5.3 | -16.38 | -3.2 | -16.8 | -1.7 |
| `band_mid` | -12.4 | -15.92 | -10.68 | -19.1 | -10.5 |
| `band_presence` | -17.8 | -30.24 | -13.52 | -30.3 | -13.1 |
| `band_air` | -23.6 | -31.86 | -14.74 | -34.2 | -11.2 |
| `brightness_hz` | 1966 | 1183.4 | 3030 | 1169 | 3633 |
| `side_to_mid_db` | -8.5 | -11.8 | -5.78 | -14.5 | -5.0 |
| `correlation` | 0.75 | 0.58 | 0.864 | 0.52 | 0.93 |

Chance of the sync shares (median of 7): `cuts_with_sound` 0.86, `cut_hit_frac` 0.27, `cut_frame_sync` 0.44,
`cuts_on_beat` 0.28. Median loudness arc (LU vs integrated at 0, 10 .. 100 %): -1.3, -0.8, +0.3, -0.3, -0.2,
+0.2, 0.0, -0.4, +0.9, -0.3, -7.4; p10-p90 between 10 % and 90 % about -5.8 .. +2.0 LU.

Resulting ok ranges with the bundled values (check values for an implementation): `onset_rate` 2.16 .. 8.52;
`hit_rate` floor 0.05 (min - tol <= 0, so half the min) .. 1.86; `music_frac` >= 0.62; `cuts_with_sound` >= 0.45;
`first_sound_s` <= 0.58; `end_silence_s` <= 2.15; `max_gap_s` <= 1.91; `fade_s` <= 3.04 (no lower bound);
`lra` 0.7 .. 13.2; `crest_lu` 0.6 .. 4.0; `final_lu` -3.7 .. 7.1.

### 3.5 Playbook facts (for the skill docs and the fix wording)

Measured on the 7 references, or established practice; the skills-and-docs unit restates them for agents:

1. Music leads: music in 89-100 % of 5 s windows in 6 of 7 films. Pick the track first and cut to it.
2. Loud, steady body: median -14.8 LUFS; loudest 3 s only 1.1-3.5 LU above integrated; LRA 1.7-12.2 LU.
3. Start at once: first sound within 0.04-0.48 s; 6 of 7 open within about plus or minus 2 LU of integrated.
4. Land the ending: last 3 s at body level (median +0.2 LU); short fades (median 0.27 s); short dead air (median
   0.04 s).
5. Dense rhythm, few big accents: 2.5-8.2 onsets per second, about 1.2-1.7 salient hits per second in 6 of 7.
6. Not a hit on every cut: sound on visual events is near chance in most references; deliberate accents go on the
   reveal, the logo, the key UI action and section changes.
7. Real recordings and produced music; check with realness.
8. Master for the web: about -14 LUFS, true peak -1 dBTP or lower on the master; 6 of 7 references clip after AAC
   decoding, a common flaw not to copy.
9. Whoosh into transitions: peak on the cut, start 0.2-0.6 s before it. Riser, stop-down (0.1-0.5 s), impact on
   the reveal, once or twice per film.
10. UI foley: one click per on-screen click on the press frame; one key per typed character from multiple takes;
    UI sounds short, bright (2-6 kHz) and about 10-15 dB under the music peaks.
11. Logo sting: a hit plus a 2-4 s tonal tail ending within about 0.5 s of the last frame.
12. Voice-over: duck the music 3-6 dB under it; keep SFX out of 1-4 kHz while it speaks.

## 4. Errors and edge cases

| case | behaviour |
|---|---|
| FILE missing | input error `file not found: <path>`; CLI exit 1 with the error line |
| FILE is a folder | input error: a folder, not a media file |
| `--video` without a picture | input error: no picture (no video stream) |
| `--video` equal to an audio-only FILE | video dropped (`video` null), no error |
| `--profile` missing / not JSON / no metrics | input errors (section 2.3) |
| silent file or under 3 s | unscored result, verdict `n.a.`, no actions, summary says nothing to score; the heavy pass does not run |
| file under 10 s | shape rows `n.a.` with the clip reason |
| mono mix | `side_to_mid_db` = -40 dB, status low, the action says mono |
| no picture | `cuts_with_sound` `n.a.` with the no-picture reason |
| fewer than 3 visual events | `cuts_with_sound` `n.a.` |
| realness absent or failing | `p_real` `n.a.`; partial; thresholds fall back to 0.5 / 0.5; the text says realness is not available |
| `--fast` | partial with both names; the text asks to run without `--fast` before shipping |
| a metric the profile has no data for | `n.a.`, basis "no reference data" |
| profile from fewer than 3 films | accepted; the listen unit's profile carries a warning that the ranges are far too narrow |
| a reference film critiqued | leave-one-out note; scored against the others |
| over-compressed mix | not flagged (by design: two references are as dense) |

## 5. Acceptance tests

Test soundtrack helper: stereo noise bed (amplitude 0.05) with decaying noise hits (0.15 s, amplitude 0.8, decay
30 ms); right channel = 0.9 x left + 0.1 x independent noise. Profile helper `prof(key=(lo, hi), ...)`: `n` 7,
min lo, max hi, median the mean, p10 lo, p90 hi.

| id | input | expected |
|---|---|---|
| C1 | profile onset_rate 2.5-8.0, music_frac 0.67-1.0, first_sound_s 0-0.5; values lufs_i -14.0, true_peak -0.5, onset_rate 1.0, music_frac 0.3, first_sound_s 0.0 | lufs_i ok, basis web; true_peak high; with lossy true: true_peak ok; onset_rate low with range [2.2, 8.3]; music_frac low and critical; first_sound_s range low end null and status ok; band_sub `n.a.`; tempo weight info |
| C2 | the same profile; lufs_i -17.5 vs -25.0 alone | -17.5 counts as major (near miss), -25.0 as critical |
| C3 | values lufs_i -14.0, onset_rate 5.0, music_frac 0.9 | verdict launch-grade, score 1.0 |
| C4 | profile hit_rate 0.1-1.66, music_frac 0.67-1.0; p_real 0.3 | status low, weight critical, range low end = the realness hi threshold; with lufs_i -14 and music_frac 1.0 the verdict is needs work |
| C5 | p_real 0.52 when the realness hi threshold is above 0.55; p_real 0.99 | minor; ok |
| C6 | hit_rate 0.0 against that profile | low, range low end 0.05 |
| C7 | the bundled profile | n 7; film ids ref_1..ref_7; no file names, `.mp4` or paths anywhere; every scored metric (except p_real) has n >= 6 |
| C8 | 12 s soundtrack, hits at 2, 6, 9 s, the first 1.5 s zeroed; fast mode | JSON-serialisable; a valid verdict word; first_sound_s high; an action for first_sound_s that contains `0:01.5`; cuts_with_sound `n.a.` and named in the summary; ASCII text |
| C9 | 8 s of zeros; 1 s of noise | verdict `n.a.`; no actions; the summary says nothing to score; no audio-pass cache file was written |
| C10 | 20 s of identical-channel noise, fast | side_to_mid_db value -40 and low; an action that mentions mono. A 6 s soundtrack: every shape row `n.a.`, the summary contains the clip-under-10 s reason, partial = [realness, music presence], and the text's second line mentions partial |
| C11 | missing file; a folder; an audio file as `--video`; FILE as its own `--video`; a missing profile path; a profile `{"n": 1}`; CLI `critique nope.wav --fast` | errors: file not found; folder; has no picture; `video` null (no error); profile not found; not a profile; exit 1 and stderr exactly `hark critique: error: file not found: <path>` |
| C12 | a profile built from 2 soundtracks without AudioSet; critique one of them against it | profile_n 2; music_frac `n.a.`; onset_rate scored; a scorecard of cuts_with_sound 0.1 against it is `n.a.` with basis no reference data; the profile warns about only 2 references |
| C13 | CLI `critique <8 s soundtrack> --fast --json` | parsed JSON has scored > 10, a summary, partial = [realness, music presence] |
| C14 | every SPEC key except p_real | exists in the listen metric table |

**Evaluation targets** (reproduce on equivalent data, index data asset A6):

| check | target |
|---|---|
| leave-one-out, each reference vs the other six | 125 of 154 scored checks in range (81 %); reference-range metrics 120 of 140 (86 %); no-overfit baseline about 71 % |
| leave-one-out verdicts | 1 launch-grade, 5 close, 1 needs work (the -22 LUFS sound-design film) |
| known-bad mixes (14) | 13 have the expected metric flagged; over-compression is not caught |
| quiet (-12 dB) | lufs_i low |
| hot (+12 dB, clipped) | lufs_i high and true_peak high |
| folded to mono | side_to_mid_db low |
| low-passed at 3 kHz (12 dB/oct) | band_presence and band_air low |
| 2.5 s silence before | first_sound_s high |
| 4 s silence after | end_silence_s high |
| 12 s fade-out | fade_s high and final_lu low |
| 2.5 s muted mid-film | max_gap_s high |
| SFX stem alone / ambience stem alone | music_frac low |
| a code-synthesized music bed | p_real low (critical) |
| 30 s 440 Hz sine; 30 s pink noise | p_real, music_frac and onset_rate low; verdict not launch-grade |
| a real-recordings soundtrack vs a synthesized one on the same picture | close (about 90 %) vs needs work (about 80 %, 1 critical: p_real) |
| visual events vs ffmpeg scene cuts | 80-100 % of scene cuts also found within 1 frame on films with at least 5 scene cuts |
