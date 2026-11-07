# recipes-motion, part 3: glitches (glitch, burstglitch)

This part belongs to the index `recipes-motion.md`. Read the index first: it defines the recipe contract, the shared
primitives (P1 to P26) and the seed-bank format. Notation: `bw(order)` is a Butterworth filter (P3), "crush" is the
bit-crush and sample-and-hold of P25, and "box" is the gate window of P26.

Both recipes use 2 random generators:

- the **render generator** (the `rng` argument): all noise samples, so each render has new noise;
- the **pattern generator**: a generator seeded from the integer `pattern` (glitch v2: pattern × 7919 + 13;
  glitch v6: pattern × 7919 + 61; burstglitch: pattern × 7919 + 101). It decides the layout: pitches, burst choices,
  drop-outs, offsets, level jitter. So a parameter set with a fixed `pattern` gives the same gesture on every render,
  and only the noise varies.

A clean implementation keeps this split. It need not reproduce the source's exact draw sequence.

## 10. glitch

### 10.1 The sound

A designed digital glitch or stutter for edits: crackle first, then a gated stutter that speeds up into the cut and
stops dead on it. Measured facts about one pro example (0.7 s): crackle first (sparse high grains, 4 to 12 kHz), then a
dense broadband stutter of 11 bursts at 28 ms median spacing (36 per second), speeding up from 32 ms to 22 ms into the
cut; bright (centroid 9.2 kHz, tilt +3.8 dB/octave, the 8 kHz band strongest); energy shares re total: under 150 Hz
-21.7 dB, 150 Hz to 1 kHz -24.8, 1 to 4 kHz -17.0, 4 to 12 kHz -1.2, over 12 kHz -6.6; fully decorrelated (L/R
correlation 0.04, side as loud as mid); it ends exactly on the cut.

Techniques the design uses: buffer repeats of 5 to 50 ms slices, bit-crush 4 to 10 bits, sample-rate reduction (2 to
8 kHz, aliasing on purpose), data chirps (square bursts at 1 to 4 kHz, a new pitch per burst), sharp pitched transients
that survive chopping, a steady low layer so it does not thin out. Mistakes to avoid: chops drifting off the grid,
stacked harshness, a thin low end.

Target "a short digital glitch stutter with bit-crushed noise bursts that speed up and cut off". Alternatives: "radio
static", "a helicopter", "a woodpecker", "rain on a tin roof". Refset `pro_glitch`. The seed bank also serves the
category `glitch` (the round-6 family).

There are 2 render paths. A parameter set that has the key `roll0` renders with the version-6 path (10.6). Every
other set (all `pro_glitch` members, the old `glitch` members) renders with the version-2 path (10.4).

### 10.2 Interface

`SPACE`, `DEFAULTS` (10.3), `SPACE6`, `DEFAULTS6`, `FAMILY6` (10.6), `PRO_SHARES` = (-21.7, -24.8, -17.0, -1.2, -6.6),
`render(p, rng)`, `render6(p, rng)`, `glitch_stats(x) -> dict`, `study_extra(x, row=None) -> float`,
`generic_extra(x, row=None) -> float` (10.7).

### 10.3 SPACE and DEFAULTS (version 2)

| key | range | scale | default | meaning |
|---|---|---|---|---|
| dur | 0.35 to 0.85 | | 0.65 | length up to the hard end |
| crackle | 0.12 to 0.5 | | 0.35 | share of the length that is crackle before the stutter |
| crackle_rate | 40 to 300 | log | 200 | crackle grains per second at the end of the crackle |
| crackle_hp | 1500 to 6000 | log | 3500 | |
| crackle_db | -30 to -6 | | -14 | |
| grid0 | 0.026 to 0.046 | | 0.032 | first stutter interval (s) |
| grid1 | 0.015 to 0.028 | | 0.022 | last interval |
| accel | 0.6 to 2.5 | log | 1.2 | how the grid speeds up (> 1: late) |
| gate | 0.35 to 0.95 | | 0.7 | burst length / slot |
| swell | 0 to 10 | | 4.0 | crescendo across the stutter (dB) |
| bits | 3 to 10 | | 5.0 | |
| srate | 1500 to 16000 | log | 6000 | sample-and-hold rate |
| tonal | 0 to 1 | | 0.35 | data-chirp share |
| tone_f | 500 to 5000 | log | 1800 | |
| fm | 0 to 1 | | 0.3 | FM "modem" share |
| slices | 1 to 5 | int | 3 | distinct buffers |
| repeat | 0 to 0.9 | | 0.5 | chance a burst replays the previous buffer |
| rev | 0 to 0.5 | | 0.15 | chance a burst plays reversed |
| pitch_step | -2 to 2 | | 0.0 | semitones per burst |
| low_db | -50 to -14 | | -34 | low pulse under the bursts (-39.5 or less = none) |
| decay | 0.15 to 3 | log | 0.6 | burst decay, in slots |
| lvl_jit | 0 to 8 | | 4.0 | burst level spread (dB) |
| floor_db | -72 to -36 | | -52 | bed between bursts |
| width | 0 to 1 | | 0.9 | share of bursts whose right channel reads its own buffer |
| hp | 300 to 7000 | log | 4500 | stutter high-pass |
| bright | -6 to 9 | | 3.0 | 6 kHz peaking gain |
| drive | 0 to 12 | | 4.0 | tanh glue (dB) |
| end_tail | 0.004 to 0.025 | log | 0.012 | the last burst's own decay after the cut |
| pattern | 0 to 999 | int | 11 | layout seed |
| skip | 0 to 0.5 | | 0.0 | round 3: share of stutter slots left empty |
| accent_db | 0 to 12 | | 0.0 | round 3: unaccented bursts sit this far under the accented ones |

`skip` and `accent_db` are read only when the parameter set has them. The render does not fill them in from
`DEFAULTS`, so older sets render as before. The presence of `skip` also switches the crackle grain window (10.4 step 9).

### 10.4 render, version 2

Merge p over `DEFAULTS` without skip and accent_db.

1. dur, n_end = samples of dur, tail_n = samples of 4 × end_tail, n = n_end + tail_n. Output stereo y of n zeros.
2. Stutter start t_st = dur × crackle.
3. **Grid** (shared with burstglitch): from t = t_st, with span = max(0.001, dur - t_st): u = min(1, (t - t_st) /
   span)^accel, slot d = grid0 + (grid1 - grid0) u. Stop when t + d > dur + 0.25 d. Else record (t, d) and advance by d.
   If no slot fits, use one slot (t_st, span).
4. **Buffers**: L = samples of (largest slot + 4 ms). Make `slices` buffers of L samples. One buffer:
   - Noise: white noise (render generator), crushed at bits + 2 bits and srate (P25), scaled to unit std, times
     (1 - 0.6 max(tonal, fm)).
   - If tonal > 0.02: base f = tone_f × 2^uniform(-1, 1); 1 to 3 steps; each step a pitch f × 2^(k / 6) with k an integer
     in -5 to +5; the steps split the buffer into equal parts. A band-limited square (70 % chance) or saw (P23) on
     that pitch track. Add tonal × 1.4 × that.
   - If fm > 0.02: carrier uniform(800, 6000) Hz; ratio one of 0.5, 1.0, 1.41, 2.0, 3.0, 7.13; index uniform(1.5, 8).
     Add fm × 1.2 × sin(2π fc t + index × sin(2π fc ratio t)).
   - Divide by the absolute peak and crush at bits and srate.
   (The pitch, step, ratio and index draws use the pattern generator. The noise uses the render generator.)
5. **Bursts** (k = 0 to nb - 1; last = the final burst). Keep the previous buffer index per channel (start 0).
   - Round 3 skip: if skip is present, k > 0, not last and a pattern draw < skip, leave this slot empty.
   - Gate length g = max(16, samples of d × gate). Last burst: g = max(16, n_end - start sample): it runs to the cut.
   - Level: 10^((swell × (k / max(1, nb - 1) - 1) - |lvl_jit × normal draw| × (not last)) / 20). The swell is a
     crescendo up to 0 dB at the last burst; the last burst has no jitter.
   - Round 3 accent: if accent_db is present, not last, and a pattern draw > 0.35, times 10^(-accent_db / 20).
   - Speed factor 2^(k × pitch_step / 12).
   - Per channel c = 0, 1:
     - Right channel only: with chance 1 - width (a pattern draw > width), copy the left channel's choice (buffer,
       reverse flag, offset).
     - Else: buffer = the previous one with chance repeat, else a random one; reversed with chance rev; offset =
       samples of uniform(0, 3 ms) for the right channel when not last, else 0 (both channels end on the cut).
     - Read the buffer (reversed if chosen). If the speed factor is not 1, resample it by index i × factor (truncated,
       clamped to the last sample).
     - Segment length m = min(g, L), or g for the last burst. If m > L, tile the buffer to m samples, else take the
       first m.
     - Envelope: box (P26, 0.4 ms edges) × exp(-t / (decay × d)).
     - Last burst: append tail_n more samples of the tiled buffer. Envelope: a hold part exp(-t / max(decay × d,
       0.5 × (m - 1) / SR)) over m - 1 samples, then the last hold value × exp(-t / (end_tail / 1.3)) over tail_n + 1
       samples, cut to the segment length; the first 0.4 ms ramps linearly from 0.
     - Add segment × envelope × level × exp(0.12 × normal draw (render generator)) at the burst start + offset, cut at n.
   - Low pulse (only when low_db > -39.5): m = min(samples of d × gate + 6 ms, n - start). f0 = uniform(55, 110) Hz
     (pattern). sin(2π f0 t) × exp(-t / 0.012) × box (1 ms edges), times 10^(low_db / 20) × level, into a mono low track.
6. `bw(4)` high-pass the stutter at hp.
7. Add the low track to both channels, times the stutter's absolute peak.
8. **Crackle** (only when crackle > 0.02 and t_st > 0.02), per channel independently:
   - Poisson times from 0 to t_st: with u = t / t_st, rate = crackle_rate × (0.25 + 0.75 u^1.5), gap exponential with
     mean 1 / rate.
   - Grain length uniform 1.5 to 6 ms. Grain = white noise × w × exp(-t / (half the grain length)), where w is a Hann
     window when skip is present (round 3), else the square root of a Hann window.
   - With chance 0.2 the grain is instead a low blip: 1.5 × sin(2π f t) × Hann, f uniform 150 to 900 Hz.
   - Gain 10^((crackle_db + 18 log10(uniform(0.05, 1)) + 6u) / 20).
   - Filter: 0.85 × `bw(2)` high-pass at crackle_hp + 0.15 × `bw(2)` high-pass at 120 Hz (the low pops keep a little
     body).
   - Add the crackle times the current absolute peak of y.
9. **Bed** (only when floor_db > -71.5): stereo white noise, `bw(2)` high-pass 3 kHz, crushed at 6 bits and srate.
   Ramp u = min(1, t / max(0.02, t_st + 0.02)). Add 10^(floor_db / 20) × peak(y) × bed / (4 std(bed)) × u, only for
   t < dur.
10. Peaking EQ (P24) at 6 kHz, gain bright, Q 0.5.
11. If drive > 0.2: normalise the peak to 1, then tanh saturation with drive dB, oversampling 2 (P11).
12. `bw(8)` low-pass 18 kHz. Set the first 2 samples to 0. Fade in 0.5 ms, out 3 ms (P9).

The output of version 2 is not peak-normalised (the forge wrapper does it).

### 10.5 What version 6 changes and why

Version 2 at its round-3 settings is sparse high-passed hiss ticks: it reads as "static" at best. Version 6 chops a
SOURCE with clear pitch and transients (a stepped synth chord, data chirps, an FM modem, a little noise). The repeat
slice shrinks across the glitch (roll0 to roll1 s), so the repeat turns into a rising buzz (the beat-repeat roll).
Bursts sit on an accelerating grid with hard gates and digital drop-outs between them. Each burst has its own
bit-crush (bits0 to bits1) and sample-rate drop (srate0 to srate1, plus random octave drops). Stereo is wide: the
right channel reads its own slice, and bursts jump hard left or right. It ends on the cut.

### 10.6 Version 6: SPACE6, DEFAULTS6, FAMILY6, render6

| key | range | scale | default | meaning |
|---|---|---|---|---|
| dur | 0.22 to 0.85 | | 0.55 | |
| crackle | 0 to 0.35 | | 0.12 | crushed crackle lead-in share |
| crackle_db | -30 to -6 | | -18 | |
| grid0 | 0.03 to 0.11 | | 0.06 | first slot |
| grid1 | 0.012 to 0.045 | | 0.022 | last slot |
| accel | 0.6 to 2.5 | log | 1.2 | |
| gate | 0.45 to 1 | | 0.8 | 1 = back to back |
| roll0 | 0.006 to 0.07 | log | 0.03 | repeat slice length at the start |
| roll1 | 0.0025 to 0.03 | log | 0.006 | at the cut (1 / slice = the buzz pitch) |
| srate0 | 2500 to 24000 | log | 16000 | sample-and-hold rate at the start |
| srate1 | 700 to 12000 | log | 3000 | at the cut |
| sdrop | 0 to 0.8 | | 0.3 | chance a burst drops 1 or 2 more octaves of sample rate |
| bits0 | 4 to 12 | | 8 | |
| bits1 | 2 to 8 | | 4 | |
| tone | 0 to 1 | | 0.6 | synth chord share |
| tone_f | 110 to 1400 | log | 330 | |
| notes | 1 to 3 | int | 2 | |
| chirp | 0 to 1 | | 0.3 | data chirp share |
| fm | 0 to 1 | | 0.3 | |
| noise | 0 to 1 | | 0.3 | |
| jump | 0 to 1 | | 0.5 | chance a burst reads from a new place (else it plays on) |
| rev | 0 to 0.5 | | 0.15 | |
| pitch_step | -2 to 2 | | 0.0 | semitones per burst |
| drop | 0 to 0.4 | | 0.12 | chance a burst is a drop-out |
| width | 0 to 1 | | 0.8 | chance the right channel reads its own slice and rate |
| pan_jump | 0 to 1 | | 0.3 | chance a burst jumps hard to one side |
| decay | 0.3 to 6 | log | 2.0 | burst decay in slots (large = flat) |
| swell | 0 to 10 | | 4.0 | |
| lvl_jit | 0 to 8 | | 3.0 | |
| low_db | -60 to -10 | | -30 | low thump under loud bursts (-55 or less = none) |
| hp | 60 to 2000 | log | 150 | |
| lp | 5000 to 18000 | log | 14000 | |
| bright | -6 to 6 | | 0.0 | |
| drive | 0 to 12 | | 4.0 | |
| end_tail | 0.004 to 0.03 | log | 0.01 | |
| floor_db | -72 to -40 | | -56 | bed in the drop-outs (-71.5 or less = exact silence) |
| pattern | 0 to 999 | int | 601 | |

`FAMILY6` (ordered list of (name, overrides of DEFAULTS6)):

| key | stutter | crush | data | dropout | scramble | hit |
|---|---|---|---|---|---|---|
| roll0 | 0.035 | 0.02 | 0.012 | 0.06 | 0.025 | 0.015 |
| roll1 | 0.004 | 0.011 | 0.005 | 0.012 | 0.006 | 0.003 |
| grid0 | 0.07 | 0.055 | 0.045 | 0.09 | 0.05 | 0.035 |
| grid1 | 0.02 | 0.025 | 0.018 | 0.03 | 0.02 | 0.014 |
| gate | 0.9 | 0.75 | 0.6 | 0.7 | 0.85 | 0.75 |
| srate0 | 16000 | 12000 | 20000 | 24000 | 8000 | 10000 |
| srate1 | 2500 | 1500 | 6000 | 1200 | 2000 | 2000 |
| sdrop | - | 0.6 | - | 0.15 | 0.5 | - |
| bits0 | 8 | 6 | 7 | 10 | 6 | 6 |
| bits1 | 4 | 3 | 5 | 3 | 4 | 3 |
| tone | 0.5 | 0.25 | 0.0 | 0.35 | 0.3 | 0.3 |
| tone_f | 280 | - | - | 180 | 600 | 220 |
| notes | - | - | - | 2 | - | - |
| chirp | 0.2 | 0.1 | 0.8 | 0.0 | 0.3 | 0.2 |
| fm | 0.3 | 0.4 | 0.6 | 0.5 | 0.8 | 0.3 |
| noise | 0.6 | 0.9 | 0.5 | 0.7 | 0.6 | 0.8 |
| jump | 0.25 | 0.8 | 0.9 | 0.1 | 1.0 | 0.5 |
| rev | - | - | - | - | 0.45 | - |
| pitch_step | - | - | - | - | 1.0 | - |
| drop | 0.08 | - | 0.05 | 0.15 | 0.05 | 0.0 |
| width | - | - | 1.0 | - | - | - |
| pan_jump | - | 0.6 | 0.5 | - | 0.4 | - |
| swell | 8 | 8 | 9 | 8 | 9 | 6 |
| lvl_jit | 2 | 2 | 2 | 2 | 2 | - |
| low_db | - | - | - | -24 | - | -22 |
| hp | 250 | 300 | 500 | 120 | - | - |
| bright | 3 | 3 | - | - | - | - |
| crackle | - | - | 0.0 | 0.0 | - | 0.0 |
| dur | 0.6 | 0.5 | 0.45 | 0.7 | 0.55 | 0.3 |
| pattern | 611 | 612 | 613 | 614 | 615 | 616 |

A dash means the DEFAULTS6 value. Characters: stutter = a beat-repeat roll (slices 35 to 4 ms, a rising buzz, 16 to
2.5 kHz, 8 to 4 bits); crush = crushed noise bursts with random sample-rate octave drops and hard left/right jumps;
data = data chirps and FM modem, fast slices, fully wide; dropout = a synth line breaking up (long slices, drop-outs,
24 kHz to 1.2 kHz, 10 to 3 bits); scramble = reversed slices from all over the source, FM, climbing a semitone per
burst; hit = a 0.3 s dense roll with low thumps.

**Source** (0.6 s, peak 1). All layout draws use the pattern generator; the noise uses the render generator.
- Chord (tone > 0.01): a pitch track that holds tone_f × 2^(k / 12) (k integer -7 to +7) for uniform(40, 120) ms at a
  time. One waveform for the whole source: square or saw, 50/50. Intervals (semitones) 0, 7, 12, 3, 10, the first
  `notes` of them. Each note: a band-limited oscillator at pitch × 2^(interval / 12) × (1 + 0.004 × note index).
  Average the notes. Add tone × that.
- Chirps (chirp > 0.01): a pitch track that holds 1000 × 2^uniform(0, 2) Hz for uniform(5, 30) ms at a time; add chirp ×
  0.8 × a band-limited square.
- FM (fm > 0.01): carrier uniform(500, 3000), ratio one of 0.5, 1.0, 1.41, 2.0, 3.0, 7.13, index uniform(2, 9). Add
  fm × sin(2π fc t + index sin(2π fc ratio t)).
- Noise (noise > 0.01): add noise × 0.5 × white noise.
- Divide by the absolute peak.

**Crackle lead-in** (6): per channel, Poisson times up to t_end with rate 220 × (0.25 + 0.75 u^1.5); grain 1.5 to 6 ms of
white noise crushed at 4 bits and a rate chosen from 4000, 8000, 16000 Hz, times Hann; gain 10^((crackle_db +
14 log10(uniform(0.05, 1)) + 6u) / 20); then `bw(2)` high-pass 1500 Hz.

**render6(p, rng)**: merge p over `DEFAULTS6`.

1. n_end, tail_n = samples of 4 end_tail, n = n_end + tail_n. t_st = dur × crackle when crackle > 0.02, else 0.
2. Grid as 10.4 step 3 from t_st to dur.
3. Source S of NS samples. Read positions per channel: 2 random integers in [0, NS / 2).
4. Per burst k (u = k / max(1, nb - 1)):
   - Drop-out: if k > 0, not last, a draw < drop, and fewer than 2 drop-outs in a row so far: skip the burst. Else
     reset the run count.
   - g = max(32, samples of d × gate); last: max(32, n_end - start).
   - Slice length Ls = max(24, samples of roll0 × (roll1 / roll0)^u).
   - Speed 2^(k × pitch_step / 12). Rate sr0 = srate0 × (srate1 / srate0)^u. Bits b0 = bits0 + (bits1 - bits0) u.
   - Level = 10^((swell × (u - 1) - |lvl_jit × normal| × (not last)) / 20).
   - Side jump: with chance pan_jump and not last, the burst plays with channel gains (1.0, 0.12) or (0.12, 1.0) (50/50).
   - own = a draw < width.
   - For the left channel, and for the right channel when own: with chance jump, move the read position to a random
     integer in [0, NS - Ls - 1). Reverse with chance rev. Rate srk = sr0, or with chance sdrop sr0 / 2 or sr0 / 4.
     Bits bk = round(b0 + uniform(-1, 1)). Slice = S from min(position, NS - Ls - 1), Ls samples, reversed if chosen,
     resampled by the speed factor (as 10.4). Segment = slice tiled to g samples, divided by the slice's absolute
     peak, crushed at max(2, bk) bits and rate min(srk, SR / 2). Advance the position by samples of d, modulo
     max(1, NS - Ls - 1). Else the right channel reuses the left segment.
   - Envelope, not last: box (0.3 ms edges) × exp(-t / (decay × d)).
     Last: exp(-t / (g / SR / 3.45)) over g samples (about 30 dB down at the cut), the first 0.4 ms ramped from 0,
     then the end value × exp(-t / (end_tail / 1.3)) over tail_n samples; the segments are extended by tail_n tiled
     samples.
   - Write per channel at the start sample + offset (offset = samples of uniform(0, 2 ms) for the right channel when
     own and not last, else 0): segment × envelope × level × (side gain when jumping).
   - Low thump (low_db > -55 and level > 0.5): m = min(samples of min(0.05, d), n - start), sin(2π f t) with f
     uniform(50, 90), × exp(-t / 0.015) × box (1 ms), × 10^(low_db / 20) × level, into a mono low track.
5. `bw(2)` high-pass at hp. pk = absolute peak.
6. Add the low track × 3 pk to both channels. If t_st > 0.02, add the crackle lead-in × 2 pk.
7. Bed (floor_db > -71.5): stereo white noise, `bw(2)` high-pass 2 kHz, crushed at 6 bits and 8 kHz; add 10^(floor_db
   / 20) × pk × bed / (4 std) for t < dur.
8. `bw(2)` high-pass 25 Hz, `bw(4)` low-pass at lp, peaking EQ 6 kHz gain bright Q 0.5.
9. drive > 0.2: normalise the peak to 1, tanh saturation with drive dB, oversampling 2.
10. `bw(8)` low-pass 18 kHz, first 2 samples 0, fade in 0.5 ms and out 3 ms, normalise to an absolute peak of 0.89.

### 10.7 glitch_stats, study_extra, generic_extra

`glitch_stats(x)` (mono fold m = channel mean; a mono input is duplicated):

| field | definition |
|---|---|
| corr | Pearson correlation of L and R; 1.0 when L - R has std ≤ 1e-9 |
| centroid_hz | power-weighted mean frequency of the whole-signal FFT of m |
| s412_db | 10 log10 of the 4 to 12 kHz power share |
| shares | power shares in dB for bands [0, 150), [150, 1000), [1000, 4000), [4000, 12000), [12000, 24001) Hz, 1 decimal |
| end_db | 10 log10(mean power of the last 10 ms) - the loudest 5 ms block (dB) |
| bursts | onset count: in 1 ms block levels normalised to 0 dB peak, an onset is a block above -14 dB whose previous block is at or under -14; keep onsets at least 12 ms after the previous kept one |
| early_ms, late_ms | medians of the onset gaps in the first and second half; none when fewer than 4 gaps |
| med2_db | median 2 ms block level - the loudest 2 ms block |

(1e-14 is added inside each log.)

`study_extra(x)` (the pro glitch box), penalty sum:
- min(1, max(0, corr - 0.3) / 0.4)
- min(1, max(0, 6500 - centroid, centroid - 11000) / 2500)
- min(1, max(0, -4 - s412_db) / 6)
- min(1, max(0, end_db + 30) / 10)
- 0.5 if bursts < 6
- min(1, max(0, late - early + 2) / 8), or 0.5 when early is none
- min(1.5, Σ over the 5 bands of max(0, |share - PRO_SHARES| - 6) / 12)
- min(1, max(0, med2_db + 29) / 6)
Return -min(3, 0.6 × sum).

`generic_extra(x)` (no references, gesture rules only): min(1, max(0, corr - 0.5) / 0.4) + min(1, max(0, end_db + 30) /
10) + (0.5 if bursts < 6) + the accelerating term above. Return -min(3, 0.6 × sum).

## 11. burstglitch

### 11.1 The sound

A digital glitch that HITS: its loudest burst comes first (within about 20 ms), then bursts speed up into the cut.
Measured design targets (the source's private study set, as facts): onset 18 ms (16 to 22) from the first moment
within 20 dB of the peak to the peak, 0.23 s long (0.1 to 0.45), falling 20 dB in 76 ms (71 to 118). Music-video
glitches are mid (centroid 902 Hz, 802 to 1281) and near mono (correlation 0.98). The effects-led film's glitch is
crackle, then 11 bright bursts (about 10 kHz) speeding up into the cut, fully decorrelated, with a thud at 100 to
400 Hz under each crack. The design never draws a rising chirp: chirps step down.

Target "a short digital glitch burst". Alternatives: "radio static", "a woodpecker", "rain on a tin roof", "a camera
shutter". Refset `glitch`. Category `glitch`.

### 11.2 Interface

`SPACE`, `FAMILIES = {'glitch': [(name, params), ...]}`, `DEFAULTS` = copy of the first member, `TARGETS`,
`render(p, rng)`, `recipe_for(cat='glitch', preset=0)` (as darkriser, module name `burstglitch`).

### 11.3 SPACE

| key | range | scale | meaning |
|---|---|---|---|
| dur | 0.1 to 0.8 | | |
| crackle | 0 to 0.6 | | share of the length before the first burst |
| crackle_db | -45 to -20 | | under the bursts' peak (kept under -20 so the first burst is the onset) |
| crackle_rate | 40 to 600 | log | |
| crackle_hp | 1500 to 9000 | log | |
| grid0 | 0.018 to 0.07 | log | first interval (s) |
| grid1 | 0.008 to 0.04 | log | last |
| accel | 0.5 to 2.5 | log | |
| gate | 0.25 to 0.9 | | |
| decay | 0.15 to 2 | log | burst decay in slots |
| accent_db | 0 to 12 | | the first burst over the others |
| fall_db | -12 to 6 | | level change across the rest of the train |
| lvl_jit | 0 to 8 | | |
| bits | 3 to 10 | | |
| srate | 1500 to 20000 | log | |
| tonal | 0 to 1 | | |
| tone_f | 300 to 6000 | log | |
| repeat | 0 to 0.9 | | |
| lo_f | 150 to 7000 | log | band-pass low edge on the bursts |
| hi_f | 1500 to 18000 | log | high edge |
| width | 0 to 1 | | |
| low_db | -50 to 0 | | low pulse under each burst re the bursts' peak (-50 = none) |
| low_f | 60 to 500 | log | |
| end_tail | 0.004 to 0.06 | log | |
| pattern | 0 to 999 | int | |

### 11.4 Members (in order)

| key | burst | mid | chirp |
|---|---|---|---|
| dur | 0.726 | 0.2159 | 0.2788 |
| crackle | 0.4906 | 0.1444 | 0.2608 |
| crackle_db | -35.95 | -32.74 | -45.0 |
| crackle_rate | 579.1 | 40.0 | 94.3 |
| crackle_hp | 7237 | 3000 | 1734 |
| grid0 | 0.0229 | 0.03 | 0.03896 |
| grid1 | 0.02146 | 0.01798 | 0.009084 |
| accel | 1.588 | 0.8936 | 0.7294 |
| gate | 0.6991 | 0.6 | 0.7683 |
| decay | 0.4964 | 0.7133 | 1.285 |
| accent_db | 3.442 | 3.068 | 6.973 |
| fall_db | -12.0 | -4.756 | -9.256 |
| lvl_jit | 7.164 | 2.992 | 2.413 |
| bits | 7.138 | 6.225 | 4.329 |
| srate | 5507 | 8000 | 6357 |
| tonal | 0.02252 | 0.2935 | 0.85 |
| tone_f | 3311 | 636.8 | 446.1 |
| repeat | 0.5548 | 0.371 | 0.2995 |
| lo_f | 5000 | 150 | 459.1 |
| hi_f | 16990 | 2764 | 6342 |
| width | 1.0 | 0.1 | 0.7961 |
| low_db | -40.98 | -50.0 | -50.0 |
| low_f | 156.6 | 150 | 150 |
| end_tail | 0.02849 | 0.005826 | 0.02571 |
| pattern | 17 | 5 | 29 |

Characters: burst = quiet crackle, then a bright 5 to 17 kHz burst train speeding up into the cut, wide, a faint low
thud under each burst; mid = a 0.22 s crushed stutter around 1 kHz that hits first, near mono; chirp = a pitched,
crushed stutter (square tones stepping down) that hits first and rolls faster.

### 11.5 render(p, rng)

Merge p over `DEFAULTS`. Pattern generator seed pattern × 7919 + 101.

1. n_end = samples of dur, tail = samples of 5 × end_tail, n = n_end + tail.
2. t_st = dur × crackle. Grid as 10.4 step 3 from t_st to dur.
3. Buffers: L = samples of (largest slot + 4 ms). Two sets (one per channel) of 3 buffers. One buffer: white noise
   (render generator). If tonal > 0.02: f = tone_f × 2^uniform(-0.5, 0.5) (pattern); 1 to 3 steps; step pitches
   f × 2^(k / 6) with k integers in -4 to +4, sorted so the pitch steps DOWN through the buffer; buffer = (1 - 0.7 tonal)
   × noise / 3 + tonal × band-limited square on that track. Divide by the absolute peak, crush at bits and srate.
4. Per burst k:
   - g = max(16, samples of d × gate); last: max(16, n_end - start).
   - Level in dB: 0 for k = 0 (the accent); else -accent_db + fall_db × (k - 1) / max(1, nb - 2) - |lvl_jit × normal
     (pattern)|.
   - Per channel: own = (left) or (a pattern draw ≤ width). Not own: reuse the left's (buffer index, offset) and read
     from the left set. Own: buffer = previous with chance repeat, else random of 3; offset = samples of uniform(0,
     2 ms) for the right channel when not last and k > 0, else 0.
   - Segment as glitch v2 (m = min(g, L) or g for the last, tiled if longer). Envelope box (0.4 ms) × exp(-t / (decay
     × d)). Last: append tail tiled samples, the envelope continues as end value × exp(-t / (end_tail / 1.3)).
   - Add segment × envelope × 10^(dB / 20) × exp(0.1 × normal (render generator)).
5. `bw(2)` band-pass from lo_f to max(hi_f, 1.5 lo_f). pk = absolute peak.
6. Low pulses (only when low_db > -49.5; a set without the key counts as -50): per burst, m = min(samples of d × gate
   + 10 ms, n - start), skip if m < 16; f0 = low_f × 2^uniform(-0.4, 0.4) (pattern); sin(2π f0 t + random phase
   (render)) × exp(-t / max(0.004, 0.5 d)) × box (1 ms) × (1 for the first burst, 10^(-accent_db / 20) for the others).
   Add pk × 10^(low_db / 20) × low / peak(low) to both channels.
7. Crackle (only when crackle > 0.02 and t_st > 0.01), per channel: Poisson grains with rate crackle_rate × (0.25 +
   0.75 u^1.5), grain 1.5 to 6 ms of white noise × Hann, gain 10^((18 log10(uniform(0.05, 1)) + 6u) / 20). `bw(2)`
   high-pass at crackle_hp. Add crackle / peak(crackle) × pk × 10^(crackle_db / 20).
8. `bw(6)` low-pass 18.5 kHz. First 2 samples 0. Fade in 0.5 ms, out 3 ms. Divide by the absolute peak (before the
   fade) and multiply by 0.89.
