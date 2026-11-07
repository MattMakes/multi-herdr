# recipes-motion, part 1: the pass engine (whoosh, swish, swipe, brightswish)

This part belongs to the index `recipes-motion.md`. Read the index first: it defines the recipe contract, the shared
primitives (P1 to P14) and the seed-bank format that this part uses.

The 4 recipes of this part share one engine, the **pass engine** of the `whoosh` recipe. `swish`, `swipe` and
`brightswish` are parameter sets (and in one case one extra layer) on top of that engine.

## 1. whoosh

### 1.1 The sound

An object or a designed air layer passes the listener. The level rises on an accelerating curve to one peak and then
falls into a smooth tail. Several layers (air body, low weight, sub, hiss top, whistle tone, grains) share the same
level curve and the same filter motion, so they read as one event. The filters open as the level rises and sink after
the peak, so the spectral centroid falls across the pass. A Doppler glide makes the pitch high before the peak and
low after it. The tail fades out, it is never cut hard.

The recipe has 3 kinds (families), each with its own base parameter set and members:

| kind | target text | alternatives (for the critic) | refset name |
|---|---|---|---|
| `fast` | "a fast crisp air whoosh" | "a wind gust", "rain", "a sword swing", "a jet engine" | `whoosh_fast` |
| `big` | "a big deep whoosh transition" | "a strong wind", "a jet plane flying over", "waves", "a car passing" | `whoosh_big` |
| `pro_big` | "a deep low designed whoosh swell" | "a strong wind", "a jet plane flying over", "a car passing", "a bass drum" | `pro_whoosh_big` |

The module-level `TARGET`, `ALTS` and `REFSET` are those of `fast`. The module default refs are 4 files
`<ref root>/whoosh/whoosh_fast_01.wav` to `_04.wav` (the ref root is the forge-core reference folder, see the index).

### 1.2 Interface

- `SPACE`: the search space, table 1.3. Every entry is `(lo, hi)` or `(lo, hi, 'log')`.
- `OPTIONAL`: the "off" value of each optional key, table 1.4. A parameter set that lacks an optional key renders as
  if the key had its off value.
- `DEFAULTS`: the current `fast` base (table 1.5, column `fast`).
- `BASES`: `{fast, big, pro_big}` to their base dicts.
- `PRESETS[kind][member]`: overrides of the kind's base (tables 1.6 to 1.8).
- `KINDS[kind]`: the target, alternatives and refset of the kind.
- `CHARACTER`: the names of the character parameters (section 1.9).
- `preset(kind, member) -> dict`: the base of the kind, then the member's overrides on top.
- `local_space(params, space=None, keys=None, char_w=0.12, free_w=0.3, character=None) -> dict`: section 1.9.
- `recipe(kind='fast', defaults=None, module_name=<this module>) -> recipe object`: an object with `TARGET`,
  `ALTS`, `REFSET` of the kind, `SPACE`, `DEFAULTS` (a copy of `defaults`, else of the kind's base), `render`, `KIND`
  and a module name. The module name decides which seed bank the forge uses, so it stays `whoosh` unless the caller
  passes another name.
- `envelope(p) -> (t, a, tp, n)`: the shared level curve (section 1.10).
- `render(p, rng) -> float array (n, 2)`: section 1.11. A parameter set without the key `rise` is a version-1 set and
  goes to `render_v1` (section 1.13).
- `shelf(x, f0, gain_db, high=True, s=0.8) -> array`: an RBJ cookbook shelving biquad (P12), applied along axis 0.
- `render_v1(p, rng)` and `DEFAULTS_V1`: section 1.13.

### 1.3 SPACE (version 2)

Times are seconds, frequencies Hz, levels dB.

| key | lo | hi | scale | meaning |
|---|---|---|---|---|
| rise | 0.02 | 0.6 | log | time from -30 dB to the peak |
| rise_k | 0.45 | 1.6 | | exponent of the rise curve in dB (< 1 accelerates into the peak) |
| hold | 0.0 | 0.25 | | plateau at the peak |
| fall | 0.03 | 0.8 | log | time from the end of the hold to -30 dB |
| fall_k | 0.5 | 2.2 | | exponent of the fall curve (> 1 holds then drops, < 1 drops into a long tail) |
| start_db | -44 | -22 | | level of the first sample relative to the peak |
| track | 0.3 | 1.2 | | filter motion m = amplitude^track |
| f_peak | 60 | 4000 | log | air band centre at the peak |
| up_oct | 0 | 3 | | octaves under f_peak at the start |
| down_oct | 0 | 3.5 | | octaves under f_peak at the end |
| dop_st | 0 | 12 | | Doppler glide in semitones (total, high to low) |
| dop_w | 0.005 | 0.15 | log | width of the Doppler glide |
| q_air | 0.6 | 5 | log | Q of the air bands |
| lp_ratio | 1.2 | 8 | log | low-pass "cliff" above the air band, as a multiple of the air centre |
| flut_depth | 0 | 0.6 | | depth of the log-normal flutter AM |
| flut_f | 6 | 60 | log | bandwidth of the flutter control noise |
| rustle | 0 | 0.9 | | depth of the fast AM of the air body |
| rustle_f | 40 | 260 | log | bandwidth of that AM |
| low_db | -40 | 6 | | level of the low weight layer |
| low_f | 50 | 400 | log | low weight low-pass corner |
| low_smooth | 0.002 | 0.08 | log | smoothing of the low weight envelope |
| sub_db | -50 | 10 | | level of the sub sine |
| sub_f | 32 | 110 | log | sub frequency before the peak |
| sub_glide | 0 | 1.5 | | octaves the sub sinks across the peak |
| sub_drive | 0 | 3 | | tanh drive of the sub (adds harmonics 2 and 3) |
| hiss_db | -45 | 0 | | level of the hiss top |
| hiss_hp | 800 | 8000 | log | hiss high-pass corner |
| hiss_open | 0 | 2.5 | | octaves the hiss low-pass opens with the motion |
| hiss_pow | 1 | 4 | | hiss envelope exponent |
| tone_db | -50 | -3 | | level of the whistle tone |
| tone_q | 6 | 60 | log | Q of the whistle band |
| tone_ratio | 0.5 | 4 | log | whistle frequency as a multiple of the air centre |
| tone_pure | 0 | 1 | | blend from band noise (0) to a sine (1) |
| grain_db | -50 | -3 | | level of the grain layer |
| grain_rate | 50 | 3000 | log | grain density at the peak (events per second) |
| grain_f | 800 | 9000 | log | grain band centre |
| room_wet | 0 | 0.5 | | reverb wet level |
| rt60 | 0.15 | 2 | log | reverb decay time |
| pan | 0 | 0.5 | | balance pan swing across the pass |
| width | 0 | 0.6 | | decorrelated air side level |
| drive_db | 0 | 9 | | tape saturation drive |
| end_db | -75 | -50 | | the edit ends where the smoothed level is this far under the peak |
| end_fade | 0.02 | 0.15 | log | length of the final fade |
| hiss_rustle | 0 | 1 | | fabric friction AM depth on the hiss (optional) |
| hiss_lo | 600 | 4000 | log | with hiss_rustle: the hiss high-pass corner instead of hiss_hp (optional) |
| eq_lo_db | -18 | 3 | | low shelf gain (optional) |
| eq_lo_f | 80 | 300 | log | low shelf corner (optional) |
| eq_hi_db | -15 | 12 | | high shelf gain (optional) |
| eq_hi_f | 1500 | 9000 | log | high shelf corner (optional) |
| low_lag | 0 | 0.08 | | delay of the low weight envelope (optional) |
| hiss_pre | 0.3 | 3 | | hiss envelope exponent before the peak (optional) |
| hiss_tail | 0.3 | 3 | | hiss envelope exponent after the peak (optional) |
| hiss_mfloor | 0 | 1 | | floor of the hiss low-pass motion (optional) |
| hiss_width | 0 | 1 | | level of an independent hiss as side signal (optional) |
| room_damp | 0.1 | 0.8 | | reverb high damping (optional) |
| low_width | 0 | 1 | | level of an independent low band as side signal (optional) |
| tail_sweep | 0 | 3 | | octaves the hiss band and half of it the air body keep sinking after the peak (optional) |
| tail_tau | 0.05 | 0.6 | log | time constant of that sink (optional) |
| hiss_q | 0.6 | 3 | | resonance of the hiss low-pass (optional) |
| tail_gust | 0 | 0.8 | | slow gust AM of the hiss in the tail only (optional) |

### 1.4 OPTIONAL (off values)

| key | off | key | off |
|---|---|---|---|
| low_lag | 0.0 | hiss_mfloor | 0.0 |
| hiss_rustle | 0.0 | hiss_width | 0.0 |
| hiss_lo | none | room_damp | 0.55 |
| eq_lo_db | 0.0 | low_width | 0.0 |
| eq_lo_f | 150 | tail_sweep | 0.0 |
| eq_hi_db | 0.0 | tail_tau | 0.25 |
| eq_hi_f | 4000 | hiss_q | 0.6 |
| hiss_pre | none | tail_gust | 0.0 |
| hiss_tail | none | | |

`none` means "not set": the render uses the fallback that section 1.11 names.

### 1.5 Base parameter sets

The bases build on each other. `R2` is the old fast base. Every later base is `R2` plus the changes in its column.
A blank cell means "same as R2". The final `fast` is the round-5 value; the columns show each change once.

`R2` (all 43 version-2 core keys):

| key | R2 | key | R2 | key | R2 |
|---|---|---|---|---|---|
| rise | 0.065 | q_air | 1.1 | hiss_open | 1.2 |
| rise_k | 0.9 | lp_ratio | 3.0 | hiss_pow | 1.8 |
| hold | 0.0 | flut_depth | 0.18 | tone_db | -40 |
| fall | 0.075 | flut_f | 24 | tone_q | 20 |
| fall_k | 0.75 | rustle | 0.0 | tone_ratio | 1.7 |
| start_db | -34 | rustle_f | 90 | tone_pure | 0.0 |
| track | 0.45 | low_db | -9 | grain_db | -50 |
| f_peak | 420 | low_f | 130 | grain_rate | 300 |
| up_oct | 1.5 | low_smooth | 0.03 | grain_f | 3000 |
| down_oct | 1.8 | sub_db | -40 | room_wet | 0.12 |
| dop_st | 2.0 | sub_f | 60 | rt60 | 0.35 |
| dop_w | 0.03 | sub_glide | 0.3 | pan | 0.15 |
| | | sub_drive | 0.5 | width | 0.12 |
| | | hiss_db | -13 | drive_db | 2.0 |
| | | hiss_hp | 2500 | end_db | -66 |
| | | | | end_fade | 0.04 |

`fast` (the module `DEFAULTS`) = R2 with these values:

| key | value | key | value |
|---|---|---|---|
| rise | 0.13 | hiss_pow | 1.2 |
| rise_k | 0.7 | room_wet | 0.1 |
| hold | 0.005 | rt60 | 0.58 |
| fall | 0.2 | end_fade | 0.06 |
| fall_k | 0.58 | eq_lo_db | -7 |
| track | 0.5 | eq_lo_f | 145 |
| down_oct | 2.0 | eq_hi_db | -4 |
| q_air | 1.0 | eq_hi_f | 6000 |
| lp_ratio | 4.0 | room_damp | 0.3 |
| low_db | -15 | hiss_tail | 0.75 |
| low_f | 170 | hiss_mfloor | 0.35 |
| low_smooth | 0.04 | hiss_pre | 0.8 |
| hiss_db | -24 | tail_sweep | 0.6 |
| hiss_hp | 2000 | tail_tau | 0.25 |
| hiss_open | 0.9 | hiss_q | 1.4 |

`big` = R2 with these values:

| key | value | key | value | key | value |
|---|---|---|---|---|---|
| rise | 0.36 | q_air | 1.2 | sub_drive | 1.0 |
| rise_k | 0.8 | lp_ratio | 4.5 | hiss_db | -16 |
| fall | 0.36 | flut_depth | 0.25 | hiss_hp | 2000 |
| fall_k | 0.85 | flut_f | 12 | hiss_open | 0.8 |
| start_db | -36 | low_db | -2 | hiss_pow | 1.3 |
| track | 0.8 | low_f | 120 | room_wet | 0.22 |
| f_peak | 380 | low_smooth | 0.04 | rt60 | 0.9 |
| up_oct | 1.4 | sub_db | -6 | pan | 0.12 |
| down_oct | 2.3 | sub_f | 55 | width | 0.2 |
| dop_st | 3.0 | sub_glide | 0.5 | drive_db | 3.0 |
| dop_w | 0.08 | hiss_tail | 0.75 | end_fade | 0.08 |
| | | hiss_mfloor | 0.35 | room_damp | 0.35 |

`pro_big` = `big` without its 3 round-4 keys (hiss_tail, hiss_mfloor and room_damp are absent, so they take their
off values), then these values:

| key | value | key | value | key | value |
|---|---|---|---|---|---|
| rise | 0.3 | low_db | 0.0 | hiss_db | -30 |
| rise_k | 1.2 | low_f | 90 | hiss_hp | 1000 |
| hold | 0.02 | low_smooth | 0.03 | hiss_open | 1.4 |
| fall | 0.165 | sub_db | 2.0 | hiss_pow | 1.2 |
| fall_k | 0.8 | sub_f | 46 | room_wet | 0.1 |
| start_db | -34 | sub_glide | 0.5 | rt60 | 0.5 |
| f_peak | 90 | sub_drive | 0.8 | pan | 0.0 |
| up_oct | 0.8 | q_air | 0.8 | width | 0.03 |
| down_oct | 1.5 | lp_ratio | 5.0 | drive_db | 2.0 |
| dop_st | 2.0 | | | end_db | -64 |
| | | | | end_fade | 0.06 |

All other `pro_big` keys keep the `big` value (track 0.8, dop_w 0.08, flut_depth 0.25, flut_f 12, start of the
tone and grain keys from R2, and so on).

### 1.6 Members of `fast`

| member | overrides of `fast` | character |
|---|---|---|
| crisp | grain_db -17, grain_rate 1400, grain_f 4800, hiss_db -27 | gritty air: bright soft crackle grains carry the top |
| sleeve | hiss_rustle 0.5, hiss_pre 1.3, hiss_db -20, hiss_lo 1200, eq_hi_db -10 | fabric friction on the hiss, duller top |
| whistle | tone_db -3, tone_q 50, tone_pure 1.0, tone_ratio 1.5, dop_st 12, dop_w 0.04, tail_tau 0.4, hiss_q 2.2 | a pure tone gliding a full octave (the designed motion-graphics whoosh) |
| heavy | fall 0.19, f_peak 300, flut_depth 0.35, flut_f 20, room_wet 0.14, rt60 0.55, tail_tau 0.4, hiss_q 2.2 | heavier, fluttering swing |
| reverse | rise 0.25, rise_k 0.55, hold 0.0, fall 0.08, fall_k 1.0, room_wet 0.15 | a "suck" that accelerates into the cut and stops softly |
| tight | fall 0.1, fall_k 0.75, f_peak 750, q_air 2.5, room_wet 0.12, rt60 0.35, grain_db -20, grain_rate 2000, grain_f 6000, tail_sweep 0.0, hiss_q 0.6 | a short thin zip with no tail sweep |

### 1.7 Members of `big`

| member | overrides of `big` |
|---|---|
| swell | none (the base) |
| cape | rise 0.26, rise_k 0.9, fall 0.32, hold 0.05, f_peak 330, q_air 0.9, flut_depth 0.6, flut_f 13, rustle 0.6, rustle_f 70, hiss_db -14, hiss_lo 1400, hiss_rustle 0.6, grain_db -24, grain_rate 700, grain_f 3500, low_db -4, sub_db -24, width 0.45, pan 0.25, room_wet 0.18 |
| jet | tone_db -3, tone_q 50, tone_ratio 1.4, tone_pure 0.9, dop_st 12, dop_w 0.1, f_peak 700, up_oct 1.0, down_oct 2.6, q_air 1.4, sub_db -18, low_db -8, hiss_db -20, rise 0.42, fall 0.36, pan 0.3 |
| deep | rise 0.5, fall 0.45, f_peak 260, up_oct 0.9, down_oct 1.8, low_db -1, low_f 90, sub_db -2, sub_f 46, sub_glide 0.8, sub_drive 1.4, flut_depth 0.55, flut_f 6, drive_db 7, grain_db -26, grain_rate 200, grain_f 1200, hiss_db -22, rt60 1.3, room_wet 0.28, width 0.08, pan 0.0 |
| gust | rise 0.45, rise_k 1.1, fall 0.45, f_peak 700, up_oct 1.8, down_oct 2.4, q_air 0.8, lp_ratio 3.5, low_db -12, sub_db -50, hiss_db -14, hiss_hp 2200, flut_depth 0.6, flut_f 6, width 0.5, room_wet 0.35, rt60 1.0 |
| reverse | rise 0.6, rise_k 0.55, hold 0.0, fall 0.06, fall_k 1.4, room_wet 0.2, rt60 0.6 |

Characters: swell = smooth air swell; cape = a flapping cape or sail (flutter plus fabric, wide); jet = a gliding
whistle over a thin air bed; deep = a dark sub boom; gust = a bright gusting pass with a long room (6 Hz flutter at
full depth); reverse = a suck-in that stops at the cut.

### 1.8 Members of `pro_big`

| member | overrides of `pro_big` |
|---|---|
| swell | none |
| reverse | rise 0.5, rise_k 0.55, hold 0.0, fall 0.05, fall_k 1.4 |
| long | rise 0.42, fall 0.24, hold 0.05, sub_f 42, sub_glide 0.7, rt60 0.8, flut_depth 0.6, flut_f 6 |
| windy | f_peak 200, q_air 0.7, lp_ratio 3.0, hiss_db -27, hiss_rustle 0.5, hiss_lo 1200, rustle 0.5, rustle_f 50, sub_db -2 |
| tonal | sub_db 8, low_db -10, sub_f 70, sub_glide 1.5, sub_drive 0.3, tone_db -6, tone_q 50, tone_ratio 4.0, tone_pure 1.0, dop_st 12 |
| gritty | drive_db 9, sub_drive 3.0, grain_db -20, grain_rate 300, grain_f 1500 |

### 1.9 Character parameters and the local search box

`CHARACTER` = rise, fall, hold, f_peak, up_oct, down_oct, dop_st, low_db, sub_db, sub_f, hiss_db, tone_db,
tone_ratio, grain_db, grain_f, width, pan, fall_k, flut_depth, rustle, hiss_rustle, tone_pure, q_air.

`local_space(params, space, keys, char_w, free_w, character)` builds a narrow search box around one parameter set,
so that a forge run tunes a family member without losing its character:

1. Use `space` (default `SPACE`) and `character` (default `CHARACTER`).
2. For each key of the space: skip it when `keys` is given and does not hold it, when the space entry is a list (a
   choice), or when `params` lacks the key.
3. Clip the value v into [lo, hi]. The half-width w is `char_w` for a character key, else `free_w`.
4. Linear key: the box is [max(lo, v - w (hi - lo)), min(hi, v + w (hi - lo))].
5. Log key: do step 4 on ln(lo), ln(hi), ln(v), then exponentiate. Keep the `'log'` tag.
6. Drop a key whose box is narrower than 1e-9.

### 1.10 The level curve

Input: a parameter set p. Output: time axis t, amplitude a (0 to 1), peak time tp, sample count n.

1. Peak time: tp = rise × (max(-start_db, 1) / 30)^(1 / rise_k). The curve below is -30 dB at one `rise` before the
   peak, so this puts the first sample at `start_db`.
2. End estimate: t_end = tp + hold + fall × (95 / 30)^(1 / fall_k) + 0.9 × rt60 × min(1, 4 × room_wet + 0.2) + 0.05.
   (The source fall reaches -95 dB, plus a reverb allowance, plus 50 ms.)
3. Length: n = samples of min(tp + max(t_end - tp, 0.1), 4.5) s. So a render is at most 4.5 s before the edit.
4. For each sample, d = t - tp. Before the peak, x = -d / rise and k = rise_k. After it, x = max(d - hold, 0) / fall
   and k = fall_k.
5. Level in dB: L = -30 × x^k. Amplitude: 10^(max(L, -140) / 20).
6. Round the peak: smooth the amplitude with a zero-phase Hann window (P5) of width
   clip(0.3 × min(rise, fall + hold), 0.004, 0.1) s.
7. Divide by the maximum, so the peak is 1.

### 1.11 The version-2 render

Notation: `a` is the level curve, d = t - tp, `lp`, `hp`, `bp` and `bpn` are the state-variable filter modes of P2
(low, high, band with peak gain Q, band with peak gain 1), `bw(order, f)` is a Butterworth filter (P3), and
`ctrl(fc)` is a control noise (P6): white noise, 2nd-order Butterworth low-pass at fc, scaled to unit standard
deviation. `pink` and `brown` are the colored noises of P7. `level(y)` is the mix level of P8 (the peak of the 10 ms
block RMS).

If p has no key `rise`, call the version-1 render (section 1.13) and stop.

**Motion and pitch.**

1. m = a^track.
2. Doppler factor: dop = 2^(-(dop_st / 12) × 0.5 × tanh(d / dop_w)). It is 2^(+dop_st/24) long before the peak and
   2^(-dop_st/24) long after it.
3. Air centre: f_air = 2^(log2(f_peak) - span × (1 - m)) × dop, where span = up_oct before the peak and down_oct after.
4. Tail sweep (only when tail_sweep > 0): g = 1 - exp(-max(d, 0) / tail_tau); f_air = f_air × 2^(-0.5 × tail_sweep × g).
   (The air body sinks half the sweep. The hiss sinks the full sweep, step 22.)
5. Flutter: flut = exp(flut_depth × ctrl(flut_f) - 0.5 × flut_depth²). The second term keeps the mean near 1.

**Air body (always present, mixed at 0 dB).**

6. Sum 3 band-passes (`bpn`, Q = q_air) of independent pink noises, at f_air × 1.0, 1.6 and 2.5 (each clipped to
   30 to 20000 Hz), with gains 1.0, 0.55 and 0.3.
7. Low-pass the sum (`lp`, Q 0.7) at clip(f_air × lp_ratio, 60, 20000): the cliff above the ridge.
8. If rustle > 0: multiply by u / rms(u), where u = exp(1.2 × rustle × ctrl(rustle_f)).
9. Multiply by a and by flut.

**Low weight (only when low_db > -39.5).**

10. Envelope: smooth a with a Hann window of low_smooth s (P5), divide by its maximum.
11. Delay that envelope by low_lag s (zeros in front, same length). Note: the source rounds the delay with the
    "at least 1 sample" rule of P1, so it delays by 1 sample even when low_lag is 0. This is inaudible. Either
    behaviour is acceptable.
12. Band: brown noise, `lp` at clip(low_f × (0.6 + 0.4 m), 25, 2000) with Q 0.75, then `bw(2)` high-pass at
    max(28, low_f / 2.5). Multiply by the envelope. Mix at low_db.
13. If low_width > 0: make a second, independent band the same way (same envelope). Keep it for the side signal.

**Sub (only when sub_db > -49.5).**

14. Envelope: smooth a with a Hann window of max(low_smooth, 0.02) s, divide by its maximum.
15. Glide: s = 0.5 + 0.5 tanh(d / (0.5 fall + 0.02)); f_sub = sub_f × 2^(-sub_glide × s). The sub sinks sub_glide
    octaves across the peak.
16. Sine on the phase 2π Σ f_sub / SR plus a random start phase in [0, 2π).
17. If sub_drive > 0: with g = 1 + 2 sub_drive and b = 0.15 sub_drive, y = tanh(g y + b) - tanh(b). The bias adds the
    2nd harmonic, the drive the 3rd.
18. `bw(2)` low-pass at 600 Hz, times the envelope. Mix at sub_db.

**Hiss top (only when hiss_db > -44.5).**

19. Fabric mode is on when hiss_rustle > 0. The high-pass corner h_lo is hiss_lo (or hiss_hp when hiss_lo is not set)
    in fabric mode, else hiss_hp.
20. Low-pass motion: mh = m when hiss_mfloor ≤ 0, else hiss_mfloor + (1 - hiss_mfloor) m.
21. Low-pass corner: h_lp = clip(hiss_hp × 2^(hiss_open × mh) × 1.5 × dop, 500, 20000).
22. If tail_sweep > 0: with sh = 2^(-tail_sweep × g) (g of step 4), h_lp = clip(h_lp × sh, 300, 20000) and a moving
    high-pass corner h_hp = clip(h_lo × sh, 150, 16000). High-pass white noise through two cascaded `hp` stages at h_hp
    with Q 0.54 and then Q 1.31 (together about a 4th-order Butterworth that can move per sample).
    Else: high-pass white noise with `bw(4)` at the fixed h_lo.
23. Low-pass with `lp` at h_lp, Q = hiss_q (off value 0.6).
24. Fabric mode: multiply by u / rms(u), u = exp(1.4 × hiss_rustle × ctrl(1.8 × rustle_f)).
25. Envelope: if neither hiss_pre nor hiss_tail is set, e = a^hiss_pow. Else, with w = 0.5 + 0.5 tanh(d / 0.005),
    e = (1 - w) a^pre + w a^post, where pre = hiss_pre (else hiss_pow) and post = hiss_tail (else hiss_pow).
26. Tail gust (only when tail_gust > 0): gw = clip(d / 0.04, 0, 1), gu = exp(tail_gust × ctrl(5.5) - 0.5 tail_gust²),
    e = e × (1 - gw + gw × gu). The gusts start 0 to 40 ms after the peak.
27. Layer = hiss × e × flut. Mix at hiss_db.
28. If hiss_width > 0: make a second independent hiss through the same filters and the same fabric AM recipe (a new
    AM draw), times the same e × flut. Keep it for the side signal.

**Whistle tone (only when tone_db > -49.5).**

29. ft = clip(f_air × tone_ratio, 40, 16000).
30. Band noise: `bpn` of white noise at ft with Q tone_q, scaled to unit standard deviation.
31. Sine: √2 × sin(2π Σ ft / SR) (unit RMS, start phase 0).
32. Tone = (1 - tone_pure) × band noise + tone_pure × sine, times a^1.5. Mix at tone_db.

**Grains (only when grain_db > -49.5).**

33. Rate per sample: grain_rate × a. Each sample is an event with probability rate / SR. Each event has a log-normal
    weight (μ 0, σ 0.5).
34. Smooth the event train with a 3 ms Hann window (P5): each event becomes a soft 3 ms bump, never a one-sample click.
35. Multiply `bpn` band noise (white, centre clip(grain_f × dop, 200, 18000), Q 1.2) by the bumps and by a. Mix at
    grain_db.

**Mix.** x = Σ over layers of 10^(db / 20) × layer / level(layer). The air body has db 0.

**Stereo.**

36. Pick a side: +1 or -1 with equal chance. Pan curve: pan(t) = side × pan × tanh(d / max(dop_w, 0.01)).
37. Air side (only when width > 0): `bpn` of white noise at clip(2 f_air, 200, 18000), Q 0.8, times a and flut, then
    `bw(2)` high-pass at 300 Hz. Scale it to width × 0.5 × level(x) / level(side).
38. Add the hiss side: hiss_width × 10^(hiss_db / 20) × second hiss / level(first hiss layer).
39. Add the low side: low_width × 10^(low_db / 20) × second low band / level(first low band).
40. L = x (1 + pan) + side, R = x (1 - pan) - side. The mono sum (L + R) / 2 equals x.

**Space and finish.**

41. Reverb (only when room_wet > 0.005): the forge-core reverb (P10) with rt60, size 0.45, predelay 0.006 s,
    damp = room_damp (off value 0.55), wet = room_wet, width = min(1, 0.3 + 1.5 width), low cut 120 Hz (lows stay dry),
    stereo, tail max(0.2, 1.1 rt60) s, 8 delay lines. If the result holds a non-finite sample, render the reverb
    again with 16 lines.
42. Saturation (only when drive_db > 0.05): divide by the absolute peak, then the forge-core saturator (P11) in tape
    mode with drive_db, oversampling 2.
43. Low shelf (only when |eq_lo_db| > 0.05): P12 at eq_lo_f (off value 150), gain eq_lo_db, slope 0.8.
44. High shelf (only when |eq_hi_db| > 0.05): P12 at eq_hi_f (off value 4000), gain eq_hi_db, slope 0.8.
45. `bw(4)` high-pass at 25 Hz, then `bw(4)` low-pass at 19 kHz.
46. The edit (section 1.12) with end_db and end_fade.

The output is not peak-normalised. The forge-core render wrapper normalises it (see the index).

### 1.12 The edit

Input: stereo y, end_db (negative), end_fade (s). A 1.5 ms start ramp.

1. Mono = mean of the channels. Cut it into whole 5 ms blocks. Block level e = 20 log10(block RMS + 1e-12).
2. Smooth e with a 4-point moving average ("same" length, zero padded at the edges).
3. i = the block of the maximum. j = the first block at or after i with e < e[i] + end_db. If there is none, j is the
   number of blocks.
4. End sample = min(length, j × 240 + samples of end_fade / 2). Truncate there.
5. Fade out: multiply the last min(samples of end_fade, length - 1) samples by a raised cosine from 1 to 0.
6. Fade in: multiply the first samples of 0.0015 s by sin²(φ), φ from 0 to π/2.

### 1.13 Version 1 (old parameter sets)

Version-1 parameter sets are a physical model of a stick swung past a close microphone. They have no key `rise`.
Three seed-bank entries of the `whoosh` bank are version-1 sets. The render stays so that those entries reproduce.

Fixed constants: length 0.8 s, 6 shedding bands, band root 0.45, wake ratio 0.9, hiss pink share 0.5.

`DEFAULTS_V1` (40 keys):

| key | value | key | value | key | value |
|---|---|---|---|---|---|
| tc | 0.0706346 | wake_db | -14.0966 | buffet_db | -9.18542 |
| s_rise | 0.0380857 | wake_tau | 0.0532322 | buffet_f | 95.0096 |
| s_fall | 0.0578346 | wake_q | 4.45435 | buffet_delay | 0.0219106 |
| beta | 1.49847 | wake_dark | 0.613911 | buffet_tau | 0.0262978 |
| prox_w | 0.0243735 | wake_hiss_db | -16.0 | flut_depth | 0.0178753 |
| f_peak | 398.391 | hiss_db | -19.345 | flut_f | 30.2817 |
| f_track | 0.108498 | hiss_hp | 1671.88 | whip_db | -16.8109 |
| q_shed | 7.05767 | hiss_f | 9908.19 | whip_q | 22.6043 |
| steep | 0.23086 | hiss_pow | 2.57151 | whip_ratio | 1.75668 |
| a_pow | 2.90371 | push_db | -4.56985 | pan_w | 0.102279 |
| doppler | 0.00143736 | push_w | 0.0251241 | width | 0.0730709 |
| | | push_f | 125.898 | room_wet | 0.158579 |
| | | | | rt60 | 0.138233 |
| | | | | air_db | -12.0 |
| | | | | floor_db | -75.3278 |
| | | | | cut_db | -46.4213 |
| | | | | cut_fade | 0.0281062 |

A version-1 band at centre fc and Q q: `bp` of white noise divided by q (so its peak gain is 1). With steep > 0, blend
(1 - steep) × band + steep × (`lp` of the band at 1.08 fc, Q 0.9).

Steps (d = t - tc, noises from P7/P13 as named):

1. Shape: s = s_rise before tc, s_fall after. U = exp(-0.5 |d / s|^beta).
2. Proximity: rr = √(1 + (d / prox_w)²). Radial speed v = (d / prox_w) / rr × U. Doppler factor dop = 1 - doppler × v.
3. Amplitude: amp = U^a_pow / rr. Flutter as version 2 step 5.
4. Tip frequency: f_tip = max(f_peak × max(U, 0.001)^f_track × dop, 30).
5. Shedding: Σ for k = 1 to 6 of s_k³ × band(max(f_tip × s_k, 25), q_shed), with s_k = 0.45 + 0.55 k / 6. Multiply by
   amp and flut.
6. Wake: on = 0.5 + 0.5 tanh(d / max(0.25 s_rise, 0.003)), after = max(d, 0). Envelope on × exp(-after / wake_tau).
   Centre max(0.9 f_peak × (1 + after / wake_tau)^(-wake_dark), 40). Band with Q wake_q and the same steep.
7. Hiss: source 0.5 × white + 0.5 × 3 × unit-RMS pink (P13). `lp` (Q 0.6) at max(hiss_f × U^0.7 × dop, 400), then
   `bw(4)` high-pass at hiss_hp, times U^hiss_pow / rr × flut.
8. Wake hiss: unit-RMS pink, `bw(4)` high-pass at hiss_hp, `bw(2)` low-pass at 9 kHz, times on × exp(-after / (0.4 wake_tau)).
9. Push: window exp(-0.5 (d / push_w)²) on brown noise low-passed (`bw(2)`) at push_f; normalise by level(); add
   0.7 × a Ricker wavelet (1 - (d / w)²) exp(-0.5 (d / w)²) with w = 0.35 push_w, low-passed (`bw(2)`) at 2 push_f and
   normalised by level().
10. Buffet: tb = t - (tc + buffet_delay). Envelope exp(-0.5 (tb / max(0.5 s_rise, 0.004))²) before 0, exp(-tb /
    buffet_tau) after. Brown noise `bw(2)` low-passed at buffet_f, times the envelope.
11. Whip: band(min(f_tip × whip_ratio, 16000), whip_q) without steep, times U⁴ / rr.
12. Mix: shedding / level(shedding) plus each other layer at its dB over its own level(): wake (wake_db), hiss
    (hiss_db), wake hiss (wake_hiss_db), push (push_db), buffet (buffet_db), whip (whip_db).
13. Stereo: side ±1 at random; pan = side × pan_w × tanh(d / prox_w). A delay of int(uniform(0.2, 0.9) ms × SR)
    samples; side signal = (delayed mix - mix), `bw(2)` high-pass 300 Hz, times width. L = x (1 + pan) + side,
    R = x (1 - pan) - side.
14. Per channel: the forge-core small room (P14, rt60, size 0.4, wet room_wet), the forge-core mic model (P14, low cut
    25 Hz, air_db, floor_db), `bw(4)` low-pass 19 kHz, keep the first n + 0.1 s. Stack the channels at the shorter
    length.
15. If cut_db is not set: trim the tail at -62 dB keeping 0.03 s (P14), then fade in 2 ms and out 60 ms (P9).
    Else: 5 ms block RMS in dB (no smoothing), i = loudest block, j = first block at or after i under e[i] + cut_db
    (else the block count), end = min(length, j × 240 + samples of cut_fade), then fade in 2 ms and out cut_fade.

## 2. swish

### 2.1 The sound

A thin, fast object cuts the air: a cane, a rod, a blade, a thrown stick. Compared to a whoosh it is short and steep
(80 to 300 ms around the pass), thin (little or no low weight, the air band higher and narrower) and it can carry an
aeolian whistle: a thin rod sheds a near-pure tone at f = 0.2 U / d (Strouhal number 0.2; 30 m/s past a 1 cm rod gives
about 600 Hz), heard as a narrow band (Q 15 to 40) that glides with the speed and the Doppler shift.

Target "a thin quick swish of a cane through the air". Alternatives: "a sword being sharpened", "a short burst of
steam", "a page being flipped", "a bird wing flap". Refset `swish`. No default refs. One kind, `swish`.

### 2.2 Interface

`SPACE` = the whoosh `SPACE`. `CHARACTER` = the whoosh `CHARACTER`. `preset(kind, member)`, `local_space(params,
space=None, keys=None, char_w=0.12, free_w=0.3)` (the whoosh function with this module's space and character),
`recipe(kind='swish', defaults=None)` (module name `swish`, so the forge uses the `swish` seed bank),
`render(p, rng)`.

**render(p, rng)**: build the set in this order, later wins: this module's `DEFAULTS`, then the whoosh `OPTIONAL`
off values, then p. Pass it to the whoosh render. So an optional key that p lacks is off, even when `DEFAULTS` sets it.
This keeps old (round 2 and round 3) parameter sets rendering as they did. Full sets (from `preset()` or a forge run)
are not changed by this rule.

### 2.3 DEFAULTS

Built in 3 steps on the whoosh R2 base:

Step A (old swish base) = whoosh R2 with: rise 0.045, rise_k 0.8, hold 0.0, fall 0.08, fall_k 0.7, start_db -34,
track 0.6, f_peak 520, up_oct 1.4, down_oct 1.8, dop_st 4.0, dop_w 0.02, q_air 1.8, lp_ratio 2.2, flut_depth 0.15,
flut_f 30, low_db -16, low_f 180, low_smooth 0.02, sub_db -50, hiss_db -14, hiss_hp 3800, hiss_open 1.2, hiss_pow 1.8,
tone_db -16, tone_q 28, tone_ratio 1.0, tone_pure 0.25, grain_db -50, room_wet 0.15, rt60 0.35, pan 0.2, width 0.1,
drive_db 1.5, end_db -66, end_fade 0.03.

Step B = A with: rise 0.07, rise_k 1.0, hold 0.004, fall 0.14, fall_k 0.58, lp_ratio 3.0, low_db -24, low_f 220,
hiss_db -24, hiss_hp 2400, hiss_pow 1.2, room_wet 0.22, rt60 0.5, end_fade 0.05, eq_lo_db -10, eq_lo_f 150,
eq_hi_db -4, eq_hi_f 6000.

Step C (`DEFAULTS`) = B with: fall 0.21, room_wet 0.1, room_damp 0.3, end_db -62, hiss_tail 0.85, hiss_mfloor 0.3,
hiss_pre 0.8.

### 2.4 Members (kind `swish`)

| member | overrides of DEFAULTS | character |
|---|---|---|
| cane | none | aeolian cane whistle |
| blade | rise 0.055, fall 0.085, fall_k 0.8, f_peak 900, q_air 2.8, tone_db -22, tone_ratio 1.3, hiss_db -19, hiss_hp 3200, low_db -32, dop_st 5, room_wet 0.1, rt60 0.3 | bright, steep |
| stick | rise 0.08, fall 0.17, fall_k 0.55, end_db -60, f_peak 380, q_air 1.3, low_db -16, low_f 180, flut_depth 0.5, flut_f 9, tone_db -30, hiss_db -27, room_wet 0.14, rt60 0.5 | a thrown stick: lower, rounder, longer |
| zing | rise 0.06, rise_k 0.8, fall 0.13, f_peak 700, tone_db -3, tone_q 50, tone_ratio 1.2, tone_pure 0.9, dop_st 9, dop_w 0.015, hiss_db -25 | a strong Doppler-gliding whistle |
| soft | rise 0.08, f_peak 560, q_air 1.0, tone_db -40, rustle 0.6, rustle_f 120, hiss_rustle 0.6, hiss_lo 1500, hiss_db -21, grain_db -26, grain_rate 900, grain_f 3500, width 0.35, room_wet 0.14, rt60 0.5, pan 0.3 | dark, airy, fabric |

## 3. swipe

### 3.1 The sound

Cloth or paper slid or swept past, for UI slides and panel moves. A soft wide surface moves much air slowly: the low
weight layer carries most of the energy (real cloth recordings hold nearly all of it under 150 Hz) with a soft mid band
on top. Texture, not tone: the air band is roughened by fast AM (rustle, 50 to 200 Hz) and fine crackles (grains,
denser at speed). No whistle. Slower, smoother envelopes than a whoosh: 40 to 150 ms rise, an optional hold, a soft
fall. Wider in stereo (real cloth sits at L/R correlation 0.3 to 0.6).

Target "a soft cloth swipe". Alternatives: "a gust of wind", "a broom on a floor", "paper tearing", "a zipper
pulled". Refset `swipe`. No default refs. One kind, `swipe`.

### 3.2 Interface

Same shape as swish (section 2.2), with module name `swipe`, kind `swipe`, and
`CHARACTER` = the whoosh `CHARACTER` plus rustle, grain_rate, hiss_rustle, low_lag (rustle and hiss_rustle are listed
twice; duplicates are harmless). `render(p, rng)` uses the same merge rule as swish with this module's `DEFAULTS`.

### 3.3 DEFAULTS

Step A = whoosh R2 with: rise 0.12, rise_k 0.9, hold 0.03, fall 0.14, fall_k 0.8, start_db -34, track 0.7,
f_peak 350, up_oct 1.2, down_oct 2.0, dop_st 1.0, dop_w 0.05, q_air 0.9, lp_ratio 3.5, flut_depth 0.3, flut_f 14,
rustle 0.55, rustle_f 90, low_db 4, low_f 110, low_smooth 0.05, sub_db -50, hiss_db -24, hiss_hp 2500, hiss_open 0.8,
hiss_pow 1.5, tone_db -50, grain_db -24, grain_rate 600, grain_f 2600, room_wet 0.12, rt60 0.35, pan 0.15, width 0.35,
drive_db 1.5, end_db -66, end_fade 0.05.

Step B = A with: rise 0.16, rise_k 1.0, hold 0.03, fall 0.17, fall_k 0.65, track 0.7, f_peak 380, q_air 0.8,
lp_ratio 4.0, rustle 0.5, rustle_f 110, low_db -3, low_f 120, low_smooth 0.08, low_lag 0.05, eq_lo_db -2,
eq_lo_f 120, hiss_db -15, hiss_lo 1800, hiss_hp 3000, hiss_open 1.2, hiss_pow 1.0, hiss_rustle 0.7, grain_db -18,
grain_rate 1500, grain_f 5500, width 0.4, eq_hi_db -4, eq_hi_f 9000.

Step C (`DEFAULTS`) = B with: hiss_pre 0.4, hiss_tail 0.5, hiss_mfloor 0.6, hiss_width 0.7, room_damp 0.3,
low_lag 0.02, low_smooth 0.05, low_db -5, low_width 0.75, hiss_db -20.

Design intent of step C: the brush leads and outlives the body, the low swell follows close behind the brush (not a
thump on the onset), and an independent fabric hiss and low band as side signals make the image wide (L/R correlation
near 0.6) with the mono sum unchanged.

### 3.4 Members (kind `swipe`)

| member | overrides of DEFAULTS | character |
|---|---|---|
| cloth | none | a sleeve or cloth swoop |
| paper | rise 0.12, fall 0.13, f_peak 520, q_air 1.1, low_db -6, low_f 150, grain_db -16, grain_rate 2500, grain_f 6000, hiss_db -19, hiss_lo 2000, hiss_rustle 0.5, rustle_f 150, width 0.3 | brighter, crackly, little low end |
| velvet | rise 0.2, fall 0.22, fall_k 0.6, hold 0.05, f_peak 260, low_db -1, low_f 95, low_smooth 0.07, low_lag 0.06, rustle 0.35, rustle_f 70, grain_db -24, hiss_db -19, flut_depth 0.4 | dark, slow, heavy |
| flick | rise 0.1, rise_k 0.8, fall 0.13, f_peak 450, flut_depth 0.55, flut_f 32, low_db -7, low_smooth 0.06, low_lag 0.03, grain_db -18, grain_rate 900, hiss_db -17, pan 0.3 | a quick sheet flick with flutter |
| glide | rise 0.18, hold 0.1, fall 0.16, fall_k 0.9, f_peak 450, up_oct 1.6, down_oct 1.6, q_air 1.0, rustle 0.2, hiss_rustle 0.35, low_db -5, grain_db -32, hiss_db -19, dop_st 2, dop_w 0.1, width 0.45, room_wet 0.18, rt60 0.5 | a smooth held UI glide |

## 4. brightswish

### 4.1 The sound

High, wide air flicks for text and pane moves, and a designed bright whip. Measured targets from the source's private
study set (facts, used as design goals):

- Bright swish: about 0.25 s long (0.21 to 0.29), centroid about 2.1 kHz (0.8 to 3.6 kHz), its loudest point about
  134 ms (115 to 150) after the first moment within 20 dB of the peak (it rises, then stops), fairly wide (L/R
  correlation 0.76, range 0.43 to 0.94).
- Designed whip: about 0.74 s, centroid about 2.7 kHz, decorrelated (correlation 0.07), reaches its peak in about
  15 ms, then falls about 45 semitones while it rings out (-20 dB about 0.45 s after the peak), with a comb-like fan of
  falling partials and a granular flutter.
- The whip always falls. A rising synth whip is out of scope for this recipe: no member may have a rising glide.

Build: the pass engine with the air band high (f_peak 2 to 5 kHz), no low weight or sub, the hiss about as loud as the
air body, a decorrelated hiss side and air side. The whip adds a fast rise, a band that sinks after the peak (track >
1, so it falls with the level), a deep fast flutter and one extra layer: a feedback comb whose fundamental falls over
the file.

Two categories: `swish` (target "a bright quick swish") and `whoosh_fast` (target "a bright whip whoosh").
Alternatives for both: "a short burst of steam", "a page being flipped", "a sword being sharpened", "rain". The refset
name is the category name.

### 4.2 Interface

- `COMB_SPACE`, table 4.3. `COMB_KEYS` = its 4 key names. `COMB_OFF` = comb_mix 0.0, comb_f0 1000, comb_f1 200,
  comb_fb 0.5.
- `SPACE` = the whoosh `SPACE` plus `COMB_SPACE`.
- `SWISH`, `WHIP`: the 2 bases (section 4.4). `BASES` = {swish: SWISH, whoosh_fast: WHIP}.
- `FAMILIES[cat]`: an ordered list of (member name, overrides) pairs (section 4.5).
- `DEFAULTS` = a copy of `SWISH`. `TARGETS` = {swish: "a bright quick swish", whoosh_fast: "a bright whip whoosh"}.
- `preset(cat, name)`: the base of cat, then the member's overrides.
- `preset_full(cat, params)`: the base of cat, then params.
- `recipe(cat='swish', name=None)`: an object with the category's target, the alternatives, `REFSET` = cat, `SPACE`,
  `DEFAULTS` = `preset(cat, name or <first member of cat>)`, `render`, module name `brightswish`.
- `render(p, rng)`: section 4.6.

The duration and apex knobs of the whoosh family (forge-core knobs, "dur shape" and "apex") apply to these members too.

### 4.3 COMB_SPACE

| key | lo | hi | scale | meaning |
|---|---|---|---|---|
| comb_mix | 0 | 1 | | parallel mix of the comb (0 = off below 0.01) |
| comb_f0 | 150 | 4000 | log | comb fundamental at the first sample (Hz) |
| comb_f1 | 40 | 1500 | log | comb fundamental at the last sample (Hz) |
| comb_fb | 0 | 0.92 | | comb feedback |

### 4.4 Bases

`SWISH` = whoosh `fast` (final), then all whoosh `OPTIONAL` off values, then `COMB_OFF`, then:

| key | value | key | value | key | value |
|---|---|---|---|---|---|
| rise | 0.13 | flut_f | 30 | grain_db | -50 |
| rise_k | 0.8 | low_db | -40 | room_wet | 0.08 |
| hold | 0.0 | sub_db | -50 | rt60 | 0.35 |
| fall | 0.07 | hiss_db | -6 | room_damp | 0.3 |
| fall_k | 1.1 | hiss_hp | 3000 | pan | 0.15 |
| start_db | -32 | hiss_open | 1.2 | width | 0.35 |
| track | 0.7 | hiss_pow | 1.3 | hiss_width | 0.5 |
| f_peak | 2300 | hiss_tail | none | drive_db | 1.0 |
| up_oct | 1.0 | hiss_pre | none | end_db | -62 |
| down_oct | 1.3 | hiss_mfloor | 0.0 | end_fade | 0.03 |
| dop_st | 3.0 | tail_sweep | 0.0 | eq_lo_db | -12 |
| dop_w | 0.03 | hiss_q | 0.8 | eq_lo_f | 300 |
| q_air | 1.4 | tone_db | -50 | eq_hi_db | 0.0 |
| lp_ratio | 2.6 | | | eq_hi_f | 6000 |
| flut_depth | 0.2 | | | | |

Note: low_db -40 is under the -39.5 gate, so the low weight layer is off. sub_db -50 is under -49.5, so the sub is off.

`WHIP` = `SWISH` with:

| key | value | key | value | key | value |
|---|---|---|---|---|---|
| rise | 0.02 | q_air | 2.5 | width | 0.6 |
| rise_k | 1.0 | lp_ratio | 2.2 | hiss_width | 1.0 |
| hold | 0.0 | flut_depth | 0.6 | pan | 0.1 |
| fall | 0.5 | flut_f | 32 | end_db | -60 |
| fall_k | 0.75 | hiss_db | -12 | end_fade | 0.06 |
| start_db | -30 | hiss_hp | 2500 | comb_mix | 0.5 |
| track | 1.2 | hiss_tail | 1.0 | comb_f0 | 2200 |
| f_peak | 4200 | room_wet | 0.15 | comb_f1 | 160 |
| up_oct | 0.5 | rt60 | 0.6 | comb_fb | 0.7 |
| down_oct | 3.5 | | | | |
| dop_st | 6.0 | | | | |
| dop_w | 0.05 | | | | |

### 4.5 Members

These members were hand-designed corners of the bases, then tuned toward measured features. The values are facts
of the design, given to 4 significant digits.

Category `swish`, member order: bright, flick.

`bright` (the bright swish of music videos: about 2.4 kHz, a 100 ms rise, a quick fall, fairly wide), overrides of SWISH:
rise_k 0.9287, hold 0.05657, fall_k 1.127, track 0.6361, up_oct 1.004, down_oct 2.161, q_air 2.813,
flut_depth 0.3016, flut_f 33.74, hiss_db -3.588, hiss_hp 2054, hiss_open 1.037, hiss_pow 1.567, grain_rate 2789,
grain_f 2753, room_wet 0.0, rt60 0.4884, pan 0.1457, width 0.4008, eq_lo_db -10.06, eq_hi_db -12.75,
hiss_width 0.7457, comb_f0 327.0, comb_f1 66.57, comb_fb 0.5578.
(comb_mix stays 0, so the comb is off. grain_db stays -50, so the grains are off. room_wet 0 turns the reverb off.)

`flick` (rises and stops dead: higher, fluttering, decorrelated, a touch of falling comb), overrides of SWISH:
rise 0.09435, rise_k 0.45, fall 0.03, fall_k 1.335, track 0.7737, f_peak 800.3, up_oct 1.338, down_oct 3.046,
dop_st 2.201, q_air 0.6, lp_ratio 2.779, flut_depth 0.524, flut_f 44.69, hiss_db -2.058, hiss_hp 3720,
hiss_open 0.7442, hiss_pow 1.669, grain_rate 381.1, grain_f 3588, room_wet 0.06281, rt60 0.307, pan 0.05213,
width 0.5124, end_fade 0.02, eq_lo_db -12.61, eq_hi_db 0.8779, hiss_width 1.0, comb_mix 0.3188, comb_f0 4000,
comb_f1 99.3, comb_fb 0.4513.

Category `whoosh_fast`, one member: `whip` (a 20 ms hit, a band and a ringing comb falling about 45 semitones, wide),
overrides of WHIP:
rise 0.02, rise_k 0.7391, hold 0.05657, fall_k 1.127, track 1.071, f_peak 4000, up_oct 0.2647, dop_st 10.95,
q_air 3.505, lp_ratio 1.2, flut_f 25.21, hiss_db 0.0, hiss_hp 1807, hiss_open 0.0, hiss_pow 1.51, grain_db -46.8,
grain_rate 50, grain_f 5843, room_wet 0.03593, rt60 0.6411, pan 0.2634, eq_lo_db -15.23, eq_hi_db 1.169,
comb_mix 1.0, comb_f0 1464, comb_f1 40, comb_fb 0.7323.

### 4.6 render(p, rng)

1. q = SWISH, then p on top.
2. y = the whoosh render of q without the 4 comb keys. (The whoosh render sees a full set, so every optional key has
   the value of q.)
3. Falling comb (only when comb_mix ≥ 0.01), per channel:
   - u runs linearly from 0 at the first sample to 1 at the last (u = i / max(1, n - 1)).
   - Delay in samples: D(i) = SR / (comb_f0 × (comb_f1 / comb_f0)^u). The fundamental falls exponentially from
     comb_f0 to comb_f1 over the file.
   - Feedback comb with a fractional, time-varying delay (P4): z[i] = x[i] + comb_fb × y_c[i - D(i)], where y_c is
     the comb output, read with linear interpolation between the 2 neighbouring past outputs. Only read when
     i - D(i) ≥ 1; else z[i] = x[i].
   - Scale z by (1 - comb_fb). Output = x + comb_mix × (z - x).
4. Fade out over min(end_fade, 0.3 × duration) s with a raised cosine (P9), no fade in.
5. Normalise to an absolute peak of 0.89.
