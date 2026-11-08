# recipes-impacts, part tuned: mallet, drip, pluck

This part belongs to the spec `recipes-impacts.md` (the index). The index defines the recipe contract, the space
notation, the forge-core primitives (`P-*` codes) and the shared rules. The tonal building blocks T1-T15 are in
part `recipes-impacts-tonal.md`.

## 1. Recipe `mallet`

### 1.1 Sound

Mallet notes that are cut short by a damper (glockenspiel, celesta, metallophone, marimba), a brief soft pluck, and
a builder for runs of chime notes.

Measured (film catalog, music films, high and medium events at least 8 dB clear of the music):
- Glockenspiel and celesta notes (28 events, 14 films): 0.33 s (0.28-0.44), 25 ms (16-45) to the peak, -20 dB 129 ms
  (38-256) after it, centroid 1079 Hz (550-1505), nothing under 150 Hz, tonal share 0.62 (0.55-0.68), wide (L/R
  correlation 0.67, 0.33-0.99). The notes are gated: they stop within about 0.35 s, where a free bar rings 1.5-3 s.
- Short plucks (21 events, 8 films): 0.3 s (0.25-0.35), 49 ms (15-131) to the peak, t20 112 ms (104-150), centroid
  359 Hz (335-809), 12 % under 150 Hz, tonal share 0.58, mono. It sounds like a low, soft pluck of a damped string, or a bar hit with felt.
- Chime runs (13 events, 11 films): 0.35-1 s of discrete gated notes about 110 ms apart, centroid about 980 Hz.
- The older bank's kalimba rang 5 times too long (1.7-2.7 s) and was mono. Faded to 0.33 s, the audit called it
  truncated.

Model:
- Note (style `note`): the partial set of a bar, on f0. Every partial has its own decay, and the higher ones decay
  sooner. Mallet hardness controls the strength of the upper partials and the brightness of the contact noise. A
  bar bends in 2 planes, which gives a slow beat. The rise is soft. A damper engages at `gate` s: from then on all
  partials decay with `gate_tau`, so the note ends within some tens of ms rather than fading away. Optionally a
  second bar sounds an octave away (film scores often double a glockenspiel this way). The finish then applies band
  limits, width, a pan per note and a small room.
- Pluck (style `pluck`): the harmonics 1-12 of a string, shaped by the pluck point, the upper ones dying faster, a
  soft finger contact that low-passes them, the same damper, and a body resonance.
- Run: several gated notes on a scale, panned note by note, the last one accented and allowed to ring longer.

| Field | Value |
| --- | --- |
| `TARGET` | `a short glockenspiel note` |
| `ALTS` | `a wine glass clink`, `a phone notification`, `a xylophone note`, `a triangle`, `a music box` |

The module has no `REFSET` and no `TARGETS`.

### 1.2 Bars

Each bar is a list of partials (ratio to f0, amplitude):

| Bar | Partials | Model |
| --- | --- | --- |
| `glock` | (1.0, 1.0), (2.756, 0.35), (5.404, 0.12), (8.933, 0.05) | glockenspiel, a free-free steel bar |
| `celesta` | (1.0, 1.0), (2.0, 0.05), (3.98, 0.22), (6.9, 0.05) | a steel bar over a resonator |
| `metal` | (1.0, 1.0), (3.9, 0.25), (9.9, 0.08) | a tuned metallophone bar (1 : 4 : 10) |
| `marimba` | (1.0, 1.0), (3.92, 0.3), (9.8, 0.08) | wood, the same tuning, dies faster |
| `tine` | (1.0, 1.0), (5.8, 0.12), (6.27, 0.2) | a clamped tine (music box, kalimba) |

### 1.3 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `style` | note, pluck | choice | note | struck bar or plucked string |
| `bar` | glock, celesta, metal, marimba, tine | choice | glock | partial set (note style) |
| `f0` | 150 - 3200 Hz | log | 1046.5 | pitch |
| `decay` | 0.05 - 1.6 s | log | 0.6 | time constant of the fundamental before the damper |
| `slope` | 0.3 - 1.6 | lin | 0.9 | tau ~ ratio^-slope |
| `hard` | 0 - 1 | lin | 0.5 | mallet hardness |
| `strike` | -60 - -6 dB | lin | -26 | contact noise relative to the note; -59 or less = off |
| `beat` | 0 - 6 Hz | lin | 1.2 | doublet beat |
| `onset` | 0.0005 - 0.06 s | log | 0.004 | soft rise to the peak |
| `gate` | 0.05 - 1.0 s | log | 0.2 | damper start |
| `gate_tau` | 0.005 - 0.08 s | log | 0.03 | damper time constant |
| `tilt` | 0.6 - 2.5 | lin | 1.4 | pluck: harmonic amplitudes k^-tilt |
| `pos` | 0.05 - 0.5 | lin | 0.18 | pluck: pluck point (fraction of the string) |
| `contact` | 0.0003 - 0.01 s | log | 0.003 | pluck: finger contact |
| `body` | -60 - 0 dB | lin | -14 | pluck: body resonance relative to the string; -59 or less = off |
| `body_f` | 90 - 400 Hz | log | 180 | pluck: body resonance pitch |
| `hp` | 60 - 800 Hz | log | 300 | finish high-pass |
| `lp` | 3000 - 16000 Hz | log | 12000 | finish low-pass |
| `width` | 0 - 1 | lin | 0.5 | stereo width |
| `pan` | -1 - 1 | lin | 0 | position |
| `room` | 0 - 0.35 | lin | 0.1 | room wet |
| `rt` | 0.1 - 0.8 s | log | 0.3 | room rt60 |
| `oct` | -60 - 0 dB | lin | -60 | octave double level; -59 or less = off |
| `oct_ratio` | 0.5, 2.0 | choice | 0.5 | octave under or over |
| `jit` | 0 - 1 | lin | 0.5 | take spread: at 1, decay about +-8 %, gate +-5 %, hardness +-0.04; the pitch never moves |

Fine keys: decay, slope, hard, strike, beat, onset, gate, gate_tau, tilt, pos, contact, body, body_f, hp, lp, width,
room, rt, oct. The character keys style, bar, f0, pan, oct_ratio and jit stay pinned.

`family_space(params)` = T15 pinned family space of (SPACE, DEFAULTS merged with params, the fine keys).

### 1.4 Family `mallet`

`FAMILY['mallet']` is a list of 4 full parameter dicts. `FAMILY_NAMES['mallet']` = `glock`, `pluck`, `celesta`,
`marimba`. Merged values:

| Key | `glock` | `pluck` | `celesta` | `marimba` |
| --- | --- | --- | --- | --- |
| style | note | pluck | note | note |
| bar | glock | glock | celesta | marimba |
| f0 | 1274 | 284.2 | 784 | 392 |
| decay | 0.5027 | 0.1234 | 0.7148 | 1.6 |
| slope | 0.8703 | 0.3789 | 0.4249 | 0.3 |
| hard | 0.6343 | 0.1259 | 0.5432 | 0.45 |
| strike | -14.89 | -49.8 | -49.98 | -16.89 |
| beat | 4.807 | 2.786 | 1.934 | 1.747 |
| onset | 0.00162 | 0.06 | 0.00548 | 0.004282 |
| gate | 0.08478 | 0.1256 | 0.08034 | 0.06156 |
| gate_tau | 0.0422 | 0.01994 | 0.04521 | 0.03 |
| tilt | 0.6 | 0.7913 | 1.4 | 1.4 |
| pos | 0.4314 | 0.07761 | 0.18 | 0.18 |
| contact | 0.00436 | 0.00563 | 0.003 | 0.003 |
| body | -21.52 | -40.86 | -14 | -14 |
| body_f | 360.3 | 125.1 | 180 | 180 |
| hp | 194.3 | 372.5 | 678.6 | 120 |
| lp | 6499 | 4270 | 8593 | 16000 |
| width | 0.3488 | 0.08427 | 0.8211 | 0.3335 |
| pan | 0 | 0 | 0 | 0 |
| room | 0.2474 | 0.07512 | 0.1831 | 0.1626 |
| rt | 0.4467 | 0.6068 | 0.4 | 0.6887 |
| oct | -2.483 | -60 | -60 | -60 |
| oct_ratio | 0.5 | 0.5 | 0.5 | 0.5 |
| jit | 0.4495 | 0.02373 | 0.4 | 0.4 |

Characters: `glock` = a gated glockenspiel note doubled an octave under. A struck bar alone stays too pure (tonal
share 0.8-0.95, the film bell notes are at 0.62) and too bright. The octave double 2.5 dB down puts all 7 features
inside the bell-note inter-quartile ranges. `pluck` = a felt-soft string pluck (5-6 of 7 features inside). `celesta`
= a wide G5 note (correlation about 0.5) gated at 80 ms. `marimba` = a dry wooden G4 note. The last 2 are purer than
the typical bell note.

Study targets (T15 format: lo, hi, scale), measured with T14:

| Key | Note targets | Pluck targets |
| --- | --- | --- |
| dur_s | 0.28, 0.44, 0.1 | 0.25, 0.35, 0.08 |
| attack_ms | 16, 45, 12 | 15, 131, 20 |
| t20_ms | 38, 256, 60 | 104, 150, 30 |
| centroid_hz | 550, 1505, 300 | 335, 809, 100 |
| lo150 | 0.0, 0.04, 0.05 | 0.02, 0.17, 0.08 |
| tonal | 0.55, 0.68, 0.1 | 0.4, 0.62, 0.1 |
| corr | 0.33, 0.99, 0.2 | 0.95, 0.99, 0.1 |

The 2 tables are public: the note table is `STUDY`, the pluck table is `STUDY_PLUCK`.

### 1.5 Building blocks

**M1 damper** (t, gate, gate_tau) = exp(-max(t - gate, 0) / max(gate_tau, 1e-4)). It multiplies every partial's own
decay, so the timbre stays the same while the damper stops the note.

**M2 take spread** (p, rng): j = jit x 3 standard normals. decay x= exp(0.08 j1). gate x= exp(0.05 j2). hard =
clip(hard + 0.04 j3, 0, 1). Every take is a played note, never a copy.

**M3 rise** (t, onset) = 0.5 - 0.5 cos(pi clip(t / max(onset, 1e-4), 0, 1)).

**M4 note** (p, rng, n, f0) -> mono, the strike at t = 0:
1. For each partial (q, a) of the bar: f = f0 q. Skip it when f >= 0.42 x 48000 (no draws). a' = a x q^(1.5 (hard -
   0.5)), so a harder mallet makes the upper partials louder. tau = max(decay x q^-slope, 0.003). b = beat x uniform(0.6,
   1.4). am = 1 - 0.3 (0.5 - 0.5 cos(2 pi b t)) when b > 0.01, else 1. Add a' exp(-t / tau) am sin(2 pi f t +
   uniform(0, 2 pi)). Per partial: the beat draw, then the phase draw.
2. Multiply by M1 and by M3.
3. pk = peak. When strike > -59: lo = 800 + 3000 hard. Add pk x 10^(strike / 20) x T8 burst(n, lo, min(4 lo,
   16000), tau = 0.0006 + 0.002 (1 - hard), rise = max(0.0002, 0.3 onset)).

**M5 pluck** (p, rng, n, f0) -> mono:
1. For k = 1..12: f = f0 k (1 + 0.0004 k^2) (a little stiffness). Stop at the first k with f >= 0.4 x 48000. a =
   k^-tilt x |sin(pi k pos)| / (1 + (2 pi f contact / 6)^2). tau = max(decay k^-slope, 0.004). Add a exp(-t / tau)
   sin(2 pi f t + uniform(0, 0.3)).
2. Multiply by M1 and by M3.
3. When body > -59: bd = P-SVF of y at body_f, peak gain 3.0, mode bandn. y = y + 10^(body / 20) x bd / peak(bd) x
   peak(y).
The pluck ignores bar, hard, strike and beat.

**M6 voice** (p, rng, n, f0): y = M5 when style is pluck, else M4, at f0. When oct > -59: add 10^(oct / 20) x the
same style at f0 x oct_ratio (its own draws). With 2 lines an octave apart the tonal share drops to about 0.6
without adding noise.

**M7 length** (p) = min(2.0, gate + 7 gate_tau + 0.02, max(onset + 7 decay, 0.06) + 0.02) s.

### 1.6 Render algorithm (one note)

1. p = M2(DEFAULTS merged with p).
2. n = N(M7(p)).
3. y = M6(p, n, f0).
4. Return T13 finish(hp, lp, room_wet = room, rt60 = rt, width, cut = 500, pan, size = 0.3).

### 1.7 The run builder

Scales (semitones): `penta` 0 2 4 7 9; `major` 0 2 4 5 7 9 11; `lydian` 0 2 4 6 7 9 11; `sus` 0 2 5 7 10.

Run space = the note space plus:

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `notes` | 2 - 8 | int | 4 | note count |
| `step` | 0.05 - 0.18 s | lin | 0.11 | note spacing |
| `swing` | 0 - 0.3 | lin | 0 | odd notes come (1 + swing) late |
| `scale` | penta, major, lydian, sus | choice | penta | scale |
| `pattern` | up, down, arc | choice | up | contour |
| `span` | 1 - 3 | int | 1 | scale degrees per note |
| `spread` | 0 - 1 | lin | 0.6 | pan spread note by note |
| `accent` | -6 - 6 dB | lin | 2 | last-note gain |
| `last_gate` | 1 - 4 | lin | 2 | last-note gate multiplier |
| `ramp` | -6 - 6 dB | lin | 0 | gain ramp from first to last note |

Run defaults = the note defaults plus the defaults above.

**Run frequencies** (public, `run_freqs(p)`): k = int(notes). idx_i = i x int(span), i = 0..k-1. `down` reverses
idx. `arc`: h = floor((k + 1) / 2); idx = the first h entries followed by the first k - h entries reversed (so an
even count repeats the top note). Semitones s = 12 floor(i / len(scale)) + scale[i mod len(scale)] for each i in
idx. f = f0 x 2^(s / 12).

**Run render** (public, `render_run(p, rng)`):
1. p = run defaults merged with p. fs = run frequencies, k = their count.
2. last = p with gate x last_gate.
3. Onset times: t_i = i x step x (1 + swing when i is odd, else 1).
4. n = N(t_(k-1) + M7(last) + 0.05). out = n x 2 zeros.
5. For each note i, in order:
   1. q = M2(last when i = k - 1, else p).
   2. g = 10^((ramp x i / max(1, k - 1) + (accent when i = k - 1, else 0)) / 20).
   3. pan_i = pan + spread x (s_i x (0.4 + 0.6 i / max(1, k - 1))), with s_i = +1 for odd i and -1 for even i.
   4. m = N(M7(q)). y = M6(q, m, f_i).
   5. st = T13 finish(y, hp, lp, width, cut 500, pan = clip(pan_i, -1, 1), end_db = -70, fade = 0.004), with no room
      (room_wet 0).
   6. Add g x st at sample N(t_i), cut at n.
6. out = T12 room(out, wet = room, rt60 = rt, size 0.3, width 0.3 + width).
7. P-TRIM at -60 dB, keep 0.01 s.
8. Multiply the last min(len, N(0.006)) samples by cos^2 of a ramp 0 -> pi/2.

Family `chime_run` (`RUNS['chime_run']`, names `RUN_NAMES['chime_run']` = `flourish`, `up4`), each the run defaults
merged with:
- `flourish`: f0 802.3, decay 0.06476, slope 0.4859, hard 0.3246, strike -51.63, beat 2.417, onset 0.00098, gate
  0.2162, gate_tau 0.04794, tilt 0.7552, pos 0.3692, contact 0.00257, body -19.32, body_f 400, hp 142.8, lp 3000,
  width 0.01723, room 0.09026, rt 0.2059, jit 0.5908, notes 3, step 0.04412, swing 0.06969, scale lydian, pattern
  arc, span 2, spread 0.1809, accent -4.474, last_gate 3.365. The chime run of the music films: 3 damped notes that rise and
  fall back, 44 ms apart, close to mono, all 7 features inside the inter-quartile ranges.
- `up4`: f0 1046.5, notes 4, step 0.11, span 2, gate 0.12, gate_tau 0.03. A plain run with the reference spacing of
  about 110 ms. Each note moves 2 scale degrees, because with steps of only 2-3 semitones the
  audit heard a single rising glide.

**Run recipe object** (public `RUN`), the same pattern as the pop version 2 and the typed run objects:

| Field | Value |
| --- | --- |
| module name | the mallet module's name |
| SPACE | the run space |
| DEFAULTS | the run defaults |
| render | the run render; its function name is `render_run`, which the forge render blocks name |
| TARGET | `a quick run of chime notes` |
| ALTS | `a music box melody`, `a harp glissando`, `a phone ringtone`, `a xylophone scale` |

### 1.8 Variations

Per take: the decay, gate and hardness spread (scaled by jit), the beat rate and phase of each partial, the contact
noise and the finish room. In a run, every note has its own spread and draws. The pitch never
moves.

## 2. Recipe `drip`

### 2.1 Sound

A drop of water landing in water (the "bloop"), modelled from the physics of a bubble.

Measured (film catalog, 29 drops of 9 music-led films at least 8 dB clear of the music; 51 events in 14 films, the
opening sound of 3 of those films): 0.19 s (0.11-0.33), 39 ms (16-66) from the first sound to the peak, -20 dB 129 ms
(90-188) after it, centroid 596 Hz (379-1045), 10 % under 150 Hz, tonal share 0.55 (0.48-0.61), near mono (0.95).
The catalog's glide value (frames of 43 ms) is 0, yet listeners hear a rising pitch. The rise is over within the
first few tens of ms. No older bank member is a drip: the pops are 3-5 times shorter and harder, and the older bloop rises slowly (+6
semitones over 55-75 ms), which the audit reads as a synth whip.

Physics (Minnaert, Longuet-Higgins): the impact on the surface makes a brief splash and traps a bubble. The bubble's
volume oscillates at f = 3.26 Hz m / R, with R its radius. While the cavity above it closes, the pitch goes up (most
of the rise within about 3 rtau). The ring grows, then decays through radiation and thermal losses (Q about 10-30).

Model: a sine that follows the rising pitch (f0 = the pitch when the bubble separates), a quiet 2nd harmonic from
the nonlinear oscillation that decays sooner, the cavity hiss that follows the pitch, an optional smaller daughter bubble, the
splash click, a soft low thump of the impact, then the finish (T13).

The rising pitch is intended. With the defaults it is complete within about 40 ms (rtau of 12 ms or less), so the
forge audit's check for rising glides (more than 4 semitones over 50 ms or longer) does not fire. A slower bubble
(a large drop, a bloop) rises 6-12 semitones over 50-100 ms and does fire the check, although it is a natural sound
and not a synth sweep. So
the module declares an audit allowance (2.6).

| Field | Value |
| --- | --- |
| `TARGET` | `a water drop falling into water, a drip` |
| `ALTS` | `a bubble pop`, `a mouse click`, `a wooden knock`, `a synth beep`, `a finger snap` |

The module has no `REFSET` and no `TARGETS`.

### 2.2 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `f0` | 250 - 1600 Hz | log | 520 | bubble pitch at the pinch-off |
| `rise` | 0 - 14 st | lin | 6 | pitch rise as the cavity closes |
| `rtau` | 0.003 - 0.03 s | log | 0.008 | rise speed; most of the rise within 3 rtau |
| `onset` | 0.001 - 0.06 s | log | 0.02 | ring build-up |
| `curve` | 0.6 - 2.5 | lin | 1.0 | build-up shape |
| `tau` | 0.015 - 0.12 s | log | 0.05 | ring decay (20 dB in 2.3 tau) |
| `h2` | 0 - 0.6 | lin | 0.25 | 2nd harmonic level |
| `hdec` | 1 - 3 | lin | 1.6 | 2nd harmonic decay exponent |
| `breath` | -60 - -2 dB | lin | -10 | cavity hiss relative to the ring; -59 or less = off |
| `bq` | 1 - 8 | log | 3 | hiss band peak gain |
| `daughter` | -60 - -3 dB | lin | -60 | smaller bubble level; -59 or less = off |
| `d_ratio` | 1.3 - 2.6 | lin | 1.8 | daughter pitch ratio |
| `d_delay` | 0.004 - 0.04 s | lin | 0.012 | daughter delay |
| `click` | -60 - -6 dB | lin | -24 | splash level; -59 or less = off |
| `click_hz` | 800 - 8000 Hz | log | 2500 | splash centre |
| `click_ms` | 0.2 - 3 ms | log | 0.6 | splash decay |
| `thump` | -60 - -6 dB | lin | -20 | impact thump level; -59 or less = off |
| `thump_f` | 70 - 220 Hz | log | 120 | thump pitch |
| `hp` | 60 - 300 Hz | log | 80 | finish high-pass |
| `lp` | 2500 - 14000 Hz | log | 6000 | finish low-pass |
| `room` | 0 - 0.35 | lin | 0.12 | room wet |
| `rt` | 0.06 - 0.5 s | log | 0.18 | room rt60 |
| `width` | 0 - 0.8 | lin | 0.15 | stereo width |
| `jit` | 0 - 1 | lin | 0.5 | take spread: at 1, pitch about +-3 %, rise +-10 %, decay +-8 % |

Fine keys: rtau, onset, curve, tau, h2, hdec, breath, bq, d_ratio, d_delay, click, click_hz, click_ms, thump,
thump_f, hp, lp, room, rt, width. The character keys f0, rise and daughter, and also jit, stay pinned.

`family_space(params)` = T15 pinned family space of (SPACE, DEFAULTS merged with params, the fine keys).

### 2.3 Family `drip`

`FAMILY['drip']` is a list of 5 full parameter dicts. `FAMILY_NAMES['drip']` = `drop`, `bubble`, `plink`, `bloop`,
`tiny`. Merged values:

| Key | `drop` | `bubble` | `plink` | `bloop` | `tiny` |
| --- | --- | --- | --- | --- | --- |
| f0 | 306.4 | 254.5 | 694.6 | 420 | 1150 |
| rise | 8.033 | 9.767 | 3.332 | 10 | 0.9682 |
| rtau | 0.00719 | 0.00977 | 0.01 | 0.025 | 0.004 |
| onset | 0.02135 | 0.01237 | 0.03436 | 0.02 | 0.006 |
| curve | 2.192 | 1.123 | 0.7727 | 1.0 | 1.296 |
| tau | 0.0411 | 0.03743 | 0.03 | 0.05 | 0.01727 |
| h2 | 0.09568 | 0.1299 | 0.1039 | 0.25 | 0.3661 |
| hdec | 1.765 | 1.755 | 2.57 | 1.6 | 1.6 |
| breath | -22.71 | -26.28 | -24.02 | -10 | -2 |
| bq | 6.109 | 1.0 | 1.697 | 3 | 3 |
| daughter | -3 | -3 | -8.628 | -6 | -60 |
| d_ratio | 2.271 | 1.976 | 1.607 | 2.2 | 1.8 |
| d_delay | 0.03833 | 0.02129 | 0.04 | 0.03 | 0.012 |
| click | -40.83 | -51.96 | -44.91 | -24 | -21.63 |
| click_hz | 2300 | 7393 | 1378 | 2500 | 4898 |
| click_ms | 2.804 | 0.4349 | 0.5007 | 0.6 | 0.6 |
| thump | -23.28 | -8.339 | -39.42 | -20 | -40 |
| thump_f | 164.8 | 155.3 | 121.8 | 120 | 120 |
| hp | 183 | 148 | 97.33 | 80 | 200 |
| lp | 10010 | 5147 | 4560 | 6000 | 12230 |
| room | 0.2097 | 0.1926 | 0.07537 | 0.12 | 0.0 |
| rt | 0.08963 | 0.1608 | 0.1075 | 0.18 | 0.12 |
| width | 0.164 | 0.297 | 0.1842 | 0.15 | 0.2436 |
| jit | 0.2181 | 0.7479 | 0.225 | 0.5 | 0.3 |

Characters: `drop` = the films' drop (5 of 7 features inside the inter-quartile ranges). `bubble` = a lower bubble
with its daughter (6 of 7). `plink` = a small high plink, deliberately higher and purer than the typical drop (4 of 7).
`bloop` = a big drop's slow bubble; its climb (about 5 semitones over 0.1-0.15 s) needs the audit allowance. `tiny` =
a 1.15 kHz droplet of about 0.1 s. Without the cavity hiss (breath -22 to -26 dB) the tail would sound like a comb ring.

Study targets (T15 format: lo, hi, scale), measured with T14:

| Key | lo | hi | scale |
| --- | --- | --- | --- |
| dur_s | 0.11 | 0.33 | 0.08 |
| attack_ms | 16 | 66 | 15 |
| t20_ms | 90 | 188 | 30 |
| centroid_hz | 379 | 1045 | 200 |
| lo150 | 0.02 | 0.25 | 0.1 |
| tonal | 0.48 | 0.61 | 0.1 |
| corr | 0.87 | 0.99 | 0.1 |

### 2.4 Bubble (B1)

Inputs: p, rng, n, f0, tau, rise. Output: mono.

1. e = T1 soft envelope(onset, tau, hold 0, curve).
2. f = T4 rising track(f0, rise, rtau).
3. y = T6 tone on f with one part (ratio 2.0, amp h2, decay hdec) and envelope e.
4. When breath > -59: y += 10^(breath / 20) x T7 pitch-following noise(n, centre f, q = bq, envelope e).

### 2.5 Render algorithm

1. p = DEFAULTS merged with p.
2. Take spread: j = jit x 3 standard normals. f0 x= exp(0.03 j1). rise x= exp(0.1 j2). tau x= exp(0.08 j3).
3. The daughter is on when daughter > -59.
4. n = N(min(1.0, onset + 7 tau + (d_delay when the daughter is on, else 0) + 0.02)).
5. y = B1(p, n, f0, tau, rise).
6. Daughter on: k = N(d_delay). Add 10^(daughter / 20) x B1(p, n - k, f0 x d_ratio, 0.6 tau, 0.6 rise) from sample k.
7. pk = peak(y).
8. When click > -59: y += pk x 10^(click / 20) x T8 burst(n, lo = click_hz / 2, hi = 2 click_hz, tau = click_ms /
   1000, rise 0.0002).
9. When thump > -59: y += pk x 10^(thump / 20) x T9 soft thump(n, thump_f, tau 0.012, rise 0.002). No draw.
10. Return T13 finish(hp, lp, room_wet = room, rt60 = rt, width, cut = 400), other T13 inputs at their defaults.

### 2.6 Audit allowance

Public constants:

| Name | Value | Meaning |
| --- | --- | --- |
| `RISES` | true | the rising pitch is intended, so the audit must allow it |
| `RISE_MAX_ST` | 14.0 | the largest rise a bubble can produce (the space maximum plus the take spread) |
| `RISE_MAX_S` | 0.25 s | the rise must end within this time (an exponential approach keeps creeping, so the window is generous) |
| `RISE_START_S` | 0.06 s | the rise must begin this early: it comes from the bubble separating at the onset, not from a later sweep |

`allows(glide)`: the input is the result of the forge audit's rising-glide measurement (fields `rise_st`, `from_s`,
`to_s`). Return true when glide is empty or none, or when `from_s` is none. Else return true only when rise_st <=
RISE_MAX_ST, from_s <= RISE_START_S and to_s - from_s <= RISE_MAX_S. A climb outside these limits stays a warning.

`check(x, role = none, sound = none)`: the forge audit of x with category `drip` and the given role and sound. The
forge audit applies `allows` to the `drip` category itself, so this is a shortcut.

### 2.7 Variations

Per take: the pitch, rise and decay spread (scaled by jit), the tone phases, the cavity hiss, the daughter's own
draws, the splash and the finish room.

## 3. Recipe `pluck`

### 3.1 Sound

One plucked tine of a kalimba (thumb piano), modelled from physics. The model has one part per physical element:
- Tine: a steel beam clamped at the bridge, with a free tip. A beam fixed at one end has strongly inharmonic bending
  modes; Euler-Bernoulli theory gives the ratios 1 : 6.27 : 17.55 : 34.39. On real tines the tuning mass and the
  bent tip move the 2nd mode to about 5-7 times f0. Mode 1 is the pitch heard. Mode 2 is the bright metallic ping at
  the start; it decays far sooner, because air and bridge losses rise with frequency (tau scales roughly as f^-0.9).
- 2 polarisations: the tine vibrates vertically and, slightly stiffer, horizontally. No pluck is purely vertical, so
  each note holds 2 nearby modes `beat_hz` apart, which gives a real kalimba its slow shimmer.
- Geometric nonlinearity: a big deflection lengthens the tine slightly. That adds a quiet 2nd harmonic whose level
  tracks the square of the envelope, so it decays at twice the rate.
- Thumb: the thumb presses the tine down for some milliseconds, then the tine slides free. That release is not
  instantaneous (`thumb_ms`), so the high modes receive less energy (the excitation is low-passed). Added on top: a
  little noise of skin or nail at the slip, a brief metallic buzz when the tine snaps free and rattles against the
  bridge, and a low thud as the thumb pushes the instrument against what it rests on.
- Box: the tine moves the bridge, and the bridge moves a wooden box. The box has a Helmholtz resonance (from the
  sound hole) and many brief plate modes of the wood (150 Hz-2.5 kHz, 3-30 ms) that colour the start of the note.
- Sympathetic tines: the bridge also drives the other tines a little, most strongly those whose pitch matches a
  partial of the played note (octaves, fifths). They build up slowly and leave a faint chord under the note.
- A small room, a microphone close to the instrument, a floor of room tone, and slight stereo width.

| Field | Value |
| --- | --- |
| `TARGET` | `a kalimba note, a thumb piano pluck` |
| `ALTS` | `a music box`, `a xylophone note`, `a glockenspiel`, `an electric piano note`, `a synthesizer pluck` |
| `REFS` | 5 recorded kalimba notes under the reference root, sub-folder `kalimba` (data asset, see the seeds part) |
| `REF_F0` | 392.0, 440.0, 493.9, 587.3, 659.3 Hz (the measured notes of the 5 references: G4 A4 B4 D5 E5, in REFS order) |
| `LEN` | 3.0 s (the render buffer) |

The module has no `REFSET`, no `TARGETS` and no family. The bank file `bank/pluck.json` holds its 3 tuned members
(seeds part).

Sympathetic tines (`TINES`): a 17-tine kalimba in C, C4 to E6: 261.63 x 2^((12 o + s) / 12) for o = 0, 1, 2 and s in
the major scale 0 2 4 5 7 9 11, the first 17 values in that order.

`note(name, a4 = 440)`: a note name to Hz. The first letter (C D E F G A B, any case) gives 0 2 4 5 7 9 11. Then any
number of `#` (+1) or `b` (-1). Then the octave as an integer. f = a4 x 2^((s + 12 (octave + 1) - 69) / 12). So `C5`
= 523.25 Hz and `A4` = 440. An unknown letter or a missing octave is an error.

### 3.2 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `p2_ratio` | 5 - 7 | lin | 5.0 | 2nd bending mode / f0 (ideal bar 6.27) |
| `p2_lvl` | 0.02 - 0.6 | log | 0.386 | 2nd mode level before the thumb low-pass |
| `p3_lvl` | 0 - 0.25 | lin | 0.2038 | 3rd mode level (2.8 x the 2nd mode) |
| `tau1` | 0.35 - 2.4 s | log | 0.3529 | fundamental decay at C5; higher notes shorter, about f^-0.5 |
| `tau_exp` | 0.5 - 1.4 | lin | 1.309 | upper modes: tau ~ tau1 (f / f0)^-tau_exp |
| `beat_hz` | 0.2 - 4 Hz | log | 2.759 | polarisation split |
| `beat_lvl` | 0 - 0.6 | lin | 0.01985 | 2nd polarisation level |
| `h2_lvl` | 0 - 0.25 | lin | 0.1751 | nonlinear 2nd harmonic |
| `thumb_ms` | 0.1 - 4 ms | log | 0.3572 | release contact width; soft flesh = long = darker |
| `zing_lvl` | 0 - 0.4 | lin | 0.09041 | release slap and bridge rattle |
| `zing_f` | 2500 - 12000 Hz | log | 6908 | buzz band |
| `zing_tau` | 0.004 - 0.06 s | log | 0.006356 | buzz decay |
| `skin_lvl` | 0 - 0.25 | lin | 0.2027 | skin or nail slip noise |
| `skin_f` | 1500 - 9000 Hz | log | 1500 | slip noise band |
| `helm_f` | 160 - 420 Hz | log | 197.2 | box Helmholtz resonance |
| `helm_tau` | 0.008 - 0.06 s | log | 0.01108 | Helmholtz decay |
| `box_lvl` | 0 - 0.8 | lin | 0.1924 | share of the wooden box |
| `box_lo` | 150 - 600 Hz | log | 545.6 | lowest plate mode |
| `box_tau` | 0.003 - 0.03 s | log | 0.01542 | plate mode decay at box_lo |
| `knock_lvl` | 0 - 0.8 | lin | 0.5643 | low thud of the instrument |
| `knock_f` | 40 - 160 Hz | log | 77.94 | thud pitch |
| `knock_tau` | 0.008 - 0.08 s | log | 0.03731 | thud decay |
| `symp_lvl` | 0 - 0.25 | lin | 0.02239 | sympathetic tines |
| `symp_rise` | 0.005 - 0.08 s | log | 0.07959 | sympathetic ring-up |
| `room_wet` | 0 - 0.35 | lin | 0.2094 | room wet |
| `rt60` | 0.15 - 0.9 s | log | 0.4881 | room rt60 |
| `width` | 0 - 0.5 | lin | 0.0 | stereo width |
| `air_db` | -9 - 0 dB | lin | -7.11 | microphone top at 10 kHz |
| `floor_db` | -88 - -58 dB | lin | -64.71 | room-tone floor relative to the peak |
| `rumble` | 0 - 0.8 | lin | 0.3376 | share of low rumble in the room tone (a real recording always has some background) |
| `f0` | not in the space | - | absent | note pitch in Hz; absent or 0 = pick a reference pitch |

The defaults are the pick of a forge run against a launch film's kalimba.

### 3.3 Building blocks

t = k / 48000. pk is a peak taken at the step that says so.

**P1 mode** (t, f, tau, a, attack): ph = uniform(0, 2 pi). y = a exp(-t / tau) sin(2 pi f t + 0.15 ph). The phase is
near 0 because the tine is released from rest. k = max(2, N(attack)). When k < len(y), multiply the first k samples
by 0.5 - 0.5 cos of k points from 0 to pi.

**P2 tine** (p, f0, n):
1. T = thumb_ms / 1000. lp(f) = 1 / (1 + 0.5 f T). tau1' = tau1 x (f0 / 523.25)^-0.5. att = max(0.35 T, 0.0002).
2. y = P1(f0, tau1', 1.0).
3. When beat_lvl > 0.005: add P1(f0 + beat_hz, 0.85 tau1', beat_lvl).
4. f2 = p2_ratio f0. Add P1(f2, tau1' (f2 / f0)^-tau_exp, p2_lvl lp(f2) / lp(f0)).
5. f3 = 2.8 f2. When p3_lvl > 0.005 and f3 < 0.45 x 48000: add P1(f3, tau1' (f3 / f0)^-tau_exp, p3_lvl lp(f3) /
   lp(f0)).
6. f4 = 5.5 f2 (the 4th bending mode, 34.4 / 6.27). When p3_lvl > 0.005 and f4 < 0.45 x 48000: add P1(f4, tau1' (f4
   / f0)^-tau_exp, 0.5 p3_lvl lp(f4) / lp(f0)).
7. When h2_lvl > 0.005: add P1(2 f0, tau1' / 2, h2_lvl).
Every P1 call uses att and draws 1 phase.

**P3 box impulse response**: length N(0.25). ir[0] = 1. Add 0.9 sin(2 pi helm_f t) exp(-t / helm_tau) x 2 pi helm_f /
48000 x 6. Draw 18 frequencies fi = exp(uniform(ln box_lo, ln 2500)) (one vector draw). For each fi: tau = box_tau
(fi / box_lo)^-0.5; add uniform(0.2, 1.0) x sin(2 pi fi t + uniform(0, 6.3)) x exp(-t / tau) x 2 pi fi / 48000 x 2.
Per mode: the amplitude draw, then the phase draw.

**P4 sympathetic tines** (p, f0, n): zeros with no draw when symp_lvl < 0.003. Else rise = 1 - exp(-t / symp_rise).
For each tine frequency ft: skip it (no draw) when |ft / f0 - 1| < 0.03 (the played tine). cents = the minimum over
h in 0.5, 1, 1.5, 2, 3, 4 of |1200 log2(ft / (h f0))|. a = symp_lvl x (0.15 + exp(-(cents / 30)^2)) x uniform(0.5,
1.0). tau = 1.2 x tau1 x (ft / 523.25)^-0.5. Add a sin(2 pi ft (1 + 0.0015 g) t + uniform(0, 6.3)) exp(-t / tau) x
rise. Per tine: the amplitude draw, the normal, the phase draw.

**P5 skin** (p, n): m = N(0.02). w = m normals x exp(-t / 0.003), band-passed (order 2) from skin_f / 2 to min(2
skin_f, 0.45 x 48000), normalised, at the start of n zeros.

**P6 knock** (p, n): m = min(n, N(0.4)). f = knock_f (1 + 0.05 g). y = sin(2 pi f t) exp(-t / knock_tau) (1 -
exp(-t / 0.004)) + 0.5 sin(2 pi 2.3 f t) exp(-t / (0.5 knock_tau)) (1 - exp(-t / 0.002)). Normalise, at the start
of n zeros.

**P7 zing** (p, f0, n): m = min(n, N(0.4)). w = m normals band-passed (order 2) from 0.5 zing_f to min(1.8 zing_f,
0.45 x 48000). gate = max(0, sin(2 pi f0 t))^4, so the noise buzzes at the period of the note instead of hissing. y =
w (0.35 + gate) exp(-t / zing_tau). Normalise, at the start of n zeros.

**P8 room tone** (n, rumble): w = pink noise (P-NOISE) of n samples. lo = brown noise (P-NOISE) of n samples,
low-passed at 120 Hz (order 2), scaled to RMS 1. Output (1 - rumble) w + 2 rumble lo.

### 3.4 Render algorithm

1. f0 = p's `f0` when it is present and not 0. Else one uniform choice from REF_F0 (1 draw), so a forge run compares
   like with like against the references.
2. n = N(LEN). pre = N(0.004).
3. tine = P2(p, f0, n - pre). pk = peak(tine).
4. body = tine convolved with P3, cut to len(tine), scaled to peak pk.
5. x = (1 - 0.5 box_lvl) tine + box_lvl body. At low pitches the tine radiates little sound by itself, so the box supplies the body of the note.
6. x += pk x P4.
7. x += skin_lvl x pk x P5 (always drawn).
8. x += knock_lvl x pk x P6 (always drawn; knock_lvl is 0 when absent).
9. When zing_lvl > 0.003 (0 when absent): x += zing_lvl x pk x P7.
10. Put pre samples of silence before x.
11. Stereo (P-STEREO, width, pan 0).
12. P-ROOM(rt60, size 0.3, damp 0.6, wet room_wet).
13. P-MIC(low cut 20 Hz, air_db, floor -140 dB, so no microphone noise floor).
14. Cut to n samples.
15. pk = peak(x). For each channel: add P8(n, rumble, 0 when absent) x pk x 10^(floor_db / 20). Each channel gets its
    own draws (pink, then brown).
16. P-TRIM at floor_db - 2 dB, keep 0.05 s.
17. P-FADE: in 0.0005 s, out min(0.4, 0.3 x the length in s).

There is no final normalisation. The caller (the forge finish or the bank) sets the level.

The render does not merge the defaults. Every space key except knock_lvl, zing_lvl and rumble must be in p. A
missing key is an error (see the index, open questions).

### 3.5 Variations

Per take: the reference pitch (when f0 is absent), every mode phase, the box plate modes, the sympathetic tine
levels, detunings and phases, the slip noise, the thud pitch, the buzz noise, the stereo delays, the room, and the
room-tone floor per channel.
