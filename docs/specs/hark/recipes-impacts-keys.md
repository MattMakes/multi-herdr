# recipes-impacts, part keys: keypress, typing

This part belongs to the spec `recipes-impacts.md` (the index). The index defines the recipe contract, the space
notation, the forge-core primitives (`P-*` codes) and the shared rules. The shared UI layer functions L1-L9 are in
part `recipes-impacts-ui.md`, section 1.3.

## 1. Recipe `keypress`

### 1.1 Sound

One press of a mechanical keyboard key with a deep, "thocky" low end, modelled from the physics of the key. It was tuned over 15 rounds against real
recorded keys. The last rounds fixed the tonal balance: the case body moved below 260 Hz (at 500-630 Hz it read as a
boxy "tok", +16 dB), the spring ping at 8-10 kHz was restored, the high shell modes begin above the 4-6 kHz dip of real
keys, the finger push is shorter, and a high-pass of the recording session takes out a 15-45 Hz push. The distance from the real keys'
median band balance went from 8-13 dB to 5.4-5.6 dB (real keys sit 3.1-6.0 dB from each other).

Parts, and what the real keys show:

1. Bottom-out contact at the hit. The stem and keycap slam into the housing and plate in well under 1 ms.
   - A click over the full band. In a spectrogram of a real key the onset is a single vertical line up to 20 kHz,
     and 7-14 kHz has fallen 20 dB about 2 ms later. Model: a wide-band crack and many dense high shell modes, both
     driven by a half-sine contact pulse. The pulse length `click_tc` sets the top of the spectrum. Plus a direct
     pressure snap: the first large, one-sided spike.
   - Keycap and plate modes: a main mode at about 1.25-1.6 kHz and a group of modes up to about 2.3 times it, plus
     band noise for the part of the group whose modes lie too close to separate. The stem stays held down by the finger: the cap
     rings undamped for about 1 ms, then the damping rises and leaves only a residual ring. When the cap rang for more than about 4.5 ms an audio classifier labelled it a dropped coin
     to an audio classifier, so `cap_tau` stops there.
   - The case body (110-260 Hz) has a slightly longer ring.
   The levels of these parts are ratios of energy in the first 5 ms. They are not ratios of peaks.
2. The "thock": a low wave at 58-77 Hz. It peaks 20-35 ms after the hit, beats (with dips near 45
   ms and 85 ms) and decays with a time constant of about 50-60 ms. Physically the board and the desk move on their
   feet (`thock_f`) and beat against the low mode of the case (`case_f`). The finger's smooth push drives them
   (`thock_rise`). A microphone picks up pressure, and pressure follows acceleration, so the slow push itself is
   removed by a high-pass.
3. The release, 70-125 ms after the hit: 2 small high ticks (the stem leaves the housing, the spring leaves its
   seat), `tick_dt` ahead of the top-out. Then the top-out: a contact that is smaller and brighter, with the cap
   modes moved, and a small
   kick to the low wave. The ticks matter: without them a critic score fell by 4.
4. A faint friction swell before the hit, a spaced stereo microphone pair in a small room (without the room a
   typicality score fell to -10: real keys are close-miked but not dry), a noise floor (low rumble plus hiss rolled
   off near 18-20 kHz), and an edit 0.12-0.4 s after the hit with a fade.

The mode layout, the microphone positions and the room shape are properties of the board and the recording session. They come from the `key`
parameter through a fixed per-key rng. Each press varies only force, timing and noise, so repeated presses of one key
match each other.

Version 2 keys (read only when present; a parameter set without them renders the version 1 sound exactly):

| Key | Purpose |
| --- | --- |
| `top_lvl`, `top_lo`, `top_tau` | a wide-band rattle of the keycap at the contact: noise from `top_lo` up to 12 kHz; its energy is set relative to the cap modes |
| `jacket_lvl`, `jacket_dt` | the click jacket of a clicky switch: a bright click 3-12 ms ahead of the bottom-out. 0 = a linear or tactile switch |
| `pre_gap` | silence before the key starts to travel (version 1 drew it at random from 3-20 ms) |
| `width` | narrows the spaced microphone pair toward mono: mid + width x side |

Version 1 problems that version 2 fixed: the finals were clones (an embedding cosine of 0.965 between variants; real
sets 0.67-0.72), the tails were long (time to -40 dB 1.67 x the held-out median), the onset was soft and the top dull
(4-12 kHz at -18 dB of the total; real clicky keys -3 to -14 dB). The `width` fix: without it the pair decorrelated
the 2-12 kHz layer to a correlation of 0.04-0.47, but real keys sit at 0.53-1.0 (median 0.76). A width of 0.2 gives a
correlation of about 0.9-0.98.

| Field | Value |
| --- | --- |
| `TARGET` | `a single mechanical keyboard key press with a crisp click and a short thock` |
| `ALTS` | `a wooden knock`, `a computer mouse click`, `a typewriter key`, `a cork pop` |
| `REFSET` | `typing_key` |
| `REFS` | 6 files under `<REF_ROOT>/keyboard/`: `kb_thock_01.wav` .. `kb_thock_04.wav`, `kb_enter_01.wav`, `kb_space_01.wav` |

The module has no families. Its palette lives in its seed file (part seeds).

### 1.2 Parameters

Ranges marked (refs) were narrowed to the measured span of real keys after a search exploited regions that no real
recording occupies.

| Key | Range | Scale | Meaning |
| --- | --- | --- | --- |
| `click_tc` | 3e-5 - 4e-4 | log | bottom-out contact time (s) |
| `click_tau` | 1.5e-4 - 1e-3 | log | high shell mode decay (refs: 7-14 kHz 20 dB down in about 2 ms) |
| `click_lvl` | 0.1 - 2.0 | log | click energy relative to the cap modes (first 5 ms) |
| `click_lo` | 6500 - 9500 Hz | log | lowest high shell mode |
| `click_tilt` | -3 - 3 dB/oct | lin | tilt of the high shell modes |
| `crack` | 0.2 - 2.0 | log | broadband crack relative to the high modes |
| `snap` | 0 - 1 | lin | direct pressure spike |
| `cap_f` | 900 - 1800 Hz | log | main keycap or plate mode |
| `cap_tau` | 0.0015 - 0.0045 | log | cap mode decay (refs: 1.2-3 kHz 20 dB down in 6-10 ms) |
| `cap_noise` | 0 - 1 | lin | band-noise share against discrete modes |
| `cap_n` | 6 - 30 | int | cluster mode count |
| `cap_cl` | 0.1 - 1.0 | log | level of the mode group below the main cap mode |
| `cap_fast` | 0.0005 - 0.003 | log | time (s) the cap rings freely before the finger damps it |
| `cap_res` | 0.05 - 1.0 | log | residual ring after the clamp (1 = no clamp) |
| `mid_f` | 110 - 260 Hz | log | case body mode |
| `mid_tau` | 0.003 - 0.016 | log | case body decay |
| `mid_lvl` | 0.01 - 0.2 | log | case body energy relative to the cap |
| `thock_f` | 55 - 90 Hz | log | board and desk mode (refs: 58-77 Hz) |
| `thock_tau` | 0.045 - 0.1 | log | low wave decay (refs: about 50-60 ms) |
| `thock_lvl` | 0.05 - 0.8 | log | low wave amplitude relative to the click peak |
| `thock_rise` | 0.001 - 0.025 | log | finger push length (s) |
| `case_f` | 70 - 220 Hz | log | the case's low mode |
| `case_lvl` | 0 - 0.8 | lin | its level |
| `case_dec` | 0.3 - 1.0 | lin | its decay relative to the board mode |
| `up_delay` | 0.07 - 0.125 | lin | top-out time after the hit (refs: 75-120 ms) |
| `up_lvl` | 0.08 - 0.35 | log | top-out level (refs: 10-22 dB under the bottom-out) |
| `up_pitch` | 0.9 - 1.5 | lin | cap mode shift at the top-out |
| `tick_lvl` | 0 - 1 | lin | release ticks level |
| `tick_dt` | 0.004 - 0.025 | log | ticks lead before the top-out |
| `pre_len` | 0.004 - 0.025 | log | key travel before the hit (friction swell) |
| `pre_lvl` | 0.001 - 0.06 | log | friction level |
| `ping_f` | 7500 - 11000 Hz | log | spring ping (refs: an 8-10 kHz line, 10-40 ms) |
| `ping_tau` | 0.004 - 0.03 | log | ping decay |
| `ping_lvl` | 0.04 - 0.3 | lin | ping level |
| `room_wet` | 0 - 0.4 | lin | room share |
| `rt60` | 0.05 - 0.4 | log | room RT60 |
| `room_size` | 0 - 1 | lin | room size |
| `room_damp` | 0.2 - 0.95 | lin | high-frequency damping |
| `room_model` | choice `sparse`, `diffuse` | choice | the small procedural room (P-ROOM: 6 early echoes and a tail) or a diffuse field (K12) |
| `floor_db` | -78 - -55 dB | lin | noise floor relative to the peak (refs: tails at -53 to -77 dB) |
| `rumble` | 0 - 0.7 | lin | low share of the noise floor |
| `clip` | 0.12 - 0.4 | lin | clip length after the hit |
| `hpf` | 36 - 46 Hz | log | session high-pass (order 8) |
| `key` | 0 - 999 | int | the board identity (seeds the fixed layout) |
| `top_lvl` | 0 - 2.5 | lin | v2: keycap rattle energy relative to the cap |
| `top_lo` | 1800 - 4500 Hz | log | v2: rattle band bottom |
| `top_tau` | 0.0006 - 0.004 | log | v2: rattle decay |
| `jacket_lvl` | 0 - 0.9 | lin | v2: click jacket level |
| `jacket_dt` | 0.003 - 0.012 | log | v2: click jacket lead |
| `pre_gap` | 0.0005 - 0.006 | log | v2: lead before the key travel |
| `width` | 0 - 0.6 | lin | v2: side level of the spaced pair |

Defaults. The version 1 defaults (the final of round 15, values rounded to 6 significant digits) are kept as a
constant set for reference and for the seed bank:

| Key | v1 default | Key | v1 default | Key | v1 default |
| --- | --- | --- | --- | --- | --- |
| `click_tc` | 7.62977e-05 | `mid_tau` | 0.00918634 | `tick_dt` | 0.0111911 |
| `click_tau` | 0.000400034 | `mid_lvl` | 0.0902506 | `pre_len` | 0.0128195 |
| `click_lvl` | 0.195527 | `thock_f` | 86.8567 | `pre_lvl` | 0.0207414 |
| `click_lo` | 7323.18 | `thock_tau` | 0.0549253 | `ping_f` | 9083.43 |
| `click_tilt` | -1.25571 | `thock_lvl` | 0.226067 | `ping_tau` | 0.00578368 |
| `crack` | 0.395643 | `thock_rise` | 0.00834742 | `ping_lvl` | 0.122274 |
| `snap` | 0.0 | `case_f` | 70.0 | `room_wet` | 0.261726 |
| `cap_f` | 1489.18 | `case_lvl` | 0.581134 | `rt60` | 0.113685 |
| `cap_tau` | 0.00399449 | `case_dec` | 0.736866 | `room_size` | 0.212985 |
| `cap_noise` | 1.0 | `up_delay` | 0.124575 | `room_damp` | 0.95 |
| `cap_n` | 7 | `up_lvl` | 0.0969741 | `room_model` | `sparse` |
| `cap_cl` | 0.357156 | `up_pitch` | 1.04402 | `floor_db` | -55.0 |
| `cap_fast` | 0.000995191 | `tick_lvl` | 0.581597 | `rumble` | 0.0177183 |
| `cap_res` | 0.749554 | `mid_f` | 205.793 | `clip` | 0.38353 |
| `hpf` | 44.5337 | `key` | 999 | | |

The version 1 defaults have no version 2 keys. The module defaults are the version 1 defaults with these changes:
click_lvl = 0.6, thock_tau = 0.05, clip = 0.24, top_lvl = 0.8, top_lo = 2500, top_tau = 0.0015, jacket_lvl = 0.0,
jacket_dt = 0.008, pre_gap = 0.002, pre_len = 0.006, width = 0.2. It is the version 1 key with a bright top, a brief
edit, and a file whose first sample is the attack.

### 1.3 Building blocks (private to this recipe)

kr(part) is the fixed per-key rng: a new generator seeded with key x 7919 + part x 104729 + 17. Part numbers: 0
bottom-out layout, 1 top-out layout, 5 release ticks, 6 click jacket, 9 microphone pair, 10 and 11 the room of
channel 0 and 1. r is the per-press render rng.

**K1 ring** (freqs, taus, amps, n, rng). Sum of a x exp(-t / d) x sin(2 pi f t + phase), each computed only over its
first min(n, floor(9 d x 48000) + 2) samples. A mode with a = 0 or f >= 0.47 x 48000 is skipped (no draw). The
phase is uniform [0, 2 pi) from the given rng.

**K2 cloud** (f_lo, f_hi, count, tau_lo, tau_hi, n, kr, r, tilt = -3, detune = 0.002). Frequencies: count values
log-uniform in [f_lo, f_hi] from kr, sorted. Amplitudes: 10^(tilt x log2(f / f_lo) / 20) x uniform(0.3, 1) from kr.
Then each frequency is detuned by (1 + detune x g) with g from r. u = log(f / f_lo) / log(f_hi / f_lo), clipped to
[0, 1]. Decay = exp(log tau_lo + u (log tau_hi - log tau_lo)). Return K1 with r. So the layout is fixed per key and
only the detune and the phases vary per press.

**K3 hiss** (lo, hi, tau, n, rng, order = 2). m = min(n, floor(9 tau x 48000) + 2). White noise of m samples,
Butterworth band-pass from lo to min(hi, 21600), multiplied by exp(-t / tau). Zero-padded to n.

**K4 pulse** (tc). k = max(1, round(tc x 48000)) samples of sin(pi (i + 0.5) / k), normalised to a sum of 1 (unit
area). **K4b drive** (ir, tc) = the linear convolution of ir with K4(tc), cut to the length of ir.

**K5 unit** = divide by the peak. **K6 energy** (x, ms = 5) = divide by the RMS of the first max(8, N(ms / 1000))
samples.

**K7 crack** (r, n, tau, tc). m = min(n, max(N(9 tau), 8)). w = white x exp(-t / tau) over m samples, high-passed at
1200 Hz (order 1), zero-padded to n, then K4b drive with tc.

**K8 contact** (p, r, n, part, pitch = 1, bright = 1). A single plastic impact (part 0 = bottom-out, part 1 = top-out).
1. tc = click_tc x exp(0.15 g).
2. hf = K4b(K5(K2(click_lo, 18000, 40, 1.6 click_tau, 0.6 click_tau, n, kr(part), r, tilt = click_tilt)), tc).
   The higher modes decay faster (tau falls from 1.6 to 0.6 x click_tau).
3. crack = K5(K7(r, n, 0.4 click_tau, 0.5 tc)).
4. cf = cap_f x pitch. modes = K1([cf], [1.2 cap_tau], [1]) + cap_cl x K5(K2(1.08 cf, 2.3 cf, cap_n, 0.9 cap_tau,
   0.45 cap_tau, n, kr(part), r)) (default tilt -3).
5. band = K3(0.75 cf, 2.4 cf, 0.8 cap_tau, n).
6. cap = (1 - cap_noise) x K6(modes) + cap_noise x K6(band).
7. Clamp: cap x= cap_res + (1 - cap_res) x exp(-t / cap_fast).
8. cap = K4b(cap, max(tc, 0.00012)).
9. Case body: mf = mid_f x pitch, ratios (1, uniform(1.35, 1.6), uniform(1.9, 2.4)) from kr(part). mid = K1(mf x
   ratios, (mid_tau, 0.7 mid_tau, 0.5 mid_tau), (1, 0.5, 0.3)).
10. Snap: k = max(3, floor(uniform(0.25, 0.6) from kr(part) x 1e-3 x 48000)) samples of a negative Hann window, at
    sample 0.
11. x = bright x click_lvl x K6(K5(hf) + crack_level x crack) + K6(cap) + mid_lvl x K6(mid), where crack_level is
    the `crack` parameter.
12. When top_lvl is present and > 0: top = K4b(K3(top_lo, 12000, top_tau, n, order 2), tc). x += bright x top_lvl x
    K6(top).
13. Return K5(K5(x) + snap_level x snap), where snap_level is the `snap` parameter.

**K9 ping** (p, r, n, pitch_k = 1). f = ping_f x pitch_k. K5(K1([f, 1.013 f, 1.62 f], [ping_tau, 0.8 ping_tau,
0.4 ping_tau], [1, 0.5, 0.1])).

**K10 low wave** (p, r, n).
1. f = thock_f x (1 + 0.01 g).
2. ir = exp(-t / thock_tau) sin(2 pi f t) + case_lvl x exp(-t / (thock_tau x case_dec)) x sin(2 pi case_f t + 0.5).
3. force = a raised-cosine bump (0.5 - 0.5 cos over a full period) of k = max(2, N(thock_rise)) samples, normalised
   to a sum of 1.
4. y = ir convolved with force, cut to n. Return K5(high-pass order 2 at 0.75 x min(f, case_f) of y).

**K11 pre-travel friction** (p, r, n). m = N(pre_len). w = band-pass 700-9000 Hz (order 2) of m white samples. Grit:
for each sample, with probability 0.004 a value 4 x g, else 0 (stick-slip micro clicks), high-passed at 2000 Hz
(order 1). w = (w + grit) x u^2 with u a ramp 0 -> 1 over m. Normalise to peak 1, zero-pad to n. Real keys sit at
-45 to -28 dB in the 5-25 ms before the hit.

**K12 diffuse room** (x, kr, p). The room of a small furnished space, modelled as a diffuse field (sparse echoes comb-filter into a
metallic ring that a classifier heard as a dropped coin).
1. m = N(min(0.6, 1.1 rt60 + 0.03)). w = m white samples from kr.
2. ramp = 0.0015 + 0.008 room_size. onset(t) = (1 - exp(-t / ramp)) for t > 0.0008 s, else 0.
3. 3 bands of w: low-pass 700 Hz (k = 1.0), band-pass 700-4000 Hz (k = 0.75), high-pass 4000 Hz (k = 0.25), each
   order 2. rt_b = max(0.02, rt60 x (1 - room_damp x (1 - k))). ir = sum of band x exp(-6.91 t / rt_b).
4. ir x= onset.
5. Desk reflection: d = floor(uniform(0.8e-3, 2.5e-3) from kr x 48000). ir[d] += 0.35 x sqrt(sum(ir^2)).
6. ir = 0.6 x low-pass 9000 Hz (order 1) of ir + 0.4 x ir.
7. Return x + room_wet x (x convolved with ir, cut to len x) / sqrt(sum(ir^2)).

**K13 spaced pair** (x, p, r). kr = kr(9). For each of 2 channels:
1. ir = zeros of floor(0.007 x 48000) samples. ir[floor(uniform(0, 0.25e-3) x 48000)] = 1.
2. 6 reflections: at floor(uniform(0.3e-3, 6.5e-3) x 48000) add uniform(0.03, 0.12) x a random sign (all from kr).
   Low-pass the reflections at 12 kHz (order 1) and add them to ir.
3. y = x convolved with ir, cut to len x.
4. Room: `diffuse` -> K12(y, kr(10 + c), p). `sparse` -> P-ROOM(y, kr(10 + c), rt60, size = room_size,
   damp = room_damp, wet = room_wet), cut to len x.
Return the 2 channels as stereo.

**K14 noise floor** (n, r, rumble, low_edge = 25). hiss = pink + white noise (P-NOISE, unit RMS each), low-passed
order 4 at uniform(17000, 20000). rum = brown noise band-passed low_edge-120 Hz (order 2). y = (1 - rumble) x hiss /
std(hiss) + 2 rumble x rum / std(rum). Return y / std(y).

### 1.4 Render algorithm

1. v2 = `top_lvl` is present in p.
2. pre = pre_len + (pre_gap x exp(0.2 g) when v2, else uniform(0.003, 0.02)). When v2 and jacket_lvl > 0.005:
   pre += jacket_dt.
3. dur = pre + clip x (1 + 0.05 g). n = N(dur + 0.05). nh = N(0.12).
4. force = exp(0.1 g): this press's strength. Brightness follows force.
5. Place pre_lvl x K11 at pre - pre_len.
6. When v2 and jacket_lvl > 0.005: jk = K4b(K5(K2(2500, 15000, 24, 0.0012, 0.0004, nh, kr(6), r, tilt 0)), 4e-5).
   Place jacket_lvl x jk at max(0, pre - jacket_dt x (1 + 0.06 g)).
7. Place K8(p, r, nh, part 0, bright = force) at pre.
8. Place ping_lvl x K9(p, r, nh) at pre.
9. Place thock_lvl x force x K10(p, r, n - N(pre)) at pre.
10. tu = pre + up_delay x (1 + 0.04 g).
11. When tick_lvl > 0.005: t1 = tu - tick_dt x (1 + 0.08 g). 2 ticks: (offset 0, gain 1) and (offset uniform(0.003,
    0.009), gain uniform(0.4, 1.0)). Each tick = K4b(K5(K2(3000, 14000, 14, 0.0008, 0.0003, nh, kr(5), r)), 5e-5),
    placed at min(t1 + offset, tu - 0.001) with gain x tick_lvl x up_lvl. Both ticks use the same kr(5) layout
    generator in sequence, so the 2nd tick's layout differs from the 1st.
12. Place up_lvl x K8(p, r, nh, part 1, pitch = up_pitch, bright = 1.2) at tu.
13. Place 0.7 x ping_lvl x up_lvl x K9(p, r, nh, pitch_k = 1 + 0.01 g) at tu.
14. Place 0.3 x thock_lvl x up_lvl x K10(p, r, n - N(tu)) at tu.
15. st = K13(x, p, r).
16. When `width` is present: mid = (L + R) / 2, side = (L - R) / 2, L = mid + width x side, R = mid - width x side.
17. High-pass st at 22 Hz (order 2). pk = peak.
18. Each channel += K14(n, r, rumble, low_edge = hpf when present else 25) x pk x 10^(floor_db / 20).
19. When `hpf` is present: high-pass st at hpf, order 8.
20. Cut st to N(dur). Fade in 2 ms, fade out uniform(0.025, 0.05) s.

Output: stereo, length about pre + clip (0.13-0.45 s). Peak is not normalised by the recipe.

### 1.5 Version 2 checks

**Key statistics** (x). m = the channel mean.
- onset_ms: band-pass m at 2-12 kHz (order 4). Energy in dB of 1 ms frames (48 samples, mean square, + 1e-14).
  ip = loudest frame. i20 = the first frame at or before ip within 20 dB of it. onset_ms = ip - i20.
- top_db: 10 log10(energy of the full-signal FFT power at 2000 <= f < 12000 / total + 1e-14), 1 decimal.
- t40_ms: 5 ms frames in dB. j = loudest. The first frame from j at or below e[j] - 40 dB, times 5 ms. When none,
  (frames - j) x 5.

**Key extra penalty** (x) = -0.75 x (min(1, max(0, -5 - top_db) / 6) + min(1, max(0, t40_ms - 200) / 100)). Range
0 to -1.5. It requires a top end that is bright enough (2-12 kHz share at least -5 dB; real keys -11 to -0.3, median -2.8) and
a tail that is not long (t40 at most 200 ms). The onset is reported only, because real clicky keys have 2 onsets
8-35 ms apart. A critic uses this as an extra score term for the `typing_key` category.

### 1.6 Variations

Per press: contact time exp(0.15 g), force exp(0.1 g), clip length 5 %, top-out time 4 %, tick lead 8 %, jacket lead
6 %, pre-gap exp(0.2 g), thock pitch 1 %, mode detune 0.2 %, all phases and noise. Fixed per `key`: the shell mode
layout and amplitudes, the cap cluster, the case ratios, the snap width, the tick and jacket layouts, the microphone
delays and reflections, the room impulse responses.

## 2. Recipe `typing` (one key, and the typed run)

### 2.1 Sound

Typing sounds as motion design uses them: one key tick (as in product launch films) and a full typed run in a single
file. It does not imitate a recorded keyboard. In the film study, designed typing measured as high-passed noise with
no other content. Each key is a burst of noise at 3-12 kHz (spectral ridge 8.6-11 kHz) with almost nothing under 2 kHz (band levels relative to the
total: 2 kHz -28, 4 kHz -13, 8 kHz -7, 16 kHz -14 dB; the bands at and below 1 kHz sit at the noise floor), tilt +12 to +19 dB per octave,
attack 2-5 ms, -20 dB after 11-18 ms, -40 dB after about 60 ms, an end that is convex (it holds and then stops),
mono, 50-80 ms in all. The fine structure: a sharp start with 2 peaks close together (the touch, then the hit), a fall
of about 20 dB within 5-8 ms of the peak, then a long quiet tail (from -25 down to -60 dB over 50 ms) with a few small
bumps of rattle. Some keys add a quiet release tick 40-80 ms after the hit.

Parts:
- Burst: white noise -> an order-6 high-pass at `lowcut` -> a peaking boost at the ridge -> a gentle low-pass at
  `hicut`. Envelope: an attack with a power curve, then a fast decay (`tau1`, about 20 dB) into a faint long tail
  (`tail_db`, `tau2`).
- Touch: an earlier, smaller peak, `touch_dt` ahead of the hit, at `touch_db`.
- Rattle: 1 to 3 small bumps in the tail, positions fixed per `key`.
- Tink: an optional quiet tonal layer, 3 high partials (5-12.5 kHz, set per key), gone within a few ms.
- Release: an optional faint second tick `rel_dt` after the hit, brighter.
- Edit: the file is `length` long, the last `gate` is a smooth gate. Mono (2 identical channels; film keys have a
  correlation of 1.0).

Round 3 keys (read only when present; the defaults hold them at 0, which renders the round 2 key exactly): `grain`
and `grain_rate` replace the smooth hiss with sparse impulses (velvet noise: a crackly key), `tink_glide` glides the
tink partials up to +-12 semitones over about 5 ms (a digital blip with pitch), and `tink_db` goes up to +2 dB (a key that
sounds like glass). These ranges are there for variety: the round 2 family were near clones (embedding cosine 0.926; film keys
0.849).

| Field | Value |
| --- | --- |
| `TARGET` | `a short crisp high-passed digital typing tick for a motion graphics title` |
| `ALTS` | `a mouse click`, `a drop of water`, `a hi-hat`, `a spray can hiss` |
| `REFSET` | `pro_typing_key` |

The module has no families. Its palette lives in its seed file. The key entries and the run entries share 1 seed
file. Their seed-bank context differs by target, so they do not mix.

### 2.2 Parameters (one key)

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `ridge` | 6500 - 13500 Hz | log | 9600 | spectral ridge (refs 8.6-11 kHz) |
| `ridge_db` | 0 - 12 dB | lin | 6.0 | boost at the ridge |
| `ridge_q` | 0.5 - 2.5 | log | 1.0 | ridge Q |
| `lowcut` | 2500 - 5000 Hz | log | 3400 | order-6 high-pass |
| `hicut` | 12000 - 20000 Hz | log | 16000 | order-2 low-pass |
| `att` | 0.0015 - 0.005 | log | 0.003 | attack to the peak (s) |
| `att_curve` | 0.6 - 3.0 | log | 1.2 | attack shape. Below 1 snaps up, above 1 swells |
| `tau1` | 0.0012 - 0.006 | log | 0.0025 | fast drop time constant |
| `tail_db` | -40 - -12 dB | lin | -24.0 | long tail level relative to the peak |
| `tau2` | 0.008 - 0.05 | log | 0.016 | long tail decay |
| `touch_db` | -30 - -4 dB | lin | -12.0 | touch peak relative to the hit |
| `touch_dt` | 0.0015 - 0.008 | log | 0.003 | touch lead (s) |
| `rattle_db` | -40 - -18 dB | lin | -30.0 | tail bumps. At or below -39.5: off |
| `tink_db` | -40 - 2 dB | lin | -32.0 | tonal partials relative to the burst. At or below -39.5: off |
| `rel_db` | -40 - -14 dB | lin | -40.0 | release tick. At or below -39.5: off |
| `rel_dt` | 0.035 - 0.075 | log | 0.05 | release delay after the touch |
| `length` | 0.05 - 0.1 | lin | 0.075 | file length (refs 59-99 ms) |
| `gate` | 0.004 - 0.02 | log | 0.008 | final gate length |
| `key` | 0 - 999 | int | 7 | key identity (fixed layout) |
| `grain` | 0 - 1 | lin | 0.0 | round 3: share of velvet-noise excitation |
| `grain_rate` | 300 - 4000 /s | log | 1500 | round 3: impulses per second |
| `tink_glide` | -12 - 12 st | lin | 0.0 | round 3: tink partial glide |

### 2.3 Building blocks

kr(part) = a new generator seeded with key x 7919 + part x 104729 + 31. Only part 0 is used.

**Y1 designed band** (w, p, ridge = p.ridge, lowcut = p.lowcut). High-pass order 6 at lowcut, peaking EQ (P-PEQ) at
ridge with ridge_db and ridge_q, low-pass order 2 at hicut, then a low-pass order 8 at 18 500 Hz (energy above
20 kHz counts as a defect in the polish checks).

**Y2 burst envelope** (n, att, curve, tau1, tail, tau2). a = max(2, N(att)), k = min(a, n). e[i] = (i / a)^curve for
i < k. For i >= k: td = (i - (k - 1)) / 48000 and e = (1 - tail) exp(-td / tau1) + tail exp(-td / tau2).

### 2.4 One key (mono)

Inputs: p, r, force = 1, ridge_k = 1.

1. L = length x (1 + 0.06 g). Release on when rel_db > -39.5. T_release = touch_dt + rel_dt x (1 + 0.08 g).
2. n = N(max(L, T_release + 0.025 when release is on else 0)).
3. ridge' = ridge x ridge_k x (1 + 0.015 g). The band uses min(ridge', 16000).
4. w = n white samples.
5. When grain > 0: block k = max(1, floor(48000 / grain_rate)). One impulse per block at a random offset in
   [0, k), dropped when beyond n. Each impulse has a random sign and an amplitude uniform(0.3, 1). Scale the
   impulses to unit RMS. w = (1 - grain) w + grain x impulses.
6. K_hit = N(touch_dt x (1 + 0.1 g)). tail = 10^((tail_db + 1.5 g) / 20).
7. Hit envelope from K_hit: Y2(n - K_hit, 0.6 att, att_curve, tau1, tail, tau2), zero before K_hit.
8. Touch envelope from 0: 10^(touch_db / 20) x Y2(n, att, att_curve, 0.7 tau1, 0.3 tail, 0.5 tau2).
9. env = the per-sample maximum of the 2 envelopes.
10. Rattle, when rattle_db > -39.5: count = an integer in [1, 3] from kr. For each bump: tj = K_hit / 48000 +
    uniform(0.012, 0.045) from kr x (1 + 0.1 g). i = N(tj). When i < n - 8: env from i += 10^((rattle_db +
    uniform(-4, 2) from kr) / 20) x exp(-t / uniform(0.001, 0.003) from kr) x min(1, t / 0.0004).
11. y = Y1(w x env, ridge'). The noise is shaped first, then filtered, so the band stays clean under the fast edges.
12. Tink, when tink_db > -39.5: fs = 3 values uniform(5000, 12500) from kr, sorted, x ridge_k. rms = RMS of the
    first N(0.01) samples of y. For each partial (amps 1, 0.6, 0.4; decays: 3 values uniform(0.002, 0.006) from kr):
    f' = f x (1 + 0.003 g). With tink_glide = gl not 0: f_i(t) = min(f' x 2^((gl / 12)(1 - exp(-t / 0.005))),
    17000) and the phase is the running sum. Else phase = 2 pi f' t. Add a x exp(-t / tau) x sin(phase + uniform(0,
    6.28)) from K_hit. Ramp the first 24 samples after K_hit up linearly. y += 10^(tink_db / 20) x tk x rms /
    (RMS of tk over N(0.01) samples from K_hit).
13. Release, when on: i = N(T_release), m = n - i. er = Y2(m, 0.0008, 1, 0.6 tau1, 0.5 tail, 0.4 tau2). rr = Y1(m
    white x er, ridge = min(1.15 ridge', 16000)). y from i += 10^(rel_db / 20) x rr / peak(rr) x peak(y).
14. Edit: g = min(n - 2, N(gate)). end = n when release is on, else N(L). The gain is 1, then a raised-cosine
    fall over the g samples before end, then 0. Cut y at end.
15. Return force x y / peak(y).

### 2.5 Render (one key)

1. p = defaults merged with p. force = exp(0.08 g).
2. y = one key (force). Fade out 1.5 ms (no fade in: the file starts on the first sample).
3. Return [y, y] (stereo, identical channels).

### 2.6 The typed run

A second recipe object in the same module, for a typed run as one file: keys at jittered spacing (62-148 ms in the
reference run, median about 87 ms = 11.5 keys per second), level and timing jitter, no key layout used twice in
succession, and an optional soft low swell that rises under the end. It is forged in `designed` mode against the refset
`pro_typing_run`.

| Field | Value |
| --- | --- |
| `TARGET` | `a quick run of crisp high-passed digital typing ticks for an animated title` |
| `ALTS` | `rain on a window`, `a hi-hat pattern`, `a clock ticking`, `popcorn popping` |
| `REFSET` | `pro_typing_run` |
| module name | the typing module's name (so the forge files it under `typing`) |

Run parameters are the key parameters plus:

| Key | Range | Default | Meaning |
| --- | --- | --- | --- |
| `rate` | 8.5 - 14.0 | 11.5 | keys per second |
| `jit` | 0.12 - 0.35 | 0.22 | timing jitter (sd of the log interval) |
| `lvl_jit` | 1.0 - 4.5 | 2.5 | level jitter (dB sd) |
| `accent` | 0 - 0.35 | 0.15 | share of accented keys (+3 to +5 dB) |
| `gap` | 0 - 0.2 | 0.08 | share of word gaps (1.4-1.7 x the interval) |
| `var` | 0.01 - 0.08 | 0.04 | ridge spread between the keys of the run |
| `dur` | 1.1 - 1.9 | 1.5 | run length (s; reference run 1.55 s) |
| `swell_db` | -60 - -26 | -42.0 | low swell level. At or below -59: off |
| `swell_len` | 0.2 - 0.5 | 0.3 | swell length (s) |

All run ranges are linear. Run defaults = key defaults plus these.

**Run render**:
1. p = run defaults merged with p. med = 1 / rate.
2. Key times: start at t = 0. While t < dur - 0.07: record t; d = med x exp(jit x g); with probability `gap`,
   d x= uniform(1.4, 1.7); t += clip(d, 0.62 med, 1.75 med).
3. n = N(last time + length + 0.06).
4. Layouts: 5 key identities key + 101 j, j = 0..4.
5. For each key time: pick a layout index k uniform in [0, 5). When k equals the previous index, k = (k + 1 +
   uniform integer in [0, 4)) mod 5, so the same layout never plays twice in a row. lvl = 0.6 x lvl_prev + lvl_jit
   x g (a smooth level walk, starting at 0). gain_db = lvl, plus uniform(3, 5) with probability `accent`.
   ridge_k = exp(var x (k - 2) / 2 + 0.3 var x g). Place one key (layout k, force 1, ridge_k) at the key time
   with gain 10^(min(gain_db, 6) / 20).
6. When swell_db > -59: swell length m = N(min(swell_len, last key time)). Swell = brown noise of m samples,
   band-passed 90-900 Hz (order 2), times u^2.5 (u a ramp 0 -> 1), normalised to peak 1, placed so it ends at the
   last sample. y += 10^(swell_db / 20) x peak(y) x swell.
7. Air bed: white noise band-passed from lowcut to 16000 Hz (order 2), divided by its std, scaled to -68 dB re
   peak(y), added. This fills the gaps between keys, which would otherwise be exact zeros.
8. Fade in 2 ms, fade out 4 ms. Return [y, y].

### 2.7 Study checks

Envelope in dB (Y3): 1 ms frames (48 samples) of mean square + 1e-14, in dB, relative to the maximum.

**Key statistics** (x), m = channel mean:
- attack_ms: ip = the loudest Y3 frame; i20 = the first frame at or before ip at or above -20 dB; ip - i20.
- t20_ms, t40_ms: smooth m^2 with a centred moving average of 10 ms (480 samples), take every 48th sample, in dB
  relative to the maximum. From the maximum, the first index at or below -20 (or -40) dB. None when absent.
- lo2k_db: share of the Hann-windowed FFT power below 2000 Hz, dB re total, 1 decimal.
- length_s: length in s, 3 decimals.

Distance helper: outside(v, lo, hi, scale) = 1 when v is none, else max(0, lo - v, v - hi) / scale.

**Key extra penalty** = -min(3, 0.75 x (min(1, outside(attack, 1.5, 5.5, 3)) + min(1, outside(t20, 10, 19, 6)) +
min(1, outside(t40, 42, 72, 25)) + min(1, max(0, lo2k + 30) / 10) + min(1, outside(length, 0.05, 0.1, 0.03)))).

**Run statistics**: onsets are the 1 ms Y3 frames i >= 1 where e[i] > -32 >= e[i - 1]. Keep an onset only when it is
at least 45 frames after the last kept one. Spacing d = differences of the kept onsets (or [0] when fewer than 2).
Output: keys (count), med_ms, min_ms, max_ms, lo2k_db.

**Run extra penalty** = -min(3, 0.75 x (min(1, outside(med, 72, 100, 20)) + min(1, max(0, lo2k + 28) / 10) + 0.5
when keys < 10)).

### 2.8 Variations

Per key: length 6 %, release time 8 %, ridge 1.5 %, touch lead 10 %, tail level 1.5 dB sd, rattle timing 10 %, tink
detune 0.3 %, force exp(0.08 g), noise. Fixed per `key`: rattle count, positions, levels and decays; tink
frequencies and decays. Per run: key spacing, word gaps, layout order, level walk, accents, ridge spread.
