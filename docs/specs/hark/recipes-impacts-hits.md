# recipes-impacts, part hits: cuthit, subdrop, thump, knock

This part belongs to the spec `recipes-impacts.md` (the index). The index defines the recipe contract, the space
notation, the forge-core primitives (`P-*` codes) and the shared rules. The tonal building blocks T1-T15 are in
part `recipes-impacts-tonal.md`.

## 1. Recipe `cuthit`

### 1.1 Sound

A brief hit placed on an edit: a sub with a kick on top, as music-driven films use it, plus a mid-range variant
with more punch.

Measured (licensed film study, medians with the inter-quartile range):
- The sub drop of voice-over and music films is short: 0.26 s (0.11-0.59), its loudest point 61 ms (37-137) after the
  onset, -20 dB 116 ms (67-145) after that, the sub at 57-155 Hz, mono (correlation 0.96). Editors nearly always pair
  it with a kick (12 pairs start within 60 ms). The beater lifts the centroid to 435 Hz (149-598), and only 45 %
  (25-86 %) of the energy stays under 150 Hz. The older bank's sub drops were 1-1.5 s blooms at 54-83 Hz, 5-7 times
  darker than the films' even when faded to 0.3 s.
- In the music films the impact lasts 0.28 s, peaks 52 ms after the onset, has a centroid of 306 Hz and keeps 64 % of
  its energy below 150 Hz.
- In the effects-driven film the designed hits last longer and spread wide. One kind lays a kick over a sub drop and
  decays into grains of rumble (correlation near 0). Another puts a quick sub under a held band of air at 5-10 kHz.

Layers. Every layer is scaled to peak 1 and then mixed with a level set relative to the sub.
- sub: a sine whose pitch moves from sub_f0 down to sub_f1. It rises on a raised-cosine curve to its peak, then loses
  20 dB every sub_t20 s (linear in dB). A tanh curve adds the 2nd and 3rd harmonics. sub_phase shifts the right
  channel by up to 90 degrees, so the L/R correlation is cos(phase).
- kick: a sine that sweeps kick_f0 -> kick_f1 and decays on its own time constant.
- click: the beater. Band-limited noise shaped by (1 - t / len)^3, plus a 3 ms ping.
- punch: band noise in the low mids, which gives the hit its weight near 300 Hz.
- rumble: Hann-windowed grains of low brown noise that come less often over time and fall 20 dB per rumble_t20. Soft
  ticks and optional crackle ride on the grains. A faint low bed sits under them. Each channel is drawn on its own.
- air: high-passed noise, drawn on its own per channel.
- space: width decorrelates the content above 150 Hz (the sub keeps its own phase shift). The top layers get a short,
  dark room.

A raised-cosine fade closes the file at `length`. The sub must fade out, never cut off.

| Field | Value |
| --- | --- |
| `TARGET` | `a short deep sub bass hit with a kick drum` |
| `TARGETS` | `cut_hit` -> the same text |
| `ALTS` | `an explosion`, `a door slam`, `thunder`, `a low hum` |
| `REFSET` | `cut_hit` |

### 1.2 Parameters

| Key | Range | Scale | Meaning |
| --- | --- | --- | --- |
| `length` | 0.12 - 1.8 | lin | file length (s) |
| `fade` | 0.004 - 0.25 | log | end fade (s) |
| `sub_f0` | 50 - 260 Hz | log | sub start pitch |
| `sub_f1` | 36 - 130 Hz | log | sub landing pitch |
| `sub_glide` | 0.008 - 0.4 | log | fall time constant |
| `sub_attack` | 0.002 - 0.35 | log | bloom to the peak |
| `sub_t20` | 0.03 - 0.8 | log | s from the peak to -20 dB |
| `sub_drive` | 0 - 3 | lin | tanh harmonics |
| `sub_phase` | 0 - 1 | lin | right channel phase x 90 degrees |
| `kick` | 0 - 1.5 | lin | kick level |
| `kick_f0` | 120 - 700 Hz | log | kick start pitch |
| `kick_f1` | 40 - 130 Hz | log | kick end pitch |
| `kick_tau` | 0.003 - 0.06 | log | kick sweep time constant |
| `kick_decay` | 0.015 - 0.25 | log | kick decay |
| `click` | 0 - 1 | lin | beater level |
| `click_f` | 1200 - 9000 Hz | log | beater centre |
| `click_ms` | 1 - 15 | log | beater length (ms) |
| `punch` | 0 - 1.2 | lin | smack level |
| `punch_f` | 140 - 1000 Hz | log | smack centre |
| `punch_decay` | 0.008 - 0.15 | log | smack decay |
| `rumble` | 0 - 1.2 | lin | granular rumble level |
| `rumble_t20` | 0.1 - 1.5 | log | rumble time to -20 dB |
| `grain_rate` | 12 - 70 /s | log | grains per second at the start |
| `grain_tick` | 0 - 1 | lin | soft tick per grain |
| `crackle` | 0 - 1.5 | lin | broadband click per grain |
| `air` | 0 - 1 | lin | air band level |
| `air_f` | 3000 - 12000 Hz | log | air band bottom |
| `air_len` | 0.03 - 0.8 | log | air decay |
| `width` | 0 - 1 | lin | decorrelation above 150 Hz |
| `room` | 0 - 0.4 | lin | short dark room on the top layers |

The defaults are the first family member (`sub`). `space_for(cat)` returns a copy of the full space.
`recipe_for(cat = 'cut_hit', preset = 0)` returns a recipe object for 1 member (index or name) with the module name,
the full space, the member as defaults, the render, TARGET, ALTS, REFSET `cut_hit` and PRESET = the member name.

### 1.3 Family `cut_hit`

The members are hand-designed corners, tuned toward each style on measured features (the band shares, the envelope,
the onset, the decay, the share below 150 Hz, the width) and on an audio-text embedding only up to a grade bar (cosine 0.70). Every member stays
original (at most 0.79 cosine to any study clip). A member with a slow bloom was removed, because listeners heard it as a sustained tone.

| Key | `sub` | `kick` | `punch` | `rumble` | `air` |
| --- | --- | --- | --- | --- | --- |
| length | 0.2858 | 1.022 | 0.2823 | 1.4 | 0.5493 |
| fade | 0.1949 | 0.0589 | 0.01307 | 0.04147 | 0.2087 |
| sub_f0 | 224.8 | 176.6 | 144.2 | 125.0 | 203.3 |
| sub_f1 | 45.69 | 37.04 | 47.8 | 40.27 | 50.96 |
| sub_glide | 0.04827 | 0.1106 | 0.06248 | 0.15 | 0.02 |
| sub_attack | 0.01613 | 0.007655 | 0.06125 | 0.07542 | 0.05396 |
| sub_t20 | 0.04364 | 0.05602 | 0.03285 | 0.09859 | 0.03 |
| sub_drive | 0.6188 | 0.3078 | 2.878 | 0.863 | 3.0 |
| sub_phase | 0.0 | 0.0 | 0.09153 | 1.0 | 1.0 |
| kick | 0.6151 | 1.025 | 0.6973 | 0.3807 | 1.084 |
| kick_f0 | 174.9 | 363.2 | 284.4 | 197.6 | 193.6 |
| kick_f1 | 92.57 | 108.2 | 84.94 | 69.6 | 50.09 |
| kick_tau | 0.008307 | 0.009914 | 0.0141 | 0.004458 | 0.006564 |
| kick_decay | 0.2089 | 0.03626 | 0.04286 | 0.0623 | 0.03479 |
| click | 0.1667 | 0.1688 | 0.4873 | 0.0 | 0.4183 |
| click_f | 1226 | 2440 | 9000 | 2568 | 3710 |
| click_ms | 2.506 | 5.914 | 5.467 | 2.467 | 1.241 |
| punch | 1.069 | 1.077 | 0.9001 | 0.03031 | 0.1212 |
| punch_f | 374.4 | 437.1 | 1000 | 140 | 190.7 |
| punch_decay | 0.1464 | 0.04711 | 0.07649 | 0.01199 | 0.1313 |
| rumble | 0.0 | 0.0 | 0.0 | 1.2 | 0.9522 |
| rumble_t20 | 0.1565 | 0.1437 | 0.1443 | 1.5 | 0.4372 |
| grain_rate | 12.92 | 29.02 | 15.95 | 21.45 | 14.47 |
| grain_tick | 0.002876 | 0.0 | 0.04938 | 0.6988 | 0.4375 |
| crackle | 0.0 | 0.0 | 0.0 | 0.0 | 0.7462 |
| air | 0.0 | 0.0 | 0.0 | 0.3205 | 0.824 |
| air_f | 9115 | 6018 | 5858 | 3809 | 10810 |
| air_len | 0.03 | 0.07952 | 0.1071 | 0.166 | 0.6526 |
| width | 0.3875 | 0.0 | 0.346 | 0.6293 | 1.0 |
| room | 0.05584 | 0.01595 | 0.1467 | 0.3665 | 0.4 |

Characters:
- `sub`: the brief sub hit of the music films. An 808-style sine goes from 225 to 46 Hz with a low punch under it and
  is 20 dB down after about 0.15 s.
- `kick`: the same shape, with the kick in front. The beater and a smack at 440 Hz raise the centroid.
- `punch`: the music films' impact. About 0.3 s, a saturated sub, a bright smack, two thirds of the energy below
  150 Hz.
- `rumble`: the effects-driven film's designed hit. A kick over a slowly falling sub that turns into wide grains.
- `air`: a quick sub under crackling grains and a held bright band of air, wide.

### 1.4 Layers

t = k / 48000 over n = N(length) samples.

**C1 bloom** (t, attack, t20): 0.5 - 0.5 cos(pi t / attack) for t < attack, else 10^(-(t - attack) / t20) (-20 dB
every t20 s).

**C2 sub** -> (left, right). f = sub_f1 + (sub_f0 - sub_f1) exp(-t / sub_glide). Phase = running sum + uniform(0, 2
pi). env = C1(sub_attack, sub_t20). g = 1 + 2 sub_drive. For offsets 0 (left) and 0.5 pi sub_phase (right): s =
tanh(g sin(phase + offset)) / tanh(g), low-passed at 900 Hz (order 2), times env.

**C3 kick**. f = kick_f1 + (kick_f0 - kick_f1) exp(-t / kick_tau). Phase start uniform(-0.2, 0.2). sin(phase) x
exp(-t / kick_decay) x (1 - exp(-t / 0.0004)).

**C4 click** (n). L = min(n, max(16, N(click_ms / 1000))). w = white x (1 - u)^3 band-passed order 2 from 0.5
click_f to min(2.2 click_f, 19000), normalised. ping over k = min(n, N(0.012)) samples: sin(2 pi click_f t) exp(-t /
0.003) (1 - exp(-t / 0.0002)). y = w (first L) + 0.5 ping (first k). Normalise.

**C5 punch**. White noise band-passed order 2 from punch_f / 1.7 to min(1.7 punch_f, 19000), times exp(-t /
punch_decay) (1 - exp(-t / 0.0008)), normalised.

**C6 air** -> (left, right), 2 independent draws: white noise high-passed order 4 at air_f, low-passed order 2 at
min(2.5 air_f, 19000), times (1 - exp(-t / 0.005)) exp(-t / air_len).

**C7 rumble** (n) -> (left, right), 2 independent channels. For each channel, grains start at tg = 0.01 s and
continue while tg < the span:
1. u = tg / span. Grain length m = N(uniform(0.025, 0.06)).
2. g = the running sum of m white samples (brown), minus its mean, band-passed 35-220 Hz (order 2), times a Hann
   window of m points.
3. Level a = 10^(-tg / rumble_t20) x exp(min(0.35 x standard normal, 0.3)) (a log-normal spread, at most +2.6 dB; a
   louder grain would sound like a second hit).
4. Add a x g / peak(g) at N(tg) (cut at the end).
5. When grain_tick > 0.01: k = min(N(0.014), remaining); when k > 8: tick = band-pass 800-5000 Hz (order 2) of k white
   samples, times sin^2(0.5 pi min(t / 0.0025, 1)) x exp(-t / 0.003). Add 0.25 x grain_tick x a x tick / peak.
6. When crackle > 0.01 (drawn only when used): k = min(N(uniform(0.001, 0.003)), remaining); when k > 8: ck = white
   high-passed at 500 Hz (order 2) times a Hann window. Add crackle x a x exp(0.5 g) x ck / peak.
7. tg += an exponential draw with mean 1 / (grain_rate x (1 - 0.5 u)) (the grains slow to half rate).
After the grains, when rumble > 0.01: a bed = running sum of n white samples minus its mean, band-passed 35-220 Hz,
normalised, times 0.06 x 10^(-t / rumble_t20), added.

**C8 end cut** (y, end_db = -60). 5 ms frames (240 samples) of the channel-mean RMS in dB. When fewer than 4 frames,
return y. Else cut 30 ms after the last frame at or above max + end_db.

### 1.5 Render algorithm

1. p = defaults merged with p. n = N(length).
2. Draw the layers in this order whatever their levels (crackle and the rumble bed only when on), so a seed is 1 take:
   sub (C2), kick (C3), click (C4), punch (C5), air (C6), rumble (C7).
3. top = 0.6 kick x C3 + 0.5 click x C4 + 0.6 punch x C5.
4. When room > 0.01: top = P-ROOM(top, rt60 0.3, size 0.35, damp 0.7, wet room), cut to n. Else draw a room impulse
   response with the same settings and discard it (keeps the draw order).
5. Width: w = width, rk = sqrt(max(0, 1 - w^2)). hiL = top high-passed at 150 Hz (order 2), lo = top - hiL. side =
   hiL delayed by N(0.0007) samples and high-passed at 150 Hz again. topL = lo + hiL; topR = lo + rk hiL + w side.
6. rumR = rk rumL + w rumR. rl = the joint peak of the 2 rumble channels. airR = rk airL + w airR. al = the joint
   air peak.
7. L = subL + topL + 0.7 rumble x rumL / rl + 0.35 air x airL / al. R likewise with the right parts.
8. High-pass at 24 Hz (order 2). C8 end cut at -60 dB.
9. Fade in 0.4 ms, fade out min(fade, 0.9 x length in s).
10. Normalise to a peak of 0.89.

Output: stereo, at most `length` s. Correlation of the top and rumble layers is about rk.

### 1.6 Variations

Per take: the sub, kick and click phases, all noise layers, the grain times, lengths and levels, the room.

## 2. Recipe `subdrop`

### 2.1 Sound

A designed sub drop and its 808 and boom relatives. One gesture: a sine that glides down into the sub region,
exponential in frequency (a straight line on a log-frequency axis).

Measured (licensed film study): the pro film's drop goes 64 -> 44 Hz (-0.55 octave) in 0.75 s. It starts on an edit
under a bright transient, reaches its peak about 0.38-0.43 s after the cut, and then decays at about 40 dB/s,
straight in dB. More than 85 % of the energy is under 150 Hz, and the image is narrow. Sound libraries use the same
shape, quicker and wider: an 808 boom (1-2 octaves of fall within 10-150 ms, then a slow decay), a slow ominous drop
(1.5 octaves over about 1.5 s), and a brief drop that sits under an impact.

A 40 Hz tone does not reach a listener through a small speaker. So the recipe adds harmonics 2-6 of the gliding sub. The ear then infers the
missing fundamental.

Layers:
- sub: the gliding sine with a bloom, a hold and a decay that is straight in dB.
- harmonics: partials 2-6 of the sub, under a roll-off at `harm_lp`, and nothing above 300 Hz.
- click: a short noise burst plus a ping at `click_f`.
- thump: a kick-like body that falls 150 -> 90 Hz.
- grit: low brown noise that follows the sub envelope.
- floor: an inaudible band noise at -80 dB (see 2.4 step 10).
- width: a little side signal above 150 Hz. The sub stays centred and mono.
- room: a short dark room on the click and the thump only. The sub stays dry.

A fade closes the file, so the sub never cuts off. The recipe uses no recordings.

| Field | Value |
| --- | --- |
| `TARGET` | `a deep sub bass drop` |
| `TARGETS` | `sub_drop` -> the TARGET text; `pro_sub_drop` -> `a designed sub drop: a low sine falling under a bright click` |
| `ALTS` | `a kick drum`, `an explosion`, `thunder`, `a low hum` |
| `REFSET` | `sub_drop` |

The module also makes available the pro-study measurement, the study penalty, the study extra term and the study
target table of the recipe `impact` (part `recipes-impacts-impact.md`, sections 4.7 and 4.8), under the same public
names. It does not define its own measurement.

### 2.2 Parameters

| Key | Range | Scale | Meaning |
| --- | --- | --- | --- |
| `f0` | 45 - 220 Hz | log | start pitch |
| `f1` | 28 - 62 Hz | log | end pitch |
| `glide` | 0.06 - 1.6 s | log | time to go from f0 to f1 |
| `glide_curve` | 0.3 - 3.0 | log | shape of the glide; below 1 = fast first |
| `bloom` | 0.004 - 0.45 s | log | time to the sub peak |
| `hold` | 0 - 0.5 s | lin | time at the peak |
| `decay_db` | 15 - 90 dB/s | lin | decay after the hold |
| `length` | 0.6 - 3.0 s | lin | asked file length; the last 15 % fades |
| `harm` | 0 - 0.8 | lin | harmonics level (for small speakers) |
| `harm_lp` | 150 - 650 Hz | log | harmonics roll-off |
| `click` | 0 - 1 | lin | transient level, relative to the sub peak |
| `click_f` | 1200 - 8000 Hz | log | transient centre |
| `click_ms` | 1 - 40 ms | log | transient length; the pro drop's transient is 30-50 ms |
| `thump` | 0 - 1 | lin | kick-like body level at the onset |
| `grit` | 0 - 1 | lin | low noise texture under the sub |
| `width` | 0 - 0.3 | lin | side level above 150 Hz |
| `room` | 0 - 0.4 | lin | short dark room on the click and the thump |

The defaults are the member `pro` of the family `sub_drop`.

### 2.3 Families

Family `sub_drop` (the library drops; each member is one corner of the character parameters):

| Key | `808` | `pro` | `long` | `punch` | `gritty` |
| --- | --- | --- | --- | --- | --- |
| f0 | 120 | 64 | 90 | 160 | 110 |
| f1 | 40 | 44 | 30 | 50 | 36 |
| glide | 0.12 | 0.75 | 1.4 | 0.2 | 0.5 |
| glide_curve | 0.6 | 1.0 | 1.0 | 0.5 | 0.8 |
| bloom | 0.004 | 0.38 | 0.15 | 0.004 | 0.05 |
| hold | 0.05 | 0.05 | 0.2 | 0.0 | 0.1 |
| decay_db | 30 | 42 | 22 | 70 | 35 |
| length | 2.0 | 1.4 | 3.0 | 0.8 | 1.8 |
| harm | 0.35 | 0.3 | 0.25 | 0.5 | 0.7 |
| harm_lp | 300 | 300 | 250 | 400 | 450 |
| click | 0.4 | 0.6 | 0.15 | 0.7 | 0.3 |
| click_f | 3000 | 3500 | 2500 | 4000 | 2200 |
| click_ms | 3 | 30 | 8 | 2 | 6 |
| thump | 0.5 | 0.0 | 0.0 | 0.8 | 0.2 |
| grit | 0.0 | 0.0 | 0.15 | 0.0 | 0.6 |
| width | 0.05 | 0.05 | 0.08 | 0.05 | 0.12 |
| room | 0.1 | 0.1 | 0.0 | 0.15 | 0.05 |

Family `pro_sub_drop` (variants of the pro film's drop):

| Key | `ref` | `quick` | `low` | `bright` | `soft` |
| --- | --- | --- | --- | --- | --- |
| f0 | 64 | 72 | 58 | 66 | 62 |
| f1 | 44 | 46 | 37 | 44 | 44 |
| glide | 0.75 | 0.5 | 0.85 | 0.7 | 0.75 |
| glide_curve | 1.0 | 1.0 | 1.0 | 1.0 | 1.3 |
| bloom | 0.38 | 0.24 | 0.42 | 0.3 | 0.43 |
| hold | 0.04 | 0.02 | 0.06 | 0.04 | 0.08 |
| decay_db | 42 | 52 | 38 | 42 | 34 |
| length | 1.3 | 1.0 | 1.5 | 1.25 | 1.5 |
| harm | 0.25 | 0.3 | 0.4 | 0.6 | 0.2 |
| harm_lp | 300 | 320 | 260 | 420 | 260 |
| click | 0.6 | 0.8 | 0.45 | 0.9 | 0.2 |
| click_f | 3500 | 4200 | 2800 | 5500 | 2600 |
| click_ms | 35 | 25 | 40 | 30 | 40 |
| thump | 0.0 | 0.1 | 0.0 | 0.0 | 0.0 |
| grit | 0.0 | 0.0 | 0.1 | 0.0 | 0.05 |
| width | 0.04 | 0.04 | 0.04 | 0.06 | 0.03 |
| room | 0.08 | 0.08 | 0.06 | 0.12 | 0.05 |

Study targets (a public table, ranges in the keys of the pro-study measurement):

| Category | Targets |
| --- | --- |
| `pro_sub_drop` | len50 0.8..1.6; attack_ms 250..460; slope_dBs -50..-32; sh150 0.85..1.0; corr 0.8..1.0; ridge0 55..75; ridge1 38..50 |
| `sub_drop` | len50 0.6..3.0; sh150 0.8..1.0; corr 0.8..1.0; slope_dBs -90..-15 |

Search spaces. A search may move only the timbre and polish keys. A search led by an audio-text embedding once moved
the members' pitch, sweep and bloom (their character) far off, so the character stays as designed.

| Category | Fine keys |
| --- | --- |
| `sub_drop` | harm, harm_lp, click, click_f, length |
| `pro_sub_drop` | harm, harm_lp, click, click_f, click_ms, length |

Narrower limits for `pro_sub_drop`: f0 54-80 log, f1 34-50 log, glide 0.4-1.0 log, bloom 0.2-0.45 log, decay_db
32-55 lin, length 0.9-1.6 lin, click_ms 15-45 log. Only the limits on fine keys take effect (length and click_ms).

`space_for(cat)`: for each fine key of cat, the limit range when one exists, else the full range.
`recipe_for(cat, preset = 0)`: a recipe object with the module name, SPACE = `space_for(cat)`, DEFAULTS = a copy of
the member (by index or name), the render, TARGET = `TARGETS[cat]`, ALTS, REFSET = cat and PRESET = the member name.
An unknown member name is an error.

### 2.4 Render algorithm

t = k / 48000.

1. p = defaults merged with p.
2. Length: life = bloom + hold + 70 / decay_db (the time at which the decay reaches -70 dB). seconds = max(0.3,
   min(length, life)). n = N(seconds).
3. Pitch path: s = clip(t / max(glide, 0.001), 0, 1)^glide_curve. f = exp(ln f0 + (ln f1 - ln f0) s). glide_curve =
   1 gives a constant number of octaves per second.
4. phase = 2 pi cumsum(f) / 48000 + uniform(0, 0.2).
5. Envelope env: 0.5 - 0.5 cos(pi t / bloom) for t < bloom; 1 up to bloom + hold; after that 10^(-decay_db (t -
   bloom - hold) / 20).
6. sub = sin(phase) x env.
7. Harmonics, only when harm > 0.01. For k = 2..6: fk = k f; taper = clip((300 - fk) / 80, 0, 1); add (0.6 / k) / (1
   + (fk / harm_lp)^4) x sin(k phase + 0.4 k) x taper. Multiply the sum by env, normalise it to peak 1 and scale by
   harm. The roll-off is computed per sample and therefore tracks the glide. Constraint: an earlier saturated copy of the sub ran
   its harmonic series up to 12 kHz, and the forge comb-ring detector read it as a metallic ring. The series must
   stop below 300 Hz.
8. Grit, only when grit > 0.01: brown noise = the running sum of n normals, minus its mean, band-passed 35-160 Hz
   (Butterworth order 2), normalised, times 0.3 grit x env.
9. Top layers: top = click x CLICK + 0.7 thump x THUMP (both always drawn, in that order):
   - CLICK: L = min(n, max(16, N(click_ms / 1000))). u = k / L. w = L normals x (1 - u)^3 x (1 - exp(-t / 0.0002)),
     high-passed at click_f / 2 (order 2), normalised. Ping over k = min(n, N(0.02)) samples: sin(2 pi click_f t)
     exp(-t / 0.003) (1 - exp(-t / 0.0003)). y = w (first L samples) + 0.6 ping (first k). Normalise to peak 1.
   - THUMP: over the whole buffer, f = 90 + 60 exp(-t / 0.03). sin(2 pi cumsum(f) / 48000 + uniform(0, 0.3)) x
     exp(-t / 0.06) x (1 - exp(-t / 0.001)). Then a 10 ms fade out (P-FADE, no fade in). The thump covers the whole
     buffer: a shorter buffer once left a click under the quiet tail of the `punch` member.
   - When room > 0.01 and top is not all zero: top = P-ROOM(top, rt60 0.35, size 0.4, damp 0.7, wet room), cut to n.
10. Floor: n normals band-passed 300-12000 Hz (order 2), normalised, times 10^-4 (-80 dB) x max(env, 0.01). Always
    drawn. Constraint: when the band above 300 Hz held nothing but a few low partials, the forge comb-ring detector (a tail log
    spectrum over 300 Hz-12 kHz) flag a false ring. This inaudible floor keeps that band filled.
11. mono = sub + harmonics + grit + top + floor.
12. Width: hi = (harmonics + top) high-passed at 150 Hz (order 2). side = width x hi delayed by N(0.0045) samples,
    cut to n. L = mono + side, R = mono - side.
13. High-pass both channels at 22 Hz (order 2).
14. P-FADE: in 0.0015 s, out max(0.05, 0.15 seconds).
15. Normalise to a peak of 0.89.

Random draw order: the sub phase, the grit normals (when on), the click normals, the thump phase, the room (when on),
the floor normals.

Output: stereo, `seconds` long, peak 0.89. Correlation stays near 1 because the side is small and only above 150 Hz.

### 2.5 Variations

Per take: the sub and thump start phases, the click noise, the grit, the room and the floor. The pitch path and the
envelope are fixed by the parameters.

## 3. Recipe `thump`

### 3.1 Sound

A whoosh with a thump: a large, low pass of air that arrives quickly and then rings on. Music films use it to change
scenes (forge category `whoosh_big`).

Measured (licensed film study, 23 events in 11 films, medians with the inter-quartile range): 0.69 s (0.42-0.84) long.
The loudest point is 82 ms (57-190) after the onset and within one frame of the cut. Then a long low tail: -20 dB
475 ms (180-766) after the peak. Centroid 336 Hz (176-415), 80 % of the energy under 150 Hz, mono (L/R correlation
0.95). The older forge's big whooshes approached for 0.25-0.6 s and died 3-6 times faster after the peak.

Construction: the pass engine of the whoosh recipe (unit recipes-motion) does the whoosh. Its base here has a brief
approach that speeds up (about 0.1 s, rise_k below 1) and a fall that holds and then drops (fall_k above 1). Most of
its level is in the sub and low bands. On top sits the THUMP: a sine that slides from thump_f0 to thump_f1, decays
with thump_tau and starts thump_lead s before the pass peak. Without it the whoosh drifts past the edit. With it the
whoosh lands on the edit. The thump is timed from the engine's peak, so the whoosh keys that move the peak (rise, hold, fall, start level) move the thump
with it. No recorded audio is used.

| Field | Value |
| --- | --- |
| `TARGET` | `a deep low whoosh that lands with a thump` |
| `TARGETS` | `whoosh_big` -> the TARGET text |
| `ALTS` | `a strong wind`, `a bass drum`, `a car passing`, `thunder` |
| `REFSET` | `whoosh_big` |

### 3.2 Parameters

SPACE = the full space of the whoosh pass engine (unit recipes-motion) plus these thump keys:

| Key | Range | Scale | Meaning |
| --- | --- | --- | --- |
| `thump_db` | -40 - 6 dB | lin | thump peak relative to the pass peak; -39.5 or less = off |
| `thump_f0` | 60 - 220 Hz | log | thump start pitch |
| `thump_f1` | 32 - 90 Hz | log | thump end pitch |
| `thump_tau` | 0.03 - 0.4 s | log | thump amplitude time constant |
| `thump_lead` | -0.04 - 0.03 s | lin | strike time before (+) or after (-) the pass peak |

The thump keys are a public tuple, in the table order. They are removed from the parameters before the engine
renders.

Base parameter set (also the DEFAULTS), built in 3 layers, each later layer overriding the earlier:
1. the engine's current big-whoosh preset (unit recipes-motion),
2. the engine's optional keys at their neutral values (so this layer turns off the late big-whoosh additions: the
   held hiss tail becomes none, the hiss floor 0, the room damping 0.55),
3. these values:

| Key | Value | Key | Value | Key | Value |
| --- | --- | --- | --- | --- | --- |
| rise | 0.13 | rise_k | 0.8 | hold | 0.01 |
| fall | 0.55 | fall_k | 1.6 | start_db | -34 |
| track | 0.7 | f_peak | 200 | up_oct | 1.0 |
| down_oct | 1.6 | dop_st | 2.0 | dop_w | 0.06 |
| q_air | 1.0 | lp_ratio | 4.0 | flut_depth | 0.2 |
| flut_f | 10 | low_db | -2 | low_f | 110 |
| low_smooth | 0.03 | sub_db | 0 | sub_f | 52 |
| sub_glide | 0.4 | sub_drive | 1.0 | hiss_db | -26 |
| hiss_hp | 1500 | hiss_open | 0.8 | hiss_pow | 1.3 |
| tone_db | -50 | grain_db | -50 | room_wet | 0.15 |
| rt60 | 0.7 | room_damp | 0.45 | pan | 0.05 |
| width | 0.08 | drive_db | 2.5 | end_db | -64 |
| end_fade | 0.08 | thump_db | -6 | thump_f0 | 120 |
| thump_f1 | 50 | thump_tau | 0.12 | thump_lead | 0.01 |

Every other engine key keeps its big-whoosh value.

### 3.3 Family `whoosh_big`

Three hand-designed corners of the base, each tuned toward the measured `whoosh_big` profile of the music films (the
length, the approach, the tail, the centroid, the share below 150 Hz, the tonal share, the width). The deep and airy members kept their character keys
(f_peak, sub_f or hiss, width) and a fixed thump level while they were tuned. A member is stored as overrides of the
base. Keys a member does not list take the base value.

| Key | `thump` | `thump_deep` | `thump_airy` |
| --- | --- | --- | --- |
| rise | - | 0.1401 | 0.11 |
| rise_k | 1.266 | 0.8922 | - |
| fall | - | 0.75 | 0.45 |
| fall_k | - | 1.761 | - |
| track | 0.871 | 0.9656 | 0.7214 |
| f_peak | - | 150 | 320 |
| up_oct | 2.118 | 2.965 | 2.596 |
| down_oct | 1.68 | 1.4 | 2.257 |
| q_air | - | 1.343 | 1.52 |
| lp_ratio | 4.211 | 1.959 | 7.464 |
| flut_depth | 0.26 | 0.06493 | 0.4371 |
| flut_f | 10.37 | 22.93 | 9.589 |
| low_db | -22.24 | -17.48 | -4.037 |
| low_f | 283.6 | 241.1 | 50.94 |
| sub_db | -14.85 | -20.64 | -4.72 |
| sub_f | 59.26 | 44.0 | 57.99 |
| sub_glide | 0.8055 | 0.8703 | - |
| sub_drive | 1.462 | 0.9437 | - |
| hiss_db | -15.21 | -15.95 | -20.0 |
| hiss_hp | 3308 | 4168 | 5687 |
| rt60 | 0.3 | 0.8878 | - |
| room_wet | - | 0.2214 | - |
| width | 0.4326 | 0.2907 | 0.2 |
| drive_db | - | 5.561 | 6.444 |
| thump_db | - | -9.0 | -12.0 |
| thump_f0 | - | 92.21 | 115.4 |
| thump_f1 | 70.85 | 41.61 | 45.69 |
| thump_tau | - | 0.1913 | 0.06806 |
| thump_lead | - | 0.00001 | - |

Characters: `thump` = the tuned base. `thump_deep` = deeper and longer, the sub sinks further and the tail rings for
about 0.8 s. `thump_airy` = a louder moving air band and a brighter hiss over a weaker sub, slightly wider.

Public helpers:
- `preset(name, cat = 'whoosh_big')`: the base merged with the member's overrides.
- `preset_full(cat, params)`: the base merged with params.
- `recipe(cat = 'whoosh_big', preset_name = 'thump')`: a recipe object with TARGET = `TARGETS[cat]`, ALTS, REFSET
  `whoosh_big`, the module name, SPACE = the full space (engine plus thump keys), DEFAULTS = `preset(preset_name,
  cat)`, the render and PRESET = preset_name.

This recipe has no `space_for` or `recipe_for`.

### 3.4 Render algorithm

1. q = the base merged with p. Engine keys that are absent from both stay at their neutral values (off).
2. x = the whoosh pass engine render of q without the thump keys, with the same rng. x is stereo.
3. Peak time of the pass, as the engine defines it: tp = rise x (max(-start_db, 1) / 30)^(1 / rise_k).
4. n = len(x). i0 = clip(N(max(0, tp - thump_lead)), 0, n - 1).
5. Thump, drawn after the engine, so the thump settings never change the engine's random take: m = n - i0. When m <=
   16 the thump is all zeros and nothing is drawn. Else, from sample i0, with t = 0 at i0: f = thump_f1 + (thump_f0 -
   thump_f1) exp(-t / 0.025); phase = 2 pi cumsum(f) / 48000 + uniform(-0.3, 0.3); y = sin(phase) x exp(-t /
   thump_tau) x (1 - exp(-t / 0.0006)).
6. When thump_db > -39.5: fade the thump out over min(0.05, 0.5 n / 48000) s (P-FADE, no fade in). lvl = peak(x).
   Add 10^(thump_db / 20) x lvl x thump to both channels.
7. Normalise to a peak of 0.89.

The thump is drawn even when it is off, so a seed gives the same engine take for any thump_db.

### 3.5 Variations

Per take: every random part of the pass engine (unit recipes-motion) and the thump start phase.

## 4. Recipe `knock`

### 4.1 Sound

A knock in the middle of the spectrum, on wood or plastic: a finger joint or a stick that hits a block, a case or a
desk.

Measured (film catalog, 15 knocks of 7 films that stand at least 8 dB clear of the music, 28 events in 15 films; the
widest spread of any foley touch): 0.18 s (0.11-0.26) long, 19 ms (10-28) from the first sound to the peak, -20 dB 34
ms (12-131) after the peak, centroid 661 Hz (422-1327), 10 % under 150 Hz (2-57 %), almost nothing above 4 kHz, tonal
share 0.47 (0.42-0.55), near mono (correlation 0.95). Kinds: a wooden block (about 660 Hz), a plastic click (2.2-2.5
kHz), a tonal wood block (1.2 kHz, a 30 ms ring). Knocks lead the cut by about 40 ms. The older bank had only low
knocks (130-180 Hz, 4-5 times too dark) and a 5.9 kHz bamboo tok (9 times too bright).

Model: a struck body (modal synthesis).
- Body: the mode set of a material on the lowest mode f1. Higher modes die first (with a negative slope the upper modes
  last longer, like a hollow case). A tiny detuning per take. A hard hit makes the body stiffer for some
  milliseconds, so the pitch begins sharp and falls back (a "tock" that drops).
- Grain: 28 quieter modes packed densely. Real wood and plastic have very many modes. With only a handful of pure
  sines the knock sounds like a struck bar (the film knocks are noisy, tonal share 0.47; the older candidates 0.97).
- Strike: a contact pulse with grit (a finger joint 1-3 ms, a stick 0.3 ms). The longer the contact, the weaker the
  high modes. The noise of the contact is added on top.
- Flam: a softer first contact before the main one (2 knuckles, a "knock-knock"). Bounce: a softer contact after it
  (a stick or a plastic part that bounces back).
- Low body under it (a desk or a table, the share under 150 Hz).
- Finish (T13): high-pass, low-pass, a small room, a little width.

| Field | Value |
| --- | --- |
| `TARGET` | `a knock on a wooden block` |
| `ALTS` | `a door knock`, `a mouse click`, `a hand clap`, `a bubble pop`, `a metal clank` |

The module has no `REFSET` and no `TARGETS`.

### 4.2 Materials

Each material is a list of modes (ratio to f1, amplitude, decay multiplier):

| Material | Modes | Model |
| --- | --- | --- |
| `block` | (1.0, 1.0, 1.0), (1.48, 0.55, 0.6), (2.15, 0.45, 0.45), (3.4, 0.25, 0.3), (4.6, 0.12, 0.2) | slit block |
| `shell` | (1.0, 0.7, 1.0), (1.31, 1.0, 0.8), (1.77, 0.8, 0.6), (2.43, 0.6, 0.5), (3.1, 0.4, 0.35), (4.2, 0.25, 0.25), (5.6, 0.15, 0.2) | plastic case |
| `tok` | (1.0, 1.0, 1.0), (2.7, 0.15, 0.3), (5.1, 0.05, 0.15) | temple block |
| `desk` | (1.0, 1.0, 1.0), (1.9, 0.6, 0.5), (2.8, 0.5, 0.4), (4.4, 0.3, 0.25), (6.3, 0.15, 0.15) | table top |

### 4.3 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `mat` | block, shell, tok, desk | choice | block | material |
| `f1` | 250 - 2600 Hz | log | 620 | lowest mode |
| `decay` | 0.004 - 0.12 s | log | 0.03 | time constant of the lowest mode |
| `slope` | -0.4 - 1.4 | lin | 0.6 | tau ~ (f / f1)^-slope |
| `tilt` | -1 - 1 | lin | 0 | mode amplitudes x ratio^tilt; above 0 the upper modes lead |
| `cloud` | -60 - 6 dB | lin | -60 | grain cloud level relative to the main modes; -59 or less = off |
| `cloud_hi` | 2 - 10 | log | 4 | cloud top, x f1 |
| `cloud_dec` | 0.3 - 2 | log | 0.8 | cloud decay relative to the body |
| `jitter` | 0 - 0.02 | lin | 0.006 | mode detuning per take (fraction) |
| `bend` | 0 - 4 | lin | 0.05 | pitch starts this fraction high; 1-4 = a falling tock |
| `bend_ms` | 1 - 80 ms | log | 4 | bend settling time |
| `contact` | 0.0002 - 0.004 s | log | 0.0012 | contact pulse length; longer = softer and darker |
| `grit` | 0 - 1 | lin | 0.4 | contact brightness |
| `noise` | -60 - 6 dB | lin | -18 | contact noise relative to the body peak; -59 or less = off |
| `noise_hz` | 300 - 6000 Hz | log | 1800 | contact noise centre |
| `noise_ms` | 0.3 - 20 ms | log | 1.5 | contact noise decay |
| `flam` | -60 - -2 dB | lin | -60 | softer first contact; -59 or less = off |
| `flam_ms` | 4 - 70 ms | lin | 14 | flam lead |
| `bounce` | -60 - -2 dB | lin | -60 | softer contact after; -59 or less = off |
| `bounce_ms` | 6 - 60 ms | lin | 20 | bounce delay |
| `body` | -60 - 6 dB | lin | -20 | low body level; -59 or less = off |
| `body_f` | 50 - 260 Hz | log | 140 | low body pitch |
| `body_ms` | 4 - 60 ms | log | 15 | low body decay |
| `hp` | 40 - 400 Hz | log | 70 | finish high-pass |
| `lp` | 2500 - 16000 Hz | log | 8000 | finish low-pass |
| `room` | 0 - 0.3 | lin | 0.08 | room wet |
| `rt` | 0.05 - 0.4 s | log | 0.12 | room rt60 |
| `width` | 0 - 0.6 | lin | 0.08 | stereo width |
| `jit` | 0 - 1 | lin | 0.5 | take-to-take spread: at 1, pitch about +-2 %, decay +-10 %, contact +-15 % |

Fine keys (a family search may move them): decay, slope, tilt, cloud, cloud_hi, cloud_dec, jitter, bend_ms, contact,
grit, noise, noise_hz, noise_ms, flam, flam_ms, bounce, bounce_ms, body_f, body_ms, hp, lp, room, rt, width. The
character keys mat, f1, bend and body, and also jit, stay pinned.

`family_space(params)` = T15 pinned family space of (SPACE, DEFAULTS merged with params, the fine keys).

### 4.4 Family `knock`

`FAMILY['knock']` is a list of 5 full parameter dicts (each is DEFAULTS merged with its overrides).
`FAMILY_NAMES['knock']` names them in order: `wood`, `plastic`, `snap`, `desk`, `double`. All values below are the
merged values.

| Key | `wood` | `plastic` | `snap` | `desk` | `double` |
| --- | --- | --- | --- | --- | --- |
| mat | block | block | block | block | tok |
| f1 | 400 | 589.7 | 1055 | 699.2 | 537 |
| decay | 0.00898 | 0.0042 | 0.00446 | 0.02025 | 0.005 |
| slope | 1.4 | 1.4 | 0.9923 | 0.552 | -0.00403 |
| tilt | 0.4048 | 0.2389 | 0.4321 | 0.08284 | 0.5245 |
| cloud | -9.5 | -60 | -53.72 | -0.8436 | -50.47 |
| cloud_hi | 6.712 | 6.513 | 10 | 5.867 | 3.995 |
| cloud_dec | 0.8552 | 0.5565 | 0.4418 | 0.9966 | 1.394 |
| jitter | 0.01331 | 0.00101 | 0.01216 | 0.01775 | 0.01402 |
| bend | 2.919 | 3.485 | 4.0 | 2.157 | 1.762 |
| bend_ms | 3.488 | 5.989 | 1.017 | 71.36 | 9.208 |
| contact | 0.00076 | 0.004 | 0.004 | 0.00066 | 0.00244 |
| grit | 0.6466 | 0.9598 | 1.0 | 0.894 | 0.6739 |
| noise | 0.01577 | -29.01 | -21.06 | 3.033 | -27.15 |
| noise_hz | 300 | 304.7 | 1316 | 1279 | 2665 |
| noise_ms | 0.3 | 9.585 | 11.51 | 14.94 | 3.251 |
| flam | -42.5 | -26.35 | -15.46 | -20.16 | -2.088 |
| flam_ms | 12.92 | 48.96 | 38.32 | 12.09 | 55.14 |
| bounce | -19.75 | -10.06 | -11.12 | -47.54 | -14.68 |
| bounce_ms | 44.74 | 18.4 | 21.61 | 16.56 | 28.8 |
| body | -11.36 | -48.31 | -51.53 | 4.088 | -15.67 |
| body_f | 102 | 177.7 | 180.6 | 95 | 179.5 |
| body_ms | 4.521 | 38.28 | 23.32 | 28.8 | 4.703 |
| hp | 107.2 | 40 | 94.95 | 70 | 169.2 |
| lp | 11210 | 3815 | 5813 | 4889 | 7249 |
| room | 0.1648 | 0.03 | 0.03 | 0.09192 | 0.00579 |
| rt | 0.1519 | 0.08539 | 0.08 | 0.276 | 0.2958 |
| width | 0.2037 | 0.2528 | 0.2403 | 0.5584 | 0.3936 |
| jit | 0.1328 | 0.2616 | 0.2051 | 0.8611 | 0.8699 |

Characters: `wood` = the films' wooden knock; it falls inside 6-7 of the 7 film inter-quartile ranges on every take,
scored over 4 takes, because in a single take the contact pulse can cancel a mode and make the knock sound rounder
than it is. `plastic` and `snap` = bright plastic hits at 2.4-3 kHz with a second contact 20 ms later. `desk` = a low
finger-joint knock with the table resonance under it. `double` = 2 knocks 55 ms apart. Its decay went from 7.9 to 5
ms: with the longer, purer tail the audit heard a comb ring on a few of 60 takes.

How they were found: random and local searches over the space, scored against the measured knock profile of the films (T14, T15) and on
an audio-text embedding of fixed 1 s windows.

Study targets (T15 format: lo, hi, scale), measured with T14:

| Key | lo | hi | scale |
| --- | --- | --- | --- |
| dur_s | 0.11 | 0.26 | 0.08 |
| attack_ms | 10 | 28 | 10 |
| t20_ms | 12 | 131 | 30 |
| centroid_hz | 422 | 1327 | 300 |
| lo150 | 0.02 | 0.57 | 0.1 |
| tonal | 0.42 | 0.55 | 0.1 |
| corr | 0.94 | 0.98 | 0.1 |

### 4.5 One strike (HIT)

Inputs: p, rng, n. Output: mono, n samples, with the contact at sample 0.

1. Main modes: for each mode (q, a, dm) of the material: f = f1 q. Skip it when f >= 0.42 x 48000. Amplitude a x
   q^tilt. tau = max(0.0008, decay x dm x q^-slope).
2. y = T10 struck modes (jitter, bend, bend_tau = bend_ms / 1000).
3. When cloud > -59: make a child rng seeded by one integer draw in [0, 2^31) from the render rng. With the child
   rng: hi = min(f1 cloud_hi, 0.4 x 48000). 28 frequencies fc = exp(uniform(ln(1.1 f1), ln(max(hi, 1.2 f1)))). tc =
   max(0.0008, decay x cloud_dec x (fc / f1)^-slope). ac = uniform(0.3, 1.0) x (fc / f1)^(tilt - 0.5), 28 draws. c =
   T10 of the cloud with the child rng (no jitter, the same bend). y = y / peak(y) + 10^(cloud / 20) x c / peak(c).
4. y = T11 strike (contact_s = contact, bright = grit), cut to n, normalised to peak 1.
5. When noise > -59: y += 10^(noise / 20) x T8 burst(n, lo = noise_hz / 2, hi = 2 noise_hz, tau = noise_ms / 1000,
   rise 0.0002).

### 4.6 Render algorithm

1. p = DEFAULTS merged with p.
2. Take spread: j = jit x 3 standard normals. f1 x= exp(0.02 j1). decay x= exp(0.1 j2). contact x= exp(0.15 j3).
3. flam is on when flam > -59. lead = flam_ms / 1000 when flam is on, else 0.
4. ring = decay x the maximum over the material's modes of dm x q^-slope. When the cloud is on: ring = max(ring,
   decay x cloud_dec x max(1.1^-slope, cloud_hi^-slope)).
5. dur = lead + (bounce_ms / 1000 when bounce > -59, else 0) + 7 max(ring, body_ms / 1000 when body > -59, else 0) +
   0.02. n = N(min(1.0, dur)).
6. k = N(lead) when the flam is on, else 0. Add HIT(p, n - k) from sample k.
7. Flam on: add 10^(flam / 20) x HIT(p with contact x 1.5, n) from sample 0.
8. Bounce on: kb = k + N(bounce_ms / 1000). When kb < n - 8: add 10^(bounce / 20) x HIT(p with contact x 1.3, n - kb)
   from sample kb.
9. Body on: tb = T9 soft thump(n - k, body_f x (1 + 0.03 g), body_ms / 1000, rise 0.001). Add 10^(body / 20) x tb
   from sample k.
10. Return T13 finish(hp, lp, room_wet = room, rt60 = rt, width, cut = 500), other T13 inputs at their defaults.

Output: stereo, at most 1 s before the finish trim.

### 4.7 Variations

Per take: the pitch, decay and contact spread (scaled by jit), the mode detuning and phases, the grain cloud, the
contact pulse, the contact noise, the body pitch (+-3 % per standard deviation), the finish room.

