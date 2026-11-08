# forge-core part 2: DSP sources and effects

Index: [forge-core.md](forge-core.md). This part covers the sound building blocks that recipes combine: module
`hark.forge.dsp` (noise, envelopes, resonators, filters, a small room, a microphone, finishing, control curves,
band-limited oscillators, Shepard glissandi, air turbulence, designed sources, layering) and module
`hark.forge.fx` (reverb, saturation, transient shaping, limiting, EQ matching, Doppler, stereo tools, glitch tools,
granular synthesis, reverse swell).

Every function is deterministic for a given random generator. No recorded audio is read anywhere.
Sample rate SR = 48 000 Hz. Mono = `(n,)`, stereo = `(n, 2)`, dtype float64 unless stated.

## Interface: `hark.forge.dsp`

| function | signature |
|---|---|
| `rng` | `rng(seed)` -> `numpy.random.default_rng(int(seed) & 0xFFFFFFFF)` |
| `n_of` | `n_of(seconds)` -> max(1, round(seconds x SR)) |
| `t_of` | `t_of(n)` -> arange(n) / SR |
| `noise` | `noise(seconds, r, color='white')` |
| `env_exp` | `env_exp(seconds, tau, attack=0.0005)` |
| `env_curve` | `env_curve(points, seconds, shape='lin')` |
| `contact` | `contact(seconds, r, bright=0.7)` |
| `modal` | `modal(freqs, decays, amps, seconds, r, jitter=0.003, glide=0.0)` |
| `modal_cloud` | `modal_cloud(f_lo, f_hi, count, decay_lo, decay_hi, seconds, r, tilt=-3.0)` |
| `excite` | `excite(resonance, excitation)` |
| `butter` | `butter(x, kind, f, order=2)` |
| `peq` | `peq(x, f0, gain_db, q=0.9)` |
| `svf` | `svf(x, cutoff, q=0.7, mode='band')` |
| `room_ir` | `room_ir(r, rt60=0.25, size=0.5, damp=0.6, seconds=None)` |
| `room` | `room(x, r, rt60=0.25, size=0.5, damp=0.6, wet=0.12)` |
| `mic` | `mic(x, r, low_cut=35.0, air_db=-2.0, floor_db=-66.0, color=None)` |
| `stereo` | `stereo(x, r, width=0.3, pan=0.0)` |
| `fade` | `fade(x, fin=0.001, fout=0.02)` |
| `place` | `place(dst, x, t, gain=1.0)` |
| `trim_tail` | `trim_tail(x, floor_db=-70.0, keep=0.01)` |
| `normalize` | `normalize(x, peak_db=-1.0)` |
| `curve` | `curve(spec, n, shape='lin')` |
| `sweep` | `sweep(f0, f1, seconds, curve='exp', power=1.0)` |
| `osc` | `osc(freq, shape='sine', seconds=None, phase=0.0)` |
| `shepard` | `shepard(seconds, r, up=True, rate=0.5, f_lo=40.0, f_hi=10000.0, kind='tone', q=6.0, tilt=-1.5)` |
| `wander` | `wander(seconds, r, rate=4.0, octaves=3)` |
| `air` | `air(seconds, r, center=1200.0, lo_db=6.0, hi_db=4.5, flutter=0.5, rate=6.0, bright=0.6, whistle=0.0)` |
| `sub_drop` | `sub_drop(seconds=1.6, r=None, f0=120.0, f1=34.0, drop=0.25, decay=0.6, harm=0.3, click=0.25, attack=0.004)` |
| `BELLS` | dict of bell presets (Constants) |
| `bell` | `bell(f0, seconds, r, partials='bar', decay=1.2, slope=0.8, bright=0.5, beat=1.0, stretch=0.0, strike=0.3)` |
| `fm` | `fm(fc, ratio, index, seconds, decay=1.0, index_decay=0.3, attack=0.002)` |
| `onset` | `onset(x, db=-24.0)` |
| `align` | `align(x, t=0.0, db=-24.0)` |
| `pan_to_stereo` | `pan_to_stereo(x, pan=0.0)` |
| `stack` | `stack(layers, length=None, onset_db=None, peak_db=None)` |
| `xfade` | `xfade(a, b, seconds=0.02, power=True)` |

The sample-loop kernels (state-variable filter, oscillator) can be JIT-compiled (numba) when it is installed. The
result must be the same with or without the JIT.

Random draw order is part of the contract: a re-render from the same seed must produce the same samples. Each
algorithm below lists its draws in order.

## Algorithms: `hark.forge.dsp`

### Noise and envelopes

`noise(seconds, r, color)`: n = n_of(seconds). Draw n standard normals. For `pink` or `brown`: real FFT; frequency
axis f with f[0] replaced by f[1]; divide the spectrum by f^0.5 (pink) or f^1 (brown, any color that is not white
or pink); inverse FFT to length n. Scale to unit RMS (add 1e-12 to the RMS).

`env_exp(seconds, tau, attack)`: e = exp(-t / max(tau, 1e-5)). a = n_of(attack). When a > 1, multiply the first a
samples by a linear ramp 0..1 (a points, inclusive).

`env_curve(points, seconds, shape)`: sort the (t, value) points. `lin`: linear interpolation of values over t.
`exp`: linear interpolation of 20 log10(max(value, 1e-6)), then back to linear. Outside the points, hold the end
values.

`contact(seconds, r, bright)`: n = max(2, n_of(seconds)). pulse = 0.5 - 0.5 cos(linspace(0, 2 pi, n)) (a raised
cosine from 0 up to 1 and back to 0). grit = n standard normals x pulse x bright. Return pulse + grit.

### Resonators

`modal(freqs, decays, amps, seconds, r, jitter, glide)`: for each (f, d, a): skip when f >= 0.95 x SR/2 or a = 0.
f' = f (1 + jitter x one normal draw). Instantaneous frequency inst(t) = f' (1 - glide (1 - exp(-t / max(d, 1e-4)))).
Phase = 2 pi cumsum(inst) / SR + one uniform draw in [0, 2 pi). Add a exp(-t / max(d, 1e-5)) sin(phase). Draws per
kept mode: normal, then uniform.

`modal_cloud(f_lo, f_hi, count, decay_lo, decay_hi, seconds, r, tilt)`: draw `count` frequencies log-uniform in
[f_lo, f_hi], sort them. u = log(f / f_lo) / log(f_hi / f_lo) (denominator floored at 1e-9). Decay = exp(log decay_lo
+ u (log decay_hi - log decay_lo)) (higher modes die faster when decay_hi < decay_lo). Amplitude =
10^(tilt log2(f / f_lo) / 20) x uniform draws in [0.3, 1.0] (one per mode, drawn as one vector). Call `modal` with
jitter 0.

`excite(resonance, excitation)`: full linear convolution (FFT) of the excitation with the resonance, cut to the
resonance length.

### Filters

`butter(x, kind, f, order)`: clip f (scalar or pair) into [10, 0.95 x SR/2]. Butterworth in second-order sections,
kind `low`, `high`, `band` (band-pass, f = (lo, hi)) or `bandstop`. Filter forward only (causal), along axis 0, so
stereo works.

`peq(x, f0, gain_db, q)`: an RBJ peaking biquad. A = 10^(gain_db / 40); w0 = 2 pi min(f0, 0.95 SR/2) / SR;
alpha = sin(w0) / (2q); b = [1 + alpha A, -2 cos w0, 1 - alpha A]; a = [1 + alpha / A, -2 cos w0, 1 - alpha / A];
normalise by a[0]; filter along axis 0.

`svf(x, cutoff, q, mode)`: a time-varying topology-preserving-transform state-variable filter. Cutoff and q can be
scalars or one value per sample. Cutoff is clipped into [20, 0.45 SR]. Per sample i:
- g = tan(pi fc_i / 48000), k = 1 / max(q_i, 0.05)
- a1 = 1 / (1 + g (g + k)), a2 = g a1, a3 = g a2
- v3 = x_i - ic2; v1 = a1 ic1 + a2 v3; v2 = ic2 + a2 ic1 + a3 v3
- ic1 = 2 v1 - ic1; ic2 = 2 v2 - ic2 (states start at 0)
- output: `low` = v2; `band` = v1; `bandn` = k v1; `high` = x_i - k v1 - v2.
Gains: low and high are 1 in their pass bands (with q above 0.707 the response rises to about q at the cutoff). `band` peaks at
q at the cutoff (q 8 is +18 dB). `bandn` peaks at 1.

### Room, microphone, stereo

`room_ir(r, rt60, size, damp, seconds=None)`: seconds = given, else min(2.0, 1.2 rt60 + 0.05). n samples.
1. tail = n normals x exp(-6.91 t / max(rt60, 0.02)) (60 dB down at rt60).
2. lo = first-order Butterworth low-pass of the tail at 1500 + (1 - damp) x 9000 Hz.
3. mix(t) = (t / seconds)^0.7 (linear ramp 0 to 1 over the IR, raised to 0.7). tail = (1 - mix damp) tail +
   mix damp lo (the highs die faster later in the tail).
4. ir = 0.3 tail; ir[0] += 1.
5. 6 early reflections: for each, delay d = int(uniform(0.002, 0.004 + 0.02 size) x SR); when d < n, add
   uniform(0.15, 0.45) x a random sign (choice of -1, 1). Draw order per reflection: delay, amplitude, sign.

`room(x, r, rt60, size, damp, wet)`: build `room_ir`, set ir[0] = 0 (the direct sound is added dry). Convolve x (each
channel the same IR for stereo). Output length = len(x) + len(ir) - 1. Output = dry x (padded) + wet x conv /
(sqrt(sum ir^2) + 1e-9).

`mic(x, r, low_cut, air_db, floor_db, color)`: first-order high-pass at low_cut; peaking EQ at 10 kHz, air_db,
q 0.6; when `color = (f0, gain_db)`, a peaking EQ there with q 0.8. pk = max |y| + 1e-12. Add pink noise (unit RMS) x
pk x 10^(floor_db / 20). Stereo gets an independent noise per channel (drawn channel by channel).

`stereo(x, r, width, pan)`: mono in. Delays d1, d2 = int(uniform(0.2, 0.9) x 1e-3 x SR) each (two draws).
side[n] = x[n - d1] - x[n - d2] (zero before the delay). High-pass the side at 300 Hz (2nd order). L = x + width side,
R = x - width side. Constant-power pan: theta = (clip(pan, -1, 1) + 1) pi / 4; L x cos(theta) sqrt(2),
R x sin(theta) sqrt(2). Centre pan keeps each channel at unit gain.

### Finishing

`fade(x, fin, fout)`: copy. a = min(n, n_of(fin)); b = min(n, n_of(fout)). When a > 1, the first a samples times
0.5 - 0.5 cos(linspace(0, pi, a)). When b > 1, the last b samples times 0.5 + 0.5 cos(linspace(0, pi, b)). Works on
stereo.

`place(dst, x, t, gain)`: index i = n_of(t) when t > 0, else 0. When i >= len(dst), do nothing. Add gain x the first
m = min(len(x), len(dst) - i) samples of x into dst at i, in place. Return dst.

`trim_tail(x, floor_db, keep)`: level m = |x| (stereo: max over channels of the signed values, then abs; that is,
abs of the per-sample channel maximum). Loud samples have m >= max(m) x 10^(floor_db / 20). End = min(n, last loud
index + n_of(keep)); with no loud sample, n. Return x[:end].

`normalize(x, peak_db)`: x x 10^(peak_db / 20) / (max |x| + 1e-12).

### Control curves and oscillators

`curve(spec, n, shape)`: scalar -> n copies; a list of (t, value) pairs -> `env_curve(points, n / SR, shape)[:n]`; an
array of length n -> itself; any other array -> linear resampling to n points over its index range.

`sweep(f0, f1, seconds, curve='exp', power=1.0)`: u = linspace(0, 1, n). `lin`: f0 + (f1 - f0) u. `exp`:
f0 (f1 / f0)^u (equal octaves per second). `ease`: u -> u^power first, then the exp law (power > 1 starts slow and
speeds up; power < 1 moves fast first).

`osc(freq, shape, seconds, phase)`: a band-limited oscillator. A scalar freq becomes a constant track of
n_of(seconds or 1.0) samples. Phase ph starts at phase mod 1. Per sample: dt = clamp(f / 48000, 1e-9, 0.49);
- sine: sin(2 pi ph)
- saw: 2 ph - 1 - blep(ph, dt)
- square: (1 when ph < 0.5 else -1) + blep(ph, dt) - blep((ph + 0.5) mod 1, dt)
- then ph += dt, wrap by subtracting 1 when ph >= 1.
PolyBLEP correction blep(t, dt): when t < dt, with u = t / dt: 2u - u^2 - 1. When t > 1 - dt, with u = (t - 1) / dt:
u^2 + 2u + 1. Else 0.
- tri: the square wave (above), then a leaky integrator y[i] = 4 f_i / SR x sq[i] + 0.9995 y[i-1], with the initial
  state chosen as -0.9995 (1 - 2 (phase mod 1)) (so it starts from -1 at phase 0). Harmonics fall 12 dB per octave.
Peak about 1.

`shepard(seconds, r, up, rate, f_lo, f_hi, kind, q, tilt)`: a Shepard-Risset glissando. octs = log2(f_hi / f_lo).
Voices v = max(2, round(octs)). sign = +1 up, -1 down. For voice k:
u(t) = (k / v + sign x rate x t / octs) mod 1; f(t) = f_lo 2^(u octs); amplitude a(t) = (0.5 - 0.5 cos(2 pi u)) x
10^(tilt u octs / 20). `tone`: add a sin(2 pi cumsum(f) / SR + one uniform phase draw). `noise`: add a x svf(n
normals, f, q, `bandn`) x sqrt(q). Normalise the sum to unit RMS. Each voice draws in voice order.

### Air

`wander(seconds, r, rate, octaves=3)`: a smooth random control, mean 0, unit standard deviation. Compute at 1 kHz:
m = max(16, int(1000 n / SR) + 4) points. For octave i = 0 .. octaves-1: fr = clip(rate / 3^i, 0.05, 400) Hz;
pad = int(2000 / fr) + 8; draw m + 2 pad normals; 2nd-order Butterworth low-pass at fr / 500 (normalised to the
1 kHz Nyquist of 500 Hz), zero-phase (forward-backward); take the middle m points; divide by sqrt(fr) and multiply by
0.8^i; add up. Linearly interpolate to the n sample times (in ms). Subtract the mean, divide by std + 1e-12.

`air(seconds, r, center, lo_db, hi_db, flutter, rate, bright, whistle)`: moving-air turbulence. Steps:
1. Spectrum of n normals. u = log2(max(f, 5) / center). Gain in dB = -10 log10(2^(-lo_db / 3.0103 u) +
   2^(hi_db / 3.0103 u)). Apply as an amplitude 10^(dB / 20). The density rises lo_db per octave below the centre and
   falls hi_db per octave above it. Inverse FFT, unit RMS.
2. c = wander(seconds, r, rate).
3. When bright > 0: low-pass svf with cutoff clip(6 center x 2^(1.2 bright flutter c), 200, 20000), q 0.6 (louder
   gusts are brighter).
4. When whistle > 0: c2 = 0.6 c + 0.8 wander(seconds, r, 0.4 rate); wh = svf(n normals, center x 2^(0.5 c2), 9,
   `bandn`); x = x / RMS(x) + whistle x wh / RMS(wh).
5. Gusts: x = x exp(1.1 flutter c). Return unit RMS.
Draw order: spectrum normals, wander, then (whistle only) the second wander and the whistle normals.

### Designed sources

`sub_drop(seconds, r, f0, f1, drop, decay, harm, click, attack)`: r defaults to rng(0). f(t) = f1 + (f0 - f1)
exp(-t / max(drop, 1e-3)). ph = 2 pi cumsum(f) / SR. y = sin ph + harm (0.55 sin(2 ph + 0.3) + 0.3 sin(3 ph + 0.7)).
Envelope exp(-t / max(decay, 1e-3)); its first n_of(attack) samples times a half raised cosine 0..1. When click > 0:
k = min(n, 12 ms); ck = (k normals high-passed at 1800 Hz) x exp(-t / 1.2 ms) + 0.6 sin(2 pi 2400 t) exp(-t / 3 ms);
add click x ck / peak(ck) to the first k samples. Then fade (fin 0, fout = 30 % of the length) and normalise to
-1 dBFS. A long fade is used because a sub must not stop audibly.

`bell(f0, seconds, r, partials, decay, slope, bright, beat, stretch, strike)`: partials = a BELLS name or a
(ratios, amps) pair. For each partial (q, a): q' = q^(1 + stretch); f = f0 q'; skip when f > 0.45 SR;
a' = a q'^(1.2 (bright - 0.5)); beat offset b = beat x uniform(0.4, 1.6) x (1 when q' >= 1 else 0.5); envelope
a' exp(-t / max(decay q'^-slope, 1e-4)) (highs die first); add envelope x (sin(2 pi f t + phase1) +
0.7 sin(2 pi (f + b) t + phase2)) / 1.7, phases uniform in [0, 6.3). Draw order per partial: beat, phase1, phase2.
The first 1.5 ms times a linear ramp 0..1. When strike > 0: k = min(n, 6 ms); s = band-pass (2 f0, min(12 f0, 18000))
of k normals x exp(-t / 0.8 ms); add strike x s x peak(y) / peak(s). Fade out over 15 % of the length, normalise to
-1 dBFS.

`fm(fc, ratio, index, seconds, decay, index_decay, attack)`: index I(t) = index exp(-t / max(index_decay, 1e-4)).
y = sin(2 pi fc t + I(t) sin(2 pi fc ratio t)) exp(-t / max(decay, 1e-4)). The first n_of(attack) samples times a
half raised cosine. Fade out over 15 % of the length. No normalisation (peak about 1). No random draws.

### Layering

`onset(x, db)`: m = |x| (stereo: max over channels). Return 0.0 for an empty or all-zero signal. Else the first index
where m >= max(m) 10^(db / 20), divided by SR.

`align(x, t, db)`: d = (n_of(t) when t > 0, else 0) - round(onset(x, db) x SR). d >= 0: prepend d zeros. Else drop
the first -d samples.

`pan_to_stereo(x, pan)`: theta = (clip(pan, -1, 1) + 1) pi / 4; g = [cos theta, sin theta] x sqrt(2). Mono x ->
x[:, None] g. Stereo -> x g per channel.

`stack(layers, length, onset_db, peak_db)`: each layer is a dict `{'x', 't' (s, default 0), 'gain' (dB, default 0),
'pan' (optional)}` or a tuple `(x, t, gain_db[, pan])`. When onset_db is given, t -= onset(x, onset_db) (the layer's
onset lands on t, not its first sample). When t < 0, drop the first n_of(-t) samples and set t = 0. Output is stereo
when any layer is stereo or has a pan (not None); else mono. Length = n_of(length) when given, else the max over
layers of round(t SR) + len(x). Each layer is placed with gain 10^(gain / 20); in a stereo mix a mono layer or a
panned layer goes through `pan_to_stereo(x, pan or 0)`. When peak_db is given, normalise the mix.

`xfade(a, b, seconds, power)`: k = min(n_of(seconds), len a, len b), u = linspace(0, 1, k). power: fade-out cos(u pi/2),
fade-in sin(u pi/2) (equal power, for unrelated material); else 1 - u and u (for correlated material). Result =
a without its last k, then the overlap a_tail fo + b_head fi, then the rest of b.

## Constants: `hark.forge.dsp`

Bell presets `BELLS` (ratios; amplitudes):

| name | ratios | amplitudes |
|---|---|---|
| church | 0.5, 1.0, 1.183, 1.506, 2.0, 2.514, 2.662, 3.011, 4.166, 5.433, 6.796, 8.215 | 0.55, 0.8, 0.7, 0.35, 1.0, 0.45, 0.4, 0.3, 0.25, 0.18, 0.12, 0.08 |
| tubular | 1.0, 2.756, 5.404, 8.933, 13.34 | 0.35, 1.0, 0.7, 0.4, 0.2 |
| bar | 1.0, 2.756, 5.404, 8.933 | 1.0, 0.3, 0.12, 0.06 |
| glass | 1.0, 2.32, 4.25, 6.63, 9.38 | 1.0, 0.45, 0.25, 0.12, 0.06 |
| bowl | 1.0, 2.71, 5.13, 8.27 | 1.0, 0.6, 0.3, 0.12 |

The church ratios are the classic hum, prime, tierce, quint, nominal series. The bar ratios are a free-free beam.

Other numbers: 60 dB decay constant 6.91 (= ln 1000); early reflections 6, delay 2 ms to 4 + 20 size ms, amplitude
0.15-0.45; IR tail weight 0.3; mic default low cut 35 Hz, air -2 dB at 10 kHz (q 0.6), floor -66 dB re peak;
stereo delays 0.2-0.9 ms, side high-pass 300 Hz; svf q floor 0.05, cutoff clip 20 Hz to 0.45 SR; oscillator dt clamp
0.49; triangle leak 0.9995; sub_drop click 12 ms, high-pass 1800 Hz, 1.2 ms decay, 2400 Hz tone at 0.6 with 3 ms
decay; bell strike 6 ms, 0.8 ms decay, band 2 f0 to min(12 f0, 18 kHz); bell attack 1.5 ms; fades: sub_drop 30 %,
bell and fm 15 %.

## Interface: `hark.forge.fx`

All effects take mono or stereo float input unless stated, and keep mono as mono unless stated. Randomness comes only
from an explicit generator.

| function | signature |
|---|---|
| `IR_CACHE_MB` | module constant, 64 (reverb IR cache budget per process, MB) |
| `ir_cache_stats` | `()` -> `{'hits', 'misses', 'entries', 'mb'}` (mb with 1 decimal) |
| `ir_cache_clear` | `()` clears the cache and zeroes hits and misses |
| `ir_grid` | `ir_grid(rt60=1.6, size=0.6, predelay=0.01, damp=0.5, bass=1.2, mod=1.0, diffuse=0.7, width=1.0, tail=None, lowcut=0.0)` -> dict of snapped values |
| `early` | `early(x, r, size=0.5, taps=18, damp=0.5, width=1.0)` -> stereo |
| `reverb_ir` | `reverb_ir(r, rt60=1.6, size=0.6, predelay=0.01, damp=0.5, bass=1.2, mod=1.0, diffuse=0.7, early_db=-6.0, width=1.0, late='noise', lines=16, tail=None, inputs=1, hf_hz=4000.0, lf_hz=250.0, air=1.0, lowcut=0.0, seed=None, cache=True)` -> array `(n, inputs, 2)` |
| `reverb` | `reverb(x, r, rt60=1.6, size=0.6, predelay=0.01, damp=0.5, bass=1.2, mod=1.0, diffuse=0.7, early_db=-6.0, width=1.0, wet=0.3, dry=1.0, late='noise', lines=16, tail=None, stereo=None, air=1.0, lowcut=0.0, ir_seed=None, grid=False)` |
| `saturate` | `saturate(x, drive_db=6.0, mode='tanh', mix=1.0, bias=0.0, os=None, level='rms')` |
| `transient` | `transient(x, attack_db=3.0, sustain_db=0.0, attack_ms=8.0, speed=1.0, floor_db=-60.0)` |
| `limit` | `limit(x, ceiling_db=-1.0, lookahead=0.0025, release=0.08, true_peak=True)` |
| `split` | `split(x, freqs)` -> list of len(freqs) + 1 bands |
| `match_curve` | `match_curve(x, target, amount=1.0, max_db=12.0, smooth=1)` -> list of 28 gains dB |
| `eq_bands` | `eq_bands(x, gains_db, phase='min', taps=4096)` |
| `eq_match` | `eq_match(x, target, amount=1.0, max_db=12.0, smooth=1, phase='min', iters=2)` |
| `doppler` | `doppler(x, speed=40.0, dist=4.0, t_close=None, air=1.0, width=0.8, stereo=True, c=343.0, floor_db=-30.0)` |
| `decorrelate` | `decorrelate(x, amount=1.0, cut=250.0)` -> stereo |
| `pan` | `pan(x, pos)` -> stereo |
| `width` | `width(x, w)` -> stereo |
| `sweep_filter` | `sweep_filter(x, f0, f1, q=4.0, curve_='exp', power=1.0, mode='bandn')` |
| `varispeed` | `varispeed(x, speed)` |
| `tape_stop` | `tape_stop(x, start=None, seconds=0.6, curve_=1.6)` |
| `crush` | `crush(x, bits=8, rate=None, mix=1.0)` |
| `stutter` | `stutter(x, start, length, repeats=4, gate=0.85, decay_db=0.0, pitch_step=0.0, fade=0.002)` |
| `glitch` | `glitch(x, r, grid=0.03, repeat=0.25, reverse=0.1, drop=0.08, octave=0.08, crush_=0.08, fade=0.0015)` |
| `granular` | `granular(src, seconds, r, grain=0.06, density=40.0, pos=(0.0, 1.0), pos_jitter=0.02, pitch=0.0, pitch_jitter=0.0, time_jitter=0.5, spread=0.6, reverse=0.0)` -> stereo |
| `reverse_swell` | `reverse_swell(hit, r, seconds=1.0, rt60=2.5, size=0.8, damp=0.4, swell_db=-6.0, gap=0.0, hit_room=0.15)` -> stereo |

## Algorithms: `hark.forge.fx`

### Shared helpers

- RMS helper: sqrt(mean(x^2)) + 1e-12.
- Schroeder all-pass of integer delay d and gain g, sample by sample with a circular buffer v of length d (starts at
  zeros): read v = buf[j]; u = x_i + g v; y_i = v - g u; buf[j] = u; advance j.
- First-order shelf coefficients for gain G at corner fc (fc clipped to [20, 0.45 SR]): K = tan(pi fc / SR),
  s = sqrt(G). High shelf (gain G at Nyquist, 1 at DC): b0 = s + K, b1 = K - s, a0 = 1/s + K, a1 = K - 1/s. Low shelf
  (gain G at DC, 1 at Nyquist): b0 = 1 + K s, b1 = K s - 1, a0 = 1 + K / s, a1 = K / s - 1. Return (b0/a0, b1/a0,
  a1/a0) for the filter y = b0 x + b1 x[-1] - a1 y[-1].
- Cubic Hermite read at fractional positions pos (zero outside the signal): i = floor(pos), fr = pos - i, with
  neighbours x[-1], x0, x1, x2: c1 = 0.5 (x1 - x[-1]); c2 = x[-1] - 2.5 x0 + 2 x1 - 0.5 x2; c3 = 0.5 (x2 - x[-1]) +
  1.5 (x0 - x1); value = ((c3 fr + c2) fr + c1) fr + x0.
- One-pole time constant coefficient for ms milliseconds: exp(-1 / max(1, ms x 1e-3 x SR)).

### Reverb IR cache

The cache is a process-wide LRU map guarded by a lock. Budget IR_CACHE_MB megabytes (sum of the stored IR array
bytes). On insert, evict the oldest entries until the total is within budget. A hit moves the entry to the newest
position and counts a hit. A miss counts a miss.

`reverb_ir` keys and behaviour:
- `seed` is an integer: a fixed room. The IR is built from `rng(seed)`. The caller's generator `r` is not touched.
  The key is ('seed', seed, every parameter).
- `seed` is None and `cache` is true and `r` has a bit generator: the key is ('state', the exact text form of the
  generator state, every parameter). On a hit, set `r` to the state that the build left behind (stored with the
  IR). So a replay of the same (params, seed) is sample-identical to a fresh build, and costs no build. Different
  seeds never hit.
- Else no cache.
- Return a copy of the cached IR on a hit. Store a copy on a miss.

The parameter part of the key: rt60, size, predelay, damp, bass, mod, diffuse (floats), early_db (float or None),
width, late (text), lines (int), tail (float or None), inputs (int), hf_hz, lf_hz, air, lowcut (floats).

`ir_grid` snaps room settings so a search keeps hitting the cache. Times (rt60, tail) snap to 1/24 octave in log2
(2.9 %, under the about 5 % just-noticeable difference of reverb time); lowcut snaps to 1/12 octave; size, damp,
bass, mod, diffuse, width snap to steps of 0.05 (rounded to 4 decimals); predelay snaps to 0.5 ms (5 decimals).
A log snap is 2^(round(log2 v / step) x step); it leaves 0, None and negative values as they are.

### `_reverb_ir` (the IR builder)

Output shape (n, inputs, 2), with predelay zeros in front. Steps:
1. lines must be 4, 8, 16 or 32 (Hadamard mixing). Any other value is an assertion error.
2. rt_lo = rt60 x bass (the decay below lf_hz). rt_hi = rt60 x max(0.05, 1 - 0.85 damp) (the decay above hf_hz).
   tail = 1.2 x max(rt60, rt_lo) when not given. pd = n_of(predelay) when predelay > 0, else 0.
   n = max(n_of(tail), 64).
3. Delay lengths: lo = (0.006 + 0.024 size) x SR samples. Targets: nl values log-spaced over [lo, 2.7 lo], each times
   exp(uniform(-0.06, 0.06)) (one vector draw). For each target in order, take the nearest prime (from the primes up
   to int(3.2 lo) + 50) that is not used yet. Then permute the delay list randomly (one permutation draw).
4. Per-pass gain for a decay time rt: g(rt) = 10^(-3 dl / (max(rt, 0.02) SR)) per line. gm = g(rt60), gl = g(rt_lo),
   gh = g(rt_hi). Low shelf at lf_hz with gain gl / gm, high shelf at hf_hz with gain gh / gm. Scale the low shelf's
   b0 and b1 by gm (so the shelf pair applies the mid gain too).
5. For each input c:
   a. Impulse x of length n. When diffuse > 0, pass it through 4 series all-passes with delays
      int([1.1, 1.7, 2.9, 4.3] x (0.6 + 0.8 size) x 1e-3 x SR) + 3c + j (j = 0..3) and gain min(0.8, diffuse).
   b. Input vector B: random signs (1 x nl) / sqrt(nl). Output taps Cm: random signs (nl x 2). When the two columns
      are equal or opposite (|dot| >= nl), flip the sign of the second half of column 2 (no extra draw). Then
      Gram-Schmidt: column 2 -= column 1 x dot(col1, col2) / nl. Normalise both columns to unit norm.
   c. Modulation depth per line = mod x (3 + 9 uniform(0, 1)) x (0.5 + 0.5 size) samples; rate per line uniform
      (0.15, 1.1) Hz; phase per line uniform (0, 2 pi). Draw order: depth vector, rate vector, phase vector.
   d. Run the FDN (below) for n output samples.
   e. Divide by sqrt(mean(gm^2 / (1 - gm^2)) / nl) (unit energy).
   f. When late = 'noise': hand the tail to a stochastic late field. Take the line k whose delay is nearest the median
      delay. Evaluate its shelf pair's magnitude H at 64 log-spaced frequencies from 20 Hz to 0.999 x SR/2. The
      per-frequency decay time is rt(f) = -3 dl_k / (SR log10(min(H, 0.999999))). Crossover time tx = 0.06 + 0.1 size.
      u(t) = clip((t - 0.5 tx) / tx, 0, 1). y = y cos(u pi/2) + late_noise sin(u pi/2) (equal power).
   g. Early reflections: when early_db is not None and early_db > -90: e = 10^(early_db / 10); y /= sqrt(1 + e); add
      `early(unit impulse, r, size, damp=damp, width=width)` cut to n, times sqrt(e / (1 + e)).
   h. Width: mid = (L + R) / 2, side = (L - R) / 2; L' = mid + width side, R' = mid - width side.
6. Air absorption (air > 0): per channel, an STFT with 1024-sample frames and hop 256 (overlap 768, even boundary
   extension). Per bin frequency f, a loss rate in dB per second of air x 0.17 x 343 x (f / 10000)^2. Multiply each
   frame at time t by 10^(-rate x max(t - 512 / SR, 0) / 20). Inverse STFT, cut to n. (Air absorbs about 0.17 dB per
   metre at 10 kHz and grows with f^2. A long tail gets darker as it decays.)
7. lowcut > 0: 2nd-order high-pass of the IR at lowcut.
8. Fade the last min(n, n_of(min(0.1, 0.2 tail))) samples with (1 - u)^2 (u from 0 to 1).
9. Prepend pd zero rows.

FDN (one input channel, nl lines, 2 outputs):
- Each line has a circular buffer of length L = the smallest power of 2 not less than int(max delay + max depth + 8).
- Per output sample i, for each line k:
  - delay d = dl_k + depth_k sin(2 pi rate_k i / 48000 + phase_k);
  - a first-order all-pass (Thiran) fractional read: M = floor(d - 0.5), fd = d - M, eta = (1 - fd) / (1 + fd);
    y = eta buf[w - M] + buf[w - M - 1] - eta ya_k; ya_k = y. (Unity gain at every frequency, so the modulation does
    not darken the tail.)
  - the low shelf then the high shelf in transposed direct form (state zl_k, zh_k): t = lb0 y + zl; zl = lb1 y -
    la1 t; u = hb0 t + zh; zh = hb1 t - ha1 u; v_k = u.
- Outputs: out[i, c] = sum_k C[k, c] v_k.
- Mix: an in-place fast Walsh-Hadamard transform of v (butterflies p + q, p - q for h = 1, 2, 4 ... < nl), then
  scale by 1 / sqrt(nl).
- Write: buf[k, w] = v_k + (when i < input length) sum_c B[c, k] x[i, c]. Advance w.

Late noise field (Polack model), for n samples and width 1: twice (mid, then side), draw n + 1024 normals, STFT with
frame 1024 and overlap 768, multiply each bin at time t by exp(-6.91 t / rt(f)) (rt interpolated from the 64-point
table), inverse STFT, cut to n. High-pass the side at 200 Hz (the low end stays coherent). L = m + w sd,
R = m - w sd, divided by sqrt(1 + 0.9 w^2). Scale by sqrt(13.8 / (median(rt) x SR)) (unit energy at the median
decay). The noise tail has no ringing modes. Measured with a long FFT at rt60 2.5-5 s, a pure FDN tail (late = 'fdn') has
peaks 13-16 dB above the local mean; the noise tail has 8-10 dB.

`early(x, r, size, taps, damp, width)`: mono mix of x. t_hi = 0.006 + 0.05 size. IR length m = n_of(t_hi) + 2,
2 channels. Per channel c: tap times = sort(exp(uniform(log 0.003, log t_hi, taps))); for c = 1 multiply by
(1 + width x uniform(-0.12, 0.12, taps)); add random sign x 0.003 / t at index min(m - 1, n_of(t)) (1/distance level).
Darken by distance: split the IR index range into 3 equal parts; low-pass part j (0, 1, 2) at 14000 - damp x 4000 x
(j + 1) Hz (1st order, applied to that part alone, zero elsewhere) and sum. Normalise each channel to unit energy.
Convolve the mono x with each channel. Output length len(x) + m - 1, stereo.

### `reverb`

1. Mono input is treated as 1 channel. `stereo` defaults to true for a stereo input and false for mono.
2. When grid is true, snap the room settings with ir_grid.
3. Build the IR with inputs = the input channel count and seed = ir_seed.
4. Wet = sum over input channels c of conv(x[:, c], ir[:, c, :]) (2 output channels). Output length
   len(x) + len(ir) - 1.
5. Not stereo: y = wet x mean over the 2 wet channels x sqrt(2) (keeps the wet energy); add dry x the input mono mix
   to the first len(x) samples. Return mono.
6. Stereo: out = wet x wetv; add dry x the input (a mono input is copied to both channels).

The wet IR has unit energy, so `wet` is the reverb level relative to the direct sound (0.3 is about -10 dB).
The output length is len(x) + predelay + tail. Measured on the whoosh recipe: building the IR took
3.0 s of 7.0 s for 4 renders (43 %). A fixed room (ir_seed) or a grid removes up to that share.

### `saturate`

1. Gain g = 10^(drive_db / 20). Oversampling factor os = given, else 8 for `hard` and `fold`, 4 otherwise.
2. Upsample by os with a polyphase resampler (Kaiser window, beta 9.0) when os > 1.
3. Curves:
   - `tape`: high shelf +4 dB at 3000 Hz (corner fc / os on the upsampled signal); bb = 0.12 + bias;
     u = tanh(g u + bb) - tanh(bb); high shelf -4 dB at 3000 Hz.
   - `hard`: clip(g u + bias, -1, 1) - clip(bias, -1, 1).
   - `fold`: sin(pi/2 (g u + bias)) - sin(pi/2 bias).
   - else (`tanh`): tanh(g u + bias) - tanh(bias).
4. Downsample by os, cut to the input length.
5. `tape`: 1st-order low-pass at 17 kHz. When bias != 0 or tape: 1st-order high-pass at 8 Hz (removes DC).
6. Level: `rms` scales y to the input RMS; `peak` to the input peak; None leaves it.
7. Output = mix y + (1 - mix) x.
Measured reason for oversampling: a 7 kHz sine driven 18 dB through tanh has aliases 53 dB under the harmonics with
oversampling and 14 dB under without.

### `transient`

Level-independent attack and sustain shaping. Stereo is linked (detector on the per-sample max of |x| over
channels). With s = speed:
1. ef = envelope follower of m with attack coefficient coef(0.1 s ms) and release coefficient coef(15 s ms).
   Follower: e = a e + (1 - a) x_i, with a = attack coefficient when x_i > e, else release.
2. es = a slow-rising copy of ef with coefficient coef(attack_ms x s): s_i = att s + (1 - att) ef_i, capped at ef_i
   (it follows every fall at once).
3. Attack weight a = clip(20 log10((ef + 1e-9) / (es + 1e-9)) / 6, 0, 1).
4. el = follower with coef(0.3 s) attack and coef(350 s) release; eq = follower with coef(0.3 s) attack and
   coef(25 s) release. Decay weight d = clip(20 log10((el + 1e-9) / (eq + 1e-9)) / 9, 0, 1) x (1 - a). A steady tone
   has el = eq, so d = 0.
5. Gate = clip((20 log10(el / (max m + 1e-12) + 1e-12) - floor_db) / 12, 0, 1). Nothing under floor_db is lifted.
6. Gain dB = attack_db a + sustain_db d gate. De-zipper: follow (gain - min gain) with coef(0.2 ms) attack and
   coef(5 ms) release, then add min gain back.
7. Multiply x by 10^(gain / 20). A boost can push peaks over 0 dBFS. Limit afterwards.

(The time constants are in ms: 0.1 ms, 15 ms, attack_ms, 0.3 ms, 350 ms, 25 ms, 0.2 ms, 5 ms, each times speed
except the de-zipper.)

### `limit`

Offline look-ahead peak limiter, zero latency, stereo linked.
1. c = 10^(ceiling_db / 20). pk = per-sample max |x| over channels. With true_peak: upsample 4x (polyphase), take the
   max |.| over each group of 4 upsampled samples, and take the max with pk.
2. need = min(1, c / max(pk, 1e-12)).
3. L = max(1, n_of(lookahead)). g = running minimum of need over a window of 2L + 1 (edge mode nearest).
4. Release: s starts at 1; per sample s = 1 - (1 - s) x coef(release x 1000 ms); when g_i < s, s = g_i; output s.
5. Smooth: running mean over 2L + 1 (edge nearest). Every averaged value spans only gains not above the sample's
   own need, so the smoothed gain never exceeds the need.
6. y = x g, then a final clip to [-c, c].

### `split`

Linkwitz-Riley 4th-order band split. One LR4 stage = a 2nd-order Butterworth section applied twice (low or high).
For each frequency f in ascending order: pass every band already split through the LR4 all-pass at f (low + high
of itself), so all bands share the same phase; append the low part of the rest; the rest becomes its high part.
Return the bands low to high plus the rest. The bands sum to an all-pass of x (flat magnitude).

### EQ matching

The band set is the tone module's 28 1/3-octave centres (part 3, `CEN`, 31.5 Hz to 16 kHz).

Target forms: a tone reference object (its median profile), a non-empty list or tuple of files or arrays (their
tone reference median), a 28-value array (a profile in dB), or a signal (its profile).

`match_curve(x, target, amount, max_db, smooth)`:
1. p = profile(x), t = the target profile, d = t - p.
2. Centre on the loud bands: weights w = 10^(p / 10); d -= sum(w d) / sum(w).
3. Where p < -50 dB, cap d at +6 dB (EQ cannot create what is not there).
4. Clip d to +-max_db.
5. Smooth `smooth` times with the kernel [0.25, 0.5, 0.25] (edge-padded by 1).
6. Return amount x d, each rounded to 2 decimals, as a list.

`eq_bands(x, gains_db, phase, taps)`: FFT size nf = 4 taps. Gain curve in dB on the FFT grid: linear interpolation
of the gains over log2 frequency (log2 of max(f, 1)), flat beyond the end bands.
- `linear`: H = 10^(g / 20); h = inverse FFT, rotated by taps/2, cut to taps, Hann-windowed; delay = taps / 2.
- `min` (default): minimum phase through the real cepstrum. c = inverse FFT of ln|H|. Fold: c'[0] = c[0];
  c'[1 .. nf/2 - 1] = 2 c[...]; c'[nf/2] = c[nf/2]; rest 0. h = inverse FFT of exp(FFT(c')), cut to taps. Window: ones
  for the first half, the falling half of a Hann window for the second half. Delay 0.
- Convolve and return the part from `delay` of length len(x). Minimum phase has no pre-ringing on transients.

`eq_match(x, target, amount, max_db, smooth, phase, iters)`: tot = zeros. Repeat max(1, iters) times: tot =
clip(tot + match_curve(y, target, 1.0, max_db, smooth), -max_db, max_db); y = eq_bands(x, tot, phase) (always from the
original x). When amount != 1, y = eq_bands(x, amount x tot, phase). Scale y to the RMS of x.

### Motion

`doppler(x, speed, dist, t_close, air, width, stereo, c, floor_db)`: the source moves on a straight line past the
listener. x (mono mix) is what the source emits.
1. T = len / SR. t0 = T / 2 or t_close. v = clamp(speed, 1e-3, 0.95 c). Distance at emission time te:
   R(te) = hypot(v (te - t0), dist).
2. Output times: from t_first = R(0) / c to t_last = T + R(T) / c, step 1 / SR (int((t_last - t_first) SR) samples).
3. For a listening time t, the emission time solves t = te + R(te) / c. With b = t - t0:
   te = t0 + (c^2 b - sqrt(v^2 c^2 b^2 + dist^2 (c^2 - v^2))) / (c^2 - v^2).
4. y = Hermite read of x at te x SR. Level: times max(dist / R(te), 10^(floor_db / 20)) (0 dB at the closest point).
5. Air: loss at 10 kHz att = air x 0.17 x max(R - dist, 0) dB. Low-pass cutoff fc = 10000 / max(10^(att / 10) - 1,
   1e-9)^0.25, clipped to [200, 0.45 SR], svf low mode, q 0.707.
6. Mono when stereo is false. Else pan by s = clip(v (te - t0) / R, -1, 1) x width; theta = (s + 1) pi / 4;
   [y cos theta, y sin theta] x sqrt(2).

`decorrelate(x, amount, cut)`: mono mix. side = x through 3 all-passes in series: (149, 0.6), (263, -0.55),
(397, 0.5) (delay samples, gain). High-pass the side at `cut` (2nd order). Scale side to 0.7 x RMS(x). L = x +
amount side, R = x - amount side, both divided by sqrt(1 + 0.49 amount^2). Deterministic (no generator).

`pan(x, pos)`: pos as a control curve (scalar, per-sample array or breakpoints; part `curve`), clipped to [-1, 1].
theta = (p + 1) pi / 4; gains [cos, sin] x sqrt(2) per sample. Mono -> stereo; stereo is balanced.

`width(x, w)`: mono input is decorrelated first (amount 1). k = clip(curve(w), 0, 2). mid = (L + R) / 2,
side = (L - R) / 2 x k. Return [mid + side, mid - side]. 0 = mono, 1 = unchanged, 2 = wider.

`sweep_filter(x, f0, f1, q, curve_, power, mode)`: frequency track = dsp.sweep(f0, f1, len / SR, curve_, power),
padded at the end with its last value to the input length. Apply svf per channel with that track and mode.

`varispeed(x, speed)`: scalar speed: n = int(len / max(speed, 1e-3)), constant track. Breakpoints: n =
n_of(max breakpoint time); an array: n = its length. Track = max(curve(speed, n), 0). Read positions = [0,
cumsum(track[:-1])]; keep positions < len(x) - 1. Hermite read. Speed 2 is an octave up, 0 is stopped.

`tape_stop(x, start, seconds, curve_)`: start defaults to max(0, T - 0.6 seconds). Track = n_of(start) ones, then
(1 - u)^curve_ for u from 0 to 1 over n_of(seconds) points. Positions as in varispeed. Hermite read. Fade the last
min(len, 5 ms) linearly to 0.

### Glitch and grains

`crush(x, bits, rate, mix)`: when rate is given: sample-and-hold with index floor(floor(i rate / SR) SR / rate),
capped at len - 1 (aliasing on purpose). When bits is given: q = 2^(bits - 1), y = round(y q) / q (no dither).
Return mix y + (1 - mix) x.

Edge fade helper: k = min(k, len / 2); when k > 1, multiply the first k samples by a raised cosine rising 0..1 and
the last k by the same reversed.

`stutter(x, start, length, repeats, gate, decay_db, pitch_step, fade)`: i0 = n_of(start) when start > 0, else 0;
L = n_of(length); slice = x[i0 : i0 + L]. Output = x before i0, then `repeats` copies: copy k is read at rate
2^(k pitch_step / 12) (Hermite read of positions 0, rate, 2 rate ...; plain copy when pitch_step is 0), zeroed after
g = max(2, int(L gate)) samples, edge-faded over n_of(fade) on its first g samples, times 10^(k decay_db / 20);
then x after the slice.

`glitch(x, r, grid, repeat, reverse, drop, octave, crush_, fade)`: slice length L = max(16, n_of(grid)). k =
n_of(fade). For each slice in order: one uniform draw u; cumulative thresholds of (repeat, reverse, drop, octave,
crush_):
- u < t1 and a previous output slice of the same length exists: use a copy of the previous output slice;
- else u < t2: reversed;
- else u < t3: silence;
- else u < t4: octave up: take every 2nd sample, edge-fade two copies, concatenate, cut and zero-pad to the slice
  length;
- else u < t5: crush with 6 bits and 6000 Hz;
- else keep.
Edge-fade the slice, write it, and remember it as the previous slice. Same length as x.
(Note: when u < t1 but no previous slice matches, the next tests use the same u, so the slice may still be
reversed and so on, because u < t1 < t2.)

`granular(src, seconds, r, grain, density, pos, pos_jitter, pitch, pitch_jitter, time_jitter, spread, reverse)`:
1. Mono source s. Output n = n_of(seconds) stereo.
2. Position control pc: a 2-tuple of scalars scans linearly from pos[0] to pos[1] over the output; anything else
   goes through `curve`.
3. period = 1 / max(density, 1e-3). gl = n_of(grain). Hann window of gl.
4. Loop t = 0, period, 2 period ... while t < seconds:
   - tt = t + time_jitter x period x uniform(-0.5, 0.5) (draw 1). i = n_of(tt) when tt > 0, else 0.
   - When i < n: rate = 2^((pitch + pitch_jitter x normal) / 12) (draw 2); p0 = (pc[min(i, n-1)] + pos_jitter x
     normal (draw 3)) x (len(s) - gl rate); read positions clip(p0, 0, max(0, len(s) - gl rate - 2)) + arange(gl) x
     rate; grain = Hermite read x window; reverse it when uniform (draw 4) < reverse; pan theta = (clip(spread x
     uniform(-1, 1) (draw 5), -1, 1) + 1) pi / 4; add grain x cos theta x sqrt(2) to L and x sin theta x sqrt(2) to R,
     cut to fit.
5. Divide by sqrt(max(1, density x grain x 0.375)) (a Hann window has 3/8 of the power of a rectangle). The output
   RMS is about the source's.

`reverse_swell(hit, r, seconds, rt60, size, damp, swell_db, gap, hit_room)`:
1. Mono mix hm of the hit. wet = reverb(hm, r, rt60, size, damp, predelay 0, wet 1, dry 0, stereo, tail =
   max(seconds, rt60) + 0.1).
2. k = n_of(seconds). Swell = the first k samples of the wet, reversed. Fade in its first min(k, n_of(0.35 seconds))
   samples with a squared linear ramp.
3. after = reverb(hm, r, 0.6 rt60, size, damp, predelay 0, wet hit_room, dry 1, stereo) when hit_room > 0; else the
   hit as stereo.
4. Scale the swell so its peak is peak(after) x 10^(swell_db / 20).
5. Output length k + (n_of(gap) when gap > 0) + len(after). Place the swell at 0 and `after` at seconds + gap. The
   hit's onset sits exactly at seconds + gap.
Generator draws: the first reverb, then the second reverb (when hit_room > 0).

## Constants: `hark.forge.fx`

| item | value |
|---|---|
| IR cache budget | 64 MB per process, LRU |
| ir_grid steps | times 1/24 octave; lowcut 1/12 octave; size, damp, bass, mod, diffuse, width 0.05; predelay 0.5 ms |
| FDN lines | 16 default; allowed 4, 8, 16, 32 |
| delay spread | lo = (6 + 24 size) ms; spread to 2.7 lo; +-6 % jitter; distinct primes up to 3.2 lo + 50 |
| decay split | rt_lo = bass rt60 below 250 Hz; rt_hi = max(0.05, 1 - 0.85 damp) rt60 above 4000 Hz |
| input diffusers | 4 all-passes, [1.1, 1.7, 2.9, 4.3] ms x (0.6 + 0.8 size), gain min(0.8, diffuse) |
| modulation | depth (3 + 9 U) (0.5 + 0.5 size) mod samples; rate 0.15-1.1 Hz |
| late crossover | tx = 0.06 + 0.1 size s, equal power, centred at 0.5 tx |
| late STFT | 1024 frame, overlap 768; side high-pass 200 Hz; level sqrt(13.8 / (median rt SR)) |
| air absorption | 0.17 dB per m at 10 kHz, x 343 m/s, grows with f^2; starts after 512 samples |
| IR end fade | min(0.1 s, 0.2 tail), squared |
| early reflections | 18 taps, 3 ms to (6 + 50 size) ms, R channel times +-12 % x width, darkening 14 kHz - 4 kHz damp per third |
| saturate oversampling | 4 (tanh, tape), 8 (hard, fold); Kaiser beta 9 |
| tape | shelf +-4 dB at 3 kHz, bias 0.12, low-pass 17 kHz; DC high-pass 8 Hz |
| transient | detectors 0.1 / 15 ms, 0.3 / 350 ms, 0.3 / 25 ms; scales 6 dB (attack), 9 dB (decay), gate 12 dB above floor; de-zipper 0.2 / 5 ms |
| limiter | true peak 4x; lookahead 2.5 ms; release 80 ms |
| decorrelate all-passes | (149, 0.6), (263, -0.55), (397, 0.5); side 0.7 RMS; norm sqrt(1 + 0.49 a^2) |
| doppler | c 343 m/s; speed cap 0.95 c; floor -30 dB; air 0.17 dB per m at 10 kHz; cutoff floor 200 Hz |
| glitch crush | 6 bits, 6000 Hz |
| granular overlap | 0.375 x density x grain |
| tape stop end fade | 5 ms |

## Errors and edge cases (dsp and fx)

- `reverb_ir` with lines not in {4, 8, 16, 32}: assertion error.
- `onset` of silence returns 0.0.
- `place` past the end of dst does nothing.
- `trim_tail` of all-quiet input keeps n_of(keep) samples past the last loud sample, which is the peak sample at
  least; it never returns more than the input.
- Filters clip cutoffs into the valid band, so no cutoff error is raised for any value.
- `doppler` clamps the speed under 0.95 c so the emission-time formula stays real.
- The reverb tap matrix has a fallback when the two random columns are equal or opposite. Without it the second
  column becomes zero and the IR becomes NaN.
- `glitch` octave-up keeps the slice length by zero padding.
- `granular` with a source shorter than one grain at the pitch rate clips the read start to 0.

## Acceptance tests (dsp and fx)

D1. `rng(7)` twice gives the same `noise(0.1, r, 'pink')`; the output has RMS 1.0 within 1e-6.
D2. `osc(1000, 'saw', 0.1)` has peak within [0.9, 1.15] and, in its spectrum, the content above 24 kHz aliased back
    at least 40 dB under the fundamental. `osc(1000, 'tri', 0.1)` has its 3rd harmonic about 19 dB (1/9) under its
    fundamental (within 2 dB).
D3. `svf(white noise, 1000, 8, 'band')` peaks about +18 dB at 1 kHz; with mode `bandn` it peaks about 0 dB.
D4. `sweep(100, 1600, 1.0, 'exp')` is 400 Hz at its midpoint (within 1 %).
D5. `sub_drop(1.6, rng(0))` peaks at -1 dBFS within 0.01 dB, and its last sample is 0.
D6. `bell(1000, 1.0, rng(1), 'bar')` has spectral peaks near 1000, 2756 and 5404 Hz.
D7. `stack([(a, 0, 0), (b, 0.1, -6, 0.5)])` returns stereo; its length is max(len a, 4800 + len b).
D8. `xfade(a, b, 0.02)` has length len a + len b - 960.
D9. `reverb(impulse, rng(1), rt60=1.0, wet=1, dry=0, stereo=True)`: the energy decay curve (Schroeder integral) of
    the output falls 60 dB within 0.8-1.2 s (mid band 500-2000 Hz); the L/R correlation of the tail is under 0.5.
D10. Two calls `reverb(x, rng(5), ...)` with the same parameters give identical samples, and the second is an IR cache
     hit (`ir_cache_stats()['hits']` grows by 1). After the hit, the generator state equals the state after a fresh
     build (the next draw is the same).
D11. `reverb(..., ir_seed=3)` leaves the caller's generator unchanged.
D12. `limit(x, -1.0)` on a signal that peaks at +6 dBFS returns every sample at or under 10^(-1/20); a signal that
     peaks at -6 dBFS is unchanged within 1e-9.
D13. `split(x, [200, 3000])` returns 3 bands; their sum has the same magnitude spectrum as x within 0.1 dB.
D14. `saturate(sine 7 kHz, 18, 'tanh')` has its strongest alias component at least 45 dB under the strongest
     harmonic. With level 'rms' the output RMS equals the input RMS within 1e-9.
D15. `transient(steady sine, attack_db=6, sustain_db=6)` changes the steady part by under 0.5 dB after the first
     50 ms.
D16. `eq_match(pink noise, white-noise profile)` moves the 1/3-octave profile difference RMS (after centring) down by
     at least half; the output RMS equals the input RMS.
D17. `doppler(sine 1000 Hz, speed 40, dist 4)`: the pitch at the start is above 1000 Hz and at the end below; the
     ratio approaches (c / (c - v)) / (c / (c + v)) = 1.27 for v = 40.
D18. `crush(x, bits=1)` has only the values -1, 0, 1 (q = 1).
D19. `glitch(x, rng(3))` returns the same length as x and the same samples for the same seed.
D20. `decorrelate(x, 1.0)`: (L + R) / 2 equals x / sqrt(1.49) and the L/R correlation of white noise is under 0.8.
