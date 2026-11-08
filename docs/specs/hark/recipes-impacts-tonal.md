# recipes-impacts, part tonal: the shared tonal and UI building blocks (T1-T15)

This part belongs to the spec `recipes-impacts.md` (the index). The recipes `ui_pop`, `knock`, `mallet`, `drip` and
`shutter` call these building blocks. In the source they live in a private helper module of the recipes package. The
ticket file list does not name that module, but the 5 recipes cannot be rebuilt without it, so this part specifies
it. The names are descriptive. The clean side picks its own names.

The building blocks share 3 design rules, taken from a study of film UI sounds:
- a soft onset (20-60 ms in film pops and dings, not 5 ms),
- a real ring-down that ends at 0.2-0.4 s (not a fade at the end of a long ring),
- some stereo width (L/R correlation 0.3-0.7 on mallet notes, not 1.0).

Notation: t is the time in seconds of each sample (k / 48000). N(s) is the sample count of s seconds (index, P-N).
g is a standard normal draw from the render rng. Every random draw uses the rng that the caller passes.

## T1 Soft envelope

Inputs: t, `onset`, `tau`, `hold` = 0, `curve` = 1, `dcurve` = 1.

- u = clip(t / max(onset, 1e-4), 0, 1)^curve
- rise = 0.5 - 0.5 cos(pi u)
- x = max(t - onset - hold, 0) / max(tau, 1e-4)
- e = rise x exp(-x) when dcurve = 1, else rise x exp(-x^dcurve)

`curve` above 1 starts the rise slower. `dcurve` = 1 is an exponential decay (20 dB down in 2.3 tau). `dcurve`
above 1 is a body that damps as it rings: slower first, then faster, with t40 / t20 = 2^(1 / dcurve).

## T2 Two-rate decay

Inputs: t, `onset`, `fast`, `tf`, `ts`, `curve` = 1.

- rise as in T1 (onset, curve)
- tm = max(t - onset, 0)
- e = rise x (fast x exp(-tm / max(tf, 1e-4)) + (1 - fast) x exp(-tm / max(ts, 1e-4)))

Most of a strike dies fast; a quiet ring carries on.

## T3 Settling glide

Inputs: t, `f0`, `bend_st` = 0, `ptau` = 0.02. Output: Hz per sample.

f(t) = f0 x 2^((bend_st / 12) x exp(-t / max(ptau, 1e-4)))

A positive bend starts above f0 and falls onto it.

## T4 Rising track

Inputs: t, `f0`, `rise_st`, `rtau`. f(t) = f0 x 2^((rise_st / 12) x (1 - exp(-t / max(rtau, 1e-4)))). Most of the
rise is done in about 3 rtau (a shrinking bubble cavity).

## T5 Nyquist fade of a partial

For a partial at ratio q on track f: gain = clip((0.45 x 48000 - q f) / 2400, 0, 1). The partial fades out over the
last 2400 Hz below 21 600 Hz.

## T6 Tone with partials

Inputs: track f (Hz per sample), rng, parts = list of (ratio q, amp a, decay d), envelope e (default all ones).

1. phase = 2 pi x cumulative sum(f) / 48000 + a uniform start in [0, 2 pi).
2. y = sin(phase) x e.
3. For each part with a > 0, in list order: y += a x T5(f, q) x e^d x sin(q x phase + a new uniform phase in
   [0, 2 pi)).

A decay d above 1 makes the partial die faster than the body. The ratio may be inharmonic. A part with a <= 0 does
not draw a random phase.

## T7 Pitch-following noise

Inputs: n, rng, centre fc (Hz per sample or a number), q, envelope e.

1. fc = min(fc, 16000), broadcast to n samples.
2. b = time-varying state-variable band-pass (P-SVF, mode band, peak gain q) of n white Gaussian samples at fc, q.
3. live = the samples where e > 0.1 x (max(e) + 1e-12).
4. Scale b so that its RMS over the live samples equals sqrt(0.5) (the RMS of a unit sine). So 0 dB means "as
   strong as the tone".
5. Output = b x e.

## T8 Contact burst

Inputs: n, rng, `lo`, `hi`, `tau`, `rise` = 0.0003, filter order = 2. Output length n, peak 1.

1. k = min(n, N(max(6 tau + rise, 0.002))).
2. w = Butterworth band-pass (order) from max(lo, 20) to min(hi, 21600) of k white Gaussian samples.
3. w x= (1 - exp(-t / max(rise, 1e-5))) x exp(-t / max(tau, 1e-5)).
4. Normalise w to peak 1 and put it in the first k samples of n zeros.

## T9 Soft thump

Inputs: n, `f`, `tau`, `rise` = 0.002. y = sin(2 pi f t) x (1 - exp(-t / max(rise, 1e-4))) x exp(-t / max(tau,
1e-4)), normalised to peak 1. No random draw. The phase starts at 0.

## T10 Struck modes

Inputs: n, rng, freqs, amps, taus, `jitter` = 0, `bend` = 0, `bend_tau` = 0.004.

1. b(t) = 1 + bend x exp(-t / max(bend_tau, 1e-4)). A hard hit makes the body stiffer for some milliseconds, which raises the pitch briefly.
2. For each mode (f, a, tau): skip it when a <= 0 or f >= 21600 (no random draw). Else f' = f x (1 + jitter x g);
   phase = 2 pi cumsum(f' b) / 48000 + uniform [0, 2 pi); y += a exp(-t / max(tau, 1e-5)) sin(phase).

Draw order per mode: the jitter normal, then the phase uniform.

## T11 Strike

Inputs: resonator response y, rng, `contact_s`, `bright` = 0.5. Output = the response convolved with a contact
pulse (P-CONTACT of contact_s s, brightness bright), cut to the length of y (P-EXCITE). The longer the contact
(a soft mallet, a fingertip), the weaker the high modes.

## T12 Small room on stereo

Inputs: stereo y, rng, `wet`, `rt60`, `size` = 0.15, `width` = 0.3. When wet <= 0.005 return y with no rng draw.
Else return the algorithmic reverb (P-VERB) with rt60, size, predelay 0.004, damp 0.6, wet, stereo output, width.
The reverb keeps its own default dry level (1.0).

## T13 Finish (mono to stereo)

Inputs: mono y, rng, `hp` = 80, `lp` = 12000, `room_wet` = 0, `rt60` = 0.15, `width` = 0, `cut` = 500,
`end_db` = -60, `fade` = 0.006, `pan` = 0, `size` = 0.15.

1. Pre-fade the dry end: kd = min(len, N(0.02), floor(0.15 len)). Multiply the last kd samples by cos^2 of a ramp
   0 -> pi/2. The dry signal is about 60 dB down there, but a low body cut inside one of its cycles leaves a step
   that the room rings on and an audit hears as a click.
2. High-pass Butterworth order 4 at hp.
3. When lp < 17500: low-pass Butterworth order 2 at lp. At 17500 or more there is no low-pass.
4. When width > 0.005: decorrelate to stereo (P-DECOR, amount width, decorrelated side only above `cut` Hz). Else
   copy y to both channels.
5. When |pan| > 0.005: theta = (clip(pan, -1, 1) + 1) pi / 4; L x= sqrt(2) cos(theta), R x= sqrt(2) sin(theta).
6. T12 room with room_wet, rt60, size, width 0.3 + width.
7. Trim the tail (P-TRIM) at end_db relative to the peak, keep 0.01 s.
8. Multiply the last min(len, N(fade)) samples by cos^2 of a ramp 0 -> pi/2.
9. Multiply the first min(len, N(0.0003)) samples by a raised-cosine rise 0 -> 1.

## T14 Catalog measurement

Input: a render on silence, mono or stereo. A mono input is duplicated to 2 channels. m = the mean of the 2
channels. These are the same definitions the film catalog used, so a render can be compared with the film numbers.

Level track: frames centred every 1 ms (48 samples), each the mean of m^2 over a 5 ms window (240 samples, clipped
at the edges), then smoothed with a centred 10-frame moving average. ip = the index of the maximum, pk = its value.
All frame indexes are milliseconds.

| Output | Definition |
| --- | --- |
| `dur_s` | (last frame >= pk x 10^-4.5, or n / 48 when none) x 0.001 minus (first frame >= pk x 10^-3) x 0.001, rounded to 3 decimals. This is from the -30 dB onset to the last frame within 45 dB |
| `attack_ms` | ip minus the first frame at or before ip with value >= pk x 10^-2 (within 20 dB), or none |
| `t20_ms` | the first frame index, counted from ip, with value < pk x 10^-2, or none |
| `t40_ms` | the same with pk x 10^-4, or none |
| `centroid_hz` | spectral centroid of the active spectrum, rounded to an integer |
| `lo150` | energy share below 150 Hz, 3 decimals |
| `hi4k` | energy share at or above 4000 Hz, 3 decimals |
| `tonal` | mean, over active frames, of the energy share within a quarter octave of the strongest bin in 80-12000 Hz; none when fewer than 2 frames; 2 decimals |
| `glide_st` | 12 log2(median of the last k3 ridge frequencies / median of the first k3), k3 = max(1, min(3, count / 3)), 1 decimal; none when fewer than 2 |
| `corr` | sum(L R) / sqrt(sum(L^2) sum(R^2)), 2 decimals |

The level values are mean squares, so 10^-2 is 20 dB and 10^-4 is 40 dB.

Spectrum: zero-pad m to at least 2048 samples. Frames of 2048 samples, hop 240, Hann window, power spectrum.
Frame energy = the sum over bins. Active frames: energy >= 1 % of the maximum frame energy. The active spectrum is
the mean power over the active frames. For `tonal` and `glide_st`, take for each active frame (in time order) the
frequency of the strongest bin in 80-12000 Hz (skip a frame whose maximum is 0). The tonal share of that frame is
the energy in [fr / 2^0.25, fr x 2^0.25] over the frame's energy in 80-12000 Hz.

## T15 Target distance and pinned family space

Target distance: input a measurement dict and targets {key: (lo, hi, scale)}. For each key: add 1 when the value is
missing (none); add min(2, (lo - v) / scale) when v < lo; add min(2, (v - hi) / scale) when v > hi. 0 means every
target is met.

Pinned family space: input a full space, a member's full parameter dict and a list of fine keys. Output: for each
key of the space that is also in the parameter dict, the full range when the key is a fine key, else a one-choice
list holding the member's value. Keys missing from the parameter dict are dropped.
