# hark fit: quality measure (part of fit.md)

This part specifies the fit quality measure: how close a render is to its recording. Every fitter, the engine's
choice, the post stage, the reports and the acceptance tests use it. The index is `fit.md`.

Module: `hark.forge.fit.quality`. Pure numpy. A picture writer uses matplotlib. CLAP and describe are optional
model checks that load only on request.

## Interface

All audio is 48 kHz float64. A sound is a path, a 1-D array (mono) or an `(n, 2)` array (stereo).

| function | returns |
|---|---|
| `load(x)` | A path loads through core-io as `(n, 2)` float64 at 48 kHz. An array comes back as float64, shape unchanged. |
| `mono(x)` | The channel mean of an `(n, 2)` array, or the 1-D array unchanged. |
| `quality(original, resynth, clap=False, describe=False)` | The metrics dict below. |
| `score(q)` | One float. Lower is better. |
| `floor(render_fn, seeds=(11, 12), clap=False)` | `quality(render_fn(11), render_fn(12))`: the distance between 2 takes of one model. |
| `text(q, fl=None)` | One readable line of a quality dict, with the floor's `mel_db` when `fl` is given. |
| `pair_png(path, original, resynth, title=None, q=None, extra=None)` | Writes the side-by-side picture. Returns `path`. `extra` is `(label, array)` for a third column (morph: A, morph, B). |
| `peak_rms_db(x, win_s=0.05)` | The loudest 50 ms RMS (channel-mean power) in dB. |
| `onset_index(m, db=-30, hop=48)` | The sample index of the first 1 ms frame within 30 dB of the loudest 1 ms frame. |
| `attack(m)` | `(attack_ms, sharpness_db_per_ms)` of the first onset. |
| `env_db(m, ms=5)` | Non-overlapping frame power envelope in dB. |
| `melspec(m, n_fft)` | Log-mel power, frames x 64. |
| `bandgram(m, nfft=1024, hop=512)` | Third-octave band power in dB per frame. |
| `mel_distance(a, b, detail=False)`, `env_corr(a, b)`, `excess(a, b)`, `stereo(a, b)`, `tone_db(a, b)`, `t40(m)` | The single metrics. |
| `clap_cos(a, b)`, `describe_pair(a, b)` | The optional model checks (listen unit). |
| constants `SR = 48000`, `FRAMES = (256, 1024, 4096)`, `N_MEL = 64`, `WIDTH_W = 0.04`, `WIDTH_CAP_DB = 20`, `TAIL_CORR_W = 1.0` | |

### The metrics dict

Field names are the public interface. A pair `(a, b)` holds the original's value first, then the render's.

| field | type | meaning |
|---|---|---|
| `mel_db` | float, 3 decimals | Multi-resolution log-mel distance in dB. 0 means the same spectrogram. |
| `mel_256`, `mel_1024`, `mel_4096` | float, 2 decimals | The distance at each FFT size. |
| `env_corr` | float, 4 decimals | Pearson correlation of the 5 ms dB envelopes. |
| `attack_ms` | pair, 2 decimals | First onset to its local peak, ms. |
| `sharp_db_ms` | pair, 2 decimals | Onset sharpness, dB per ms. |
| `onset_err` | float, 3 decimals | `abs(log2(sharp_render / sharp_original))`, octaves. |
| `tone_db` | float, 3 decimals | RMS difference of the 2 third-octave balance profiles, dB. |
| `excess_db`, `missing_db` | float, 3 decimals | Energy the render has where the original is quiet, and the reverse. |
| `t40_ms` | pair, 1 decimal | From the 5 ms envelope peak until it first falls 40 dB. |
| `level_db` | float, 2 decimals | Render minus original, loudest 50 ms RMS, dB. |
| `len_s` | pair, 4 decimals | Lengths in seconds. |
| `side_db` | pair, 2 decimals | Side / mid energy ratio over the original's active span, dB, floored at -40. |
| `width_db` | float, 3 decimals | Per-octave-band width error, weighted by the original's band level. |
| `tail_corr` | pair of float or null, 4 decimals | L/R correlation over the original's tail. Null when the tail is shorter than 20 ms. |
| `tail_corr_err` | float, 4 decimals | Absolute difference of the 2 tail correlations. 0 when either is null. |
| `clap_cos` | float, 4 decimals | Only with `clap=True`. Cosine of the 2 CLAP audio embeddings. |
| `describe` | object | Only with `describe=True`. Per key `original` and `resynth`: `tag` (top tag term), `z` (2 decimals or null), `audioset` (top label), `p` (2 decimals or null), `tags3` (the top 3 tag terms). |

## Behaviour

1. The mono measures use the channel mean of each sound.
2. Both sounds are cut at the original's onset minus 5 ms. The same cut applies to the render, so a timing error in
   the render still counts.
3. Each mono sound is then levelled so that its own loudest 50 ms RMS is 0 dB. The level difference is reported
   only in `level_db` and does not enter the score.
4. Both are zero-padded at the end to the same length.
5. The stereo measures use the onset cut, but no levelling, because they are ratios.

## Algorithms

### Onset index

1. Cut the mono signal into frames of 48 samples (1 ms). Take the mean power of each frame in dB.
2. Return the start sample of the first frame whose level is more than `max - 30 dB`.
3. When there are fewer than 2 frames, return 0.

### Alignment

`on = max(0, onset_index(A) - 240)` samples (240 samples = 5 ms). Cut both mono signals from `on`. Scale each by
`10^(-peak_rms_db/20)`. Zero-pad both to the longer length.

### Log-mel spectrogram

- Mel scale: `mel(f) = 2595 log10(1 + f/700)`.
- 64 triangular filters with centres equally spaced in mel from 30 Hz to 20 kHz (66 points, filter i uses points
  i, i+1, i+2 as its left, centre and right edge). Filter weight is the triangle on the linear frequency axis
  (peak 1 at the centre). A filter with no bin inside it gets weight 1 on the bin nearest its centre.
- Frames: Hann window of length `n_fft` (the symmetric Hann of `n_fft + 2` points with both zero end points
  removed), hop `n_fft / 4`, centred: the signal is placed `n_fft / 2` samples into a zero buffer. Frame count is
  `ceil(len / hop)`, at least 1.
- Power spectrum per frame, filtered, plus 1e-20, in dB (`10 log10`, floor 1e-20).

### mel_db

For each `n_fft` in 256, 1024, 4096:
1. Compute both log-mel spectrograms of the aligned signals.
2. `top` = the maximum cell of the two. Floor both at `top - 80` dB.
3. Active cells: the louder of the 2 values is above `top - 60`.
4. The distance is the mean of `|MA - MB|` over the active cells (0 when none is active).

`mel_db` is the mean of the 3 distances.

### env_corr

1. 5 ms envelopes (240 sample frames, mean power in dB) of the 2 aligned signals.
2. Floor both at `top - 60` where `top` is the larger maximum.
3. Active frames: the louder of the 2 is above `top - 60`.
4. Return 0 when 3 or fewer frames are active or either active envelope has a standard deviation under 1e-9.
5. Otherwise return the Pearson correlation over the active frames.

### attack and sharpness

1. Envelope on 0.125 ms frames (6 samples). When it has fewer than 4 frames, return `(0.0, 1.0)`.
2. Onset frame: the first frame above `max - 20 dB`.
3. Local peak: the loudest frame in the onset frame and the next 80 frames (10 ms).
4. Walk back from the peak while the frame is above `peak - 20 dB`. Call that frame `j`.
5. Find the first frame after `j`, up to the peak, at or above a level. Interpolate the crossing time linearly inside
   the frame before it. Do not interpolate when the found frame is `j` itself or the 2 frames are equal.
6. Rise time = (crossing of `peak - 3 dB` minus crossing of `peak - 20 dB`) x 0.125 ms, at least 0.02 ms.
7. `attack_ms` = 0.125 x (peak frame - onset frame). Sharpness = 17 dB / rise time.

The sharpness does not depend on a recording floor or a fade-in under the sound, because it uses only the rise from
20 dB to 3 dB under the local peak.

### onset_err

`abs(log2(sharpness_render / sharpness_original))`. 1 octave means twice as sharp or twice as blunt.

### tone_db

The tone profile of a sound (forge-core unit): 28 third-octave bands with centres `1000 x 2^(k/3)` for k = -15 .. 12
(31.5 Hz to 15.9 kHz), band edges `centre / 2^(1/6)` to `centre x 2^(1/6)`. Band power from one FFT of the whole
mono sound, in dB relative to the total of the 28 bands, floored at -60 dB. `tone_db` is the RMS over the 28 bands of
the difference of the 2 profiles. It uses the unaligned mono sounds.

### Band-gram

Third-octave band power per frame:
- FFT 1024, hop 512, centred Hann framing as for the mel spectrogram.
- The 28 tone bands, except that the 3 lowest bands are merged: band index 2 holds 22-112 Hz and bands 0 and 1 are
  empty (their power is 0, so they sit at the floor).
- dB with floor 1e-20.

### excess_db and missing_db

Parameters: tolerance 6 dB, depth 100 dB.
1. Band-grams of the aligned signals A (original) and B (render).
2. `top` = the larger maximum. Floor both at `top - 100`.
3. Active cells: the louder is above `top - 90`.
4. `excess = mean over active cells of max(0, B - A - 6)`.
5. `missing = mean over active cells of max(0, A - B - 6)`.
6. When no cell is active, both are 0.

Purpose: these catch a hiss band, a noise floor or a ring that outlives the original, 60 to 100 dB down, where the
mel and envelope measures do not look. They follow CLAP's verdicts on fits closely.

### t40

From the loudest 5 ms frame, the time until the first frame more than 40 dB under it, in ms (5 ms per frame). When
the envelope never falls that far, the time to the last frame.

### Stereo measures

The space helpers (`fit-space.md`) give: `_stereo(x)` (any input as `(n, 2)`, mono duplicated), `active_span`,
`side_powers`, `tail_span` and `corr`.

1. Make both `(n, 2)`. Cut both at `onset_index(mean of A's channels) - 240`. Zero-pad to one length. No levelling.
2. When neither input is 2-D, or the aligned length is under 64 samples, return `side_db = (-40, -40)`,
   `width_db = 0`, `tail_corr = (null, null)`, `tail_corr_err = 0`.
3. `end` = the end of the original's active span, at least `min(len, 960)` samples (20 ms).
4. For each sound, over samples `[0, end)`: octave-band mid power `PM` and side power `PS` per frame (with
   `M = (L + R) / 2`, `S = (L - R) / 2`). Sum over frames per band, plus 1e-30.
   - `side_db` = `max(-40, 10 log10(sum PS / sum PM))` over all bands, 2 decimals.
   - Per band ratio `clip(10 log10(PS_band / PM_band), -40, 10)`.
   - Per band level `10 log10(PM_band)`.
5. Weights from the original's band levels: `w = clip((level - max level + 40) / 40, 0, 1)`.
6. `width_db = sum(w x min(|ratio_a - ratio_b|, 30)) / max(sum w, 1e-9)`, 3 decimals.
7. Tail: `tail_span` of the original (from 30 ms after the original's loudest 10 ms to the end of its tail; null
   when shorter than 20 ms). `tail_corr` = the L/R correlation of each sound over that span.
   `tail_corr_err = |a - b|`, or 0 when either is null.

### Score

```
score = mel_db
      + 0.3 x tone_db
      + 8 x (1 - env_corr)
      + 1 x min(onset_err, 2)
      + 1.5 x excess_db
      + 1 x missing_db
      + 0.04 x min(width_db, 20)
      + 1 x tail_corr_err
      + 20 x (1 - clap_cos)          only when clap_cos is present and not null
```

Missing optional fields count as 0. For a mono pair the stereo terms are 0.

Measured reasons for the weights (facts from the source's evaluation):
- With the excess and missing terms, the model-free score chose fits with a mean CLAP of 0.927 on 12 reference
  recordings. Without them it chose 0.856, and the worst choice was 0.52. The best candidates averaged 0.944.
- The stereo terms cost a mono re-make of a wide recording (width error 15 dB, tail correlation off by 0.6) about
  1.2, which is what 0.06 of CLAP costs. They break ties between candidates that sound alike. They do not override a
  clearly closer candidate.
- The CLAP-including choice agreed with the model-free choice on 11 of 12.

### Floor

The floor is the quality of render seed 11 against render seed 12 of the same model. A model with noise in it cannot
beat its own floor on `mel_db`. The report and the bank's grade use "mel gap" = render `mel_db` minus floor
`mel_db`.

### CLAP and describe

- `clap_cos`: embed the mono float32 of both sounds with the CLAP audio model of the listen unit (one batch). The
  embeddings are unit vectors, so the dot product is the cosine. Same-category real recordings sit at 0.4 to 0.95 to
  each other.
- `describe_pair`: write both sounds to temporary 48 kHz float WAV files, run the listen unit's `describe` on each,
  and keep the top tag and the top AudioSet label of each.
- Both need the optional ML extras (listen unit). When they are absent, the listen unit's install prompt applies.

### Text line

`text(q, fl)` gives one line. It holds: mel dB (with floor), env r (3 decimals), tone dB, onset error in octaves with
both sharpness values, excess and missing, both t40 values, level with sign. When the larger `side_db` is above -40,
it adds both side values and the width error, and both tail correlations when both exist. It adds the CLAP cosine
when present and both describe tags when present.

### Pair picture

- Columns: original, resynthesis, and an optional third (`extra`). All are zero-padded to the longest.
- Top row: a spectrogram per column (FFT 1024, hop 128, centred Hann), one shared dB range from `top - 90` to `top`,
  log frequency axis 40 Hz to 22 kHz.
- Bottom row, left: the 5 ms envelopes of all columns on one axis (range: 80 dB under the top, plus 3 dB headroom,
  never below -120).
- Bottom row, right: the tone profiles of all columns on a log frequency axis.
- An optional title. When `q` is given, the text line is appended to the title.
- Figure size 4.4 in per column by 6.2 in, 110 dpi, PNG.

## Constants

| name | value | use |
|---|---|---|
| sample rate | 48000 Hz | every array |
| mel FFT sizes | 256, 1024, 4096 | `mel_db` |
| mel hop | n_fft / 4 | `mel_db` |
| mel bands | 64, 30 Hz to 20 kHz | `mel_db` |
| mel floor / active range | 80 dB / 60 dB under top | `mel_db` |
| onset frame | 1 ms (48 samples), 30 dB under max | alignment |
| alignment lead | 5 ms (240 samples) | alignment |
| level window | 50 ms RMS | levelling, `level_db` |
| envelope frame | 5 ms | `env_corr`, `t40` |
| envelope floor / active | 60 dB / 60 dB | `env_corr` |
| attack frame | 0.125 ms (6 samples) | `attack` |
| attack onset gate | 20 dB under max | `attack` |
| local peak search | 10 ms | `attack` |
| sharpness rise | 20 dB to 3 dB under peak, 17 dB span, min 0.02 ms | `attack` |
| t40 drop | 40 dB | `t40` |
| band-gram | FFT 1024, hop 512, 28 bands, lowest 3 merged to 22-112 Hz | `excess` |
| excess tolerance / depth | 6 dB / 100 dB, active within 90 dB | `excess` |
| side floor | -40 dB | `side_db`, width ratio |
| width ratio clip | -40 to +10 dB | width |
| width weight range | 40 dB | width |
| width difference cap | 30 dB | width |
| min stereo length | 64 samples | stereo |
| min tail span | 20 ms | tail correlation |
| tail start | 30 ms after the loudest 10 ms | tail correlation |
| floor seeds | 11, 12 | floor |
| score weights | tone 0.3, env 8, onset 1 (cap 2), excess 1.5, missing 1, width 0.04 (cap 20), tail 1, CLAP 20 | score |

## Errors and edge cases

- A silent or constant envelope gives `env_corr = 0`.
- A sound shorter than 4 attack frames gives sharpness 1.0 dB/ms. Two such sounds give `onset_err = 0`.
- A sound shorter than 50 ms is levelled by the mean power of the whole sound.
- `log2` of a sharpness ratio never sees 0, because the rise time is at least 0.02 ms (sharpness at most 850 dB/ms).
- A mono original against a stereo render still runs the stereo measures: the mono one is duplicated, so its side
  is -40 dB.
- NaN never appears in a dict: every dB uses a floor (1e-20 power, 1e-30 for the stereo sums).

## Acceptance tests (quality)

These are part of the index's acceptance list. Use synthetic signals made in the test.

| id | input | expected |
|---|---|---|
| Q-1 | Any sound against itself (a 0.3 s decaying 1 kHz sine plus a click) | `mel_db = 0`, `env_corr = 1` (to 4 decimals), `onset_err = 0`, `tone_db = 0`, `excess_db = missing_db = 0`, `level_db = 0`, `score` under 1e-6. |
| Q-2 | A sound against itself scaled by 0.5 | All levelled measures as in Q-1. `level_db = -6.02` within 0.01. |
| Q-3 | A sound against itself delayed by 20 ms | `mel_db` and `score` are larger than for Q-1 (the timing error counts). |
| Q-4 | A 0.5 s decaying tone against the same tone plus white noise 40 dB under its peak for the whole length | `excess_db > 0`, `missing_db = 0`. Reverse the pair: `missing_db > 0`, `excess_db = 0`. |
| Q-5 | A stereo decorrelated noise burst with a 0.5 s exponential tail against its mono sum duplicated to 2 channels | `side_db[1] = -40`, `side_db[0] > -20`, `width_db > 3`, `tail_corr[1]` near 1, `tail_corr_err > 0.3`. |
| Q-6 | Two mono sounds | `side_db = (-40, -40)`, `width_db = 0`, `tail_corr = (null, null)`, `tail_corr_err = 0`. |
| Q-7 | A step onset (rise in 1 sample) against the same sound with a 5 ms linear fade-in | The faded sound has lower sharpness. `onset_err > 1`. |
| Q-8 | `score` of a dict with only the required fields | Equals `mel + 0.3 tone + 8 (1 - env) + min(onset, 2)`. Adding `clap_cos = 0.9` adds exactly 2.0. |
| Q-9 | `floor(fn)` with `fn` that returns noise drawn from seed `s` | Calls `fn(11)` and `fn(12)`. `mel_db > 0`. |
