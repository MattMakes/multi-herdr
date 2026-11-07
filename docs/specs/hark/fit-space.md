# hark fit: space layer and stereo measures (part of fit.md)

This part specifies the space module: the stereo measures shared by the quality measure and the fitters, and a
fitted diffuse tail layer for stereo recordings whose tail is decorrelated. The index is `fit.md`.

Module: `hark.forge.fit.space`. Pure numpy and scipy. It uses `band_masks` and `finish` from the sines module.

Why it exists: in a reverberant tail the left and right channels carry largely independent signals (a diffuse
field). In the reference dings, glass, bells and swells the tail L/R correlation is 0.1 to 0.5. A model fitted to
the mid plays one signal in both channels, so its tail correlation is 0.9 or higher. The wet layer convolves the dry
render with impulse responses made from seeded noise. Its side part adds `+w` to L and `-w` to R, which leaves the
mono sum unchanged, and with it every mono measure and CLAP. A mid part is added only when the mono tail of the dry
is shorter than the recording's. The wet comes from the dry render, not from band noise, so a tonal ring stays tonal
in the side: a bell's reverb consists of the bell's own partials spread in time.

## Interface

| function | contract |
|---|---|
| `is_stereo(x, tol_db=-60)` | True when x has 2 channels and its side energy is more than -60 dB relative to its mid. |
| `side_powers(x, centers=BANDS, nfft=1024, hop=240)` | `{'t', 'PM', 'PS', 'C'}`: times of the frame centres (s), and per band and frame 3 powers: mid, side, and the real part of the cross power `Re(S conj(M))`. |
| `diffuse_power(P)` | `max(PS - C^2 / max(PM, 1e-30), 0)`. |
| `band_powers(m, centers=BANDS, nfft=1024, hop=240)` | Per band x frame power of a mono signal. |
| `active_span(x)` | `(onset, peak, end)` sample indices. |
| `tail_span(x, start_s=0.03, min_s=0.02)` | `(first, last)` sample of the tail, or null. |
| `corr(L, R)` | Zero-lag correlation, null when either channel has energy under 1e-20. |
| `tail_corr(x, span=None)` | L/R correlation over the tail, null when there is no tail. |
| `needs_space(x, max_corr=0.7)` | True when x is stereo and its tail correlation is at most 0.7. |
| `shaped_side(mid, bands, times, pan, diffuse, sign=1, sr=48000)` | A side signal made from a mid signal. |
| `impulse(model, part='side', seed=0, flat_iters=4)` | One part's impulse response. |
| `wet(model, dry, seed=0, n=None, part='side')` | One part of the wet layer. |
| `tail_s(model)` | Duration in s by which the wet outlasts the dry. |
| `fit_space(x, sr=48000, dry=None, max_corr=0.7, iters=3, need_db=-15, mid_need_db=-10)` | A space model, or null. |
| `render_space(model, dry, seed=0, width=1, ring=False, limit=True)` | Dry plus wet, `(n, 2)`. |
| `describe(model)` | One line of words. |

### Space model (plain JSON)

| field | meaning |
|---|---|
| `kind` | `"space"` |
| `version` | 1 |
| `predelay` | s (0.002 at fit) |
| `build` | s: the impulse responses' rise time |
| `bands` | octave centres, Hz: 125, 250, 500, 1000, 2000, 4000, 8000, 16000 |
| `rt60` | s per band |
| `side_db` | dB per band relative to a unit-energy band impulse; -150 means off |
| `mid_db` | the same for the mid part; all -150 means side only |
| `tail_corr` | the recording's tail correlation |
| `fit` | `need_db`, `mid_need_db` (null when no mid part), `iters`, `dry` (bool), `side_offset_db`, `tail_corr_dry`, `tail_corr_wet` |

## Algorithms

### Stereo framing

- Any input becomes `(n, 2)` float64. A 1-D or 1-column input is duplicated. More columns: the first 2.
- Frames centred at `k x hop` for k = 0 .. `len // hop`. The signal is placed `nfft/2` samples into a zero buffer of
  length `(K-1) hop + nfft`. Hann window (symmetric Hann of `nfft + 2` points without the zero ends). rfft.
- Bands: `band_masks` of the octave centres (log-frequency triangles that sum to 1).
- `M = (L + R)/2`, `S = (L - R)/2`. Per band and frame: `PM = |FM|^2 W`, `PS = |FS|^2 W`, `C = Re(FS conj(FM)) W`.
- `C / PM` measures the share of the side that follows the mid (a pan). `PS - C^2 / PM` is the side power with no
  correlation to the mid (the diffuse part).

### Active span

1. Mono = channel mean. Envelope: 1 ms hops, 5 ms window mean power (centred, edges clipped).
2. When the envelope has fewer than 3 points, return `(0, 0, len)`.
3. Smooth over 10 points (moving mean). Peak point `ip` and level `pk`.
4. Floor threshold: with 80 or more points, `fl` is the lower of the median dB over points 5 to 49 (the pre-roll)
   and over the points from 25 to 12 before the end. The threshold is `clip(fl + 3 - dB(pk), -45, -12)` dB relative to
   the peak. With fewer points it is -40 dB.
5. Onset: the first point at or above `pk x 1e-3` (30 dB under the peak).
6. End: the last point at or above the threshold (the peak point when none).
7. Return `(onset x 48, ip x 48, min(len, last x 48 + 48))`.

### Tail span

Start `peak + 30 ms`. End: the end of the active span. Null when shorter than 20 ms.

### Shaped side

Purpose: build a side signal from a mid signal, so the tail of a ringing partial keeps the source's width.
1. FFT of the mid with size `next_fast_len(n + 8192)`. Zero the DC bin.
2. For each band: when any pan keyframe has `|g| > 1e-4`, add `interp(t, times, pan_b) x` the band's mid. When
   any diffuse keyframe is above 1e-4, add `interp(t, times, diffuse_b) x` the band's quadrature (the band spectrum
   times `-1j x sign`, a Hilbert transform: zero correlation with the mid at lag 0, and an equal envelope).

### Impulse response

1. Clip rt60 to [0.02, 3.0] s. A part is on in the bands where its gain is above -140. When none is on, return a
   single zero.
2. Length `L = (predelay + build + min(3, max rt60 of on bands)) x sr + 1` samples. `build` is at least 1e-4.
3. Generator seed `(seed x 7919 + salt)`, 32-bit mask, salt 4099 for the side and 8191 for the mid (independent
   noises). One white normal draw of length `next_fast_len(2L)`, FFT.
4. Time axis `t = k/sr - predelay`. Rise `1 - exp(-t / build)` for t at or above 0, else 0.
5. Per on band: envelope `rise x exp(-6.907755 t / rt60)` (60 dB at rt60). Band noise = inverse FFT of spectrum times
   the band mask, first L samples, times the envelope.
6. Flatten each band 4 times: take its spectrum, set the magnitude to the band mask and keep the phase, inverse,
   multiply by the envelope again. Reason: in decaying noise the power at a given frequency follows an exponential
   distribution, so for a tonal dry with few partials the wet level varied by 2 to 3 dB (standard deviation) between
   seeds. After flattening the variation is 0.3 to 1 dB.
7. Normalise the band to unit energy and multiply by `10^(gain/20)`. Sum the bands.

### Wet and tail length

- `wet`: the dry's channel mean convolved (FFT) with the part's impulse response, cut or zero-padded to n (default
  the dry length). Zeros when the impulse is all zero or the dry is empty.
- `tail_s`: 0 when no part is on. Otherwise `predelay + build + min(3, max rt60 over bands where either part is on)`.

### Orthogonal wet side

Per octave band (FFT size `next_fast_len(n + 8192)` so the band split does not wrap in time):
1. Band-split the wet side and each reference signal.
2. Gram-Schmidt the references' band signals into an orthonormal basis (skip a vector with norm under 1e-12).
3. Remove from the wet band its projection on each basis vector. Sum the bands.

The references are the dry's side `(L - R)/2` and the dry's mid plus the wet mid. Reason: if the random wet
correlates with the dry's side or mid, it can cancel part of it or shift the stereo image. Once orthogonal, the wet
can only make the sound wider.

### Render

1. Dry as `(n, 2)` copy. A null or empty model returns the dry.
2. `ring=True`: append `tail_s x sr + 1` zero samples.
3. Wet side (made orthogonal) times `width`, wet mid.
4. `L += mid + side`, `R += mid - side`.
5. Finish without a fade-in (the dry has its own fade from zero), with the -1 dBFS peak gain when `limit`.

`render_space(model, dry, seed)` is a pure function of its arguments.

### Fit

Input: the recording X (a path loads at 48 kHz, an array is resampled to 48 kHz) and optionally the dry render of
the chosen model (its seed-0 render through its post stage).

1. Return null when X is not stereo, has no tail span, or its tail correlation is above 0.7.
2. Side powers of X. Diffuse power smoothed over 5 frames. Tail frames: frame times inside the tail span. Return
   null with fewer than 3 tail frames.
3. Energies over the tail: `E_dif` (diffuse, per band) and `E_mid` (smoothed mid, per band).
4. Needs: side = `E_dif`, mid = 0.
5. With a dry: cut or pad it to X's length. Its tail correlation over the same span `tc_dry`. Return null when
   `tc_dry` is null or `tc_dry <= tc + 0.1` (the dry is about as decorrelated already).
   - Side need = `max(E_dif - dry's smoothed diffuse tail energy, 0)` per band.
   - Mid shortfall = `max(E_mid - dry's smoothed mid tail energy, 0)`. The mid need is the shortfall where it is at
     least `E_mid x 10^(-10/10)`, else 0.
   Without a dry, `tc_dry` is 1.
6. `need_rel = dB(total side need) - dB(total E_dif)`. Return null when the total is 0 or `need_rel < -15`.
7. Live bands per part: need above `max(need) x 1e-4`.
8. Starting rt60 per live side band: a line fit to the band's diffuse dB over the tail frames within 30 dB of the
   band's tail maximum (at least 4 points). The slope must be under -1 dB/s, else none. `rt60 = -60 / slope`. Bands
   without a value take the median of the known values (0.5 s when none). Clip to `[0.05, rt_max]` with
   `rt_max = clip(4 x tail span length, 0.1, 3.0)`.
9. Build: the time of the loudest summed diffuse frame minus the time of the loudest summed mid frame, clipped to
   [5 ms, 80 ms].
10. Start model: predelay 0.002, gains 0 dB on live bands and -150 elsewhere.
11. Source for the wet: the dry, or the recording's own channel mean when there is no dry.
12. Iterate 3 times. For each part with live bands:
    - Render the wet part at seed 0, band powers smoothed over 5 frames, tail energy `Ew` per band.
    - Gain per live band with `Ew > 0`: add `10 log10(need / Ew)`.
    - Side bands: when both the wet's decay rate and the needed rate exist, multiply rt60 by `clip(sw / sn, 0.5, 2)`
      and clip to `[0.05, rt_max]` (a wet that decays slower than needed gets a shorter rt60). Convolution adds the dry's
      decay to the wet's, so the result rings longer than the impulse alone.
    - Store gains clipped to [-150, 40] dB, 2 decimals.
13. Mid check: when a mid part is on, drop it (all -150) when it makes the mono `mel_distance` to X worse by more
    than 0.02 dB than the source alone.
14. Side level onto the recording's tail correlation:
    - For each seed in 0, 1, 2, 3, render the side and mid wet parts over the tail span. With
      `a = L_src + wm`, `b = R_src + wm`, `s = ws`, pool the 6 products `a.a, b.b, s.s, a.b, a.s, b.s`.
    - For offsets from -12 to +12 dB in 0.5 dB steps, `g = 10^(offset/20)`:
      `corr(g) = (ab - g as + g bs - g^2 ss) / sqrt((aa + 2 g as + g^2 ss)(bb - 2 g bs + g^2 ss))`.
    - Choose the offset whose correlation is closest to the recording's. Add it to every live side gain.
    - Reason: a short tail contains few independent cycles (about 5 for a 40 ms tail at 125 Hz), so the tail
      correlation of a single render varies a lot. The products are pooled over 4 renders.
15. Keep the layer only when `|tc_wet - tc| < |tc_dry - tc|`. Else return null.
16. Store the `fit` diagnostics.

Measured facts: over 29 dev clips from wide categories, the engine added the layer to 3 chosen models (1 modal, 2
hybrids), and the median tail correlation error went from 0.21 to 0.02. It added it to no chosen sines model,
because the sines side already matched. Across seeds, the correlation varied by +-0.02 around the fitted value on a
230 ms bell tail and by +-0.25 on tails of 40 to 70 ms. Averaged over seeds it was within 0.15 of the recording's,
where the dry alone was 0.23 to 0.29 away.

### Describe

No model: "no space layer". No part on: silent. Else: side only or side + mid, the rt60 range and band range of on
bands, the build in ms, the recording's tail correlation.

## Constants

| name | value |
|---|---|
| bands | 125, 250, 500, 1000, 2000, 4000, 8000, 16000 Hz |
| analysis FFT / hop | 1024 Hann / 240 samples |
| tail start | 30 ms after the loudest smoothed 10 ms |
| min tail | 20 ms |
| max tail correlation to fit | 0.7 |
| dry already decorrelated margin | 0.1 |
| max rt60 / span factor | 3.0 s / 4 x the tail span |
| rt60 clip in fit | 0.05 s to rt_max |
| rt60 clip in impulse | 0.02 to 3.0 s |
| rate correction clip | 0.5 to 2 |
| side offset range / step | +-12 dB / 0.5 dB |
| flatten passes | 4 |
| level fit seeds | 0, 1, 2, 3 |
| need threshold | -15 dB |
| mid need threshold | -10 dB |
| mid mel tolerance | 0.02 dB |
| iterations | 3 |
| gain clip | -150 to 40 dB |
| predelay | 2 ms |
| build clip | 5 to 80 ms |
| seed salts | side 4099, mid 8191, multiplier 7919 |
| smoothing | 5 frames |
| active span envelope | 1 ms hop, 5 ms window, 10-point smoothing |
| active floor clip | -45 to -12 dB re peak, 3 dB over the floor; -40 dB for short sounds |

## Errors and edge cases

- Mono input to `fit_space` returns null. `tail_corr` of a mono input is 1 (both channels equal), so it never needs
  space.
- A silent channel gives `corr = null`.
- A null model in `render_space` returns the dry as `(n, 2)` with no finish.
- An empty dry in `shaped_side` returns an empty array.
