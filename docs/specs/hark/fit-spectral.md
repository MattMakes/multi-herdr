# hark fit: spectral fitter (part of fit.md)

This part specifies the spectral fitter. It fits noisy sounds (whooshes, swishes, cloth and paper swipes, risers,
downlifters, wind, shimmer beds, glitch textures) as a time-varying 48-band noise gesture with stereo, texture
statistics and tonal ridges. Every render draws fresh noise. The index is `fit.md`.

Module: `hark.forge.fit.spectral`. Pure numpy and scipy. FFT calls may use several workers. The worker count comes
from the environment variable `HARK_THREADS` (default 2, invalid values give 2).

## Interface

| function | contract |
|---|---|
| `fit(x, name=None, n_bands=48, tol_db=1.0, max_keys=160, gesture_ms=25, max_rank=16, iters=3, diff_frames=3, ridges=True, fine=False, texture=True, stereo=True, seeds=(1, 2), max_ridges=4, ridge_db=8, ridge_ms=50, ridge_lo=400, denoise=True, adapt=True, quiet_db=50, grain_norm_ms=0, grain_halves=False, verbose=False)` | A model of a path or a 48 kHz array (`(n,)` or `(n, 2)`). |
| `DETAIL` | The detail fit options: `gesture_ms=10, tol_db=0.5, max_keys=600, denoise=False, adapt=False, max_ridges=100, ridge_db=5, ridge_ms=20, ridge_lo=60`. |
| `render(model, seed=0, length=1, speed=1, sweep=1, shift=0, tilt=0, roughness=1, width=1, ridges=1, gain_db=0, doppler=0, proximity=0, pan=1, motion=0, level=True, limit=True, steady=True)` | `(n, 2)` float32 at 48 kHz. |
| `trajectory(model)` | The gesture's motion per keyframe (below). |
| `bend(y, t_pk, tau, semitones)` | A pass-by pitch bend as a time warp. |
| `pass_pan(y, t_pk, tau, amount)` | A pass-by pan. |
| `analyse(x, cents=None, stereo=True)` | `{'D': (B, T) band power density, 'stereo': group L/R/cross power or null, 'T'}`. |
| `centers(n=48)`, `warp(f)`, `unwarp(e)`, `weights(freqs, cents)`, `bandwidths(cents)`, `n_frames(n)` | The band axis. |
| `check(model)` | `ValueError` when not a dict of kind `spectral_noise` or version above 1. |
| `summary(model)` | One paragraph of words. |
| `to_code(model, var='MODEL')` | Readable Python that holds the model and renders it through `hark.forge.fit.spectral.render`. |
| `compare(a, b, clap=False)` | The fitter's own distance set (below). |
| `fit_report(x, clap=False, seeds=(11, 12), **fit_kw)` | Fit plus 2 renders: `{'model', 'renders', 'orig', 'fit', 'floor', 'seconds'}`. |
| `save(model, path)`, `load(path)` | Compact JSON. `load` runs `check`. |

### Model (plain JSON)

| field | meaning |
|---|---|
| `kind` | `"spectral_noise"` |
| `version` | 1 |
| `source` | File name or null. |
| `sr` | 48000 |
| `dur` | Length, s, 4 decimals. |
| `level_db` | The recording's loudest 50 ms RMS (channel-mean power), dB. |
| `bands` | `{'n': 48, 'f_lo': 20, 'f_hi': 20000, 'scale': 'ln(1+f/80)'}` |
| `fit` | `gesture_ms` (the smoothing chosen), `rank`, `tol_db`, `fluct_db`. |
| `env` | The gesture: `t` (keyframe times, s, 4 decimals) and `db` (per keyframe, 48 band power densities in dB relative to full-scale white noise, 1 decimal). Linear in dB between keyframes. |
| `stereo` | Null for mono. Else `groups` (0, 300, 1000, 2500, 5000, 10000, 24000 Hz), `ild` (per keyframe per group L/R level difference, dB, 2 decimals), `icc` (per keyframe per group inter-channel correlation, 3 decimals). |
| `fine` | Null, or `{'f0': 40, 'step': 1/24, 'db': [...]}`: a static fine spectral curve. |
| `texture` | Null, or `{'am', 'grains', 'stats'}`. |
| `ridges` | List of tonal ridges: `t` (s), `f` (Hz, 1 decimal), `db` (amplitude dB re full scale, 1 decimal), `jitter` (relative frequency jitter). |
| `finish` | true on every fit of this version. The engine then ends renders with its fade-in and peak gain. |
| `layer` | Set by the engine on a hybrid's or gesture's noise layer (`"air"`). |

Texture fields:
- `grains`: `density` (clicks per second), `spread` (1.0, log-normal sigma in nepers), `norm_ms` (only when above 0).
- `am`: `rates` (the 19 rate centres), `spec` (normalised rate shape), `depth` (per band group, std of log amplitude in
  nepers, at most 0.6), `coh` (coherence across groups, 0 to 1).
- `stats`: the measured statistics behind those choices, for reports: `lenv_real`, `lenv_gauss`, `fast_var_real`,
  `fast_var_gauss`, `fast_ratio`, `fast_ratio_halves`, `am_var_real`, `am_var_base`.

## Algorithms

### Band axis

- Warp `e(f) = ln(1 + f / 80)`. About 1/3 octave per band under 300 Hz and about 1/6 octave above 2 kHz.
- 48 centres equally spaced on the warp from 20 Hz to 20 kHz.
- Band weight on the warp axis (raised cosine, partition of unity above the first centre):
  - Rise into band b: from the previous centre (for band 0, from `ec_0 - (ec_1 - ec_0)`, so no shelf to DC),
    `sin^2(pi/2 x (e - lo) / (ec_b - lo))`.
  - Fall from band b to the next centre: `cos^2(pi/2 x (e - ec_b) / (ec_{b+1} - ec_b))`. The last band is 1 above its
    centre.
- Bandwidth of a band: the integral of its weight over a 96001-point grid from 0 to 24 kHz (cached).
- FFT size per band: `2^round(log2(2 x sr / bandwidth))`, clipped to 512 .. 4096.
- Band groups for stereo and texture: index of the group interval that holds the centre. Group centres
  `sqrt(max(lo, 50) x hi)`, the first set to 150 Hz. A band's group parameter is interpolated between the 2 nearest
  group centres in log2 frequency (end values held).

### Analysis

- Grid: hop 240 samples (5 ms). Frame count `n // 240 + 1`.
- STFT per size N: periodic Hann `0.5 - 0.5 cos(2 pi k / N)`, frame k centred at `k x 240` (N/2 zeros in front),
  scaled by `1 / sqrt(sum w^2)` so that white noise of variance s^2 gives `|X|^2 = s^2`.
- Mid density per band: for the bands that use size N, the frame power (mean of L and R powers for stereo) times the
  band weights normalised to sum 1 over bins. `D[b, k]` is power density per bin.
- Stereo, at size 1024: per group (bins in the group's interval), the summed L power, R power and real cross power
  `Re(XL conj(XR))`.
- Input loading: a mono input becomes 2 equal channels. The model is mono when both channels are equal (allclose).

### Ridges (tonal)

1. STFT of the mono mix, size 2048, on the 5 ms grid. Power dB.
2. Local floor on both sides: a median over 8 bins, read 6 bins below and 6 bins above, take the higher, plus 1.6 dB
   (an exponential variable's median is its mean minus ln 2 in log terms). With this floor, the edge of a
   band-limited noise does not count as a peak.
3. Active frames: total power within 50 dB of the loudest frame.
4. Per active frame: candidates in `[ridge_lo, 16 kHz]` that stand more than `ridge_db` over the floor and are a local
   maximum against 2 bins on each side. The 12 most prominent.
5. Parabolic peak on dB values (only when the curvature is negative). Join each peak to the live track with the
   nearest last frequency within 0.04 octave (each track once per frame). Else start a track.
6. A track ends after 2 frames without a peak (also over inactive frames).
7. Keep tracks with at least `max(2, round(ridge_ms / 5))` frames and a median prominence of at least `ridge_db + 2`.
   Order by total excess power. Keep `max_ridges`.
8. Ridge model: amplitude `A = sqrt(6 p / 2048)` with p = peak minus floor density (with a Hann window, a sinusoid of
   amplitude A gives a peak bin density of `A^2 N / 6`). Keyframes by the greedy keyframe choice on
   `[log2 f / 0.006, dB / 1.5]` with tolerance 1 and at most 60 keys. Jitter: the std of log2 f around its 5-frame
   smooth, times ln 2, clipped to [0.0005, 0.02].
9. Remove each ridge from the band densities: `D -= W(f) A^2 sr / (4 bw)`, at least 5 % of the old value.

### Gesture smoothing

- Start `k = round(gesture_ms / 5)` frames. Onset frame: the first frame whose total band power is within 30 dB of the
  maximum.
- Smoothing: a normalised Hann of `k + 1` points without its first zero, edge mode nearest, applied separately before
  and after the onset frame, so the attack stays sharp.
- Texture-aware widening (when `adapt`): `k_max = max(k, min(32, T // 6))`. Repeat: measure the wander of the
  smoothed total level (dB) around a running quadratic (Savitzky-Golay order 2, window
  `min(T or T - 1 to be odd, 4k + 1)`, needs at least 5) as the RMS over frames within 30 dB of the maximum and more
  than k frames from the onset (needs more than 4 frames). Stop when the wander is at most `2 tol_db` or `k = k_max`.
  Else `k = min(k_max, ceil(1.5 k))`.
- Target: smoothed density in dB, floored 90 dB under its top. Active cells: above `top - 70`.

### Low-rank denoise (when `denoise`)

1. Cell weights: band power `G = target + 10 log10(bw)`, weight `clip((G - (max G - 50)) / 25, 0, 1)`.
2. Noise level per band: analyse white noise of the same length (fixed generator seed 12345) with the same
   smoothing. The std of each band's dB over frames, skipping `nfft / 2 / hop` frames at each end when more than 8
   frames remain. The weighted RMS of that std is the noise level.
3. For rank K = 1 .. 16: weighted rank-K approximation (10 rounds: fill the low-weight cells with the current
   estimate, subtract the per-band mean, SVD, keep K components). Stop at the first K whose weighted RMS residual is
   at most the noise level.
4. Quiet cells (weight under 1) keep their measured level: `W x lowrank + (1 - W) x measured`. Floor at
   `top - 90`.

### Analysis-by-synthesis gain correction

3 iterations x 2 seeds (1, 2):
1. Render the current gesture (every frame as a keyframe) mono with seed `7919 (iteration + 1) + seed`.
2. Analyse with the same smoothing. Average the dB over seeds.
3. `diff = target - mean`, zero outside active cells, uniform filter over 3 bands x 3 frames.
4. `gain += 0.85 x clip(diff, -12, 12)`, floored at `top - 90`.

These iterations correct the level losses from overlapping bands, window spread and the slow filters of the low
bands.

### Stereo trajectory

- Smooth the group L, R and cross powers over 7 frames.
- `ild = clip(dB(PL) - dB(PR), -20, 20)`, `icc = clip(C / sqrt(PL PR), -1, 1)`.
- Active cells: group power within 45 dB of the maximum. Each quiet cell gets the mean over the active cells of the
  group, weighted by power, so low-level noise does not move the image around. A group with no active power gets ild 0 and icc 1.
- Group weights: the share of each group's total power.

### Keyframes

Greedy insertion:
1. Start with the first and last frame plus the required frames (the peak and the onset).
2. Linear interpolation between keys. Per frame error
   `sqrt(sum_cells W (Yhat - Y)^2 / max(sum W, 0.25 max over frames of sum W))`.
3. Insert the frame with the largest error until it is at most the tolerance or there are `max_keys` keys.

The rows Y are `[gain dB per band (48), ild / 2 per group (6), icc x 10 per group (6)]`. The weights are the
band weights `clip((G - (max G - quiet_db)) / 25, 0, 1)` and, for the stereo columns, `0.5 x` a level weight
`clip((level - (max - 40)) / 20, 0, 1)` times the group share times 7. The tolerance is `max(tol_db, wander)`. The
peak is the loudest frame of the total. The onset is the first frame within 30 dB of it.

### Fine curve (opt-in)

1. Grid: 40 Hz times `2^(k / 24)` up to 20 kHz.
2. Long-term PSD (Welch, size `min(4096, 2^floor(log2(max(len, 256))))`, 75 % overlap, Hann). Each point is the mean
   over +-1/48 octave (the nearest bin when that holds none).
3. Ratio of the original's PSD to the mean of 2 mono renders (seeds 41, 42) in dB. Remove a 9-point trend (+-1/6
   octave). Clip to +-12 dB. Zero where the original is more than 60 dB under its maximum. Fade in from 150 Hz to
   300 Hz (too few bins below).
4. Keep only when its RMS is at least 0.4 dB. Measured: on a long riser CLAP rose from 0.82 to 0.88. On short sounds
   estimation noise costs a little, so it is off by default.

### Texture statistics

Group envelope of a model at 1 kHz frames (48 samples): per group, `sqrt(sum over its bands of density x bw / (sr/2))`.

Statistics of a mono signal against a group envelope (per group with at least 0.4 % of the power share):
1. Band-pass the signal to the group by an FFT mask. RMS per 48-sample frame.
2. Active frames: within 30 dB of the group envelope's maximum and more than 15 ms from the onset. Need at least 24.
3. Log ratio `r = ln(a) - ln(envelope)`.
4. Modulation spectrum: weight frames by the active mask smoothed over 5 frames, remove the weighted mean, FFT, sum
   the power into 19 half-octave rate bands (centres 1 .. 512 Hz, edges `rate x 2^(+-1/4)`), normalise to the total
   (without DC) times the variance.
5. The same spectrum for each half of the active span (needs 12 frames per half).
6. Impulsiveness: the std of the log analytic envelope (full rate) relative to the group envelope over the active
   frames. Gaussian noise gives 0.64 nepers (Rayleigh). Sparse crackle gives up to about 1.5. Unlike kurtosis,
   this measure is not dominated by the single loudest click.
7. Low-rate series: the centred log ratio low-passed at 64 Hz (for coherence).

Averages over render seeds use the mean ignoring missing values.

### Texture fit (seeds 31, 32, 33)

1. Ridge signals per seed are rendered once and reused (noise bands 400 dB down).
2. Statistics of the original (`Sr`) and of plain Gaussian renders (`S0`). Return null when no group has both.
3. Group weights: the original's shares over valid groups, normalised.
4. Fast variance: the sum over rates at or above 128 Hz. `hx` = weighted excess of the original over Gaussian.
   `hratio` = weighted ratio. Grainy when `hx > max(0.03, 3 x weighted std over seeds)` and `hratio > 1.5`. With
   `halves`, also each half's ratio must be above 1.5. Clicks add variance across all rates, flutter only at its own rate.
5. Grain density (when grainy): loss = weighted squared difference of the impulsiveness statistic between renders
   with grains and the original. Grid 25, 60, 150, 400, 1000, 2500, 6000, 15000 per second. Then try the geometric
   mean with each neighbour of the best. Keep the lowest. Round to 1 decimal.
6. AM (flutter), from the original against a base (Gaussian or grain renders):
   - Low rates: 6 to 64 Hz.
   - Significant: `spec_real > 1.25 x spec_base + 2 x std over base seeds`.
   - Excess = real minus base where significant, limited to the minimum of the 2 halves' excess
     (`half_real - 1.25 half_base`, at least 0): stationary flutter only.
   - Depth per group = `min(sqrt(sum excess), 0.6)`. Shape = group-weighted excess over rates.
   - None when `max(depth where group weight > 0.03) <= 0.08` or the shape is empty.
   - Coherence: the correlation of the low-rate series of the 2 loudest groups (over frames active in both, at least
     24), original minus base, clipped to [0, 1].
7. When AM is found and the sound is grainy: fit the density again with the AM included, then fit the AM again against grain-only renders.
8. Depth correction: render with AM and grains. Per group, `depth x sqrt(clip(want / got, 0.25, 4))`, at most 0.6,
   with want = real minus base low variance and got = rendered minus base (at least 1e-6).
9. Store the stats.

### Synthesis core

Inputs: length n, key times, key densities (dB), centres, seed, optional stereo, texture, ridges, mono flag, rate
scale, roughness, fine curve and its shift, a power flag, optional ridge signal.

1. Seed sequence from `|seed| mod 2^32`, spawned into 5 generators: A (main noise), B (second noise), C (clicks), M
   (modulation), R (ridges).
2. FFT size `next_fast_len(max(n, 64))`.
3. Excitation A: white normal. With grains and roughness above 0: a click train at density
   `density / roughness^2 x rate_scale` (rougher means sparser clicks).
   - Click count from a Poisson draw. Positions uniform. Amplitudes `exp(spread x normal) x random sign`, added at the
     positions.
   - With `norm_ms` above 0: the window is `max(norm_ms, 20 clicks / rate)` in samples. When it is under half the
     length, level the train: centred moving average of power over the odd window, gain `1 / sqrt(max(p, 1e-3 x mean
     p))`, smoothed by the same moving average, applied. A window holds at least 20 clicks on average.
   - Normalise to unit RMS.
4. Excitation B: white normal, only for stereo.
5. Fine curve: multiply the spectra by `10^(interp(log2(f / 2^shift / 40), grid, db)/20)` (0 dB outside).
6. AM (with depth above 0 and roughness above 0): at 1 kHz control rate, a common unit-variance Gaussian process and
   one per group with the rate spectrum (rates times the rate scale; variance per half-octave band converted to
   density, interpolated in log rate, zero under the first band's lower edge, unit std). Per group
   `z = sqrt(coh) z_common + sqrt(1 - coh) z_group` and modulation `exp(sigma z - sigma^2)` with
   `sigma = depth x roughness`, interpolated to samples.
7. Stereo control: `theta = 0.5 arccos(icc)`, `r = 10^(ild/10)`, parameters `c = cos theta`, `s = sin theta`,
   `gl = sqrt(2r / (1 + r))`, `gr = sqrt(2 / (1 + r))`, interpolated over key times per group.
8. Per band: band noise from spectrum A times the band weight. Amplitude `10^(interp(env)/20)`. Times the band's
   blended modulation. Mono or a mono model: add to L. Stereo: band noise B too, then
   `L += amp gl (c nA + s nB)`, `R += amp gr (c nA - s nB)`. Since `c^2 - s^2 = icc`, the correlation comes back.
9. Expected power (power flag): per band `amp^2 x 2 sum w^2 / N`, plus the overlap with the band below
   `2 x 2 sum(w_prev w) / N x amp_prev amp`. The same with the K-weighting gain squared per bin (ITU-R BS.1770 at 48 kHz:
   the 2-stage filter of the forge-core unit). Flutter has a mean square of 1, so it does not enter.
10. Ridges: frequency and dB interpolated over the ridge's key times, zero outside its span. Frequency jitter from a
    Gaussian process with unit weight at rates up to 16 Hz, times `1 + jitter z`. Phase from the cumulative sum plus a
    random start. Amplitude times a 5 ms fade at both ends. Expected power `0.5 A^2` (K-weighted with the gain at the
    median frequency). Mono: add to L. Stereo: add `y gl` to L and `y gr` to R of the ridge's group.
11. A precomputed ridge signal replaces the ridges (mono only).
12. Output: mono `(n,)`, or `(n, 2)` (a mono model copies L to R).

### Render

1. `speed = max(speed, 0.001)`. `s = max(length, 0.001) / speed`. `n = max(64, round(dur x s x sr))`. Key times times s.
2. Octave shift `oct = shift + 0.5 log2(speed)` (faster is higher by 0.5 octave per doubling).
3. Centroid per keyframe in log2 Hz, weighted by `(p / max p)^0.3` with p the band power (a sub rumble does not rule
   it). Mean centroid weighted by keyframe power.
4. Shift per keyframe: `oct + (sweep - 1) (centroid - mean)`. When any is non-zero, resample each keyframe's density
   along the warp axis at `cents / 2^o`.
5. Tilt: add `tilt x log2(cents / 1000)`.
6. Proximity: `d = clip(level - max level, -40, 0)` per keyframe. Add
   `proximity x d x max(0.5 + 0.15 log2(cents / 1000), 0)`. Farther is quieter and duller. Negative flattens.
7. Stereo: `icc = clip(1 - width (1 - icc), -1, 1)`, `ild = ild x width x pan`.
8. Ridges (when the ridge gain is non-zero): times times s, frequency times `2^(oct + (sweep - 1)(centroid at t -
   mean))`, at most 0.45 sr, dB plus `20 log10(ridges)`.
9. Synthesize with power.
10. Level (`level` and `steady`): gain `10^((level_db + gain_db - peak 50 ms mean of expected power)/20) x take gain`.
    The take gain puts the take's K-weighted energy (channel mean) on the expected K-weighted energy. Reason: over a short gesture a noise
    band contains few independent bins. When each take was levelled on its own loudest 50 ms, takes of the same
    model differed by 2 to 7 LU. Now they are within about 0.5 LU. The fit quality numbers level both sounds, so they do
    not move.
11. `steady=False`: gain to put the take's own loudest 50 ms RMS at `level_db + gain_db` (the older levelling; a
    hybrid's air layer uses it, because its post stage was measured so).
12. Motion (doppler or motion non-zero): read the trajectory. `t_pk = peak x s`,
    `tau = max(0.015, 0.25 (rise + fall) x s)`. Apply `bend`, then `pass_pan`.
13. Limit (`limit`): when the sample peak is over -0.5 dBFS, scale down to it. The engine renders with `limit=False`.
14. Return float32.

### Trajectory

Per keyframe:
- `level_db`: total band power in dB.
- `centroid_hz`: the loudness-weighted centroid.
- `tilt_db_oct`: the weighted slope of density over log2(f/1000), weights = band power relative to the keyframe's
  loudest, only bands within 40 dB. Pink noise gives -3, white 0.
- `pan_db`: group-power-weighted ild (+ is left). `width`: 1 minus the power-weighted icc.
- `peak_s`: the loudest keyframe. `rise_s`: from the first keyframe within 20 dB of the peak to the peak. `fall_s`:
  from the peak to the last keyframe after it within 20 dB.

### Bend and pass pan

- Bend: read rate `2^(semitones x tanh((t_pk - t) / tau) / 12)`. Output length up to `ceil(n 2^(|st|/12)) + 2`.
  Position is the cumulative rate, shifted so the peak stays at `t_pk`. Stop where the position passes the input end.
  Linear interpolation, 0 outside. The sound approaches higher and recedes lower. A receding tail gets longer.
- Pass pan: position `clip(amount x tanh((t - t_pk) / tau), -1, 1)`, angle `(position + 1) pi / 4`, gains
  `sqrt(2) (cos, sin)` (constant power, unity in the centre).

### Summary text

Source, length, keyframe count, the 20 dB rise to the peak, the time it stays within 20 dB, centroid at onset, peak and
end, stereo (median and minimum correlation, pan range) or mono, flutter rate, depth in dB (8.686 x nepers) and
coherence, crackle density, up to 4 ridges with their start and end frequency, and the model size in kB.

### Fitter's own compare

`compare(a, b)`: both mono, trimmed at their own onsets (1 ms frames, 30 dB, 5 ms lead), levelled to their loudest
50 ms RMS, zero-padded. Mel distance as in the quality part (periodic Hann). 5 ms envelope correlation (0 with 3 or
fewer active frames). `attack_ms` on a 2 ms envelope smoothed over 3: from the first frame within 20 dB of the peak to
the peak. Tone dB. Lengths. Optional CLAP cosine. `fit_report` gives `fit = compare(orig, render seed 11)` and
`floor = compare(render 11, render 12)`. A fit is as good as the representation allows when the fit distance is near
the floor.

## Constants

| name | value |
|---|---|
| hop | 240 samples (5 ms) |
| bands | 48, 20 Hz to 20 kHz, warp ln(1 + f/80) |
| groups | 0, 300, 1000, 2500, 5000, 10000, 24000 Hz |
| rate bands | 2^(0 .. 9 in 0.5 steps): 1 to 512 Hz |
| AM rates | 6 to 64 Hz, depth cap 0.6 nepers |
| speed pitch | 0.5 octave per doubling |
| FFT sizes | 512 .. 4096, about 2 cycles per bandwidth |
| envelope floor | 90 dB under top, active 70 dB |
| grain spread | 1.0 neper |
| grain levelling | at least 20 clicks per window |
| denoise | max rank 16, 10 EM rounds, white-noise seed 12345 |
| gain iterations | 3 x 2 seeds, step 0.85, clip 12 dB, filter 3 x 3 |
| keyframes | tolerance 1 dB, max 160 (detail 0.5 dB, 600) |
| gesture smoothing | 25 ms (detail 10 ms), x1.5 steps, max 32 frames or T/6 |
| ridges | max 4, 8 dB, 50 ms, from 400 Hz (detail 100, 5 dB, 20 ms, 60 Hz), floor 8-bin median at +-6 bins + 1.6 dB, join 0.04 octave, 12 candidates per frame |
| stereo | smoothing 7 frames, active 45 dB, ild clip 20 dB |
| quiet weight | 50 dB |
| texture seeds | 31, 32, 33; grain grid 25 .. 15000 |
| fine | 40 Hz, 1/24 octave, clip 12 dB, minimum 0.4 dB RMS, seeds 41, 42 |
| motion tau | 0.25 x (rise + fall), at least 15 ms |
| sample peak cap | -0.5 dBFS |
| minimum render length | 64 samples |
| thread env | `HARK_THREADS`, default 2 |

## Errors and edge cases

- Kind or version mismatch raises `ValueError`.
- A mono source gives `stereo = null` and both output channels equal.
- A recording too short for the texture statistics (fewer than 24 active 1 ms frames per group) gives no texture.
- With no keyframes beyond the first and last (2 frames or fewer), every frame is a key.
- `doppler = 0` and `motion = 0` skip the trajectory read.
- `ridges = 0` drops all ridges.
- A zero K-weighted energy gives a take gain of 1.
