# hark timeline: analysis part (events, loudness, beat grid, cues, sync, tags, sections, transcript)

Part of the timeline unit spec. Index: `docs/specs/hark/timeline.md`. The exact interface (CLI flags, Python
entry points, JSON fields) is in the index. This part gives the behaviour, the algorithms, the constants, the
errors and the acceptance tests of `hark timeline`.

All signals are at the core sample rate of 48 000 Hz (core-io unit). "Mono" means the core-io mono fold of a
(possibly stereo) float array. All times are in seconds unless a field name ends in `_ms`. "dB" of an amplitude
`a` means `20*log10(a)`; "dB" of a power `p` means `10*log10(p)`.

## 1. Behaviour

`hark timeline FILE` listens through a whole soundtrack or film the way a sound supervisor does and returns:

1. a list of **events**: transient onsets ("hits") and slow build-ups ("swells"), each with its time at the
   attack, its level, how far it rises over the sound just before it, its length and its spectral centroid;
2. optional **tags** on the most important events: a guessed kind (WHOOSH, RISER, HIT, KEY, UI, STEP, FOLEY,
   NOTE, DRUM, VOICE) from the CLAP audio-text model, plus AST labels on a few events;
3. **sections**: contiguous stretches that sound alike, each with genre / mood / instrument / ambience / voice
   tags and an integrated level;
4. a **loudness** picture: momentary and short-term LUFS curves, integrated LUFS, LRA, true peak, gaps
   (silences, drops) and clipping;
5. a **beat grid**: tempo, beat times, a downbeat guess, pulse clarity and a confidence word;
6. **sync**: for each picture cut (from a video) and each cue (from a cue list), the audio event that answers it,
   the signed offset in ms, a status word, and statistics that include what random times would score ("chance");
7. a plain-ASCII **transcript** (`text`) and a one-paragraph **summary**.

Inputs:

- FILE is an audio file or a video. When FILE has a real picture stream, its own picture cuts are used.
- `--video V` gives the picture for a separate soundtrack.
- `--cues C` gives cue times (several formats, section 5.1).
- `--from A --to B` zooms into `[A, B]`. The zoom is analysed inside 30 s of context on each side, so the sync
  verdicts, the accent threshold and the beat grid near the zoom edges equal those of the full analysis.
- `--detail low|normal|high` sets the model budget (constants table 3.1).
- `--fresh` recomputes everything, ignores every cache and stores the new result.

The tags are hints. The measurements (time, level, prominence, length, centroid, loudness, offsets) are facts.
The text output must say so.

Model work is optional at the level of the whole tool (core-io installs models on demand). When the models are
absent and the user does not install them, the timeline cannot tag events or build sections. Open question Q3 in
the index covers the exact fallback.

## 2. Algorithms

### 2.1 Shared building blocks

**Log-mel spectrogram** `logmel(m, n_fft, hop, n_mels, fmin, fmax)`:

1. Pad the mono signal by `n_fft/2` samples on both sides. Use reflection padding when the signal is longer than
   `n_fft`, else zero padding.
2. Cut frames of `n_fft` samples every `hop` samples. Frame `i` is centred at time `i*hop/48000`.
3. Multiply each frame by a Hann window of length `n_fft`. Take the power spectrum `|rfft|^2`.
4. Apply a triangular mel filter bank on the HTK mel scale (`mel(f) = 2595*log10(1 + f/700)`): `n_mels + 2` edge
   frequencies equally spaced in mel between `fmin` and `fmax`; filter `k` rises linearly from edge `k` to edge
   `k+1` and falls to edge `k+2`; no area normalisation.
5. Output `10*log10(filterbank_power + 1e-12)` in dB, shape `(n_mels, n_frames)`.

Process frames in blocks (for example 4096 frames) to bound memory. Results must not depend on the block size.

**Robust z-score** `robust_z(v, size)`:

- `med` = running median of `v` over `size` samples (edge mode: repeat the nearest value).
- `mad` = running median of `|v - med|` over `size` samples, times 1.4826.
- `floor` = 0.02 times the 99th percentile of the strictly positive values of `v` (1e-6 when there are none).
- `z = (v - med) / (mad + floor + 1e-9)`.

**Run finder** `runs(mask, min_len)`: the `(start, end)` index pairs (end exclusive) of every run of true values
with length `>= min_len`.

**Time format** `m:ss.ss` (minutes, then seconds with 2 decimals and a leading zero) comes from core-io. Write it
as `fmt_t` below. `None` formats as `-`.

**ASCII rule**: every text line (summary, transcript, sheet summary) is made ASCII-safe. A non-ASCII character
becomes its Python-style backslash escape (`\xf6`, `—`), so a name stays recoverable. JSON output keeps the
exact strings.

### 2.2 Event detection

Input: an array `x` (mono or stereo), and a flag `lead` that is true when `x` starts at the start of the file.
Output: a list of event objects sorted by time, with times relative to `x`.

If the mono signal is shorter than 0.1 s, return no events.

**Step 1: onset strength (5 ms frames).**

1. `L = logmel(mono, n_fft=1024, hop=240, n_mels=96, fmin=30, fmax=16000)`.
2. Clamp `L` from below at `max(L) - 80` dB.
3. For each frame `t >= 2`: take the max of `L[:, t-2]` over each mel band and its two neighbours (a 3-band
   maximum filter along frequency). The flux of band `b` is `L[b, t]` minus that maximum. This is a
   "superflux" difference with a 2-frame (10 ms) lag that ignores small vibrato.
4. The onset strength `odf[t]` is the mean over bands of the positive part of the flux. `odf[0] = odf[1] = 0`.

**Step 2: candidate frames.**

1. `z = robust_z(odf, 401)` (401 frames, about 2 s).
2. A frame is a candidate when all hold: `odf[t]` equals the maximum of `odf` over the 11 frames centred at `t`
   (plus or minus 25 ms); `z[t] >= zthr` (default 3.0); `odf[t] > 0`.

**Step 3: band envelopes (1 ms).** Make three envelopes at 1 ms resolution (48 samples per value). Each value is
the maximum absolute sample in that millisecond, plus 1e-7:

- `full`: for stereo input, the maximum over both channels; for mono, the mono signal.
- `low`: mono through a 4th-order Butterworth low-pass at 200 Hz.
- `high`: mono through a 4th-order Butterworth high-pass at 2000 Hz.

Run the two filters on the signal with a mirrored lead-in prepended: the first `p = min(len - 1, 4800)` samples
after the first sample, in reverse order (samples `p, p-1, ..., 1`). Drop the filtered lead-in afterwards. The
filters then start without a step transient. Use second-order sections and a causal (one-pass) filter.

**Step 4: refine each candidate to its attack.** For a candidate frame at time `tc = t*240/48000`, let
`ic = round(tc*1000)` (ms index). For each band envelope `e`:

1. Search window `[ic-15, ic+50)` ms, clamped to the envelope. Skip the band when the window has fewer than 5
   values.
2. `ip` = index of the maximum in the window (the event peak).
3. Bed: the median of `e[ic-160 : ic-40]` (clamped at 0). When that slice has fewer than 20 values, use the
   median of `e[0 : ic+250]`. Floor the bed at `1e-4` (-80 dBFS).
4. Threshold `thr = bed + 0.1*(e[ip] - bed)` (the attack is the first millisecond whose excess over the bed is
   within 20 dB of the peak excess).
5. Walk back from `ip`: while `j > max(0, ic-40)` and `e[j-1] >= thr`, decrease `j`. The attack index is `j`.
6. Record `(ratio = e[ip]/bed, band, j, ip, bed)`.

The "best" band has the largest ratio. The timing band is `full` when the `full` ratio is at least 4.0 (12 dB),
else the best band, because filtered bands smear time. Reject the candidate when no band qualified or when
`20*log10(best ratio) < min_contrast_db` (default 3 dB).

An accepted candidate gives a hit event:

| field | value |
|---|---|
| `t` | attack index `j` of the timing band, divided by 1000 |
| `shape` | `hit` |
| `band` | the best band (`full`, `low` or `high`) |
| `contrast_db` | `min(20*log10(best ratio), 60)` |
| `z` | `min(z[t], 99)` |
| `length_ms` | step 5, on the timing band |

**Step 5: length.** On envelope `e` with attack `j`, peak `ip` and bed `bed`: `floor = max(0.1*e[ip],
1.41*bed)` (20 dB under the peak, or 3 dB over the bed). Look at up to 2000 ms after the peak. Find the first
position `k` where 30 consecutive milliseconds are all below `floor`. When there is none, `k` is the length of the
looked-at slice. `length_ms = min(ip + k - j, 2000)`.

**Step 6: merge close hits.** Sort hits by `t`. Walk in order. When a hit is less than `min_gap` (default 30 ms)
after the last kept hit, it replaces the kept hit only if its `contrast_db` is larger; else drop it.

**Step 7: lead event.** When `lead` is true, the `full` envelope has more than 5 values, the maximum of its first
50 ms is at least 1e-3 (-60 dBFS), and no kept hit is earlier than 50 ms: let `ip` be the position of that
maximum and `j` the first millisecond (at or before `ip`) whose value is at least `0.1*e[ip]`. Insert a hit at the
front with `t = j/1000`, band `full`, `contrast_db` 60, `z` 0, a length from step 5 with bed `1e-4`, and the flag
`starts_file: true`. A file that starts with sound therefore always has an event in its first milliseconds.

**Step 8: prominence over the bed (K-weighted).**

1. K-weight the signal with the BS.1770 filter pair (core-io). Sum the squared channels.
2. Compute the mean power of 10 ms (480 sample) windows that start every 1 ms (48 samples), from sample 0 while
   the window start is before `len - 480` (at least 1 window). Convert to dB with `+1e-12`. Index `i` is the
   window that starts at `i` ms. This curve is not offset to LUFS; it is "about LUFS".
3. For an event at `j = round(t*1000)`:
   - bed: -80 for a `starts_file` event; else the median of the curve over `[j-300, j-20)` (clamped) when that
     slice has at least 20 values, else the 10th percentile of the curve over `[0, j+2000)`; floor at -80.
   - `prominence_db = max(0, max(curve[j : j+80]) - bed)`; 0 when `j` is past the curve.
   - `bed_db` = the bed, rounded to 0.1.

**Step 9: swells.** Append the build-up events of section 2.3 (computed with the hits of step 7 as the hit list).
Sort all events by `t`.

**Step 10: per-event levels.** For each event:

- `peak_dbfs`: dB of the maximum absolute sample over all channels in `[t, t + d)`, where
  `d = clamp(length_ms/1000, 0.05, 0.3)` s (at least 1 sample). Round to 0.1.
- `local_lufs`: the momentary loudness curve (0.4 s blocks, 0.05 s hop, block-centre times; core-io) linearly
  interpolated at `t`. When the signal is not longer than 0.4 s, use a single point (0 s, -70). Round to 0.1.
- `centroid_hz`: the excess centroid below.
- Round `t` to 3 decimals; `prominence_db`, `contrast_db` and `z` to 1 decimal.

**Excess centroid** (what the event adds over the bed):

1. `a = int(t*48000)`, `n = int(clamp(length_s, 0.03, 0.12)*48000)`. Segment `seg = mono[a : a+n]`. Bed
   segment `pre = mono[a - 0.15 s : a - 0.01 s]` (clamped at 0).
2. When `seg` has fewer than 256 samples, the centroid is `null`.
3. Spectrum: apply a Hann window of the segment's own length, then an FFT of length 4096 (zero-padded; a longer
   windowed segment is truncated to its first 4096 samples). Power `P = |X|^2 / len(seg)`.
4. When `pre` has at least 256 samples, compute its power `Q` the same way. `D = max(P - Q, 0)`. Use `D` in
   place of `P` when `sum(D) > 0.2*sum(P)`.
5. Centroid over bins from 20 Hz to 20 kHz: `sum(f*P)/(sum(P) + 1e-30)`, as an integer (truncate).

### 2.3 Build-ups (swells) and level peaks

**Level envelopes** `level_envs(x)` (10 ms frames):

1. Mono; `n = len // 480`. When `n < 3`, return an empty map.
2. Two signals: the mono signal (`full`) and its 2 kHz 4th-order Butterworth high-pass (`high`, with the mirrored
   lead-in of section 2.2 step 3). A whoosh under music shows best in the highs.
3. For each signal: RMS of each 10 ms frame, in dB with `+1e-9`, floored at -80, then a 9-frame (90 ms) moving
   average (edge mode: repeat the nearest value).
4. Frame `i` is at time `i*0.01`.

**Swell detection** (whooshes, risers, swells timed at their peak), given the hit list:

1. When the `full` envelope has fewer than 60 frames (0.6 s), return none.
2. For each band envelope `s`, for each frame `i` where `s[i]` equals the maximum of `s` over the 31 frames
   centred at `i` (plus or minus 150 ms) and `s[i] > -70`:
   1. `a = max(0, i-200)` (2 s back). Skip when `i - a < 25`.
   2. `j` = the position of the minimum of `s[a : i-15]` (the start of the rise).
   3. `rise = s[i] - s[j]`, `dur = (i - j)/100` s, `after = s[i : i+100]`.
   4. Keep only when `rise >= 8` dB, `dur >= 0.25` s, `after` has at least 10 frames, and
      `s[i] - min(after) >= 4` dB.
   5. Keep only when at least 60 % of the first differences of `s[j..i]` are positive (mostly monotonic).
   6. `t = i/100`. Skip when a hit lies within 50 ms of `t - 0.03` (the peak is just a hit's attack).
   7. Skip when a swell already found (in any band) lies within 0.15 s of `t`.
   8. `length_ms` = 10 times the first index in `after` where `after < s[i] - 10`; 1000 when there is none.
   9. Emit `{t, shape: swell, band, contrast_db: rise, prominence_db: rise, bed_db: round(s[j], 1), z: 0,
      rise_ms: int(dur*1000), length_ms}`.

**Level peak** `level_peak(envs, t)`: a peak near a cut or cue that the onset detector misses (a soft whoosh or
swell peaking on the cut). Defaults: radius 0.15 s, minimum rise 3 dB, minimum fall 2 dB, local window 0.3 s.
For each band envelope `s`:

1. `c = round(t*100)`. Search `[c-15, c+15]` frames (clamped). Skip when the range has fewer than 3 frames.
2. `i` = the position of the maximum in that range.
3. Skip when `s[i]` is below the maximum of `s` over `[i-30, i+30]`, or when `i < 5`.
4. `rise = s[i] - min(s[i-60 : i])` (0.6 s before). `after = s[i : i+31]`.
5. Skip when `rise < 3`, `after` has fewer than 5 frames, or `s[i] - min(after) < 2`.

Return the band with the largest rise as `{t: round(i/100, 3), band, rise_db: round(rise, 1)}`, or `null`.

### 2.4 Loudness picture

Input: the analysed window `x`, flags `at_start` (the window begins where the file begins) and `at_end` (it ends
where the file ends). Loudness curves, integrated loudness, LRA and true peak are core-io functions:

- loudness curve `(t_mid, LUFS)` of blocks of length `w` with hop `h` after K-weighting, `-0.691 +
  10*log10(sum over channels of mean square)`; for a signal shorter than one block, a single block over all of it;
- integrated loudness: BS.1770 gating (absolute -70, relative -10 LU), on 0.4 s blocks with 0.1 s hop;
- LRA: 3 s blocks with 0.1 s hop, absolute gate -70, relative gate -20 LU, 95th minus 10th percentile; `null`
  with fewer than 5 gated blocks;
- true peak: 4x oversampled peak in dBTP.

Output fields:

1. `true_peak_dbtp`: rounded to 0.1, or `null` for digital silence (peak sample exactly 0).
2. `digital_silence`: true when every sample is exactly 0.
3. When the duration is at least 0.4 s: momentary curve (0.4 s, hop 0.1), floored at -70. When its maximum is
   above -70: `integrated_lufs` (rounded 0.1) and `max_momentary {t, lufs}`. Always: `momentary {t0, dt: 0.1,
   lufs: [...]}` (t0 = first block centre, 3 decimals; values 1 decimal). `integrated_lufs` stays `null` when
   every block is gated out.
4. When the duration is at least 3 s: short-term curve (3 s, hop 0.5), floored at -70. When its maximum is above
   -70: `max_short_term {start: t_mid - 1.5, end: t_mid + 1.5, lufs}`. Always: `short_term {t0, dt: 0.5, lufs}`.
5. `lra_lu`: rounded to 0.1 when the duration is at least 6 s and core-io returns a value, else `null`.
6. `silences`: the gap list below.
7. `clipping`: a sample is clipped when its absolute value (maximum over channels) is at least 0.999. Count every
   clipped sample (`samples`), the runs of at least 3 consecutive clipped samples (`runs`), and list the start times
   of the first 20 runs (`times`, 3 decimals).

**Gaps** (stop-downs, drops, silences):

1. When the window is shorter than 0.5 s, there are none.
2. Fast curve: loudness of 0.1 s blocks with hop 0.05 s, floored at -70.
3. Threshold: -70 when integrated is `null` or at most -60; else `max(-60, integrated - 20)`.
4. Low mask: `F <= thr` when integrated is `null`, else `F < thr`.
5. Close 1-2 frame breaks: pad the mask by 3 frames on each side with its edge values, apply a binary closing with
   a 7-frame structuring element, remove the padding, then AND with `F < thr + 6`.
6. Each run of at least 5 frames (0.25 s) is a gap: `start = max(0, t[s] - 0.05)` (exactly 0 when the run starts
   at the first frame), `end = min(dur, t[e-1] + 0.05)` (exactly `dur` when the run reaches the last frame).
7. `kind`: `digital silence` when the peak absolute sample in `[start, end)` is below 1e-5; else `near silence`
   when the run's maximum `F` is below -60; else `drop`.
8. Fields: `start`, `end`, `dur` (2 decimals), `kind`, `level_lufs` (mean `F` over the run, 1 decimal).
9. `where`: `whole file` when the gap touches both edges, `lead-in` when it touches the start only, `tail` when it
   touches the end only. Touching the start means `start <= 0.05` and `at_start`; touching the end means
   `end >= dur - 0.05` and `at_end`. A gap that touches no file edge has no `where` and is a real stop-down.

### 2.5 Beat grid

Input: the mono context signal. Defaults: 60 to 200 BPM.

1. When the signal is shorter than 5 s, return `{tempo: null, beats: [], downbeats: [], confidence: none, note}`
   with a note that says the file is too short for a beat grid.
2. **Pulse envelopes** at 10 ms: `L = logmel(m, n_fft=2048, hop=480, n_mels=64, fmin=30, fmax=8000)`, clamped
   at `max(L) - 80`. Positive first difference along time (the first frame's difference is 0). Two envelopes:
   the mean over all 64 bands (broadband) and the mean over the lowest 6 bands (the low band, kick drum). For each:
   subtract a 101-frame (1 s) moving average, keep the positive part, divide by its standard deviation (+1e-9).
3. **Period per envelope**:
   1. Subtract the mean. Autocorrelation through an FFT of length `2N` (no circular wrap). Normalise by lag 0.
   2. Lags (in 10 ms frames) from `int(6000/max_bpm)` up to (excluding) `min(int(6000/min_bpm), N/2)`. When the
      range is empty, this envelope gives no candidate.
   3. Soft tempo prior centred at 120 BPM: `prior(lag) = exp(-0.5*(log2(6000/lag/120)/0.9)^2)`.
   4. Score `(ac[lag] + 0.5*ac[min(2*lag, N-1)]) * prior(lag)`.
   5. `P` = the best lag, refined by a parabola through the scores of its two neighbours (when both exist):
      `P += 0.5*(y0 - y2)/(y0 - 2*y1 + y2 + 1e-12)`.
   6. Pulse clarity = `ac[round(P)]`.
4. Take the envelope whose best score is larger. When neither gives a candidate, return the too-short result.
5. **Beat tracking by dynamic programming** (Ellis 2007 style) on that envelope `env`:
   1. Allowed previous-beat distances: integer lags from `max(1, int(P/2))` to `int(2P)` inclusive. Penalty
      `100*(ln(lag/P))^2`.
   2. `score = env` (a copy), `back = -1` everywhere.
   3. For each frame `t` from `int(P/2)` to the end: over the allowed lags with `t - lag >= 0`, take
      `c = score[t - lag] - penalty(lag)` and its best value. When the best value is positive, add it to
      `score[t]` and set `back[t] = t - best lag`.
   4. The last beat is the best-scoring frame among the last `int(P)` frames. Follow `back` links to the start.
   5. With fewer than 4 beats, return the empty result with a note that no steady pulse was found.
6. Regularity = standard deviation of the inter-beat intervals divided by their mean.
7. Confidence: `clear` when clarity `>= 0.30` and regularity `< 0.1`; else `weak` when clarity `>= 0.15`; else
   `none`.
8. **Downbeat phase** (4/4 assumed):
   1. STFT with `n_fft = 4096`, hop 480, Hann window, zero padding of `n_fft/2` on both sides; magnitudes.
   2. Pitch-class profile: bins from 55 Hz to 2000 Hz; class `round(12*log2(f/440)) mod 12`; per frame, the sum of
      magnitudes per class.
   3. Low energy per frame: `log(1 + sum of magnitudes of bins below 150 Hz)`.
   4. Clip beat frames to the last STFT frame. For each beat, the mean profile from this beat to the next (the
      beat's own frame when the span is empty; the last beat uses its own frame). Normalise each profile to unit
      length (+1e-9).
   5. Harmony change at beat `k`: `1 - dot(profile[k], profile[k-1])`; 0 for the first beat.
   6. Kick at beat `k`: the maximum of the positive first difference of the low energy over plus or minus 3 frames
      around the beat.
   7. Accent = harmony change / its standard deviation + kick / its standard deviation (+1e-9 each).
   8. Phase = the `p` in 0..3 whose beats `p, p+4, p+8, ...` have the largest mean accent.
9. **Double tempo**: `tempo_alt = round(12000/P, 1)` when `12000/P <= max_bpm` and the autocorrelation at
   `round(P/2)` is at least half the clarity; else `null`. A listener can tap along at either value. The tempo is never
   switched to the double; tuning showed that switching changed agreement with an independent tracker by at most 1
   in 52 excerpts.
10. Output: `tempo = round(6000/P, 1)`, `tempo_alt`, `beats` (frame times, 3 decimals), `downbeats` (every 4th
    beat from the phase), `pulse_clarity` (3 decimals), `beat_regularity` (3 decimals), `confidence`, and a note
    that beats have 10 ms resolution, downbeats are a guess under 4/4, and the tempo may be half or double the felt
    tempo.

A grid with confidence `none` still carries tempo and beats. Only `clear` and `weak` grids are used for
beat offsets and on-beat statistics.

### 2.6 Cues

`read_cues(cues)` returns `(cue list, fps or null)`. Accepted inputs:

1. `null`: no cues.
2. A string that names an existing file: read it as JSON.
3. A string that starts with `[` or `{` (after trimming): parse it as JSON.
4. Any other string: numbers separated by commas or semicolons (empty items ignored), in seconds. A parse failure
   raises an input error that names the expected formats and quotes the first 60 characters.
5. A JSON object: `fps` from key `fps`; the list from `events`, else `cues`, else empty.
6. A list. Each item is a number (seconds; kind `cue`, id `null`) or an object. For an object, the time key is the
   first of `t`, `time`, `sec`, `seconds` that is present; the frame key is the first of `f`, `frame`. A time key
   wins over a frame key. A frame key needs `fps`: `t = frame/fps`. A frame without fps raises an error that says to
   pass an object with `fps` and `events`. An item without time or frame raises an error that quotes the item
   (first 80 characters of its JSON) and says to give `t` in seconds or `f` with `fps`.
7. Each cue is `{t: round(t, 4), kind: item kind or "cue", id: item id or null}`. Sort by `t`.

### 2.7 Sync of sound to picture cuts and cues

Input: the target times (cuts as numbers or cues as objects), the event list (all events of the context, sorted
by time), optional beat times, an optional frame duration `frame_s`, and optional level envelopes.

Derived event sets:

- **strong**: events with `prominence_db >= min(6, 40th percentile of all prominences)`.
- **swells**: events with shape `swell`.
- **accents** (a deliberate hit): events with `prominence_db >= max(6, 80th percentile of all prominences)`.
  `accent_min_db` is that threshold, rounded to 0.1.

Tolerance `tol1 = frame_s` when given, else 0.04 s.

**Nearest event within radius `r`**: look at the neighbours around the insertion point of `t` in the sorted times
(index `k-1`, `k`, `k+1`); keep the one with the smallest `|time - t|` that is at most `r`. Ties keep the first
found.

**The event that answers a target at `t`** (first rule that matches wins):

| order | rule | status |
|---|---|---|
| 1 | any event within `tol1` | `OK` |
| 2 | a strong event within 0.1 s | `near` |
| 3 | any event within 0.1 s | `near` |
| 4 | a swell within 0.3 s | `swell` |
| 5 | a level peak (section 2.3) at `t`, when level envelopes are given | `peak` |
| 6 | a strong event within 0.25 s | `off` |
| 7 | nothing | `no sound` |

For rule 5, the answer is a pseudo-event `{t: peak t, prominence_db: rise_db, shape: peak, band}`.

**Item fields** (one per target, in input order): `t` (3 decimals); `kind` and `id` copied from a cue object when
they are not null; `offset_ms = (event t - t)*1000` (1 decimal; negative = sound early) or `null`; `status`; when
an event answers: `event_t`, `event_prominence_db`, and `event` (the event's label) when the event has a label;
`accent_offset_ms` = the offset of the nearest accent within 0.1 s, or `null`; when beats are given:
`beat_offset_ms = (t - nearest beat)*1000` (1 decimal; note the sign: target minus beat).

**Statistics** (`stats`): always `n`. When `n > 0`:

- Let `K` be `within_1_frame` when `frame_s` is given, else `within_40ms`.
- `hit_K`: share of items with status `OK` (3 decimals).
- `hit_within_100ms`: share with status `OK` or `near`.
- `median_abs_offset_ms`: median of `|offset_ms|` over every item that has an offset (all statuses), or `null`.
- `answered_by_swell`, `answered_by_level_peak`: counts of statuses `swell` and `peak`.
- `no_sound`: the times of items with status `no sound` or `off` ("without a hit").
- `frame_ms` (when `frame_s`) or `tolerance_ms`: `tol1*1000`, 1 decimal.
- Chance range `[lo, hi] = [min item t - 1, max item t + 1]`.
- When there are events: `chance_K` and `chance_within_100ms`.
- When there are accents: `accent_min_db`; `accent_K` = share with `|accent_offset_ms| <= tol1*1000 + 1e-6` (a
  `null` offset is a miss); `accent_chance_K`; `accent_within_100ms`; `accent_chance_within_100ms`.
- When at least 2 beats are given: `on_beat` = share with `|beat_offset_ms| <= 50`; `on_beat_chance =
  min(1, 2*0.05 / median inter-beat interval)` (3 decimals).

**Chance** `chance(times, lo, hi, tol)`: on a grid of times every 5 ms from `lo` (inclusive) to `hi` (exclusive),
the share whose distance to the nearest of `times` is at most `tol` (3 decimals; 0 when `times` is empty). This is
what random edit points would score with this event density. A 60 % hit rate means little when chance is 50 %.

### 2.8 Event tags (CLAP and AST)

**Budget** (constants table 3.1): `tag` events get CLAP tags, `ast` of those also get AST labels.

**Which events to tag** `pick(events, n, near_times, lo, hi)`:

1. An event is "near" when a cut or cue time lies within 0.1 s of it.
2. Priority order: near events first, then larger `prominence_db`, then larger `contrast_db`.
3. Bursts: walk the priority order. An event is a burst follower when an already kept event of the same band lies
   less than 0.5 s from it. Followers go after all kept events, in their priority order. One typing run then takes
   one slot, not the whole budget.
4. Spread over time: split `[lo, hi]` into `max(1, n // 2)` equal bins. In the combined order, the first event that
   falls in each not-yet-seen bin goes to a "first" list. The result is `first` followed by the rest of the
   combined order, cut to `n` items, then sorted by event index.
5. Return the chosen indices and the combined order (used for AST ranking).

**Event excerpt span**: for a swell, `[t - rise_ms/1000 (0.5 s when absent), t + 0.3]`; for a hit,
`[t - 0.02, t + clamp(length_ms/1000, 0.3, 1.5)]`. Clamp to `[0, duration]` and round to 3 decimals. The excerpt
is the mono signal from the span start to `max(start + 480 samples, end)`.

**Event vocabulary** (kind, then the caption-like phrases; every phrase is embedded through each of 5 prompt
templates and the template embeddings are averaged, then the average is normalised to unit length):

Templates, where `{}` is the phrase: `{}`, `the sound of {}`, `a recording of {}`, `{} sound effect`,
`an audio clip of {}`.

| kind | phrases |
|---|---|
| WHOOSH | a fast air whoosh; a swoosh passing by; a short swish; a whip pan whoosh transition |
| RISER | a rising riser building tension; a reverse cymbal swell; an uplifter sweep |
| HIT | a deep cinematic boom; a punchy impact hit; a heavy thud; a sub bass drop; a soft thump |
| KEY | a mechanical keyboard key press; typing on a computer keyboard; a laptop key click; a spacebar press |
| UI | a mouse click; a soft UI button tap; a notification ding; a chat message pop; a toggle switch click; a camera shutter click |
| STEP | footsteps on grass; footsteps on wood; a small soft footstep |
| FOLEY | a plastic click; lego bricks clicking; a wooden block hit; glass clinking; paper rustling; leaves rustling; a cork pop; a bubble pop; a water drop; a small object dropping on a table |
| NOTE | a piano note; a bell chime; a plucked string note; an orchestral stab; a celesta note; a glockenspiel note |
| DRUM | a kick drum; a snare drum hit; a hi-hat; a drum hit |
| VOICE | a spoken word; a person talking; a breath; a short vocal sound |

The phrase list has 49 unique phrases. The background table (constants table 3.4) must cover exactly these
phrases.

**Scoring** (hub-phrase damping): with CLAP audio embeddings `A` (one row per excerpt, unit length) and phrase
embeddings `E`:

1. `cos = A @ E^T`.
2. `d = cos - 0.5 * bg[phrase]`, where `bg` is the table 3.4 value of the phrase (the mean cosine of the phrase
   over 536 real background events). Some phrases score high on any short sound. Without this damping they win
   everywhere.
3. `z` = `d` standardised within each event row: `(d - row mean)/(row std + 1e-9)`.

**Tag fields** set on each chosen event:

- `tags`: the top 3 phrases by `z`: `[{tag, kind, z (2 decimals)}]`.
- `kind`: the kind of the top phrase.
- `label`: the top phrase without a leading `a `, `an ` or `the `.
- `exposed`: true when `prominence_db >= 12` (the event dominates its excerpt; its tag is right about twice as
  often).

**AST labels**: the chosen events in the combined priority order, first `ast` of them. For each, run AST on the
same excerpt span (cached). Set `ast` to the top 4 AudioSet labels with probability at least 0.05, as
`[[label, p], ...]`. When the AudioSet `Speech` probability is at least 0.5 and the kind is not already VOICE,
set `kind = VOICE`, `label = "speech (AST p)"` with `p` at 2 decimals, and `exposed = true`.

Return `{tagged, ast, clap_windows}` (counts). With no events or a zero tag budget, return zeros and run no model.

Use CLAP with repeat padding of each excerpt to the model window (core-io `embed_audio` option). Tuning showed
repeat padding beats zero padding and bed subtraction on the tuning mixes, at half the cost of bed subtraction.

**Per-span model cache**: each excerpt's CLAP embedding and AST probability vector are cached on disk next to
the model's own cache. The key hashes: the source file id (core-io) plus a mono-decode marker for mono sources,
the span start and end in file time (3 decimals), the output kind (`clap-repeat` or `ast`), and a span-definition
version (start at 1; bump when an excerpt definition changes). A cached entry that fails to load is recomputed.
Writes are atomic (temporary file, then replace). With reuse off (`--fresh`), recompute and overwrite.

If saving the shared text-embedding cache fails with a permission error (a file locked by another process),
continue with the embeddings in memory for this process.

### 2.9 Sections

Input: the analysed window `x` (not the context), its file-time offset `t0`, window length `win`, hop `hop`, the
AST crop budget, merge threshold 0.84, minimum section length 4 s, novelty threshold 3.0.

When the window is shorter than 1 s, return no sections and `{windows: 0, ast: 0}`.

**Vocabulary**: every term of the shared listening vocabulary (listen unit asset `vocab.json`: categories of terms
and prompt templates). Embed each term through the vocabulary's own templates (mean, unit length). The section
tags use only the categories `music_genre`, `music_mood`, `instruments`, `ambience`, `voice`, but the z-scores are
taken over all terms.

**Windows**: starts at `0, hop, 2*hop, ...` up to `max(dur - win, 0)`. When there is no start, or the last start
plus `win` ends more than 0.5 s before the end, add a start at `max(0, dur - win)`. Span = `[start, min(dur,
start + win)]`. Embed each window with CLAP (repeat padding, per-span cache, file-time keys).

**Novelty bounds** (strong timbre or level changes, Foote novelty) on the mono window:

1. `L = logmel(m, n_fft=2048, hop=2400, n_mels=40, fmin=30, fmax=16000)` (50 ms frames).
2. Average groups of 10 frames into 0.5 s frames: `n = frames // 10`. Kernel half width `w = 4 s / 0.5 = 8`.
   When `n < 4w`, there are no bounds.
3. Standardise each mel band over time (`(F - mean)/(std + 1e-6)`), then normalise each 0.5 s frame to unit length.
4. Self-similarity `S = F^T F`.
5. Checkerboard kernel of size `2w`: sign `+1` on the two diagonal blocks and `-1` on the two off-diagonal blocks,
   times a Gaussian taper `g(k) = exp(-0.5*((k + 0.5)/(0.5*w))^2)` for `k = -w .. w-1`, as `outer(g, g)`.
6. `nov[i] = sum(K * S[i-w : i+w, i-w : i+w])` for `w <= i < n - w`; 0 elsewhere.
7. `z = robust_z(nov, n)`.
8. Bounds: frames where `nov` equals its maximum over `2w + 1` frames, `z >= 3.0`, and `w <= i < n - w`. Return
   `(i*0.5, z)` pairs (time 2 decimals, z 1 decimal).

**Merging windows into sections** (agglomerative, neighbours only):

1. Start with one segment per window. A segment's vector is the unit-normalised mean of its window embeddings.
   A segment's length is the end of its last window minus the start of its first.
2. Junction time between neighbours `a` and `b`: `start(b first window) + h/2`, where `h = win` when `b`'s first
   window starts at or after the end of `a`'s last window, else `h` = the overlap (`end(a last) - start(b first)`).
   The junction is **blocked** when a novelty bound lies within `hop` of it.
3. Max segment count `max_n = max(2, ceil(dur/8))`.
4. Repeat while more than 1 segment remains:
   1. For each neighbour pair: `sim = dot(vector(a), vector(b)) - (1 if blocked else 0)`.
   2. "Short" pairs: pairs where either segment is shorter than the minimum length (4 s).
   3. `j` = the pair with the largest `sim`.
   4. When `sim[j] < 0.84` and the segment count is at most `max_n`: stop if there are no short pairs, else set `j`
      to the short pair with the largest `sim`.
   5. Merge pair `j`.
5. Boundaries: 0, then for each junction a coarse time (the midpoint of the overlap, `(start(a last) + start(b
   first) + win)/2`, when the windows overlap, else `start(b first)`), refined by the novelty refine step below with
   radius `hop`; then the duration rounded to 2 decimals.
6. Sliver merge: walk boundaries `k = 1 ..`; when the section before or after boundary `k` is shorter than 2 s
   (half the minimum length), merge segment `k` into segment `k-1` and drop boundary `k`; else advance.

**Novelty refine** (move a coarse boundary to the strongest spectral change within plus or minus `radius`):
take the signal from `t - radius - 1` to `t + radius + 1` (clamped); when it is shorter than 2 s, keep `t`.
Compute `logmel(n_fft=2048, hop=2400, n_mels=40)` (fmin 30, fmax 16000). For each frame `k` from 10 to `frames -
10` with `|a + 0.05k - t| <= radius`, the change is the Euclidean distance between the mean of the 10 frames
before `k` and the mean of the 10 frames from `k`. Return the time of the largest change, rounded to 2 decimals.

**Section fields**: `start`, `end` (2 decimals), `windows` (count), `coherence` (mean dot product of the section's
window embeddings with its vector, 3 decimals); for each of the five categories the top 2 terms by z within that
category: `[{tag, cat, z (2 decimals), cos (3 decimals)}]`; `lufs` (integrated loudness of the section when it is
longer than 0.4 s and above -70, rounded 0.1, else `null`); `digital_silence` (every sample 0).

**Section AST** (budget `ast_sec`; no AST when 0):

1. Order sections by length, longest first. Plan 1 crop for each of the first `budget` sections.
2. Then, in the same order, give a second crop to a planned section longer than 15 s while the total stays below
   the budget.
3. Crop length `w = min(section length, 10.24 s)`. One crop: centred. Two crops: starts at `start + (d - w)*(j +
   0.5)/2` for `j = 0, 1`.
4. Average the AST probability vectors of a section's crops. Set `ast = {Music, Speech, Silence}` (3 decimals) and
   `ast_top` = the top 5 labels with probability at least 0.05.
5. Return the number of crops.

**Section label** (text):

1. `digital silence` when every sample is 0.
2. `near silence` when `lufs` is `null` or below -55.
3. Music decision: with AST, `Music >= 0.3`; without AST, the top genre z is at least the top ambience z.
4. When music: the genre (short form) with a `?` when its z is below 2.5; the top instrument when its z is at
   least 1.5; the top mood with the word `music` removed when its z is at least 2.5 (weaker moods were wrong about
   45 % of the time under sound effects). Join with `, `.
5. Extra sources: AST top labels (lowercased) that are not generic and have probability at least 0.08, at most 3,
   as `also: x, y (AST)`. Generic AudioSet labels: Music; Musical instrument; Speech; Silence; Sound effect;
   Inside, small room; Outside, rural or natural; Outside, urban or manmade; Inside, large room or hall; Animal.
   When there are no extras: `amb: <top ambience>` when its z is at least 2.5 (music) or 1.5 (no music).
6. Voice: `voice: <top voice term>` when AST `Speech >= 0.3`, or, without AST, the top voice z is at least 2.5.
7. Join the parts with ` | `. With no part, a fallback that says no confident music, ambience or voice tag.

**Merge neighbours that read alike**: the key of a label is (the first comma item of its first ` | ` part, the
list of the other parts). Walk neighbours. Merge `a` and `b` when their keys are equal and no novelty bound lies
within 1.0 s of `b.start`. The merged section keeps every field of the longer of the two, with `start = a.start`,
`end = b.end`, `windows = a.windows + b.windows`, and a recomputed level (`lufs`, `digital_silence`). The label is
not recomputed. Return the sections and `{windows: number of CLAP windows, ast: number of AST crops}`.

### 2.10 The analysis run

1. Validate `detail`. Check that FILE (and `--video`) exist.
2. Probe FILE (core-io). Picture detection: a file is a video when it has a video stream with
   `0 < fps <= 240` and its extension is not an audio extension (cover art in an MP3 or M4A reports a huge frame
   rate such as 90 000 and is not a video).
3. Without `--video`, a video FILE is its own picture. A `--video` without a picture stream is an error.
4. A FILE with no audio stream and no picture is an error.
5. Read the cues.
6. Result cache lookup (section 2.12).
7. **Load range**:
   - Picture only (no audio stream): analyse digital silence (stereo zeros) of the probed duration; context start
     0; window `[max(0, start), min(duration, end)]`.
   - Full file (no zoom): decode everything. A file shorter than 0.1 s is an error that suggests the one-shot
     commands of the listen unit.
   - Zoom: the duration from the probe (else from a decode). `t0 = max(0, start)`, `t1 = min(duration, end)` or the
     duration. A window shorter than 0.1 s is an error. Context `[max(0, t0 - 30), min(duration, t1 + 30)]`; decode
     only the context.
8. The window `x` is the slice of the context from `t0 - c0` to `min(t1 - c0, context length)`.
9. Loudness of the window (2.4), with `at_start = t0 <= 0.01` and `at_end = t0 + dur >= duration - 0.01`.
10. When the window is digital silence, there are no events. Else detect events on the whole context (2.2) with
    `lead = (c0 == 0)`.
11. Level envelopes of the context (2.3). Beat grid of the mono context (2.5).
12. With a picture: frame rate from video info; picture cuts (sheet-video part) strictly inside
    `(t0 + 0.05, t0 + dur - 0.05)`, as context times.
13. Cues inside `[t0, t0 + dur]` (1e-6 slack), as context times.
14. Window events: context events with `wa <= t < wb` (1e-6 slack on the low side).
15. Tag the window events (2.8) on the context signal, with near times = cuts plus cue times and the spread range
    `[wa, wb]`.
16. Section hop: `max(config hop, round((dur - win)/max(1, max_win - 1), 2))`, so long files keep at most
    `max_win` windows.
17. Sections: for digital silence, one section `{start 0, end dur, windows 0, label "digital silence", lufs null,
    digital_silence true}` when the window is at least 1 s, else none; no model runs. Else 2.9 on the window.
18. When the beat grid confidence is `clear` or `weak`: each window event gets `beat_offset_ms = (t - nearest
    beat)*1000` (1 decimal).
19. Sync (2.7) of cuts and of cues against **all context events**, with the beats (clear or weak only), the frame
    duration (`1/fps` of the video for cuts; `1/cue fps` for cues when the cue file gives fps) and the context level
    envelopes. The context makes a zoom's verdicts equal those of the full analysis.
20. Shift every time to file time: loudness gaps, `max_momentary.t`, `max_short_term`, curve `t0`, clipping
    times, section bounds (add `t0`); events, beats, downbeats, sync item times, `event_t` and `no_sound` (add `c0`).
    Keep only beats and downbeats inside the window.
21. Record stage timings (`decode_s`, `loudness_s`, `events_s`, `beats_s`, `cuts_s`, `tag_events_s`,
    `sections_s`, `total_s`, seconds with 2 decimals) and model call counts.
22. Build the summary (2.11) and the transcript (2.11).

### 2.11 Summary and transcript

**Summary**: one ASCII paragraph of sentences, in this order. Write the wording in your own words; the tokens in
capitals are fixed because tests and agents search for them.

1. File name, window `fmt_t(start)-fmt_t(end)` and duration (1 decimal).
2. Exactly one of:
   - no audio stream: `NO AUDIO STREAM`, picture only, the number of picture cuts, every cut or cue unsounded;
   - digital silence: `DIGITAL SILENCE`, every sample is zero;
   - integrated loudness present: integrated LUFS and true peak (1 decimal), LRA when present, and the loudest
     short-term window with its level;
   - else: the true peak and either "below the -70 LUFS gate: near silence" (window at least 0.4 s) or "too short
     for a loudness measurement".
3. At most one peak sentence: `CLIPPING` with the run count and the first run time; else, for lossy decoded audio
   with true peak above 0 dBTP, a warning that it clips on playback (a master should leave the mixer at -1 dBTP or
   lower before encoding); else, for lossy audio above -1 dBTP, a note that codec overshoot is included and is fine
   for a delivered file; else, for true peak above -1 dBTP, a risk of overs after encoding.
4. Gaps without `where` (stop-downs): their count and up to 5 of `start dur kind`.
5. Edge gaps (`lead-in` or `tail`; not `whole file`): `Quiet <where> <dur> s (<kind>)`, joined by "and".
6. Beat grid when there is a tempo and confidence is not `none`: `~<tempo>` (and `or <tempo_alt>` when present)
   BPM, the confidence, clarity (2 decimals) and the beat count. Else, unless digital silence: no steady beat grid,
   with the clarity (at least 0) and the 0.15 threshold when a clarity exists.
7. Sections (unless digital silence): the count and up to 6 `start-end label`.
8. Events: the count, the rate per second (1 decimal), the tagged count, and the counts per kind (largest first,
   lower case), marked as hints. Then the strongest accents over a running bed (section 2.11 "top accents") with
   their time, `+N dB` and label.
9. One sync sentence per kind (`cuts`, then `cues`) when it has items:
   - the item count; the `OK` share "within 1 frame" or "within 40 ms" with its chance; the share within 100 ms;
     the median absolute offset;
   - the accent share with the accent threshold in dB over the bed, plus "ranked over the +-30 s context" when the
     run is zoomed (context differs from `[start, end]`), with its chance;
   - the on-beat share with its chance;
   - the count met by a whoosh or riser peak (`swell`); the count met only by a level peak;
   - the count without a hit and up to 6 of their times.

**Top accents** `top_accents(events, n=3)`: events whose `bed_db` is above -60 (a running bed), in order of
`prominence_db`, keeping those at least 0.5 s from every already kept one, at most 3. Over a bed below -60 (after
a gap) the rise means nothing.

**Transcript** (`text`): ASCII lines.

1. The summary, a blank line.
2. When there are sections: a `SECTIONS` header, then one line per section: `fmt_t(start)-fmt_t(end)`, the label,
   the level `N LUFS` (0 decimals) when known, and `[AST music p, speech p]` (2 decimals) when AST ran. A blank
   line.
3. A column header line that starts with `TRANSCRIPT` and names the columns: time, kind, what (CLAP tag /
   runner-up), peak, over bed, length, centroid, sync.
4. Chronological rows, sorted by (time, rank) with rank 0 for section changes, 1 for events and gaps, 2 for
   unanswered cuts or cues:
   - **Section change**: `==`, the section span, its label, and `(+N untagged onsets, R/s)` when the section holds
     events without a label and without a sync mark (rate over the section length).
   - **Event** (only events with a label, or events that carry a sync mark): time (`fmt_t`, right-aligned in 8
     characters); kind column: the kind plus `?` when not exposed, or `-` for an untagged event; what: the label
     plus ` / <runner-up>` where the runner-up is the first of the top 3 tags with a different kind (short form), or
     `<band> swell` / `<band> onset` for an untagged event, cut to 46 characters; `peak_dbfs` with unit `dBFS`;
     the rise over the bed as a signed dB number when `bed_db > -60`, else the word `silent`; `length_ms` with unit
     `ms`; the centroid as `N Hz` below 1000, `x.y kHz` from 1000, `-` when unknown; the first AST label (skip
     `Music` unless it is the only label) as `[AST label p]`; then ` | ` and the marks joined by `; `.
   - **Marks**: for each cut or cue item whose answering event exists and whose status is not `off` or `peak`:
     `<who> <offset:+.0f> ms <status>`, where `who` is `cut` for cuts and the cue's kind (or `cue`) for cues; e.g.
     `cue -20 ms OK`. Each of the top 3 accents gets the mark `strong accent (summary)`.
   - **Unanswered cut or cue** (status `no sound`, `off` or `peak`): time; `CUT` or `CUE`; for cues the kind (when
     not `cue`) and the id; then the reason. `peak`: no onset, the level peak offset and its rise in dB, a question
     whether it is a soft whoosh or swell or just the bed, and the token `<- CHECK`. `no sound`: no onset or level
     peak within 250 ms, then `<- NO HIT`. `off`: no onset within 100 ms, the offset of the strong sound found,
     then `<- NO HIT`. When the cue kind or id contains a build-up word (whoosh, swoosh, swish, riser, rise, swell,
     sweep, wave, uplift), add a hint that a soft build-up can still be missed and to look at the sheet.
   - **Gap**: time, `GAP`, its kind, its duration (2 decimals), its level (`< -70` when at most -70, else 0
     decimals) in LUFS, and `= <where>` when it has one.
5. A blank line and a `NOTES` paragraph in your own words: times, levels, loudness and sync offsets are
   measurements; kinds and tags are CLAP guesses; `?` means the event is less than 12 dB over its bed so its tag is
   a weak hint (right kind about 1 in 4 on the tuning mixes, 1 in 2 for exposed events, often wrong on music and
   voice); AST speech overrides the kind with VOICE; section genres with `?` are weak; "over bed" is the rise over
   the level just before the event and `silent` means that bed was below -60; the full data is in `--json`.

### 2.12 Result cache

- Key: a hash (24 hex characters) of: the core-io file id of FILE; the file id of the video (or none); the cue key
  (the file id of a cue file, else a hash of the cue value serialised with sorted keys, or `none`); start; end;
  detail; a code version of this unit; a core version (decode, features, video cut detection, configuration).
  Cached results must never outlive a change of the analysis code or of the core decode code. A version constant
  bumped on each change, or a hash of the source files, both meet the rule.
- Location: `<hark cache root>/timeline/<key>.json` (core-io defines the cache root).
- Read: when caching is on and `--fresh` is off and the file exists, return the stored result with
  `cached: true` and `file` set to the path as given now. A file that fails to parse is ignored.
- Write: when caching is on or `--fresh` is set, write the result atomically (temporary file in the same folder,
  then replace). The stored result does not contain `cached`. The returned result has `cached: false`.
- `--fresh` also disables reuse of the per-span model cache (2.8), so every model output is recomputed and
  rewritten.

## 3. Constants

### 3.1 Detail budgets

| detail | CLAP-tagged events | AST event labels | section window s | section hop s | max section windows | AST section crops |
|---|---|---|---|---|---|---|
| low | 12 | 0 | 4.0 | 4.0 | 30 | 0 |
| normal | 20 | 1 | 3.0 | 2.0 | 32 | 3 |
| high | 96 | 16 | 3.0 | 1.5 | 200 | 12 |

### 3.2 Thresholds and windows

| name | value | meaning |
|---|---|---|
| sample rate | 48 000 Hz | core rate |
| onset hop | 240 samples (5 ms) | onset frame |
| onset log-mel | n_fft 1024, 96 mels, 30-16 000 Hz | onset spectrogram |
| robust z window | 401 frames | about 2 s |
| onset peak window | 11 frames | plus or minus 25 ms |
| onset z threshold | 3.0 | candidate |
| min band contrast | 3 dB | candidate |
| min gap between hits | 30 ms | merge |
| band split | low < 200 Hz, high > 2 kHz | 4th-order Butterworth |
| attack window | -15..+50 ms; bed -160..-40 ms; walk back to -40 ms | attack refine |
| attack level | 10 % of the peak excess (20 dB) | first ms |
| full-band timing ratio | 4.0 (12 dB) | timing band |
| length floor | 20 dB under peak or 3 dB (x1.41) over bed, held 30 ms, cap 2000 ms | length |
| lead event | peak of first 50 ms >= 1e-3, no hit before 50 ms | file start |
| bed floor | -80 dBFS (1e-4) | beds |
| prominence | K-weighted 10 ms power at 1 ms; bed -300..-20 ms median; peak over 80 ms | prominence |
| silent bed | -60 | "over bed" shown as `silent` below |
| level envelope | 10 ms RMS, -80 floor, 90 ms smoothing | swells, level peaks |
| swell | rise >= 8 dB over 0.25-2 s, >= 60 % rising steps, then falls >= 4 dB within 1 s | swell |
| swell peak window | 31 frames (plus or minus 150 ms), > -70 dB | swell |
| swell vs hit | skip when a hit is within 50 ms of `t - 0.03`; swells 0.15 s apart | swell |
| level peak | within 150 ms, local max over plus or minus 0.3 s, rise >= 3 dB over 0.6 s, fall >= 2 dB within 0.3 s | sync |
| near | 0.10 s | tag priority, sync |
| sync search radius | 0.25 s | `off` |
| default tolerance | 40 ms (one frame when the frame rate is known) | `OK` |
| swell answer radius | 0.30 s | `swell` |
| on beat | 50 ms | `on_beat` |
| strong | prominence >= min(6, p40) | sync |
| accent | prominence >= max(6, p80) | sync |
| chance grid | 5 ms | chance |
| exposed | 12 dB | tag confidence |
| burst | 0.5 s, same band | tag budget |
| tag damping | 0.5 times the background mean | hub phrases |
| context | 30 s each side | zoom |
| section kernel | 4 s half width on 0.5 s frames | novelty |
| novelty z | 3.0 | section change |
| merge cosine | 0.84 | sections |
| min section | 4 s (slivers under 2 s merged) | sections |
| label z | ok 2.5, weak 1.5 | labels |
| section AST crop | 10.24 s, second crop over 15 s | AST |
| pulse clarity | clear >= 0.30 (and regularity < 0.1), weak >= 0.15 | beat grid |
| tempo range and prior | 60-200 BPM, prior at 120 BPM, width 0.9 octave | beat grid |
| DP penalty | 100 * ln(lag/P)^2, lags P/2..2P | beat grid |
| gap | 0.1 s loudness, 0.05 s hop; threshold max(-60, I - 20) (or -70); >= 0.25 s | gaps |
| clipping | abs >= 0.999, runs >= 3 samples, first 20 listed | clipping |
| minimum length | 0.1 s file; 0.4 s momentary; 3 s short-term; 5 s beat grid; 6 s LRA; 1 s sections | lengths |

### 3.3 Build-up words

whoosh, swoosh, swish, riser, rise, swell, sweep, wave, uplift.

### 3.4 Tag background means (mean cosine of each phrase over 536 real background events)

| phrase | mean | phrase | mean |
|---|---|---|---|
| a fast air whoosh | -0.0137 | a swoosh passing by | -0.0271 |
| a short swish | 0.0538 | a whip pan whoosh transition | 0.0173 |
| a rising riser building tension | -0.0403 | a reverse cymbal swell | -0.0354 |
| an uplifter sweep | -0.0522 | a deep cinematic boom | -0.0294 |
| a punchy impact hit | 0.0361 | a heavy thud | 0.0383 |
| a sub bass drop | -0.0194 | a soft thump | -0.0344 |
| a mechanical keyboard key press | 0.1109 | typing on a computer keyboard | -0.0065 |
| a laptop key click | 0.1327 | a spacebar press | 0.1456 |
| a mouse click | 0.0743 | a soft UI button tap | 0.0578 |
| a notification ding | 0.0487 | a chat message pop | 0.1634 |
| a toggle switch click | 0.1030 | a camera shutter click | 0.0964 |
| footsteps on grass | -0.0284 | footsteps on wood | -0.0025 |
| a small soft footstep | 0.0282 | a plastic click | 0.0729 |
| lego bricks clicking | -0.0168 | a wooden block hit | 0.0335 |
| glass clinking | -0.0419 | paper rustling | -0.0469 |
| leaves rustling | -0.0228 | a cork pop | 0.0633 |
| a bubble pop | 0.0977 | a water drop | 0.1012 |
| a small object dropping on a table | 0.0316 | a piano note | 0.0143 |
| a bell chime | -0.0081 | a plucked string note | 0.0687 |
| an orchestral stab | 0.0640 | a celesta note | 0.0378 |
| a glockenspiel note | 0.0350 | a kick drum | 0.0659 |
| a snare drum hit | 0.0322 | a hi-hat | 0.0719 |
| a drum hit | 0.0758 | a spoken word | -0.0140 |
| a person talking | -0.1026 | a breath | 0.0520 |
| a short vocal sound | 0.0318 | | |

These means depend on the exact CLAP checkpoint (core-io) and the templates above. With another checkpoint,
regenerate them (index, data asset A2).

## 4. Errors and edge cases

| case | behaviour |
|---|---|
| `detail` not low, normal or high | input error that lists the allowed values |
| FILE or `--video` missing | file-not-found error naming the path |
| `--video` without a picture stream (an audio file) | input error: the named file has no picture stream |
| FILE with neither audio nor picture | input error: no audio stream |
| FILE shorter than 0.1 s | input error with the length; suggests the one-shot listen commands |
| zoom window shorter than 0.1 s (for example `--from 5` on a 3 s file) | input error: nothing to analyse between the start and the end (or "the end"), with the file length |
| picture-only video | analysed as digital silence of the same length; summary `NO AUDIO STREAM`; every cut has status `no sound` |
| audio file with cover art | not a video (no frames lane, no cuts, `cut_method` null) |
| digital silence | `DIGITAL SILENCE`; integrated and true peak null; one `digital silence` section; one gap with `where: whole file`; no model runs; momentary values all at -70 |
| file shorter than 0.4 s | no momentary curve; the summary says too short for a loudness measurement |
| file that starts with sound | an event in its first milliseconds (`starts_file`) with bed -80 |
| gap at the file start or end | `lead-in` or `tail`, not a stop-down; in a zoom only when the window touches the file edge |
| quiet event after a gap | prominence is huge over the -80 floor; the transcript shows `silent`; top accents skip it |
| no beat grid (noise, ambience) | confidence `none`; no beat offsets; summary says no steady beat grid |
| frame cues without fps | input error that shows the object form with `fps` |
| cue without a time | input error that quotes the cue |
| bad cue string | input error naming the 3 accepted forms |
| non-ASCII file names | backslash escapes in text; exact in JSON |
| corrupt cache entry | ignored and recomputed |
| models not installed | Q3 in the index |
| lossy decoded audio | true peak includes codec overshoot: up to 0 dBTP is normal for a delivered file; above 0 dBTP it clips on playback |

## 5. Acceptance tests

The acceptance tests and evaluation targets of this part are in `docs/specs/hark/timeline-analysis-tests.md`.
