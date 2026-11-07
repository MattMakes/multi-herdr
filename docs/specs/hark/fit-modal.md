# hark fit: modal fitter (part of fit.md)

This part specifies the modal fitter. It fits struck and ringing sounds (clicks, taps, keys, impacts) as strikes of
damped sinusoids, a contact transient, a sub-20 Hz pulse and band noise envelopes. The index is `fit.md`.

Module: `hark.forge.fit.modal`. Pure numpy and scipy. During a fit, limit the linear algebra library to 1 thread
when a thread-pool control is available (small matrices are about 5 times faster on 1 thread than on a contended
pool). This is an optimisation, not a contract.

Not for: glides and chirps (a rising pop or a zip splits into many false strikes). Designed impacts with long reverb
tails do fit, but their tail is mostly noise bands.

## Interface

| function | contract |
|---|---|
| `fit(x, max_strikes=10, kmax=16, nmax=384, passes=2, skip_ms=0.5, prune_db=-50, max_modes=80, onset_rel=0.2, noise_tol_db=1, analysis_s=3, attack_ms=2.5, verbose=False, ramps=RAMPS_MS, pre_cap_s=0.002, side_t=True)` | A modal model of a path or a 48 kHz array (`(n,)` or `(n, 2)`). |
| `render(model, seed=0, **knobs)` | Validate, then `render_model`. `(n, 2)` for a stereo model or `stereo=True`, else `(n,)`. |
| `render_model(model, seed, size=1, damping=1, brightness=0, hardness=1, noise=0, width=1, room=0, vary=0, spacing=1, gain=0, stereo=None, ring=None)` | The renderer. |
| `check(model)` | `ValueError` when it is not a dict of kind `modal`, or its version is above 1. |
| `onsets(x, rel=0.2, floor_db=-36, min_gap_ms=12, max_n=10)` | Strike onsets in samples. |
| `onset_strength(m, nper=256, hop=32, cap=1.5)` | `(flux, frame_db, frame_centre)`. |
| `fade_out(x, cands=(0.005, 0.01, 0.02, 0.03, 0.05, 0.1, 0.25), gain_db=0.5)` | Length of a linear fade-out at the end, s, or 0. |
| `esprit(s, kmax=16, dyn_db=60)` | `(poles, order)`. |
| `ls_modes(m, starts, freqs, taus, end=None, ridge=1e-5, cross=10)` | Complex amplitudes by least squares. |
| `band_masks(n_fft, edges=NOISE_EDGES, h=0.4)`, `band_centres(edges=NOISE_EDGES, top=22000)` | The noise bands. |
| `summary(model)` | A few lines of text. |
| `to_code(model, name='sound')` | A standalone numpy + scipy script that renders the same samples as `render`. |
| `mel_distance(x, y)`, `env_corr(x, y)`, `rise_ms(x)` | The fitter's own quick checks (not the fit quality measure). |
| `save(model, path)`, `load(path)` | Compact JSON. `load` runs `check`. |

### Model (plain JSON)

| field | meaning |
|---|---|
| `kind` | `"modal"` |
| `version` | 1 |
| `sr` | 48000 |
| `length_s` | Length of the recording, s, 6 decimals. |
| `channels` | 1 or 2. |
| `fade_out_s` | The recording's estimated linear fade-out, s (0 when none). |
| `strikes` | One per onset: `t` (s, 6 decimals), `ramp_ms` (the modes' attack ramp), `modes` (rows `[Hz, tau s, amplitude, phase rad]` referenced at the onset, sorted by frequency), `attack` (the contact as fast modes, same row format, no ramp). |
| `sub` | Null, or `{'rate': 100, 'x': [...]}`: the residual under 20 Hz, decimated to 100 Hz, 4 significant digits. |
| `noise` | `edges` (27 band edges, Hz), `h_erb` (0.4), `env` (28 bands, each a list of `[t s, dB]` breakpoints of band power). |
| `side` | Null for mono. For stereo: per noise band, the side / mid amplitude ratio of the whole recording, capped at 1.5, 4 decimals. |
| `side_t` | Stereo only: the sines module's side model of the recording (pan and diffuse gains per octave band and keyframe). |
| `coherent` | true on every fit of this version. |
| `fit` | `skip_ms`, `modes` (count), `mode_share_db`, `resid_db`. |
| `layer` | `"deterministic"` on a hybrid's modal layer (set by the engine). |

A mode row means `amp x exp(-t/tau) x cos(2 pi f t + phase) x ramp(t)` with t measured from the strike time. A
later strike also holds exact copies of earlier poles that still ring at its onset, so it can re-excite or stop them
(a finger that lifts or holds).

## Algorithms

### Onset strength

1. STFT: 256 samples, hop 32, zero boundary, padded.
2. `S = log(1 + |X| / ref)` with `ref = 0.001 x max |X| + 1e-12`.
3. Flux at frame k (from k = 2): `sum over bins of min(max(S_k - S_{k-2}, 0), 1.5)`. The cap stops a glide's few bins
   from outvoting a broadband strike.
4. Frame energy in dB. Frame centre sample `k x 32`.

### Onsets

1. Silent or empty input: `[0]`.
2. Candidates: frames that are a local flux maximum over +-6 frames (+-4 ms), with flux at least `rel x max flux`
   (0.2), the maximum frame energy over the frame and the next 8 within 36 dB of the loudest frame, and a rise of
   that maximum over the minimum of the frame and the 8 before of at least 3 dB (1 dB when the flux is at least half
   the maximum: a second contact over a loud ring).
3. Skip a candidate whose centre is under 128 samples (unless the first 5 ms already hold 25 % of the peak) or
   within 192 samples of the end (the STFT padding).
4. Refine in a window of +-240 samples (+-5 ms), at least 100 samples and not silent:
   - When the RMS of its first 96 samples is under 5 % of the window peak (from near silence): the first sample at or
     above 10 % of the window peak.
   - Else (over a ringing strike): the first sample-to-sample difference at or above
     `min(max(0.3 x max diff, 4 x median of the first 96 diffs), max diff)`.
5. Minimum gap 12 ms: a candidate closer than that to the previous onset is dropped, but when its flux is more than
   2.5 times the previous one's, the previous onset takes its flux value (the time stays).
6. No onset: the first sample above 10 % of the peak.
7. More than `max_n` (10): keep the strongest by flux, in time order.

### Fade-out estimate

1. 1 ms hops. When there are fewer than 20, return 0.
2. Envelope: power smoothed by an 8 ms Hann (normalised), sampled at the hop centres, in dB. 8 ms stops low modes'
   ripple from looking like a fade.
3. End window: the last `W = min(0.45 x length, 0.6 s)`, frames within 80 dB of the loudest. At least 10 frames, else
   return 0.
4. For a candidate length L: the ramp in dB `r = 20 log10(clip((T - t)/L, 0.001, 1))`. Fit a line to `env - r`. The
   error is the mean absolute dB residual. L = 0 means no ramp.
5. The fade length is the candidate (5, 10, 20, 30, 50, 100, 250 ms, only up to `W/2`) with the lowest error, but
   only when it beats the no-ramp error by more than 0.5 dB. Else 0.

### Un-fading

When a fade-out of length `fo` is found: over the last `nf = fo x sr` samples divide the signal by
`max(ramp, 0.25)` where `ramp` falls linearly to 0. The modes are fitted on the un-faded signal, up to
`n_fit = n - 0.25 nf`. The render applies the fade again.

### ESPRIT

1. N samples (at least 12). `L = max(4, N // 3)`, `M = N - L + 1`. Hankel matrix L x M.
2. SVD. Model order by MDL on the squared singular values `lambda`, with p = L:
   - Cap: `min(kmax, p - 2, count of singular values above sv_0 x 10^(-dyn_db/20))`.
   - For k = 0 .. cap: `MDL(k) = -M (p - k) ln(g / a) + 0.5 k (2p - k) ln M` with g and a the geometric and arithmetic
     means of `lambda[k:]`. Take the minimum.
3. Signal subspace: the first k left singular vectors. `Phi` = least-squares solution of `U_top Phi = U_bottom`
   (rows without the last, rows without the first). Poles = eigenvalues of `Phi`.
4. SVD failure or order 0 returns no poles.

### Mode candidates per strike segment

Mode bands (Hz): 20, 60, 120, 200, 320, 500, 750, 1100, 1600, 2300, 3300, 4700, 6600, 9300, 13000, 21500 (15 bands).

For a real segment of length Ns (at least 32):
1. FFT of size `next_fast_len(2 Ns)`.
2. Per band (lo, hi), bandwidth B: transition width `tw = max(0.15 B, 8 Hz)`. A mask on the full (two-sided) spectrum
   with a raised-cosine rise from `lo - 2tw` to `lo - tw`, flat to `hi + tw`, and a raised-cosine fall to `hi + 2tw`.
   Only positive frequencies, so the result is analytic.
3. Decimation `D = max(1, min(floor(sr / (1.15 (B + 4 tw))), Ns // 48))`. Skip the band when `Ns // D < 16`.
4. Demodulate by the band centre `fc = (lo + hi)/2` and decimate by D (plain sample skip).
5. Windows: the first 384 decimated samples, and a second window from `3 x 384 / 4` when the signal is long enough
   (more than `288 + 192` samples) and its mean power is above 1e-4 of the first window's.
6. ESPRIT on each window. Keep poles with radius in (1e-6, 0.99999). Frequency `fc + angle x fs_d / (2 pi)`,
   `tau = -1 / (ln|z| fs_d)`.
7. Keep a candidate when its frequency is in [lo, hi), tau at least 0.3 ms, `pi f tau >= 2` (Q at least 2) and tau
   under 30 s.

### Dedupe

A candidate is a duplicate of a kept one when `|f - f_e| < max(0.3, 0.0015 f, 0.25 / (pi sqrt(tau tau_e)))` and
`|ln(tau / tau_e)| < 1.2`. Kept modes are the existing ones plus earlier new candidates.

### Least squares of damped exponentials

Each mode k has pole `z_k = exp(-1/(tau sr) + 2j pi f / sr)` and start sample `s_k`. The mode is
`Re(c_k z_k^(n - s_k))` for `s_k <= n < end`. Unknowns: real and imaginary parts.

- Gram matrix in closed form from geometric sums `sum_{n<L} w^n = (1 - w^L)/(1 - w)` (L when `|1 - w| < 1e-12`), with
  `L = max(end - max(s_i, s_j), 0)` and both modes advanced to the later start. With
  `S1 = p_i p_j G(z_i z_j)` and `S2 = p_i conj(p_j) G(z_i conj(z_j))`:
  `RR = Re(S1 + S2)/2`, `II = Re(S2 - S1)/2`, `RI = Im(S1 - S2)/2`, `IR = Im(S1 + S2)/2`. Block matrix
  `[[RR, RI], [IR, II]]`. This keeps many modes over a long file cheap.
- Projection: per group of modes with the same start, `sum m[n] z^(n - s)` over at most `16 x max tau` (in samples)
  or to `end`, computed in chunks of 8192 samples.
- Solve `(G + lambda diag(G)) x = b`, `lambda` starting at the ridge (1e-5). Use a least-squares solve when the
  matrix is singular.
- Cancellation test, up to 5 tries, multiplying `lambda` by 10 each time a test fails:
  - Within one strike: when the sum of the modes' separate energies is more than 2 times the strike's joint energy
    (near-duplicates cancel).
  - Across strikes: when the sum of strike energies is more than `cross` (10) times the total energy (a pile of
    close strikes builds the sound from huge opposite rings). A stopped ring that cancels its own tail stays under
    that ratio.
- Result `c = x_re - 1j x_im`.

Mode energy: `0.5 |c|^2 / (1 - exp(-2/(tau sr)))`.

### Cancelling clusters

Per strike, sort its modes by frequency. A cluster is a run of modes whose neighbours are closer than
`max(2 Hz, half the sum of their bandwidths)` (bandwidth `1/(pi tau)`). In a cluster of 2 or more where the separate
energies add to more than 2 times the energy of their sum, keep only the mode that alone explains the most of the
signal (`b_k^T G_k^-1 b_k`, a matching-pursuit choice) and drop the others.

### The fit

1. Load as `(n, channels)`. Mono = channel mean. Onsets (`rel` 0.2, at most `max_strikes`). Fade-out. Un-fade.
2. `skip = round(skip_ms x sr / 1000)` (0.5 ms). Candidates start at `skip + 0.5 ms` after each onset.
3. Active end: the last 12-sample frame (24-sample window) within 75 dB of the loudest, plus 12 samples, at most
   `n_fit`. Segment end of strike i: the next onset or the active end.
4. Analysis window per strike: at most `analysis_s` (3 s).
5. Passes (2). In each pass:
   - For each strike with at least 48 samples in its segment: candidates from the working signal, deduped against
     the strike's modes. Amplitudes by least squares on the working signal from `onset + skip` to the segment end
     (the last strike to `n_fit`). Keep the strongest (at most `max_modes - existing`, 80 per strike) whose energy is
     within `prune_db` (-50 dB) of the sum. Subtract them from the working signal.
   - Carry-over: each later strike gets copies of earlier strikes' poles that still have more than 1 % amplitude
     (`exp(-(gap)/(tau sr)) > 0.01`) at its onset, the 40 longest, deduped. Its own modes follow, deduped against them.
   - Joint fit of all strikes' modes on the un-faded signal up to `n_fit`. Prune per strike at -50 dB of the
     strike's sum and refit when anything was pruned.
   - Up to 3 rounds: drop cancelling clusters and refit.
   - The working signal for the next pass = signal minus all modes.
6. Reference each strike's modes at the onset: amplitude `|c| exp(skip / (tau sr))`, phase
   `angle(c) - 2 pi f skip / sr`, wrapped. Round: Hz 2 decimals, tau and amplitude 5 significant digits, phase 4
   decimals. Drop rows with amplitude or tau not above 0.
7. Attack ramp per strike: try each ramp in 0, 0.05, 0.1, 0.2, 0.35, 0.5, 0.8, 1.2 ms. The ramp is a raised cosine
   `0.5 - 0.5 cos(pi k / R)` over R samples. Measure the squared error over `[onset, onset + skip + 1.5 ms)` against
   the signal minus the earlier strikes' modes. Keep the lowest (strictly lower by a relative 1e-9). Add the strike's
   modes with that ramp to the mode signal.
8. Sub pulse: when the signal is longer than 4 x 480 samples, low-pass the residual at 20 Hz (4th-order Butterworth,
   forward and backward). When its energy is more than 0.001 of the signal's, decimate by 480 (polyphase) and store
   it. Subtract its upsampled (polyphase x 480) waveform.
9. Contact modes per strike: full-rate ESPRIT (order cap 24, dynamic range 50 dB) on the residual from the onset to
   `min(onset + attack_ms, next onset, n)` (at least 24 samples). Keep poles with radius in (1e-4, 0.9999), angle at
   least 0, tau from 0.01 ms to 2 ms, frequency under 0.47 sr. Amplitudes by least squares over `3 x 2 ms` from the
   onset. Rows sorted by frequency. Subtract them.
10. Deterministic signal = modes + sub + contact. When there is a fade-out, apply it to the deterministic signal, and
    the residual is the original minus that.
11. Noise envelopes (below), with the pre-onset cap.
12. Stereo: side per noise band `sqrt((W^2 . |S|^2) / (W^2 . |M|^2))` from one FFT of the whole sound
    (`next_fast_len(n)`), capped at 1.5. With `side_t` on, add the sines side model of the recording.
13. Diagnostics: mode count, mode share (dB of the mode signal's energy relative to the signal), residual level.

### Noise bands

27 edges (Hz): 50, 100, 200, 300, 400, 510, 630, 770, 920, 1080, 1270, 1480, 1720, 2000, 2320, 2700, 3150, 3700,
4400, 5300, 6400, 7700, 9500, 12000, 14000, 16000, 18500. That gives 28 bands (critical bands with a low and a top
split).

Masks: power-complementary, zero phase. On the ERB-rate axis `u(f) = 21.4 log10(1 + 0.00437 f)`, for edge e:
`s_e = clip((u - u(e) + h) / (2h), 0, 1)` with h = 0.4. Band b is `sin(pi s_{b-1} / 2)` (when b > 0) times
`cos(pi s_b / 2)` (when b < last). The squares sum to 1.

Band centres: `e_0 / sqrt(2)`, the geometric means of neighbouring edges, and `sqrt(e_last x 22000)`.

### Noise envelopes

Purpose: the noise carries what the deterministic part does not. Each band's power is the original's minus the
deterministic part's, so modes plus noise keep the band energy even where the modes misfit.

1. FFT size `next_fast_len(n + 16384)`. Grid every 0.5 ms (24 samples).
2. Band power by the analytic signal: the band spectrum, doubled for positive frequencies, inverse FFT, half the
   squared magnitude.
3. Three smoothings with normalised Hann windows: 0.5 ms, 6 ms, 30 ms.
4. Weight `near`: 1 from 8 ms before to 15 ms after an onset, falling linearly to 0 at 30 ms. Weight `far`: rises
   linearly from 0 at 150 ms to 1 at 300 ms after the latest onset.
5. Power = `near x short + (1 - near) x ((1 - far) x medium + far x long)`.
6. Per band: `max(P_original - P_deterministic, 1e-6 x P_original)`.
7. Pre-onset cap (2 ms): for each onset whose cap start index `a` is at least 1, the band power from `a` to the
   onset may not rise above the minimum of the 3 grid values up to `a`. Reason: the analysis smeared a strike's
   noise up to 1.5 ms ahead of it, 10 to 20 dB over the recording. This was the main cause of blunt modal onsets.
   Measured: on 27 modal dev clips the onset error fell from 0.39 to 0.25 octave (a glockenspiel note from 3.6 to
   0.1), CLAP and mel unchanged.
8. Floor at the overall maximum minus 100 dB.
9. Breakpoints: Ramer-Douglas-Peucker with tolerance `noise_tol_db x (1 + far)`: 1 dB near strikes, 2 dB in long
   tails. Round t to 5 decimals and dB to 2.

### Render plan

Generators (numpy `default_rng` with a seed sequence): jitter `[seed, 1]`, noise `[seed, 2]`, side sign `[seed, 3]`,
room `[seed, 5]`, each with `seed & 0xFFFFFFFF`.

Knobs:

| knob | default | effect |
|---|---|---|
| `size` | 1 | Mode and contact frequencies divided by size, decays times `size / damping`, noise spectrum shifted with them, contact tau times size. |
| `damping` | 1 | Decays divided by damping. Noise tails shortened. |
| `brightness` | 0 | dB per octave about 1 kHz on modes, contact and noise. |
| `hardness` | 1 | Contact ramp divided by hardness, contact tau divided by `sqrt(hardness)`, plus `4 log2(hardness)` dB per octave on modes and the noise around strikes. |
| `noise` | 0 | dB on all noise bands. |
| `width` | 1 | Factor on the side. |
| `room` | 0 | 0 to 1 wet of a small stochastic room. |
| `vary` | 0 | 0 to 1 seeded jitter. |
| `spacing` | 1 | Factor on the time between strikes. |
| `gain` | 0 | Output dB. |
| `stereo` | null | null: stereo when the model has 2 channels. |
| `ring` | null | null: true when vary > 0. |

Steps:
1. Strike times: the first stays. Each later gap is `(t_i - t_{i-1}) x spacing`, plus for a model without `coherent`
   `vary x 2 ms x` a normal draw, at least 3 ms.
2. Coherent: 3 vary fields of log frequency (frequency, tau, amplitude) drawn in that order. Each is
   `sqrt(1/3) x sum of 6 cosines of 2 pi log2(max(f, 20)) c + p`, c uniform in [0.25, 1.25], p uniform in [0, 2 pi).
   The same fields apply to every strike. Not coherent: one normal draw per mode for each of the 3.
3. `stretch = max(size / damping, 0.001)`. `tilt = brightness + 4 log2(max(hardness, 0.001))`.
4. Per strike: `f = f0 / size x (1 + 0.006 vary zf)`, `tau = tau0 x stretch x exp(0.12 vary zt)`,
   `a = a0 x 10^((tilt log2(max(f, 20)/1000) + 1.5 vary za)/20)`.
5. Phase continuity: when coherent and (vary > 0, size != 1 or spacing != 1), every mode of a later strike gets
   `phase + 2 pi (f (tn_i - tn_0) - f0 (t_i - t_0))`, so it keeps its phase on the first strike's timeline.
6. Drop modes and contact modes at or above 0.47 sr. Contact: `fa = f / size`,
   `ta = tau x size / sqrt(hardness)`, `aa = a x 10^(tilt log2(max(fa, 20)/1000)/20)`.
7. Length: `last new strike + (length_s - last old strike) x max(1, stretch)`.
8. Ring (see below).
9. Noise envelope on the new time axis, grid 0.5 ms:
   - Dense old envelope by linear interpolation of each band's breakpoints.
   - Time map per new grid point: segment = the latest new strike at or before it. Relative time
     `rel = t - tn_seg`. Old relative time = `rel` for the first 2 ms, then `2 ms + (rel - 2 ms) / stretch`. Old time =
     `t0_seg + old rel`.
   - Segment end: the next old strike, or the length (for a coherent model in ring mode, the length minus the fade).
     When the segment moved (stretch != 1 or its length changed), the cut is one grid step earlier.
   - Past the cut, the level keeps falling at its slope over the last 10 ms before the cut (slope clipped to
     [-3000, 0] dB/s) for the excess time.
   - Size: shift the band levels along log2 of the centres by `log2(size)` (linear interpolation between bands).
   - Add `brightness x log2(centre / 1000)` and the `noise` knob.
   - Hardness: in a window from 1 ms before to 4 ms after each strike (falling to 0 at 8 ms), add
     `4 log2(hardness) x log2(centre / 1000)`.
   - Vary: add one normal draw per band times `1.5 vary` dB.
10. Sub: shifted by the first strike's move.
11. Fade: the model's fade-out, or 0 in ring mode.

### Ring-out

Purpose: a trimmed recording stops while its modes still ring (a key's release 70 ms before the cut), and vary
lengthens some decays further. A take must not stop on them.

1. Reference: the largest strike mode sum `sqrt(sum a^2)`. Threshold: 60 dB under it. Contact modes do not count.
2. Ring end: the latest `t_strike + tau ln(a / threshold)` over modes above the threshold.
3. Level at the nominal end: the summed mode power at that time in dB relative to the reference.
4. When the ring end is more than 1 ms past the nominal length, extend by
   `min(max(ring end - length, lo), max(lo, 0.5 x (length - first strike)))`, where `lo` is 200 ms when the level at
   the end is above -50 dB, else 60 ms.
5. The added stretch fades 60 dB, linear in dB, over its length. A coherent model's noise continues un-faded into it
   at its slope from before the recording's fade-out (from the fade, the modes rang alone: a comb in the tail).

### Synthesis

- Modes per strike from its onset sample `round(t sr)` to the end: `Re(sum C z^k)` with `C = a e^(j ph)`, computed in
  blocks of 4096 samples up to `14 x max tau` samples.
- The strike ramp (raised cosine) multiplies the modes. The contact modes are added without a ramp.
- A later strike of a coherent model: the strike's whole contribution (modes and contact) rises over a raised-cosine
  ramp of 0.15 ms (7 samples). From full amplitude it put a step into the earlier ring (an impact's takes clicked at
  2 later strikes). The first strike keeps its step.
- Sub: the waveform upsampled x 480 (polyphase), added at its shift.
- Fade-out (non-ring): a linear ramp to 0 over the fade length at the end of the mode signal. The noise bands carry
  the fade themselves.
- Noise: FFT size `next_fast_len(n + 16384)`. Per band: white normal noise of length n (one draw per band, in band
  order, from the noise generator) times `10^(E/20) / sqrt(mean(W_b^2))`, FFT, times the band mask. Sum the bands,
  inverse FFT, first n samples.
- Mid = modes + noise. Ring: the fade over the added stretch.
- Stereo:
  - Sign: -1 when the first uniform draw of the sign generator is under 0.5, else +1.
  - With `side_t` and width > 0: per octave band (log2 triangles at `bands / size`), the band's mid times the pan
    gain plus the band's quadrature (times `-1j x sign`) times the diffuse gain. The gains are read at each grid
    point's old time (the time map), then interpolated to samples. Then times width.
  - Else with `side` and width > 0: the mid's quadrature shaped by `sqrt(sum_b rho_b^2 W_b^2) x width`.
  - Output `[mid + side, mid - side]`.
- Room (room > 0): per channel, an impulse response of `rt60 = 0.25 + 0.15 size` s: white noise times
  `exp(-6.9 t / rt60)`, one-pole low-pass (`b = 0.35`, `a = [1, -0.65]`), the first 4 ms zeroed, unit energy. Add
  `room x` the convolution, cut to the input length.
- End: a 1 ms linear fade to 0 on the last samples. Output gain.

### Quick checks

- `mel_distance(x, y)`: 3 FFT sizes 256, 1024, 4096 with 32, 64, 96 mel bands (30 Hz to 20 kHz, empty filters
  removed), hop 1/4. Floor 80 dB under x's loudest cell. Mean absolute dB over cells where either is above the
  floor. Returns `(mean, [per size])`.
- `env_corr(x, y)`: Pearson correlation of 2 ms RMS dB envelopes, floored 60 dB under x's peak.
- `rise_ms(x)`: 1 ms RMS envelope at 0.125 ms hops. From up to 30 ms before the loudest point, the time from the first
  point at 10 % to the first point at 90 % of the peak.

### Summary text

First line: version, length in ms, strike count, mode count, mode share and residual level. Then per strike: its time,
ramp, mode count and the 5 strongest modes by `a^2 tau` as Hz and decay in ms.

### Standalone code

`to_code` writes a script with the constants (sample rate, grid, sub decimation, ring constants, strike ramp), the
model as a literal (one mode per line), its own copies of the plan, ring-end, masks, ring, modes, noise, room, side
and render functions, and `render(seed, **knobs)`. Run as a script it writes a float32 WAV (argument 1: seed,
argument 2: path). It renders the same samples as `render`. It must not import hark.

## Constants

| name | value |
|---|---|
| mode bands | 20, 60, 120, 200, 320, 500, 750, 1100, 1600, 2300, 3300, 4700, 6600, 9300, 13000, 21500 Hz |
| noise edges | see above (27 edges, 28 bands) |
| ERB crossover half-width | 0.4 ERB-rate units |
| envelope grid | 0.5 ms |
| sub cut / decimation | 20 Hz / 480 (100 Hz), energy share 0.001 |
| ring | 60 dB, at most 0.5 x span, at least 60 ms, 200 ms when above -50 dB at the end |
| later-strike ramp | 0.15 ms |
| attack ramps | 0, 0.05, 0.1, 0.2, 0.35, 0.5, 0.8, 1.2 ms |
| pre-onset cap | 2 ms |
| onset flux | STFT 256, hop 32, cap 1.5, lag 2 frames, rel 0.2, +-6 frame maximum, energy within 36 dB, rise 3 dB (1 dB at half max), gap 12 ms, max 10, take-over factor 2.5 |
| ESPRIT | L = N/3, MDL order, kmax 16, dynamic range 60 dB |
| band candidates | window 384 decimated samples, overlap at 3/4, transition max(0.15 B, 8 Hz), decimation margin 1.15 |
| candidate filters | tau 0.3 ms to 30 s, Q (pi f tau) at least 2, radius under 0.99999 |
| dedupe | max(0.3 Hz, 0.15 %, 0.25/(pi sqrt(tau tau'))), log tau within 1.2 |
| LS ridge / tries / cross ratio | 1e-5, x10 per try, 5 tries / 10 |
| prune | -50 dB of the strike sum |
| max modes per strike | 80 |
| carry-over | amplitude above 1 %, 40 longest |
| cancel cluster | max(2 Hz, mean bandwidth), energy ratio 2, 3 rounds |
| skip | 0.5 ms (candidates from 1 ms) |
| active end | 75 dB under the peak |
| contact | 2.5 ms window, order 24, 50 dB, tau 0.01 to 2 ms, under 0.47 sr |
| noise smoothing | 0.5 ms near strikes (-8 ms to +15 ms, fade to 30 ms), 6 ms, 30 ms after 150-300 ms |
| noise floor | 100 dB under the maximum, residual floor 1e-6 of the original |
| noise breakpoint tolerance | 1 dB near strikes, 2 dB in tails |
| vary sd | frequency 0.6 %, log tau 0.12, level 1.5 dB, noise bands 1.5 dB, old strike times 2 ms |
| min strike gap at render | 3 ms |
| room | rt60 0.25 + 0.15 size s, decay 6.9, low-pass 0.35 / 0.65, 4 ms gap |
| end fade | 1 ms |

## Errors and edge cases

- Kind or version mismatch: `ValueError` from `check`.
- A silent input gives one onset at 0 and no modes. The model is noise only (all bands at the floor).
- A strike with no modes still has a row in `strikes` with an empty `modes` list and ramp 0.
- A segment under 48 samples gets no candidates.
- When no mode survives any pass, the joint fit is skipped and every strike has empty modes.
- Mode or contact frequencies at or above 0.47 sr after knobs are dropped at render.
- `stereo=True` on a mono model with no side gives 2 equal channels.
