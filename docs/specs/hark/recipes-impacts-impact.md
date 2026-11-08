# recipes-impacts, part impact: the recipe `impact`

This part belongs to the spec `recipes-impacts.md` (the index). The index defines the recipe contract, the space
notation, the forge-core primitives (`P-*` codes) and the shared rules.

## 1. Overview

A deep cinematic boom impact (a trailer hit) and the designed motion-design hit, in one module. 3 generations:

- Version 2 ("v2", rounds 1-8 of the first forge test and the launch film): a physical trailer boom. Kept so its
  finals, seed entries and demo scripts re-render. A parameter set with the key `pre_len` renders with version 2.
- Version 3 ("v3", forge v2 rounds 2-6): 1 render, 2 idioms, each a family of finished presets:
  - `impact`, the cinematic boom against real references. In the references a boom loses 12-18 dB each second,
    linear in dB, for 3-5 s (t40 1.4-2.0 s), and its spectrum stays wide for the first second.
  - `pro_impact`, the designed hit of motion design. Body and sub grow to the peak in 50-290 ms. Most energy is
    below 150 Hz. The tail has 2 parts: grains of rumble (a pulse about each 28 ms, coming less often over time, with
    ticks between 1 and 10 kHz) and a held air band at 5-10 kHz. The tail is decorrelated and stops on a hard gate.
- A stand-alone air riser function (`swell`) used by the launch film.

Module metadata:

| Field | Value |
| --- | --- |
| `TARGET` | `a deep cinematic boom impact` |
| `TARGETS` | `impact` -> `a deep cinematic boom impact`; `pro_impact` -> see section 4.6 |
| `ALTS` | `a kick drum`, `an explosion`, `a door slam`, `thunder` |
| `REFSET` | `impact` |
| `REFS` | `<REF_ROOT>/impact/impact_01.wav` .. `impact_03.wav` |

Constants: the low-rate layers run at LO = 6000 Hz and are up-sampled by 8 to 48 kHz. Membrane mode ratios (Bessel
ratios of a circular membrane): 1.0, 1.594, 2.136, 2.296, 2.653, 2.918.

## 2. Version 2: the physical trailer boom

### 2.1 Physics

- A large, loosely tensioned body, like a drum, hit hard. Because tension changes with amplitude, the fundamental
  begins sharp and falls as the level decays. A few inharmonic overtones of the membrane fade before the
  fundamental.
- The contact: a wide-band crack that lasts from a few ms up to some tens of ms. It is softer than the body, but it
  supplies the high frequencies.
- The rest of the structure: many modes in the low mids (70 Hz up to about 1.5 kHz). The higher a mode, the sooner it
  fades.
- A sub layer: a 30-60 Hz sine that decays slowly.
- Rumble: brown noise through a falling low-pass.
- A whoosh-down: noise through a resonant band that moves downward after the contact.
- A flanged tail: a comb filter with feedback and a drifting delay. Its peaks sit at k / delay and all move down
  together, so the tail holds a set of falling partials.
- Distant echoes: each one comes later, softer and duller than the one before.
- A large hall: the low end rings the longest; diffuse early reflections, a pre-delay, a tail that differs per
  channel.
- A reversed lead-in: the contact's hall tail played backwards, filtered and faded in before the hit.
- An optional air riser before the hit.
- Saturation, as on a mixing console, that acts only on the loud section; then a high-frequency roll-off; last, a
  microphone that adds its own floor of noise.
- Round 8 added the roll: the slow low movement of a large space after the hit, which grows and fades like distant
  thunder. In a blind test a listener had found the impact by its brief tail (in real booms the low movement goes on at about -10 to -14 dB for 2-3 s).

History rules that shaped the model (each one is a constraint for the clean side):
1. A hard-clipped sub gives square-wave harmonics: use soft saturation followed by a roll-off.
2. A single short noise burst (or a short impulse cloud) as the crack prints static stripes through the hall tail.
   Use a smooth pulse plus debris spread over at least 60 ms.
3. A sum of Butterworth band-passes cancels at each crossover by phase (it showed as a dark line at 6 kHz). Split the hall
   decay bands subtractively so they sum back exactly.
4. A striker mode longer than 3 ms prints a thin line at about 2 kHz through the hall.
5. A lone sliding fundamental leaves 100-600 Hz empty. The shell cloud fills it.
6. The roll draws its noise last, so older parameter sets re-render exactly.

### 2.2 Version 2 parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `pre_len` | 0.08 - 1.6 | log | 0.7875 | reverse swell length (s) |
| `pre_level` | 0.02 - 1.0 | log | 0.133 | reverse swell level re the peak |
| `pre_bright` | 200 - 16000 Hz | log | 600 | reverse swell low-pass |
| `pre_low` | 0 - 1 | lin | 0.5775 | share of the low body in the swell source |
| `riser_level` | 0 - 0.6 | lin | 0.0 | air riser level re the peak |
| `riser_f` | 800 - 12000 Hz | log | 3063 | riser end centre |
| `crack_level` | 0.01 - 0.8 | log | 0.72 | contact level |
| `crack_ms` | 0.2 - 3.0 | log | 1.907 | striker mode decay (ms) |
| `crack_f` | 600 - 9000 Hz | log | 1133 | striker mode frequency |
| `contact_ms` | 0.15 - 3.0 | log | 0.6308 | contact pulse length (ms) |
| `crackle` | 0 - 1.5 | lin | 1.035 | debris level |
| `debris_ms` | 60 - 500 | log | 66.93 | debris spread (ms) |
| `thump_f0` | 70 - 400 Hz | log | 141.9 | membrane start pitch |
| `thump_f1` | 22 - 70 Hz | log | 30.16 | membrane end pitch |
| `drop_tau` | 0.006 - 0.3 | log | 0.03454 | pitch slide time constant |
| `thump_tau` | 0.12 - 2.0 | log | 0.2087 | membrane decay |
| `modes_level` | 0 - 0.6 | lin | 0.2633 | membrane overtones level |
| `shell_level` | 0 - 0.8 | lin | 0.2035 | shell cloud level |
| `shell_lo` | 70 - 250 Hz | log | 121.7 | lowest shell mode |
| `shell_span` | 2 - 10 | log | 3.57 | shell span (ratio) |
| `shell_tau` | 0.05 - 1.2 | log | 0.3507 | shell decay at the bottom |
| `sub_f` | 24 - 62 Hz | log | 60.13 | sub frequency |
| `sub_tau` | 0.15 - 3.0 | log | 0.8391 | sub decay |
| `sub_level` | 0 - 1 | lin | 0.0 | sub level |
| `rumble_level` | 0.02 - 1.2 | log | 0.1937 | rumble level |
| `rumble_cut` | 80 - 1500 Hz | log | 103.9 | rumble low-pass |
| `rumble_tau` | 0.3 - 3.5 | log | 1.658 | rumble decay |
| `sweep_level` | 0.005 - 0.5 | log | 0.1123 | whoosh-down level |
| `sweep_hi` | 250 - 8000 Hz | log | 365.9 | whoosh-down start centre |
| `sweep_tau` | 0.08 - 1.2 | log | 0.5692 | whoosh-down time constant |
| `echo_level` | 0 - 0.6 | lin | 0.485 | echo level |
| `echo_delay` | 0.08 - 0.6 | log | 0.1192 | echo spacing |
| `comb_mix` | 0 - 0.8 | lin | 0.1677 | flanged tail mix |
| `comb_d0` | 0.15 - 2.0 ms | log | 0.2453 | comb start delay |
| `comb_ratio` | 1.2 - 6.0 | log | 3.23 | comb delay growth ratio |
| `comb_fb` | 0.2 - 0.85 | lin | 0.7521 | comb feedback |
| `hall_wet` | 0.05 - 1.2 | log | 0.8017 | hall level |
| `hall_rt` | 0.5 - 8.0 | log | 3.897 | hall RT60 |
| `hall_damp` | 0 - 1 | lin | 0.2887 | hall high damping |
| `predelay` | 0 - 0.06 | lin | 0.0 | hall pre-delay |
| `drive` | 0 - 1 | lin | 0.3529 | console saturation |
| `sat_tone` | 1500 - 14000 Hz | log | 8003 | console roll-off |
| `width` | 0 - 1 | lin | 0.3363 | per-channel independence of the diffuse layers |
| `floor_db` | -95 - -66 dB | lin | -88.67 | microphone floor |
| `roll_level` | 0 - 1.5 | lin | 0.4072 | roll level re the peak |
| `roll_cut` | 60 - 400 Hz | log | 141.7 | roll low-pass |
| `roll_hold` | 0.8 - 3.2 | lin | 2.26 | roll hold (s) |
| `roll_fall` | 0.25 - 1.5 | log | 0.5904 | roll fall time constant |
| `roll_rate` | 0.3 - 3.0 Hz | log | 0.6466 | roll swell rate |
| `roll_depth` | 0 - 0.8 | lin | 0.5458 | roll swell depth |

The defaults are the round 8 pick for the launch film. Neutral values that switch the newer layers off (for seeding
from older rounds): comb_mix = 0, riser_level = 0, roll_level = 0. The hit length is TAIL = 5.0 s.

### 2.3 Version 2 building blocks

**I1 IIR noise** (n, rng, colour). White Gaussian. Pink: filter with numerator (0.049922035, -0.095993537,
0.050612699, -0.004408786) and denominator (1, -2.494956002, 2.017265875, -0.522189400) (a standard economy pink
filter: -3 dB per octave within 0.5 dB above about 10 Hz). Brown: a leaky integrator 1 / (1 - 0.997 z^-1). Scale to
unit RMS.

**I2 microphone on stereo** (y, rng, floor_db, low_cut = 20, air_db = -1). High-pass order 1 at low_cut, peaking EQ
at 10 kHz with air_db and Q 0.6, then add to each channel an independent I1 pink noise x peak x 10^(floor_db / 20).

**I3 glide** (t, f_start, f_end, tau, rng) = sin(2 pi (f_end t + (f_start - f_end) tau (1 - exp(-t / max(tau,
1e-4)))) + uniform(-0.3, 0.3)). A closed-form phase of an exponential slide.

**I4 envelope** (t, attack, tau) = exp(-t / max(tau, 1e-4)), and for t < attack multiplied by 0.5 - 0.5 cos(pi t /
attack).

**I5 filters**: Butterworth low-pass and high-pass at a given rate, cutoff clipped to [10, 0.45 x rate].

**I6 up-sample** (x, n): polyphase resampling by 8 (6 kHz -> 48 kHz), cut or zero-padded to n.

**I7 hall impulse response** (rng, rt60, damp, size = 0.8, predelay = 0.02, seconds = min(5.0, 1.1 rt60 + 0.1)).
1. w = white noise of N(seconds).
2. 4 decay times: 1.25 rt60, rt60, rt60 (1 - 0.5 damp), rt60 (1 - 0.8 damp).
3. Split w subtractively at 250, 1500 and 6000 Hz: band k (k = 0, 1, 2) = order-2 low-pass of the remainder at the
   k-th crossover, and the remainder loses that band. Band 3 is the last remainder. The bands sum back to w exactly.
4. tail = sum of band_k x exp(-6.91 t / max(rt_k, 0.05)), times (1 - exp(-t / (0.008 + 0.03 size))) (diffuse
   build-up).
5. 40 early reflections at d = floor(uniform(0.003, 0.02 + 0.08 size) x 48000): add uniform(0.05, 0.2) x random sign
   x exp(-d / 48000 / 0.06).
6. ir = 0.25 tail + low-pass(reflections, 7000 Hz, order 1). Normalise to unit energy. Prepend N(predelay) zeros
   when predelay > 0.

**I8 console saturation** (x, drive, tone). When drive <= 0.001 return x. g = 1 + 7 drive, pk = peak. y = pk x
tanh(g x / pk) / g x (1 + 0.15 drive). Low-pass order 2 at tone. Small signals pass at about unity.

**I9 flange** (x, p). When comb_mix < 0.01 return x. Delay in samples d(t) = comb_d0 / 1000 x 48000 x (1 +
(comb_ratio - 1)(1 - exp(-t / 1.2))). Feedback comb y[i] = x[i] + comb_fb x y[i - d[i]], with linear interpolation
between the 2 neighbour samples, and only when i - d[i] >= 1. y x= (1 - comb_fb). Return x + comb_mix x (y - x).

### 2.4 Version 2 layers

All "low" layers run at 6 kHz on t = k / 6000.

**Body** (membrane). x = I3(t, thump_f0, thump_f1, drop_tau) x I4(t, 0.0015, thump_tau). When modes_level > 0.005:
for overtone j = 0..4 (ratios 1.594 .. 2.918): skip it when thump_f0 x ratio > 2700 Hz; a = modes_level x 0.75^j x
uniform(0.6, 1); x += a x I3(t, thump_f0 x ratio, thump_f1 x ratio, drop_tau) x I4(t, 0.001, thump_tau / (1.5 +
0.8 j)).

**Shell cloud**. lo = shell_lo, hi = min(lo x shell_span, 2520). 28 modes log-uniform in [lo, hi], sorted. u =
log(f / lo) / log(hi / lo). tau = shell_tau x exp(-1.6 u) (top modes ring about 5 x shorter). a = 10^(-4 log2(f /
lo) / 20) x uniform(0.3, 1). x = sum of a sin(2 pi f t + uniform phase) exp(-t / tau), times (1 - exp(-t /
0.0015)). Normalise to peak 1.

**Rumble**. w = I1 brown. c = min(rumble_cut, 2520). a = low-pass(w, c), b = low-pass(w, 0.3 c) (order 2, 6 kHz).
k = exp(-t / max(0.5 rumble_tau, 0.05)). y = k a + 1.4 (1 - k) b. High-pass at 22 Hz. mod = 1 + the sum of 3 terms
0.12 sin(2 pi uniform(1.5, 6) t + uniform phase). y x= mod x I4(t, 0.012, rumble_tau). Normalise.

**Crack** (48 kHz, n samples). m = min(n, N(0.6)).
1. Contact pulse: a full-period raised cosine over k = max(2, N(contact_ms / 1000)) samples.
2. Striker mode over m: 0.8 sin(2 pi crack_f t) exp(-t / (crack_ms / 1000)) (1 - exp(-t / 0.0002)).
3. High-pass the first m samples at 30 Hz (order 1).
4. When crackle > 0.01: td = debris_ms / 1000. count = floor(60 + 3000 td). Times exponential with mean 0.5 td,
   kept below 0.58 s. At each time add g x exp(-time / td) (g standard normal). Band-pass order 2 from 0.45 crack_f to
   min(4 crack_f, 21600). Normalise to peak 1. Add crackle x it.
5. Normalise to peak 1.

**Whoosh-down** (48 kHz). w = I1 pink. fc = 70 + (sweep_hi - 70) exp(-t / sweep_tau). y = P-SVF band(w, fc, Q 2.5)
x I4(t, 0.004, 1.8 sweep_tau). Normalise.

**Air riser** (48 kHz, n samples). u = t / t_last (0 -> 1). fc = 150 x (riser_f / 150)^(u^1.5). y = P-SVF band(I1
pink, fc, Q 1.2) x (exp(3.5 u) - 1) / (exp(3.5) - 1). Normalise. Steepest at the end.

**Roll** (n samples at 48 kHz, made at 6 kHz). m = floor(n x 6000 / 48000) + 8.
1. w = low-pass(I1 brown, roll_cut) + 0.5 low-pass(I1 pink, 1.8 roll_cut), then high-pass 24 Hz (all order 2).
2. k = max(2, floor(6000 / max(roll_rate, 0.05))). lf = low-pass(I1 white of m + k samples, roll_rate) with the first k
   samples dropped, scaled to unit RMS.
3. e = (1 - exp(-t / 0.15)) exp(-t / (2.5 roll_hold)), and for t > roll_hold also times exp(-(t - roll_hold) /
   roll_fall).
4. y = w x e x exp(roll_depth x lf - 0.5 roll_depth^2) (a smooth random log-amplitude with unit mean).
5. Up-sample (I6) to n. Normalise.

**Echoes** (x, p). Zero when echo_level < 0.01. d = 0. For k = 0, 1, 2: d += echo_delay x uniform(0.8, 1.3); stop when
N(d) >= length; add echo_level x 0.6^k x low-pass(x, 4000 x 0.5^k, order 2) delayed by N(d).

### 2.5 Version 2 render

1. pre = pre_len. K_hit = N(5.0). tl = the 6 kHz time base of 5 s.
2. low = Body + sub_level x sin(2 pi sub_f tl + uniform(0, 1)) x I4(tl, 0.02, sub_tau) + shell_level x Shell.
   low = I8(I6(low, K_hit), drive, sat_tone).
3. crack = Crack(K_hit). direct = low + crack_level x crack.
4. R_low = I6(Rumble), W_down = Whoosh-down.
5. 2 hall impulse responses: I7(hall_rt, hall_damp, 0.8, predelay) and I7(hall_rt, hall_damp, 0.8, predelay +
   uniform(0.001, 0.006)).
6. Output length = N(pre) + K_hit + the longer impulse response length. For each channel c:
   - When width > 0.01: rum = (1 - width) R_low + width x a new I6(Rumble), and the same for the whoosh. Else the
     shared ones.
   - dry = direct + rumble_level x rum + sweep_level x swp. dry += Echoes(dry). Fade out the last 0.4 s.
   - Place dry at N(pre). wet = I9(dry convolved with the channel's impulse response). Add hall_wet x wet at N(pre).
7. Reverse swell: m = N(0.2). head = crack[:m] / peak + pre_low x low[:m] / peak. pk = output peak. For each channel:
   IR_lead = I7(max(0.3, 1.6 pre), 0.4, 0.6, 0, seconds = 1.2 pre + 0.25). sw = (head convolved with IR_lead), first N(pre)
   samples, reversed. Low-pass order 2 at pre_bright. Fade in min(0.25 pre, 0.1), fade out 3 ms. Add sw x pre_level x
   pk / peak(sw) to the first N(pre) samples. When riser_level > 0.005 and N(pre) > 64: add riser_level x pk x
   (Air riser over N(pre), faded in 10 ms and out 2 ms).
8. Roll, when roll_level > 0.005 (drawn last): K_roll = min(output length - N(pre), N(roll_hold + 6 roll_fall +
   0.5)). rc = Roll(K_roll). For each channel: rl = (1 - width) rc + width x a new Roll when width > 0.01, else rc.
   Add roll_level x pk x (rl faded out over 0.3 s) at N(pre).
9. I2 microphone (floor_db). Trim the tail at -66 dB keeping 0.05 s. Fade in 2 ms, out 0.25 s.

Output: stereo, about pre_len + 3-6 s. The contact lands at pre_len.

## 3. The stand-alone air riser `swell`

A public function. It renders a riser whose last sample is its loudest, so an edit can cut from it straight into a
hit.

Inputs (defaults): seconds, rng, f_start = 150, f_end = 6000, q = 1.2, curve = 1.5, grow = 3.5, width = 0.6,
room_wet = 0.25, tube = 0.5, tube_lo = 90, tube_hi = 600, flutter = 0.25, rev = 0.5.

Layers:
- Air pulled in: pink noise (I1) through a P-SVF band-pass whose centre rises f = f_start (f_end / f_start)^(u^curve),
  u = t / t_last, while the level grows lvl = (exp(grow u) - 1) / (exp(grow) - 1). The noise band is scaled to unit
  RMS.
- A shortening air column (tube > 0): the same noise through 6 normalised band-passes (P-SVF mode `bandn`, Q 9) at k x
  ft for k = 1..6, ft = tube_lo (tube_hi / tube_lo)^(u^curve), each capped at 19 200 Hz, amplitude 1 / k. The pipe sum
  is scaled to unit RMS and added with weight tube.
- Turbulence (flutter > 0): k = floor(48000 / 12). lf = low-pass at 12 Hz of white noise of n + k samples, first k
  dropped. Multiply by exp(flutter x lf / RMS(lf) - 0.5 flutter^2).
- Multiply by lvl.
- A reverse wash (rev > 0): strike = 0.02 s of zeros with I1 white noise in the first 4 ms. IR_back = I7(1.4 seconds,
  0.45, 0.7, 0, seconds + 0.1).

Per channel: y = (1 - width) x shared layer + width x a new layer, normalised to peak 1. When rev > 0: tail = (strike
convolved with IR_back), first n samples, reversed, low-passed at 0.8 f_end (order 2), normalised, and added as rev x
tail x (0.3 + 0.7 u). Room: ir = I7(0.9, 0.5, 0.6, 0.004) (one for both channels); out = y + room_wet x (y convolved
with ir, first n) / sqrt(sum(ir^2)).

Finish: I2 microphone at -80 dB, fade in 30 ms and out 6 ms, normalise to peak 1. Output stereo.

## 4. Version 3: the current render

### 4.1 Round notes (constraints for the clean side)

- Round 3: the contact got a 5-15 ms sheen above 6 kHz. Crack and air shock (1-4 kHz) went up and sub and body went
  down. 6 cinematic members. `pre_shape` 1 = a riser that climbs linearly in dB, starting at -34 dB. `pro_impact` got a
  straight-in-dB fall into the gate and sparser, sharper high grains.
- Round 4: the sheen was too strong, so it got quieter, longer and band-limited (`sheen_top`). The crack got darker,
  the whole hit got a 2nd-order low-pass (`top`), and new kinds of boom came in: a struck metal plate and falling debris. `pro_impact`
  got random grain timing (`grain_jit`).
- Round 6: `pro_impact` got a designed boom body (a sine that falls from about 95-140 Hz to 36-52 Hz and draws
  no random numbers), a rumble that starts shortly after it (`roll_in`), and grains at about 36 per second, close
  to the film.
- Every new key has a neutral value that reproduces the previous round, and a new layer draws random numbers only
  when it is used. So the older families re-render exactly.
- A family member may only set keys that are in the space. The forge's final scoring pass maps each member through
  the space, and a key missing from the space takes the first member's value.

### 4.2 Version 3 parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `pre` | 0 - 1.2 | lin | 0.35 | swell before the contact (s). At or below 0.02: none |
| `pre_level` | 0.02 - 0.7 | log | 0.12 | swell peak re the hit |
| `crack` | 0.03 - 2.0 | log | 0.35 | crack level re the body peak |
| `crack_f` | 900 - 7000 Hz | log | 2200 | crack centre |
| `crack_ms` | 2 - 18 | log | 6.0 | crack length (ms) |
| `snap` | 0 - 1.5 | lin | 0.0 | a 1.5 ms snap above 5 kHz on the contact |
| `sheen` | 0 - 2 | lin | 0.0 | round 3: 5-15 ms burst above 6 kHz on the contact |
| `sheen_ms` | 4 - 16 | log | 9.0 | sheen length (ms) |
| `sheen_f` | 4500 - 9000 Hz | log | 6000 | sheen high-pass |
| `pre_shape` | choice {0, 1} | choice | 0 | 1 = straight-in-dB swell from -34 dB |
| `debris` | 0 - 1 | lin | 0.4 | debris crackle (spread 40-250 ms) |
| `blast` | 0 - 1 | lin | 0.5 | air shock level |
| `blast_f` | 250 - 3000 Hz | log | 900 | air shock centre |
| `blast_tau` | 0.03 - 0.8 | log | 0.25 | air shock decay |
| `body_f0` | 70 - 230 Hz | log | 140 | membrane start pitch |
| `body_f1` | 34 - 75 Hz | log | 46 | membrane end pitch |
| `drop_tau` | 0.015 - 0.14 | log | 0.05 | pitch slide time constant |
| `body_tau` | 0.08 - 0.7 | log | 0.32 | membrane decay |
| `body_attack` | 0.001 - 0.3 | log | 0.004 | rise of body and sub to their peak |
| `modes` | 0 - 0.6 | lin | 0.25 | membrane overtones |
| `body` | 0.1 - 1.2 | log | 1.0 | body level; the other levels are scaled to a body peak of 1 |
| `drive` | 0 - 1 | lin | 0.35 | body saturation |
| `sub` | 0 - 1.2 | lin | 0.7 | sub level |
| `sub_f` | 28 - 60 Hz | log | 40 | sub landing pitch |
| `sub_semis` | 0 - 24 st | lin | 12.0 | sub fall onto sub_f |
| `sub_tau` | 0.2 - 2.2 | log | 1.1 | sub decay |
| `shell` | 0 - 0.8 | lin | 0.3 | low-mid shell |
| `shell_lo` | 90 - 260 Hz | log | 130 | shell bottom |
| `roll` | 0 - 1.5 | lin | 0.6 | roll level |
| `roll_cut` | 55 - 320 Hz | log | 140 | roll low-pass (order 4) |
| `roll_hold` | 0.2 - 3.0 | lin | 1.6 | roll hold |
| `roll_fall` | 0.12 - 1.6 | log | 0.6 | roll fall |
| `roll_rate` | 0.3 - 3.0 Hz | log | 0.7 | roll swell rate |
| `roll_depth` | 0 - 0.8 | lin | 0.45 | roll swell depth |
| `grain_rate` | 18 - 48 /s | log | 30.0 | granular pulses per second |
| `grain_depth` | 0 - 1 | lin | 0.0 | granular depth |
| `air` | 0 - 1 | lin | 0.0 | sustained high air band |
| `air_f` | 2500 - 11000 Hz | log | 7000 | air band centre |
| `air_div` | 0.25 - 1.0 | lin | 1.0 | round 3: high grains run at grain_rate x air_div |
| `grain_sharp` | 2 - 6 | lin | 3.0 | round 3: exponent of the high grain envelope |
| `air_q` | 0.5 - 3.0 | lin | 0.8 | round 3: air band Q |
| `hall_wet` | 0.03 - 1.0 | log | 0.45 | hall level. At or below 0.031: no hall |
| `hall_rt` | 0.4 - 6.0 | log | 3.2 | hall RT60 |
| `hall_damp` | 0 - 1 | lin | 0.4 | hall high damping |
| `hall_tone` | 3000 - 12000 Hz | log | 5000 | low-pass on the signal sent into the hall |
| `width` | 0 - 1 | lin | 0.55 | decorrelation of the diffuse layers |
| `gate` | 0 - 1.6 | lin | 0.0 | 0 = let it ring out; above 0 = stop it with a hard gate at this time (s) after the contact |
| `gate_ms` | 6 - 40 | lin | 20.0 | gate fade (ms) |
| `gate_fall` | 0.08 - 0.6 | log | 0.25 | round 3: straight-in-dB fall before the gate (s) |
| `gate_db` | 0 - 30 dB | lin | 0.0 | round 3: depth of that fall |
| `floor_db` | -110 - -70 dB | lin | -90.0 | microphone floor. At or below -105: none |
| `sheen_top` | 7000 - 20000 Hz | log | 20000 | round 4: sheen low-pass. At or above 19000: none |
| `top` | 6000 - 20000 Hz | log | 20000 | round 4: 2nd-order low-pass on the whole hit. At or above 19000: none |
| `plate` | 0 - 1 | lin | 0.0 | round 4: struck metal plate |
| `plate_f` | 150 - 900 Hz | log | 400 | plate lowest mode |
| `plate_tau` | 0.2 - 2.0 | log | 0.8 | plate lowest mode decay |
| `scatter` | 0 - 1 | lin | 0.0 | round 4: falling debris |
| `scatter_s` | 0.3 - 2.5 | lin | 1.2 | debris duration |
| `grain_jit` | 0 - 1 | lin | 0.0 | round 4: random grain timing and level |
| `boom` | 0 - 2.5 | lin | 0.0 | round 6: designed boom body re the body peak |
| `boom_f0` | 80 - 260 Hz | log | 150 | boom start pitch |
| `boom_f1` | 34 - 70 Hz | log | 48 | boom end pitch |
| `boom_drop` | 0.015 - 0.15 | log | 0.045 | boom pitch time constant |
| `boom_tau` | 0.08 - 0.9 | log | 0.3 | boom decay |
| `boom_att` | 0.002 - 0.12 | log | 0.01 | boom rise |
| `boom_drive` | 0 - 1 | lin | 0.3 | boom tanh drive |
| `roll_in` | 0 - 0.4 | lin | 0.0 | round 6: delay (s) from the contact to the start of the rumble |

The defaults are the cinematic boom, family `impact`, member `classic` (round 2).

### 4.3 Version 3 building blocks

All "low" layers run at 6 kHz on tl = k / 6000 and are up-sampled with I6. All others run at 48 kHz.

**J1 rise** (t, a) = 1, and for t < a: 0.5 - 0.5 cos(pi t / a).

**J2 crack** (n). m = min(n, N(0.4)).
1. A full-period raised-cosine pulse over k = max(2, N(0.0006)) samples.
2. L = min(m, max(8, N(crack_ms / 1000))). u = i / L. w = white x (1 - u)^3 x (1 - exp(-i / (48000 x 0.0003))).
   Band-pass order 2 from 0.5 crack_f to min(3 crack_f, 21600). Add 1.4 x w / peak(w).
3. When debris > 0.01: td = 0.04 + 0.2 debris. count = floor(80 + 2500 td). Times exponential with mean 0.45 td,
   kept below 0.38 s. Add g x exp(-time / td) at each time (index clipped to m - 1). Band-pass order 2 from 0.35
   crack_f to min(4 crack_f, 21600). Add 0.5 x debris x it / peak.
4. High-pass at 40 Hz (order 1). Normalise. Zero-pad to n.

**J3 snap** (n). L = N(0.0015). white x (1 - u)^4, high-pass 5000 Hz order 2, normalised. Zero-padded.

**J4 sheen** (n), stereo. L = max(16, N(sheen_ms / 1000)). env = (1 - u)^2 x (1 - exp(-i / (48000 x 0.0002))). A
shared white noise c. Per channel: (0.7 c + 0.3 new white) x env, high-pass order 4 at sheen_f; when sheen_top <
19000 also low-pass order 2 at sheen_top. Normalise jointly to peak 1.

**J5 plate** (n), stereo. A struck metal plate (a big plate's nonlinear "bwang").
1. m = min(n, N(min(4.0, 5 plate_tau + 0.1))).
2. 40 modes: f = plate_f x exp(uniform(0, log(min(16, 9000 / plate_f)))), sorted. a = (f / plate_f)^-0.6 x
   uniform(0.3, 1). tau = plate_tau x (f / plate_f)^-0.55. Phases: uniform per mode and channel.
3. glide(t) = 0.0012 x (1 - exp(-t / 0.1)) (the phase integral of a 1.2 % pitch excess that decays in 0.1 s).
4. on = 1 - exp(-t / 0.0008). Channel c = sum of a exp(-t / tau) on sin(2 pi f (t + glide) + phase_c).
5. Normalise jointly.

**J6 scatter** (n), stereo, independent per channel. T = scatter_s. L = min(n, N(T + 0.25)).
1. count = floor(60 + 260 T). Times = 0.03 + exponential(mean 0.4 T), kept below T.
2. Amplitudes g x exp(-time / (0.5 T)) x uniform(0.2, 1)^2, added at the times (index clipped to L - 1).
3. Convolve with a 4 ms grain: white noise x exp(-i / (48000 x 0.0012)).
4. hi = band-pass 700-6000 Hz, lo = band-pass 250-1400 Hz (order 2). Channel = hi / peak + 0.6 lo / peak.
5. Normalise jointly.

**J7 body** (tl). a = body_attack. late = max(t - a, 0). x = I3(t, body_f0, body_f1, drop_tau) x exp(-late /
body_tau). When modes > 0.005: for overtone j = 0..4 add modes x 0.75^j x uniform(0.6, 1) x I3(t, body_f0 x ratio,
body_f1 x ratio, drop_tau) x exp(-late / (body_tau / (1.5 + 0.8 j))). (No Nyquist skip here.) x x= J1(t, a). When
drive > 0.01: g = 1 + 6 drive, pk = peak, x = pk tanh(g x / pk) / tanh(g), then low-pass order 2 at min(2700, 900 +
2500 (1 - drive)) at the 6 kHz rate. Normalise.

**J8 sub** (tl). f1 = sub_f, f0 = f1 x 2^(sub_semis / 12). a = max(body_attack, 0.008). I3(t, f0, f1, 0.09) x J1(t,
a) x exp(-max(t - a, 0) / sub_tau).

**J9 grains** (t, rate, depth, rng, slow = 0.18, jit = 0). A gain curve around 1. When depth <= 0.01 return ones.
1. g = zeros. tk = 0. While tk < the last time:
   - sp = (1 + slow x min(tk / 1.2, 1.5)) / rate (the spacing grows: the rumble slows down).
   - i0 = max(0, floor((tk + uniform(-0.12, 0.12) x sp) x 48000)). L = min(remaining, floor(1.6 sp x 48000)).
   - When L > 8: add uniform(0.55 - 0.4 jit, 1.0) x (1 - exp(-u / 0.0025)) x exp(-u / (0.32 sp)) from i0.
   - tk += sp x (1 + jit x uniform(-0.6, 0.9)) when jit > 0, else sp.
2. g /= (95th percentile of g + 1e-9).
3. Return (1 - depth) + depth x clip(g, 0, 1.6).

**J10 roll** (n). As the version 2 roll, with these changes: the low-passes are order 4 (brown at roll_cut, pink
at 1.4 roll_cut), the high-pass is 26 Hz, the build-up time constant is 0.08 s. After the up-sampling, multiply by
J9(t, grain_rate, grain_depth, jit = grain_jit). Normalise.

**J11 air** (n, a). y = P-SVF normalised band (`bandn`) of I1 pink at air_f, Q air_q. e = J1(t, max(a, 0.02)) x
exp(-max(t - a, 0) / max(0.25, 0.5 roll_hold + roll_fall)). When grain_depth > 0.01: e x= J9(t, grain_rate x
air_div, min(1, 1.5 grain_depth), jit = grain_jit)^grain_sharp. Return y e / peak(y e).

**J12 boom** (tl). No rng draw. td = max(boom_drop, 0.001). phase = 2 pi (boom_f1 t + (boom_f0 - boom_f1) td (1 -
exp(-t / td))). x = sin(phase) x J1(t, boom_att) x exp(-max(t - boom_att, 0) / boom_tau). When boom_drive > 0.01:
g = 1 + 8 boom_drive, x = tanh(g x) / tanh(g), low-pass order 2 at 1100 Hz (6 kHz rate). Normalise.

**J13 blast** (n). The air shock: fc = blast_f x (0.5 + 0.5 exp(-t / max(blast_tau, 0.02))) (the band sinks an
octave). y = P-SVF `bandn` of I1 pink at fc, Q 0.55 (about 2 octaves wide), times (1 - exp(-t / 0.002)) exp(-t /
blast_tau). Normalise.

**J14 gate curve** (n, at, ms). Ones. From i = N(at), when i < n: a raised-cosine fall to 0 over max(8, N(ms /
1000)) samples (cut at n), then zeros.

### 4.4 Version 3 render

1. When the key `pre_len` is present: render version 2 (section 2.5).
2. p = defaults merged with p. ring = roll_hold + 5 roll_fall + 0.4. T_hit = min(7.0, max(ring, 3 sub_tau + 0.3,
   1.2 hall_rt + 0.3)). When gate > 0: T_hit = min(T_hit, gate + gate_ms / 1000 + 0.01). n = N(T_hit).
3. tl = 6 kHz time base of floor(6000 T_hit) + 8 samples. a = body_attack.
4. body = I6(J7). sub = I6(J8). shell = I6(version 2 Shell with shell_lo, span 4.5, tau 0.3, times J1(tl, a)) when
   shell > 0.01, else zeros. crack = J2.
5. direct = body_level x body + 0.6 shell_level x shell + crack_level x crack (the keys `body`, `shell`, `crack`).
6. When boom > 0.005: direct += boom x I6(J12). When snap > 0.005: direct += snap x J3.
7. Optional stereo layers, each drawn only when used: sheen J4 (sheen > 0.005), plate J5 (plate > 0.005), scatter J6
   (scatter > 0.005). In this order.
8. Diffuse shared layers: rc = J10 when roll > 0.01; ac = J11(a) when air > 0.005; bc = J13 when blast > 0.01.
9. Hall on when hall_wet > 0.031: 2 impulse responses I7(hall_rt, hall_damp, 0.8, 0.012) and I7(hall_rt, hall_damp,
   0.8, 0.012 + uniform(0.001, 0.005)).
10. K_lead = N(pre) when pre > 0.02, else 0. Output length = K_lead + n + (the longer IR length when the hall is on and
    gate <= 0, else 0).
11. P_ref = 1.25 (the levels assume a body peak of 1 plus the normal contact). For each channel c:
    - dry = direct + sheen x J4_c + plate x J5_c + scatter x P_ref x J6_c (each when present).
    - Roll: rl = (1 - width) rc + width x a new J10 when width > 0.01, else rc. When roll_in > 0.005: rl x= J1(t,
      roll_in). dry += roll x P_ref x rl.
    - Air: the same mix with a new J11; dry += air x P_ref x al.
    - Blast: the same mix with a new J13; dry += blast x P_ref x bl.
    - Add (dry + sub x P_ref x sub) at K_lead, faded out over 0.3 T_hit when gate <= 0 (no fade when gated).
    - When the hall is on: the hall hears direct + 0.5 (dry - direct), low-passed at hall_tone (order 1) and
      high-passed at 110 Hz (order 2). In a long hall a low tone would ring as a constant line, so the roll
      carries the low tail instead. Convolve with the channel IR and add hall_wet x wet at K_lead (cut to the output).
12. Swell, when K_lead > 64: m = N(0.15). head = crack[:m] + 0.6 body[:m]. pk = output peak. Per channel: IR_lead =
    I7(max(0.4, 1.8 pre), 0.4, 0.6, 0, seconds = 1.2 pre + 0.2). sw = (head convolved with IR_lead), first K_lead,
    reversed, low-passed order 2 at 2500 Hz.
    - pre_shape 1: ev = sqrt of a centred 480-sample moving average of sw^2 + 1e-9. sw x= 10^((-34 + 34 v^1.3) / 20)
      / (ev / max(ev)), with v a ramp 0 -> 1 over K_lead. Then low-pass order 2 at 4000 Hz. The reversed hall's own rise
      is flattened into a straight-in-dB rise from -34 dB, so the swell starts audibly.
    - pre_shape 0: sw x= v^1.5.
    - Fade in 10 ms, out 2 ms. Add sw x pre_level x pk / peak(sw) to the first K_lead samples.
13. High-pass the whole output at 24 Hz (order 2). When top < 19000: low-pass order 2 at top.
14. When floor_db > -105: I2 microphone (floor_db, low_cut 22, air_db -0.5).
15. When gate > 0 (the gate comes last, after the filters, so no filter ringing leaves a tail):
    - When gate_db > 0.1: gf = max(0.02, min(gate_fall, 0.8 gate)). From the contact, multiply by 10^(-gate_db x
      clip((t - (gate - gf)) / gf, 0, 1) / 20).
    - Multiply from the contact by J14(gate, gate_ms). Append 20 ms of silence.
    - Fade in 1 ms.
16. When gate <= 0: trim the tail at -70 dB keeping 0.05 s, fade in 1 ms and out min(0.35, 0.1 x length in s).
17. Normalise to a peak of 0.89.

Output: stereo. The contact lands at `pre` (K_lead). Length: gated about pre + gate + 0.05 s, else up to pre + 7 s
plus the hall.

### 4.5 Version 3 families

A family is a list of (name, full parameter set) pairs. Each member is a finished sound. The forge searches around
each member (4.6). The members are built from named base sets. Merges apply left to right: a later set overrides an
earlier one.

Base sets:

| Base | Definition |
| --- | --- |
| BP1 | a full designed-hit set, NOT built on the defaults: pre=0, pre_level=0.1, crack=0.3, crack_f=3000, crack_ms=4, debris=0.1, blast=0.05, blast_f=300, blast_tau=0.15, body_f0=120, body_f1=75, drop_tau=0.05, body_tau=0.18, body_attack=0.1, modes=0.1, body=0.35, drive=0.15, sub=0.15, sub_f=48, sub_semis=7, sub_tau=0.5, shell=0.1, shell_lo=110, roll=1.5, roll_cut=200, roll_hold=0.7, roll_fall=0.5, roll_rate=1.5, roll_depth=0.4, grain_rate=36, grain_depth=1.0, air=0.2, air_f=4500, hall_wet=0.04, hall_rt=0.6, hall_damp=0.4, width=1.0, gate=1.1, gate_ms=15, floor_db=-110 |
| BC1 | defaults + sub=0.35, sub_f=42, sub_tau=0.7, body_f0=160, body_f1=62, body_tau=0.25, shell=0.8, shell_lo=110, crack=0.8, crack_f=1500, crack_ms=10, debris=0.7, blast=0.8, blast_f=600, blast_tau=0.3, hall_wet=0.8, roll=0.45, roll_cut=200, pre=0, snap=0.6, hall_tone=9000, roll_depth=0.22, roll_rate=0.5 |
| BC3 | BC1 + blast=1.0, blast_f=1200, sub=0.2, body=0.7, crack=1.4, crack_f=2500, roll=0.35, sheen=1.4, sheen_ms=7, sheen_f=6000 |
| BP3 | BP1 + air=0.7, air_f=8500, air_q=1.6, gate_db=15, gate_fall=0.25, gate_ms=8, air_div=0.5, grain_sharp=4.0 |
| BC4 | BC3 + sheen=0.3, sheen_ms=14, sheen_top=11000, top=13000, crack_f=1600, crack=0.95 |
| BP4 | defaults + BP3 + grain_jit=0.7, crack_f=1800, snap=0, sheen=0 |
| BG(rate) | grain_rate=rate, grain_jit=0.5, boom_drive=0.1 |

Because BP1 is not built on the defaults, a BP1 or BP3 member lacks the keys snap, sheen, sheen_ms, sheen_f,
hall_tone and pre_shape. The render fills them from the defaults. The forge's final pass instead filled missing
space keys at mid-range, which gave the round 3 finals a bright onset click. Round 4 members are complete sets.

Current family `impact` (round 4). Cinematic booms with a straight-in-dB decay of 12-18 dB per second over 3-5 s.

| Member | Definition |
| --- | --- |
| `classic` | BC4 |
| `deep` | BC4 + body_f0=95, body_f1=38, body=0.8, sub=0.55, sub_f=34, sub_tau=1.0, roll=0.55, roll_cut=110, roll_hold=2.1, crack=1.1, crack_f=1500, blast=0.9, blast_f=600, blast_tau=0.45, shell=0.7, hall_rt=4.5, hall_damp=0.6, hall_tone=4000, sheen=0.45, sheen_ms=16, top=11000 |
| `punch` | BC4 + body_f0=230, body_f1=66, drop_tau=0.03, body_tau=0.22, crack=0.95, modes=0.5, shell=0.5, roll=0.25, crack_f=1800, crack_ms=7, blast_tau=0.12, roll_hold=0.9, roll_fall=0.4, hall_rt=2.1, hall_wet=0.45, sub=0.15, sub_tau=0.45, sheen=0.35, sheen_ms=10 (a big struck drum, drier) |
| `metal` | BC4 + shell=0.8, shell_lo=260, modes=0.5, crack_f=2200, debris=0.4, air=0, air_f=6500, hall_damp=0.35, hall_rt=3.6, blast_f=2000, sheen=0.35, sheen_ms=12, sheen_f=7000, crack=0.95, body_attack=0.001, hall_tone=5500, plate=0.7, plate_f=380, plate_tau=0.9, top=11000 |
| `swell` | BC4 + pre=0.5, pre_level=0.35, pre_shape=1, body_attack=0.03, roll_hold=2.0, sheen=0.3, roll=0.45, hall_rt=3.8 |
| `blast` | BC4 + body=0.5, body_f0=130, body_f1=48, debris=0.6, crack_f=1400, blast=1.0, blast_f=900, blast_tau=0.5, roll=0.7, roll_hold=1.9, roll_depth=0.35, roll_rate=1.2, roll_cut=260, sheen=0.3, sheen_ms=12, crack=1.1, hall_rt=3.4, scatter=0.5, scatter_s=2.0 |

Round 3 family `pro_impact` (kept as the round 3 constant; the round 4 members start from it):

| Member | Definition |
| --- | --- |
| `rumble` | BP3 |
| `air` | BP3 + gate=0.6, body_attack=0.25, air=0.9, air_f=9000, grain_depth=0.5, roll=1.0, roll_hold=0.5, sub=0.35, gate_db=18, gate_fall=0.15, air_div=1.0, grain_sharp=3.0 |
| `short` | BP3 + gate=0.65, body_attack=0.05, crack=0.5, grain_rate=42, roll_hold=0.4, roll_fall=0.3, body=0.5, gate_db=14, gate_fall=0.2 |
| `deep` | BP3 + gate=1.3, sub=0.6, sub_f=36, body_f1=62, grain_rate=28, roll_cut=150, roll_hold=0.9, air=0.6, gate_db=16, gate_fall=0.25 |
| `swell` | BP3 + pre=0.18, pre_level=0.3, body_attack=0.2, gate=0.9, grain_rate=32 |

Round 4 family `pro_impact` = BP4 + the round 3 member + the overrides below. Every member also gets pre_shape=1,
hall_tone=6000, sheen_ms=8, sheen_f=6363.961 and crack_f=1800.

| Member | Round 4 overrides |
| --- | --- |
| `rumble` | crack=0.5122, body_f1=39.5286, body_attack=0.1351, body=0.1221, sub=0.0082, roll=1.264, roll_cut=101.7411, roll_hold=0.6747, roll_fall=0.3832, grain_rate=22.7054, grain_depth=0.9699, air=0.4949, gate=0.955, gate_db=20.3501 |
| `air` | crack=0.3841, body_f1=42.8117, body_attack=0.0444, body=0.3126, sub=0.6875, roll=1.4018, roll_cut=255.8402, roll_hold=0.7123, roll_fall=0.4128, grain_rate=22.3646, grain_depth=0.8043, air=0.6265, gate=0.8096, gate_db=15.4599 |
| `short` | crack=0.4808, body_f1=75.0, body_attack=0.0464, body=0.5303, sub=0.1124, roll=1.4651, roll_cut=194.2516, roll_hold=0.4826, roll_fall=0.2785, grain_rate=39.6209, grain_depth=1.0, air=0.6444, gate=0.6288, gate_db=14.1698 |
| `deep` | crack=0.1025, body_f1=50.8261, body_attack=0.0534, body=0.2825, sub=0.4785, roll=1.3857, roll_cut=109.0885, roll_hold=0.3807, roll_fall=0.601, grain_rate=18.6677, grain_depth=0.6976, air=0.7944, gate=1.2768, gate_db=16.8876 |
| `swell` | crack=0.3, body_f1=75.0, body_attack=0.2, body=0.35, sub=0.15, roll=1.5, roll_cut=200, roll_hold=0.7, roll_fall=0.5, grain_rate=32, grain_depth=1.0, air=0.7, gate=0.9, gate_db=15 |

These are the searched parameters of the round 3 finals with the fill-bug keys put right.

Current family `pro_impact` (round 6) = the round 4 member + these overrides:

| Member | Round 6 overrides |
| --- | --- |
| `rumble` | boom=0.6, boom_f0=120, boom_f1=42, boom_drop=0.07, boom_tau=0.3, boom_att=0.07, roll_in=0.03, crack=0.7, BG(40) |
| `air` | boom=0.55, boom_f0=130, boom_f1=46, boom_drop=0.06, boom_tau=0.25, boom_att=0.08, roll_in=0.03, BG(38) |
| `short` | boom=0.55, boom_f0=140, boom_f1=52, boom_drop=0.05, boom_tau=0.2, boom_att=0.06, roll_in=0.03, BG(42) |
| `deep` | boom=0.65, boom_f0=95, boom_f1=36, boom_drop=0.1, boom_tau=0.3, boom_att=0.08, roll_in=0.04, gate_db=12, air=0.45, BG(34) |
| `swell` | boom=0.5, boom_f0=110, boom_f1=44, boom_drop=0.08, boom_tau=0.35, boom_att=0.12, roll_in=0.06, BG(36) |

The boom rises over 60-120 ms so the loudest point still lands 50-290 ms in. It stays in the centre at 0.5-0.65 of
the body scale, so the mix as a whole stays decorrelated (correlation below 0.7; the film tails are about 0-0.2). With
grain_jit 0.5 the mean grain step is 1.075 x the base step.

Historical families (constants kept for re-rendering): round 2 `impact` and round 2 `pro_impact`, round 3 (both
categories as they were after round 3: `impact` with BC3 members, `pro_impact` with BP3 members), round 4
`pro_impact`.

Round 2 `impact`:

| Member | Definition |
| --- | --- |
| `classic` | BC1 |
| `deep` | BC1 + body_f0=110, body_f1=44, sub=0.6, sub_f=34, sub_tau=1.0, roll=0.6, roll_cut=140, roll_hold=2.4, crack=0.4, crack_f=1100, blast_f=400, blast_tau=0.45, hall_rt=4.5, hall_damp=0.6 |
| `punch` | BC1 + body_f0=210, body_f1=58, drop_tau=0.03, body_tau=0.18, crack=1.0, crack_f=2600, crack_ms=7, blast_tau=0.15, roll_hold=0.9, roll_fall=0.4, hall_rt=2.0, hall_wet=0.6, sub_tau=0.45 |
| `metal` | BC1 + shell=0.8, shell_lo=210, modes=0.5, crack_f=4000, debris=1.0, air=0.08, air_f=6500, hall_damp=0.1, hall_rt=3.6, blast_f=1200 |
| `swell` | BC1 + pre=0.9, pre_level=0.35, body_attack=0.03, roll_hold=2.0, roll=0.55, hall_rt=3.8 |

Round 2 `pro_impact`:

| Member | Definition |
| --- | --- |
| `rumble` | BP1 |
| `air` | BP1 + gate=0.6, body_attack=0.25, air=0.5, air_f=9000, grain_depth=0.5, roll=1.0, roll_hold=0.5, sub=0.35 |
| `short` | BP1 + gate=0.65, body_attack=0.05, crack=0.5, grain_rate=42, roll_hold=0.4, roll_fall=0.3, body=0.5 |
| `deep` | BP1 + gate=1.3, sub=0.6, sub_f=36, body_f1=62, grain_rate=28, roll_cut=150, roll_hold=0.9, air=0.06 |
| `swell` | BP1 + pre=0.18, pre_level=0.3, body_attack=0.2, gate=0.9, grain_rate=32 |

Round 3 `impact`:

| Member | Definition |
| --- | --- |
| `classic` | BC3 |
| `deep` | BC3 + body_f0=110, body_f1=44, body=0.8, sub=0.45, sub_f=34, sub_tau=1.0, roll=0.5, roll_cut=140, roll_hold=2.2, crack=1.3, crack_f=1600, blast_f=700, blast_tau=0.45, hall_rt=4.5, hall_damp=0.6, sheen=1.5, sheen_ms=10 |
| `punch` | BC3 + body_f0=210, body_f1=58, drop_tau=0.03, body_tau=0.18, crack=1.35, crack_f=3000, crack_ms=7, blast_tau=0.12, roll_hold=0.9, roll_fall=0.4, hall_rt=2.0, hall_wet=0.6, sub=0.15, sub_tau=0.45, sheen=1.6, sheen_ms=5 |
| `metal` | BC3 + shell=0.8, shell_lo=260, modes=0.5, crack_f=3500, debris=0.6, air=0, air_f=6500, hall_damp=0.35, hall_rt=3.6, blast_f=2000, sheen=1.8, sheen_ms=9, sheen_f=7000, crack=2.0, body_attack=0.001, hall_tone=5500 |
| `swell` | BC3 + pre=0.5, pre_level=0.35, pre_shape=1, body_attack=0.03, roll_hold=2.0, sheen=1.0, roll=0.45, hall_rt=3.8 |
| `thunder` | BC3 + body=0.5, body_f0=130, body_f1=48, debris=1.0, crack_f=1800, blast=1.0, blast_f=900, blast_tau=0.5, roll=0.7, roll_hold=1.9, roll_depth=0.35, roll_rate=1.2, roll_cut=260, sheen=1.6, sheen_ms=9, crack=1.7, hall_rt=3.4 |

### 4.6 Search spaces and recipe objects

| Field | Value |
| --- | --- |
| `TARGETS` `impact` | `a deep cinematic boom impact` |
| `TARGETS` `pro_impact` | `a deep designed sub impact with a granular rumble tail, gated` |

Search keys per category (every other key stays at the member's value):

| Category | Search keys |
| --- | --- |
| `impact` | crack, crack_f, sheen, sheen_ms, debris, blast, blast_f, blast_tau, body_f0, body_f1, body_tau, body, sub, shell, roll, roll_cut, roll_hold, roll_fall, roll_depth, hall_wet, hall_rt, hall_tone, width |
| `pro_impact` | crack, body, body_f1, body_attack, sub, roll, roll_cut, roll_hold, roll_fall, grain_rate, grain_depth, air, gate, gate_db |

`pro_impact` keeps width and air_f at the preset (wide, decorrelated tails belong to the style; an earlier search traded
them away).

Range limits that replace the full range in a category search space:

| Category | Limits |
| --- | --- |
| `pro_impact` | gate 0.55..1.35; body_attack 0.04..0.3 log; roll_cut 100..260 log; roll_hold 0.2..1.2; roll_fall 0.15..0.8 log; sub 0..0.7; air 0.2..1.0; gate_db 12..22 |
| `impact` | roll_hold 0.8..3.0; hall_rt 1.6..6.0 log; roll_fall 0.25..1.6 log |

`space_for(cat)` = {key: the limit when present, else the full range} for the search keys of the category.

`recipe_for(cat, preset = 0)`: preset is an index or a member name. Returns a recipe object with: the module name,
SPACE = space_for(cat), DEFAULTS = a copy of the member's full set, render = the module render, TARGET =
TARGETS[cat], ALTS = the module ALTS, REFSET = cat, PRESET = the member name. An unknown name is an error.

### 4.7 Measurement (pro study)

Input: a render, mono or stereo (mono duplicated). m = channel mean.

1. 1 ms RMS frames (48 samples): e = sqrt(mean(m^2) + 1e-20), edb = 20 log10(e + 1e-12). pk = the loudest frame.
   attack_ms = pk - (the first frame anywhere with edb >= edb[pk] - 20). peak_ms = pk.
2. 10 ms smoothing: es = centred 10-frame moving average of e^2, sdb = 10 log10(es + 1e-20), spk = its maximum.
   t20 / t40 = the first frame index from spk with sdb < top - 20 (- 40), or the remaining frame count when none.
3. slope_dBs: from spk, a = the first index below top - 6 (or 0), b = the first below top - 40 (or the end). When
   b - a > 5: the least-squares slope of sdb against time in s over [a, b). Else NaN. 1 decimal.
4. Active frames: sdb > top - 50. len50 = (last - first active) / 1000 s, 3 decimals.
5. gate_db40ms = sdb[end - 40] - sdb[end] when end > 40 (end = the last active frame), else 0. 1 decimal.
6. pre_gate = sdb[end - 60] - top when end > 60, else 0. 1 decimal.
7. Welch power spectrum of m (segment min(n, 16384)). Third-octave balance at centres 1000 x 2^(k / 3), k = -17..13
   (each band [c / 2^(1/6), c x 2^(1/6)), in dB of the total, -120 when the band is empty). sh150 = share below
   150 Hz, 3 decimals. tilt = the slope of the balance against log2(centre) for centres 100-10000 Hz, 2 decimals.
   Octave balance `oct` at 31.5, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000 (keys are the integer centres;
   band [c / sqrt 2, c x sqrt 2)), dB of the total, 1 decimal. cen = power-weighted mean frequency, integer.
8. corr = sum(L R) / sqrt(sum L^2 sum R^2), 2 decimals. side_db = 10 log10(sum S^2 / sum M^2), S = (L - R) / 2,
   M = (L + R) / 2, 1 decimal.
9. Ridge: STFT of m with segment 8192 and hop 960. In 25-3000 Hz take the strongest bin per frame. Keep frames whose
   peak level is within 25 dB of the maximum. ridge = (first, median, last) kept frequency, rounded; (0, 0, 0) when
   none.

Output keys: dur (s, 3 decimals), attack_ms, peak_ms, t20, t40, slope_dBs, len50, gate_db40ms, sh150, tilt, cen,
oct, corr, side_db, ridge, pre_gate.

### 4.8 Study penalty and targets

Penalty (m, targets {key: (lo, hi)}): for each key take the value (keys `ridge0`, `ridge`, `ridge1` read the 1st,
2nd and 3rd ridge item). Skip a missing or non-finite value. Outside the range: when lo > 0, v > 0 and the key is not
one of tilt, slope_dBs, corr, gate_db40ms, sh150, add |log2(v / bound)| (octaves); else add |v - bound| / max(1e-6,
hi - lo). bound is lo when v < lo, else hi.

`study_extra(targets, weight = 1)` returns a critic extra term: -weight x penalty(measure(x), targets).

| Category | Targets |
| --- | --- |
| `impact` | t40 1300..2600; slope_dBs -20..-10; tilt -6.5..-3.5; sh150 0.65..0.93 |
| `pro_impact` | len50 0.6..1.3; attack_ms 50..290; t40 250..1100; gate_db40ms 8..40; sh150 0.75..0.97; tilt -6.0..-2.0; corr -0.1..0.5; pre_gate -34..-22 |

The pre_gate target comes from the references: -30.3 and -27.5 dB 60 ms before the end.

## 5. Variations

Version 3 per render: every phase start, the membrane overtone amplitudes, the shell mode layout, all noise layers,
the grain timing and levels, the hall impulse responses (2 per render, the 2nd with a 1-5 ms extra pre-delay), the
plate mode layout, the debris and scatter times. The designed boom (J12) has no randomness. Version 2 has the same
sources plus the echo delays.
