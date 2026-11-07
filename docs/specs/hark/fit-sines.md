# hark fit: sines fitter (part of fit.md)

This part specifies the sines fitter. It fits tonal and pitched sounds (dings, chimes, pitched pops, tonal impacts)
as tracked partials, strike atoms and third-octave residual noise. The index is `fit.md`.

Module: `hark.forge.fit.sines`. Pure numpy and scipy. The space module (`fit-space.md`) imports `band_masks` and
`finish` from here. The gesture fitter and the engine use `analyze`, `synth_*`, `side_model`, `morph_side`,
`vary_field`, `fade_in_s` and `hf_rolloff`.

Method family: analysis by synthesis in the sinusoidal-modelling tradition (peak picking, frame-to-frame partial
tracking, cubic phase resynthesis, a noise residual: McAulay-Quatieri and spectral modelling synthesis). No harmonic
series is forced. Inharmonic partials keep their own frequencies. Beating of close doublets lives in a partial's
amplitude trajectory.

## Interface

| function | contract |
|---|---|
| `fit(x, sr=48000, windows=(512, 1024, 2048, 4096), max_partials=160, dyn_db=80, keep_db=60, prom_db=4, max_peaks=100, min_ms=15, amp_tol_db=1, freq_tol_cents=1, res_range_db=140, transients=True, passes=2, snr_db=10, source=None)` | A sines model of `x` (a path or an array at `sr`, mono or `(n, 2)`). |
| `analyze(x, sr, W, onsets, width, opts)` | One window size. Returns `{'model', 'det', 'res', 'atoms', 'tracks'}`: the model, the phase-exact partial signal, `x - det`, the strike atom signal, and the internal tracks. The gesture fitter and the hybrid engine use it. |
| `onsets(x, sr)` | The strike list `[{'t', 'rise', 'strength'}]` (see Algorithms). |
| `render(model, seed=0, knobs=None, **kw)` | float64 at 48 kHz, mono unless `stereo=True` (then `(n, 2)`). |
| `summary(model)`, `describe(model)` | Numbers and one line of words about the model. |
| `partial_freq(p)` | A partial's energy-weighted mean frequency. |
| `side_model(st, duration=None, bands=SIDE_BANDS, tol=0.04)` | The fitted side of a stereo source. |
| `morph_side(sa, sb, w, map_a=None, map_b=None)` | The side of a morph. |
| `vary_field(r, parts=6)` | A smooth random function of frequency. |
| `fade_in_s(model)` | The fade-in length of a render. |
| `hf_rolloff(x, sr=48000, points=HF_POINTS)` | The roll-off above 16 kHz. |
| `band_masks(centers, nfft, sr=48000)` | Log-frequency triangles that sum to 1. |
| `synth_partials(partials, n, sr, cents=None)`, `synth_atoms(transients, n, sr)`, `synth_noise(bands, times, levels, n, seed=0, sr=48000, pad=4096, hf=None)` | The synthesis kernels. |
| `finish(y, sr=48000, fade_s=0.002, peak_db=-1, limit=True)` | Fade-in and peak gain. |
| `to_code(model, name='sound')` | Standalone numpy source that renders the model at its identity. |
| `save(model, path)`, `load(path)` | Compact JSON. `load` raises `ValueError` when the kind is not `sines`. |

Internal helper names are free. The engine and the gesture fitter need `analyze` and `onsets` as public
functions.

### Model (plain JSON)

| field | type | meaning |
|---|---|---|
| `kind` | `"sines"` | |
| `version` | 1 | |
| `sr` | 48000 | |
| `duration` | s, 5 decimals | Length of the fitted signal. |
| `onset` | s | Time of the first strike, 0 when there is none. |
| `onsets` | [s] | All strike times. |
| `peak_db` | dB | Peak sample of the mono signal. |
| `width` | dB, 1 decimal | Whole-sound side / mid energy of the source; -120 for mono. |
| `partials` | list | Strongest first (by peak breakpoint level). Each: `t0` (birth, s), `rise` (s, at least 0.0002), `t1` (time of the last breakpoint, s), `rel` (release, s), `ph` (phase at `t0`, rad, wrapped to (-pi, pi]), `freq` (`[[t, Hz], ...]`), `amp` (`[[t, dBFS], ...]`). |
| `transients` | list | One entry per strike that has atoms: `t` (start, s) and `atoms` (`[[Hz, tau s, amplitude, phase rad], ...]`). |
| `residual` | object | `bands` (third-octave centres, Hz), `times` (s), `db` (per band a list of dB per time; -150 means off), `hf` (`[[Hz, dB re 16 kHz], ...]`). |
| `side` | object or absent | For a stereo source: `bands` (octave centres), `times`, `pan` (per band a signed gain per time), `diffuse` (per band a gain per time). |
| `coherent` | true | Marks a model made by this version (see Vary and Finish). |
| `analysis` | object | `window`, `hop`, `sines_share_db`, `residual_db`, `lsd_db`, `tried` (window size as string -> distance), `onset_rise_ms`. |
| `source` | string | The file name, when known. |
| `summary` | object | See Summary. |
| `layer` | `"deterministic"` | Only on a hybrid's sines layer (set by the engine). |

## Behaviour

1. Load the input. A stereo input is split into mid (channel mean) and the `(n, 2)` source. The whole-sound width is
   `10 log10(sum S^2 / sum M^2)` with `S = (L - R)/2`, floored at -120. Other rates are resampled to 48 kHz with a
   polyphase resampler.
2. Detect strikes on the mono signal once.
3. For each window size in 512, 1024, 2048, 4096: skip it when it is more than 2 x the signal length and a model
   exists already. Otherwise fit a model at that window and render it at seed 0 with default knobs. Measure the
   multi-resolution log-spectral distance (LSD) to the signal. Keep the model with the lowest distance.
4. For a stereo source with width above -60 dB, add the fitted `side`.
5. Set `coherent`, the analysis fields, `source` and `summary`.

## Algorithms

### Band masks

For an FFT of size `nfft`, frequencies `f` of the rfft bins, and band centres `c_b`: mask `b` is the linear
interpolation, over `log2(max(f, 1))`, of the unit vector `e_b` at the points `log2(c_b)`. Outside the end centres
the interpolation holds the end value, so the first band is flat to DC and the last band is flat to Nyquist. The
masks sum to 1 at every frequency.

### Residual band centres

`50 x 2^(k/3)` for k = 0 .. round(3 log2(16000/50)) = 25. That is 26 bands, 50 Hz to 16 kHz.

### Framing for peaks

- Hop `max(32, W/8)`. Frame count `n // hop + 2`. Pad the signal with `W/2` zeros in front and `W/2 + 2 hop` behind.
- Window: Blackman of length W (periodic form as scipy's `get_window('blackman', W)`).
- Zero phase and x2 zero padding: FFT size `N = 2W`. Place the second half of the windowed frame at the start of the
  buffer and the first half at the end.
- Amplitude normaliser: the window sum.

### Peak picking (per frame)

Inputs: magnitude dB per bin, a dynamic range `dyn_db` (80), prominence `prom_db` (4 at fit time), at most
`max_peaks` (100) per frame, minimum frequency 30 Hz.

1. Local level: power averaged over 33 bins (uniform filter, edge mode nearest), in dB.
2. A bin is a peak when it is greater than the bin below and not less than the bin above.
3. Keep a peak when it is above the global reference minus `dyn_db` (the global reference is the loudest bin of the
   whole STFT, or the original's loudest bin for a residual pass), above its own frame's loudest bin minus `dyn_db`,
   and above its local level plus `prom_db`.
4. Drop bins under `ceil(30 N / sr)` and the 2 top bins.
5. Local noise floor per frame: the median of each block of 32 bins in dB, interpolated across bins at the block
   centres.
6. When more than `max_peaks` peaks remain, keep the loudest ones.
7. For each peak with dB values a, c, e at bins b-1, b, b+1: offset `p = 0.5 (a - e) / (a - 2c + e)`, clipped to
   [-0.5, 0.5] (0 when the denominator is under 1e-12). Frequency `(b + p) sr / N`. Amplitude
   `10^((c - 0.25 (a - e) p)/20) x 2 / window_sum`. Phase: the bin phase plus `|p|` times the wrapped phase step to
   the neighbour bin on the side of `p`.
8. Each peak also carries its contrast: its dB minus the local noise floor at its bin.

### Partial tracking

Parameters: maximum gap 2 frames, relative tolerance 0.025, minimum tolerance `1.5 sr / W` Hz.

1. Each active track predicts `f_last + slope x (k - k_last)`.
2. Tolerance per track: `max(1.5 sr / W, 0.025 x prediction)`.
3. Cost of a peak for a track: `|f - prediction| / tolerance + 0.01 x |dB(peak) - dB(last amplitude)|`. Pairs
   outside the tolerance are impossible.
4. Assign greedily in rising cost order, each track and each peak at most once.
5. Slope update: when the track has 2 or more points or the gap is more than 1 frame, the new slope is
   `(f - f_last) / gap`. With 2 or more points the slope becomes the mean of the old slope and the new one. Otherwise
   it is replaced.
6. A track that gets no peak and whose last frame is more than 2 frames back ends.
7. Every unused peak starts a new track with slope 0.
8. After the last frame, fill each track's gaps: frequency, amplitude and contrast by linear interpolation between
   frames, phase by integrating the mean of the 2 neighbouring frequencies over one hop.

### Track filters

- Minimum length: `max(3, ceil(max(min_ms/1000, 1.25 W / sr) x sr / hop))` frames. Overlapping frames keep a noise
  peak alive for about one window, so a partial must outlive 1.25 windows.
- Peak amplitude above the strongest track's peak minus `keep_db` (60 dB).
- Sinusoidality: the energy-weighted mean contrast `sum(a^2 c) / sum(a^2)` is at least `snr_db` (10 dB). A noise peak
  stands only 5 to 9 dB over the median of its neighbourhood. A real partial stands 15 to 50 dB over it.
- Keep the `max_partials` (160) strongest by total energy `sum a^2`.

### Strike detection

1. Octave band centres 150 Hz to 12 kHz (`150 x 2^k`, 7 bands).
2. Band envelopes at 1 ms steps, with smoothing at most 8 ms (see Band envelope).
3. Floor all at `top - 70`. Total level per frame = dB of the sum of band powers.
4. Rise per band: the envelope minus its minimum over the current and previous 4 frames (the first frame is repeated
   for padding), at least 0.
5. Loud cells: above `top - 60`. Onset function: the mean rise over loud cells per frame.
6. Zero the function where the total is under `total max - 45`, and where fewer than 3 bands rise by more than 6 dB
   (a strike covers many bands, a partial that beats does not).
7. Peaks of the function above 6 dB with a minimum distance of 25 frames (25 ms). Add the first frame whose total is
   above `total max - 25` (the first onset) as a candidate.
8. Timing per candidate on the causal peak envelope (the maximum `|x|` over the trailing 2.5 ms, in dB, floor
   -240 dB):
   - Look at the window from 25 ms before to 30 ms after the candidate.
   - Peak: the loudest point from 8 ms before the candidate on.
   - Base: the larger of the minimum before the peak and `peak - 70`.
   - Skip the candidate when `peak - base < 3 dB`, unless it is the first onset.
   - Start: from the minimum point before the peak, the first point at or above
     `max(base + 0.25 (peak - base), peak - 30)`.
   - Rise end: the first point after the start at or above `peak - 1 dB`.
   - Strike: `t` = start, `rise` = max(0.3 ms, rise end - start), `strength` = the onset function value (or
     `peak - base` for the added first onset).
9. Merge strikes closer than 25 ms. Keep the stronger.

### Band envelope

For each band of `band_masks(centres)`:
1. Band-pass the signal through the mask in one big FFT with 4096 zeros of padding on each side.
2. Window length `L = clip(4 / bandwidth, 0.5 ms, max_smooth) x sr` samples, at least 2. Bandwidth is
   `centre x (2^(1/3) - 2^(-1/3))`. `max_smooth` is 30 ms by default.
3. At each sample, the power is the minimum of the trailing mean over `L` samples and the leading mean over `L`
   samples. Taking the minimum keeps the edges sharp, so a strike is not smeared backwards.
4. Sample every step (1 ms) and convert to dB (floor 1e-24).

### Split and fix onsets

With a long analysis window, a strike appears up to half a window earlier than it is. With `half = W / (2 sr)`:

1. Split: a track that crosses a strike (it starts before `t - 0.5 half` and lasts past `t + 0.55 half`) and whose
   loudest level in the 2 half windows after `t + 0.55 half` is more than 10 dB above its loudest level in the zone
   from `t - 2 half` to `t - 0.5 half` is cut in 2. The part before is kept only when it has at least 3 frames.
2. Drop pre-echo tracks: a track that has at least 90 % of its energy inside a smear zone
   `[t - 1.1 half, t + 0.55 half]` of a strike and ends inside that zone.
3. For each remaining track born between `t - 0.25 half` and `t + 1.05 half` of a strike (take the strongest such
   strike):
   - Clean frames start at `t + 0.55 half`. Fit a line in dB through the clean frames up to
     `t + 0.55 half + max(4 hops, half)`.
   - With 2 or more such frames: start level `a0 = clip(intercept at t, max - 40, max + 6)`. Replace the smeared frames
     (before `t + 0.55 half`) by `min(line, max + 6)`.
   - With 1 clean frame: `a0` = its level, and the smeared frames take it.
   - With none: `a0` = the track maximum, and all frames take it.
   - Drop the frames at or before `t`. When none remain, keep the last frame.
   - The track gets `t0 = t`, `rise` = the strike's rise, and `a0`.

### Phase-accurate resynthesis (for the residual only)

For consecutive frames of a track, with hop H, phases th0, th1 and radian frequencies w0, w1:
- `M = round(((th0 + w0 H - th1) + (w1 - w0) H / 2) / (2 pi))`
- `e = th1 - th0 - w0 H + 2 pi M`
- `alpha = 3 e / H^2 - (w1 - w0) / H`, `beta = -2 e / H^3 + (w1 - w0) / H^2`
- phase `theta(t) = th0 + w0 t + alpha t^2 + beta t^3`, amplitude linear between frames, for t = 0 .. H-1.

Head: a track with `t0` rises from 0 at `t0` to `a0` at `t0 + rise` (not past the first frame) and to the first
frame's amplitude at the first frame, with phase extrapolated back from the first frame at constant frequency. A track
without `t0` rises linearly over one hop before its first frame. Tail: one hop of linear release at constant
frequency.

### Second pass

`passes` is 2. After the first set of tracks:
1. The free room is `max_partials - tracks`. Stop when it is 0.
2. Frame the residual, pick peaks against the original's global reference level, track, filter as above, keep the
   strongest `room`, and fix their onsets.
3. Synthesize them. Keep them only when they lower the residual energy below 98 % of what it was.

### Strike atoms (transients)

At each strike (the 16 strongest at most), for the residual after the partials:
1. Split the residual at 400 Hz with a 4th-order Butterworth low-pass run forward and backward (`lo`) and its
   complement (`hi`).
2. Segment start: 0.5 ms before the strike.
3. High band: an 8 ms segment at full rate, at most 24 atoms, each pole's decay clamped so that its T60 is at most
   20 ms, ridge 1e-4. Keep atoms at or above 240 Hz.
4. Low band: a 64 ms segment decimated by 8 (plain sample skip), at most 6 atoms, only poles whose T60 is within
   96 ms (others dropped), ridge 1e-3. Keep atoms from 40 Hz to 600 Hz.
5. Skip a band when its segment is shorter than 32 samples or its peak is more than 40 dB under the signal's peak.
6. Keep a band's atoms only when they explain the strike: over the segment window x 2 + 20 ms, the energy of
   (band - atoms) must be under 70 % of the band's energy.

Matrix pencil (damped sinusoid estimation):
1. Segment length L (at least 16 and not all zero). Pencil parameter `P = L // 3`. Build the Hankel matrix of
   sliding windows of length P + 1.
2. SVD. Model order: the number of singular values that hold 99.8 % of the energy, plus 1, at most 2 x max atoms
   and at most the rank minus 1.
3. Poles: eigenvalues of `pinv(V_top) V_bottom` from the leading right singular vectors. Keep one of each conjugate
   pair (angle at least 0).
4. Decay limit: `|z|max = 10^(-3 / (max_t60 sr))`. Clamp (scale to that radius) or drop poles above it. Drop poles
   with `|z| < 0.001`.
5. Remove near-duplicate poles (angle within 1e-4 and radius within 1e-3).
6. Least squares with a small ridge `lambda = ridge x trace(B^T B) / columns`: a real pole has 1 column
   `r^k cos(wk)`, a complex pole has 2 columns `r^k cos(wk)` and `r^k sin(wk)`.
7. Atom: frequency `w sr / (2 pi)`, `tau = -1 / (sr ln r)`, amplitude `hypot(c, s)`, phase `atan2(-s, c)`. A real
   pole with a negative coefficient gets phase pi.
8. Order by `amplitude^2 x tau` falling. Keep the max atoms.

Atoms are rounded: Hz 2 decimals, tau and amplitude 7 decimals, phase 4 decimals. Strikes are sorted by time.

### Residual model

The residual is the signal minus the partials minus the atoms.

1. Band envelopes (26 third-octave bands, 1 ms steps, smoothing up to 30 ms).
2. Noise share per band and time (dB, at most 0): the noise part of a band. Purpose: leftover partial energy must
   not become hiss.
   - STFT of the residual with a 2048 window and a 240 hop (zero boundary).
   - Whiten the power by a running median over 31 bins across frequency.
   - For each band, take the bins between the midpoints to its neighbour centres in log frequency. With fewer than
     5 bins the share is 0 dB.
   - Share = `10 log10(clip(median / ln 2 / mean, 1e-4, 1))`. For pure noise the median of an exponential power is
     `mean x ln 2`, so the share is 0 dB. Narrow peaks raise the mean but not the median.
   - Median filter over 5 frames. Interpolate to the 1 ms times.
   - From 2 ms before to 30 ms after each strike, the share is 0 dB: the attack burst is noise.
3. Add the share to the envelope. Median filter over 5 steps.
4. Add the envelope bias per band. Bias calibration: band envelopes of 2 s of white noise from a fixed generator
   seed (12345), median filtered over 5, compared with the true band power. The bias is the true level minus the
   mean envelope over the middle 80 %. It is computed once per band set and cached.
5. Floor at `peak_db - res_range_db` (140 dB under the peak).
6. Live bands: maximum above the floor + 1 dB. Choose one time grid for all live bands with a multi-row simplifier
   (tolerance 2 dB, at most 96 times).
7. Store levels at 1 decimal. A level within 0.05 dB of the floor becomes -150.
8. `hf`: the roll-off of the residual above 16 kHz (below).

### HF roll-off

Grid: 16, 17, ..., 24 kHz.
1. Welch spectrum of the mono signal, segment `min(4096, largest power of 2 not above max(len, 256))`.
2. At each grid point, the mean power from `p 2^(-1/12)` to `min(p 2^(1/12), sr/2)` in dB (-300 when no bin).
3. Relative to the 16 kHz value. Make it monotone falling (a running minimum, starting at 0). Floor at -80.
   1 decimal.

Purpose: the highest residual band stays level up to Nyquist. With no `hf`, a fit adds noise above 20 kHz that a
band-limited recording does not have (on one click, -19 dB of the energy was above 20 kHz, against -34 dB in the
recording). With `hf` the
6 sines reference fits moved toward the recording (click -19.5 to -30.0 dB, recording -33.5) and `mel_db` changed by
-0.10 to 0.00.

### Simplifier (breakpoints)

Ramer-Douglas-Peucker, where the tolerance is measured on the value axis:
1. Keep the first and last point. For a span (i, j), find the inner point with the largest
   `|v - line| / tol` (tolerance per point allowed). When it is above 1, keep it and split.
2. When more than `max_pts` points remain, multiply the tolerance by 1.4 and repeat.

The multi-row version uses the maximum error over all rows, a scalar tolerance, and a growth factor of 1.3.

### Partial breakpoints

Per track:
1. Times `k x hop / sr`. Amplitude in dB. Frequency median filtered over 5 frames when the track has 5 or more.
2. With `t0`: prepend the point `(t0, dB(a0))` to the amplitude and `(t0, f_first)` to the frequency. Phase at t0:
   the first frame's phase minus `2 pi f_first (time_first - t0)`.
3. Without `t0`: `t0` is one hop before the first frame, rise is one hop, the prepended amplitude is the first
   frame's, and the phase is moved back by one hop.
4. Amplitude breakpoints: simplifier with tolerance `amp_tol_db` (1 dB), at most 64 points.
5. Frequency breakpoints, in cents (`1200 log2(max(f, 1))`): the tolerance widens with depth under the partial's
   peak, `freq_tol_cents x 2^clip((peak - level)/20, 0, 4)`, at most 48 points.
6. `rel` is one hop. `t1` is the last frame time.
7. Rounding: times 5 decimals, Hz 2 decimals, dB 1 decimal, phase 3 decimals.
8. Order by peak amplitude breakpoint, strongest first.

### Log-spectral distance (window choice)

For each size in 256, 1024, 4096 that fits the signal: STFT magnitude in dB with a hop of a quarter size, floor at
`max - 60`, active where either is above the floor, mean of `min(|a - b|, 40)` over active cells. The distance is
the mean over sizes, 0 when none applies.

### Side model (stereo)

1. Octave bands 125 Hz to 16 kHz (8 bands). Mid, side and real cross power per band and frame from the space module
   (`side_powers`, FFT 1024, hop 240).
2. Keyframe times: 0, onset - 10 ms, onset, then steps after the onset: 10 ms steps up to 0.1 s, 20 ms up to 0.3 s,
   40 ms up to 0.7 s, 80 ms after that, and the end. The onset is the start of the space module's active span.
   Clip to the duration and drop duplicates.
3. For each keyframe, weight the frame powers with a triangle that spans from the keyframe before to the keyframe after, and sum (first and last use
   10 ms beyond). When no frame falls under the tent, use the nearest frame.
4. Gains from summed mid power PM, side power PS and cross power C:
   - `pan = clip(C / PM, -1.5, 1.5)`: the part of the side coherent with the mid, signed. A pan has
     `L = M (1 + g)`, `R = M (1 - g)`.
   - `diffuse = clip(sqrt(max(PS - C^2/PM, 0) / PM), 0, 2)`: the rest of the side relative to the mid.
5. Quiet keyframes (more than 50 dB under the band's loudest keyframe) take the band's whole-sound gains. A band with
   under 1e-6 of the loudest band's total takes the whole sound's gains (all bands summed) at every keyframe.
6. Drop keyframes with the multi-row simplifier over pan and diffuse together, tolerance 0.04.
7. Round gains to 3 decimals.

Measured effect (fact): on 42 wide or decorrelated stereo targets, sines fits alone, the side / mid error fell from
13.4 dB median to 0.15 dB, and the tail L/R correlation error from 0.39 to 0.04.

### Rendering

Knobs and defaults:

| knob | default | effect |
|---|---|---|
| `transpose` | 0 | Semitones. Every partial and the residual spectrum move together. |
| `stretch` | 1 | Time factor about the first strike `on`: `T(t) = on + (t - on) x stretch`. Rises keep their length. |
| `decay` | 1 | Factor on each partial's amplitude breakpoint times after its peak, about the peak. Frequency times stay. |
| `brightness` | 0 | dB per octave about the partials' centroid. |
| `vibrato` | 0 | Cents. |
| `vibrato_rate` | 5.5 | Hz. |
| `drift` | 0 | Cents of slow random wander. |
| `residual` | 1 | Gain on the residual noise. |
| `transient` | 1 | Gain on the strike atoms, and on the noise during the 15 ms that follow each strike. |
| `vary` | 0 | 0 to 1. Seeded jitter for a new take. |
| `partials` | null | Keep the N strongest partials. |
| `inharm` | 0 | Partial ratio stretch: `f / f_low -> (f / f_low)^(1 + inharm)`. |
| `phases` | `'fit'` | `'fit'` or `'random'`. |
| `gain_db` | 0 | Output gain. |
| `stereo` | false | true returns `(n, 2)` with the fitted side. |
| `width` | 1 | Factor on the side. |
| `ring` | null | null means true when `vary > 0`. |

An unknown knob raises `ValueError` naming the knobs. A model of another kind raises `ValueError`.

Steps:
1. Jitter generator: seeded `seed x 7919 + 17` (masked to 32 bits). The residual noise uses `seed` itself.
2. `ratio = 2^(transpose/12)`. `f_ref` = the summary centroid (1000 when absent). `f_low` = the summary's lowest
   strong partial (100 when absent).
3. Identity: stretch 1, decay 1, ratio 1, vary 0, brightness 0, inharm 0, phases `'fit'`. In identity the partials,
   atoms and residual are used as stored.
4. Coherent jitter: when the model has `coherent` and `vary > 0`, draw 3 vary fields (frequency, level, decay) in
   that order before the partial loop.
5. Per partial (not identity):
   - `fmed` = its energy-weighted mean frequency. `fac = ratio x (fmed / f_low)^inharm`.
   - Coherent: detune `8 vary x field_f(fmed)` cents, level jitter `2 vary x field_l(fmed)` dB, decay jitter
     `exp(0.15 vary x field_d(fmed))`. Not coherent: normal draws with those standard deviations, one partial at a
     time.
   - Phase: the stored phase, or a uniform draw in (-pi, pi) for random phases.
   - Frequencies times `fac x 2^(detune/1200)`.
   - Levels plus jitter plus `brightness x log2(max(fmed x ratio, 1) / f_ref)`.
   - Decay: amplitude breakpoint times after the peak breakpoint move to `tp + (t - tp) x decay x jitter`. `t1`
     moves the same way when it is after the peak.
   - Then all times go through `T`. `rel` is multiplied by `max(1, stretch)`.
6. Residual (when not identity or `residual != 1`):
   - Transpose: shift each time column's band levels along log2 frequency by `log2(ratio)` (linear interpolation, end
     values held).
   - Brightness: add `brightness x log2(band x ratio / f_ref)`.
   - Vary: add one normal draw per band, standard deviation `1.5 vary` dB.
   - Times after `on + 10 ms` are stretched by `decay` about that pivot, then all times go through `T`.
   - Residual gain: add `20 log10(max(residual, 1e-6))`.
   - Bands that were off (-149 or under) stay at -150.
7. Length: `n_id = round(duration x sr)`. When decay > 1, stretch != 1 or ring, `n = ceil(max(partial ends,
   T(duration)) x sr) + 1`.
8. Ring: extend `n` to the later of the last residual time and `T(duration)`, plus 60 ms, and append a residual
   keyframe there at -150 dB for every band. The noise thus fades out linear in dB over 60 ms.
9. Pitch bend curve, when vibrato or drift is set: `vibrato x sin(2 pi rate t + random phase)`, faded in linearly
   over 150 ms from the onset, plus `drift x` a slow wander (4 sines with rates `1.5 x U(0.2, 1)` Hz and random
   phases, divided by 2).
10. Partials: `synth_partials`.
11. Atoms (when not identity or transient != 1): times through `T`, frequency times `ratio` and the detune,
    `tau / ratio`, amplitude times `transient` and `10^((gain_jitter + brightness x log2(max(f ratio, 20)/f_ref))/20)`.
    Coherent: one detune (sd `30 vary` cents) and one gain (sd `2 vary` dB) per strike for all its atoms. Not coherent:
    one draw per atom. Reason: the atoms of one strike are fitted together, and their onset steps cancel each other. With separate
    jitter per atom, the takes had clicks.
12. Noise: `synth_noise` with the residual bands, times, levels, `seed` and `hf`.
13. Transient knob on noise: a weight that rises from 0 at the strike to 1 at 7.5 ms and falls to 0 at 20 ms
    (triangle of half-width 12.5 ms centred 7.5 ms after the strike, only from 2 ms before the strike). The noise is
    multiplied by `1 + (transient - 1) x weight`.
14. Output gain.
15. Stereo:
    - With `side`: keyframe times after `on + 10 ms` stretched by decay, then through `T`. A sign from a separate
      generator (`seed x 31 + 7`): -1 when its first uniform draw is under 0.5. Side =
      `shaped_side(mono, side bands x ratio, times, pan, diffuse, sign)` (space module).
    - Without `side` (old model): a fresh residual noise draw at seed `seed + 1000003` times `10^(width/20)`, times
      the output gain.
    - Side times `width`. Output `[mono + side, mono - side]`.
16. Finish with the model's fade-in and limit flag.

### Synthesis kernels

`synth_partials`: for each partial, samples from `floor(t0 sr)` to `ceil((t1 + rel) sr) + 1` (clipped to n). Frequency
is linear in log2 between frequency breakpoints, amplitude linear in dB between amplitude breakpoints (end values
held). Optional bend: frequency times `2^(cents/1200)`. Amplitude times a linear rise from t0 over `rise` and a
linear fall over `rel` ending at `t1 + rel`. Amplitude is 0 where the frequency is at or above 0.49 sr. Phase:
`ph + 2 pi (cumsum(f) - f_first) / sr + 2 pi f_first (i0/sr - t0)`, so the phase at t0 is `ph`.

`synth_atoms`: each atom starts at the strike sample `round(t sr)` at full amplitude:
`a exp(-k / (tau sr)) cos(2 pi f k / sr + ph)` for k up to `7 tau sr`.

`synth_noise`:
1. Generator seeded with `seed` (32-bit mask). FFT size: the next power of 2 at or above `n + 2 pad` (pad 4096).
   One white normal draw of that length, FFT.
2. Masks: `band_masks(bands)`. With `hf`, multiply each mask above the first `hf` frequency by
   `10^(interp(f, hf)/20)`. A per-band keep factor `sqrt(sum shaped^2 / sum mask^2)` keeps the band's level under
   16 kHz as fitted, so the roll-off removes energy instead of re-levelling the band.
3. Per band with a maximum level at or above -140: inverse FFT of spectrum times mask, take samples
   `[pad, pad + n)`, normalise to RMS `keep`, multiply by `10^(interp(t, times, levels)/20)`. Sum.

### Finish

1. Fade-in: a half-cosine power ramp `sin^2(pi k / (2K))` over `K = round(fade_s sr)` samples, when K > 1.
2. When `limit` and the peak is over `10^(peak_db/20)`, scale the whole signal down to it (a gain, no limiter).

`fade_in_s(model)` is 0 for a model without `coherent` (it renders as before). Otherwise it is
`clip(onset - 0.5 ms, 0.25 ms, 2 ms)`: shorter when the first strike is near the start, so a recording that starts inside its
attack keeps that attack. The limit applies when the model has `coherent` and is not a hybrid's deterministic layer.

Measured effect (fact): on 78 dev targets, median CLAP changed by -0.0002, renders over -1 dBFS fell from 7 to 0,
renders that did not start at zero fell from 44 to 0.

### Vary field

Draw 6 rates uniform in [0.25, 1.25] cycles per octave, then 6 phases uniform in [0, 2 pi). The field is
`sqrt(2/6) x sum_i cos(2 pi log2(max(f, 20)) c_i + p_i)`: unit variance, smooth in log frequency. Neighbouring
partials move together and a harmonic series keeps its ratios.

### Summary

- `partials`: count (0 alone when there are none).
- Peak per partial = its loudest amplitude breakpoint. `fm` = energy-weighted mean frequency. Life = `t1 - t0`.
- Strong partials: peak within 30 dB of the loudest and life at least `min(0.1, 0.5 x longest life)`. When none
  passes, peak within 30 dB alone.
- `f_low`: the lowest strong `fm`. `centroid`: the power-weighted geometric mean of `fm`.
- `f_range`: min and max `fm`.
- `top`: the 8 loudest, each `f`, `db` and `t60` (from a line fit over the amplitude breakpoints from the peak on;
  null when under 2 points, under 10 ms, or slope not under -0.5 dB/s; else `-60 / slope`).
- `ratios`: the 12 lowest ratios of strong `fm` to `f_low`.
- `inharmonicity`: median over strong partials of `|r - round(r)| / max(round(r), 1)` (0 with one strong partial).
- `glide_cents`: for the first 8 partials, `1200 log2(last freq / first freq)`.

`describe(model)` is one line: strike time and count, partial count and range, lowest strong, a word (harmonic under
0.01, slightly inharmonic under 0.04, else inharmonic), the first 5 ratios, the centroid, the T60 range, the largest
glide when above 50 cents, the residual share, the window and the LSD. Without partials it says the model is noise
only.

### Standalone code

`to_code(model, name)` writes a numpy-only Python module that renders the model at its identity: constants (sample
rate, duration, partial tuples, strike tuples, bands, hf, times, levels, limit flag, fade length), its own copies of
the 5 kernels (band masks, partials, atoms, noise, finish) and `render(seed)`. Run as a script, it writes a 16-bit
mono WAV (argument 1: path, argument 2: seed). Our implementation writes its own kernels into that file. It must not
import hark.

## Constants

| name | value |
|---|---|
| window sizes | 512, 1024, 2048, 4096 |
| hop | `max(32, W/8)` |
| peak range / prominence / max peaks / min Hz | 80 dB / 4 dB (fit default) / 100 / 30 Hz |
| local level filter | 33 bins |
| floor median block | 32 bins |
| tracking gap / rel tol / min tol | 2 frames / 0.025 / `1.5 sr/W` |
| track min length | max(15 ms, 1.25 windows), at least 3 frames |
| keep range / sinusoidality | 60 dB / 10 dB |
| max partials | 160 |
| passes / residual pass gain | 2 / 98 % |
| strike bands | octaves 150 Hz to 12 kHz |
| strike threshold / min gap / min bands | 6 dB / 25 ms / 3 |
| peak envelope hold | 2.5 ms |
| onset smear zone | `[t - 1.1 half, t + 0.55 half]`, jump 10 dB |
| atom split | 400 Hz, Butterworth order 4, zero phase |
| high atoms | 8 ms, 24 atoms, T60 <= 20 ms (clamped), ridge 1e-4, >= 240 Hz |
| low atoms | 64 ms, decimate 8, 6 atoms, T60 <= 96 ms (dropped), ridge 1e-3, 40-600 Hz |
| atom gate | 40 dB under peak, 16 strikes, energy drop to under 70 % |
| residual bands | 26 third-octaves, 50 Hz to 16 kHz |
| residual floor / grid tol / max times | 140 dB under peak / 2 dB / 96 |
| noise-share STFT | 2048 / 240, median 31 bins, 5 frames, attack -2 ms to +30 ms |
| bias calibration | 2 s white noise, seed 12345 |
| amp tol / max amp pts | 1 dB / 64 |
| freq tol / max freq pts | 1 cent, widening x2 per 20 dB, up to x16 / 48 |
| HF grid | 16 kHz to 24 kHz in 1 kHz steps, floor -80 dB |
| LSD sizes / range / cap | 256, 1024, 4096 / 60 dB / 40 dB |
| side bands | octaves 125 Hz to 16 kHz |
| side keyframe steps | 10 ms to 0.1 s, 20 ms to 0.3 s, 40 ms to 0.7 s, then 80 ms |
| side tolerance / quiet | 0.04 / 50 dB |
| side min width | -60 dB |
| fade-in | 2 ms, at least 0.25 ms |
| peak limit | -1 dBFS |
| ring fade | 60 ms |
| vary sd | detune 8 cents, level 2 dB, decay 0.15 (log), residual bands 1.5 dB, atoms 30 cents and 2 dB |
| vibrato fade-in | 150 ms |
| generator salts | jitter `seed x 7919 + 17`, side sign `seed x 31 + 7`, old side noise `seed + 1000003` |

## Errors and edge cases

- No tracks: the model has an empty `partials` list. `summary` is `{'partials': 0}`. The render is the atoms and
  the noise.
- No strikes: `onset` is 0 and `onsets` is empty. The fade-in is 0.25 ms.
- A signal shorter than a window: the larger windows are skipped after the first model.
- A band whose levels are all at -150 is silent in the render.
- A track that would have no frame after its strike keeps its last frame.
- `morph_side` of 2 sides with different bands raises `ValueError`.
