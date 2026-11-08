# recipes-impacts, part pop: the recipes `pop` and `popin`

This part belongs to the spec `recipes-impacts.md` (the index). The index defines the recipe contract, the space
notation, the forge-core primitives (`P-*` codes) and the shared rules. The click layer functions L1-L9 are in part
`recipes-impacts-ui.md`, section 1.3.

## 1. Recipe `pop`

### 1.1 Overview

One module holds 2 generations of pop:
- Version 1: a physical model of a small bubble popping. The module-level space, defaults and families are the
  version 1 ones, and tests pin them.
- Version 2: designed motion-design pops in 3 styles (`chirp`, `blip`, `bubble`). They are exposed as a second recipe
  object (V2) for the forge in `designed` mode.

The module render function dispatches: a parameter set that has the key `style` renders with version 2, any other
set renders with version 1, exactly as before.

Module metadata (version 1):

| Field | Value |
| --- | --- |
| `TARGET` | `a small bubble pop` |
| `ALTS` | `a cork pop`, `a water drop`, `a finger snap`, `a mouse click` |
| `REFSET` | `pop` |
| `REFS` | `<REF_ROOT>/ui/ui_pop_01.wav` .. `ui_pop_04.wav` |

Version 2 recipe object:

| Field | Value |
| --- | --- |
| module name | the pop module's name |
| `TARGET` | `a short tonal pop` |
| `ALTS` | `a mouse click`, `a laser zap`, `a synth beep`, `a finger snap` |
| `REFSET` | `pro_pop` |
| `SPACE`, `DEFAULTS`, `render` | the version 2 space, defaults and render |

### 1.2 Version 1: the physics

The physical stages of a small bubble bursting, each with its model:

0. Leak before the release. A seal takes time to open. During the last few ms before the release, air escapes
   through a gap that widens. The result is turbulent noise, coloured by the same cavity, that grows until the burst.
   An onset that is an ideal step sounds like a digital tick.
1. Rupture. The liquid film tears, and the hole widens at the Taylor-Culick speed (about 10 m/s for a film 1 micron
   thick). A cap of 1 mm therefore vanishes within 0.1-3 ms, in a few separate jerks. Every jerk lets out some of the
   trapped overpressure: a pressure pulse that is positive first, followed by a smaller negative lobe (the net
   pressure is 0). The noise of the shredding film is gone within 1-2 ms. A low-pass sets the brightness of the tear.
2. Cavity ring. The pocket of gas oscillates at the Minnaert frequency f = (1 / (2 pi R)) sqrt(3 gamma p0 / rho),
   about 3.26 Hz m / R, which is 0.5-3.5 kHz for bubbles of a few mm. Damping sets Q to about 5-40. As the pocket gets
   smaller and its neck widens, the pitch goes up (+10 to +120 % within 3-80 ms). The amplitude takes a fraction of a
   ms or more to build. A weak 2nd harmonic tracks the square of the envelope.
3. Second cavity formant: a higher resonance of the same cavity, with its own lower Q, that follows most of the same
   pitch rise (a higher mode with fixed pitch sounds like a glass "ting"). The pitch wavers by a few per cent. Turbulent air
   flowing out through the hole excites the cavity, which gives a band of breath noise that tracks the pitch.
4. Daughter bubble: some ms later, a second ring, higher and briefer.
5. Air release: band-limited hiss lasting some ms.
6. Puff: a soft low half-cycle (the monopole of the volume change).
7. Background room tone ahead of the event, a small room, a microphone placed close.
8. The edit. Real kit pops are mid-band events edited clean: 0.19-0.41 s long, 40 dB down 35-145 ms after the peak,
   energy shares (medians) -26 dB below 150 Hz, -37 dB at 4-12 kHz, -43 dB above 12 kHz. Early rounds ran
   0.7-0.85 s over a -55 dB hiss floor whose low puff and wide-band tick gave (+15-18 dB at 40-80 Hz, +12-19 dB at
   2-16 kHz over the kit). So the final model adds a recording-chain high-pass and low-pass, a quieter floor, and an
   edit `clip` s after the release, but only once the ring is `cut_db` below its peak. Parameter sets without
   these keys render as before.

### 1.3 Version 1 parameters

Ranges after the round 8 limits (the limits replace some first ranges; both are listed).

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `f0` | 400 - 3500 Hz | log | 440.159 | cavity breathing frequency at the burst |
| `q` | 4 - 40 | log | 22.1207 | quality factor (decay tau = q / (pi f0)) |
| `rise` | -0.1 - 1.4 | lin | 0.991892 | pitch rise, fraction of f0 |
| `rise_tau` | 0.002 - 0.08 | log | 0.0159722 | pitch rise time constant |
| `att` | 0.0001 - 0.03 | log | 0.000357098 | ring build-up time |
| `h2` | 0 - 0.3 | lin | 0.00847939 | nonlinear 2nd harmonic |
| `wobble` | 0 - 0.05 | lin | 0.0112906 | random pitch fluctuation, fraction |
| `breath_amp` | 0 - 1 | lin | 0.0 | turbulent air driving the cavity |
| `breath_tau` | 0.001 - 0.03 | log | 0.00848874 | its decay |
| `f2_ratio` | 1.6 - 4.5 | lin | 3.82514 | 2nd formant / f0 |
| `f2_amp` | 0 - 0.8 | lin | 0.8 | 2nd formant level |
| `f2_q` | 3 - 25 | log | 25.0 | 2nd formant Q |
| `f2_follow` | 0.5 - 1.0 | lin | 0.948223 | share of the pitch rise the formant follows |
| `sub_amp` | 0 - 0.6 | lin | 0.00328062 | daughter bubble level |
| `sub_ratio` | 1.2 - 2.6 | lin | 2.08913 | daughter pitch ratio |
| `sub_delay` | 0.002 - 0.03 | log | 0.0111736 | daughter delay |
| `click_amp` | 0 - 1.5 | lin | 0.685048 | rupture pulses level |
| `click_w` | 5e-5 - 6e-4 | log | 7.72837e-05 | rupture pulse width |
| `tear` | 1e-4 - 3e-3 | log | 0.000171419 | time over which the film tears |
| `jerks` | 1 - 6 | int | 1 | number of micro-ruptures |
| `click_lp` | 2000 - 9000 Hz (first 2500 - 20000) | log | 4881.31 | rupture low-pass |
| `shred_amp` | 0 - 0.5 (first 0 - 1) | lin | 0.200779 | film-shred noise |
| `shred_tau` | 2e-4 - 3e-3 | log | 0.00153019 | its decay |
| `shred_hp` | 500 - 5000 Hz (first 500 - 8000) | log | 5000 | its high-pass |
| `hiss_amp` | 0 - 0.6 | lin | 0.0659931 | air release hiss |
| `hiss_f` | 1500 - 6000 Hz (first 1500 - 10000) | log | 2418.88 | hiss band centre |
| `hiss_q` | 0.5 - 3.0 | lin | 2.86992 | hiss Q |
| `hiss_tau` | 0.001 - 0.03 | log | 0.00113073 | hiss decay |
| `puff_amp` | 0 - 0.35 (first 0 - 1) | lin | 0.0 | low puff |
| `puff_f` | 120 - 400 Hz (first 60 - 400) | log | 121.063 | puff frequency |
| `ring_delay` | 0 - 0.003 | lin | 0.00105542 | ring start after the first rupture |
| `rt60` | 0.05 - 0.45 (first 0.08 - 0.9) | log | 0.249181 | room RT60 |
| `wet` | 0 - 0.45 (first 0 - 0.6) | lin | 0.291252 | room wet |
| `damp` | 0.3 - 0.95 | lin | 0.490701 | room high damping |
| `size` | 0.05 - 0.8 | lin | 0.05 | room size |
| `air_db` | -12 - 2 dB | lin | -0.476997 | microphone tilt at 10 kHz |
| `floor_db` | -86 - -64 (first -80 - -55) | lin | -64.4367 | microphone noise floor |
| `swell_amp` | 0 - 0.6 | lin | 0.160452 | leak before the release |
| `swell_t` | 5e-4 - 0.012 | log | 0.00252584 | leak length |
| `pre` | 0.004 - 0.035 | lin | 0.0269276 | room tone before the event |
| `hp` | 40 - 160 Hz | log | 92.6866 | recording chain high-pass (order 4) |
| `lp` | 4000 - 14000 Hz | log | 10181.4 | recording chain low-pass (order 2) |
| `clip` | 0.14 - 0.38 | lin | 0.274425 | edit point after the release |
| `cut_fade` | 0.004 - 0.03 | log | 0.0153059 | edit fade |
| `cut_db` | -56 - -42 dB | lin | (absent) | the edit waits until the ring is at least this far below its peak |

The defaults are the main pop as the final judging left it (0.27 s, a clean edit, band limits like the kit). They have no
`cut_db` key. Kit pops are cut at -46 to -62 dB.

Version 1 families. A family narrows ranges. Apply it by merging it over the space.

| Family | Ranges |
| --- | --- |
| `main` | swell_amp=0.15..0.6; swell_t=0.002..0.012 log; click_amp=0..0.7 |
| `bloop` | f0=350..900 log; rise=0.7..1.4; rise_tau=0.025..0.08 log; att=0.006..0.04 log; q=18..40 log; click_amp=0..0.35; shred_amp=0..0.2; ring_delay=0..0.025 |
| `small` | swell_amp=0.15..0.6; swell_t=0.002..0.012 log; f0=1400..3500 log; rise=0..1.0; click_amp=0..0.8; shred_amp=0..0.45 |
| `lip` | f0=450..1300 log; rise=-0.1..0.35; rise_tau=0.005..0.05 log; q=4..14 log; breath_amp=0.2..1.0; att=0.0003..0.004 log; click_amp=0..0.6; sub_amp=0..0.2 |

`bloop` is a bubble trapped under the surface (there is no film that tears; it rings up while its pitch doubles). `small` is a physically small
bubble. `lip` is a cavity that opens without the big pitch glide: little rise, lower Q, more breath.

### 1.4 Version 1 building blocks

**V1 rupture pulse** (w, n). a = max(2, N(w)), b = max(3, N(2.5 w)). pos = a full-period raised cosine (0.5 - 0.5
cos, 0 -> 2 pi) over a samples. neg = the same shape over b samples, scaled by -sum(pos) / sum(neg) (zero net
area). Output = pos then neg, cut or zero-padded to n.

**V2 tear crackle** (w, tear, jerks, n, rng). Times = 0 plus (jerks - 1) values uniform in [0, tear], sorted. The
1st jerk has amplitude 1. Jerk k >= 1 has amplitude uniform(0.25, 0.8) x 0.8^k and a sign that is negative with
probability 1/4. Add each scaled V1 pulse at its time.

**V3 wobble** (depth, n, rng). 0 when depth <= 0. Else white noise low-passed at 60 Hz (order 2), scaled to RMS
depth.

**V4 pitch track** (f0, rise, rise_tau, n, wob) = f0 x (1 + rise x (1 - exp(-t / rise_tau))) x (1 + wob).

**V5 gliding ring** (f0, q, rise, rise_tau, att, h2, n, rng, wob). f = V4. Phase = running sum of 2 pi f / 48000 +
uniform(-0.3, 0.3). tau = q / (pi f0). env = (1 - exp(-t / att)) x exp(-t / tau). y = env sin(phase) + h2 env^2
sin(2 phase + 0.5).

### 1.5 Version 1 render

Mono output. Base length 0.35 s (n samples).

1. tick = V2(click_w, tear, jerks). When click_lp < 19000: low-pass order 2 at click_lp. x = click_amp x tick.
2. Shred: white noise high-passed at shred_hp (order 2) times exp(-t / shred_tau). x += 0.5 shred_amp x shred.
3. wob = V3(wobble). body = V5(f0, q, rise, rise_tau, att, h2, wob) + f2_amp x V5(f0 x f2_ratio, f2_q, rise x
   f2_follow, rise_tau, att, h2 = 0, wob).
4. When breath_amp > 0.01: fc = V4(f0, rise, rise_tau, wob). br = state-variable band-pass (P-SVF) of white noise at
   fc, Q max(2, q / 2), times (1 - exp(-t / max(att, 0.0003))) x exp(-t / breath_tau). body += breath_amp x br /
   peak(br).
5. When sub_amp > 0.01: sub = V5(f0 x sub_ratio, 0.7 q, 0.4 rise, 0.6 rise_tau, max(att, 0.0002), h2, no wobble).
   Add sub_amp x sub into body at sub_delay.
6. Add body into x at ring_delay.
7. Hiss: P-SVF band-pass of white noise at hiss_f, Q hiss_q, times (1 - exp(-t / 0.0003)) exp(-t / hiss_tau).
   x += hiss_amp x hiss / peak(hiss).
8. Puff: sin(2 pi puff_f t) exp(-2.5 puff_f t) (1 - exp(-t / 0.0004)). x += puff_amp x puff.
9. When swell_amp > 0.01: m = N(swell_t), w = m white samples. leak = SVF band(w, f0, Q 3) + 0.6 SVF band(w, f0 x
   f2_ratio, Q 2) + 0.9 x high-pass(w, 3000 Hz, order 2). leak x= exp(5 (i / m - 1)) (from about -43 dB up to
   0 dB). Prepend swell_amp x leak / peak(leak) to x.
10. lead = the prepended length. Prepend N(pre) zeros. Small room (P-ROOM: rt60, size, damp, wet).
11. When `clip` is absent (early rounds): microphone (P-MIC: air_db, floor_db), trim the tail at -62 dB keeping
    0.02 s, fade in 1 ms and out 20 ms. Return.
12. Else: high-pass order 4 at hp, low-pass order 2 at lp, microphone (P-MIC: air_db, floor_db).
    end = N(pre) + lead + N(clip).
13. When `cut_db` is present: frames of 5 ms (240 samples) of RMS in dB. i = the loudest frame. j = the first frame
    from i whose level is below e[i] + cut_db (or k - i when none, k = frame count). died = (i + j) x 240 +
    N(cut_fade). end = min(length, max(end, died), N(pre) + lead + N(0.42)).
14. Return x cut at end, fade in 1 ms and out cut_fade.

### 1.6 Version 2: what a pro pop is

From the film study:
- The film chirp pop is a falling tone, 9.8 kHz -> 350 Hz in about 110 ms (-43 octaves per second): a 3 ms blip at
  about 10 kHz, about 30 ms of silence, then a stepped descent (4-5 ms tone bursts every 6-13 ms at 7.5, 6, 3, 1.5,
  1 kHz) that lands on a short 350-500 Hz tone with a few harmonics, falling from 12 to 40 dB down in about 30 ms,
  then a hard cut. Mono.
- In the film the blip pop is a tone burst of 5 ms near 1.2 kHz with harmonics to 5 kHz. Over music, a tonal blip falls
  1.1 kHz -> 100 Hz with a click.
- Version 1 rang a fixed 2nd formant (3.8 x 440 Hz, Q 25) inside a 0.25 s room at -10 dB: a hollow, boxy cavity tone
  that no designed pop has.

Styles:
- `chirp`: a sweep down from f_hi to f_lo over `sweep` s, stepped if asked, with an optional blip before it. The
  level peaks at fraction `peak_at` of the sweep. A tone at f_lo follows and ends on a hard cut. 2 later variants replace it when switched on:
  the natural chirp (`c_nat`) and the round 6 natural pop (`n6`).
- `blip`: a gated tone burst with harmonics and an optional pitch drop.
- `bubble`: a pure sine with a rising pitch, a brief tick, an optional second smaller ring, a band of breath noise
  that tracks the pitch, and a very small room. All resonances track the pitch.

### 1.7 Version 2 parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `style` | `chirp`, `blip`, `bubble` | choice | `chirp` | style |
| `f_hi` | 3000 - 11500 Hz | log | 9800 | chirp sweep start |
| `f_lo` | 90 - 900 Hz | log | 360 | chirp sweep end and landing tone |
| `sweep` | 0.025 - 0.14 | lin | 0.055 | sweep time |
| `ease` | 0.5 - 2.0 | lin | 1.0 | sweep time warp (below 1 fast first) |
| `steps` | 0 - 1 | lin | 1.0 | 0 smooth glide, 1 stepped |
| `step_ms` | 4 - 13 | lin | 9.0 | step block (ms) |
| `duty` | 0.45 - 1.0 | lin | 0.62 | gate duty per block |
| `pre_db` | -40 - 0 dB | lin | -2.0 | pre-blip level. At or below -39: off |
| `pre_hz` | 2500 - 11000 Hz | log | 9800 | pre-blip pitch |
| `pre_t` | 0.008 - 0.04 | lin | 0.032 | pre-blip lead |
| `peak_at` | 0.1 - 0.8 | lin | 0.3 | loudest point, fraction of the sweep |
| `start_db` | -12 - 0 dB | lin | -2.0 | sweep start level re the peak |
| `tail_db` | -20 - -3 dB | lin | -11.0 | landing tone level re the peak |
| `tail_hold` | 0 - 0.035 | lin | 0.006 | landing hold |
| `tail_tau` | 0.006 - 0.05 | log | 0.009 | landing decay (dB-linear) |
| `harm` | 0 - 0.7 | lin | 0.35 | 2nd and 3rd harmonics |
| `odd` | 0 - 1 | lin | 0.6 | share of the 3rd against the 2nd |
| `f_blip` | 700 - 2200 Hz | log | 1220 | blip pitch |
| `box` | 0.003 - 0.07 | log | 0.005 | blip gate length |
| `b_att` | 0.0004 - 0.004 | log | 0.0008 | blip attack |
| `b_rel` | 0.002 - 0.014 | log | 0.003 | blip release |
| `b_drop` | 0 - 5 st | lin | 0.0 | blip pitch drop across the box |
| `b_tau` | 0.006 - 0.3 | log | 0.3 | decay inside the box (0.3 = flat) |
| `f0` | 280 - 1400 Hz | log | 520 | bubble start pitch |
| `rise` | 0.1 - 1.6 | lin | 0.9 | bubble pitch rise, fraction |
| `rise_tau` | 0.003 - 0.04 | log | 0.012 | rise time constant |
| `q_tau` | 0.01 - 0.09 | log | 0.035 | bubble ring decay |
| `att` | 0.0004 - 0.004 | log | 0.0008 | bubble build-up |
| `h2` | 0 - 0.3 | lin | 0.08 | bubble 2nd harmonic |
| `daughter` | 0 - 0.6 | lin | 0.15 | daughter ring level |
| `d_ratio` | 1.3 - 2.4 | lin | 1.8 | daughter pitch ratio |
| `d_delay` | 0.004 - 0.016 | lin | 0.008 | daughter delay |
| `tick_db` | -40 - 6 dB | lin | -30.0 | onset click (all styles). At or below -39: off |
| `tick_hz` | 1800 - 9000 Hz | log | 3500 | click band centre |
| `tick_ms` | 1 - 12 | log | 1.5 | click decay scale. Above 1.5: the bright snap variant |
| `breath` | 0 - 0.5 | lin | 0.12 | bubble breath |
| `room` | 0 - 0.25 | lin | 0.06 | bubble room |
| `floor_db` | -110 - -62 dB | lin | -110.0 | bubble noise floor. -110 = digital silence |
| `hp` | 60 - 400 Hz | log | 150 | high-pass (order 4) |
| `lp` | 4500 - 18000 Hz | log | 16000 | low-pass (order 2; at 17500 or more: none) |
| `drive` | 0 - 8 dB | lin | 1.5 | saturation drive. At or below 0.2: off |
| `length` | 0.03 - 0.4 | lin | 0.16 | blip and bubble edit length |
| `fade` | 0.002 - 0.03 | log | 0.004 | end fade |
| `width` | 0 - 0.4 | lin | 0.15 | bubble room width |
| `gdepth` | -1 - 1 | lin | -1.0 | round 3: step gate depth. Below 0: equal to `steps` |
| `c_breath` | -60 - -6 dB | lin | -60.0 | round 3: pitch-following breath. At or below -59: off |
| `c_bq` | 1.5 - 8 | log | 3.0 | breath Q |
| `land_drop` | 0 - 7 st | lin | 0.0 | round 3: landing tone bend down |
| `s_att` | 0 - 0.5 | lin | 0.0 | round 3: soft rise into each step, fraction of the duty |
| `pre_ring` | 0 - 8 ms | lin | 0.0 | round 3: pre-blip ring tail. At or below 0.05: off |
| `c_in` | 0 - 1 ms | lin | 0.0 | round 3: fade-in. At or below 0.01: off |
| `c_air` | -60 - -10 dB | lin | -60.0 | round 4: broadband air layer. At or below -59: off |
| `c_nat` | 0 - 1 | lin | 0.0 | round 5: above 0.5 selects the natural chirp |
| `c_ptau` | 0.002 - 0.03 | log | 0.008 | natural chirp pitch settle |
| `c_tau` | 0.006 - 0.05 | log | 0.018 | natural chirp decay |
| `c_h2` | 0 - 0.3 | lin | 0.1 | natural chirp 2nd partial |
| `c_puff` | -60 - -6 dB | lin | -60.0 | air puff at the onset. At or below -59: off |
| `c_puff_hz` | 800 - 6000 Hz | log | 2500 | puff centre |
| `c_puff_ms` | 0.6 - 6 | log | 2.0 | puff decay (ms) |
| `c_thump` | -60 - -6 dB | lin | -60.0 | low half-cycle under the puff. At or below -59: off |
| `c_room` | 0 - 0.3 | lin | 0.0 | tiny room. At or below 0.005: off |
| `c_pre` | -60 - -3 dB | lin | -60.0 | soft high pre-tick. At or below -59: off |
| `c_pre_hz` | 3000 - 11000 Hz | log | 9000 | pre-tick pitch |
| `c_pre_t` | 0.01 - 0.04 | lin | 0.025 | pre-tick lead |
| `n6` | 0 - 1 | lin | 0.0 | round 6: above 0.5 selects the moving-pitch natural pop |
| `n_f0` | 150 - 12000 Hz | log | 1500 | start pitch |
| `n_f1` | 100 - 6000 Hz | log | 600 | end pitch |
| `n_ptau` | 0.002 - 0.06 | log | 0.01 | exponential approach time constant |
| `n_sweep` | 0 - 0.14 | lin | 0.0 | log-linear sweep time (above 0: replaces the approach) |
| `n_ease` | 0.4 - 2.0 | lin | 1.0 | sweep easing |
| `n_att` | 0.0003 - 0.04 | log | 0.001 | time to the peak |
| `n_tau` | 0.003 - 0.06 | log | 0.015 | decay |
| `n_h2` | 0 - 0.4 | lin | 0.1 | 2nd partial |
| `n_h3` | 0 - 0.2 | lin | 0.0 | 3rd partial |
| `n_m2` | 0 - 0.6 | lin | 0.0 | inharmonic 2nd mode |
| `n_m2r` | 1.3 - 4.5 | lin | 2.3 | its ratio |
| `n_m2d` | 1 - 4 | lin | 2.0 | its decay power |
| `n_breath` | -60 - -8 dB | lin | -60.0 | pitch-following breath. At or below -59: off |
| `n_dbl` | -60 - 0 dB | lin | -60.0 | double pop level. At or below -59: off |
| `n_dgap` | 0.02 - 0.12 | lin | 0.05 | double pop delay |
| `n_dratio` | 0.6 - 2.0 | lin | 1.33 | double pop pitch ratio |
| `n_dtau` | 0.4 - 1.6 | lin | 1.0 | double pop decay ratio |

Fine keys per style (free in a member's search space; every other key is pinned to the member's value, or to the
default when the member does not have it):

| Style | Fine keys |
| --- | --- |
| `chirp` | ease, duty, start_db, tail_tau, harm, odd, tick_db, tick_hz, hp, lp, drive, fade, peak_at, c_breath, land_drop |
| `blip` | b_att, b_rel, b_tau, harm, odd, tick_db, tick_hz, hp, lp, drive, fade |
| `bubble` | rise_tau, att, h2, d_ratio, d_delay, tick_db, tick_hz, tick_ms, breath, room, hp, lp, drive, fade, width, floor_db |

`family_space(params, style = none)`: the style is the argument, else the member's `style`, else `chirp`. The
result has every key of the version 2 space (unlike T15, keys missing from the member fall back to the defaults).

### 1.8 Version 2 building blocks

**W1 harmonics** (phase, f, harm, odd). y = sin(phase). For k = 2 with amplitude harm x (1 - odd) and k = 3 with
amplitude harm x odd: when the amplitude > 0, y += a x clip((18000 - k f) / 3000, 0, 1) x sin(k phase).

**W2 click** (n, db, hz, rng, ms = 1.5). Output length n, peak 10^(db / 20).
- When ms <= 1.5: k = min(n, N(3 ms / 1000)). Band-pass order 2 from hz / 1.5 to min(1.5 hz, 20000) of k white
  samples, times exp(-t / (ms / 1000 / 2.5)).
- Else (the bright snap): k = min(n, N(4 ms / 1000)). Band-pass order 4 from hz / 2 to min(1.7 hz, 20000), times
  (1 - exp(-t / 0.0003)) x exp(-t / (ms / 1000))^2, then a low-pass order 4 at min(4000, 2 hz). The snap stays in
  1-4 kHz.
- Normalise to peak 1, scale, put in the first k samples.

**W3 natural chirp** (round 5). A rounded sine body whose pitch falls fast and settles: a membrane or lip release,
not a stepped sweep.
1. n = N(min(0.4, max(0.03, 7 c_tau + 0.006))).
2. f = f_lo + (f_hi - f_lo) x exp(-t / c_ptau). Phase with a uniform start.
3. env = (1 - exp(-t / 0.0004)) x exp(-t / c_tau).
4. y = (sin(phase) + c_h2 x clip((18000 - 2f) / 3000, 0, 1) x exp(-2 t / c_tau) x sin(2 phase + 0.6)) x env.
5. Puff, when c_puff > -59: k = min(n, N(max(0.004, 5 c_puff_ms / 1000))). Band-pass order 2 from c_puff_hz / 2.2
   to min(2.2 c_puff_hz, 16000), times (1 - exp(-t / 0.0008)) exp(-t / (c_puff_ms / 1000)). Add 10^(c_puff / 20) x
   it / peak into the first k samples.
6. Thump, when c_thump > -59: fl = max(80, 0.35 f_lo). k = min(n, N(1.5 / fl)). th = sin(2 pi fl t) exp(-2 fl t)
   (1 - exp(-t / 0.0006)). Add 10^(c_thump / 20) x th / peak.
7. Raised-cosine fade-in over N(0.0003). cos^2 fade-out over the last N(0.004) samples (the decay ends on zero).
8. Pre-tick, when c_pre > -59: kp = N(0.0025). Over N(c_pre_t) samples: pre = sin(2 pi c_pre_hz t) x exp(-max(t -
   0.00125, 0) / 0.002). Multiply the first kp samples by a symmetric Hann window of kp points (zero ends). Prepend
   10^(c_pre / 20) x pre.
9. Room, when c_room > 0.005: P-VERB with rt60 = 0.1 + 0.4 c_room, size 0.12, predelay 0.003, damp 0.6, wet c_room,
   stereo, width 0.25. Then trim the tail at -56 dB keeping 0.012 s. Output stereo. Else mono.

**W4 one moving-pitch pop** (p, rng, n, f0, f1, tau, att) (round 6). A round body. Its log pitch travels from f0 to
f1 during the entire decay.
1. When n_sweep > 0: u = clip(t / n_sweep, 0, 1), gl = (1 - u)^n_ease. Else gl = exp(-t / n_ptau).
2. f = exp(log f1 + (log f0 - log f1) x gl). Phase with a uniform start.
3. rise = 0.5 - 0.5 cos(pi clip(t / att, 0, 1)). dec = exp(-max(t - att, 0) / tau). env = rise x dec.
4. fade_k = clip((18000 - k f) / 3000, 0, 1).
5. y = sin(phase). When n_h2 > 0: += n_h2 fade_2 dec sin(2 phase + 0.5). When n_h3 > 0: += n_h3 fade_3 dec^2
   sin(3 phase + 1.1). When n_m2 > 0: a 2nd phase on f x n_m2r with a uniform start; += n_m2 fade_{n_m2r}
   dec^n_m2d sin(phase2).
6. y x= env.
7. When n_breath > -59: b = P-SVF band of white noise at min(f, 16000), Q c_bq, scaled to RMS sqrt(0.5) over its first
   max(16, N(att + tau)) samples. y += 10^(n_breath / 20) x b x env.

**W5 moving-pitch pop or double pop** (round 6).
1. Double on when n_dbl > -59. gap = n_dgap when on, else 0.
2. dur = n_att + 6.5 n_tau x (max(1, n_dtau) when double else 1) + gap + 0.006. n = N(min(0.4, max(0.03, dur))).
3. y = W4(n, n_f0, n_f1, n_tau, n_att).
4. When double: k = N(gap). Add 10^(n_dbl / 20) x W4(n - k, n_f0 x n_dratio, n_f1 x n_dratio, n_tau x n_dtau,
   n_att) from sample k.
5. Puff as W3 step 5, but the rise time constant is 0.0006.
6. Thump as W3 step 6, but fl = max(70, 0.35 x min(n_f0, n_f1)).
7. Fades, pre-tick and room as W3 steps 7-9.

**W6 designed chirp** (round 2-4).
1. When n6 > 0.5 return W5. When c_nat > 0.5 return W3.
2. The pre-blip is on when pre_db > -39. T0 = pre_t when it is on, else 0. The tail lasts tail_hold + 5.5 tail_tau
   (about 48 dB of fall). n = N(T0 + sweep + tail + fade + 0.002).
3. Let s = t - T0 (time since the sweep start) and u = clip(s / sweep, 0, 1). The log pitch is L(u) = ln f_hi + (ln
   f_lo - ln f_hi) u^ease.
4. The gate G is 1 unless steps > 0. With steps > 0, let B = step_ms / 1000 (the block length):
   - Held pitch: the block index is floor(max(s, 0) / B). The held log pitch is L at u' = clip(index x B / sweep, 0,
     1). While s < sweep the log pitch becomes (1 - steps) L + steps x (held log pitch).
   - The landing tone starts at T_L, the sweep length rounded up to a whole number of blocks (subtract 1e-9 before
     rounding up, so an exact multiple stays).
   - Within a block, the position is x = (max(s, 0) mod B) / B. The fall edge is e_f = 0.0007 / B. The rise edge is
     e_r = max(e_f, s_att x duty). Before T_L the raw gate is clip(min(x / e_r, (duty - x) / e_f), 0, 1). From T_L on
     it is clip((s - T_L) / 0.0007, 0, 1). The raw gate then goes through 0.5 - 0.5 cos(pi x raw).
   - Depth D = steps when gdepth < 0, else gdepth. G = 1 - D (1 - shaped gate) for s >= 0, and 1 before.
5. When land_drop > 0, lower the log pitch by ln 2 x land_drop / 12 x (1 - exp(-max(s - sweep, 0) / 0.02)).
6. f = exp(log pitch). The phase starts at a uniform draw. y = W1(phase, f, harm, odd).
7. Level in dB: for u < peak_at, start_db x (1 - u / max(peak_at, 1e-3)); else tail_db x (u - peak_at) / max(1 -
   peak_at, 1e-3). Then subtract 8.686 x max(s - sweep - tail_hold, 0) / tail_tau (an exponential decay with time
   constant tail_tau). env = 10^(dB / 20) x G, and 0 for s < 0.
8. i0 = N(T0) when T0 > 0, else 0. Raised-cosine rise over a = min(N(0.0008), n - i0) samples from i0.
9. y x= env.
10. When c_breath > -59: b = P-SVF band of white noise at min(f, 16000), Q c_bq, scaled to RMS sqrt(0.5) over the
    whole length. y += 10^(c_breath / 20) x b x env.
11. When c_air > -59 (no rng draw otherwise): an = band-pass order 2 at 2500-14000 Hz of white noise, RMS sqrt(0.5).
    ea = a peak follower of env: it jumps up to env at once and falls by the factor exp(-1 / (48000 x 2.5 tail_tau))
    per sample. y += 10^(c_air / 20) x an x ea.
12. When the pre-blip is on: k = N(0.003). Add 10^(pre_db / 20) x Hann(k) x sin(2 pi pre_hz t) to the first k
    samples. When pre_ring > 0.05: kk = N(T0 + 0.002). Add 0.5 x 10^(pre_db / 20) x sin(2 pi pre_hz t) x
    exp(-t / (pre_ring / 1000)) x clip(t / 0.0015, 0, 1) to the first kk samples.
13. When tick_db > -39: add W2(n, tick_db, tick_hz, tick_ms), shifted circularly by i0 samples (so it starts at the
    sweep start).
14. When c_in > 0.01: raised-cosine fade-in over N(c_in / 1000).

Output mono.

**W7 blip**.
1. n = N(box + b_rel + fade + 0.004).
2. f = f_blip x 2^(-(b_drop / 12) x clip(t / max(box, 1e-3), 0, 1)). Phase with a uniform start. y = W1.
3. a = clip(t / b_att, 0, 1). rl = clip((t - box) / b_rel, 0, 1). y x= (0.5 - 0.5 cos(pi a)) x (0.5 + 0.5 cos(pi
   rl)) x exp(-t / b_tau).
4. When tick_db > -39: y += W2(n, tick_db, tick_hz, tick_ms).

Output mono.

**W8 bubble**.
1. n = N(max(length, 0.06) + 0.05).
2. trk = f0 x (1 + rise x (1 - exp(-t / rise_tau))). Phase start uniform(-0.3, 0.3).
3. env = (1 - exp(-t / att)) x exp(-t / q_tau). y = env sin(phase) + h2 env^2 sin(2 phase + 0.4).
4. When daughter > 0.01: d = f0 x d_ratio x (1 + 0.4 rise x (1 - exp(-t / (0.6 rise_tau)))), phase from 0, de = (1 -
   exp(-t / max(att, 0.0003))) x exp(-t / (0.6 q_tau)). Add daughter x de x sin(phase_d) at d_delay.
5. When breath > 0.01: b = P-SVF band of white noise at trk, Q 4, times (1 - exp(-t / 0.0006)) exp(-t / 0.006).
   y += breath x b / peak(b).
6. When tick_db > -39: y += W2(n, tick_db, tick_hz, tick_ms).

### 1.9 Version 2 render

1. p = version 2 defaults merged with p.
2. y = W6, W7 or W8 by style.
3. When drive > 0.2: tanh saturation (P-SAT) with drive dB, mix 0.5, RMS level matching.
4. High-pass order 4 at hp. When lp < 17500: low-pass order 2 at lp.
5. Bubble only: when room > 0.005, P-VERB with rt60 = 0.12 + 0.5 room, size 0.15, predelay 0.004, damp 0.6, wet room,
   stereo, width 0.3 + width; else duplicate to 2 channels. When floor_db > -100: microphone (P-MIC, floor_db; other
   microphone settings at their defaults). Cut to N(length).
6. Blip only: cut to min(length of y, max(N(length), N(box + b_rel + 0.002))).
7. Multiply the last min(len, N(fade)) samples by cos^2 of a ramp 0 -> pi/2 (a convex end).

Output channels: bubble is stereo. A chirp is stereo only when its W3 or W5 room ran, else mono. A blip is mono.
A recipe may return mono. The forge-core consumers (the critic, the kit writer and the audition writer) copy a
mono render to 2 channels (index, "Output").

### 1.10 Version 2 palettes

All members are the version 2 defaults with the listed overrides.

Category `pro_pop`, current palette (round 6), names `chirp`, `boop`, `bloop`, `double`, `tink`, `thup`. Every member
first takes this round 6 base: n6=1, drive=0, steps=0, pre_db=-40, tick_db=-40, harm=0, c_breath=-60, hp=120,
lp=16000, fade=0.004.

| Member | Overrides on the round 6 base |
| --- | --- |
| `chirp` | n_f0=9800, n_f1=350, n_ptau=0.03, n_att=0.012, n_tau=0.018, n_h2=0.05, n_breath=-12, c_bq=2.0, c_puff=-22, c_puff_hz=7000, c_puff_ms=0.6, c_pre=-10, c_pre_hz=9500, c_pre_t=0.026, c_room=0.08 (the film chirp, smooth 9.8 kHz -> 350 Hz) |
| `boop` | n_f0=1200, n_f1=210, n_ptau=0.014, n_att=0.0008, n_tau=0.016, n_h2=0.05, c_puff=-12, c_puff_hz=4500, c_puff_ms=0.6, hp=90, c_room=0.06 (a falling boop with a click; the over-music pop) |
| `bloop` | n_f0=260, n_f1=540, n_ptau=0.022, n_att=0.004, n_tau=0.016, n_h2=0.15, n_breath=-24, c_bq=3.0, c_room=0.1, hp=100 (a rising bubble) |
| `double` | n_f0=1700, n_f1=950, n_ptau=0.005, n_att=0.0008, n_tau=0.008, n_h2=0.1, n_m2=0.2, n_m2r=2.76, n_m2d=2.5, n_dbl=-4, n_dgap=0.07, n_dratio=0.75, n_dtau=1.2, c_puff=-16, c_puff_hz=3500, c_puff_ms=0.6, c_room=0.08 (2 rounded blips, the 2nd lower) |
| `tink` | n_f0=2300, n_f1=1900, n_ptau=0.004, n_att=0.0005, n_tau=0.012, n_h2=0.03, n_m2=0.45, n_m2r=2.76, n_m2d=1.5, c_puff=-20, c_puff_hz=6000, c_puff_ms=0.5, c_room=0.08 (a small glassy tink) |
| `thup` | n_f0=200, n_f1=300, n_ptau=0.01, n_att=0.0015, n_tau=0.012, n_h2=0.1, c_puff=-2, c_puff_hz=900, c_puff_ms=2.5, c_thump=-6, c_room=0.08, hp=80 (a low suction "thup": an air puff and a thump) |

Category `pop` (the kit's pops: a rise of 2-10 ms, a quick initial fall with t20 near 15-25 ms, then a soft room tail; each
member adds a bright snap at 1-4 kHz, about as loud as the bubble's peak, because earlier pops had 1-4 kHz at -17 dB of
the total where the references sit at -6 dB). All members have style=bubble.

| Member | Overrides |
| --- | --- |
| 1 (round bubble) | f0=520, rise=0.9, q_tau=0.014, att=0.002, room=0.15, length=0.2, tick_db=0, tick_ms=6, tick_hz=1700 |
| 2 (small, high) | f0=1100, rise=0.55, rise_tau=0.006, q_tau=0.009, tick_db=-6, tick_ms=4, tick_hz=1700, daughter=0.3, room=0.1, length=0.15 |
| 3 (slow bloop) | f0=330, rise=1.3, rise_tau=0.03, q_tau=0.03, att=0.005, tick_db=0, tick_ms=7, tick_hz=1700, daughter=0, room=0.08, length=0.25 |
| 4 (lip or mouth pop) | f0=720, rise=0.25, q_tau=0.008, breath=0.45, tick_db=-2, tick_ms=5, tick_hz=1700, daughter=0, room=0.12, length=0.16 |
| 5 (cork-like pop) | f0=450, rise=0.15, q_tau=0.01, tick_db=2, tick_ms=6, tick_hz=1700, room=0.22, length=0.22 |

The `pop` category has no member names in the source. Open question Q-POP-1 in the index.

Historical `pro_pop` palettes. The module keeps them as named constants for reproducing earlier deliveries.

Round 3 palette (also delivered as round 4 with 1 edit; constant "round 4 family"):

| Member | Overrides |
| --- | --- |
| m0 film stepped chirp | f_hi=9800, f_lo=360, sweep=0.055, ease=0.74, steps=0.6, gdepth=0.5, step_ms=7, duty=0.72, s_att=0.3, pre_db=-3, pre_ring=5, peak_at=0.55, start_db=-2.2, tail_db=-22, tail_hold=0.002, tail_tau=0.0075, harm=0.22, odd=0.2, tick_db=-18, tick_hz=4000, hp=110, lp=14000, drive=2.5, fade=0.008, c_breath=-15, c_bq=3.0, land_drop=3, c_in=0.3 |
| m1 breathy glide | f_hi=5200, f_lo=300, sweep=0.045, ease=1.4, steps=0, pre_db=-40, peak_at=0.03, start_db=-0.5, tail_db=-22, tail_hold=0.002, tail_tau=0.006, harm=0.05, odd=0, tick_db=-34, tick_hz=7000, hp=260, lp=11000, drive=1.0, fade=0.004, c_breath=-13, c_bq=2.5, land_drop=2, c_in=0.3 |
| m2 film blip | style=blip, f_blip=1220, box=0.005, harm=0.5102, odd=0.06914, step_ms=7, b_att=0.0018, b_rel=0.00202, tick_db=-32.96618, tick_hz=2533.60892, hp=81.01064, lp=4820.49678, drive=5.31339, length=0.03, fade=0.02436 |
| m3 falling blip + click | f_hi=1150, f_lo=140, sweep=0.06, ease=1.16, steps=0, pre_db=-40, peak_at=0.1, start_db=-4, tail_db=-12, tail_hold=0.003, tail_tau=0.0119, harm=0.11, odd=0.7, tick_db=-22, tick_hz=6000, hp=300, lp=13800, drive=3.0, fade=0.02, c_breath=-18, c_bq=3.0, land_drop=2, c_in=0.3 |
| m4 plip | style=blip, f_blip=1600, box=0.045, b_drop=4, harm=0.3305, odd=0, step_ms=7, b_att=0.00143, b_rel=0.002, tick_db=-7.33112, tick_hz=5329.17327, hp=78.94849, lp=13005.45302, drive=0.61736, length=0.07, fade=0.02784 |
| m5 droplet | f_hi=3200, f_lo=700, sweep=0.03, ease=0.8, steps=0, pre_db=-40, peak_at=0.45, start_db=-6, tail_db=-18, tail_hold=0.002, tail_tau=0.009, harm=0.08, odd=0, tick_db=-24, tick_hz=6500, hp=200, lp=12000, drive=1.5, fade=0.006, c_breath=-17, c_bq=4.0, land_drop=1.5, c_in=0.3 |

Round 4 edits tried on these 6 members (in order m0..m5):

| Member | Tried edit |
| --- | --- |
| m0 | steps=0.3, gdepth=0.25, harm=0.08, odd=0, start_db=-2.2, c_breath=-11, c_bq=2.0, c_air=-24, lp=9500 |
| m1 | harm=0.08, odd=0, c_breath=-10, c_bq=2.0, c_air=-20 |
| m2 | none |
| m3 | harm=0.06, odd=0.3, c_breath=-12, c_air=-22 |
| m4 | b_tau=0.018, harm=0.12 |
| m5 | harm=0.12, odd=0, c_breath=-12, c_air=-22 |

Only the m1 edit scored better (total -2.72 -> -1.21); the others scored worse. The delivered round 4 edits are the
m1 edit only (the other 5 entries are empty).

Round 5 palette (names `chirp`, `mouth`, `blip`, `drop`, `plip`, `droplet`). Every member first takes this round 5
base: c_nat=1, drive=0, steps=0, pre_db=-40, tick_db=-40, harm=0, c_breath=-60, hp=150, lp=15000, fade=0.004.

| Member | Overrides on the round 5 base |
| --- | --- |
| `chirp` | f_hi=6500, f_lo=420, c_ptau=0.009, c_tau=0.02, c_h2=0.08, c_puff=-16, c_puff_hz=3500, c_puff_ms=1.2, c_room=0.08, c_pre=-10, c_pre_hz=9500, c_pre_t=0.028 |
| `mouth` | f_hi=1500, f_lo=500, c_ptau=0.005, c_tau=0.014, c_h2=0.12, c_puff=-9, c_puff_hz=1800, c_puff_ms=1.5, c_thump=-12, c_room=0.12 |
| `blip` | f_hi=1350, f_lo=1150, c_ptau=0.006, c_tau=0.009, c_h2=0.15, c_puff=-22, c_puff_hz=4000, c_puff_ms=0.8, c_room=0.06 |
| `drop` | f_hi=1150, f_lo=230, c_ptau=0.016, c_tau=0.018, c_h2=0.1, c_puff=-16, c_puff_hz=2500, c_room=0.1, hp=120 |
| `plip` | f_hi=2000, f_lo=1350, c_ptau=0.007, c_tau=0.015, c_h2=0.06, c_puff=-14, c_puff_hz=5000, c_puff_ms=1.0, c_room=0.2 |
| `droplet` | f_hi=3200, f_lo=850, c_ptau=0.004, c_tau=0.011, c_h2=0.05, c_puff=-12, c_puff_hz=6000, c_puff_ms=0.8, c_room=0.1 |

### 1.11 Version 2 targets and penalty

The penalty uses the pro-study measurement of the motion unit (recipes-motion, the notify recipe's measurement:
`length_s`, `t40_ms`, `corr` and the band `shares` in dB of the total over the regions 0-150, 150-500, 500-1000,
1000-4000, 4000-12000, 12000-24000 Hz). low_db = 10 log10(10^(shares[0] / 10) + 1e-12), the share below 150 Hz.

| Category | Targets (key, lo, hi, scale) |
| --- | --- |
| `pro_pop` | (length_s, 0.025, 0.16, 0.05), (t40_ms, 4, 70, 25), (low_db, -200, -10, 6), (corr, 0.95, 1.0, 0.1) |
| `pop` | (length_s, 0.12, 0.42, 0.1), (t40_ms, 25, 160, 40), (low_db, -200, -18, 6) |

Penalty = the sum over targets of min(2, distance), with distance 1 for a missing value, (lo - v) / scale below,
(v - hi) / scale above, 0 inside. A category without targets has penalty 0. The extra score for a category is
-weight x penalty (weight default 1).

### 1.12 Variations

Version 1: the tear times and signs, the wobble, every phase start, all noise, the room and the microphone floor.
Version 2: the phase starts, the breath, air, puff, click and room noise. Version 2 has no explicit level or pitch
jitter, so 2 seeds of one member differ only in noise and phase.

## 2. Recipe `popin`

### 2.1 Sound

The sound of an interface element appearing (a pop-in) or a soft tap, as product launch videos use it: a low thump
under a thin tick. In the reference film that has only sound effects, it is the effect used most: each element that
appears gets one, and they come in runs about 200 ms apart.

Measured pop-in. The body is a thump at 80-150 Hz (ridge near 105 Hz; less than 0.5 dB of the energy lies above
150 Hz; tilt -4 to -6 dB per octave). On top sits a brief noise tick (1-10 kHz, about 30 ms, 30-40 dB below the
body). Often 2 bodies follow each other 60-70 ms apart. Length 0.14-0.27 s, attack 6-19 ms (median 16), t20 16-35 ms,
t40 30-80 ms. In 7 of 10 the end is convex (the level holds, then stops). The image is moderately wide: side 3.5-9 dB
below mid, L/R correlation 0.38-0.78.

The pro tap follows the same style but is shorter: a 150 Hz thump with a faint wide-band click, 0.1 s, attack 11 ms,
t20 18 ms, t40 27 ms, tilt -6.5 dB per octave, side 5 dB below mid. Recorded fingertip taps on glass or another hard
surface (refset `tap`) have the same low thump (centroid 66-190 Hz), a sharper attack (1-6 ms) and a longer fall
(t40 50-120 ms).

Layers. Each layer covers its own band, and all of them reach their peak at the same moment.
1. Body: a sine at `body_f`. Its pitch starts at body_f x (1 + drop) and falls. Raised-cosine attack, a hold, an
   exponential release (the designed thump L3 of part UI). A small 2nd harmonic keeps it audible on small speakers.
2. Tick: a brief burst of band-passed noise, `tick_db` below the body, with an optional glass-like partial.
3. Air: a faint texture at 1-14 kHz that follows the body envelope, `air_db` below it.
4. Double: a second, smaller body, `dbl_dt` later.
5. Width: the right channel of the body is delayed by width / (6 body_f), a Haas offset defined by phase. At most
   72 degrees at the body frequency, so the L/R correlation stays at about 0.3 or above and a mono sum keeps the
   thump. (A fixed delay of 2 ms had given taps a negative correlation.) The tick is decorrelated. An end gate
   follows.
6. Recorded taps are close to mono. For them the recipe adds the hollow mode of the surface (`knock`, `knock_f`: a
   desk, a phone case), a contact burst at the onset and a long quiet second stage of the body (`ring_db`,
   `ring_ms`).

| Field | Value |
| --- | --- |
| `TARGET` | `a soft round low pop as an icon appears on screen` |
| `TARGETS` | `pro_pop_in` -> `a soft round low pop as an icon appears on screen`; `pro_tap` -> `a soft low thump of a finger touching a screen`; `tap` -> the same text as `pro_tap` |
| `ALTS` | `a door slam`, `a balloon popping`, `a wooden knock`, `a cork popping` |
| `REFSET` | `pro_pop_in` |

The alternatives do not include "a kick drum": an audio-text model also labels real pro pop-ins as kick drums, so that
alternative penalised the right sound.

### 2.2 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `body_f` | 70 - 220 Hz | log | 105 | body pitch |
| `drop` | 0 - 1 | lin | 0.35 | start pitch = body_f x (1 + drop) |
| `att_ms` | 1 - 22 | log | 12.0 | body attack (ms) |
| `hold_ms` | 0.5 - 45 | log | 6.0 | body hold (ms) |
| `sag_db` | 0 - 12 dB | lin | 0.0 | level lost during the hold |
| `rel_ms` | 3 - 40 | log | 8.0 | body release (ms) |
| `harm` | 0 - 0.35 | lin | 0.12 | 2nd harmonic |
| `sub` | 0 - 0.7 | lin | 0.2 | octave-down partner sine |
| `sat` | 0 - 8 dB | lin | 2.0 | tanh drive on the body |
| `tick_db` | -34 - -2 dB | lin | -12.0 | tick peak re the body peak |
| `tick_f` | 1200 - 8000 Hz | log | 3500 | tick band centre |
| `tick_bw` | 1 - 3 oct | lin | 2.5 | tick band width (octaves, total) |
| `tick_ms` | 2 - 40 | log | 12.0 | tick decay (ms) |
| `glass` | 0 - 1 | lin | 0.0 | share of a glass-like partial in the tick |
| `air_db` | -60 - -24 dB | lin | -60.0 | air texture re the body peak. At or below -59.5: off |
| `air_tilt` | -1 - 1 | lin | 0.0 | air tilt (-1 dark, +1 bright) |
| `glass_f` | 1800 - 7000 Hz | log | 3200 | glass partial pitch |
| `dbl` | 0 - 0.9 | lin | 0.0 | 2nd body level (at or below 0.02: single hit) |
| `dbl_dt` | 0.05 - 0.08 | lin | 0.065 | 2nd body delay |
| `dbl_ratio` | 0.85 - 1.25 | lin | 1.0 | 2nd body pitch ratio |
| `width` | 0 - 1.2 | lin | 0.6 | body Haas offset as a phase: delay = width / (6 body_f). At or below 0.02: none |
| `knock` | 0 - 0.5 | lin | 0.0 | hollow surface mode level |
| `knock_f` | 280 - 900 Hz | log | 500 | surface mode pitch |
| `tick_width` | 0 - 1 | lin | 0.6 | tick decorrelation amount |
| `hp` | 35 - 70 Hz | log | 45 | body high-pass (order 2) |
| `lp` | 2500 - 16000 Hz | log | 9000 | top roll-off of the whole sound (order 2) |
| `length` | 0.08 - 0.3 | lin | 0.18 | end gate after the onset |
| `release_ms` | 6 - 40 | log | 15.0 | gate release |
| `contact_db` | -60 - 6 dB | lin | -60.0 | round 3: fingertip or nail contact re the body peak. At or below -59.5: off |
| `contact_ms` | 0.2 - 3 | log | 0.8 | contact decay (ms) |
| `contact_f` | 800 - 6000 Hz | log | 2500 | contact band centre |
| `ring_db` | -60 - -10 dB | lin | -60.0 | round 3: quiet long 2nd body stage. At or below -59.5: off |
| `ring_ms` | 10 - 90 | log | 40.0 | its release (ms) |
| `contact_lp_x` | 0 - 2.5 | lin | 0.0 | round 4: 0 = off; otherwise the contact passes a 4th-order low-pass whose corner is contact_f times this value |

The keys `contact_*`, `ring_*` are read with a fallback of -60 or 0 when absent, so older parameter sets render as
before. Rule of thumb for the shapes: hold + 2.3 rel is about t20, hold + 4.6 rel is about t40 (ms), and the measured
attack is about 0.8 x att_ms.

### 2.3 Body (one hit)

Inputs: p, rng, pitch f, level, seconds = 0.32.

1. y = L3 thump (f, seconds, att = att_ms / 1000, hold = hold_ms / 1000, rel = rel_ms / 1000, drop, harm).
2. When sag_db > 0.05: a = att_ms / 1000, h = max(0.001, hold_ms / 1000). y x= 10^(-sag_db x clip((t - a) / h, 0, 1)
   / 20). So the level falls linearly in dB from the peak to the end of the hold.
3. When sub > 0.01: y += sub x L3(0.5 f, seconds, 1.3 att, hold, 1.2 rel, drop = 0.5 drop, harm = 0).
4. When ring_db > -59.5 (no rng draw otherwise): rg = L3(f, seconds, att, hold 0, rel = ring_ms / 1000, drop = 0.5
   drop, harm). nz = white noise band-passed from 0.5 f to 3 f (order 2), normalised to peak 1, times |rg| / peak(rg).
   rg = 0.6 rg / peak(rg) + 0.8 nz (half of it a noisy ring: a surface, not a sine line). y = y / peak(y) +
   10^(ring_db / 20) x rg / peak(rg).
5. When sat > 0.05: tanh saturation (P-SAT), drive sat dB, peak level matching.
6. Return level x y / peak(y).

### 2.4 Render algorithm

Buffer n = N(0.34). pre = 0.002 s.

1. f = body_f x exp(0.012 g) (about 1.2 % pitch). lvl = 10^(0.02 g) (about 0.4 dB).
2. body bus: place Body(f, lvl) at pre. When dbl > 0.02: place Body(f x dbl_ratio, dbl x lvl) at pre + dbl_dt.
3. When knock > 0.005: kn = L2 partials (knock_f x exp(0.02 g), ratios (1, 2.7), amps (1, 0.25), tau = 0.004 +
   0.006 when knock_f < 500 else 0.004, 0.08 s, att = max(0.0005, 0.3 att_ms / 1000)). Place knock x lvl x kn in the
   body bus at pre + 0.3 att_ms / 1000.
4. Contact bus (mono), when contact_db > -59.5: c = L1 burst (contact_ms / 1000, contact_f / 2.5, min(18000, 2.5
   contact_f), att 0.0002). When contact_lp_x > 0.05: low-pass order 4 at min(18000, contact_f x max(1,
   contact_lp_x)), renormalise to peak 1. Place c x peak(body bus) x 10^(contact_db / 20) at pre.
5. High-pass the body bus at hp (order 2).
6. Tick: lo = tick_f / 2^(tick_bw / 2), hi = min(16000, tick_f x 2^(tick_bw / 2)). nz = L1(tick_ms / 1000, lo, hi,
   att 0.0004). gl = L2(glass_f, ratios (1, 2.32), amps (1, 0.3), tau = min(0.012, max(0.002, 0.6 tick_ms / 1000)),
   0.08 s, att 0.0003). tk = (1 - glass) x nz (cut or zero-padded to the length of gl) + glass x gl, normalised to
   peak 1. Both are drawn every render, also when glass = 0.
7. pk = peak of the body bus. T_tick = pre + 0.6 att_ms / 1000 (the tick lands as the body peaks). Place tk x pk x
   10^(tick_db / 20) in the tick bus at T_tick. When dbl > 0.02: place tk x pk x dbl x 10^((tick_db - 3) / 20) at
   T_tick + dbl_dt.
8. Air, when air_db > -59.5: env = sqrt of a centred 96-sample moving average of body^2. nz = white noise band-passed
   1000-14000 Hz (order 2). With a = |air_tilt|: nz = a x (high-pass at 4000 Hz order 1 when air_tilt > 0, else
   low-pass at 4000 Hz order 1) + (1 - a) x nz. Normalise nz to unit RMS. Place nz x env / max(env) x pk x
   10^(air_db / 20) in the tick bus at 0.
9. Width: d = N(width / (6 f)) when width > 0.02, else 0. L = body, R = body delayed by d samples. Add the contact
   bus to both channels.
10. Add the tick bus decorrelated to stereo (P-DECOR, amount tick_width, cut 800 Hz), cut to n.
11. Low-pass the stereo signal at lp (order 2).
12. L4 end gate (length, release_ms / 1000, start pre).
13. When the contact is on: normalise to a peak of -1.5 dBFS (this keeps the limiter's release from pressing the
    contact down into the body).
14. Limit at -1 dBFS. Fade in 0.5 ms, fade out 2 ms.

### 2.5 Families

Format as in part UI 1.5. `family_space(cat, fam)` and `family_params(cat, fam)` follow L9.

Category `pro_pop_in`. The members differ where the references differ: 150 Hz-1 kHz at -5 to -23 dB (harmonics:
`knock`), 4-12 kHz at -17 to -35 dB (texture and tick: `airy`), a darker `deep`, a rising double hit. Earlier members
were near clones to an audio-text model (variant cosine 0.895 against 0.696 for the references).

| Family | Values |
| --- | --- |
| `round` | width=0.6..1.1; knock=0; body_f=95..120 log; att_ms=12..20 log; hold_ms=5..14 log; sag_db=4..10; rel_ms=3..7 log; glass=0; air_db=-50..-38; dbl=0; length=0.12..0.18 |
| `double` | width=0.6..1.1; knock=0; body_f=95..125 log; att_ms=6..14 log; hold_ms=4..12 log; sag_db=3..9; rel_ms=3..7 log; glass=0; air_db=-50..-38; dbl=0.5..0.85; dbl_dt=0.06..0.07; dbl_ratio=1.1..1.25; length=0.16..0.22 |
| `knock` | width=0.5..0.9; knock=0; body_f=115..135 log; harm=0.25..0.35; sat=4..8; att_ms=5..10 log; hold_ms=4..10 log; sag_db=3..8; rel_ms=3..6 log; glass=0; tick_db=-10..-4; tick_f=1500..3000 log; contact_db=-16..-8; contact_ms=0.5..1.5 log; contact_f=1500..3000 log; air_db=-48..-38; dbl=0; length=0.1..0.15 |
| `deep` | width=0.7..1.1; knock=0; body_f=75..92 log; att_ms=14..22 log; hold_ms=8..20 log; sag_db=5..12; rel_ms=4..9 log; sub=0.3..0.7; harm=0..0.08; sat=0..2; glass=0; air_db=-58..-46; dbl=0; tick_db=-30..-20; lp=4000..7000 log; length=0.14..0.22 |
| `airy` | width=0.6..1.1; knock=0; body_f=100..130 log; att_ms=8..16 log; hold_ms=5..14 log; sag_db=3..9; rel_ms=3..7 log; glass=0; air_db=-32..-24; air_tilt=0.3..1.0; tick_db=-12..-6; tick_f=4000..8000 log; tick_ms=15..35 log; tick_width=0.8..1.0; lp=12000..16000 log; dbl=0; length=0.12..0.18 |

Category `pro_tap` (150 Hz thump, attack about 11 ms, a faint broadband texture, gated). Every family has a contact
burst on the onset and more air than before (the reference has energy above 12 kHz at -37 dB of the total). The
pitch drop and the sub are capped (drop at most 0.15, sub at most 0.2) because they are the kick-drum cue.

| Family | Values |
| --- | --- |
| `finger` | drop=0..0.15; sub=0..0.2; width=0.7..1.1; knock=0; glass=0; body_f=135..165 log; att_ms=8..13 log; hold_ms=5..12 log; sag_db=3..9; rel_ms=2.5..5 log; air_db=-40..-30; air_tilt=0..0.8; dbl=0; contact_db=-11..-3; contact_ms=0.4..1.2 log; contact_f=1500..3500 log; lp=9000..16000 log; length=0.07..0.1 |
| `glass` | drop=0..0.15; sub=0..0.2; width=0.7..1.1; knock=0; body_f=140..170 log; att_ms=6..11 log; glass=0.4..0.8; glass_f=2500..5000 log; tick_db=-14..-6; hold_ms=4..11 log; sag_db=3..9; rel_ms=2.5..5 log; air_db=-44..-32; air_tilt=0..1.0; dbl=0; contact_db=-9..-1; contact_ms=0.3..0.8 log; contact_f=3000..6000 log; lp=11000..16000 log; length=0.07..0.1 |
| `soft` | drop=0..0.15; sub=0..0.2; width=0.7..1.1; knock=0; glass=0; body_f=110..140 log; att_ms=10..18 log; tick_db=-24..-14; hold_ms=7..15 log; sag_db=4..10; rel_ms=3..6 log; air_db=-46..-36; dbl=0; contact_db=-17..-9; contact_ms=0.8..2.0 log; contact_f=1200..2500 log; length=0.08..0.11 |
| `knock` | drop=0..0.15; sub=0..0.2; width=0.7..1.1; knock=0; glass=0; body_f=150..210 log; att_ms=3..7 log; tick_db=-10..-2; hold_ms=3..9 log; sag_db=3..9; rel_ms=2.5..5 log; air_db=-38..-28; air_tilt=0..1.0; dbl=0; contact_db=-5..3; contact_ms=0.4..1.0 log; contact_f=2000..4500 log; lp=11000..16000 log; length=0.06..0.09 |
| `double` | drop=0..0.15; sub=0..0.2; width=0.7..1.1; knock=0; glass=0; body_f=130..165 log; att_ms=6..12 log; dbl=0.3..0.6; dbl_dt=0.05..0.065; hold_ms=4..10 log; sag_db=3..9; rel_ms=2.5..5 log; air_db=-42..-32; contact_db=-11..-3; contact_ms=0.4..1.2 log; contact_f=1500..3500 log; length=0.11..0.14 |

Category `tap` (recorded taps of a fingertip or a finger joint: mono, a sharp attack, the hollow mode of the surface,
a longer tail). After a 1 ms attack a real tap loses about 20 dB within about 7 ms and then keeps ringing softly. So: a short loud body over a quiet long
ring, and a contact burst on the onset whose 4-12 kHz skirt is darkened by `contact_lp_x`. The tick and air are
quiet, a top roll-off at 6-12 kHz, a high-pass at 50-80 Hz.

| Family | Values |
| --- | --- |
| `finger` | width=0..0.3; knock=0.05..0.3; glass=0; body_f=110..160 log; att_ms=1.0..2.0 log; hold_ms=0.5..2 log; rel_ms=2.5..5 log; ring_db=-30..-21; ring_ms=17..32 log; dbl=0; contact_db=-6..2; contact_lp_x=1.1..1.8; contact_ms=0.4..1.2 log; contact_f=1200..2800 log; tick_db=-24..-14; air_db=-60..-42; lp=7000..12000 log; hp=50..75 log; sub=0..0.15; length=0.08..0.12 |
| `glass` | width=0..0.3; knock=0.05..0.25; body_f=120..180 log; att_ms=1.0..2.0 log; glass=0.3..0.6; glass_f=1800..3200 log; tick_db=-18..-10; tick_f=1500..3500 log; dbl=0; hold_ms=0.5..2 log; tick_ms=2..8 log; rel_ms=2.5..5 log; ring_db=-30..-22; ring_ms=18..35 log; contact_db=-2..4; contact_lp_x=1.1..1.8; contact_ms=0.3..0.8 log; contact_f=2000..4000 log; air_db=-60..-40; lp=8000..12000 log; hp=50..75 log; sub=0..0.15; length=0.08..0.12 |
| `table` | width=0..0.3; knock=0.05..0.4; glass=0; body_f=80..110 log; att_ms=1.2..3 log; hold_ms=0.5..3 log; dbl=0; sub=0; rel_ms=3..6 log; ring_db=-29..-20; ring_ms=15..30 log; contact_db=-8..0; contact_lp_x=1.1..1.8; contact_ms=0.8..2.0 log; contact_f=900..2000 log; tick_db=-26..-16; air_db=-60..-46; lp=6000..10000 log; hp=55..80 log; length=0.09..0.14 |
| `knuckle` | width=0..0.3; knock=0.1..0.5; glass=0; body_f=150..220 log; att_ms=0.8..1.6 log; tick_db=-16..-8; tick_f=1500..3500 log; dbl=0; hold_ms=0.5..1.5 log; tick_ms=2..8 log; rel_ms=2.5..4.5 log; ring_db=-27..-19; ring_ms=18..35 log; contact_db=-2..4; contact_ms=0.4..1.0 log; contact_f=1500..3200 log; contact_lp_x=1.1..1.8; air_db=-60..-46; lp=7000..11000 log; hp=55..80 log; sub=0..0.1; length=0.08..0.12 |
| `soft` | width=0..0.3; knock=0.05..0.25; glass=0; body_f=95..130 log; att_ms=1.5..4 log; tick_db=-28..-18; dbl=0; hold_ms=1..4 log; rel_ms=3.5..7 log; ring_db=-28..-19; ring_ms=15..30 log; contact_db=-12..-4; contact_lp_x=1.1..1.8; contact_ms=0.8..2.0 log; contact_f=900..2200 log; air_db=-60..-44; lp=6000..10000 log; hp=50..75 log; sub=0..0.15; length=0.09..0.14 |

Some `tap` ranges leave the full space: `knuckle` att_ms starts at 0.8 (full space minimum 1.0), and the `tap` hp
ranges reach 80 Hz (full maximum 70). The family range replaces the full range.

The delivered `tap` sounds (the seed bank) are not these boxes. They are the round 3 picks with later edits: the
contact skirt low-passed on finger, glass and table (contact_lp_x 1.5-2.2), quieter ticks on glass and knuckle, the
glass contact moved from 4.6 to 3.2 kHz, shorter rings, the sub capped (0.12; table 0.3) over a 55 Hz high-pass.
Those values live in the seed file (part seeds). Measured result of that delivery: 4-12 kHz at -25.3 dB of the
total (references -29.6), t40 75-95 ms (references 90).

### 2.6 Variations

Per render: pitch exp(0.012 g), level 10^(0.02 g), knock pitch exp(0.02 g), the L3 thump start phases, all noise
(tick, contact, air, ring noise), the tick decorrelation.
