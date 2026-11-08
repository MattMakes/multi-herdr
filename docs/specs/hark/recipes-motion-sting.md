# recipes-motion, part 5: the logo sting (sting)

This part belongs to the index `recipes-motion.md`. Read the index first: it defines the recipe contract, the shared
primitives (P1 to P26) and the seed-bank format. The notation is the notation of part 4 (`recipes-motion-tonal.md`,
top): t, white, U(a, b), bw-lp / bw-hp / bw-bp, raised-cosine rise, unit peak, decor. In addition:

- `conv(x, h)` = the full linear convolution (length len(x) + len(h) - 1), computed by FFT.
- `svf(x, fc, q, mode)` = the time-varying state-variable filter (P2). The cutoff may change per sample. Modes: low,
  band (peak gain q), bandn (peak gain 1).
- `fade(x, a, b)` = the forge-core fade (P9): a raised-cosine rise over P1(a) samples and a raised-cosine fall over
  P1(b) samples. A side of only 1 sample is skipped, so b = 0 means no fade-out.
- `trim(x, dB, keep)` = the forge-core trim_tail (P14): cut after the last sample that is within dB of the peak, plus
  `keep` seconds.
- `hall(rt60, damp, size, predelay[, seconds])` = the computed hall impulse response of the impact recipe (unit
  recipes-impacts), drawn from the render generator: diffuse early reflections, then a noise tail with a
  frequency-dependent RT60.
- `bell(f, s, partials, decay, slope, bright, beat, strike)` = the forge-core additive bell (BELLS presets bar, glass,
  tubular, or an explicit (ratios, amps) pair); its output peaks at -1 dBFS. Arguments not named take the forge-core
  defaults (slope 0.8, beat 1.0, stretch 0).
- `fm(fc, ratio, index, s, decay, index_decay, attack)` = the forge-core 2-operator FM tone (no random draws).
- `air(s, center, flutter, rate)` = the forge-core moving-air noise.
- `granular(src, s, grain, density, pos, pos_jitter, pitch, pitch_jitter, spread)` = the forge-core granular cloud;
  it returns P1(s) stereo samples. Arguments not named take the forge-core defaults (time_jitter 0.5, reverse 0).

## 15. sting

### 15.1 The sound

A logo sting or reveal. Every sample is computed. The recipe has 4 idioms (`mode`):

1. **chirp** (rounds 2 to 4): a single motion. A sine with some harmonics sweeps down from about 1.5 kHz to 150 Hz
   in 85 ms (about 38 octaves per second). At the bottom it turns into a low hum: fundamental 64 to 80 Hz, rich
   harmonics shaped by a formant at about 400 to 500 Hz, the pitch dropping about 0.6 octave. The hum lasts about
   0.8 s. Its decay is concave: t20 about 60 ms, t40 about 260 ms (a quick drop to a low level, then a slow ring).
   L = R. Most energy is at 250 to 500 Hz; tilt about -7 dB/octave.
2. **reveal** (round 2): up to 4 layers. An optional build (the bloom's hall response, reversed in time); a hit when
   the logo appears (the impact recipe, low in the mix); a sub (the subdrop recipe); and a tonal bloom. The bloom is a
   chord (root, fifth, octave, and maybe a third or a ninth) of modal or 2-operator FM bell tones. A low-pass on the
   bloom opens from about 1 kHz over `open_ms`. A granular layer made from the bloom's high partials adds width, and a
   computed hall adds space. The bloom gives a bright tail of 1 to 4 s.
3. **launch** (round 3 and later): a single composed motion in 4 steps. (a) A lead-in: rising air noise, a rising
   sine sweep, the bloom's reversed hall, or nothing. (b) A bright hit when the logo appears: a noise tick above 3 to
   9 kHz and an optional light thump. (c) A short bloom: 1 note or 2 notes (never 3 or more) with a concave decay,
   plus a granular layer. (d) A short room (its input is high-passed at 500 Hz), then a smooth fade-out. t40 about 0.8 to
   1.0 s. Round 4 adds a low strike below the hit.
4. **gesture** (round 5): 5 launch-reveal motions that differ in kind. Their tones are sines plus at most a 2nd and a
   3rd partial; the other parts are band-pass filtered noise and a computed hall. The dry part is mono (L = R). The tail is wide: 2
   independent hall responses, fed only with content above 250 Hz, so the low frequencies stay mono.

Target "a bright logo sting: a chime chord ringing out". Alternatives: "a doorbell", "a church bell", "a notification
sound", "a gong". Refset `logo_sting`. The keys that give each member its character: the pitch keys (root, chord, the chirp
frequencies), material, the length keys, the brightness keys (bright, open_ms), the weight keys (hit, sub) and the
texture keys (shimmer, swell).

### 15.2 Interface

Constants: `SR`, `TARGET`, `ALTS`, `REFSET` = 'logo_sting', `CHORDS`, `MATERIALS`, `TRIANGLE`, `SPACE`, `FAMILIES`,
`FAMILIES_R2`, `FAMILIES_R3`, `FAMILIES_R4`, `DEFAULTS`, `TARGETS`, `STUDY_STING`, `DYADS`, `LEADS`, `GESTURES`,
`G_MATS`, `CHORDS_G`.

Functions: `render(p, rng)`, `space_for(cat)`, `recipe_for(cat, preset=0)`, `fx_granular(src, seconds, rng)`.
Re-exported from the impact recipe (unit recipes-impacts, not specified here): `hall_ir`, `measure`, `study_extra`,
`study_penalty`.

`FAMILIES` maps a category to a list of (name, params) pairs (the bank palette reads it, index "Interface").
`TARGETS` maps a category to its target text: logo_sting → "a short bright logo reveal sting: a whoosh into a hit
with a shimmering chime"; pro_logo_sting → "a designed logo sting: a falling chirp landing on a low hum".

Tables:

- CHORDS (semitones): fifth 0, 7, 12; major 0, 4, 7, 12; sus2 0, 2, 7, 12; add9 0, 4, 7, 14; octave 0, 12, 24;
  minor9 0, 3, 7, 14.
- MATERIALS: bar, glass, tubular, fm, triangle.
- TRIANGLE (a struck triangle or small bar cluster): ratios 1, 2.76, 3.9, 5.4, 6.7, 8.93, 10.3, 13.34; amplitudes
  0.6, 1.0, 0.5, 0.8, 0.45, 0.5, 0.3, 0.25.
- DYADS: one 0; fifth 0, 7; octave 0, 12; fourth 0, 5; ninth 0, 14; third 0, 4.
- LEADS: whoosh, chirp, swell, none.
- GESTURES: chirp_hum, zap_bloom, sweep_hit, taps_tone, swell_sting.
- CHORDS_G = DYADS + triad 0, 7, 16.
- G_MATS ((ratio, share of g_warm, decay s) per extra partial): sine = none (harmonics 2 and 3, 15.11); marimba
  (3.93, 0.5, 0.03), (9.2, 0.15, 0.012); glass (2.32, 0.45, 0.12), (4.25, 0.25, 0.06), (6.63, 0.1, 0.03); bar (2.756,
  0.4, 0.08), (5.404, 0.15, 0.03).

### 15.3 SPACE

| key | range | scale | idiom | meaning |
|---|---|---|---|---|
| mode | reveal, chirp, launch, gesture | choice | all | |
| root | 300 to 2200 | log | reveal, launch, chirp bell | root note (Hz) |
| chord | CHORDS names | choice | reveal | |
| material | MATERIALS | choice | reveal, launch | |
| bloom_decay | 0.6 to 4 | log | reveal | |
| bright | 0 to 1 | | reveal, launch | |
| open_ms | 10 to 400 | log | reveal | low-pass opening time |
| strike | 0 to 1 | | reveal | bell strike transient |
| shimmer | 0 to 0.6 | | all but gesture | granular shimmer (chirp: bell partial) level |
| hit | 0 to 0.8 | | reveal | |
| sub | 0 to 0.8 | | reveal, chirp | |
| swell | 0 to 1.2 | | reveal | build length (s) |
| swell_level | 0.05 to 0.6 | log | reveal | |
| verb | 0 to 0.8 | | all but gesture | |
| verb_rt | 0.8 to 5 | log | all but gesture | |
| width | 0 to 1 | | reveal, launch | |
| low_cut | 25 to 800 | log | reveal, launch | final high-pass |
| chirp_f0 | 700 to 3500 | log | chirp | start pitch |
| chirp_f1 | 90 to 320 | log | chirp | end pitch |
| chirp_ms | 40 to 170 | log | chirp | |
| chirp_harm | 0 to 1 | | chirp | |
| hum_f | 52 to 110 | log | chirp | hum fundamental |
| hum_sink | 0 to 1 | | chirp | octaves the hum sinks |
| hum_formant | 200 to 900 | log | chirp | |
| hum_buzz | 0 to 1 | | chirp | harmonic richness |
| hum_level | 0.2 to 1.5 | log | chirp | |
| hum_fast | 0.02 to 0.15 | log | chirp | fast drop time constant |
| hum_slow | 0.15 to 0.9 | log | chirp | slow ring time constant |
| hum_mix | 0.03 to 0.5 | log | chirp | slow ring level |
| land | 0 to 1 | | chirp | (round 3) landing blob level |
| land_f | 400 to 1300 | log | chirp | landing blob pitch |
| thud | 0 to 1 | | chirp | (round 3) low thud onto 2 × hum_f |
| sub_db | 30 to 130 | | chirp | decay rate (dB/s) of the chirp's sub; 45 = round 2 |
| bell_decay | 0.08 to 0.6 | log | chirp | (round 3) bell partial decay; absent = 0.35 |
| length | 0.5 to 1.8 | | chirp | |
| lead | whoosh, chirp, swell, none | choice | launch | |
| lead_s | 0.1 to 0.7 | | launch | lead-in length (s) |
| lead_level | 0 to 0.8 | | launch | lead peak re the body peak |
| lead_f0 | 200 to 2000 | log | launch | lead start band / pitch |
| lead_f1 | 1500 to 12000 | log | launch | lead end band / pitch |
| tick | 0 to 1.5 | | launch | strike level |
| tick_f | 2500 to 9000 | log | launch | strike high-pass |
| tick_ms | 2 to 15 | log | launch | |
| thump | 0 to 0.8 | | launch | |
| thump_f | 70 to 260 | log | launch | |
| notes | one, fifth, octave, fourth, ninth, third | choice | launch | |
| ring | 0.3 to 1.2 | | launch | bloom level re the tick |
| strike_tau | 0.04 to 0.3 | log | launch | fast part of the concave decay |
| ring_mix | 0.05 to 0.5 | | launch | level of the slow part |
| ring_tau | 0.15 to 0.6 | log | launch | slow part time constant |
| bloom_att | 0 to 0.15 | | launch | bloom swell-in with an opening low-pass |
| shimmer_tau | 0.15 to 0.8 | log | launch | |
| bloom_delay | 0 to 0.03 | | launch | bloom fades in this long after the tick |
| low | 0 to 1 | | launch | (round 4) low strike level |
| low_f | 38 to 95 | log | launch | low strike end pitch |
| low_tau | 0.06 to 0.4 | log | launch | |
| zap | 0 to 1 | | chirp | (round 4) noise riding the chirp |
| zap_k | 3 to 10 | | chirp | zap band centre / chirp pitch |
| ctick | 0 to 1 | | chirp | (round 4) landing tick above 5 kHz |
| double | 0 to 1 | | chirp | (round 4) level of an extra chirp before the main one (shorter and higher) |
| swell_trim | 0 to 0.08 | | launch | (round 4) s of the hall build-up left out of the swell lead |
| shimmer_smooth | 0 to 1 | | launch | (round 5) denser, longer grains |
| double_gap | 0 to 0.12 | | chirp | gap between pre-chirp and chirp (s); absent = 0.05 |

Gesture keys (round 5):

| key | range | scale | meaning |
|---|---|---|---|
| gesture | GESTURES | choice | |
| g_f0 | 150 to 12000 | log | lead start pitch |
| g_f1 | 60 to 4000 | log | lead end pitch |
| g_ms | 40 to 600 | log | lead length (ms) |
| g_lead | 0 to 1 | | lead level |
| g_root | 50 to 1800 | log | tone root (hum, bloom, tone or chord) |
| g_notes | one, fifth, octave, fourth, ninth, third, triad | choice | |
| g_warm | 0 to 0.6 | | extra-partial share (0 = a sine) |
| g_att | 0 to 0.12 | | tone swell-in with an opening low-pass |
| g_tau1 | 0.02 to 0.3 | log | fast decay |
| g_mix | 0.03 to 0.5 | log | slow part level |
| g_tau2 | 0.1 to 0.9 | log | slow decay |
| g_sink | 0 to 1 | | octaves the tone sinks |
| g_land | 0 to 1 | | landing blob level |
| g_land_f | 300 to 1500 | log | |
| g_air | 0 to 1 | | noise riding the lead |
| g_hit | 0 to 1 | | low strike level |
| g_low_f | 38 to 95 | log | |
| g_low_tau | 0.05 to 0.4 | log | |
| g_snap | 0 to 1 | | noise snap level |
| g_snap_f | 1500 to 9000 | log | |
| g_snap_ms | 2 to 20 | log | |
| g_gap | 0.04 to 0.14 | | taps_tone tap spacing |
| g_tap_f | 500 to 3000 | log | tap pitch |
| g_wet | 0 to 0.8 | | tail level |
| g_rt | 0.4 to 2.5 | log | tail RT60 |
| g_length | 0.4 to 1.6 | | length after the payoff (s) |
| g_tone | 0.2 to 1.5 | | tone level |
| g_verb | hall, fdn | choice | tail: hall impulse response, or the forge-core reverb |
| g_mat | sine, marimba, glass, bar | choice | tone partials |
| g_bend | 0 to 2 | | semitones the tone starts sharp (settles in about 25 ms) |
| g_chorus | 0 to 1 | | ±8 cent copies of each partial |

### 15.4 Base parameter sets

`render` overlays the given parameters on the base of their mode (15.6).

| base | values |
|---|---|
| reveal | mode reveal, root 880, chord add9, material bar, bloom_decay 2.2, bright 0.6, open_ms 120, strike 0.5, shimmer 0.25, hit 0.3, sub 0.25, swell 0, swell_level 0.2, verb 0.35, verb_rt 2.6, width 0.6, low_cut 25 |
| chirp r2 | mode chirp, chirp_f0 1500, chirp_f1 150, chirp_ms 85, chirp_harm 0.7, hum_f 76, hum_sink 0.6, hum_formant 500, hum_buzz 0.9, hum_level 0.8, hum_fast 0.045, hum_slow 0.2, hum_mix 0.06, length 0.85, sub 0, shimmer 0, verb 0.05, verb_rt 0.8, width 0, root 880, material bar, bright 0.5, land 0, land_f 800, thud 0, sub_db 45 |
| chirp r3 | chirp r2 + land 0.5, land_f 1400, thud 0.5, hum_level 0.95, hum_fast 0.05, chirp_harm 1.0, sub 0.25, sub_db 100 |
| chirp r4 | chirp r3 + zap 0, zap_k 6, ctick 0, double 0, double_gap 0.05 |
| launch | mode launch, lead whoosh, lead_s 0.45, lead_level 0.45, lead_f0 600, lead_f1 9000, tick 1.0, tick_f 5000, tick_ms 6, thump 0.08, thump_f 150, material glass, root 1318.5, notes fifth, bright 0.7, ring 0.9, strike_tau 0.12, ring_mix 0.25, ring_tau 0.3, bloom_att 0, shimmer 0.4, shimmer_tau 0.35, verb 0.2, verb_rt 1.0, width 0.6, low_cut 140, bloom_delay 0.012 |
| gesture | mode gesture, gesture chirp_hum, g_f0 1600, g_f1 160, g_ms 85, g_lead 1, g_root 76, g_notes one, g_warm 0.35, g_att 0, g_tau1 0.06, g_mix 0.12, g_tau2 0.3, g_sink 0.5, g_land 0.5, g_land_f 800, g_air 0.25, g_hit 0, g_low_f 55, g_low_tau 0.15, g_snap 0.2, g_snap_f 5000, g_snap_ms 5, g_gap 0.075, g_tap_f 1400, g_wet 0.3, g_rt 1.0, g_length 0.8, g_tone 1.0, g_mat sine, g_bend 0, g_chorus 0, g_verb hall |

`render` uses the chirp r2 base for mode chirp. The r3 and r4 chirp bases exist only to build members.

### 15.5 Families

**FAMILIES['logo_sting'] (round 5, delivered).** Built in 3 layers: round 3 (launch base + table A), round 4
(+ table B), round 5 (+ table C).

Table A (round 3, `FAMILIES_R3['logo_sting']`):

| name | values over the launch base |
|---|---|
| whoosh_glass | (none) |
| zip_ping | lead chirp, lead_s 0.16, lead_level 0.4, lead_f0 420, lead_f1 2600, tick 0.9, tick_f 6000, tick_ms 4, thump 0, material bar, root 1760, notes octave, strike_tau 0.1, ring_mix 0.2, bright 0.45, ring_tau 0.28, shimmer 0.15, verb 0.15, width 0.3, low_cut 300 |
| reverse_bloom | lead swell, lead_s 0.5, lead_level 0.35, tick 0.8, tick_f 4500, tick_ms 8, thump 0.15, thump_f 110, material bar, root 1480, notes ninth, strike_tau 0.15, ring_mix 0.3, ring_tau 0.32, shimmer 0.3, verb 0.3, verb_rt 1.2, width 0.7, low_cut 90 |
| spark | lead none, lead_level 0.3, tick 1.1, tick_f 7000, tick_ms 3, thump 0, material triangle, root 1318.5, notes one, strike_tau 0.09, bright 0.35, ring_mix 0.3, ring_tau 0.28, shimmer 0.6, shimmer_tau 0.4, verb 0.15, width 0.9, low_cut 300 |
| soft_bloom | lead whoosh, lead_s 0.3, lead_f0 300, lead_f1 4000, lead_level 0.3, tick 0.7, tick_f 4000, tick_ms 10, thump 0.18, thump_f 130, material tubular, root 987.8, notes fourth, bloom_att 0.06, strike_tau 0.16, ring_mix 0.25, ring_tau 0.32, shimmer 0.25, verb 0.3, verb_rt 1.1, width 0.5, low_cut 100 |

Table B (round 4, `FAMILIES_R4['logo_sting']`): (low, low_f, low_tau) = whoosh_glass (0.04, 55, 0.16), zip_ping
(0.044, 73, 0.1), reverse_bloom (0.045, 46, 0.24), spark (0.03, 82, 0.09), soft_bloom (0.041, 62, 0.2).

Table C (round 5, final):

| name | values over round 4 |
|---|---|
| whoosh_glass | lead_s 0.1896, lead_level 0.4168, tick 0.7978, tick_f 3631, thump 0.1648, bright 0.7076, ring 0.9497, strike_tau 0.1913, ring_mix 0.2039, ring_tau 0.2963, shimmer 0.2202, verb 0.2948, width 0.319, bloom_delay 0.0078, shimmer_smooth 1 |
| zip_ping | shimmer 0.105, shimmer_smooth 1 |
| reverse_bloom | swell_trim 0.045, lead_s 0.36, shimmer 0.21, shimmer_smooth 1 |
| spark | shimmer 0.42, shimmer_smooth 1 |
| soft_bloom | lead_s 0.2471, lead_level 0.3126, shimmer 0.175, shimmer_smooth 1 |

Why round 5 changed these: the round-4 lead-ins of whoosh_glass and soft_bloom rose to within 20 dB of the peak 100 to
150 ms before the hit; they get back the shorter leads a round-3 search found. With the coarse granular layer, the
bloom's decay had steps of up to ±5 dB; every member now uses the smooth granular layer at about 0.7 × its level.
In reverse_bloom, the level fell by 15 dB in the moment before the hit; swell_trim removes the hall's build-up, and a shorter lead avoids a
quiet start.

**DEFAULTS** = a copy of the round-4 whoosh_glass (launch base + low 0.04, low_f 55, low_tau 0.16). None of the
round-5 keys is switched on in it. `render` does not merge DEFAULTS (15.6). Only its mode ('launch') is used, when p has
no mode.

**FAMILIES['pro_logo_sting'] (round 5, delivered).** Gesture base + the values below.

| name | values |
|---|---|
| chirp_hum | g_f0 1500, g_f1 150, g_ms 85, g_root 76, g_warm 0.15, g_tau1 0.07, g_mix 0.15, g_tau2 0.2, g_sink 0.5, g_land 0.8, g_land_f 900, g_air 0.1, g_snap 0, g_tone 0.6, g_wet 0.45, g_rt 1.4, g_length 0.9, g_verb fdn |
| zap_bloom | gesture zap_bloom, g_f0 7000, g_f1 700, g_ms 60, g_lead 0.5, g_air 0.3, g_root 261.6, g_notes fifth, g_warm 0.35, g_att 0.04, g_tau1 0.08, g_mix 0.15, g_tau2 0.28, g_snap 0, g_chorus 0.2, g_wet 0.4, g_rt 1.2, g_length 0.9, g_verb fdn |
| sweep_hit | gesture sweep_hit, g_f0 220, g_f1 1760, g_ms 320, g_lead 0.7, g_air 0.6, g_hit 0.6, g_low_f 52, g_low_tau 0.12, g_tone 0.55, g_snap 0.6, g_snap_f 3500, g_snap_ms 8, g_root 330, g_notes fifth, g_warm 0.2, g_tau1 0.08, g_mix 0.12, g_tau2 0.25, g_wet 0.3, g_rt 1.0, g_length 0.75 |
| taps_tone | gesture taps_tone, g_gap 0.075, g_tap_f 1046.5, g_lead 0.8, g_root 523.25, g_notes one, g_mat bar, g_warm 0.35, g_att 0, g_bend 0.3, g_tau1 0.06, g_mix 0.12, g_tau2 0.25, g_snap 0.6, g_snap_f 7000, g_snap_ms 3, g_wet 0.35, g_rt 1.1, g_length 0.8, g_verb fdn |
| swell_sting | gesture swell_sting, g_ms 450, g_lead 0.6, g_root 293.66, g_notes triad, g_warm 0.12, g_chorus 0.4, g_tau1 0.08, g_mix 0.12, g_tau2 0.28, g_hit 0, g_low_f 48, g_snap 0, g_air 0.5, g_wet 0.45, g_rt 1.2, g_length 0.85 |

**FAMILIES_R4['pro_logo_sting'] (round 4 chirp family, kept).** Chirp r4 base + the values below.

| name | values |
|---|---|
| ref | ctick 0.4 |
| zap | chirp_f0 2600, chirp_f1 240, chirp_ms 70, hum_f 98, hum_formant 600, length 0.7, hum_slow 0.16, land_f 1100, thud 0.3, hum_buzz 0.55, sub 0.15, sub_db 110, zap 0.6, zap_k 4, chirp_harm 0.5, ctick 0.2 |
| deep | chirp_f0 950, chirp_f1 100, chirp_ms 130, hum_f 56, hum_formant 380, sub 0.35, length 1.0, hum_slow 0.26, thud 0.7, land 0.4, land_f 650, hum_level 1.05, hum_buzz 1.0, sub_db 100, ctick 0.3, hum_sink 1.0 |
| bell | shimmer 0.1, root 1100, hum_buzz 0.3, hum_formant 520, land 0.35, zap 0.2, zap_k 8, thud 0.4, bell_decay 0.16, chirp_harm 0.4, ctick 0.25, hum_level 0.75 (effective preset; see the note) |
| double | hum_slow 0.3, hum_mix 0.08, length 1.0, hum_sink 0.8, chirp_harm 0.8, land 0.3, thud 0.55, sub 0.2, sub_db 90, double 0.7, double_gap 0.012, zap 0.4, zap_k 7, chirp_f0 1700, hum_f 70 |

Note on `bell`: in the source, a comment on one line hides 4 intended keys (chirp_f0 1900, chirp_f1 190, chirp_ms 75,
hum_f 84). So the effective values come from the chirp r4 base: chirp_f0 1500, chirp_f1 150, chirp_ms 85, hum_f 76.
This spec states the effective preset (index, Open questions).

**FAMILIES_R3['pro_logo_sting'] (round 3 chirp family).** Chirp r3 base + the values below.

| name | values |
|---|---|
| ref | (none) |
| high | chirp_f0 2600, chirp_f1 240, chirp_ms 70, hum_f 98, hum_formant 600, length 0.7, hum_slow 0.16, land_f 1100, thud 0.3, hum_buzz 0.55, sub 0.15, sub_db 110 |
| deep | chirp_f0 1200, chirp_f1 120, chirp_ms 105, hum_f 62, hum_formant 380, sub 0.35, length 1.0, hum_slow 0.26, thud 0.7, land 0.4, land_f 650, hum_level 1.05, hum_buzz 1.0, sub_db 100 |
| bell | shimmer 0.1, root 1100, hum_buzz 0.4, hum_formant 520, land 0.35, thud 0.4, bell_decay 0.16, chirp_f0 1900, chirp_f1 190, chirp_ms 75, hum_f 84 |
| long | hum_slow 0.45, hum_mix 0.1, length 1.3, hum_sink 0.8, chirp_harm 0.7, verb 0.12, verb_rt 1.2, land 0.3, thud 0.55, sub 0.2, sub_db 80 |

`FAMILIES_R3` = {logo_sting: table A list, pro_logo_sting: this list}.

**FAMILIES_R2** (round 2). `logo_sting` (reveal base + values):

| name | values |
|---|---|
| glass_bloom | material glass, root 1760, chord add9, bloom_decay 1.2, bright 0.8, strike 0.8, shimmer 0.35, hit 0.15, sub 0, low_cut 450, open_ms 30 |
| triangle | material triangle, root 1480, chord fifth, bloom_decay 1.9, bright 0.85, open_ms 10, strike 0.9, shimmer 0.15, hit 0.1, sub 0, verb 0.25, low_cut 500 |
| fm_reveal | material fm, root 1320, chord major, bloom_decay 1.3, bright 0.8, swell 0.6, swell_level 0.25, hit 0.2, sub 0, open_ms 60, low_cut 350 |
| chime_hit | material tubular, root 1175, chord sus2, bloom_decay 1.5, bright 0.75, strike 0.8, hit 0.3, sub 0, shimmer 0.1, verb 0.4, verb_rt 2.4, low_cut 300, open_ms 25 |
| bar_sparkle | material bar, root 1980, chord octave, bloom_decay 1.1, bright 0.8, strike 0.9, shimmer 0.55, hit 0, sub 0, width 0.9, open_ms 10, low_cut 500 |

`pro_logo_sting` (chirp r2 base + values): ref (none); high (chirp_f0 2600, chirp_f1 240, chirp_ms 70, hum_f 98,
hum_formant 600, length 0.7, hum_slow 0.16); deep (chirp_f0 1200, chirp_f1 120, chirp_ms 105, hum_f 62, hum_formant
380, sub 0.25, length 1.0, hum_slow 0.26); bell (shimmer 0.1, root 950, hum_buzz 0.4, hum_formant 520); long
(hum_slow 0.45, hum_mix 0.1, length 1.3, hum_sink 0.8, chirp_harm 0.7, verb 0.12, verb_rt 1.2).

`FAMILIES` holds only logo_sting and pro_logo_sting. `FAMILIES_R4` = {logo_sting: round-4 list, pro_logo_sting:
round-4 chirp list}.

### 15.6 Search space, study targets, recipe_for

Search keys per category (final):

- logo_sting: lead_s, lead_level, tick, tick_f, thump, ring, strike_tau, ring_mix, ring_tau, shimmer, verb, width,
  low_cut, bright, bloom_delay. Limits: lead_s 0.15 to 0.6; ring_tau 0.15 to 0.45 log; verb 0 to 0.45; low_cut 60 to
  600 log; lead_level 0.25 to 0.7; tick 0.6 to 1.5; thump 0 to 0.4; bloom_delay 0.004 to 0.025.
- pro_logo_sting: g_lead, g_warm, g_att, g_tau1, g_mix, g_tau2, g_land, g_air, g_hit, g_low_tau, g_snap, g_snap_f,
  g_wet, g_rt, g_length. Limits: g_warm 0 to 0.45; g_wet 0.1 to 0.6; g_length 0.5 to 1.2.
- The source also keeps the round-4 chirp search for pro_logo_sting: chirp_f0, chirp_f1, chirp_ms, chirp_harm,
  hum_f, hum_sink, hum_formant, hum_buzz, hum_level, hum_fast, hum_slow, hum_mix, length, land, thud, sub, sub_db;
  limits chirp_f0 900 to 3000 log, chirp_f1 100 to 280 log, chirp_ms 50 to 130 log, hum_f 55 to 105 log, length 0.6
  to 1.6, land 0.2 to 0.9, thud 0.2 to 0.9, sub 0.1 to 0.5, sub_db 60 to 130.

`space_for(cat)` = {k: limit[k] if present, else SPACE[k]} for the search keys of cat.

`recipe_for(cat, preset=0)`: preset is a list index or a member name. Returns an object with: the module name,
SPACE = space_for(cat), DEFAULTS = a copy of the preset's params, render = this render, TARGET = TARGETS[cat], ALTS,
REFSET = cat, PRESET = the preset name. The forge searches the search keys around one preset.

`STUDY_STING` (ranges for the impact recipe's study penalty):

| cat | ranges |
|---|---|
| logo_sting | t40 650 to 1150 ms; tilt 1.5 to 10; sh150 0 to 0.12 |
| pro_logo_sting | len50 0.5 to 1.5; t20 30 to 110; t40 150 to 450; tilt -9 to -5; corr 0.9 to 1.0; ridge0 900 to 2400; ridge1 55 to 110 |

### 15.7 render(p, rng)

1. mode = p['mode'] if present, else DEFAULTS['mode'] ('launch').
2. p = (base of the mode) overlaid with p. chirp → chirp r2 base; launch → launch base; gesture → gesture base; any
   other mode → reveal base.
3. y = the idiom render: chirp (15.8), launch (15.10), gesture (15.11), otherwise reveal (15.9).
4. Return y·0.89/(max|y| + 1e-12) (the forge-core wrapper normalises again, index "Interface").

Every idiom returns (n, 2). chirp returns L = R.

### 15.8 The chirp idiom

**chirp(p, n)**: L = min(n, P1(1.25·T)), T = chirp_ms/1000. For t < L/SR: f = chirp_f0·(chirp_f1/chirp_f0)^clip(t/T,
0, 1). ph = 2π·cumsum(f)/SR + U(0, 0.3). y = sin ph + h·(0.5·sin 2ph + 0.25·sin 3ph), h = chirp_harm. Zero y wherever
2f ≥ 0.45·SR. e = (1 - exp(-t/0.003))·(0.3 + 0.7·clip(t/T, 0, 1))·clip((1.25 - t/T)/0.25, 0, 1)^1.5 (a crescendo
into the landing, then gone within a quarter of T). Return y·e in n samples, unit peak.

**hum(p, n, t0)**: tt = max(t - t0, 0). dur = max(t_last - t0, 0.1). f = hum_f·2^(-hum_sink·clip(tt/dur, 0, 1)^0.7).
ph = 2π·cumsum(f)/SR. For k = 1 .. 12: a = (1/k)^(1.6 - 1.1·hum_buzz); form = 1/(1 + (log2(k·f/hum_formant)/0.7)²);
y += a·(0.35 + form)·sin(k·ph + U(0, 6.28)) where k·f < 0.45·SR. e = (exp(-tt/hum_fast) + hum_mix·exp(-tt/hum_slow))·
(1 - exp(-tt/0.004)), zero before t0. Return y·e, unit peak.

**landing(p, n, t0)** (no random draws): tt = max(t - t0, 0); on = [t ≥ t0]·(1 - exp(-tt/0.002)).
If land > 0.01: f = land_f·2^(-0.5·(1 - exp(-tt/0.03))); y += land·sin(2π·cumsum(f·[tt > 0])/SR)·exp(-tt/0.025)·on.
If thud > 0.01: f = 2·hum_f·(1 + exp(-tt/0.04)); y += thud·sin(2π·cumsum(f·[tt > 0])/SR)·exp(-tt/0.05)·on.

**zap(p, n)**: L, T and f as in chirp. fc = min(zap_k·f, 15000). y = svf(L white samples, fc, 3, bandn).
e = (1 - exp(-t/0.002))·(0.4 + 0.6·clip(t/T, 0, 1))·clip((1.25 - t/T)/0.25, 0, 1)^1.5. Return bw-lp(13000, 2) of y·e,
in n samples, unit peak.

**ctick(n, t0)**: L = P1(0.005), u = i/L. w = bw-bp(5000, 13000, 2) of (L white samples)·(1 - u)³·(1 -
exp(-i/(0.0002·SR))). Place w/(max|w|) at sample P1(t0), cut at n.

**render (chirp)**:

1. n = P1(length). y = chirp(p, n). t0 = 0.92·chirp_ms/1000 (the chirp lands near its bottom). y = y +
   hum_level·hum(p, n, t0).
2. If land > 0.01 or thud > 0.01: y += landing(p, n, t0).
3. If zap > 0.005: y += zap·zap(p, n). If ctick > 0.005: y += ctick·ctick(n, t0).
4. If sub > 0.01: sd = the subdrop recipe's `pro_sub_drop` member 0 ('ref') overlaid with f0 = hum_f, f1 =
   0.66·hum_f, bloom 0.06, click 0, length = length, decay_db = sub_db (45 if absent). s = the channel mean of the
   subdrop render of sd. Delay s by P1(t0), cut to n. y += sub·s (no level normalisation of s).
5. If shimmer > 0.01: b = bell(root, max(0.3, length - t0), bar, decay = bell_decay (0.35 if absent), bright 0.3,
   strike 0). Delay b by P1(t0), cut to n. y += 0.6·shimmer·b.
6. If verb > 0.01: h = hall(verb_rt, 0.6, 0.5, 0.008). y = y + verb·conv(y, h)[0:n].
7. If double > 0.005: q = p with chirp_f0 = min(3500, 1.35·chirp_f0), chirp_f1 = 2.5·chirp_f1, chirp_ms = 0.55·
   chirp_ms. k = P1(1.25·q.chirp_ms/1000). pre = chirp(q, k); if zap > 0.005: pre += zap·zap(q, k). pre = (pre unit
   peak) followed by P1(double_gap) zeros (double_gap 0.05 if absent). y = [double·max|y|·pre, y] (pre-chirp first;
   the file gets longer).
8. y = bw-hp(30, 2). y = fade(y, 0.0008, min(0.12, 0.2·length)). Return [y, y].

### 15.9 The reveal idiom

**note(p, f, s)**: material fm: x = fm(f, 1.4, 3 + 5·bright, s, decay 0.45·bloom_decay, index_decay 0.12, attack
0.002) + 0.35·fm(2f, 3.5, 2.0, s, decay 0.2·bloom_decay, index_decay 0.05, attack 0.002); unit peak. Other materials:
bell(f, s, TRIANGLE for triangle else the named preset, decay = bloom_decay·(0.35 for triangle, else 0.5), slope 0.5
for triangle and glass else 0.8, bright = bright, beat 1.2, strike 0.2 + 0.6·strike).

**bloom(p)**: s = min(5.5, 1.6·bloom_decay + 0.4), n = P1(s). For j, st in CHORDS[chord]: f = root·2^(st/12); skip if
f > 9000; x = note(p, f, s)·0.85^j; d = P1(j·U(0.004, 0.012)) (a 4 to 12 ms flam; for j = 0 the P1 rule gives 1
sample); y[d:] += x[0 : n - d]. fc(t) = 900 + 17000·(1 - exp(-t/max(open_ms/1000, 0.005))). Return svf(y, fc, 0.6,
low), unit peak.

**render (reveal)**:

1. b = bloom(p), n = len(b). out = [b, b].
2. If shimmer > 0.01: src = bw-hp(2500, 2) of b[0 : P1(1)]. sh = fx_granular(src, n/SR). sh = sh·exp(-t/max(0.4,
   0.6·bloom_decay)). out += shimmer·(sh unit peak).
3. If width > 0.01: hi = bw-hp(600, 2) of b. out += decor(hi, 0.8·width, cut 250) - [hi, hi] (only the part above
   600 Hz is widened).
4. If verb > 0.01: for channel c = 0, 1: h = hall(verb_rt, 0.35, 0.8, 0.02 + 0.004·c); out[c] += verb·conv(out[c],
   h)[0:n].
5. pk = max|out|.
6. If hit > 0.01: hp = the impact recipe's final `impact` family member at index 2 ('punch') overlaid with roll_hold
   0.5, roll_fall 0.25, hall_rt 1.4, hall_wet 0.35, floor_db -110, crack_f 3500. h = impact render of hp, cut to n.
   out[0 : len h] += hit·pk·(h unit peak).
7. If sub > 0.01: s = the subdrop render of the `sub_drop` member 0 ('808') overlaid with length 1.6, click 0, cut to
   n. out[0 : len s] += sub·pk·(s unit peak).
8. If swell > 0.02: k = P1(swell). h = hall(max(0.6, 1.8·swell), 0.3, 0.7, 0, seconds 1.3·swell + 0.2).
   sw = conv(b[0 : P1(0.25)], h)[0:k], reversed in time. sw = sw·(linspace(0, 1))². sw = fade(sw, 0.01, 0.002).
   sw = swell_level·pk·(sw unit peak). out = [sw, sw] followed by out (the file gets longer).
9. out = Butterworth high-pass at low_cut, order 2 if low_cut < 60, else 4. out = trim(out, -66, 0.05).
   out = fade(out, 0.001, min(0.4, 0.15·len/SR)).

`fx_granular(src, s, rng)` = granular(src, s, grain 0.05, density 50, pos (0, 0.6), pos_jitter 0.05, pitch +12
semitones, pitch_jitter 0.15, spread 0.9).

### 15.10 The launch idiom

**lead whoosh(p, n)**: u = t/max(t_last, 1e-6). fc = lead_f0·(lead_f1/lead_f0)^(u^1.7) (steepest at the end).
lvl = 0.03 + 0.97·(e^(4.5u) - 1)/(e^4.5 - 1). base = air(n/SR, center 2000, flutter 0.35, rate 7) cut to n. For each
channel: w = 0.5·base + 0.5·(a new air signal with the same settings); y = svf(w, fc, 1.1, bandn) + 0.35·svf(w,
min(2.2·fc, 16000), 0.8, bandn); channel = y·lvl. Then fade(·, 0.03, 0.002), unit peak.

**lead chirp(p, n)**: u as above. f = lead_f0·(lead_f1/lead_f0)^(u^1.3). ph = 2π·cumsum(f)/SR + U(0, 0.3).
y = sin ph + 0.35·sin 2ph·[2f < 0.4·SR] + 0.15·sin 3ph·[3f < 0.4·SR]. y = y·(0.05 + 0.95·u^2.2). y = fade(y, 0.004,
0), unit peak. Return [y, y].

**lead swell(p, n, bloom)**: h = hall(max(0.6, 1.8·lead_s), 0.3, 0.7, 0, seconds 1.3·lead_s + 0.2).
d = round(swell_trim·SR) (swell_trim 0 if absent). sw = conv(bloom[0 : P1(0.25)], h)[d : n + d], reversed.
sw = sw·(0.03 + 0.97·linspace(0, 1)²). ld = fade(decor(sw, 0.6, cut 250), 0.03, 0). Unit peak.

**tick(p, n)**: L = min(n, max(16, P1(tick_ms/1000))), u = i/L. w = (L white samples)·(1 - u)³·(1 -
exp(-i/(0.0002·SR))); w = bw-hp(tick_f, 2) of w; y[0:L] = w unit peak. k = P1(0.0004); y[0:k] += 0.35·(0.5 -
0.5·cos(2π·i/(k - 1))) (a 0.4 ms contact pulse). Unit peak.

**thump(p, n)**: f = thump_f·(1 + exp(-t/0.03)). y = sin(2π·cumsum(f)/SR)·exp(-t/0.07)·(1 - exp(-t/0.001)). Unit
peak.

**low strike(low_f, low_tau, n)** (no random draws): f = low_f·(1 + 1.5·exp(-t/0.035)) (from 2.5 × low_f onto low_f
in about 35 ms). ph = 2π·cumsum(f)/SR. rise = clip(t/0.012, 0, 1)² (it rises over 12 ms while the tick plays).
y = (sin ph + 0.25·sin 2ph)·rise·exp(-t/low_tau). y = bw-hp(25, 2). Unit peak.

**bloom note(p, f, s)**: fm: x = fm(f, 1.4, 2 + 4·bright, s, decay 10, index_decay 0.06, attack 0.002) + 0.25·fm(2f,
3.5, 1.5, s, decay 10, index_decay 0.03, attack 0.002), unit peak. Others: bell(f, s, TRIANGLE or the preset, decay
1.2, slope 0.5 for triangle and glass else 0.7, bright, beat 1.5, strike 0). The shared envelope below sets the
length.

**short bloom(p, n)**: s = n/SR. Note j of DYADS[notes] (semitone offset st_j) has the pitch f_j = root·2^(st_j/12);
notes above 9500 Hz are left out. Each note is the bloom note of f_j over s seconds, cut to n samples, scaled by
0.7^j and delayed by d_j = P1(j·U(0.006, 0.014)) samples (a flam). y is their sum. The concave envelope is
e(t) = (1 - ring_mix)·exp(-t/strike_tau) + ring_mix·exp(-t/ring_tau).
- With bloom_att > 0.003: during t < bloom_att, e is also multiplied by 1 - (1 - t/bloom_att)² (a fast start), and
  y·e passes through svf(·, fc, 0.6, low) with fc(t) = 1500 + 16000·(1 - exp(-t/max(1.5·bloom_att, 0.005))).
- With bloom_att ≤ 0.003: the result is y·e with no filter.

The result is scaled to unit peak.

**smooth shimmer(src, s, k)** (k = shimmer_smooth): granular(src, s, grain 0.05·(1 + 0.8k), density 50·(1 + 3k), pos
(0, 0.6), pos_jitter 0.05, pitch +12, pitch_jitter 0.15, spread 0.9). At k = 1: 4 × the density of 1.8 × longer
grains, so about 14 grains overlap.

**render (launch)**:

1. The lead kind is p['lead']; a lead_level of 0.005 or less forces 'none'. The lead length n_lead is P1(lead_s),
   or 0 for 'none'. The body length is n = P1(min(4, max(0.6, 5.5·ring_tau, 6·strike_tau) + 0.3)).
2. b = short bloom(p, n). d = P1(bloom_delay) (0 if absent). If d > 1: b[0:d] *= 1 - (1 - linspace(0, 1, d))³ (the
   bloom fades in under the tick; this curve starts fast, -6 dB at 0.2 × the delay).
3. hit = tick·tick(p, n). If thump > 0.01: hit += thump·thump(p, n).
4. body = [ring·b + hit] in both channels.
5. If width > 0.01: hi = ring·bw-hp(600, 2) of b. body += decor(hi, 0.8·width, cut 250) - [hi, hi].
6. If shimmer > 0.01: src = bw-bp(2500, 9000, 4) of b[0 : P1(0.5)]. k = shimmer_smooth (0 if absent). sh =
   fx_granular(src, n/SR) if k ≤ 0.005, else smooth shimmer(src, n/SR, k). sh = sh·(1 - exp(-t/0.02))·
   exp(-t/shimmer_tau). body += shimmer·ring·(sh unit peak).
7. If verb > 0.01: send = bw-hp(500, 4) of body (a low tone in the room rang through the tail). For c = 0, 1: h =
   hall(verb_rt, 0.6, 0.5, 0.008 + 0.003·c); body[c] += verb·conv(send[c], h)[0:n].
8. pk = max|body|.
9. If n_lead > 64: k = P1(0.012); m = n_lead + k. ld = lead whoosh(p, m), lead chirp(p, m) or lead swell(p, m, b)
   (b after step 2). ld[n_lead:] *= (linspace(1, 0, k))² (the lead continues 12 ms past the hit start and fades out there).
   body = n_lead zero rows followed by body. body[0 : m] += lead_level·pk·ld.
10. out = Butterworth high-pass at low_cut, order 2 if low_cut < 60, else 4.
11. If low > 0.005: add low·pk·low strike(low_f, low_tau, n) to both channels, starting at sample n_lead (when the
    lead was placed, n_lead > 64) or at sample 0. It comes after the low cut and does not pass through the room.
12. out = bw-lp(16500, 4) (the noise parts would otherwise run flat to 24 kHz). out = trim(out, -62, 0.05).
    out = fade(out, 0.0015, min(0.3, 0.2·len/SR)).

Edge case: a lead with 0 < n_lead ≤ 64 samples (lead_s under about 1.3 ms) is skipped, with no zero prefix.

### 15.11 The gesture idiom

**sweep(n, f0, f1, T, c)**: u = clip(t/max(T, 1e-4), 0, 1)^c. f = f0·(f1/f0)^u (held at f1 after T). ph =
2π·cumsum(f)/SR + U(0, 0.3). Returns (ph, f).

**tone(n, f, p, sink = 0, att = 0, harm_tau = 0.12)**:

1. dur = max(t_last, 0.1). fr = f·2^(-sink·clip(t/dur, 0, 1)^0.7). If g_bend > 0.01: fr = fr·2^((g_bend/12)·
   exp(-t/0.025)).
2. ph = 2π·cumsum(fr)/SR + U(0, 6.28). w = g_warm, ch = g_chorus. det = (2^(8/1200) - 1)·2π·cumsum(fr)/SR.
3. osc(k, φ) = sin(k·ph + φ). If ch > 0.01: osc = (sin(k·ph + φ) + 0.5·ch·(sin(k·(ph + det) + φ + 1.3) + sin(k·(ph -
   det) + φ + 2.6)))/(1 + ch). (k may be a non-integer ratio.)
4. y = osc(1, 0). Extra partials = G_MATS[g_mat], or for sine: (2, 0.55, harm_tau), (3, 0.22, 0.6·harm_tau). For each
   (k, a, tau): a' = a·w; if k·max(fr) < 0.42·SR and a' > 0: y += a'·exp(-t/tau)·osc(k, U(0, 6.28)).
5. e = (1 - g_mix)·exp(-t/g_tau1) + g_mix·exp(-t/g_tau2). If att > 0.003: e = e·(1 - (1 - clip(t/att, 0, 1))²);
   y = svf(y·e, 900 + 15000·(1 - exp(-t/max(1.2·att, 0.004))), 0.6, low). Else e = e·(1 - exp(-t/0.0015)); y = y·e.
6. Unit peak.

**chord(n, p, att = 0)**: for j, st in CHORDS_G[g_notes]: f = g_root·2^(st/12); skip if f > 9000; x = tone(n, f, p,
att = att)·0.72^j; d = P1(j·U(0.005, 0.012)); y[d:] += x[0 : n - d]. Unit peak.

**noise ride(n, f, k, q)**: fc = clip(k·f, 200, 14000). y = svf(n white samples, fc, q, bandn). Return bw-lp(13000, 2)
of y.

**snap(n, p)**: L = min(n, max(32, P1(g_snap_ms/1000))), u = i/L. w = (L white samples)·(1 - u)³·(1 -
exp(-i/(0.0002·SR))); w = bw-bp(g_snap_f/1.6, min(1.8·g_snap_f, 16000), 2). y[0:L] = w unit peak.

**hit(n, p)**: y = g_hit·low strike(g_low_f, g_low_tau, n). If g_snap > 0.005: y += g_snap·snap(n, p). Unit peak (all
zeros stays zeros).

**tail(dry, p)**: out = [dry, dry]. If g_wet > 0.005: send = bw-hp(250, 2) of dry. With g_verb fdn: wet = the
forge-core reverb of send (rt60 g_rt, size 0.7, predelay 0.012, damp 0.5, wet 1, dry 0, stereo, width 1); out +=
g_wet·wet[0:n]. With g_verb hall: for c = 0, 1: h = hall(g_rt, 0.55, 0.6, 0.012 + 0.005·c); out[c] += g_wet·
conv(send, h)[0:n].

**render (gesture)**, with L = g_length and T = g_ms/1000:

- **chirp_hum**: n = P1(T + L). (ph, f) = sweep(n, g_f0, g_f1, T, 1). u = clip(t/T, 0, 1). ec = (1 - exp(-t/0.003))·
  (0.3 + 0.7u)·clip((1.2 - t/T)/0.2, 0, 1)^1.5. lead = (sin ph + 0.12·sin 2ph)·ec. If g_air > 0.005: lead +=
  0.5·g_air·noise ride(n, f, 5, 3)·ec. t0 = 0.92·T, i0 = P1(t0). tone[i0:] = tone(n - i0, g_root, p, sink g_sink,
  harm_tau 0.2). y = g_lead·(lead unit peak) + g_tone·tone. If g_land > 0.005: y += landing(land g_land, land_f
  g_land_f, thud 0, hum_f g_root; n, t0). If g_snap > 0.005: y[i0:] += g_snap·snap(n - i0, p).
- **zap_bloom**: n = P1(T + L). f from sweep(n, g_f0, g_f1, T, 0.6). ez = (1 - exp(-t/0.002))·clip(1 - t/(1.15·T), 0,
  1)^0.7. nz = noise ride(n, f, 1.3, 3)·ez. ph from a second sweep(n, g_f0, g_f1, T, 0.6) (a new phase offset).
  sz = (sin ph + 0.1·sin 2ph·[2f < 0.42·SR])·ez. zap = (1 - g_air)·(sz unit peak) + g_air·(nz unit peak).
  i0 = P1(0.7·T). bloom[i0:] = chord(n - i0, p, att g_att). y = g_lead·(zap unit peak) + g_tone·bloom.
- **sweep_hit**: nl = P1(T), n = nl + P1(L). (ph, f) = sweep(n, g_f0, g_f1, T, 1.6). u = clip(t/T, 0, 1).
  lvl = (0.02 + 0.98·(e^(4u) - 1)/(e^4 - 1))·[t < T + 0.006]·clip((T + 0.006 - t)/0.006, 0, 1)^0.5.
  lead = (sin ph + 0.15·sin 2ph)·lvl. If g_air > 0.005: lead += g_air·noise ride(n, f, 4, 1.4)·lvl.
  lead = fade(lead, 0.02, 0). hit[nl:] = hit(n - nl, p). ring[nl:] = chord(n - nl, p).
  y = g_lead·(lead unit peak) + hit + g_tone·ring.
- **taps_tone**: gap = g_gap. i2 = P1(2·gap), n = i2 + P1(L). For (j, lv) in (0, 0.55), (1, 0.75): k = P1(0.05);
  over k samples, fq = g_tap_f·(1 + 0.06·exp(-t/0.004))·2^(2j/12) (the second tap is a whole tone higher);
  tap = sin(2π·cumsum(fq)/SR)·exp(-t/0.011)·(1 - exp(-t/0.0004)) + 0.25·snap(k, p with g_snap_ms 2.5); i = P1(j·gap);
  y[i : i + k] += g_lead·lv·(tap unit peak). y[i2:] += g_tone·chord(n - i2, p, att g_att). If g_snap > 0.005:
  y[i2:] += 0.5·g_snap·snap(n - i2, p).
- **swell_sting** (also any other gesture name): nl = P1(T), nb = P1(L). stab = chord(nb, p). h = hall(max(0.6, 2T),
  0.4, 0.7, 0, seconds 1.3·T + 0.25). k = P1(0.012). sw = conv(stab[0 : P1(0.3)], h)[P1(0.02) : P1(0.02) + nl + k],
  reversed. sw = sw·(0.02 + 0.98·linspace(0, 1)^2.2). sw = fade(sw, 0.03, 0), unit peak. If g_air > 0.005: u =
  linspace(0, 1, len sw); a = bw-bp(3000, 12000, 2) of (white)·u³; sw = sw + g_air·(a unit peak); sw = fade(sw unit
  peak, 0.03, 0). sw[nl:] *= (linspace(1, 0))². n = nl + nb. y[0 : len sw] += g_lead·sw. y[nl:] += g_tone·stab +
  0.8·hit(nb, p).

Then, for every gesture: y = bw-hp(30, 2). out = tail(y, p). out = bw-lp(16000, 4). out = trim(out, -64, 0.05).
out = fade(out, 0.0008, min(0.25, 0.2·len/SR)).

### 15.12 Seed bank `sting.json`

28 entries in 3 contexts: "a designed logo sting: a falling chirp landing on ..." (11), "a short bright logo reveal
sting: a whoosh into a ..." (11), and the round-2 target "a bright logo sting: a chime chord ringing out" (6).
Delivered: 5 `pro_logo_sting` (chirp_hum, zap_bloom, sweep_hit, taps_tone, swell_sting: the round-5 gesture family,
33 keys, seed 0) and 5 `logo_sting` (whoosh_glass, zip_ping, reverse_bloom, spark, soft_bloom: the round-5 launch
family, 31 or 32 keys). The 18 undelivered entries are older searched sets: 6 in mode chirp, 6 in mode launch and 6 in mode reveal. Each
carries its mode, so the same `render` plays it.
