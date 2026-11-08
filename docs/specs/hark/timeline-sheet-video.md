# hark timeline: sheet and video part

Part of the timeline unit spec. Index: `docs/specs/hark/timeline.md`. This part covers the video helpers (picture
cuts, frames, stream info) and the annotated sheet image (`hark sheet`).

## 1. Video helpers

### 1.1 Behaviour

The video helpers use ffmpeg on the CPU. They give:

- stream info of a media file;
- picture cuts and big visual changes ("visual events"), as times in seconds;
- single frames as RGB images for contact strips and the sheet.

Cut detection by frame difference is the default. It also catches cuts and whip transitions on motion graphics
with white backgrounds, where the ffmpeg scene score finds almost nothing. Slow crossfades are missed or found
late. The list marks large picture changes; it is not a list of the editor's cuts.

### 1.2 Algorithms

**Info** `info(path)`: from the core-io probe: `{duration, fps, width, height, has_audio, has_video}`, where
`has_audio` is true when the probe reports an audio codec. `has_video(path)` returns the probe's `has_video` flag.

**Frame difference signal** `frame_diff(path, cache=True) -> (fps, d)`:

1. `fps` = the probed video frame rate, or 30 when unknown.
2. Decode the picture (no audio) with ffmpeg, scaled with area averaging to 96 x 54 pixels, grey scale, raw
   8-bit frames at the native frame rate, to a pipe.
3. When ffmpeg fails or gives no bytes, raise the core decode error with a message that names the path and the
   first 200 characters of ffmpeg's error output.
4. Cut the byte stream into whole frames of 96*54 bytes (drop a partial frame at the end).
5. `d[0] = 0`; `d[i]` = the mean absolute difference between frame `i` and frame `i-1`, divided by 255 (a value
   from 0 to 1).
6. Cache `(fps, d)` in `<hark cache root>/video/` with a key that hashes the core file id, the frame size and a
   version number. A cache entry that fails to load is recomputed. Writes are atomic. `cache=False` recomputes.

**Visual events** `diff_events(d, fps)` (frame indices). Defaults: ratio 4.0, floor 0.012, big 0.15, gap 0.12 s.

1. When `d` has fewer than 3 values, return none.
2. `med` = running median of `d` over `round(fps)` frames, forced to an odd size (set the lowest bit), edge mode
   "repeat nearest". This is the median of about the surrounding second.
3. `loc` = running maximum of `d` over 3 frames, edge mode "repeat nearest".
4. Candidates: frames where `d == loc` and either (`d > 0.012` and `d > 4*med + 0.004`) or `d >= 0.15`.
5. Skip frame 0.
6. Walk candidates in order. When a candidate is less than `0.12*fps` frames after the last kept one, it replaces
   the kept one only if its `d` is larger; else drop it.

So a peak at least 4 times the local median (and above a 1.2 % grey change) is an event, and so is any change of
at least 15 %. Steady motion (a high but flat difference) is not.

**Cuts** `cuts(path, method='diff', threshold=0.3, min_gap=0.2, cache=True)`:

- `diff` (default): visual events of the frame difference signal, as `round(index/fps, 3)` seconds.
- `scene`: ffmpeg's scene score. Scale the picture to 320 pixels wide (even height), select frames whose scene
  score is above `threshold`, and read their presentation times from ffmpeg's frame info output. Keep a time only
  when it is at least `min_gap` after the last kept time (3 decimals). Cache the list as JSON in
  `<hark cache root>/video/` under a key that hashes the file id, the threshold, the gap and a version number.
  When ffmpeg fails, raise the core decode error with a short reason; do not cache the failure. A cache entry
  written under an older version key is ignored. A successful scan with no cuts is cached as an empty list.
- `cache=False` recomputes in both methods.

**Frame** `frame(path, t, width=320)`: seek to `max(0, t)` (3 decimals), decode 1 frame, scale it to `width`
(even height), encode it as PNG to a pipe, open it as an RGB image. Return `null` when ffmpeg fails or gives no
output. `frames(path, times, width)` maps `frame` over the times.

### 1.3 Constants

| name | value |
|---|---|
| analysis frame | 96 x 54 grey, area scaling |
| default fps | 30 |
| ratio over local median | 4.0 (plus 0.004) |
| floor | 0.012 (1.2 % mean grey change) |
| big change | 0.15 (15 %) |
| minimum gap | 0.12 s |
| median window | about 1 s (round(fps), odd) |
| scene threshold, gap | 0.3, 0.2 s |
| scene scale width | 320 px |
| default frame width | 320 px |

### 1.4 Errors and edge cases

| case | behaviour |
|---|---|
| picture that ffmpeg cannot decode (`diff`) | core decode error naming the path and ffmpeg's reason |
| scene scan of a broken file | core decode error naming the file; nothing cached |
| still picture | no cuts (scene: an empty list, cached) |
| tampered cache entry | used as is; `cache=False` bypasses it |
| blinking UI element (a terminal cursor) | can create visual events: the detector sees any big change |
| slow crossfade | missed or late |

### 1.5 Acceptance tests

| id | input | expected |
|---|---|---|
| V1 | `d` of 300 values at 0.003; frames 100-139 set to `0.02 + 0.005*sin(k)`; frame 60 = 0.4; frame 200 = 0.05; fps 30 | events exactly [60, 200] |
| V2 | mp4 of 1.5 s black then 1.5 s white, 96 x 54, 30 fps | `cuts` = [1.5]; after the cached difference signal is overwritten with zeros, `cuts` = [] with the cache and [1.5] with `cache=False` |
| V3 | a file of random bytes named `broken.mp4`, method `scene` | decode error that contains `broken.mp4`; no scene cache file exists afterwards |
| V4 | a 1 s black mp4, method `scene` | `[]`; exactly 1 scene cache file exists |
| V5 | a broken file with a cache entry `[]` stored under the older version key | decode error; the old entry is untouched and is the only cache file |
| V6 | 3 colour clips of 2 s each (red, blue, white) at 25 fps | visual events at 2.0 and 4.0 s |

## 2. The sheet

### 2.1 Behaviour

`hark sheet FILE -o OUT.png` draws one annotated PNG of a soundtrack or film for agents that can look at images.
Every lane shares one time axis (m:ss). From top to bottom:

1. a header with the key numbers;
2. **frames** (only with a picture): one video frame per slot, taken just after a cut when the slot has one, with a
   thin tick at each frame's exact time;
3. **sections** with their labels;
4. **events**: labels for tagged events (kind plus the best CLAP tag), staggered so they never overlap, pale with
   `?` when the event is less than 12 dB over its bed; untagged onsets are short ticks in the spectrogram;
5. **spectrogram**: log frequency, 30 Hz to 16 kHz; digital silence draws black;
6. **waveform**;
7. **loudness**: momentary and short-term LUFS, the integrated level, gaps shaded yellow, clipping ticks;
8. **sync / beats**: beat ticks (tall = downbeat guess), cue and cut markers coloured by their sync status.

Picture cuts are dashed vertical lines through every lane except the event labels. The sheet is built from a
timeline result (the cached one is reused, or a result passed in). `--from/--to` zooms. An audio file's cover art
is not a video. A picture-only video draws its frames and cuts over silence.

### 2.2 Layout algorithm

All sizes: inches at 100 dpi unless the unit is px.

1. Get the timeline result `r` (passed in, or run the timeline with the same file, video, cues, zoom and detail).
   `t0, t1 = r.start, r.end`. The picture is `r.video`.
2. Audio for the drawing: the mono decode of FILE from `t0` to `t1` (no context); zeros of the same length when
   the file has no audio.
3. Width: clamp the requested width to [1000, 2400] px. Base font `fs = max(11, width/125)` pt. Left margin
   `max(92, int(6.4*fs) + 8)` px, right margin 64 px, plot width = width minus both margins. Event label font
   `lf = 0.85*fs` pt (monospace). Character width `char_px = 0.602*lf*100/72` px.
4. Lanes and heights: frames 1.55 (only with a picture), sections 0.6, labels 1.55, spectrogram 3.6, waveform
   0.95, loudness 1.75, sync / beats 0.75. Each lane's axes height is its lane height minus 0.06.
5. Header lines (text, size, weight, colour):
   1. file name, three spaces, `fmt_t(t0)-fmt_t(t1)`; `1.25*fs`, bold, black;
   2. a sub-line of items joined by four spaces; `0.95*fs`, normal, dark grey: `I LUFS   TP x dBTP   LRA y LU`
      (LRA when present) when integrated exists, else `digital silence` or `no audio stream`; `~T (or A) BPM
      (confidence)` when a tempo exists and confidence is not `none`; `N events`;
   3. for cuts, then cues, when they have items: one sync line with the count, the `OK` share within 1 frame or
      40 ms with its chance, the accent share with its chance (when present), the count met only by a level peak
      (when nonzero), and the count without a hit; `0.95*fs`;
   4. when there is any sync: a marker legend (cue triangles, cut diamonds, and the 5 status colours below), plus a
      note that dashed lines are picture cuts when there are cuts; `0.72*fs`, mid grey;
   5. when any event is labelled: a note that labels are kind plus CLAP tag, hints only, and that pale with `?`
      means under 12 dB over the bed; `0.72*fs`.
6. Header height `0.22 + sum(size*1.45/72)` over the lines. Figure height `round((header + sum(lanes) + 0.55)*100)
   / 100`. Lanes stack downward from below the header. Each line's vertical centre advances by `size*0.725/72`
   before and after it, from 0.12 in.
7. X axis: the same limits `[t0, t1]` on every lane. Tick step: the first of 0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30,
   60, 120, 300 s that gives at most 24 ticks over the span, else 600 s. First tick at the next multiple of the step.
   Tick labels only on the bottom lane: `m:ss` for steps of 1 s or more; `m:ss.s` for 0.1 and 0.5 s steps;
   `m:ss.ss` for the 0.25 s step. Axis title `time (m:ss)`. Thin grey spines.
8. Lane titles: left of the lane (frames, sections, events, sync / beats), or inside the top left on a pale box
   (spectrogram, waveform, loudness).

**Frames lane**: thumbnail height `1.55*100 - 18` px; thumbnail width = height times the video aspect
(`width/height` from video info, 16:9 by default). Slots: `max(1, plot_width // (thumb_width + 6))`. Time per slot
`k` over `[a, b]`: the first cut with `a <= cut < b - 0.2`, plus 0.15 s, but at most `b - 0.05`; else the slot
centre. Fetch each frame at 1.5 times the thumbnail width. Draw it centred in its slot, from 12 % to 100 % of the
lane height. Draw a short dark tick from 0 to 12 % at the frame's exact time. A frame that fails to decode leaves
its slot empty. Count drawn frames as `thumbnails`.

**Sections lane**: for each section (clipped to the window), a rectangle with alternating pale blue and pale sand
fills and a grey edge. Text capacity in characters: `int(section px / (0.55*fs*0.9*100/72)) - 1`. Draw the label
(cut to the capacity minus 2, plus `..`) when the capacity is at least 4, left-aligned 4 px inside the box, font
`0.9*fs`, clipped to the lane.

**Events lane** (4 rows):

1. Items: every labelled event, with priority (0 when the event answers a cut or cue, else 1; then minus its
   prominence) and text `<KIND>[?] <label>` (`?` when not exposed).
2. Greedy staggering: pixels per second = plot width / span. Sort items by priority. An item's width in seconds is
   `(len(text)*char_px + 16) / px_per_s`. Its span is `[t - 0.15*w, t + 0.85*w]`; when that passes `t1`, shift it
   to `[t1 - w, t1]`; clamp its start at `t0`. Put it in the first row (top first) where it overlaps no item
   already in that row. An item that fits no row is dropped. Output `(row, t, span start, text)`.
3. Draw each placed label: white monospace text on a square box in the kind colour, alpha 0.55 for `?` labels
   else 1.0, vertically centred in its row; a thin stem in the same colour from the lane bottom up to the row.
4. Kind chips: for each kind among placed labels, a rounded white-on-colour chip at the top right of the figure,
   right to left.
5. `labelled_events` = placed labels; `tagged_events` = items.

**Spectrogram lane**:

1. Log-frequency spectrogram with about `plot_width` columns: `n_fft = 4096`; hop `max(64, int(len/plot_width))`;
   zero padding of `n_fft/2` on both sides; Hann window; power spectrum.
2. 240 rows: triangular filters with `242` edges geometrically spaced from 30 Hz to 16 kHz (filter `k` spans edges
   `k` to `k+2`, peak at edge `k+1`). A filter that covers no FFT bin gets weight 1 on the bin nearest its centre.
   Normalise each filter to sum 1. Value `10*log10(filter power + 1e-14)`.
3. Colour map "magma". Top of the scale `max(99.7th percentile, -40)` dB (so digital silence draws black); bottom =
   top minus 75 dB. Bilinear interpolation.
4. Y ticks at 50, 100, 200, 500, 1k, 2k, 5k, 10k Hz, placed by interpolating `log f` on the row centres.
5. Event ticks at the top edge in white: 7 % of the height for labelled events (line width 1, alpha 0.9), 3.5 % for
   the others (width 0.6, alpha 0.55).

**Waveform lane**: split the samples into `min(plot_width, n)` equal column ranges; draw the filled band between
each column's minimum and maximum, scaled by the 99.9th percentile of `|m|` (at least 1e-9) and clipped to plus or
minus 1, in slate grey. Title: the full-height level in dBFS (0 decimals) and the peak dBFS (1 decimal), or
`digital silence` when the peak is 0.

**Loudness lane**: y range -60 to 0 LUFS, ticks -50..-10. Momentary curve (thin, mid blue) and short-term curve
(thick, dark blue), both floored at -60, times `t0_curve + dt*k`. Integrated level as a dashed red line with its
value printed just right of the plot, when it exists and is above -60. Each gap shaded yellow (alpha 0.45). Each
clipping time as a short red tick from -3 to 0. Legend lower left.

**Sync / beats lane**: when beats exist and confidence is not `none`, a tick per beat inside the window: height
0.28 (grey) or 0.45 (near black) for downbeats. Cue markers: downward triangles at height 0.84. Cut markers:
diamonds at height 0.6. Marker colour by status (table 2.3).

**Picture cuts**: a dashed cyan line at each cut inside the window, in every lane except events, drawn behind the
frames and in front elsewhere.

**Output**: create the output folder, save the PNG at 100 dpi on white. Return:

| field | value |
|---|---|
| `png` | absolute path |
| `width`, `height` | pixel size (`height = round(figure height*100)`) |
| `start`, `end` | `t0`, `t1` |
| `thumbnails` | frames drawn |
| `labelled_events`, `tagged_events` | placed labels, labelled events |
| `events` | all events |
| `cuts` | picture cuts |
| `timeline_summary` | the timeline summary |
| `summary` | ASCII sentence: wrote the path and size, the window, frame count, labels placed of tagged, event ticks, cut lines, and how to open it (an image viewer or a file read tool) |
| `text` | the summary, a blank line, then `Timeline: ` and the timeline summary |

### 2.3 Constants

Kind colours:

| kind | colour | kind | colour |
|---|---|---|---|
| WHOOSH | #1f77b4 | RISER | #17becf |
| HIT | #d62728 | KEY | #2ca02c |
| UI | #8c564b | STEP | #bcbd22 |
| FOLEY | #e377c2 | NOTE | #9467bd |
| DRUM | #ff7f0e | VOICE | #0b6e4f |
| `-` and unknown | #7f7f7f / #777777 | | |

Sync status colours: `OK` green #2ca02c; `near` amber #ff9f1c; `swell` blue #1f77b4; `peak` violet #9467bd;
`off` and `no sound` red #d62728; unknown grey #777777. Picture cut lines: #00a6d6, dash pattern 4 on 3 off.

Other: dpi 100; width 1000-2400 px, default 2000; fonts: a sans-serif and a monospace face that exist on every
platform (the source used the DejaVu pair); spectrogram 240 rows, 30-16 000 Hz, 75 dB range, floor -40 dB at the
top; waveform scale p99.9; loudness lane floor -60 LUFS.

### 2.4 Errors and edge cases

| case | behaviour |
|---|---|
| width outside 1000-2400 | clamped |
| no `-o` | `<file stem>_sheet.png` in the current folder |
| digital silence | black spectrogram; waveform title says digital silence; header says `digital silence` |
| no audio stream | header says `no audio stream`; drawn over zeros |
| frame decode failure | that slot stays empty |
| more labels than fit | lower-priority labels are dropped; never overlapping |
| very long section label | truncated with `..`; hidden under 4 characters of room |
| a section that starts before the zoom | clipped to the window |

### 2.5 Acceptance tests

| id | input | expected |
|---|---|---|
| S1 | 8 s click track with clicks every 0.5 s from 0.5 s, cues `1.0,2.0`, width 1400, no models | the PNG exists and its pixel size equals the returned width and height; width 1400; more than 3 labels placed; summary contains `Wrote` |
| S2 | the same file, zoom 2-4 s | `start` 2.0; the PNG exists |
| S3 | 30 items at t = 1.00 + 0.01k, texts `HIT label number k`, window 0-10 s, plot 1800 px, char 10 px, 4 rows | between 1 and 30 placed; in every row, no two placed spans (start, start + (len*10 + 16)/180) overlap |
| S4 | the 3 s black/white mp4 with clicks (analysis T14), width 1200 | at least 3 thumbnails; `cuts` = 1 |
| S5 | CLI parse `sheet x.mp4 -o o.png --width 1600` | output `o.png`, width 1600 |
| S6 | a 10 s digital-silence file | header and waveform say digital silence; the spectrogram is black (all values at the bottom of the scale) |
| S7 | a 93 s film at width 2400 | every lane title is shown in full; header lines fit the width |
