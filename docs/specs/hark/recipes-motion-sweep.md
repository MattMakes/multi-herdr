# recipes-motion, part 2: sweeps (riser, downlifter, zip, darkriser, revcymbal)

This part belongs to the index `recipes-motion.md`. Read the index first: it defines the recipe contract, the shared
primitives (P1 to P26) and the seed-bank format.

Notation as in part 1: `lp`, `hp`, `bp`, `bpn` are the state-variable filter modes of P2; `bw(order)` is a
Butterworth filter (P3); u = t / length is the progress from 0 to 1.

## 5. riser

### 5.1 The sound

A designed riser (uplifter, build) for video edits: 0.8 to 4 s of growing tension. The last sample of the file is
the moment of the hit, and the cut falls there. The sound is designed; it does not model a physical source. Rules that the design follows:

- One motion leads, the others support. Up to 7 layers, each owning a band: noise, tonal, octave, shepard, drone,
  metal, reverb send.
- The curves speed up toward the end (concave in dB). The level, the filters and the pitch all follow w = u^curve. With curve 2
  to 3, the largest part of each change happens in the final third.
- A stutter gate whose period shrinks from gate_p0 to gate_p1. The gate phase is computed from the end backward, so
  the gate is open on the final sample. Over the final 45 ms the gate opens completely. The level is at its maximum
  when the riser stops.
- A hard end: the highest level is in the final ~50 ms, followed by a fade of 2 to 20 ms.
- No layer piles up energy under 100 to 200 Hz except the drone.

Target "a cinematic riser building tension into a hit". Alternatives: "a jet engine", "an air raid siren", "heavy
rain on a roof", "a vacuum cleaner". Refset `riser`. A second category `pro_riser` has target "a short stuttering
digital riser into a cut" (`TARGETS`).

### 5.2 Interface

- `SPACE` (5.3), `SPACE_R6` and `R6_OFF` (5.9).
- `FAMILIES`: member name to a full parameter dict (5.4). Members: air, tension, stutter, shepard, rush, cymbal.
- `PRO`: the old `pro_riser` set, kept for the record (5.5).
- `FAMILIES_BY_CAT`: `riser` (5.6) and `pro_riser` (5.7), each member name to a full dict.
- `R4_EDITS = {'riser': [5 dicts]}`: edits applied after a forge run to the 5 delivered `riser` members, in the order
  air, tension, shepard, rush, cymbal (5.8). The render does not use them. They are data for the bank and the forge.
- `R6`: the "build" presets (5.9).
- `TARGETS = {'pro_riser': ...}`. `DEFAULTS` = a copy of `FAMILIES['air']`.
- `UNISON` = (-1.0, -0.45, 0.0, 0.45, 1.0): the 5 voice offsets (times detune in cents) and pans (times width).
- `FINAL_OPEN` = 0.045 s.
- Shared helpers that downlifter and zip import: `lvl`, `phase_from_end`, `gate_shape`, `unison`, `verb_ir`,
  `wet_of`, `stereo_of`, `env_db`, `band_env_db`, `side_db`, `low_share_db`, `centroid_track` (5.10).
- `measure(x) -> dict` and `target_check(x, row=None) -> float` (5.12).
- `render(p, rng)`: 5.11. `render_build(p, rng)`: 5.9.

### 5.3 SPACE

| key | range | scale | meaning |
|---|---|---|---|
| length | 0.8 to 4.0 | | s; the hit lands on the last sample |
| curve | 1 to 4 | | acceleration of level, filter and pitch (1 = straight in dB) |
| rise_db | 12 to 45 | | level increase from the start to the end (dB) |
| noise_db | -36 to 0 | | noise layer level |
| nf0 | 150 to 1500 | log | noise low-pass at the start |
| nf1 | 2500 to 16000 | log | noise low-pass at the end |
| nq | 0.7 to 4 | log | noise filter resonance |
| pink | 0 to 1 | | 0 white, 1 pink |
| tone_db | -36 to 0 | | tonal layer level |
| tmode | glide, blips, nblips | choice | tonal mode |
| wave | sine, tri, saw | choice | tonal waveform |
| tf0 | 60 to 4000 | log | tonal start pitch (Hz) |
| toct | 0.5 to 3 | | octaves of rise |
| tbright | 1.5 to 12 | log | saw low-pass = pitch × tbright |
| detune | 0 to 30 | | unison spread in cents |
| oct_db | -36 to -3 | | the +12 layer, fades in late |
| shep_db | -48 to 0 | | shepard layer level |
| shep_rate | 0.3 to 1.6 | | shepard glide, octaves per second |
| drone_db | -48 to -3 | | drone level |
| drone_f | 35 to 110 | log | drone pitch |
| metal_db | -48 to 0 | | level of non-harmonic metallic partials (like a reversed cymbal) |
| metal_f | 1500 to 5000 | log | frequency of the lowest metallic partial |
| gate_depth | 0 to 1 | | |
| gate_on | 0 to 0.7 | | the gate starts to fade in at this fraction of the length |
| gate_p0 | 0.02 to 0.5 | log | gate period at the start (s) |
| gate_p1 | 0.012 to 0.08 | log | gate period at the end (s) |
| gate_duty | 0.25 to 0.8 | | |
| gate_sharp | 0 to 1 | | 0 smooth tremolo, 1 hard gate with 1.2 ms ramps |
| gate_drone | 0 to 1 | | gate depth applied to the drone |
| blip_dec | 0.1 to 0.5 | | blip decay time constant as a share of the pulse period |
| verb | 0 to 0.6 | | reverb return level |
| rt60 | 0.6 to 3.5 | log | reverb decay |
| width | 0 to 1 | | |
| sat | 0 to 12 | | tanh saturation of the mix (dB) |
| hp | 60 to 500 | log | high-pass of every layer except the drone |
| end_ms | 2 to 20 | log | final fade (ms) |
| drone_oct | 0 to 1.5 | | drone pitch rise over the build (octaves) |
| drone_noise | 0 to 1 | | 0 saw + sine drone, 1 a low noise rumble |
| metal_n | 9 to 40 | int | metal partial count; 9 = the fixed ratio table, above 9 = random ratios, closer together |
| metal_glide | 0 to 1 | | octaves the metal partials glide up |
| nb_q | 2 to 16 | log | Q of each noise blip (tmode nblips) |

### 5.4 FAMILIES (hand-designed presets)

Every member also has: metal_db -48, metal_f 2600 and blip_dec 0.35 unless its row says otherwise, and always
drone_oct 0.08, drone_noise 0.0, metal_n 9, metal_glide 0.0, nb_q 6.0.

| key | air | tension | stutter | shepard | rush | cymbal |
|---|---|---|---|---|---|---|
| length | 2.6 | 3.2 | 0.9 | 3.8 | 1.5 | 2.2 |
| curve | 2.4 | 2.6 | 1.6 | 1.8 | 3.2 | 2.8 |
| rise_db | 36 | 32 | 16 | 30 | 30 | 40 |
| noise_db | 0 | -10 | -20 | -12 | 0 | -4 |
| nf0 | 300 | 250 | 600 | 200 | 500 | 900 |
| nf1 | 12000 | 9000 | 12000 | 7000 | 15000 | 16000 |
| nq | 1.6 | 1.2 | 1.0 | 2.2 | 2.4 | 0.8 |
| pink | 0.55 | 0.7 | 0.3 | 0.9 | 0.4 | 0.25 |
| tone_db | -20 | 0 | 0 | -36 | -10 | -36 |
| tmode | glide | glide | blips | glide | glide | glide |
| wave | tri | saw | sine | sine | sine | tri |
| tf0 | 400 | 110 | 1700 | 300 | 260 | 400 |
| toct | 1.5 | 2.0 | 2.6 | 1.0 | 2.8 | 1.5 |
| tbright | 4 | 5 | 4 | 3 | 6 | 4 |
| detune | 12 | 18 | 6 | 8 | 10 | 12 |
| oct_db | -30 | -10 | -24 | -36 | -20 | -36 |
| shep_db | -48 | -48 | -48 | 0 | -48 | -48 |
| shep_rate | 0.8 | 0.8 | 0.8 | 0.9 | 0.8 | 0.8 |
| drone_db | -30 | -14 | -12 | -22 | -22 | -48 |
| drone_f | 55 | 55 | 95 | 45 | 82 | 55 |
| metal_db | -48 | -48 | -48 | -48 | -48 | 0 |
| metal_f | 2600 | 2600 | 2600 | 2600 | 2600 | 2400 |
| gate_depth | 0 | 0.55 | 1.0 | 0 | 0.5 | 0 |
| gate_on | 0.5 | 0.25 | 0 | 0.5 | 0.1 | 0.5 |
| gate_p0 | 0.2 | 0.25 | 0.055 | 0.2 | 0.1 | 0.2 |
| gate_p1 | 0.04 | 0.06 | 0.036 | 0.04 | 0.028 | 0.04 |
| gate_duty | 0.5 | 0.5 | 0.5 | 0.5 | 0.5 | 0.5 |
| gate_sharp | 0 | 0.2 | 1.0 | 0 | 0.1 | 0 |
| gate_drone | 0 | 0 | 1.0 | 0 | 0 | 0 |
| blip_dec | 0.35 | 0.35 | 0.2 | 0.35 | 0.35 | 0.35 |
| verb | 0.3 | 0.3 | 0.08 | 0.45 | 0.22 | 0.35 |
| rt60 | 2.0 | 2.4 | 1.0 | 3.0 | 1.4 | 1.6 |
| width | 0.7 | 0.5 | 0.3 | 0.9 | 0.5 | 0.85 |
| sat | 2 | 5 | 4 | 1.5 | 2 | 1 |
| hp | 180 | 120 | 250 | 140 | 200 | 400 |
| end_ms | 6 | 5 | 4 | 8 | 4 | 4 |

Characters: air = bright noise that grows (the filter moves first, a slight glide, more and more reverb); tension = a supersaw build
(pitch leads 2 octaves, a tremolo speeding up); stutter = the pro stutter riser (stepped tone blips about 2 to 8 kHz on
an accelerating gate over a gated drone); shepard = an endless rise in a big space; rush = noise in front (a resonant
whistle that sweeps from 500 Hz to 15 kHz) over a gliding sine, with a smooth flutter that gets faster; cymbal = a
reverse-cymbal wash of metal partials and bright noise, no tone.

### 5.5 PRO (old pro set, record only)

All 5 start from the `stutter` member above (call it ST) and change:

| member | changes from ST |
|---|---|
| blips | none |
| chop | length 1.0, tmode glide, wave tri, tone_db -14, noise_db 0, nf0 700, nf1 14000, nq 2.0, gate_p0 0.04, gate_p1 0.02, gate_duty 0.45, drone_db -20, hp 500, rise_db 26, curve 2.2, verb 0.05 |
| gated_saw | length 1.4, tmode glide, wave saw, tf0 300, toct 2.0, tbright 3.0, detune 14, noise_db -12, gate_p0 0.06, gate_p1 0.022, gate_duty 0.6, drone_db -14, drone_f 60, hp 150, sat 3, rise_db 26, curve 2.2, verb 0.06 |
| roll | length 1.3, curve 2.4, tmode glide, tone_db -36, oct_db -36, noise_db 0, nf0 1200, nf1 12000, nq 0.9, pink 0.6, gate_p0 0.085, gate_p1 0.022, gate_duty 0.4, gate_sharp 1.0, drone_db -24, gate_drone 1.0, hp 300, verb 0.1, rise_db 28 |
| blips_hi | length 0.8, tf0 2600, toct 2.2, gate_p0 0.045, gate_p1 0.028, drone_db -18, drone_f 110, noise_db -14, rise_db 20 |

### 5.6 FAMILIES_BY_CAT['riser']

Each member = `FAMILIES[name]`, then the round-2 tuned values below, then the round-3 fix.

Round-2 tuned values (results of a forge search, kept as constants):

| key | air | tension | shepard | rush | cymbal |
|---|---|---|---|---|---|
| length | 2.47 | 2.816 | 3.708 | 1.756 | 2.531 |
| curve | 2.043 | 2.681 | 2.026 | 3.117 | 2.469 |
| rise_db | 33.76 | 35.24 | 30.53 | 32.74 | 41.5 |
| noise_db | -2.34 | -9.59 | -8.98 | -4.32 | -3.21 |
| nf0 | 294.7 | 214.0 | 206.5 | 468.2 | 730.3 |
| nf1 | 10379 | 7710.6 | 8656.4 | 12035 | 15030 |
| nq | 1.856 | 1.253 | 2.572 | 2.54 | 0.704 |
| pink | 0.646 | 0.814 | 0.974 | 0.344 | 0.312 |
| tone_db | -22.34 | -3.34 | -35.18 | -8.74 | -33.79 |
| tmode | glide | glide | glide | glide | glide |
| wave | tri | saw | sine | sine | tri |
| tf0 | 440.8 | 76.65 | 260.0 | 397.2 | 542.9 |
| toct | 1.225 | 1.767 | 1.046 | 2.516 | 1.24 |
| tbright | 3.63 | 6.06 | 2.34 | 5.63 | 3.7 |
| detune | 13.64 | 20.45 | 9.42 | 9.84 | 11.5 |
| oct_db | -29.46 | -10.69 | -32.85 | -19.99 | -32.17 |
| shep_db | -43.4 | -45.8 | -4.73 | -42.3 | -44.76 |
| shep_rate | 0.778 | 0.765 | 0.887 | 0.889 | 0.725 |
| drone_db | -32.06 | -16.86 | -22.49 | -24.66 | -46.7 |
| drone_f | 50.92 | 53.29 | 51.44 | 76.65 | 61.19 |
| metal_db | -46.76 | -45.95 | -44.93 | -43.25 | -4.46 |
| metal_f | 2615.8 | 2641.2 | 2843.6 | 2909.5 | 2153.3 |
| gate_depth | 0.039 | 0.558 | 0.048 | 0.497 | 0.035 |
| gate_on | 0.584 | 0.264 | 0.578 | 0.074 | 0.514 |
| gate_p0 | 0.173 | 0.254 | 0.173 | 0.146 | 0.209 |
| gate_p1 | 0.041 | 0.058 | 0.049 | 0.026 | 0.046 |
| gate_duty | 0.506 | 0.493 | 0.483 | 0.458 | 0.508 |
| gate_sharp | 0.058 | 0.213 | 0.018 | 0.188 | 0.035 |
| gate_drone | 0.103 | 0.052 | 0.098 | 0.092 | 0.05 |
| blip_dec | 0.35 | 0.35 | 0.35 | 0.364 | 0.35 |
| verb | 0.259 | 0.286 | 0.415 | 0.292 | 0.396 |
| rt60 | 1.765 | 2.495 | 2.889 | 1.687 | 1.688 |
| width | 0.586 | 0.522 | 0.909 | 0.565 | 0.96 |
| sat | 0.861 | 5.94 | 0.291 | 3.2 | 0.901 |
| hp | 193.6 | 109.6 | 108.6 | 171.7 | 403.8 |
| end_ms | 5.46 | 5.56 | 10.47 | 3.32 | 4.21 |

Round-3 fix (no steady line may last for seconds: the drone glides or becomes a rumble, the cymbal's metal glides):

| member | fix |
|---|---|
| air | drone_oct 0.9, drone_noise 0.7 |
| tension | drone_oct 1.0, drone_noise 0.5, drone_db -20 |
| shepard | drone_oct 0.7, drone_noise 0.8 |
| rush | drone_oct 0.6, drone_noise 0.3, drone_db -28 |
| cymbal | metal_n 9, metal_glide 0.6, metal_db -6, drone_db -48, width 0.75 |

### 5.7 FAMILIES_BY_CAT['pro_riser'] (noise-led stutter builds)

| member | built as |
|---|---|
| nblips | ST + length 1.0, tmode nblips, nb_q 7, tf0 1700, toct 2.4, tone_db 0, noise_db -10, nf0 900, nf1 13000, nq 1.2, gate_p0 0.05, gate_p1 0.024, gate_duty 0.5, blip_dec 0.25, drone_db -16, drone_noise 1.0, drone_oct 0.3, gate_drone 1.0, oct_db -20, rise_db 20, curve 1.8, verb 0.03, sat 2, metal_n 9, metal_glide 0 |
| chop | PRO chop + tone_db -24, drone_noise 1.0, drone_oct 0.3, nq 2.4, gate_p0 0.045, gate_p1 0.02, sat 2, drone_db -18, metal_n 9, metal_glide 0, nb_q 6 |
| roll | PRO roll + drone_noise 1.0, drone_oct 0.3, metal_n 9, metal_glide 0, nb_q 6 |
| air_gate | ST + length 1.2, tmode glide, wave saw, tf0 400, toct 2.0, tbright 2.0, tone_db -16, noise_db 0, nf0 500, nf1 12000, nq 3.0, pink 0.5, gate_depth 0.85, gate_on 0.1, gate_p0 0.07, gate_p1 0.022, gate_duty 0.55, gate_sharp 0.6, drone_db -18, drone_noise 1.0, drone_oct 0.5, gate_drone 0.8, hp 200, sat 2, rise_db 26, curve 2.2, verb 0.12, width 0.4, metal_n 9, metal_glide 0, nb_q 6 |
| nblips_hi | ST + length 0.85, tmode nblips, nb_q 10, tf0 2600, toct 2.1, tone_db 0, noise_db -12, nf0 1500, nf1 14000, gate_p0 0.045, gate_p1 0.026, blip_dec 0.22, drone_db -20, drone_f 110, drone_noise 1.0, drone_oct 0.3, gate_drone 1.0, rise_db 20, verb 0.03, sat 2, metal_n 9, metal_glide 0 |

### 5.8 R4_EDITS['riser']

Applied, in order, on top of the 5 delivered round-3 `riser` members (air, tension, shepard, rush, cymbal). The intent:
remove every "engine" cue (a low rumble that revs up, a low saw under drive, a resonant low-pass spool-up, metal
partials gliding together), keep every curve concave.

1. air: drone_db -48, hp 220, nq 1.2, curve 2.6
2. tension: drone_db -48, tf0 150, toct 1.8, hp 200, sat 3, detune 24, noise_db -5, curve 2.2
3. shepard: drone_db -48, hp 180, curve 2.4
4. rush: drone_db -48, hp 220, nq 1.6
5. cymbal: drone_db -48, metal_glide 0.2

### 5.9 The "build" render (round 6)

A parameter set with r6 > 0.5 renders with `render_build`. The build leaves out the drone, the sub, the metallic partials and the
saturated low layer: what reads as a jet engine (a rumble that opens, no rhythm, partials gliding together, no stop) is
removed. It has: a noise band that moves up, a unison chord rising about 2 octaves, an accelerating gate on everything,
an optional reversed reverb swell, and a hard end on the peak.

`SPACE_R6` (extra keys, not in `SPACE`, so a normal search does not fill them):

| key | range | meaning |
|---|---|---|
| r6 | 0 to 1 | 1 = the build render |
| nbw | 0.8 to 4 | width of the noise band in octaves (a moving high-pass at fc / 2^nbw) |
| fifth_db | -48 to 0 | the fifth (× 1.5) of the chord, relative to the root |
| tone_gate | 0 to 1 | gate depth applied to the tonal layer (the noise always gets the full gate) |
| gate_pow | 0.6 to 3 | how late the gate accelerates |
| rswell_db | -48 to 0 | reverse reverb swell level |
| rswell_rt | 1 to 4 | RT60 of that reverb (s) |
| pre_dip_ms | 0 to 60 | length of a short level dip before the final open pulse; 0 = off |

`R6_OFF` = r6 0, nbw 2.0, fifth_db -48, tone_gate 1.0, gate_pow 1.4, rswell_db -48, rswell_rt 2.0, pre_dip_ms 0.

Build base B6 = `FAMILIES['air']`, then metal_db -48, metal_f 2600, blip_dec 0.35, drone_oct 0, drone_noise 0,
metal_n 9, metal_glide 0, nb_q 6, drone_db -48, drone_f 55, shep_db -48, gate_drone 0, tmode glide, sat 1.0, nq 0.7,
then `R6_OFF`, then r6 1.0.

`R6` members (B6 plus):

| key | air | tension | shepard | rush | swell |
|---|---|---|---|---|---|
| length | 2.4 | 2.8 | 3.4 | 1.3 | 2.2 |
| curve | 2.6 | 2.4 | 2.2 | 2.2 | 2.8 |
| rise_db | 34 | 30 | 30 | 26 | 38 |
| noise_db | 0 | -9 | -8 | -3 | -8 |
| nf0 | 900 | 1200 | 700 | 1500 | 2000 |
| nf1 | 13000 | 11000 | 9000 | 15000 | 16000 |
| nbw | 2.2 | 1.8 | 2.6 | 1.6 | 1.4 |
| pink | 0.5 | 0.6 | 0.9 | 0.35 | 0.3 |
| tone_db | -16 | 0 | -36 | -4 | -22 |
| fifth_db | -8 | -4 | -48 | -6 | -6 |
| wave | tri | saw | (B6: tri) | sine | sine |
| tf0 | 700 | 300 | (B6: 400) | 1000 | 1500 |
| toct | 2.0 | 2.0 | (B6: 1.5) | 2.2 | 1.6 |
| tbright | (4) | 4.0 | (4) | (4) | (4) |
| detune | 14 | 22 | (12) | 10 | 8 |
| oct_db | -26 | -12 | -36 | -18 | -30 |
| shep_db | (-48) | (-48) | 0 | (-48) | (-48) |
| shep_rate | (0.8) | (0.8) | 0.9 | (0.8) | (0.8) |
| gate_depth | 0.6 | 0.75 | 0.45 | 0.85 | 0.35 |
| gate_on | 0.15 | 0.1 | 0.2 | 0.0 | 0.5 |
| gate_p0 | 0.25 | 0.17 | 0.5 | 0.06 | 0.12 |
| gate_p1 | 0.034 | 0.025 | 0.07 | 0.021 | 0.03 |
| gate_duty | 0.55 | 0.5 | 0.5 | 0.5 | 0.5 |
| gate_sharp | 0.3 | 0.55 | 0.0 | 0.8 | 0.2 |
| gate_pow | 1.6 | 1.5 | 1.3 | 1.2 | 2.0 |
| tone_gate | 0.6 | 1.0 | 0.8 | 1.0 | 0.5 |
| verb | 0.3 | 0.25 | 0.45 | 0.15 | 0.2 |
| rt60 | 2.0 | 2.0 | 3.0 | 1.2 | 1.6 |
| width | 0.6 | 0.5 | 0.85 | 0.4 | 0.8 |
| hp | 300 | 260 | 260 | 400 | 500 |
| end_ms | 5 | 4 | 8 | 3 | 4 |
| rswell_db | -14 | -48 | -20 | -48 | 0 |
| rswell_rt | 2.2 | (2.0) | 3.0 | (2.0) | 2.6 |

Values in brackets come from B6.

**render_build(p, rng)** (p merged as `DEFAULTS`, then `R6_OFF`, then p):

1. n = samples of length L. w = u^curve. Level lv = 10^(-rise_db (1 - w) / 20).
2. Gate period per sample: per = gate_p0 × (gate_p1 / gate_p0)^(u^gate_pow). Phase laid out from the end (P15) with
   rate 1/per and end phase 0.5 × gate_duty. Gate value g = gate shape (P16) with duty gate_duty and sharp gate_sharp.
3. Depth: on = clip((u - gate_on) / 0.2, 0, 1); fin = clip((L - t) / 0.045, 0, 1); both smoothed by s(x) = x²(3 - 2x).
   depth = gate_depth × s(on) × s(fin). Noise gate = 1 - depth (1 - g). Tone gate = 1 - tone_gate × depth (1 - g).
4. Noise source: (1 - pink) × white + pink × 1.6 × unit-RMS pink (P13). Centre fc = nf0 × (nf1 / nf0)^w.
   Low-pass 24 dB/octave at min(fc, 18000) and high-pass 24 dB/octave at max(fc / 2^nbw, 20), both unresonant
   (P17). Then `bw(4)` high-pass at hp. Stereo: decorrelate fully (P18), then set the width to width × (0.35 + 0.65 w)
   (P19).
5. Chord: wt = u^max(1, 0.8 curve); f = tf0 × 2^(toct × wt). Draw 15 random start phases in [0, 1). Detune
   det = detune × (0.6 + 0.8 w). Root = unison (P20) of f with det, width 0.5 width, phases 1 to 5. If fifth_db > -47,
   add 10^(fifth_db / 20) × unison of 1.5 f with det, width 0.7 width, phases 6 to 10. Octave = unison of 2 f with
   0.7 det, width 0.6 width, phases 11 to 15, times (clip((u - 0.4) / 0.6, 0, 1))².
6. If wave is saw: low-pass root and fifth per channel (`lp`, Q 0.7) at min(f × tbright, 18000), the octave at
   min(2 × that, 18000).
7. High-pass the chord with `bw(2)` at 0.8 hp, the octave at hp.
8. Shepard (only when shep_db > -47): an upward shepard tone (P21) over the length, rate shep_rate, 150 Hz to 9 kHz,
   `bw(2)` high-pass at hp, decorrelated at 0.6, width 0.6 × width.
9. Reverse swell (only when rswell_db > -47): a 30 ms burst of white noise, `bw(2)` high-pass 1500 Hz, times
   exp(-t / 0.008). Its reverb return alone (P22) with rt60 rswell_rt, length rswell_rt + 0.1 s, predelay 0, damp 0.3,
   low cut 400 Hz. Reverse it in time and place it so it ends on the last sample (if it is longer than the file, keep
   its last part). `bw(2)` high-pass at hp, width 0.25 + 0.5 × width.
10. Mix level of a layer (call it AT(db, x)): 10^(db / 20) × x / lvl(x, last 25 %) (P8 on the last quarter only).
11. Send = [AT(noise_db, noise) × noise gate + (AT(tone_db, chord) + AT(oct_db, octave)) × tone gate
    + AT(shep_db, shepard) × tone gate] × lv. Add AT(rswell_db, swell) × (0.25 + 0.75 × noise gate) (the swell carries
    its own reversed curve, so it does not get lv).
12. Reverb (only when verb > 0.005): mix = send + verb × wet / lvl(wet, 25 %) × lvl(send, 25 %), with the wet = the
    reverb return alone of send at rt60, cut to n samples (P22, default size 0.75, predelay 0.02, damp 0.5, low cut 200).
13. Pre-dip (only when pre_dip_ms > 0.5): d0 = L - 0.045 - pre_dip_ms / 1000. Multiply by
    1 - 0.5 exp(-0.5 ((t - d0) / (0.35 pre_dip_ms / 1000))²).
14. If sat > 0.1: tanh saturation (P11) with drive sat dB.
15. `bw(2)` high-pass 120 Hz, `bw(4)` low-pass 18 kHz. Fade in 4 ms, fade out end_ms (P9).

### 5.10 Shared helpers

- `lvl(x, frac=None)`: mono fold (channel mean), keep only the last `frac` of it when frac is given, then the peak of
  the 10 ms block RMS (at least 1 block) plus 1e-12. This is P8 with a tail option.
- `phase_from_end(rate, end_phase)`: P15.
- `gate_shape(phase, period, duty=0.5, sharp=1.0, ramp=0.0012, perc=0.0)`: P16.
- `unison(freq, wave, detune, width, phases)`: P20.
- `verb_ir(rt60, size=0.75, predelay=0.02, damp=0.5, lowcut=200)` and `wet_of(x, rt60, n_out=None, **kw)`: P22.
- `stereo_of(x)`: a mono signal duplicated to 2 equal channels; a stereo signal unchanged.
- `env_db(x, hop_ms=10)`: mono fold, block RMS in dB over whole blocks of hop_ms (at least 1 block), 1e-9 inside the
  log.
- `band_env_db(x, lo, hi, hop_ms=10)`: `bw(4)` band-pass lo to min(hi, 23000) (or a `bw(4)` low-pass at hi when lo ≤ 20),
  then `env_db`.
- `side_db(x)`: 10 log10(Σ side² / Σ mid²) with mid = (L + R)/2, side = (L - R)/2; -99 for mono input.
- `low_share_db(x, f=100)`: 10 log10 of the energy of a `bw(4)` low-pass at f over the total energy (mono fold).
- `centroid_track(x, hop_ms=20)`: mono fold, non-overlapping Hann-windowed frames of hop_ms, power spectrum; returns
  (centroid in Hz per frame, frame power).

### 5.11 render(p, rng) (rounds 1 to 4)

If r6 > 0.5: `render_build`. Else merge p over `DEFAULTS`.

1. n = samples of L; w = u^curve; lv = 10^(-rise_db (1 - w) / 20).
2. Gate: per = gate_p0 × (gate_p1 / gate_p0)^(u^1.4). Phase from the end (P15), end phase 0.5 gate_duty. g = gate
   shape with gate_duty, gate_sharp. on, fin and depth as in 5.9 step 3. gate = 1 - depth (1 - g).
3. Noise: source as 5.9 step 4. `lp` at fc = nf0 × (nf1 / nf0)^w with Q nq (resonant). `bw(4)` high-pass at hp.
   Decorrelate fully, width width × (0.35 + 0.65 w).
4. Tonal pitch: wt = u^max(1, 0.8 curve), f = tf0 × 2^(toct × wt). Draw 10 random start phases in [0, 1).
5. Blip mode is on for tmode blips or nblips:
   - Pulse index = floor(gate phase). Pitch is held per pulse at the value of f at the pulse's first sample: a
     stepped arpeggio.
   - Each blip is a small falling chirp: fb = held × 2^(-0.35 × fractional phase).
   - nblips: 2 independent white noises through `bp` (peak gain Q) at min(fb, 16000) with Q nb_q. With mid = their
     mean, tone = mid + (each - mid) × min(1, 0.5 width + 0.2).
   - blips: tone = unison of fb with detune (constant), width 0.25 width, phases 1 to 5.
   - Multiply by (percussive gate × fin + 1 - fin), where the percussive gate is P16 with sharp 1 and perc blip_dec.
     So the blips are held over the final 45 ms.
   Glide mode: tone = unison of f with detune × (0.6 + 0.8 w), width 0.5 width, phases 1 to 5.
6. Octave layer:
   - nblips: one white noise through `bp` at min(2 fb, 16000) with Q 1.5 nb_q, duplicated to stereo.
   - blips: unison of 2 × held × 2^(-0.35 × fractional phase), detune 0.7 detune, width 0.6 width, phases 6 to 10.
   - glide: unison of 2 f with the same detune, width and phases.
   - Times (clip((u - 0.4) / 0.6, 0, 1))². In blip mode also times (percussive gate with perc 0.7 blip_dec × fin
     + 1 - fin).
7. Saw only: low-pass tone and octave per channel (`lp`, Q 0.8) at min(f × tbright, 18000) and min(2 × that, 18000).
   (The cut follows the glide pitch f, also in blip mode.)
8. `bw(2)` high-pass: tone at 0.8 hp, octave at hp.
9. Shepard (only when shep_db > -47): upward shepard tone (P21), rate shep_rate, 60 Hz to 9 kHz, `bw(2)` high-pass at
   hp, decorrelated 0.6, width 0.6 width. Else a silent stereo signal.
10. Drone: df = drone_f × 2^(drone_oct × w). 3 band-limited saws (P23) at df × 2^(c / 1200) for c = -7, 0, +7 cents,
    random phases, averaged. `bw(2)` low-pass 500 Hz, times 0.8, plus 0.35 × a sine at df (random phase).
    If drone_noise > 0.01: a rumble = white noise through `bp` at clip(2.2 df, 60, 2000) with Q 1.2, then `bw(2)`
    low-pass 600 Hz. Drone = (1 - drone_noise) × drone / std(drone) + drone_noise × rumble / std(rumble).
    `bw(2)` high-pass 30 Hz. Times 10^(-0.3 rise_db (1 - w) / 20) (the drone swells only a little).
    Gate on the drone: in blip mode a second gate at twice the rate (phase × 2, period / 2, same duty and sharp),
    else g. Times 1 - gate_drone × depth × (1 - that gate).
11. Metal (only when metal_db > -47):
    - Ratios: the fixed 9 ratios 1.0, 1.37, 1.71, 2.13, 2.58, 3.11, 3.66, 4.29, 5.02. If metal_n > 9: metal_n random
      ratios exp(uniform(0, ln 6)), sorted ("dense").
    - Glide factor 2^(metal_glide × u^1.2) (it moves from the start).
    - Dense: each partial k opens at its own point: u_on = -1 for every 5th partial (k mod 5 = 0, open from the
      start), else uniform(0, 0.75). Opening = clip((w - u_on) / 0.25, 0, 1).
      Fixed: opening = clip(0.9 × w × count - 0.6 k + 1, 0, 1) (higher partials open later).
    - Per partial: fk = metal_f × ratio × (1 + 0.004 × a standard normal draw). Skip it if fk × 2^metal_glide > 19000.
      Shimmer AM: |white noise `bw(2)` low-passed at 220 Hz| divided by its mean.
      With metal_glide > 0.001: sine on the gliding phase 2π Σ fk × glide / SR plus a random phase, times AM × opening,
      divided by 1 + 0.15 × k × 9 / count. Else a fixed sine at fk, divided by 1 + 0.15 k.
    - Sum, `bw(2)` high-pass 1200 Hz, decorrelate fully, width = width.
12. Mix with AT() of 5.9 step 10 (level on the last 25 %): N = AT(noise_db, noise), T = AT(tone_db, tone),
    O = AT(oct_db, octave), S = AT(shep_db, shepard) when shep_db > -47 else 0, plus AT(metal_db, metal) when on.
    Blip mode: send = ((N + S) × gate + T + O) × lv (the blips carry their own gate). Else send = (N + T + O + S) × lv
    × gate.
13. mix = send + AT(drone_db, stereo drone) × lv^0.3. (The drone therefore gets lv^0.3 twice in total, from step 10 and
    here. This is the source behaviour.)
14. Reverb as 5.9 step 12. Saturation as 5.9 step 14.
15. `bw(2)` high-pass 30 Hz, `bw(4)` low-pass 18 kHz, fade in 4 ms and out end_ms.

### 5.12 measure and target_check

`measure(x)` (model-free riser targets). e = `env_db(x)` (10 ms), k = its length, pk = max(e).

| field | definition | pro riser value |
|---|---|---|
| length_s | samples / SR, 3 decimals | 0.8 to 4 |
| last_third | (b - two) / max(b - a, 0.001); a = median of the first k/10 blocks, b = max of the last k/10, two = 5-point moving average of e at block int(2k/3) | ≥ 0.5 (concave) |
| end_db | max of the last 5 blocks - pk | about 0 (the level is at its maximum at the end) |
| hf_rise_db | `band_env_db(x, 4000, 20000)`: max of the last k/10 blocks - median of the first k/4 | +30 to +35 |
| cent_oct | log2(median centroid of the last 3 loud frames / max(median of the first 3, 1)); loud = power > 1e-4 × max; 0 when fewer than 6 loud frames | positive |
| side_db | `side_db(x)` | -14 to -18 |
| low_db | `low_share_db(x)` (under 100 Hz) | low |

(All "k/10" counts use at least 1 block.)

`target_check(x)`: a penalty, returned as -min(3, penalty):
- + 4 × max(0, 0.45 - last_third)
- + max(0, -3 - end_db) / 4
- + max(0, 20 - hf_rise_db) / 10
- + max(0, low_db + 10) / 6
- + max(0, side_db + 6) / 6

## 6. downlifter

### 6.1 The sound

A designed downward sweep for the moment directly after an impact; the riser in reverse. The curves slow down:
v = 1 - (1 - u)^curve (fast at first, then settling). A stereo reverb of the full fall continues after the dry layers
have faded, so the sound becomes more reverberant as it goes down. The start fades in over att_ms (2 to 8 ms sounds
like a hit, 40 to 200 ms like a sweep). The end decays to silence; it is never cut.

Target "a downlifter sound effect: a sinking filtered sweep". Alternatives: "a jet engine", "an air raid siren", "a
plane flying overhead", "a gust of wind". Refset `downlifter` (it has no references, so the forge uses designed mode with
`target_check`).

### 6.2 Interface

`SPACE`, `FAMILIES` (name to full dict), `DEFAULTS` = copy of `FAMILIES['air_fall']`, `comb(x, delay_s, mix)`,
`measure(x)`, `target_check(x, row=None)`, `render(p, rng)`. It imports `centroid_track`, `env_db`, `low_share_db`,
`lvl`, `side_db`, `stereo_of`, `unison`, `wet_of` from the riser.

### 6.3 SPACE

| key | range | scale | meaning |
|---|---|---|---|
| length | 1 to 4 | | s |
| curve | 1 to 4 | | deceleration (1 straight, 3 a fast fall with a long sag) |
| drop_db | 24 to 50 | | total drop of the dry level from start to end (dB) |
| att_ms | 2 to 300 | log | swell-in at the start |
| noise_db | -36 to 0 | | |
| nf0 | 5000 to 16000 | log | noise low-pass at the start |
| nf1 | 120 to 800 | log | and at the end |
| nq | 0.7 to 4 | log | |
| pink | 0 to 1 | | |
| tone_db | -36 to 0 | | |
| tmode | fall, tape | choice | |
| wave | sine, tri, saw, square | choice | |
| tf0 | 150 to 2500 | log | tonal start pitch |
| toct | 0.7 to 3 | | octaves of fall |
| tbright | 1.5 to 10 | log | low-pass = pitch × tbright (saw, square) |
| detune | 0 to 30 | | cents |
| tape_curve | 0.7 to 2.5 | | tape-stop speed exponent |
| sub_db | -48 to 0 | | |
| sub_f0 | 60 to 140 | log | |
| sub_f1 | 28 to 50 | log | |
| shep_db | -48 to 0 | | |
| shep_rate | 0.3 to 1.6 | | |
| flange | 0 to 0.8 | | comb mix |
| fl_rate | 0.3 to 4 | log | initial comb sweep rate (Hz); it falls to about a third by the end |
| verb | 0 to 0.7 | | |
| rt60 | 0.8 to 4 | log | |
| width | 0 to 1 | | |
| sat | 0 to 10 | | tanh drive (dB) |
| hp | 40 to 300 | log | |

### 6.4 FAMILIES

| key | air_fall | synth_fall | sub_sink | tape | shepard_fall | flange_air |
|---|---|---|---|---|---|---|
| length | 2.2 | 2.8 | 1.6 | 1.3 | 3.4 | 3.0 |
| curve | 2.2 | 2.0 | 2.6 | 1.6 | 1.5 | 1.8 |
| drop_db | 40 | 36 | 38 | 30 | 34 | 38 |
| att_ms | 90 | 120 | 45 | 40 | 200 | 150 |
| noise_db | 0 | -30 | -16 | -20 | -10 | 0 |
| nf0 | 14000 | 6000 | 6000 | 9000 | 9000 | 12000 |
| nf1 | 250 | 300 | 150 | 400 | 300 | 400 |
| nq | 1.4 | 1.2 | 1.0 | 1.0 | 2.0 | 0.9 |
| pink | 0.45 | 0.7 | 0.9 | 0.5 | 0.8 | 0.6 |
| tone_db | -36 | 0 | 0 | 0 | -36 | -24 |
| tmode | fall | fall | fall | tape | fall | fall |
| wave | tri | saw | square | saw | sine | tri |
| tf0 | 800 | 880 | 400 | 330 | 800 | 1200 |
| toct | 1.5 | 2.0 | 2.5 | 1.0 | 1.0 | 2.0 |
| tbright | 4 | 4 | 2.5 | 5 | 3 | 4 |
| detune | 10 | 16 | 6 | 10 | 8 | 10 |
| tape_curve | 1.4 | 1.4 | 1.4 | 1.5 | 1.4 | 1.4 |
| sub_db | -48 | -30 | -6 | -24 | -36 | -40 |
| sub_f0 | 90 | 90 | 85 | 82 | 80 | 90 |
| sub_f1 | 38 | 38 | 34 | 36 | 40 | 38 |
| shep_db | -48 | -48 | -48 | -48 | 0 | -48 |
| shep_rate | 0.8 | 0.8 | 0.8 | 0.8 | 0.9 | 0.8 |
| flange | 0 | 0.35 | 0 | 0 | 0 | 0.7 |
| fl_rate | 1.0 | 1.2 | 1.0 | 1.0 | 1.0 | 0.8 |
| verb | 0.3 | 0.35 | 0.2 | 0.15 | 0.45 | 0.3 |
| rt60 | 1.8 | 2.4 | 1.4 | 1.2 | 3.0 | 2.2 |
| width | 0.8 | 0.6 | 0.3 | 0.4 | 0.85 | 0.7 |
| sat | 1 | 3 | 4 | 3 | 1 | 1 |
| hp | 80 | 60 | 40 | 50 | 70 | 90 |

Characters: air_fall = white noise sweeping down (the low-pass moves from 14 kHz to 250 Hz; wide); synth_fall = a supersaw
falling 2 octaves with a slow flanger (its noise stays at -30 dB: louder noise under saw partials reads as aliasing to
the polish check); sub_sink = a sine-square tone sinking over a sub drop, short and dark; tape = a sustained chord that
slows down until it stops; shepard_fall = an endless fall in a big space; flange_air = noise through a deep flanger
whose sweep slows during the fall.

### 6.5 render(p, rng)

Merge p over `DEFAULTS`.

1. n = samples of L; v = 1 - (1 - u)^curve.
2. Attack: att = sin²(0.5π × clip(t / (att_ms / 1000), 0, 1)).
3. Dry level: lv = att × 10^(-drop_db × v / 20) × (clip((1 - u) / 0.12, 0, 1))^1.5. The dry part reaches silence on
   the last sample.
4. Noise: source (1 - pink) × white + pink × 1.6 × unit-RMS pink. fc = nf0 × (nf1 / nf0)^v. `lp` at fc with Q nq,
   `bw(2)` high-pass at hp, `bw(4)` low-pass at 17 kHz.
5. Flanger (only when flange > 0.01): rate = fl_rate × (1 - 0.66 v); phase = Σ rate / SR; delay
   d = 0.0004 + 0.0086 × (0.5 - 0.5 cos 2π phase) s (0.4 to 9 ms). Apply `comb(noise, d, flange)`:
   y = (x + flange × x(t - d(t))) / (1 + flange), the delayed read linearly interpolated, 0 before the start.
6. Stereo noise: decorrelate fully, width = width.
7. Tonal, with 5 random start phases:
   - tape: a chord of 3 unisons (P20) at constant pitches tf0 × 1.0, 1.5, 2.0 with gains 1.0, 0.6, 0.5 (detune,
     width 0.5 width), in a buffer 50 ms longer than n (the extra is silent). Speed sp = max((1 - u)^tape_curve, 0).
     Read position pos = 0 then the running sum of sp (pos[i] = Σ_{j<i} sp[j]). Tone = the chord read at pos with
     linear interpolation, per channel. Pitch track ftrack = tf0 × max(sp, 0.02).
   - fall: ftrack = tf0 × 2^(-toct × v). Tone = unison of ftrack (detune, width 0.5 width).
   - saw or square: low-pass per channel (`lp`, Q 0.9) at clip(ftrack × tbright, 200, 18000).
   - `bw(2)` high-pass at hp.
8. Sub: fs = sub_f0 × (sub_f1 / sub_f0)^(1 - (1 - u)^2.2). A band-limited sine at fs (phase 0) times att ×
   exp(-u / 0.22) × (clip((1 - u) / 0.2, 0, 1))². Add 0.15 tanh(3 × sub). `bw(2)` high-pass 25 Hz. Mono.
9. Shepard (only when shep_db > -47): a downward shepard tone (P21), rate shep_rate, 60 Hz to 9 kHz, `bw(2)` high-pass
   at hp, then `lp` (Q 0.7) at min(1.3 fc, 18000) (it darkens with the noise), decorrelated 0.6, width 0.6 width.
10. Layer level AT(db, x) = 10^(db / 20) × x / lvl(x) (P8 on the whole layer).
11. dry = (AT(noise_db, noise) + AT(tone_db, tone) [+ AT(shep_db, shepard)]) × lv.
    sub layer = AT(sub_db, stereo sub) when sub_db > -47, else 0 (dry: no reverb, not multiplied by lv).
12. Tail room: tail = samples of min(1.5, 0.6 rt60). Output buffer n + tail. Put dry + sub in the first n samples.
13. Reverb (only when verb > 0.005): add verb × wet / lvl(wet) × lvl(dry), wet = the return alone (P22) of dry, rt60,
    n + tail samples, size 0.8, predelay 0.015, low cut 150 (damp default 0.5).
14. sat > 0.1: tanh saturation. `bw(2)` high-pass 25 Hz.
15. Trim the tail at -62 dB keeping 0.02 s (P14). Fade in 1.5 ms, fade out min(0.12, 0.25 × duration) s.

### 6.6 measure and target_check

`measure(x)`: e = `env_db(x)` (10 ms), k = blocks, ip = argmax, pk = e[ip]. Active blocks: e > pk - 45; end = the last
active block (else k - 1). third = ip + max(1, (end - ip) // 3). sm = 5-point moving average of e.
fall = max(pk - sm[end], 0.001).

| field | definition | target |
|---|---|---|
| length_s | duration | 1 to 4 |
| peak_frac | ip / k | early (< 0.15) |
| sag | (pk - sm[min(third, k - 1)]) / fall | decelerating (> 0.5) |
| cent_oct | as the riser's but loud = power > 1e-3 × max | < -1 |
| tail_db | 20 log10(RMS of the last 480 mono samples + 1e-12) - pk | < -45 |
| side_db | `side_db(x)` | |
| low_db | `low_share_db(x)` | |

`target_check(x)`: -min(3, penalty), penalty =
6 × max(0, peak_frac - 0.15) + (max(0, -1 + cent_oct) when cent_oct > -1, else 0) + 3 × max(0, 0.3 - sag)
+ max(0, tail_db + 45) / 8 + max(0, side_db + 4) / 6.

## 7. zip

### 7.1 The sound

A designed tonal zip: a single fast exponential pitch sweep (upward, downward or an arc), 0.06 to 0.3 s long. On a
logarithmic frequency axis the glide is a straight line (a constant number of octaves per second). Measured pro zips: about +20 oct/s (100 Hz to 1.5 kHz in
0.2 s), +30 oct/s (120 Hz to 10 kHz in 0.22 to 0.25 s), -60 oct/s (7.2 kHz to 420 Hz in about 60 ms), and an arc
1 kHz to 3 kHz to 1 kHz over 0.25 s. Timbre: a sine plus harmonics 2, 3, 5 (each fades before it would pass 18 kHz),
optional FM sheen, a filtered saw body, a thin air band that follows the pitch, a wide noise band and zipper "teeth"
(AM roughness) so it does not read as a toy "boing". Envelope: a raised-cosine attack (20 to 35 ms in pro
zips), a level slope over the sweep, and a short release that ends on the cut. Near mono (side 15 to 25 dB under mid).

Target "a quick tonal zip, a fast synth pitch sweep". Alternatives: "a bird chirp", "a squeaky door hinge", "a car
horn", "a phone ringing". Refset `pro_zip` (no forge refs, designed mode with `target_check`).

### 7.2 Interface

`SPACE`, `FAMILIES` (name to full dict), `FAMILIES_BY_CAT = {'pro_zip': {...}}`, `DEFAULTS` = copy of
`FAMILIES['up']`, `trajectory(p, n) -> pitch per sample`, `ridge(x, hop_ms=2.5, win=1024)`, `measure(x)`,
`target_check(x, row=None)`, `render(p, rng)`.

### 7.3 SPACE

| key | range | scale | meaning |
|---|---|---|---|
| shape | up, down, arc | choice | |
| length | 0.06 to 0.3 | | s |
| f0 | 80 to 8000 | log | start pitch |
| f1 | 80 to 12000 | log | end pitch (up, down) |
| fpk | 500 to 6000 | log | the arc's peak |
| pk_at | 0.3 to 0.7 | | where the arc peaks |
| arc_back | 0.5 to 1 | | the arc ends at f0 × arc_back |
| bend | -0.5 to 0.6 | | time warp: u^(2^bend) |
| h2_db | -40 to -6 | | |
| h3_db | -40 to -6 | | |
| h5_db | -48 to -12 | | |
| sub_db | -48 to -6 | | an octave below |
| fm | 0 to 1.5 | | FM index |
| fm_ratio | 1.0, 2.0 | choice | |
| saw_db | -48 to 0 | | filtered saw body |
| saw_lp | 1.5 to 6 | log | its low-pass = pitch × saw_lp |
| air_db | -40 to -10 | | level of a noise band that tracks the pitch |
| air_q | 2 to 8 | log | |
| detune | 0 to 15 | | cents |
| att_ms | 3 to 40 | log | |
| rel_ms | 4 to 40 | log | |
| tilt_db | -9 to 9 | | level difference from the start to the end of the sweep (dB) |
| verb | 0 to 0.3 | | |
| width | 0 to 0.35 | | |
| sat | 0 to 9 | | tanh drive (dB) |
| noise_db | -60 to -4 | | wide noise band; -60 = none |
| noise_ratio | 0.7 to 3 | log | its centre = pitch × noise_ratio |
| noise_q | 0.5 to 2 | log | |
| teeth | 0 to 0.7 | | zipper AM depth |
| teeth_hz | 40 to 260 | log | its rate at the mean pitch |

### 7.4 FAMILIES

All members have noise_db -60, noise_ratio 1.5, noise_q 1.0, teeth 0.0, teeth_hz 120.

| key | up | up_hi | down | arc | down_warm |
|---|---|---|---|---|---|
| shape | up | up | down | arc | down |
| length | 0.19 | 0.24 | 0.075 | 0.25 | 0.2 |
| f0 | 100 | 130 | 7200 | 1000 | 2400 |
| f1 | 1600 | 10000 | 420 | 1000 | 160 |
| fpk | 2000 | 2000 | 2000 | 3000 | 2000 |
| pk_at | 0.5 | 0.5 | 0.5 | 0.45 | 0.5 |
| arc_back | 1.0 | 1.0 | 1.0 | 0.9 | 1.0 |
| bend | 0.05 | 0.25 | -0.2 | 0.0 | -0.3 |
| h2_db | -14 | -16 | -18 | -16 | -10 |
| h3_db | -24 | -24 | -26 | -22 | -16 |
| h5_db | -30 | -36 | -40 | -36 | -28 |
| sub_db | -48 | -48 | -48 | -48 | -20 |
| fm | 0.3 | 0.15 | 0.2 | 0.25 | 0.5 |
| fm_ratio | 1 | 2 | 1 | 1 | 1 |
| saw_db | -6 | -10 | -8 | -14 | -2 |
| saw_lp | 3.0 | 2.5 | 3.5 | 2.0 | 2.2 |
| air_db | -28 | -24 | -26 | -30 | -32 |
| air_q | 4 | 5 | 4 | 5 | 3 |
| detune | 6 | 7 | 3 | 5 | 8 |
| att_ms | 28 | 30 | 6 | 22 | 10 |
| rel_ms | 10 | 6 | 12 | 18 | 22 |
| tilt_db | 6 | 4 | -6 | -3 | -8 |
| verb | 0.06 | 0.05 | 0.08 | 0.07 | 0.1 |
| width | 0.12 | 0.1 | 0.1 | 0.12 | 0.15 |
| sat | 3 | 2 | 2 | 2 | 4 |

### 7.5 FAMILIES_BY_CAT['pro_zip']

Each of the 5 = the FAMILIES member, then its round-2 tuned values, then its round-3 air values. Plus a 6th member.

Round-2 tuned values:

| key | up | up_hi | down | arc | down_warm |
|---|---|---|---|---|---|
| length | 0.178 | 0.236 | 0.103 | 0.277 | 0.187 |
| f0 | 126.06 | 176.5 | 6706.8 | 623.3 | 2505.5 |
| f1 | 1707.5 | 8869.0 | 477.6 | 1000.0 | 149.4 |
| fpk | 2405.7 | 1800.9 | 2160.3 | 3470.1 | 2586.2 |
| pk_at | 0.506 | 0.521 | 0.486 | 0.419 | 0.456 |
| arc_back | 0.957 | 0.969 | 0.948 | 0.887 | 0.984 |
| bend | 0.027 | 0.382 | -0.15 | -0.115 | -0.27 |
| h2_db | -11.4 | -13.75 | -17.95 | -14.16 | -13.85 |
| h3_db | -23.0 | -21.3 | -27.9 | -25.4 | -14.2 |
| h5_db | -26.0 | -38.1 | -40.3 | -36.9 | -32.2 |
| sub_db | -46.1 | -47.2 | -43.7 | -43.6 | -17.4 |
| fm | 0.319 | 0.066 | 0.362 | 0.24 | 0.505 |
| fm_ratio | 1 | 2 | 1 | 1 | 1 |
| saw_db | -1.99 | -9.86 | -7.4 | -10.94 | -7.25 |
| saw_lp | 2.67 | 2.26 | 3.29 | 2.3 | 2.47 |
| air_db | -28.7 | -22.0 | -25.5 | -32.7 | -35.1 |
| air_q | 4.59 | 5.65 | 3.82 | 4.34 | 2.85 |
| detune | 4.36 | 6.34 | 2.62 | 3.45 | 7.75 |
| att_ms | 34.2 | 29.8 | 7.96 | 27.7 | 13.4 |
| rel_ms | 9.54 | 6.32 | 10.07 | 19.4 | 22.8 |
| tilt_db | 7.43 | 4.96 | -5.4 | -3.0 | -8.2 |
| verb | 0.025 | 0.025 | 0.051 | 0.046 | 0.081 |
| width | 0.109 | 0.082 | 0.127 | 0.135 | 0.183 |
| sat | 2.09 | 2.5 | 2.67 | 1.61 | 3.41 |

Round-3 air values:

| member | noise_db | noise_ratio | noise_q | teeth | teeth_hz |
|---|---|---|---|---|---|
| up | -8 | 1.6 | 0.9 | 0.3 | 90 |
| up_hi | -10 | 1.3 | 0.8 | 0.2 | 120 |
| down | -12 | 1.4 | 0.9 | 0.15 | 150 |
| arc | -8 | 1.6 | 0.9 | 0.35 | 70 |
| down_warm | -12 | 2.2 | 0.8 | 0.3 | 60 |

6th member `air_up` (the noise leads, the tone under it): FAMILIES up, then the round-2 `up` values, then length 0.2,
f0 300, f1 4500, h2_db -20, h3_db -30, saw_db -12, noise_db -4, noise_ratio 1.2, noise_q 1.4, teeth 0.35,
teeth_hz 110, att_ms 25, tilt_db 6.

### 7.6 trajectory(p, n)

u = n points evenly from 0 to 1 inclusive. uw = u^(2^bend).
- arc: with a = pk_at, up = clip(u / a, 0, 1), dn = clip((u - a) / (1 - a), 0, 1). In log2 frequency: for u ≤ a,
  log2 f0 + (log2 fpk - log2 f0) × sin(0.5π up); after, log2 fpk + (log2(f0 × arc_back) - log2 fpk) × (1 - cos(0.5π dn)).
  (The arc does not use bend.)
- up: if f1 < 1.5 f0, use f1 = 4 f0. down: if f1 > f0 / 1.5, use f1 = f0 / 4. Pitch = f0 × (f1 / f0)^uw.

### 7.7 render(p, rng)

Merge p over `DEFAULTS`.

1. n = samples of L. f = trajectory.
2. 3 voices: (cents 0, pan 0), (-detune, -1), (+detune, +1). Per voice:
   - fk = f × 2^(cents / 1200); phase φ = 2π Σ fk / SR + a random start.
   - FM index fmi = fm × clip((14000 - fk (1 + 2 fm_ratio)) / 4000, 0, 1); modulation m = fmi sin(fm_ratio φ) when
     fm > 0.01, else 0.
   - v = sin(φ + m) + Σ for (h, db) in (2, h2_db), (3, h3_db), (5, h5_db) of 10^(db / 20) × clip((18000 - h fk) / 3000,
     0, 1) × sin(h φ + 0.5 h m).
   - If sub_db > -47: add 10^(sub_db / 20) sin(0.5 φ).
   - Gain 1 for the centre voice, 0.5 × min(1, detune / 4) for the others. Constant-power pan θ = (clip(pan × width,
     -1, 1) + 1) π / 4: L += gain v cos θ √2, R += gain v sin θ √2.
3. Saw body (only when saw_db > -47): 3 band-limited saws (P23) at f × 2^(c / 1200) for c = -detune, 0, +detune, random
   phases, summed. `lp` (Q 1.1) at clip(f × saw_lp, 150, 16000), then `lp` (Q 0.7) at clip(1.05 f × saw_lp, 150, 16000),
   then `bw(6)` low-pass 15 kHz. out = out / lvl(out) + 10^(saw_db / 20) × stereo saw / lvl(saw).
4. tone = out / lvl(out).
5. Air: white noise through `bpn` at clip(1.5 f, 200, 16000) with Q air_q; stereo duplicate / lvl. y = tone +
   10^(air_db / 20) × air.
6. Wide noise (only when noise_db > -59): white noise through `bp` (peak gain Q) at clip(f × noise_ratio, 250, 15000)
   with Q noise_q, `bw(4)` low-pass 16 kHz; add 10^(noise_db / 20) × stereo / lvl.
7. Envelope: a = clip(t / (att_ms / 1000), 0, 1); rl = clip((L - t) / (rel_ms / 1000), 0, 1);
   env = (0.5 - 0.5 cos πa)(0.5 - 0.5 cos π rl) × 10^(tilt_db × (t / L) / 20).
8. Teeth (only when teeth > 0.01): rate = teeth_hz × √(f / geometric mean of f); env × (1 - teeth × (0.5 - 0.5 cos(2π Σ
   rate / SR))).
9. y × env. `bw(2)` high-pass 40 Hz.
10. sat > 0.1: tanh saturation, then `bw(4)` low-pass 18 kHz.
11. Room (only when verb > 0.005): tail 0.3 s. wet = return alone (P22) of y with rt60 0.3, n + tail samples, size 0.3,
    predelay 0.004, low cut 300. y = (y padded with the tail) + verb × wet / lvl(wet) × lvl(y). Trim the tail at -60 dB
    keeping 4 ms. Fade out min(0.02, 0.1 × duration).
12. Fade in 0.5 ms, out 2 ms.
13. Remove DC without touching the edges: with a Hann window h of the output length, y = y - (mean(y) / mean(h)) × h,
    per channel.

### 7.8 ridge, measure, target_check

`ridge(x, hop_ms=2.5, win=1024)`: mono fold, zero pad win/2 on both sides, Hann frames of 1024 at a 120-sample hop;
the frequency of the strongest bin (ignoring bins 0 and 1) per frame, and the frame level (dB of the max bin) relative
to the loudest frame.

`measure(x)`:

| field | definition |
|---|---|
| length_s | duration |
| attack_ms | 5 × index of the loudest 5 ms block |
| active_s | (last - first + 1) × 0.005 over blocks within 30 dB of the loudest |
| end_db | last 5 ms block - loudest |
| side_db | `side_db(x)` |
| oct_span | range of log2(ridge) over ridge frames within 20 dB of the loudest (0 when fewer than 4) |
| oct_per_s | least-squares slope of log2(ridge) against time over those frames. If both halves have at least 3 frames, their slopes have opposite signs and both exceed 3 oct/s in size: use the steeper half and set arc = true |
| arc | boolean |

`target_check(x)`: r = |oct_per_s|. (lo, span) = (8, 1.0) for an arc, else (12, 1.5). penalty = max(0, lo - r) / 6
+ max(0, r - 75) / 20 + 1.5 × max(0, span - oct_span) + 10 × max(0, active_s - 0.33) + max(0, side_db + 10) / 6.
Return -min(3, penalty).

## 8. darkriser

### 8.1 The sound

A short, dark riser: noise and a sub grow through a stutter gate that gets faster, up to a cut. There is no rising
synth tone. Measured design targets (from the source's private study set, as facts): risers in edits are short and dark,
0.74 s (0.42 to 1.77), loudest point 248 ms (112 to 433) after the first moment within 20 dB of it, centroid 458 Hz
(374 to 590), 17 % (10 to 57 %) of the energy under 150 Hz, near mono (L/R correlation 0.94); they end about 23 ms
before the cut and fall 20 dB in about 91 ms (79 to 128). A designed sub-heavy build is 1.3 s with 86 % under 150 Hz,
crackle and a speeding stutter on top. A rising synth tone is never drawn.

Layers: body (pink noise in a low-Q band whose centre opens, Q ≤ 2, so never a tone), sub (constant pitch, a little
drive), crackle (sparse 3 to 8 ms grains, independent per channel), the gate, the level curve, width and a short room.

Target "a short dark riser building into a hit". Alternatives: "a jet engine", "a vacuum cleaner", "heavy rain on a
roof", "a helicopter". Refset `riser`. Category `riser` only.

### 8.2 Interface

`SPACE`, `FAMILIES = {'riser': [(name, params), ...]}` (ordered), `DEFAULTS` = copy of the first member, `TARGETS =
{'riser': TARGET}`, `level_db(p, t)`, `gate(p, t)`, `render(p, rng)`, `recipe_for(cat='riser', preset=0)`: an object
with module name `darkriser`, a copy of SPACE, `DEFAULTS` = the chosen member (by index, or by name), `render`,
`TARGET` of cat, `ALTS`, `REFSET`, and `PRESET` = the member name.

### 8.3 SPACE

| key | range | scale | meaning |
|---|---|---|---|
| length | 0.3 to 1.8 | | s to the peak |
| start_db | 12 to 48 | | first sample under the peak |
| curve | 0.5 to 3.5 | log | > 1: slow, then accelerating |
| tail_tau | 0.01 to 0.12 | log | fall after the peak (time constant, s) |
| body_f0 | 60 to 600 | log | initial centre frequency of the body band |
| body_f1 | 150 to 3000 | log | at the peak |
| body_q | 0.5 to 2 | | |
| sub_f | 32 to 90 | log | |
| sub_db | -40 to 12 | | relative to the body |
| sub_drive | 0 to 2 | | |
| crackle_db | -50 to -6 | | |
| crackle_rate | 20 to 600 | log | grains per second at the peak |
| crackle_hp | 1500 to 9000 | log | |
| gate_at | 0.2 to 0.95 | | where the stutter starts (share of the length) |
| gate_f0 | 3 to 16 | log | gate rate there (Hz) |
| gate_f1 | 10 to 45 | log | at the peak |
| gate_depth | 0 to 1 | | |
| duty | 0.25 to 0.85 | | |
| width | 0 to 1 | | |
| room | 0 to 0.4 | | |

### 8.4 Members (in order)

| key | dark | build | snatch |
|---|---|---|---|
| length | 0.74 | 1.2 | 0.45 |
| start_db | 41.86 | 44.04 | 24.0 |
| curve | 1.378 | 0.8445 | 3.108 |
| tail_tau | 0.02767 | 0.07787 | 0.02393 |
| body_f0 | 148.4 | 94.27 | 279.2 |
| body_f1 | 502.3 | 150.0 | 373.3 |
| body_q | 1.136 | 2.0 | 2.0 |
| sub_f | 47.85 | 33.58 | 37.65 |
| sub_db | -10.93 | 3.405 | -13.57 |
| sub_drive | 0.7114 | 1.116 | 1.628 |
| crackle_db | -50 | -6 | -22.68 |
| crackle_rate | 134.9 | 112.1 | 20.44 |
| crackle_hp | 3039 | 9000 | 4463 |
| gate_at | 0.7039 | 0.2 | 0.337 |
| gate_f0 | 7.817 | 6.657 | 7.732 |
| gate_f1 | 19.91 | 12.49 | 45.0 |
| gate_depth | 0.8088 | 0.75 | 0.2926 |
| duty | 0.3958 | 0.5742 | 0.3132 |
| width | 0.2022 | 0.0 | 0.0431 |
| room | 0.2256 | 0.1399 | 0.2851 |

Characters: dark = the short, dark riser heard in music edits (0.74 s, about 430 Hz, 20 % of its energy in the
sub layer, a deep gate that starts late); build = a designed build (1.2 s, a saturated 34 Hz sub below a low body band, strong
high crackle, a stutter that starts at 20 % of the length); snatch = a half-second steep late build under a light fast stutter.

### 8.5 Curves

Level in dB relative to the peak (T = length): for t ≤ T, -start_db × (1 - clip(t / T, 0, 1))^curve; for t > T,
-8.686 × (t - T) / tail_tau (an exponential with time constant tail_tau).

Gate (1 = open): t0 = gate_at × T; v = clip((t - t0) / max(T - t0, 0.001), 0, 1); rate = gate_f0 × (gate_f1 /
gate_f0)^v; phase = running sum of rate / SR, counting only samples with t ≥ t0; frac = (phase + 0.5 duty) mod 1 (it
starts on an open peak, so there is no step where it begins); open = 0.5 - 0.5 cos(2π min(frac / duty, 1)) when frac
< duty, else 0; g = 1 - gate_depth × (1 - open). g = 1 before t0. Then blend to fully open over the last 30 ms before T:
k = clip((t - (T - 0.03)) / 0.03, 0, 1), g = g (1 - k) + k. g = 1 after T.

### 8.6 render(p, rng)

Merge p over `DEFAULTS`.

1. n = samples of T + tail_tau × 60 / 8.686 + 0.02 (the tail reaches -60 dB, plus 20 ms).
2. a = 10^(max(level, -120) / 20). g = gate.
3. Body centre fc = body_f0 × (body_f1 / body_f0)^(m^1.5), m = clip(t / T, 0, 1). 2 bodies: each an FFT pink noise
   (P13b: unit std, 1/√f, floored at 15 Hz, exactly n long) through `bpn` at fc with Q body_q, scaled to unit RMS.
4. Sub: phase 2π sub_f t + random start; gd = 1 + 2 sub_drive; sub = tanh(gd sin φ) / tanh(gd) / 0.7.
5. Crackle, 2 channels, each:
   - Grain times are a Poisson process up to T: the next gap is exponential with mean 1 / rate, with rate =
     crackle_rate × (0.15 + 0.85 × min(1, tg / T)^1.5) at the current time tg. Stop at the first time ≥ T.
   - Each grain: length uniform 3 to 8 ms, white noise times a Hann window of that length, gain 10^(12 log10(uniform(0.05,
     1)) / 20) (that is, uniform(0.05, 1)^0.6). Add at the grain time (cut at the file end). Draw the noise first, then
     the gain.
   - `bw(2)` high-pass at crackle_hp, then scale to RMS 0.25.
6. Width: w = width, kw = √(max(0, 1 - w²)). hp200(x) = `bw(2)` high-pass at 200 Hz. Right body = (body1 - hp200(body1))
   + kw × hp200(body1) + w × hp200(body2). (The lows stay identical in both channels.) Left body = body1.
   Left crackle = crackle1; right crackle = kw × crackle1 + w × crackle2.
7. Per channel: x = body + 10^(sub_db / 20) × sub + 10^(crackle_db / 20) × crackle / 0.25, then times a × g.
8. Room: if room > 0.01, the forge-core small room (P14) with rt60 0.35, size 0.4, damp 0.6, wet room, cut to n
   samples. (If not, the source still builds a room IR to keep its random draw order. A clean implementation need not.)
9. `bw(2)` high-pass 25 Hz, `bw(4)` low-pass 18 kHz. Fade in 2 ms, fade out min(0.02, 0.5 × tail_tau × 60 / 8.686) s.
10. Normalise to an absolute peak of 0.89.

## 9. revcymbal

### 9.1 The sound

A reverse cymbal: a bright, wide swell that grows up to a cut and ends shortly after it. The recipe also makes the
darker reverse swell used in music edits. Measured design targets (source's private study set, as facts): about 1 per film at a section change,
about 1 s long (0.57 to 1.29), the loudest point 42 ms after the cut (+23 to +62 ms). Designed reverse cymbals are
bright (centroid 2.8 to 3.2 kHz, energy between 1 and 16 kHz), fully decorrelated (L/R correlation 0.08 to 0.16), with a
granular shimmer; one stops about 250 ms after its peak, one rings out (-20 dB 0.5 s later). The music-edit swells are
darker and near mono: centroid 556 Hz (310 to 884), 19 % under 150 Hz, rise 189 ms (76 to 413) to the peak, -20 dB
130 ms (68 to 243) after it.

A real reverse cymbal is a cymbal recording played in reverse. So the low partials, which decay slowly, are heard
first; the high partials, which decay fast, come last; and the sound stops at the moment of the strike. This recipe
synthesises that order forward in time. No signal is reversed, so the stop can be shaped freely.

Target "a reverse cymbal swell". Alternatives: "a jet engine", "a wind gust", "rain", "a sword being sharpened".
Refset `reverse_swell`. Category `reverse_swell` only.

### 9.2 Interface

`SPACE`, `FAMILIES = {'reverse_swell': [(name, params), ...]}`, `DEFAULTS` = copy of the first member, `TARGETS`,
`level_db(p, t) -> (dB array, peak time)`, `render(p, rng)`, `recipe_for(cat='reverse_swell', preset=0)` (as darkriser,
module name `revcymbal`).

### 9.3 SPACE

| key | range | scale | meaning |
|---|---|---|---|
| length | 0.3 to 2 | | time from sample 0 to the peak (s) |
| swell_db | 12 to 60 | | how far under the peak it starts |
| curve | 0.5 to 3.5 | log | rise shape in dB (1 = straight, a reversed exponential ring) |
| hold | 0 to 0.45 | | s of plateau after the peak |
| hold_db | -12 to 0 | | level change across the plateau |
| tail_tau | 0.008 to 0.6 | log | fall after the plateau (time constant, s) |
| cym_f | 300 to 7000 | log | centre frequency of the metallic band |
| bw | 1 to 6 | | band width in octaves |
| metal | 0 to 1 | | partials (1) vs noise (0) |
| partials | 12 to 160 | int | |
| shimmer | 0 to 1 | | slow random AM per partial |
| open | 0 to 4 | | how many octaves the low-pass opens as the level grows |
| hp | 80 to 2500 | log | |
| low_db | -50 to 0 | | low noise body (100 to 450 Hz) following the level |
| flut_depth | 0 to 1 | | |
| flut_f | 6 to 60 | log | |
| width | 0 to 1 | | |
| room | 0 to 0.5 | | |
| rt60 | 0.2 to 2 | log | |

### 9.4 Members (in order)

| key | cymbal | ring | dark | short | breath |
|---|---|---|---|---|---|
| length | 0.5037 | 0.808 | 0.5391 | 0.33 | 0.7 |
| swell_db | 19.55 | 40.86 | 30.0 | 24.0 | 30.0 |
| curve | 1.831 | 2.308 | 0.5 | 0.8282 | 1.3 |
| hold | 0.22 | 0.0 | 0.03684 | 0.08 | 0.094 |
| hold_db | -8.039 | -2.062 | -0.4249 | -6.0 | 0.0 |
| tail_tau | 0.03096 | 0.1542 | 0.03842 | 0.025 | 0.06 |
| cym_f | 2600 | 2070 | 346 | 3200 | 1200 |
| bw | 3.469 | 5.575 | 3.456 | 3.2 | 1.499 |
| metal | 0.6317 | 0.7482 | 0.259 | 0.6 | 0.12 |
| partials | 78 | 79 | 117 | 70 | 60 |
| shimmer | 0.5093 | 0.8912 | 0.3166 | 0.04221 | 0.3 |
| open | 0.0 | 3.093 | 2.554 | 0.5 | 1.5 |
| hp | 118.7 | 658.9 | 80.0 | 300 | 150 |
| low_db | -50 | -50 | 0.0 | -50 | -24 |
| flut_depth | 0.6266 | 0.4417 | 0.004984 | 0.8351 | 0.366 |
| flut_f | 18.8 | 21.97 | 28.16 | 24.0 | 14.0 |
| width | 0.98 | 1.0 | 0.35 | 0.95 | 0.5818 |
| room | 0.0 | 0.129 | 0.08302 | 0.05 | 0.2272 |
| rt60 | 1.386 | 1.209 | 0.5783 | 0.8 | 0.5846 |

Characters: cymbal = bright, wide and grainy, with a flat part after the peak and then a stop; ring = swells harder and rings out
(-20 dB a quarter second later); dark = darker, near mono, a low body, a quick fall; short = 0.33 s into the peak for a
quick cut; breath = a reversed breath in the mid band, more noise than metallic tone, half the full width.

### 9.5 Level curve

T = length, hd = hold. For t ≤ T: -swell_db × (1 - clip(t / T, 0, 1))^curve. For T < t ≤ T + hd (only when hd > 0):
hold_db × (t - T) / hd. For t > T + hd: hold_db - 8.686 × (t - T - hd) / tail_tau.

### 9.6 Texture (one channel)

1. lo = max(60, cym_f / 2^(bw / 2)), hi = min(18000, cym_f × 2^(bw / 2)).
2. k = round(partials) frequencies log-uniform in [lo, hi]. σ = max(0.3, bw / 3). Weights exp(-0.5 (log2(f / cym_f) /
   σ)²) × uniform(0.4, 1).
3. One shimmer control noise: P6 at 3 Hz (unit std). Each partial uses it circularly shifted by a random whole number of
   samples: AM = 1 + 0.35 × shimmer × shifted control.
4. Metal = Σ weight × AM × sin(2π f t + random phase), scaled to unit RMS.
5. Noise = white noise, `bw(2)` band-pass lo to hi, then a peaking EQ (P24) at cym_f, +6 dB, Q 1, scaled to unit RMS.
6. x = metal × metal + (1 - metal) × noise, scaled to unit RMS. Return x and hi.

### 9.7 render(p, rng)

Merge p over `DEFAULTS`.

1. tail = length + hold + tail_tau × 60 / 8.686 + 0.02. n = samples of min(4, tail + 0.3 rt60 × min(1, 4 room)).
2. a = 10^(max(level, -120) / 20).
3. Two textures xL, xR (and hi). Two low bodies: white noise `bw(2)` band-pass 100 to 450 Hz. Two flutters:
   exp(flut_depth × P6(flut_f) - 0.5 flut_depth²).
4. w = width, k = √(max(0, 1 - w²)). Left uses (xL, lowL, flutL). Right uses (k xL + w xR, k lowL + w lowR, k flutL +
   w flutR). Correlation is about k.
5. Per channel: cut = clip(1.3 hi × 2^(-open × (1 - √a)), 300, 20000); y = `lp` (Q 0.7) at cut. If low_db > -49.5: add
   10^(low_db / 20) × low / rms(low). Multiply by a × flutter.
6. `bw(2)` high-pass at hp.
7. Room (only when room > 0.01): the forge-core reverb (P10), rt60, size 0.5, predelay 0.008, damp 0.4, wet room,
   width 1, 16 lines, stereo, tail max(0.2, rt60), cut to n. If the result is not finite, keep the dry signal.
8. `bw(4)` low-pass 19 kHz.
9. Edit: 5 ms block RMS of the mono fold in dB (no smoothing), i = loudest, first block at or after i under e[i] - 60
   (else the block count), end = min(length, block × 240 + 20 ms). Fade in 4 ms, fade out 20 ms (P9).
10. Normalise to an absolute peak of 0.89.
