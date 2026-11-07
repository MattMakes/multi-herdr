# skills-and-docs, part 3: the sourcing, rights, music, variant, credits and mux scripts

Part of the `skills-and-docs` spec. Index: `docs/specs/hark/skills-and-docs.md`. The mixer, master and QA scripts are
in part 4 (`skills-and-docs-mixer.md`).

Scripts of `skills/sound-sourcing/scripts/` specified here:

| script | job |
|---|---|
| `analyze.py` | measures downloaded recordings (format, level, floor, bandwidth, onsets, spikes; with `--music` tempo, key, bars, sections) and draws spectrograms. |
| `rights.py` | the usage-rights policy module: classifies a rights text, loads manifests, names a source. No command line. |
| `rights_audit.py` | the usage-rights and file audit of every manifest entry. |
| `credits.py` | writes `CREDITS.md` and the video-description text for every audible recording. |
| `arrange.py` | fits a music track's bar grid and drafts a bar-line edit to the film's arc. |
| `variant.py` | applies design operations: a variant copy for a critic, or the agreed fix for the fixer. |
| `mux.py` | puts the soundtrack on the picture (stream copy of the video, AAC audio cut to the picture). |

The source's mux is a Node script. hark writes it in Python like the others (index, open question Q4).

Shared rules for every script:

- Run with the hark venv Python and `-I`. Every script except `rights.py` prints `--help`.
- Console output is ASCII only (other characters are replaced), for consoles that cannot print them.
- `--root` (default `.`) is the film project root. Relative paths resolve against it.
- Wording of messages is ours. Machine-read tokens (`PASS`, `FAIL`, `LOW`, `BLOCKER`, `warning`, `PROBLEM`, status
  words, JSON keys) are exact.

## 1. `analyze.py`

### 1.1 Interface

```
analyze.py FILE [FILE ...] [--music] [--png DIR] [--out OUT.json] [--json]
```

- One plain line per file, then a summary line `<n> file(s) analysed, <m> failed (<up to 6 names>)` plus `; wrote
  <out>` when `--out` is given.
- `--out`: the list of result objects as JSON (indent 1). Parent folders are created.
- `--json`: print the JSON list instead of the lines.
- `--png DIR`: one spectrogram image per file, `DIR/<file stem>_spec.png`.
- Exit 1 when any file is missing or fails to decode, else 0.

### 1.2 Behaviour

Decode with ffmpeg to 48 kHz float. A mono source is decoded as 1 channel and duplicated at unity gain (ffmpeg's own
mono-to-stereo conversion is 3 dB down, and the analyzer must report the true level). The probe reads the first audio
stream: codec, sample rate, channels, bit rate in kbps (the stream's or the container's, accepted only between 1 bit/s
and 10 Mbit/s).

Result fields (names exact; all levels in dBFS unless noted):

| field | how |
|---|---|
| `file`, `codec`, `src_rate`, `channels`, `kbps` | probe. A probe failure gives `probe_error`. |
| `duration` | decoded samples / 48000, 3 decimals. Under 20 ms: `error: "too short"` and nothing else. |
| `peak_dbfs`, `rms_dbfs` | over both channels. |
| `clipped_samples` | count of samples with \|x\| >= 0.999. |
| `dc` | mean of the mono mix. |
| `lufs` | BS.1770 integrated (pyloudnorm) for files longer than 0.45 s; null when silent. |
| `noise_floor_dbfs` | 10th percentile of 20 ms RMS frames of the mono mix. |
| `loudest_20ms_dbfs`, `dynamic_range_db` | max 20 ms RMS; loudest minus floor. |
| `side_to_mid_db` | RMS of (L - R)/2 minus RMS of (L + R)/2, in dB; null when the side is all zero (mono). |
| `attack_at` | time of the first 1 ms RMS frame >= 0.1 x the max 1 ms RMS (within 20 dB). |
| `peak_at` | time of the max 1 ms RMS frame. |
| `centroid_hz` | median of the per-frame spectral centroid. |
| `rolloff95_hz` | 90th percentile of the per-frame 95 % rolloff. |
| `band_db_rel_1_4k` | mean power of the bands 40-120, 8-12k, 12-16k, 16-19k, 19-22k Hz relative to 1-4 kHz, from one Hann-windowed FFT of the first 60 s of the mono mix. A low-bitrate encode shows as a cliff (16-19k far below 12-16k). |
| `onset_count`, `onsets` | onset detector (no backtracking), times in seconds, the first 40 listed. |
| `spikes_over_12db` | start times (0.1 s resolution) of 100 ms RMS windows more than 4x (12 dB) the median window; the first 30. Inside an ambience these are single events (a dog, a voice, a bump). |
| `level_std_db_100ms` | standard deviation of the 100 ms window levels in dB (a steady bed is small). |
| `png` or `png_error` | with `--png`. |
| `summary` | the plain line. |

Spectral features (centroid, rolloff, onsets, music features) run on a 22050 Hz mono copy, STFT n_fft 2048, hop 512.
For files over 400 s the source decimates by 2 without resampling, which labels a 24 kHz signal as 22050 Hz. hark must
resample properly for every length.

With `--music`:

| field | how |
|---|---|
| `tempo`, `beats` | librosa beat tracker, start tempo 120, tightness 200, beat times in seconds. |
| `tempo_from_ibi`, `ibi_std_ms` | 60 / median inter-beat interval; its standard deviation in ms (more than 4 beats). |
| `key` | top 3 of 24 keys: Pearson correlation of the mean CQT chroma with each rotated Krumhansl-Kessler profile; `[{key, r}]`, names `C C# D Eb E F F# G Ab A Bb B` + `major`/`minor`. |
| `rms_db_per_2s` | RMS per 2 s block. |
| `bar_starts_if_beat0_is_downbeat`, `rms_db_per_bar` | every 4th beat taken as a bar start (more than 8 beats), and the RMS dB of each bar. |
| `sections_at` | agglomerative segmentation of stacked, per-row-normalised chroma (12) and MFCC (13) into k = clamp(duration // 15, 4, 12) parts; the boundary times. A failure gives `sections_error`. |

Krumhansl-Kessler key profiles (public, from the music-cognition literature), index 0 = the tonic:

| | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| major | 6.35 | 2.23 | 3.48 | 2.33 | 4.38 | 4.09 | 2.52 | 5.19 | 2.39 | 3.66 | 2.29 | 2.88 |
| minor | 6.33 | 2.68 | 3.52 | 5.38 | 2.60 | 3.53 | 2.54 | 4.75 | 3.98 | 2.69 | 3.34 | 3.17 |

The spectrogram: ffmpeg `showspectrumpic` at 1400 x 420, combined channels, intensity colours, log scale, with a
legend.

The plain line names: file name, format (`<codec> <kbps> kbps <kHz> kHz <ch>ch`), duration, peak, LUFS (or "n.a."),
floor, attack, centroid, rolloff, mono or side/mid; with music the BPM (from the intervals when known), the interval
spread and the top key. Then `; CHECK: ` and a list when any of these holds:

| check | condition |
|---|---|
| digital silence | peak <= -90 dBFS |
| clipped samples | clipped_samples > 0 |
| low-bitrate mp3 | codec mp3 and kbps < 192 |
| low sample rate | src_rate < 44100 |
| dull | rolloff95 < 8000 Hz and not silent |
| spikes | any spike window (with the count) |

Rules of thumb the skill teaches with these numbers: foley and SFX want a floor under -55 dBFS between events, 0
clipped samples, rolloff above 12 kHz and no high-band cliff; an ambience wants a small level spread, side/mid above
-12 dB (real stereo), no spikes and at least 40 s of usable length; music wants a steady tempo (small interval
spread), a key, a per-bar loudness arc and section boundaries as edit points.

## 2. `rights.py` (policy module)

### 2.1 `classify(text) -> {key, status, name, url, attribution, reason}`

`status` is `allowed`, `forbidden` or `unknown`. Matching is case-insensitive on the stripped text. Steps, first hit
wins:

1. Empty text: key `none`, status unknown, reason "no rights recorded".
2. Forbidden terms (on the text with `_` read as a space), in this order, each with its reason:
   - `disputed`: the sources disagree.
   - the word `nc`, or `noncommercial` / `non-commercial`: a launch video is commercial use.
   - the word `nd`, or `noderiv...` / `no-deriv...`: trimming and syncing to picture are derivatives.
   - the word `sa`, or `sharealike` / `share-alike`: the whole film would have to carry the same licence.
   - `sampling`: Sampling+ and Sampling are retired and restrictive.
   - `personal`: personal use only.
   - `custom`, `free with credit` or `social media`: a custom term the audit cannot trust.
   - `editorial`: editorial use only.
   Result: key `forbidden`, status forbidden.
3. `restricted` anywhere: key `restricted`, status unknown (read the terms for this use).
4. Fixed allowed licences (status allowed, attribution false):

   | key | detected by | name | url |
   |---|---|---|---|
   | `cc0` | the word `cc0`, `cc-0`, `creative commons 0`, `publicdomain/zero`, the word `zero`, `public domain dedication` | CC0 1.0 (public domain dedication) | https://creativecommons.org/publicdomain/zero/1.0/ |
   | `pd` | `public domain`, `publicdomain/mark` | Public Domain Mark 1.0 | https://creativecommons.org/publicdomain/mark/1.0/ |
   | `mixkit-music` | `mixkit` and `music` | Mixkit Stock Music Free License | https://mixkit.co/license/#musicFree |
   | `mixkit-sfx` | `mixkit` without `music` | Mixkit Sound Effects Free License | https://mixkit.co/license/#sfxFree |
   | `pixabay` | `pixabay` | Pixabay Content License | https://pixabay.com/service/license-summary/ |

5. Only when step 4 found nothing: CC BY. The word `by` or `attribution`, followed within 24 non-digit characters by a
   version `1`-`4` with an optional `.0` or `.5`. Version 1.x: key `by1`, status unknown, attribution true (rare:
   confirm on the page). Else key `by` + the version without `.0` and without the dot (`by4`, `by3`, `by25`, `by2`),
   name `CC BY <x.y>`, url `https://creativecommons.org/licenses/by/<x.y>/`, attribution true, status allowed.
6. Reserved terms (`all rights reserved` or the word `copyrighted`): with no result from steps 4-5, forbidden ("all
   rights reserved"). With a result and the text `all rights reserved`: status unknown, the result's attribution, and
   a reason that says the text names both a free licence and all rights reserved.
7. Nothing found: key `unknown`, status unknown (read the item page and record the exact licence).

Known quirks to keep in tests: a text with the word `zero` reads as CC0; `CC BY 3.5` gives key `by35` with a URL that
does not exist. hark may tighten both; if it does, the tests change with it.

### 2.2 Helpers

- `rel(path, root)`: forward slashes; strips the absolute root prefix, matched case-insensitively (Windows paths).
- `load_manifests(folder) -> (entries, notes)`: every `*.json` in the folder, sorted, except `rights.json` and any file
  whose name starts with `rights.` (the audit and its backups). A file that is not valid JSON, or that is not a list
  (an object may hold the list under `entries` or `items`), gives a note and is skipped (a decision record such as
  `music_choice.json` is skipped this way). Entries are the dicts with a non-empty `file`; each gets `_manifest` = its
  manifest path.
- `source_name(entry)`: the host of `page_url` (else `download_url`) without scheme and `www.`, mapped by suffix:
  freesound.org -> `Freesound.org`, mixkit.co -> `Mixkit`, pixabay.com -> `Pixabay`, archive.org -> `the Internet
  Archive`, commons.wikimedia.org -> `Wikimedia Commons`, github.com -> `GitHub`; else the host; else the entry's
  `source` before any ` (`; else `unknown source`.

## 3. `rights_audit.py`

### 3.1 Interface

```
rights_audit.py [--root .] [--manifests audio/assets/manifests] [--report out/qa/mix_report.json]
                [--out <manifests>/rights.json] [--existing PATH] [--table PATH] [--fetch] [--fetch-all]
                [--decode scope|all|none] [--json]
```

| flag | meaning |
|---|---|
| `--report` | the mix report: in scope = the recordings in its `files_used`. |
| `--out` | the audit file to write; the agent fields are read from it first. |
| `--existing` | read the agent fields from this file instead of `--out`. |
| `--table` | Markdown table; default = `--out` with `.md`. |
| `--fetch` | compare the local bytes with `download_url`, in-scope entries only. |
| `--fetch-all` | the same for every entry. |
| `--decode` | full decode check for in-scope recordings (`scope`, default), every entry (`all`) or none. |
| `--json` | print the full result. |

Exit 2 when the manifest folder is missing or holds no entries (one line `rights_audit.py: <reason>`). Exit 1 when any
in-scope record is a blocker. Else 0.

Console: the summary, the manifest notes, then grouped findings of in-scope records that are not ok, blockers first and
the most frequent first: `  BLOCKER x<n>: <message>  [<up to 8 ids> ...]` or `  warning x<n>: ...`; then the files
written and the backups kept.

### 3.2 The audit record (one per manifest entry, the list written to `rights.json`)

The agent (the rights auditor) writes only `rights_seen`, `rights_confirmed` and `agent_notes`. The script computes
the rest on every run and keeps the agent fields, matched by id plus file path (or by id alone).

| field | meaning |
|---|---|
| `id`, `manifest`, `need`, `rank` | from the entry. |
| `in_scope` | with `--report`: the file is in `files_used`; else the earlier record's value; else true. |
| `severity` | `blocker` (any problem), `warning` (any warning), `ok`. Out of scope: `info` when not ok, else `ok`. |
| `rights_in_manifest` | the entry's `rights` text. |
| `rights`, `rights_status` | `classify()` key and status. |
| `rights_seen` | the auditor's reading of the item's page. |
| `rights_confirmed` | true only when the auditor set it AND `rights_seen` is not empty. |
| `attribution_required`, `attribution_ok`, `attribution_in_manifest` | CC BY credit check. |
| `file_ok` | the file exists and did not fail a decode. |
| `file_check` | `{path, exists, codec, sample_rate, channels, kbps, duration_s, video?, decodes_clean?, max_volume_db?, bytes_match_download_url?, fetch_note?}`. |
| `earlier_file_check` | evidence kept from an earlier audit (byte comparisons, hand checks). |
| `problems`, `warnings` | lists of messages. |
| `agent_notes` | the auditor's notes (credit forms, restrictions, doubts). |
| `audit_tool` | `sound-sourcing rights_audit v1`. A record without it is hand-written. |
| `duplicate_of` | for a second entry of the same file. |

### 3.3 Checks per entry

| check | problem (blocker) | warning |
|---|---|---|
| required fields | `file` or `rights` missing | `id`, `title`, `author` or `page_url` missing |
| classification | forbidden or unknown (with the reason) | |
| page reading | no `rights_seen`; or `rights_seen` without `rights_confirmed` | `rights_seen` classifies to a different known key than the manifest |
| CC BY credit | missing `attribution`, `author` or `page_url` | the attribution does not name the author (its part before ` (`); it names no licence (`cc by`, `attribution` or `creativecommons.org`) |
| URL ids | | see 3.4 |
| file | missing; ffprobe failed; decode errors; max volume <= -90 dB (digital silence) | carries a real video stream (not cover art); max volume < -60 dB; duration differs from the manifest analysis by > 0.05 s (file replaced?); sample rate < 44100; mp3 < 192 kbps |

When `rights_seen` is missing and the entry's `notes` contain a sentence where a rights or licence word is followed
within 40 characters by `read`, `verified`, `checked`, `seen` or `confirmed`, the problem quotes that sentence (up to
90 characters) and asks the auditor to copy the reading into `rights_seen`. A real gap the source met: 4 recordings
added after the hand audit, whose notes said the page was read. It was bookkeeping, not an unverified licence.

Decode check: one full ffmpeg decode of the first audio stream to a null output with volume detection. Lines tagged
error or fatal, or a non-zero exit, fail it; the last reported max volume is kept.

### 3.4 URL checks (all warnings)

Freesound page `freesound.org/people/<user>/sounds/<sid>`:

- an entry id `fs-<n>` with n != sid;
- a preview URL `previews/<folder>/<id>_<userid>-(hq|lq).(ogg|mp3)` whose id != sid, or whose folder != sid // 1000;
- an `-lq` preview (use `-hq`);
- `author` != `<user>` (case-insensitive).

Mixkit download `assets.mixkit.co/active_storage/sfx/<a>/<b>.(wav|mp3)`: a != b; an entry id `mk-<n>` with n != a.

### 3.5 Run order

1. Load the manifests. Load `files_used` from the report (relative, lower case). Load the earlier audit, indexed by
   (id, path) and (id, "").
2. Carry the agent fields from the earlier record. From a tool-written record: `rights_seen`, `rights_confirmed`,
   `agent_notes`, and its byte-comparison fields into `earlier_file_check`. From a hand-written record: the same two
   fields; its warnings, `problem` and `fix` become agent notes; its file-check fields that the tool does not compute
   go to `earlier_file_check`; its unknown keys are kept.
3. Audit each entry. A second entry for a file already audited is not audited again: it becomes an `info` row
   (`duplicate_of`, not in scope, one warning naming the first entry). If its rights text is not allowed, the FIRST
   record gets a problem (one recording has one licence). If it classifies to another allowed key, the first record
   gets a warning.
4. `--fetch` / `--fetch-all`: for each selected record with a `download_url` and a local file, sequentially, 1.5 s
   between requests: skip hosts that block scripts (pixabay.com: compare by hand); else HTTP GET with a clear
   User-Agent (for example `sound-sourcing-rights-check/1.0`), 60 s timeout, at most 200 MB, in memory, never run;
   compare SHA-1 with the local file. A mismatch is a warning (re-encoded or edited file).
5. With a report: every used file that no manifest lists becomes a blocker row (`id: null`, "used in the film but in
   no manifest").
6. Summary: entry count, in-scope count with ok / warning / blocker counts, rights keys in scope with counts, the
   number of records with auditor notes, byte-match counts (this run, and from earlier audits), and an instruction to
   fix blockers when any.
7. Before overwriting `--out`: copy it to `<stem>.prev.json`. If it is hand-written (no record has `audit_tool`, or it
   does not parse) and `<stem>.handwritten.json` does not exist, copy it there too.
8. Write the JSON list (UTF-8) and the Markdown table: heading, summary, then columns `id | in film | severity | rights
   | verified | credit | file | issues`, sorted in-scope first, then blocker, warning, info, ok, then id. `verified` is
   yes/NO, `credit` is n/a, ok or MISSING, `file` is `<codec> <kHz>k <ch>ch <dur>s` or MISSING, `issues` up to 300
   characters with `|` replaced.

## 4. `credits.py`

### 4.1 Interface

```
credits.py [--root .] [--report out/qa/mix_report.json] [--manifests audio/assets/manifests]
           [--design audio/sounddesign.json] [--rights audio/assets/manifests/rights.json]
           [--out CREDITS.md] [--desc out/CREDITS_description.txt] [--title "Film name"]
```

Exit 2 when the report or the manifest folder is missing, 1 when any problem is found, else 0. Console: a summary, the
2 files written, then `PROBLEM: <text>` per problem.

### 4.2 Behaviour

1. Read `files_used`. Match each file (case-insensitive, relative) to the first manifest entry with that file.
2. Problems: no manifest entry; rights not allowed (with the reason); CC BY without author, title or page_url; with
   `--rights`: no audit record for the file, or the record is not confirmed.
3. Split: music rows = entries of category `music` or with `/music/` in the path; the rest are effects. Sort effects by
   rights key in the order `by4, by3, by25, by2, cc0, pd, mixkit-sfx, mixkit-music, pixabay`, other keys last, then
   author (case-insensitive), then title.
4. Credit line per row:
   - Not CC BY, or CC BY without an `attribution`: `"<title>" by <author> (<page_url>), licensed under <name>` plus
     ` (<licence url>)` when the class has one.
   - CC BY with an `attribution` (the form the author asked for): that text, trailing full stop removed; append
     ` (<page_url>)` when it holds no `http` and no `www.`; append ` (<licence url>)` when it holds no
     `creativecommons.org`. CC BY asks for both links.
5. Processing sentence, derived from the design when `--design` is given (loaded with the mixer's loader). The verbs,
   in order: trimmed; cut into single hits (when slices exist); layered (when rules + free placements > half the
   samples + slices, or more than 1 rule) else placed; level-adjusted; equalised (any sample, slice or bus EQ); panned
   (any rule with an explicit non-zero `pan`, or any free placement pan); pitch-shifted by resampling (any sample or
   slice pitch or glide, any rule pitch or pitch ramp). Joined as `a, b and c`, then "for the film". Without a design:
   a fixed default list (trimmed, layered, level-adjusted, equalised, panned).
6. Music sentence: "Edited for the film:" then: cut and rearranged at bar lines (more than 1 segment) or trimmed to the
   picture; time-stretched from X to Y BPM (when `stretch` is not `none` and the ratio differs from 1 by more than
   1e-6); levels adjusted; a fade at the end (any `tail_fade`). Without a design: a fixed short sentence.
7. `CREDITS.md`: a level-1 heading (with `: <title>` when given); a line that says the credits live here and in the
   video description, where CC BY needs them; `## Music` with each line and the music sentence under it; `## Sound
   effects and ambience` with one sentence (real recordings from <sources, most used first>, the processing sentence,
   nothing synthesized), then one level-3 heading per rights class with its lines; `## Tools` naming ffmpeg,
   NumPy/SciPy, soxr, librosa and pyloudnorm (and Rubber Band when the music was stretched); `## PROBLEMS (fix before
   publishing)` with the problems, when any.
8. Description text: `Music:` with its lines and the music sentence; `Sound effects (CC BY):` with their lines and a
   sentence that every effect and ambience was modified (CC BY 4.0 section 3(a)(1)(B) asks to say a work was
   modified); a line listing the other authors (unique, sorted case-insensitively) with their rights classes (short
   names: CC0, public domain, Mixkit free licence, Pixabay licence); when no effect is CC BY, a sentence about the
   processing.
9. Summary: recordings credited (music and effects counts), CC BY count, problem count or "no problems". With CC BY
   music: advice to put the credit in the description and dispute a Content ID claim with the licence link.

Credits never need to be on screen.

## 5. `arrange.py`

### 5.1 Interface

```
arrange.py TRACK [--bpm X] [--phase auto|0..B-1] [--film-bpm 120] [--beats-per-bar 4]
           [--bars 30 | --arc ARC.json | --arc "-14,-12,...,0"] [--drop-film-bar K] [--drop auto|none|N]
           [--start-bar N] [--end-bar N] [--phrase 2] [--film-offset 0] [--jump 3.0] [--no-snap]
           [--file PATH] [--out PATH] [--json]
```

| flag | meaning |
|---|---|
| `--bpm` | the track tempo when known. The period is fixed to it; the phase is still fitted. |
| `--phase` | the downbeat beat phase, or `auto`. |
| `--film-bpm`, `--beats-per-bar` | the picture grid. Values in an arc JSON file win over these flags. |
| `--bars` | film bars, used only without `--arc`. |
| `--arc` | target dB per film bar: a `.json` file (part 2, 4.6: `{film_bpm, beats_per_bar, drop_film_bar, arc}`) or a comma list. |
| `--drop-film-bar` | 1-based film bar of the drop. Default: the arc file's value, else round(0.7 x film bars). |
| `--drop` | source bar (0-based) for the drop film bar: `auto` = the top drop candidate; `none` = the arc decides. |
| `--start-bar`, `--end-bar` | source bar for film bar 1 / for the last film bar. |
| `--phrase` | jumps prefer film bars that start a phrase of this many bars. |
| `--film-offset` | film time of film bar 1, seconds. |
| `--jump` | base cost of a jump. |
| `--no-snap` | keep the beat tracker's phase (skip the transient fit). |
| `--file` | the track path written into the music block. |
| `--out` | write the full result JSON. `--json` prints it instead of the table. |

Errors (exit with a message): `--drop` not auto/none/integer; track not found; fewer than 8 beats tracked (not a
steady track, or too short); fewer than 2 whole bars; `--drop`, `--start-bar`, `--end-bar` outside the source bars;
the drop film bar outside 1..film bars; a decode failure.

### 5.2 Algorithm

Audio: ffmpeg decode to mono 22050 Hz. STFT hop 256.

1. Default arc (no `--arc`), for n film bars with the drop at film bar d: big_until = d + max(2, round(0.6 (n - d))).
   For k = 1..n:
   - k < d: x = (k - 1)/max(1, d - 2). v = -14 + 10 min(1, x/0.45). If 0.55 <= x < 0.8, v = -9.5 (breakdown). If
     x >= 0.8, v = -9 + 4 (x - 0.8)/0.2 (build).
   - d <= k < big_until: v = 0 (the big section).
   - else: v = -13 + 2 min(3, n - k) (the resolve).
   Rounded to 1 decimal.
2. Beats: onset strength, beat tracker with start tempo (`--bpm` or 120) and tightness 400, times in seconds.
3. Grid fit: T0 = 60/bpm or the median beat interval. Beat numbers: 0, then the running sum of max(1, round(interval /
   T0)). Least-squares fit beat time = t0 + k T. Repeat 5 times on the inliers (|residual| < 35 ms); with fewer than 8
   inliers use all beats. Report the median absolute residual and the inlier fraction. With `--bpm`, T = 60/bpm.
4. Transient snap (unless `--no-snap`): the beat tracker's onset envelope lags real attacks by 20-50 ms, and a splice on
   a lagging grid chops the downbeat's attack. Compute a log-compressed spectral flux: Hann window 512 samples (23 ms),
   hop 64 (2.9 ms), log(1 + 100 |X|), the sum of positive frame-to-frame differences. Try shifts from -s to +s in 1 ms
   steps, s = min(100 ms, 0.3 T). Score each shift by the mean flux at the grid points t0 + shift + kT that stay
   inside the track. Take the best shift. The fit ratio = best score / median score. t0 += shift.
5. Grid: the beat times from (t0 mod T) to the track end, step T. bpm = 60/T.
6. Downbeat phase: low = the sum of STFT magnitudes below 150 Hz; low onset = its positive difference. Per grid beat:
   the max low onset in the frames from 2 before to 3 after the beat; harmonic change = 1 - cosine similarity of the
   mean chroma over the previous beat and over the next beat. z-score both series. Score of phase p = mean of the low
   z-scores at beats p, p + B, ... + the mean of the change z-scores at those beats. The phase = the best score (or
   `--phase`). Confidence = best - second. The alternative = the second (or the best when `--phase` chose another).
   LOW confidence = confidence < 0.5 with `--phase auto`.
7. Local disagreement: windows of 8 bars, hop 4 bars, over the summed z-scores. In a window where another phase wins
   by >= 0.5, record `{from, to, phase, gap}`. Merge consecutive windows with the same phase.
8. Bars: every B-th grid beat from the phase, kept when the whole bar fits in the track (1 ms tolerance).
9. Bar features: MFCC 13 (from the dB power spectrogram); per bar: loudness (RMS dB of the samples), density (the
   fraction of onset-envelope frames above 1.5 x its median), mean chroma (unit length), mean MFCC 2..13 (unit length);
   feature vector = the 2 concatenated. rel = loudness - the 90th percentile of bar loudness. novelty = distance of the
   feature to the previous bar's (0 for bar 0). rise = loudness difference to the previous bar.
10. Drop candidates: lift_i = rel_i - median(rel of up to 4 bars before), 0 for bar 0. Eligible: rel > -3 and mean rel
    of bars i..i+3 > -4; when no bar is eligible, rel above its 75th percentile. Score = lift/3 + 2 x novelty for
    eligible bars. The 6 best are the candidates.
11. Cost matrices: D(i, j) = feature distance + |loudness_i - loudness_j| / 6. Jump cost J(i, j) = jump + 2.5 x
    D(next(i), j), with next(i) = min(i + 1, last bar): a jump must land on a bar that sounds like the bar that would
    follow naturally. J(i, i + 1) = 0 (the natural continuation). J(i, i) += 1.5 (repeating a bar).
12. Dynamic programming over film bars k = 0..K-1 (0-based):
    - cost(0, j) = |rel_j - arc_0|/3 + 0.4 j (prefer starting near the top). `--start-bar` keeps only that bar, without
      the 0.4 j term.
    - For k >= 1: step cost = J + 1.0 for every non-continuation when k is not a multiple of `--phrase` (jumps prefer
      phrase starts). cost(k, j) = min over i of cost(k - 1, i) + step(i, j), plus |rel_j - arc_k|/3. Keep the argmin
      for backtracking.
    - At k = drop film bar - 1 keep only the drop source bar (when one is set). At the last k keep only `--end-bar`
      (when set). These anchors are not applied when K = 1.
    - Backtrack from the cheapest final bar.
13. Segments: runs of consecutive source bars. src_from = the first bar's time, src_to = src_from + bars x B x T,
    at = film_offset + (film bar - 1) x film bar length (B x 60 / film_bpm).
14. Music block: `{file, src_bpm (3 decimals), target_bpm, stretch, gain_db: 0, segments}` with each segment `{label,
    src_from, src_to, at, fade_in: 0.008, fade_out: 0.06}`, the label naming source bars and film bars. ratio =
    film_bpm / bpm. `stretch` = `none` when |ratio - 1| < 0.002, and then `src_bpm` is set to film_bpm (played
    unstretched on the film grid); else `rubberband`.

### 5.3 Output

Result JSON fields: `summary, track, duration, bpm, beat_period, film_bpm, beats_per_bar, film_bars, drop_film_bar,
arc, stretch_ratio_to_film, t0, transient_shift_ms, transient_fit_ratio, grid_median_residual_ms, grid_inlier_frac,
downbeat_phase, phase_scores, phase_confidence, phase_low_confidence, phase_alternative {phase, first_downbeat},
local_phase_disagreements, drop_source_bar, bars [{i, t, rel_db, novelty, rise_db, density}], drop_candidates, map
(source bar per film bar), segments [{src_from, src_to, at, src_bars, film_bars}], music, total_cost`.

The summary line states: BPM with grid residual and inlier share; the downbeat formula `<first> + <bar s>*n s` with
the phase and its confidence (with LOW: the other grid and the `--phase` value that selects it; with a given phase:
the automatic choice); source bars, film bars and segment count; the drop film bar and its source bar (or "chosen by
the arc"); the stretch ratio; and the local disagreements (up to 4).

Text mode also prints: the transient shift and fit ratio, the phase scores and the drop candidates; a LOW warning with
advice; the bar table (`bar t rel_dB nov rise dens`, the top 3 drop candidates marked); the map `film:source ...`; the
ready-to-paste music block; the file written.

The downbeat is the weak point. The source measured its phase choice on 2 hand-labelled tracks, both seen while it
was built: one correct (confidence 0.93), one half a bar off (confidence 0.15, flagged LOW, a bass on beats 1 and 3
whose accents move by half a bar later in the track). On both, the top drop candidate matched a section start that the
music supervisor marked by hand. This is not independent validation. Under LOW confidence the music supervisor decides
between the 2 printed grids from the first note, the section entrances and the chord changes, and runs again with
`--phase N`. The draft is a first draft: every splice is checked (QA edges, hark timeline), and `at` is nudged by a few
ms so the measured downbeat transient lands on the picture's downbeat.

## 6. `variant.py`

### 6.1 Interface

```
variant.py --ops OPS --out DIR [--render] [--qa] [--cues PATH] [--cache DIR] [--root .] [--design audio/sounddesign.json]
variant.py --ops OPS --apply [--backup out/audio-review/backup_before_fix] [--root .] [--design audio/sounddesign.json]
```

- `--ops`: a path to a JSON list of operations; when no such file exists, the argument itself is parsed as JSON
  (`[]` gives an unchanged copy).
- Exactly one of `--out` (a variant) and `--apply` (edit the real design), else exit with a message.
- A missing design exits with a message.

### 6.2 Operations (applied in order)

`f` = an include's file name without `.json` (fragments are keyed by base name). `sd` = a dotted path in the top-level
design.

| operation | effect |
|---|---|
| `{"f", "sample": N, "set": {...}}` (also `"slice": N`) | set keys of that sample or slice. |
| `{"f", "sample": N, "eq": {...}}` | merge keys into its `eq` (creating it). May be combined with `set`. |
| `{"f", "rule": N, "set": {...}}` | set keys of the first rule whose `name` equals N (compared as text). |
| `{"f", "rule": N, "delete": true}` | remove that rule. |
| `{"f", "rule_add": {...}}` | append a rule. |
| `{"f", "place": S, "set": {...}}` / `"delete"` | the first free placement whose `sample` equals S. |
| `{"f", "place_add": {...}}` | append a free placement. |
| `{"f", "amb": N, "set": {...}}` / `"delete"` | the first ambience whose `name` equals N. |
| `{"f", "segment": i, "set": {...}}` | set keys of music segment i. |
| `{"f", "pool": N, "value": [...]}` | replace (or create) the pool. |
| `{"sd": "a.b.c", "value": V}` | set a top-level path, creating missing objects (`buses.music.auto`, `master.lufs`). |

Any unknown fragment, missing item, bad index or unknown operation stops the run: `nothing written: <reason>`. Every
applied key prints `<where>: <old> -> <new>` (JSON, each side cut to 80 characters), or `<where>: unchanged (<old>)`
when the value is already as asked. The first line counts changes and already-as-asked operations and names the
destination.

### 6.3 Writing

Format-preserving: a file is rewritten so that only the changed values change in the text. Hand alignment, key
order, comments and the spelling of untouched numbers stay. Rules:

1. Parse the original text into a map from each value's path to its text span (item start, value start, value end)
   and the ordered child keys of each container.
2. Compare old and new values recursively:
   - Objects: recurse into keys kept; remove each run of deleted keys together with one separator; append new keys
     after the last item, using the separator text found between the container's last 2 items (comma, line break and
     indent), or `, ` when there are fewer than 2.
   - Lists: align old and new items by identity (an object's `name`, `sample` and `label` values when present, else
     its whole JSON) with a longest-matching-blocks diff. Equal items, and one-for-one replacements, are patched inside.
     Inserted items go after the preceding item with the separator. Deleted or unevenly replaced runs are replaced as
     one stretch.
   - Scalars and type changes: replace the value text with compact JSON (non-ASCII kept).
3. Apply the edits from the end. If 2 edits overlap, or the result does not parse to exactly the new value, fall back
   to a full rewrite with indent 1 and a trailing newline.
4. Report each written file as `patched` (only the changed values rewritten) or `reformatted`.

`--apply`: the backup folder must not exist (else exit and ask for a new `--backup`). Copy the top-level design to
`<backup>/<name>` and each fragment to `<backup>/design/<name>`. Write only the files whose content changed. A `diff -r`
against the backup is then the reviewable record of the fix.

`--out DIR`: write every fragment to `DIR/design/<name>` and the top level to `DIR/sounddesign.json`. In the copy,
`include` points to the copied fragments (absolute paths) and `cues` is set to the absolute path of `--cues`, or of the
design's `cues` when it has one. Sample `file` paths stay relative to `--root`.

`--render` or `--qa` with `--out`: run `mix.py --root <root> --design DIR/sounddesign.json --out DIR/soundtrack.wav
--stems DIR/stems --report DIR/report.json --cues-out DIR/cues.json --trace DIR/adds.json [--cache]` (the master is
written, so `master.json` lands in DIR too). Print the last 1200 characters of its output and 800 of its errors. Stop
with its exit code when it fails. `--qa` then runs `qa.py --root <root> --design DIR/sounddesign.json --report
DIR/report.json --trace DIR/adds.json --stems DIR/stems --master DIR/soundtrack.wav --out DIR/qa.json [--cache]` and
prints its digest. `DIR/qa.json` then sits next to the base render's `out/qa/qa.json` for a before/after comparison.

## 7. `mux.py`

### 7.1 Interface

```
mux.py [--video out/film.mp4] [--audio out/audio/soundtrack.wav] [--out <video>] [--abr 320k]
       [--keep-old out/superseded/<name>_prev<ext>] [--root .]
```

Exit 0 when the file is written and verified, 1 with `mux: <reason>` on stderr otherwise. `-h`/`--help` prints the
usage.

### 7.2 Behaviour

1. Fail when the audio or the video file is missing.
2. Probe the video: the video stream, frame rate (from the rational rate), duration (the stream's, else the
   container's). Fail when there is no video stream. Probe the audio's duration the same way.
3. One frame = 1/fps (1/60 when unknown). When |audio - video| > 1 frame, print a note: the audio is longer (it is cut
   at the picture's end) or SHORTER (the end will be silent), with seconds and frames.
4. When `--out` is the video itself: move the video to the keep-old path first (refuse when that file exists), and use
   it as the input. Print where the old version is.
5. Write to `<out stem>.muxing<ext>`: map the first video stream of the video and the first audio stream of the audio;
   copy the video; encode AAC at `--abr`, 48 kHz, 2 channels; duration limited to the video duration (6 decimals);
   with `+faststart` for mp4, m4v and mov. On failure delete the temporary file and fail.
6. Probe the result: it must have a video and an audio stream. Rename it to `--out`.
7. Print the file, its size in MB, the copied video codec, fps and duration, and the audio codec, bit rate and duration.

When the mux fails after step 4, the original stays at the keep-old path. hark should say so in the failure message.

After the mux, the skill verifies the audio track of the MP4 against the WAV by cross-correlation (lag must be 0
samples) and listens to the finished file with hark timeline and the sounded cues. Never re-time the audio to fix a
picture-side offset.

## 8. Acceptance tests

| # | input | expected properties |
|---|---|---|
| S1 | `classify` on `CC BY 4.0`, `Attribution 4.0`, `Creative Commons 0`, `CC BY-NC 4.0`, `CC BY-SA 3.0`, `Mixkit Sound Effects Free License`, `Mixkit Stock Music Free License`, `free with credit on social media`, ``, `CC BY 1.0`, `CC BY 4.0, all rights reserved` | keys/status: by4 allowed attribution; by4 allowed; cc0 allowed no attribution; forbidden; forbidden; mixkit-sfx allowed; mixkit-music allowed; forbidden; none unknown; by1 unknown; unknown (contradiction). |
| S2 | rights audit: 1 manifest entry CC BY 4.0, no audit file, a valid WAV | exit 1; the record is a blocker with a problem about no page reading; `rights.json` and `rights.md` written. |
| S3 | S2 plus an audit record with `rights_seen` text and `rights_confirmed: true`, attribution, author, page_url | exit 0; severity ok; a second run keeps the agent fields and writes `rights.prev.json`. |
| S4 | rights audit with `--report` whose `files_used` has a file in no manifest | a blocker row with id null; exit 1. Entries not in `files_used` are out of scope and never block. |
| S5 | the same file in 2 manifests, the second under `CC BY-NC` | the first record gets a problem about one recording having one licence; the duplicate row is `info` with `duplicate_of`. |
| S6 | Freesound entry `fs-123456` with page `.../sounds/123457/` and preview `previews/123/123456_9-lq.ogg` | warnings for the id mismatch, the page/preview mismatch and the lq preview; the folder 123 is correct for 123456. |
| S7 | credits: 1 CC BY effect with an author-requested attribution lacking links, 1 CC0 effect, 1 CC BY music, a design with slices and EQ | `CREDITS.md` has Music, Sound effects and ambience (by4 group before cc0), Tools; the CC BY line ends with the page link and the licence link; the description lists the CC0 author under the other authors; the processing sentence includes "cut into single hits" and "equalised"; exit 0. |
| S8 | credits with `--rights` where 1 used file is unconfirmed | `## PROBLEMS` section present; exit 1. |
| S9 | analyze on a 1 s 48 kHz mono test file with a burst at 0.25 s | `channels` 1, `side_to_mid_db` null, `attack_at` 0.25 +-0.002, peak equals the burst peak (no 3 dB loss from mono). |
| S10 | analyze on a 10 ms file | result `error: "too short"`; exit 1. |
| S11 | arrange on a generated click track at exactly 120 BPM with an accent every 4 beats and a louder second half, `--bars 8` | bpm 120 +-0.05; downbeat phase = the accent; stretch `none` for film 120; segments start on bar times; the drop bar is in the louder half. |
| S12 | arrange `--drop 99` on a track with 20 bars | exit with a message naming the range 0..19. |
| S13 | variant `--out` with ops `[{"f": "ui", "rule": "ui.click", "set": {"gain_db": -3}}]` on a hand-aligned fragment | only the `gain_db` text of that rule differs (`diff` shows 1 changed line); the copy's include paths are absolute; the real design is untouched. |
| S14 | variant with an op naming a missing rule | `nothing written: ...`; no file in the variant folder changes. |
| S15 | variant `--apply` twice with the same `--backup` | the second run refuses and asks for a new backup path. |
| S16 | mux a 2.0 s 30 fps test video with a 2.5 s WAV | a note that the audio is longer and is cut; output has 1 video stream (copied, same codec) and 1 AAC 48 kHz stereo stream; audio duration within 1 AAC frame of 2.0 s; exit 0. |
| S17 | mux with `--out` equal to `--video` and an existing keep-old file | exit 1; the original video is untouched. |
