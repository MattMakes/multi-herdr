# recipes-motion, part 4: tonal cues (notify, shimmer, hover)

This part belongs to the index `recipes-motion.md`. Read the index first: it defines the recipe contract, the shared
primitives (P1 to P26) and the seed-bank format. Notation used here:

- t = sample index / 48000 (seconds); "white" = standard normal samples from the render generator; U(a, b) = a
  uniform draw from the render generator; N(0, 1) = a standard normal draw.
- `bw-lp(f, k)`, `bw-hp(f, k)`, `bw-bp(lo, hi, k)` = Butterworth low-, high- and band-pass of order k (P3).
- "raised-cosine rise over k samples" = multiply the first k samples by 0.5 - 0.5·cos(π·i/(k - 1)), i = 0 .. k - 1.
- "cos² fade over k samples" = multiply the last k samples by cos²(π/2 · i/(k - 1)), i = 0 .. k - 1 (convex: it
  holds, then stops).
- "unit peak" = divide by (max |x| + 1e-12).
- `decor(x, a, cut)` = the forge-core mono-to-stereo decorrelator (P18). `verb(...)` = the forge-core reverb (P10)
  with the named arguments; arguments not named take the forge-core defaults (wet 0.3, dry 1.0).
- `sat(x, dB, ...)` = the forge-core saturator (P11). `peq(x, f, g, Q)` = peaking EQ (P24).

Random draws: the spec names each random quantity and its distribution. A clean build need not repeat the source's
draw order (index, section "Behaviour").

## 12. notify

### 12.1 The sound

A short bright notification ding for motion design. Facts measured on the 4 dings of one pro launch film:

- Pitch: a fundamental between 1.8 and 2.2 kHz plus a single non-harmonic partial at about 3.4 times it (6.3 to
  6.9 kHz). No energy below 500 Hz.
- Two stages. Stage 1 (the head) is bright and stays nearly level for 30 to 50 ms. A gate then lowers it by 18 to
  22 dB within a few ms. Stage 2 (the body) is about 20 dB lower and lasts to 0.22 to 0.36 s, with a tremolo of 20
  to 30 Hz. The body has lines at 0.5, 1.5 and 3.5 times the head pitch. The level holds, then stops (a convex
  fade).
- Attack 1 to 15 ms (a step at about -30 dB, then a short rise). t20 27 to 42 ms, t40 30 to 180 ms. Near mono: side
  5 to 13 dB under mid.
- A sequence of dings steps up 1 semitone per ding (1822, 1934, 2045, 2168 Hz).

Real notification bells (the `notify` refset: struck bells, glass, kalimba) ring out for 0.8 to 2 s.

Target "a short bright notification ding". Alternatives: "a phone ringtone", "an alarm clock beep", "a wine glass
clink", "a xylophone note", "a typewriter bell". Refset `pro_notify`.

Styles (`style`):

| style | sound |
|---|---|
| ding | the two-stage pro ding: head, gated drop, tremolo body; 0.2 to 0.4 s |
| success | 2 dings with one parameter set; the second is `interval` semitones higher and starts `gap` s after the first |
| bell | a modal struck tone (bell, glass, celesta or bar) with a free ring-out; 0.6 to 1.8 s (up to 2.6 s) |
| chime | (round 6) a struck note of a chosen material with a two-rate decay and an optional 2-note motif |

### 12.2 Interface

Module constants: `SR` = 48000, `TARGET`, `ALTS`, `REFSET` = 'pro_notify', `SEMI` = 2^(1/12), `SPACE`, `DEFAULTS`,
`PARTIALS`, `MATS`, `FAMILY` (keys `pro_notify`, `notify`), `FAMILY_NAMES`, `FAMILY_R3`, `FAMILY_R5`,
`FAMILY_NAMES_R5`, `FAMILY_R6`, `FAMILY_NAMES_R6`, `FAMILY_MID`, `FAMILY_NAMES_MID`, `FINE`, `R4_EDITS`, `RING_KEYS`,
`REGIONS`, `TARGETS`.

Functions: `render(p, rng)`, `family_space(params, style=None)`, `ring_scaled(p, ring=None)`, `measure(x)`,
`pro_penalty(x, cat)`, `extra_for(cat, weight=1.0)`, and an outside-distance helper (12.12) that shimmer also uses.
`measure` is shared: shimmer imports it.

### 12.3 SPACE and DEFAULTS

| key | range | scale | default | meaning |
|---|---|---|---|---|
| style | ding, success, bell, chime | choice | ding | 'chime' is last, so older choice indices do not change |
| f0 | 1100 to 2600 | log | 1822 | head fundamental (Hz) |
| att | 0.0008 to 0.012 | log | 0.003 | rise time after the onset step (s) |
| step_db | -36 to -18 | | -30 | onset step level before the rise |
| hold | 0.018 to 0.07 | | 0.032 | head hold time (s) |
| hold_tau | 0.04 to 0.4 | log | 0.15 | sag time constant during the hold |
| drop_db | -26 to -12 | | -24 | gate floor (head level after the drop) |
| drop_ms | 2 to 14 | | 5 | drop time |
| ring_tau | 0.02 to 0.15 | log | 0.05 | decay of the head after the drop |
| bright | 0.05 to 0.9 | | 0.35 | level of the inharmonic head partial |
| p2 | 3.2 to 3.6 | | 3.42 | its ratio |
| oct | 0 to 0.3 | | 0.06 | level of a 2 × partial |
| detune | 0 to 3 | | 1.2 | doublet beat (Hz) |
| tick_db | -40 to -18 | | -30 | mallet tick level |
| tick_f | 4000 to 11000 | log | 7000 | tick band centre |
| body_db | -26 to -12 | | -23 | body peak level re head |
| body_ratio | 0.5, 1.0, 1.5 | choice | 0.5 | body fundamental / f0 |
| body_in | 0.004 to 0.03 | | 0.012 | body fade-in |
| body_tau | 0.08 to 0.6 | log | 0.3 | body decay |
| body_p3 | 0 to 0.8 | | 0.45 | body partial at 3 × |
| body_p7 | 0 to 0.6 | | 0.6 | body partial at 6.965 × |
| trem_hz | 12 to 34 | | 24 | body tremolo rate |
| trem | 0 to 0.85 | | 0.55 | tremolo depth |
| drive | 0 to 9 | | 3 | saturation (dB) |
| verb | 0 to 0.35 | | 0.12 | plate send |
| rt60 | 0.15 to 0.7 | log | 0.3 | |
| width | 0 to 1 | | 0.8 | body and reverb decorrelation |
| head_width | 0 to 0.35 | | 0.15 | head decorrelation |
| length | 0.2 to 0.42 | | 0.26 | file length (s) |
| fade | 0.02 to 0.09 | | 0.07 | end fade |
| hp | 450 to 900 | log | 550 | high-pass (Hz) |
| interval | 3, 4, 5, 7 | choice | 7 | success / chime 'up': semitones to the second note |
| gap | 0.05 to 0.13 | | 0.085 | second-note delay (s) |
| second_db | -6 to 1 | | -2 | second-note level |
| partials | celesta, glass, bar, bowl, tine | choice | celesta | bell partial set |
| decay | 0.2 to 1.6 | log | 0.8 | bell fundamental decay (s) |
| slope | 0.4 to 1.2 | | 0.8 | upper partials decay faster: tau ∝ ratio^-slope |
| hard | 0 to 1 | | 0.4 | mallet hardness |
| beat | 0.3 to 4 | log | 1.0 | bell doublet AM rate (Hz) |
| room | 0 to 0.3 | | 0.12 | bell room send |
| early_db | 0 to 18 | | 0 | (round 3) bell early fast decay, dB lost in about 0.1 s |
| early_tau | 0.02 to 0.12 | log | 0.05 | its time constant |
| head_drive | -1 to 9 | | -1 | (round 3) ding head drive; < 0 = use `drive` |
| air | -60 to -10 | | -60 | (round 3) band-noise breath under the head, dB re head; ≤ -59 = off |
| air_tau | 0.02 to 0.2 | log | 0.06 | its decay |
| air_lo | 0 to 12000 | | 0 | (round 4) breath lower band edge; 0 = min(3000, 1.4·f0) |
| p3 | 0 to 0.5 | | 0 | (round 3) struck-bar head partials at 2.756 × and 5.404 × |
| p3_tau | 0.008 to 0.06 | log | 0.02 | their decay |
| strike | -60 to -4 | | -60 | (round 3) bell strike modes, dB re ring; ≤ -59 = off |
| strike_tau | 0.006 to 0.04 | log | 0.015 | their decay |
| strike_max | 6 to 20 | | 20 | (round 4) highest strike-mode ratio kept |
| mat | film, glass, bowl, tine, wood, fm | choice | film | (round 6) chime material (12.5) |
| c_fast | 0.5 to 0.97 | | 0.9 | chime share of the fast decay |
| c_tf | 0.004 to 0.03 | log | 0.012 | fast decay time constant |
| c_ts | 0.06 to 0.4 | log | 0.15 | slow decay time constant |
| c_mallet | -60 to -6 | | -30 | chime mallet noise level; ≤ -59 = off |
| c_hard | 0 to 1 | | 0.5 | felt (0) to metal (1) |
| c_bend | 0 to 60 | | 0 | strike pitch drop (cents) |
| c_sub | 0 to 0.6 | | 0 | octave-under body level |
| c_motif | single, up, flam | choice | single | one strike; a second note up; a quieter second strike on the same note |
| c_index | 0.5 to 6 | | 2.5 | 'fm' material: peak modulation index |
| c_itau | 0.006 to 0.06 | log | 0.02 | its decay |
| ring | 0.1 to 4 | log | 1.0 | decay knob: multiplies the time constants that 12.8 lists for the style |
| soft | 0 to 0.08 | | 0 | soft-mallet onset: the whole output fades in along a raised cosine of `soft` s |
| bell_len | 0 to 2.6 | | 0 | bell file length; 0 = max(length, 0.5) |

The source builds the chime keys and the knob keys (ring, soft, bell_len) in later steps. Family members created
before those steps do not contain them. `render` merges DEFAULTS first, so they get the default values. With ring 1,
soft 0 and bell_len 0, the knobs have no effect.

### 12.4 PARTIALS (bell style)

| set | ratios | amplitudes |
|---|---|---|
| celesta | 1, 2, 3.98, 6.9, 10.1 | 1, 0.06, 0.22, 0.05, 0.03 |
| glass | 1, 2.32, 4.25, 6.63, 9.38 | 1, 0.45, 0.25, 0.12, 0.06 |
| bar | 1, 2.756, 5.404, 8.933 | 1, 0.35, 0.14, 0.06 |
| bowl | 1, 2.71, 5.13, 8.27 | 1, 0.6, 0.3, 0.12 |
| tine | 1, 2, 5.8, 6.27 | 1, 0.12, 0.12, 0.25 |

### 12.5 MATS (chime style)

Each entry is (ratio, amplitude, decay multiplier on both decay rates). The fundamental is the strongest in every
set.

| mat | partials |
|---|---|
| film | (1, 1, 1), (3.42, 0.3, 0.45) |
| glass | (1, 1, 1), (2.32, 0.3, 0.5), (3.4, 0.28, 0.35), (4.25, 0.12, 0.25), (6.63, 0.05, 0.15) |
| bowl | (1, 1, 1), (2.71, 0.4, 0.6), (3.38, 0.22, 0.4), (5.13, 0.14, 0.3) |
| tine | (1, 1, 1), (3.44, 0.12, 0.3), (5.8, 0.08, 0.25), (6.27, 0.1, 0.2) |
| wood | (1, 1, 1), (3.4, 0.18, 0.3), (3.98, 0.12, 0.22), (9.2, 0.03, 0.1) |
| fm | (1, 1, 1) (not used as partials: 'fm' is a 1 : 2.4 FM pair, 12.10) |

### 12.6 Families

Every member is DEFAULTS overlaid with the values in its column. The ladder is f0 = 1822 × SEMI^k, k = 0 .. 4:
1822.0, 1930.34, 2045.13, 2166.74, 2295.58 Hz (L0 .. L4).

**FAMILY['pro_notify'] (round 5, delivered).** A clean base, then per-member changes. Names in `FAMILY_NAMES`.

Clean base: style ding, step_db -30, hold 0.034, hold_tau 0.25, drop_db -20, drop_ms 4, ring_tau 0.05, bright 0.25,
p2 3.42, oct 0, detune 0.8, tick_db -30, tick_f 9000, body_db -22, body_ratio 0.5, body_in 0.03, body_tau 0.13,
body_p3 0.25, body_p7 0.2, trem 0.35, trem_hz 24, drive 0, head_drive 0, air -60, p3 0, verb 0.15, rt60 0.3,
width 0.7, head_width 0.15, length 0.3, fade 0.05, hp 500.

| name | f0 | changes over the clean base |
|---|---|---|
| ding_1822 | L0 | att 0.0015, length 0.26 |
| ding_1930 | L1 | att 0.012, step_db -26, hold 0.04, body_db -21, width 0.9, head_width 0.25, length 0.27 |
| ding_2045 | L2 | att 0.003, hold 0.036, body_tau 0.2, body_db -20, length 0.36, trem 0.45 |
| ding_2167 | L3 | att 0.011, step_db -26, hold 0.04, bright 0.32, width 0.9, head_width 0.25, length 0.3 |
| ding_2296 | L4 | att 0.004, hold 0.03, bright 0.2, body_ratio 1.0, body_p3 0.15, body_p7 0, body_db -19, body_tau 0.16, trem 0.25, length 0.26 |

`FAMILY_R5['pro_notify']` is a copy of this list. `FAMILY_NAMES_R5` is a copy of the names.

**FAMILY_R3['pro_notify'] (round 3, the earlier delivered set).** Columns m1 to m5.

| key | m1 | m2 | m3 | m4 | m5 |
|---|---|---|---|---|---|
| style | success | ding | ding | ding | ding |
| f0 | L0 | L2 | L0 | L1 | L3 |
| interval | 7 | 7 | 7 | 7 | 7 |
| att | 0.002 | 0.002 | 0.0015 | 0.002 | 0.003 |
| step_db | -28 | -30 | -22 | -34 | -20 |
| hold | 0.026 | 0.036 | 0.03 | 0.022 | 0.018 |
| hold_tau | 0.05 | 0.1 | 0.1 | 0.2 | 0.05 |
| drop_db | -12 | -14 | -13 | -12 | -12 |
| drop_ms | 12 | 8 | 6 | 8 | 5 |
| ring_tau | 0.07 | 0.06 | 0.05 | 0.045 | 0.04 |
| bright | 0.3 | 0.2 | 0.35 | 0.5 | 0.7 |
| p2 | 3.4 | 3.55 | 3.44 | 3.46 | 3.3 |
| oct | 0.12 | 0.15 | 0.05 | 0.05 | 0.08 |
| detune | 0.6 | 2.0 | 1.0 | 0.5 | 0.8 |
| tick_db | -26 | -24 | -22 | -25 | -26 |
| tick_f | 9000 | 9000 | 7000 | 9500 | 8000 |
| body_db | -20 | -18 | -19 | -20 | -20 |
| body_ratio | 0.5 | 0.5 | 0.5 | 1.0 | 1.5 |
| body_in | 0.02 | 0.015 | 0.006 | 0.02 | 0.011 |
| body_tau | 0.14 | 0.16 | 0.13 | 0.12 | 0.12 |
| body_p3 | 0.5 | 0.7 | 0.45 | 0.7 | 0.6 |
| body_p7 | 0.3 | 0.45 | 0.3 | 0.2 | 0.4 |
| trem | 0.45 | 0.5 | 0.55 | 0.25 | 0.6 |
| trem_hz | 24 | 18 | 24 | 24 | 30 |
| drive | 2 | 3 | 3 | 3 | 3 |
| head_drive | 0 | 0 | 0 | 0 | 0 |
| air | -19 | -18 | -17 | -19 | -18 |
| air_tau | 0.05 | 0.06 | 0.04 | 0.05 | 0.035 |
| p3 | 0.3 | 0.25 | 0.35 | 0.2 | 0.25 |
| p3_tau | 0.02 | 0.025 | 0.018 | 0.015 | 0.012 |
| verb | 0.22 | 0.22 | 0.12 | 0.3 | 0.16 |
| rt60 | 0.3 | 0.3 | 0.35 | 0.3 | 0.5 |
| width | 0.8 | 1.0 | 0.8 | 0.8 | 0.8 |
| head_width | 0.25 | 0.3 | 0.2 | 0.2 | 0.25 |
| length | 0.4 | 0.34 | 0.28 | 0.26 | 0.24 |
| fade | 0.05 | 0.06 | 0.05 | 0.06 | 0.05 |
| hp | 500 | 450 | 600 | 520 | 450 |
| gap | 0.06 | 0.085 | 0.085 | 0.085 | 0.085 |
| second_db | -1.5 | -2 | -2 | -2 | -2 |

**FAMILY['notify'] (round 3 values).** All style bell.

| member | f0 | partials | decay | slope | hard | early_db | early_tau | strike | strike_tau | beat | length | room | hp | tick_db | drive | width |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| celesta C5 | 523.25 | celesta | 0.5 | 1.0 | 0.7 | 8 | 0.05 | -14 | 0.015 | 0.8 | 2.3 | 0.12 | 180 | -26 | 1.0 | 0.2 |
| glass A5 | 880 | glass | 0.42 | 1.1 | 0.75 | 7 | 0.04 | -12 | 0.012 | 1.6 | 2.0 | 0.15 | 300 | -24 | 1.0 | 0.2 |
| bar G5 | 783.99 | bar | 0.38 | 1.0 | 0.85 | 8 | 0.035 | -10 | 0.01 | 1.0 | 1.8 | 0.1 | 300 | -22 | 1.5 | 0.2 |
| small bell A4 | 440 | bowl | 0.6 | 0.9 | 0.6 | 10 | 0.07 | -16 | 0.02 | 1.2 | 2.5 | 0.15 | 150 | -30 | 1.0 | 0.25 |
| tine D5 | 587.33 | tine | 0.36 | 1.1 | 0.65 | 5 | 0.04 | -18 | 0.012 | 0.6 | 1.8 | 0.1 | 150 | -26 | 1.0 | 0.15 |
| celesta E6 | 1318.5 | celesta | 0.34 | 1.0 | 0.8 | 7 | 0.03 | -12 | 0.01 | 0.9 | 1.7 | 0.12 | 300 | -22 | 1.0 | 0.2 |

**R4_EDITS.** `R4_EDITS['notify']` is a list of 6 edits, one per `notify` member in table order. Each edit sets
strike_max 12 and slope 1.1, and these (hard, strike) pairs: (0.55, -20), (0.6, -18), (0.65, -17), (0.48, -22),
(0.5, -24), (0.62, -18). The delivered `notify` members are the round-3 members with their edit applied. The table
above holds the round-3 values without the edit. `R4_EDITS['pro_notify']` = 5 copies of {body_db: -13}. It was not
delivered.

**FAMILY_R6['pro_notify'] (round 6, not delivered).** Names in `FAMILY_NAMES_R6`. Chime base: style chime, drive 0,
head_drive 0, hp 500, verb 0.14, rt60 0.3, width 0.7, fade 0.05, detune 0.8.

| name | definition |
|---|---|
| film_ding | a copy of round-5 member ding_1822 (style ding) |
| fm_bell | chime base + f0 L1, mat fm, att 0.005, c_fast 0.9, c_tf 0.011, c_ts 0.16, c_index 1.8, c_itau 0.015, c_mallet -40, c_hard 0.4, length 0.3, detune 0 |
| wood | chime base + f0 L4, mat wood, att 0.002, c_fast 0.95, c_tf 0.007, c_ts 0.09, c_mallet -14, c_hard 0.1, length 0.22, c_bend 40, detune 0, verb 0.1 |
| ding_dong | chime base + f0 L3, mat glass, att 0.0015, c_fast 0.9, c_tf 0.012, c_ts 0.12, c_mallet -26, c_hard 0.7, length 0.4, c_motif up, interval -4, gap 0.11, second_db -2 |
| octave_body | chime base + f0 L2, mat film, att 0.0015, c_fast 0.9, c_tf 0.013, c_ts 0.1, c_mallet -30, c_hard 0.7, length 0.3, c_sub 0.1 |

Note: ding_dong uses interval -4, which is outside the SPACE choices. `render` accepts any number.

**FAMILY_MID['mid_ding'] (the rounded mid ding of the decay-knob study).** Names in `FAMILY_NAMES_MID` = tine,
wood. Base: style chime, drive 0, c_motif single.

| key | tine | wood |
|---|---|---|
| f0 | 830.61 | 880 |
| mat | tine | wood |
| att | 0.00316 | 0.00814 |
| detune | 0.9 | 1.68 |
| verb | 0.255 | 0.35 |
| rt60 | 0.358 | 0.363 |
| width | 0.313 | 0.229 |
| length | 0.425 | 0.317 |
| fade | 0.0435 | 0.0613 |
| hp | 135.6 | 254.7 |
| c_fast | 0.623 | 0.596 |
| c_tf | 0.0187 | 0.0291 |
| c_ts | 0.0623 | 0.077 |
| c_mallet | -59 | -25.3 |
| c_hard | 1.0 | 0.058 |
| c_bend | 18.8 | 22.9 |
| c_sub | 0.45 | 0.45 |
| c_index | 1.655 | 4.38 |
| c_itau | 0.0097 | 0.0064 |
| ring | 0.82 | 0.65 |
| soft | 0.049 | 0.047 |

Note: the mallet is on only when c_mallet > -59 (12.10). The tine member has c_mallet -59, so it has no mallet.

### 12.7 FINE and family_space

`FINE[style]` lists the keys a forge run may tune for one member. All other keys stay fixed.

| style | free keys |
|---|---|
| ding | att, step_db, head_width, hold_tau, drop_db, drop_ms, ring_tau, p2, oct, detune, tick_db, tick_f, body_in, body_tau, body_p3, body_p7, drive, verb, rt60, fade, hp, air, air_tau, p3, p3_tau |
| success | the ding list + gap, second_db |
| bell | slope, hard, beat, room, tick_db, tick_f, drive, fade, att, early_db, early_tau, strike, strike_tau |
| chime | att, c_fast, c_tf, c_ts, c_mallet, c_hard, c_bend, detune, verb, rt60, fade, hp |

`family_space(params, style=None)`: style = the argument, else params['style'], else 'ding'. For each SPACE key: a
free key keeps its SPACE spec. Any other key becomes a one-element list [params[k]] (or [DEFAULTS[k]] when params
lacks it). A style without a FINE entry pins every key.

### 12.8 The decay knob

`RING_KEYS`: bell → decay, early_tau, strike_tau. ding and success → hold_tau, ring_tau, body_tau, air_tau, p3_tau.
chime → c_tf, c_ts, c_itau.

`ring_scaled(p, ring=None)`: ring = the argument, else p['ring'], else 1. If ring = 1, return a copy of p. Else
return p with each RING_KEYS[style] key (present in p) multiplied by ring. A style that is not in RING_KEYS changes
nothing.

Soft onset (key `soft` = s > 0): k = min(len, P1(s)). If k > 1, apply a raised-cosine rise over k samples to the
whole signal (all channels).

### 12.9 Common pieces

**Partial.** partial(f, amp, detune d): φ = U(0, 2π). y = sin(2πft + φ). If d > 0: y = (y + 0.6·sin(2π(f +
d·U(0.6, 1.4))t + φ)) / 1.6. Both tones start in phase, then beat. Return amp·y.

**Tick** (ding and bell). k = min(n, P1(0.004)). w = bw-bp(tick_f/1.6, min(1.6·tick_f, 20000), 2) of k white samples,
times exp(-t/0.0007), unit peak, times 10^(tick_db/20). It sits at sample 0 of an n-sample zero buffer.

**Finish(x, drive override)** (mono). drive = p['drive'], unless an override ≥ 0 is given. If drive > 0.2:
x = sat(x, drive, mix 0.6) (other saturator arguments at defaults). Then x = bw-hp(hp, 4).

**End(y, fade f).** Cut or zero-pad y to n_len samples. k = min(len, P1(f)), with f = the given fade or p['fade'].
Apply a cos² fade over the last k samples. Then a raised-cosine rise over the first P1(0.0003) samples.

### 12.10 render(p, rng)

1. p = DEFAULTS overlaid with p. If p['ring'] ≠ 1, p = ring_scaled(p).
2. n_len = P1(L) with L = length, or for style bell L = bell_len if bell_len > 0, else max(length, 0.5).
   n = n_len + P1(0.05). The 50 ms margin is rendered and then cut by End.
3. Style bell: y = bell(p, n) (below); soft onset; Finish(y); y = decor(y, 0.5·width, cut 600); if room > 0:
   y = verb(y, rt60 0.35 + 1.5·room, size 0.35, predelay 0.008, damp 0.55, wet room, stereo, width 0.8). If early_db > 0
   or strike > -59: End with fade max(fade, 0.25). Else End with fade max(fade, 0.4·n_len/48000).
4. Style chime: y = chime(p, n) (below); soft onset; Finish(y); y = decor(y, 0.5·width, cut 1200). If verb > 0.005:
   wet = verb(y, rt60, size 0.25, predelay 0.006, damp 0.45, wet 1, dry 0, stereo, width 0.4 + width); zero-pad y to
   the length of wet; y = y + verb·wet. End(y).
5. Styles ding and success:
   1. (head, body) = ding(p, n).
   2. success: q = p with f0·SEMI^interval. (h2, b2) = ding(q, n). g = 10^(second_db/20), off = P1(gap). Delay h2 and
      b2 by off samples (cut to n) and scale by g. body = body·clip(1 - t/(gap + 0.03), 0.25, 1) + b2 (the first
      body ducks under the second). head = head + h2.
   3. head = Finish(head, override head_drive). body = Finish(body).
   4. S = decor(body, width, cut 600). y = decor(head, head_width, cut 1500) + S.
   5. If verb > 0.005: send = 0.3·[head, head] + S (the plate is fed mainly by the body, so the head gate stays a
      gate). wet = verb(send, rt60, size 0.25, predelay 0.006, damp 0.45, wet 1, dry 0, stereo, width 0.4 + width).
      Zero-pad y to the length of wet. y = y + verb·wet.
   6. Soft onset. End(y).

**ding(p, n)** returns (head, body), both mono, both starting at t = 0.

1. head = partial(f0, 1, detune).
2. If f0·p2 < 0.45·SR: head += bright·exp(-t/0.09)·partial(f0·p2, 1, 0).
3. If oct > 0: head += oct·partial(2·f0, 1, 0).
4. If p3 > 0: for (q, a) in (2.756, 1.0), (5.404, 0.6), if f0·q < 0.4·SR:
   head += p3·a·exp(-t/(p3_tau·2.756/q))·partial(f0·q, 1, 1.5).
5. Head envelope (multiply head by e):
   - s = 10^(step_db/20). rise = s + (1 - s)·(0.5 - 0.5·cos(π·clip(t/att, 0, 1))).
   - sag = exp(-t/hold_tau).
   - dt = max(drop_ms/1000, 0.001). fl = 10^(drop_db/20). u = clip((t - hold)/dt, 0, 1).
     gate = 1 - (1 - fl)·(0.5 - 0.5·cos(πu)).
   - after = exp(-(t - hold - dt)/ring_tau) for t > hold + dt, else 1.
   - e = rise·sag·gate·after, and e[0] = 0.
6. Air (if air > -59): lo = air_lo if air_lo > 0, else min(3000, 1.4·f0). w = bw-bp(lo, 16000, k) of n white samples,
   k = 2 if lo < 6000 else 4; scale w to unit RMS. ea = exp(-t/air_tau), its first P1(0.001) samples times a linear
   ramp 0 to 1. head += 0.7·10^(air/20)·w·ea. The air is added after the head envelope, so the gate does not cut it.
7. head += Tick.
8. Body: F = f0·body_ratio. body = partial(F, 1, 0.8). norm = 1. For (m, a) in (3, body_p3), (6.965, body_p7): if
   a > 0 and F·m < 0.45·SR: body += a·partial(F·m, 1, 0); norm += a. body = body/norm.
9. Body envelope: t0 = 0.6·hold. ein = clip((t - t0)/max(body_in, 0.001), 0, 1).
   env = (0.5 - 0.5·cos(π·ein))·exp(-max(t - t0, 0)/body_tau).
   trem = 1 - trem_depth·(0.5 - 0.5·cos(2π·trem_hz·t + θ)), θ = U(0, 1) (radians: the source draws the tremolo
   phase from 0 to 1, not 0 to 2π).
   body = body·env·trem·10^(body_db/20).

**bell(p, n)** returns a mono signal.

1. (ratios, amps) = PARTIALS[partials]. ge = 10^(early_db/20) - 1.
2. For each (q, a), with f = f0·q, skipped if f > 0.45·SR:
   - a' = a·q^(1.5·(hard - 0.5)). tau = max(decay·q^-slope, 0.004).
   - b = beat·U(0.5, 1.5). am = 1 - 0.35·(0.5 - 0.5·cos(2πbt)) (loudest at the strike).
   - env = exp(-t/tau). If ge > 0: env = env·(1 + ge·min(1, 0.5 + 0.25q)·exp(-t/early_tau)).
   - y += a'·env·am·sin(2πft + U(0, 6.3)).
3. If ge > 0: y = y/(1 + 0.75·ge).
4. Strike modes (if strike > -59): for q in 6.1, 8.7, 11.9, 15.3, 19.6: f = f0·q·U(0.98, 1.02); skip if f > 17000 or
   q > strike_max; ys += U(0.5, 1)·exp(-t/(strike_tau·√(6.1/q)))·sin(2πft + U(0, 6.3)). y += 10^(strike/20)·ys.
5. Raised-cosine rise over P1(max(0.5·att, 0.0005)) samples.
6. Mallet: k = min(n, P1(0.006)). m = bw-lp(2000 + 14000·hard, 2) of k white samples, times exp(-t/0.001).
   y[0:k] += (0.05 + 0.25·hard)·(m unit peak)·max|y|.
7. y += Tick·max|y| (the peak after step 6).

**strike(p, n, f0)** (chime) returns a mono signal.

1. bend(t) = 2^((c_bend/1200)·exp(-t/0.01)) (a hard strike starts sharp, then settles). a = max(att, 0.0006).
   rise = 0.5 - 0.5·cos(π·clip(t/a, 0, 1)). tm = max(t - a, 0).
2. mat fm: idx = c_index·exp(-tm/c_itau) + 0.25. Modulator phase = 2π·cumsum(2.4·f0·bend)/SR + U(0, 6.3). Carrier
   phase = 2π·cumsum(f0·bend)/SR + U(0, 6.3). env = c_fast·exp(-tm/c_tf) + (1 - c_fast)·exp(-tm/c_ts).
   y = sin(carrier + idx·sin(modulator))·env. (Sidebands fall at 1.4 × and 3.4 ×.)
3. Other mats: for each (q, a, dm) in MATS[mat], f = f0·q, skipped if f > 0.42·SR: ph = 2π·cumsum(f·bend)/SR +
   U(0, 6.3). tone = sin(ph). If detune > 0: tone = (tone + 0.5·sin(ph + 2π·detune·U(0.6, 1.4)·t))/1.5.
   env = c_fast·exp(-tm/(c_tf·dm)) + (1 - c_fast)·exp(-tm/(c_ts·dm)). y += a·env·tone.
4. If c_sub > 0: y += c_sub·(0.5 - 0.5·cos(π·clip(t/0.02, 0, 1)))·exp(-tm/c_ts)·sin(2π·cumsum(0.5·f0·bend)/SR +
   U(0, 6.3)).
5. y = y·rise.
6. Mallet (if c_mallet > -59): k = min(n, P1(0.008)), h = c_hard. w = bw-bp(1500 + 4000h, min(4000 + 12000h,
   18000), 2) of k white samples, times (1 - exp(-t/0.0003))·exp(-t/(0.0035 - 0.0027h)).
   y[0:k] += 10^(c_mallet/20)·(w unit peak).

**chime(p, n)**: y = strike(p, n, f0). If c_motif is up or flam: off = P1(gap); f2 = f0·SEMI^interval (up) or f0
(flam); y2 = strike(p, n - off, f2). For up only: y = y·clip(1 - t/(gap + 0.04), 0.3, 1). Then
y[off:] += 10^(second_db/20)·y2.

### 12.11 measure(x)

Shimmer imports this function. Input mono or (n, 2); mono is used for both channels.
m = mean of the 2 channels. "Hann(N)" is the symmetric Hann window, w[i] = 0.5 - 0.5·cos(2πi/(N - 1)).

Block level env_db(m, ms): h = max(1, int(48000·ms/1000)), k = len(m) // h. If k < 1, return [-120]. Else the RMS of
each full h-sample block, + 1e-12, in dB.

| field | definition |
|---|---|
| length_s | len(m)/48000, rounded to 3 decimals |
| attack_ms | e1 = env_db(m, 1); pk = argmax(e1); first = first index with e1 ≥ e1[pk] - 20; attack_ms = pk - first |
| t20_ms, t40_ms | e2 = env_db(m, 2) smoothed by a 5-point moving average (same length, zero padding at the ends); p = argmax(e2); the first index j ≥ 0 after p where e2[p + j] < e2[p] - 20 (or - 40), times 2 ms; null if never |
| peak_t | pk/1000, rounded to 4 decimals |
| shares | P = \|rfft(m·Hann(len))\|², tot = ΣP + 1e-20; for each of REGIONS = (0, 150), (150, 500), (500, 1000), (1000, 4000), (4000, 12000), (12000, 24000) Hz (lower edge in, upper edge out): 10·log10(ΣP_band/tot + 1e-12), rounded to 1 decimal |
| tilt | 21 third-octave centres c = 1000·2^(k/3), k = -10 .. 10; band [c/2^(1/6), c·2^(1/6)); level = max(-60, 10·log10(ΣP_band/tot + 1e-12)); tilt = slope of the least-squares line of level against log2(c) (dB/octave), rounded to 2 decimals |
| corr | Pearson correlation of L and R if both have non-zero standard deviation, else 1.0; 3 decimals |
| side_mid_db | M = (L + R)/2, S = (L - R)/2; 10·log10(mean(S²)/(mean(M²) + 1e-20) + 1e-12); 1 decimal |
| head_hz | a = int(first·48000/1000); seg = m[a : a + 1920]; only if len(seg) > 256: the frequency of the largest bin of \|rfft(seg·Hann(len), 8192)\|, rounded to 0 decimals; else the field is absent |
| ridge | frames of 512 samples every 240 samples (start i = 0, 240, ... while i < len - 512), times Hann(512); per frame: level = 10·log10(mean(frame²) + 1e-20) and the peak frequency of \|rfft(frame, 4096)\|. If there is at least 1 frame: keep the frames within 30 dB of the loudest, in time order; start_hz, end_hz = the first and last kept peak; max_hz, min_hz; sweep_oct = log2(max(end, 1)/max(start, 1)) rounded to 2 decimals. All Hz values are rounded to integers. Absent if there are no frames. |

### 12.12 TARGETS, pro_penalty, extra_for

Outside distance outside(v, lo, hi, s): 1.0 if v is null or absent; (lo - v)/s if v < lo; (v - hi)/s if v > hi;
else 0.

`TARGETS` (key, lo, hi, scale):

| cat | targets |
|---|---|
| pro_notify | (length_s, 0.2, 0.42, 0.1), (attack_ms, 0, 15, 10), (t20_ms, 20, 45, 20), (t40_ms, 150, 340, 80), (head_hz, 1700, 2400, 400), (low_db, -200, -38, 8), (corr, 0.6, 0.97, 0.2) |
| notify | (length_s, 0.5, 2.6, 0.4), (attack_ms, 0, 10, 10), (sub_db, -200, -20, 8) |

`pro_penalty(x, cat)`: m = measure(x). low_db = 10·log10(10^(shares[0]/10) + 10^(shares[1]/10) + 1e-12) (the energy
under 500 Hz). sub_db = shares[0]. sweep_oct = ridge.sweep_oct if ridge is present. Penalty = Σ over TARGETS[cat] of
min(2, outside(m[key], lo, hi, scale)). An unknown cat gives 0.

`extra_for(cat, weight=1.0)` returns a function (x, row) → -weight·pro_penalty(x, cat), for the critic's extra check.

### 12.13 Seed bank `notify.json`

23 entries. Context targets: "a short bright notification ding" (11) and "a struck bell notification chime" (12).
Delivered: 5 `pro_notify` (members ding_1822 to ding_2296, the round-5 family, seed 0) and 6 `notify` (members
notify_01 to notify_06: the round-3 bells with the round-4 edit, each with its own seed). The member names
notify_01 .. 06 follow the delivery order in the bank, not the table order of 12.6. Each entry holds a full parameter
set (51 keys).

## 13. shimmer

### 13.1 The sound

Shimmer, sparkle and chime textures for motion design. Facts from 2 trims of one pro film:

- A grain cloud, 1.95 s: about 50 short tonal grains per second (one per 20 ms) between 1 and 10 kHz, above a
  constant tone at about 500 Hz. The level climbs for 0.5 to 1 s along a straight line in dB from about -45 dB, then
  falls over about 0.8 s. Tilt +6.5 dB/octave. L/R correlation 0.08: the side signal is as strong as the mid. The
  grains change position in the stereo field.
- A melodic run, 1.95 s: notes like a glockenspiel (partials from 270 Hz to 10 kHz), alternating hard left and hard
  right, ended by a gate.

The `shimmer` refset holds real recordings (bar chimes, wind chimes, glockenspiel runs, dense metal textures). They
ring for 2 to 5 s. Design rules: pitches on a scale (pentatonic or major) so the texture does not clash with music; density
that changes over time; no harsh 8 to 12 kHz glare; width.

Target "a sparkling magical shimmer". Alternatives: "a xylophone run", "a piano trill", "rain on a window", "jingling
keys", "a synthesizer pad". Refset `pro_shimmer`.

Styles: `sparkle` (scale-tuned grains whose rate and level follow a rise, hold and fall curve, each grain at a random
position in the stereo field, with a tone bed, an air layer and a wide plate); `run` (a run of `notes` bar notes going up, down or in an arc, panned
note by note); `chimes` (wind chimes or a bell tree: tuned tubes struck at random, the strikes thinning out).

### 13.2 Interface

`SR`, `TARGET`, `ALTS`, `REFSET` = 'pro_shimmer', `SCALES`, `BAR`, `TUBE`, `SPACE`, `DEFAULTS`, `FAMILY`
(`pro_shimmer`, `shimmer`), `FINE`, `TARGETS`; `render(p, rng)`, `family_space(params, style=None)`,
`scale_freqs(lo, hi, scale, key)`, `envelope(p, n)`, `pro_penalty(x, cat)`, `extra_for(cat, weight=1.0)`. It uses
notify's `measure` and outside distance.

Constants: SCALES penta = 0, 2, 4, 7, 9; major = 0, 2, 4, 5, 7, 9, 11; lydian = 0, 2, 4, 6, 7, 9, 11 (semitones).
BAR = ratios (1, 2.756, 5.404), amplitudes (1, 0.32, 0.1). TUBE = ratios (1, 2.76, 5.40, 8.93), amplitudes
(0.5, 1, 0.6, 0.25).

The extra key `r2_path` is not in SPACE or DEFAULTS. If it is present and true, runs and chimes render on the
round-2 path (13.6). If it is absent, the current path is used.

### 13.3 SPACE and DEFAULTS

| key | range | scale | default | meaning |
|---|---|---|---|---|
| style | sparkle, run, chimes | choice | sparkle | |
| length | 1 to 3 | | 1.95 | s (members use up to 4.4) |
| start_db | -50 to -20 | | -42 | envelope level at t = 0 |
| swell | 0.2 to 1.1 | | 0.85 | time to the peak |
| rise_curve | 0.6 to 2.5 | | 1.2 | > 1: the swell speeds up into the peak |
| hold | 0 to 0.3 | | 0.1 | |
| fade | 0.4 to 1.4 | | 0.85 | peak to -45 dB time |
| gate_end | 0, 1 | choice | 0 | 1: a 30 ms gate ends the file |
| rate | 20 to 75 | | 55 | grains per second at the peak |
| dens_floor | 0.1 to 0.7 | | 0.5 | grain rate where the level is low, as a fraction of the peak rate |
| g_lo | 350 to 3500 | log | 450 | lowest note (Hz) |
| g_hi | 4500 to 11000 | log | 9000 | highest note |
| scale | penta, major, lydian | choice | penta | |
| key | 0 to 11 | int | 2 | scale root (0 = C) |
| g_tau | 0.0015 to 0.06 | log | 0.004 | grain decay |
| g_noise | 0 to 1 | | 0.6 | noise fraction of a grain (band noise centred on the grain's note) |
| cluster | 1 to 4 | int | 2 | notes per grain |
| g_bell | 0 to 1 | | 0.3 | 0 = sine grains, 1 = full bar partials |
| g_jit_db | 0 to 9 | | 6 | grain level spread |
| tilt | -3 to 6 | | 1.5 | dB/octave on grains and notes |
| spread | 0.3 to 1 | | 0.95 | pan spread |
| decor | 0 to 1.5 | | 1.2 | grain-bus decorrelation |
| octave | 0 to 0.5 | | 0.15 | octave-up echo of each grain |
| tone_hz | 250 to 800 | log | 500 | tone bed pitch, snapped to the scale root |
| tone_db | -40 to -6 | | -10 | tone bed level; ≤ -39 = off |
| tone_h | 0 to 0.5 | | 0.2 | tone harmonics |
| vib | 0 to 0.012 | | 0.004 | tone vibrato depth (fraction) |
| tone_w | 0.2 to 1.2 | | 1.0 | tone decorrelation |
| air_db | -50 to -16 | | -30 | air wash level; ≤ -49 = off (for the bed test) |
| air_hp | 4500 to 9000 | log | 6500 | air wash high-pass |
| verb | 0 to 0.7 | | 0.3 | |
| rt60 | 0.5 to 2.8 | log | 1.2 | |
| hp | 150 to 1200 | log | 250 | final high-pass |
| deess | 0 to 9 | | 3 | cut at 9 kHz (dB) |
| notes | 6 to 24 | int | 12 | run notes |
| run_t | 0.4 to 1.6 | | 1.2 | run duration |
| direction | up, down, arc | choice | up | |
| accel | 0.6 to 1.6 | | 1.0 | note times ∝ (k/(N-1))^accel |
| n_decay | 0.1 to 2.5 | log | 0.5 | note decay |
| hard | 0 to 1 | | 0.5 | mallet hardness |
| tubes | 4 to 9 | int | 6 | chimes: number of tubes |
| strikes | 4 to 30 | int | 14 | chimes: number of strikes |
| ring | 0, 1 | choice | 0 | (round 3) 1: runs and chimes ring out freely, only the end fades |
| beat | 0 to 3 | | 0 | doublet beat per partial (Hz) |
| detune | 0 to 12 | | 0 | random detune per strike (cents) |
| swing | 0 to 8 | | 0 | slow pitch wobble (cents, 0.7 to 2 Hz) |
| sizzle | -60 to -8 | | -60 | strike noise 5 to 18 kHz per note, dB re note; ≤ -59 = off |
| f_decay | 0 to 0.8 | | 0 | higher notes decay faster: tau ∝ (f/1000)^-f_decay |
| tail_fade | 0.2 to 1.2 | | 0.3 | ring mode end fade |

### 13.4 Families

Each member is DEFAULTS overlaid with the listed values.

`FAMILY['pro_shimmer']`:

| # | character | values |
|---|---|---|
| 1 | the film's sparkle | DEFAULTS unchanged |
| 2 | a quick bright glint | length 1.4, swell 0.35, fade 0.7, rate 65, g_lo 1200, tone_db -18, g_tau 0.003, cluster 3, hold 0.05, air_db -26 |
| 3 | the film's chime run | style run, notes 16, run_t 1.4, direction arc, gate_end 1, n_decay 0.09, hard 0.95, length 1.65, start_db -18, swell 0.9, hold 0.3, fade 2.0, tone_db -22, tone_hz 270, verb 0.2, rt60 0.6, g_lo 400, g_hi 6000, air_db -34 |
| 4 | a falling glitter run | style run, notes 18, run_t 0.75, direction down, accel 0.8, gate_end 1, n_decay 0.08, hard 0.8, length 1.15, start_db -4, swell 0.1, hold 0.2, fade 1.4, tone_db -40, scale major, key 7, g_lo 800, g_hi 7000, verb 0.25, rt60 0.9 |
| 5 | a warm wash | length 2.4, swell 1.0, fade 1.0, rate 34, g_hi 6000, g_tau 0.012, g_noise 0.15, cluster 1, tone_hz 400, tone_db -12, verb 0.5, rt60 2.0, g_bell 0.7, scale lydian, key 5 |

`FAMILY['shimmer']` (round 3, ring-outs):

| # | character | values |
|---|---|---|
| 1 | bar-chime sweep | style run, ring 1, notes 22, run_t 0.9, direction up, accel 0.9, n_decay 0.8, f_decay 0.3, length 4.2, tail_fade 0.7, tone_db -40, scale major, g_lo 1800, g_hi 9000, hard 0.65, g_bell 0.5, beat 1.2, detune 3, swing 1, sizzle -3, verb 0.45, rt60 3.2, spread 0.95, air_db -30, air_hp 3500, deess 2, decor 0.8 |
| 2 | bell-tree stroke | style run, ring 1, notes 14, run_t 0.8, direction down, accel 0.8, n_decay 0.6, f_decay 0.25, length 4.0, tail_fade 0.7, tone_db -40, g_lo 1500, g_hi 8000, g_bell 0.15, hard 0.55, beat 1.8, detune 5, swing 1.5, sizzle -5, verb 0.5, rt60 3.2, spread 0.9, air_db -32, air_hp 4500, decor 0.8 |
| 3 | wind chimes | style chimes, ring 1, tubes 6, strikes 12, n_decay 0.7, f_decay 0.2, length 4.2, tail_fade 0.9, tone_db -40, g_lo 900, g_hi 3200, hard 0.75, beat 0.9, detune 4, swing 4, sizzle -6, verb 0.42, rt60 3.0, spread 1.0, air_db -30, air_hp 4000, tilt 3, decor 1.0 |
| 4 | glock run | style run, ring 1, notes 8, run_t 0.7, direction up, accel 1.0, n_decay 0.55, f_decay 0.3, length 3.6, tail_fade 0.6, tone_db -40, scale major, key 0, g_lo 1000, g_hi 4200, hard 0.8, g_bell 0.6, beat 0.8, detune 2, swing 1, sizzle -9, verb 0.42, rt60 2.9, spread 0.9, air_db -32, air_hp 3000, decor 0.8 |
| 5 | a ringing sparkle | length 4.2, swell 0.6, fade 3.4, rate 40, g_tau 0.06, g_noise 0, cluster 1, g_bell 1.0, verb 0.5, rt60 3.0, tone_db -30, octave 0.15 |
| 6 | bell tree, slow up-and-down | style run, ring 1, notes 16, run_t 1.1, direction arc, accel 1.0, n_decay 0.75, f_decay 0.35, length 4.4, tail_fade 0.8, tone_db -40, scale penta, key 9, g_lo 1300, g_hi 7000, hard 0.5, g_bell 0.8, beat 1.4, detune 6, swing 2, sizzle -5, verb 0.48, rt60 3.2, spread 1.0, air_db -30, air_hp 4000, decor 0.8 |

Note: sizzle -3 (member 1) is above the SPACE range. `render` accepts it.

`FINE`:

| style | free keys |
|---|---|
| sparkle | start_db, rise_curve, dens_floor, g_tau, g_noise, g_bell, g_jit_db, tilt, spread, decor, octave, tone_db, tone_h, vib, tone_w, air_db, air_hp, verb, rt60, hp, deess |
| run | accel, hard, g_bell, g_jit_db, tilt, spread, octave, verb, rt60, hp, deess, air_db, beat, detune, sizzle |
| chimes | hard, g_jit_db, tilt, verb, rt60, hp, deess, air_db, beat, detune, swing, sizzle (spread stays pinned) |

`family_space(params, style=None)`: the same rule as notify (12.7), with the default style 'sparkle'.

### 13.5 Helpers

**scale_freqs(lo, hi, scale, key)**: for octave o = 0 .. 10 and each s in SCALES[scale]:
f = 16.3516·2^(o + (s + key)/12). Keep lo ≤ f ≤ hi. Return them sorted. If none, return [√(lo·hi)].

**envelope(p, n)** (linear level): up = clip(t/swell, 0, 1). dB = start_db·(1 - up^(1/max(rise_curve, 0.001))). For
t > swell + hold: dB = -45·max((t - swell - hold)/fade, 0) (a straight line in dB, unbounded below). Level =
10^(max(dB, -90)/20). The envelope does not use gate_end.

**Pan gains(pos)**: θ = (clip(pos, -1, 1) + 1)·π/4; gL = √2·cos θ, gR = √2·sin θ.

**Tilt gain(f)** = 10^(tilt·log2(f/2000)/20).

**note(f, n, tau, partials, hard, bell, beat = 0, swing = 0, sizzle = none)** returns a mono struck note.

1. If swing > 0.05: wob(t) = 2^((swing/1200)·sin(2π·U(0.7, 2.0)·t + U(0, 6.3))).
2. For each (q, a): skip q > 1 when bell ≤ 0; fq = f·q, skip fq > 18000. aa = a·(bell if q > 1 else 1)·
   q^(1.2·(hard - 0.5)). env = exp(-t/max(tau·q^-0.9, 0.002)). φ = U(0, 6.3). z = sin(2π·fq·t + φ), or with wob:
   sin(2π·fq·cumsum(wob)/SR + φ). If beat > 0.02: fb = fq + beat·U(0.5, 1.5)·√q; z = (z + 0.7·sin(2π·fb·t + φ))/1.7
   (the partner tone has no wobble). y += aa·env·z.
3. k = min(n, P1(0.0006)); raised-cosine rise over k samples.
4. If hard > 0.05: m = min(n, P1(0.003)); w = bw-hp(min(2f, 12000), 2) of m white samples, times exp(-t/0.0006);
   y[0:m] += 0.25·hard·(w unit peak).
5. If sizzle is given and > -59: m = min(n, P1(0.08)); w = bw-bp(max(5000, 1.5f), 18000, 2) of m white samples, times
   exp(-t/U(0.005, 0.016)); raised-cosine rise over the first k samples of w (k from step 3);
   y[0:m] += 10^(sizzle/20)·0.35·w/(RMS of the first P1(0.01) samples of w + 1e-12).

### 13.6 Layers

**sparkle(p, n)** returns (stereo grains, env).

1. env = envelope(p, n). notes = scale_freqs(g_lo, g_hi, scale, key).
2. Grain times: t = 0. While t < n/SR: t += an exponential draw with mean 1/rate; e = env[min(n - 1, int(t·SR))];
   d = dens_floor + (1 - dens_floor)·min(1, e); keep t if U(0, 1) < d and t < n/SR - 0.01.
3. gl = P1(min(0.35, 7·g_tau)).
4. Each kept time: i = int(t·SR), m = min(gl, n - i); skip if m < 16. y = 0 (m samples). For each of `cluster` notes:
   f = a uniformly chosen note; yc = (1 - g_noise)·note(f, m, g_tau, BAR, hard 0, bell g_bell). If g_noise > 0.01:
   w = bw-bp(f/1.25, min(1.25f, 20000), 2) of m white samples, times exp(-t/g_tau); yc += g_noise·(w unit peak).
   y += yc·tilt gain(f).
5. g = env[i]·10^(U(-1, 1)·g_jit_db/20)/√cluster.
6. If octave > 0.01 and 2f < 0.42·SR (f = the cluster's last note): y2 = note(2f, m, 0.7·g_tau, BAR, 0, bell 0) (a
   sine); y += octave·(y2 delayed by P1(0.006) samples, cut to m).
7. pos = (+1 or -1, equal chance)·U(0.35, 1)·spread. Add g·gL·y to L and g·gR·y to R at sample i.
8. Multiply the bus by 0.25. If decor > 0.01: mid = (L + R)/2, side = (L - R)/2; bus = decor(mid, decor, cut 400) +
   [side, -side].

**run(p, n)** returns (stereo notes × 0.3, envelope(p, n)).

1. notes as above, N = notes. Index track: up = linspace(0, len - 1, N); down = linspace(len - 1, 0, N); arc =
   (len - 1)·sin(linspace(0, π, N))^0.8; rounded to integers.
2. For k = 0 .. N - 1: tk = run_t·(k/max(N - 1, 1))^accel + U(-0.004, 0.004)·[k > 0]. i = int(max(tk, 0)·SR).
   m = min(P1(min(6·n_decay, cap)), n - i), cap = 4 on the round-2 path, else 6; skip if m < 16.
   f = notes[idx_k], times 2^(U(-1, 1)·detune/1200) except on the round-2 path. tau = n_decay·(f/1000)^-f_decay.
   y = note(f, m, tau, BAR, hard, 0.8·g_bell + 0.2, beat, swing, sizzle). g = tilt gain(f)·10^(U(-1, 1)·
   g_jit_db/40). pos = (+1 if k is odd, else -1)·spread·U(0.6, 1). Pan and add at i.

**chimes(p, n)** returns (stereo strikes × 0.3, ones(n)).

1. notes as above. T = tubes. pick = round(linspace(0, len - 1, min(T, len))). Each tube = (notes[j], pan
   U(-1, 1)·spread).
2. t = 0. For k = 0 .. strikes - 1: i = int(t·SR); stop if i ≥ n - 16. Pick a tube uniformly; detune its pitch by
   2^(U(-1, 1)·detune/1200) except on the round-2 path. m = min(P1(min(5·n_decay, cap)), n - i) (cap as in run).
   tau as in run. y = note(f, m, tau, TUBE, hard, 0.8, beat, swing, sizzle). g = 10^(U(-1, 1)·g_jit_db/20)·0.92^k.
   Pan and add at i. t += an exponential draw with mean 0.06 + 0.012·k (the strikes thin out).

**bed(p, n, benv)** returns stereo.

1. root = 16.3516·2^(key/12). f = root·2^round(log2(tone_hz/root)).
2. vib(t) = 1 + vib·sin(2π·5.1·t + U(0, 6.3)). ph = 2π·cumsum(f·vib)/SR. tone = sin ph + tone_h·(0.5·sin 2ph +
   0.3·sin 3ph). Scale to RMS 0.05·10^(tone_db/20).
3. If tone_db ≤ -39 and not the round-2 path: tone = 0 (off means off).
4. tone = decor(tone·benv, tone_w, cut 200).
5. Two independent air signals: bw-lp(14000, 2) of bw-hp(air_hp, 2) of n white samples, each scaled to RMS
   0.05·10^(air_db/20).
6. Return tone + [air1·benv, air2·benv].

### 13.7 render(p, rng)

1. p = DEFAULTS overlaid with p. n = P1(length). (grains, env) = sparkle, run or chimes by style. y = grains.
2. Bed, if tone_db > -39 or air_db > -49:
   - h = P1(0.2), k = max(1, n // h). ref = √(max over k blocks of the block mean of the per-sample channel-mean of
     grains²) + 1e-9 (the RMS of the loudest 200 ms).
   - benv: sparkle → env. ring → a 20 ms block RMS e of the grains (per-sample channel-mean power), kk = max(1, n //
     P1(0.02)) blocks, linearly interpolated from the block centres to every sample (held flat beyond the first and
     last centre), divided by (max e + 1e-12). Otherwise → envelope(p, n).
   - y = y + bed(p, n, benv)·ref/0.05.
3. If ring and style ≠ sparkle and decor > 0.01: the same mid/side decorrelation as sparkle step 8, with cut 500.
4. y = bw-hp(hp, 4). If deess > 0.2: y = peq(y, 9000, -deess, 0.8).
5. If verb > 0.01: y = verb(y, rt60, size 0.7, predelay 0.015, damp 0.45, wet verb, stereo, width 1).
6. Cut y to n. If style ≠ sparkle and not ring: y = y·envelope(p, n) (the envelope shapes the whole run or chime mix).
7. End fade: k = P1(0.03) if gate_end; else P1(min(tail_fade, 0.4·length)) if ring; else P1(min(0.3,
   0.15·length)). Apply a cos² fade over the last k samples.
8. If ring: a raised-cosine rise over the first P1(0.0005) samples.

### 13.8 TARGETS, pro_penalty, extra_for

| cat | targets (key, lo, hi, scale) |
|---|---|
| pro_shimmer | (length_s, 1.4, 2.6, 0.4), (corr, -0.2, 0.45, 0.2), (tilt, 2, 7, 3), (low_db, -200, -30, 6) |
| shimmer | (length_s, 1.5, 5.0, 0.5), (low_db, -200, -25, 6) |

`pro_penalty(x, cat)`: m = measure(x); low_db = 10·log10(10^(shares[0]/10) + 1e-12) (under 150 Hz only, unlike
notify); Σ min(2, outside(...)). `extra_for(cat, weight)` as in notify.

### 13.9 Seed bank `shimmer.json`

23 entries. Contexts "a sparkling magical shimmer" (11) and "bar chimes and wind chimes ringing out" (12). Delivered:
6 `shimmer` (shimmer_01 to shimmer_06, ring-outs, 50 keys) and 5 `pro_shimmer` (pro_shimmer_01 to pro_shimmer_05,
43 keys, from round 2 before the round-3 keys existed). pro_shimmer_04 and pro_shimmer_05 are runs. They re-render as
delivered only when `r2_path` = 1 is added. The bank entries do not carry that key.

## 14. hover

### 14.1 The sound

A short, quiet, high tick for a cursor hover or a scroll step in a launch video. No energy below 500 Hz. Pro film
measurements: 35 to 80 ms long, attack 0 to 5 ms, t20 6 to 25 ms, centroid 2.8 to 9.5 kHz, tilt +2.4 to +6.2 dB/octave, band levels re
total 1 kHz -23, 2 kHz -18, 4 kHz -13, 8 kHz -8 dB. Two kinds: tonal pings (one partial at 6 to 15 kHz, about 10 ms)
and filtered noise ticks (2 to 12 kHz, 25 to 40 ms, with the spectral peak moving down during the decay). Real ticks come in 2
registers: about 4.5 kHz (t40 65 to 90 ms) and 10 to 12 kHz (t40 25 to 40 ms). In a mix a hover sits 12 to 20 dB
under the click. It is heard many times, so it must stay plain.

Layers: a ping (one decaying sine, plus an optional quiet second partial above it); a tick (band-pass filtered noise;
the band centre moves down by `fall` octaves during the decay); an optional repeat tick 4 to 16 ms later (a scroll
step); a round-3 contact impulse with small grains after it. Then a steep high-pass, a gentle low-pass, a very short
bright room and an end gate. `width` spreads only the tick in stereo; the ping is mono.

Target "a tiny high soft tick when the cursor hovers over a button". Alternatives: "a clock ticking", "a drop of
water", "a mouse click", "a cricket chirp". Refset `hover_tick`. `TARGETS` (target text per category):
hover_tick → "a tiny high soft tick when the cursor hovers over a button"; pro_hover_tick → "a tiny airy digital tick
for an interface animation".

### 14.2 Interface

`TARGET`, `ALTS`, `TARGETS`, `REFSET`, `SPACE`, `DEFAULTS`, `FAMILIES` ({cat: {family: spec}}), `render(p, rng)`,
`family_space(cat, fam)`, `family_params(cat, fam)`. It uses 5 helpers of the click recipe (unit recipes-impacts):
partials, burst, gate, narrow, centre. Their behaviour is restated in 14.5 so this unit can be built alone.

`render` does not merge DEFAULTS. The caller passes a full parameter set. `family_params` gives one.

### 14.3 SPACE and DEFAULTS

| key | range | scale | default | meaning |
|---|---|---|---|---|
| ping | 0 to 1 | | 0.5 | ping share; the tick gets 1 - ping |
| ping_f | 3000 to 15000 | log | 8000 | |
| ping_ms | 1 to 8 | log | 6 | ping decay (ms); longer pings draw a ringing line |
| ping_up | 0 to 0.5 | | 0.15 | upper partner level (off if ≤ 0.01) |
| ping_ratio | 1.5 to 2.4 | | 1.9 | partner ratio |
| tick_f | 2500 to 11000 | log | 6000 | noise band centre at the onset |
| tick_q | 0.8 to 4 | log | 1.6 | |
| tick_ms | 1.5 to 20 | log | 6 | tick decay (ms) |
| snap | 1 to 3 | | 1.5 | decay exponent: higher = faster first fall |
| fall | 0 to 1.5 | | 0.6 | octaves the band falls |
| detent | 0 to 0.8 | | 0 | second tick level (off if ≤ 0.02) |
| detent_dt | 0.004 to 0.016 | | 0.008 | second tick delay (s) |
| attack_ms | 0.05 to 3 | log | 0.3 | |
| hp | 500 to 2500 | log | 900 | |
| lp | 8000 to 20000 | log | 16000 | |
| sat | 0 to 6 | | 1 | tanh drive (dB); off if ≤ 0.05 |
| length | 0.02 to 0.11 | | 0.045 | s after the 2 ms pre-roll |
| release_ms | 3 to 25 | log | 10 | end gate release |
| width | 0 to 1 | | 0.3 | tick decorrelation and contact L/R independence |
| verb | 0 to 0.35 | | 0 | tiny room; off if ≤ 0.003 |
| rt | 0.04 to 0.3 | log | 0.1 | room RT60 |
| contact | 0 to 40 | | 0 | (round 3) contact impulse level re the tick or ping peak; off if ≤ 0.01 |
| contact_ms | 0.08 to 1 | log | 0.2 | contact decay (ms) |
| grains | 0 to 0.5 | | 0 | (round 3) micro-grain level re the contact; off if ≤ 0.005 |

### 14.4 FAMILIES

A family spec maps keys to a range (lo, hi) or (lo, hi, 'log'), or to a fixed value.

`FAMILIES['pro_hover_tick']`:

| family | spec |
|---|---|
| ping | detent 0, ping (0.6, 0.9), ping_f (8000, 14000, log), ping_ms (1.5, 3, log), snap (2, 3), contact (3, 7), grains 0, contact_ms (0.25, 0.6, log), width (0.6, 1), verb (0, 0.06), lp (13000, 17000, log), length (0.025, 0.045) |
| air | detent 0, ping (0, 0.15), tick_f (7000, 11000, log), fall (0.4, 1.2), tick_ms (2, 6, log), snap (2, 3), contact (12, 30), grains (0.05, 0.35), contact_ms (0.3, 0.8, log), width (0.8, 1), verb (0, 0.06), lp (13000, 17000, log), length (0.03, 0.055) |
| glass | detent 0, ping (0.3, 0.6), ping_f (7000, 11000, log), ping_ms (1.5, 3, log), snap (2, 3), tick_f (7000, 11000, log), ping_up (0.15, 0.4), contact (6, 12), grains (0, 0.15), hp (1000, 2200, log), contact_ms (0.25, 0.6, log), width (0.6, 1), verb (0, 0.06), lp (13000, 17000, log), length (0.03, 0.05) |
| soft | detent 0, ping (0, 0.3), ping_ms (1.5, 3, log), tick_f (5000, 8000, log), tick_ms (2, 6, log), snap (1.5, 2.5), attack_ms (0.3, 1.2, log), contact (1.5, 4), grains 0, contact_ms (0.4, 1, log), width (0.6, 1), hp (800, 1500, log), verb (0, 0.08), lp (12000, 16000, log), length (0.03, 0.06) |
| detent | ping (0, 0.35), ping_ms (1, 3, log), tick_f (7000, 11000, log), snap (2, 3), tick_ms (1.5, 4, log), detent (0.35, 0.8), detent_dt (0.006, 0.012), contact (12, 30), grains (0.05, 0.35), contact_ms (0.2, 0.5, log), width (0.7, 1), verb (0, 0.06), lp (13000, 17000, log), length (0.025, 0.045) |

`FAMILIES['hover_tick']`:

| family | spec |
|---|---|
| high | detent 0, ping (0, 0.3), ping_ms (1, 3, log), tick_f (8000, 11000, log), tick_ms (1.5, 5, log), snap (1.5, 3), hp (1500, 2500, log), verb (0.05, 0.3), rt (0.05, 0.25, log), length (0.04, 0.07) |
| mid | detent 0, ping (0, 0.3), ping_ms (1, 4, log), tick_f (3000, 5000, log), tick_ms (4, 14, log), snap (1, 2), hp (700, 1400, log), verb (0.05, 0.3), rt (0.05, 0.25, log), length (0.065, 0.11) |
| ping | detent 0, ping (0.75, 1), ping_f (5000, 9000, log), ping_ms (2, 7, log), snap (1.5, 3), verb (0.05, 0.3), rt (0.05, 0.25, log), length (0.045, 0.085) |
| air | detent 0, ping (0, 0.12), tick_f (5000, 9000, log), fall (0.6, 1.4), tick_ms (3, 12, log), verb (0.05, 0.3), rt (0.05, 0.25, log), length (0.05, 0.095) |
| detent | ping (0, 0.3), ping_ms (1, 3, log), tick_f (4500, 8000, log), snap (1.5, 3), detent (0.35, 0.8), detent_dt (0.006, 0.014), verb (0.05, 0.3), rt (0.05, 0.25, log), length (0.05, 0.095) |

The design intent: pro ticks are dry, fast and high (down 40 dB in 6 to 14 ms, bright onset). In round 3 every pro
family got a decorrelated contact impulse over its ping or tick, because the reference peaks are such an impulse
(t20 1 to 3 ms, L/R correlation about 0). Each family has its own contact level range: ping 3 to 7 ×, soft 1.5 to 4 ×,
glass 6 to 12 ×, air and detent 12 to 30 ×.

`family_space(cat, fam)` = narrow(SPACE, FAMILIES[cat][fam]). `family_params(cat, fam)` = centre(DEFAULTS,
FAMILIES[cat][fam]) (14.5).

### 14.5 Helpers from the click recipe

**narrow(space, fam)**: copy space. For each key of fam: a range (lo, hi[, kind]) becomes (lo, hi, kind) where kind =
the given tag, else 'int' if the space entry is an int range, else linear (written as the 2-tuple (lo, hi)). A fixed
value v becomes [v].

**centre(defaults, fam)**: copy defaults. A range becomes its centre: √(lo·hi) for 'log', else (lo + hi)/2, rounded
to an integer when the default value is an integer. A fixed value is copied.

**partials(f0, ratios, amps, tau, seconds, att_s)**: decays d = tau·ratio^-0.6. y = the forge-core modal sum over
P1(seconds) samples: for each mode with f < 0.95·Nyquist and a ≠ 0: f' = f·(1 + 0.004·N(0, 1)), φ = U(0, 2π),
a·exp(-t/max(d, 1e-5))·sin(2π·cumsum(f')/SR + φ) (no glide). Attack: a = max(2, P1(att_s)) samples times
sin²(π/2·i/(a - 1)). Unit peak.

**burst(seconds, f_lo, f_hi, att_s, order)**: n = P1(max(6·seconds, 0.004)). e = exp(-t/max(seconds, 5e-5))². First
max(2, P1(att_s)) samples of e times sin². lo = max(40, f_lo), hi = min(21500, f_hi). y = bw-bp(lo, max(1.05·lo, hi),
order) of n white samples, times e. Unit peak.

**gate(x, length, release, start)**: end = min(len, P1(start + length)). rl = max(8, min(end - 1, P1(release))). Gain
1 before end - rl, then 0.5 + 0.5·cos(π·i/(rl - 1)) over rl samples. Output = x·gain cut to end samples.

### 14.6 render(p, rng)

1. lvl = 10^(0.03·N(0, 1)) (about ±0.3 dB). fs = exp(0.01·N(0, 1)) (about ±1 % pitch). n = P1(0.14). pre = 0.002.
2. (pg, tk, ct) = one(p, fs) (below). Place pg·lvl, tk·lvl and ct·lvl into zero buffers of n samples at time pre.
   Placing at time τ: start index P1(τ) if τ > 0, else 0; add as much as fits.
3. If detent > 0.02: (pg2, tk2, ct2) = one(p, 1.06·fs). Place each times detent·lvl at pre + detent_dt.
4. st = [pg, pg] + decor(tk, width, cut 1500) (cut to n) + ct.
5. st = bw-hp(hp, 4), then bw-lp(lp, 2).
6. If contact > 0.01: scale st to unit peak (a large contact would otherwise be clipped flat by the saturator).
7. If sat > 0.05: st = sat(st, sat dB, mode tanh, level peak).
8. If verb > 0.003: st = verb(st, rt60 rt, size 0.1, predelay 0.002, damp 0.5, wet verb, early_db -10, width 1,
   stereo, lowcut 1500).
9. last = detent_dt if detent > 0.02, else 0. ring = 5.76e-3·max(ping_ms if ping > 0.05 else 0, tick_ms/snap if
   ping < 0.95 else 0) s. (5.76 = ln 10^2.5: the time to fall about 50 dB.)
10. st = gate(st, max(length, last + ring + release_ms/1000), release_ms/1000, start pre). The file never ends before
    the hit has decayed about 50 dB.
11. Forge-core limiter, ceiling -1 dBFS, true peak (default lookahead and release).
12. Forge-core fade: in 0.5 ms, out 2 ms (P9).

**one(p, f_scale)** returns (ping·pg, tick, contact).

1. Ping: k = 2 if ping_up > 0.01, else 1. pg = partials(ping_f·f_scale, [1, ping_ratio][:k], [1, ping_up][:k],
   ping_ms/1000, 0.1 s, att max(0.0001, attack_ms/1000)).
2. Tick: tau = tick_ms/1000. m = P1(max(0.012, 6·tau)). fc(t) = tick_f·f_scale·2^(-fall·(1 - exp(-t/(1.5·tau)))). y =
   time-varying SVF band-pass (mode 'band', P2) of m white samples, cutoff min(fc, 20000), Q tick_q.
   e = exp(-t/tau)^snap, its first max(2, P1(attack_ms/1000)) samples times sin². tk = y·e, unit peak.
3. Zero-pad pg and tk to the same length M. tk = (1 - ping)·tk.
4. Contact (only if contact > 0.01; else ct = zeros (M, 2)):
   - A = contact·max(max|tk|, max|ping·pg|). θ = 0.5π·min(1, width).
   - Events: the main contact (time 0, amplitude 1, decay contact_ms). If grains > 0.005, add 4 grains: times
     U(0.002, 0.025) s, levels grains·10^(U(-20, 0)/20)·(±1 with equal chance), decay 0.1 ms.
   - Each event: c1, c2 = 2 independent burst(decay/1000, 2000, 15000, att 0.00005, order 4) (a 2 to 15 kHz band:
     a 20 kHz edge put too much energy above 20 kHz). L = c1, R = cos θ·c1 + sin θ·c2 (width 1 gives independent
     channels). k = P1(time) (so the main contact starts at sample 1). If k < M, add A·amplitude·(L, R) from k, cut
     at M.
   - No all-pass decorrelator here: it smeared the short spike over several ms.

### 14.7 Seed bank `hover.json`

17 entries. Contexts: "a tiny airy digital tick for an interface animation" (10) and "a tiny high soft tick when
the cursor hovers over a button" (7). Delivered: 5 `pro_hover_tick` (ping, air, glass, soft, detent; 24 keys each,
with contact) and 5 `hover_tick` (high, mid, ping, air, detent; 21 keys, no contact keys). Entries are full parameter
sets for `render` (keys missing from an entry, such as contact, read as off: `render` treats an absent contact or
grains as 0). Some delivered values lie outside the current family ranges (for example contact 23.3 for the pro ping
family, whose range is 3 to 7). For delivered members the bank values are the reference.
