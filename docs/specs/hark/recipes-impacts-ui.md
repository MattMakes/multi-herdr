# recipes-impacts, part UI: click, toggle, ui_pop, shutter

This part belongs to the spec `recipes-impacts.md` (the index). The index defines the recipe module contract, the
parameter-space notation, the primitives from forge-core (`P-*` codes) and the shared conventions. Every number here
is a fact of the behaviour. Times are seconds unless a key ends in `_ms` (milliseconds). The sample rate is 48 000 Hz.

## 1. Recipe `click`

### 1.1 Sound

A click for user interfaces, designed for product and launch videos. It is a single clean event: bright, an
attack of 0-3 ms, and an abrupt end. It is a designed sound, not a physical model. The measured target of a professional UI click is: attack 0-3 ms,
time to -20 dB 6-16 ms, total length 30-60 ms, spectral centroid 5-10 kHz, spectral tilt +4 to +12 dB per octave,
mono. Partials with pitch at 5-8 kHz are fine (the click then sounds like a "tink" rather than a snap of noise). The "mouse" variant, used under
music, has a low body (centroid about 430 Hz, time to -40 dB about 37 ms). Real close-up recorded clicks are broader:
centroid 1-5 kHz, time to -40 dB 33-107 ms.

Four layers, each owns a band, and all peak together:

1. Transient: a noise burst 0.15-1.5 ms long, band-passed around `pitch`, with a squared exponential decay.
2. Tink: 1 to 3 damped partials at `pitch` times (1, `inharm`, `inharm`^1.6).
3. Body: a short damped sine at `body_f` with a small downward pitch glide.
4. Grit: a short band-passed noise tail (plastic texture). An optional second, quieter click (`rel`) follows.

Then the chain: high-pass, a de-harsh dip, light saturation, a tiny dark early-reflection space, an optional small
dark room tail, an end gate, a limiter, edge fades.

Module metadata:

| Field | Value |
| --- | --- |
| `TARGET` | `a clean bright user interface click` |
| `TARGETS` | `click` -> `a clean bright user interface click`; `pro_click` -> `a crisp high digital interface tick for a product video` |
| `ALTS` | `a coin dropped on a table`, `a mechanical keyboard key press`, `a camera shutter`, `a woodblock hit` |
| `REFSET` | `click` |
| `REFS` | 4 files under `<REF_ROOT>/ui/`: `ui_click_01.wav` .. `ui_click_04.wav` |

`REF_ROOT` is the reference-recording root (index, section "Reference root").

### 1.2 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `pitch` | 1200 - 9500 Hz | log | 6000 | centre of the tink partials and of the transient band |
| `bright` | 0 - 1 | lin | 0.5 | transient band top: 0 = about one octave around pitch, 1 = up to about 16 kHz |
| `snap_ms` | 0.15 - 1.5 | log | 0.4 | transient decay time constant (ms) |
| `tone` | 0 - 1 | lin | 0.6 | tink partials against noise snap |
| `partials` | 1 - 3 | int | 2 | count of tink partials |
| `inharm` | 1.25 - 2.8 | lin | 1.62 | ratio of the 2nd partial; the 3rd is `inharm`^1.6 |
| `tink_ms` | 1.5 - 25 | log | 5.0 | decay time constant of the 1st partial (ms) |
| `body` | 0.005 - 0.8 | log | 0.08 | body level relative to the transient |
| `body_f` | 150 - 1200 Hz | log | 600 | body frequency |
| `body_ms` | 1.5 - 12 | log | 5.0 | body release time constant (ms). Above 12 ms the body rings as a narrow line |
| `body_drop` | 0 - 0.5 | lin | 0.15 | body pitch drop, fraction |
| `grit` | 0 - 0.6 | lin | 0.15 | grit tail level |
| `grit_ms` | 2 - 30 | log | 6.0 | grit decay (ms) |
| `rel` | 0 - 0.9 | lin | 0.0 | level of a 2nd click (0 = none) |
| `rel_dt` | 0.008 - 0.09 | lin | 0.05 | delay of the 2nd click (s). 8-16 ms is a double tick, 25-90 ms a release click |
| `room` | 0 - 0.3 | lin | 0.06 | early-reflection wet share |
| `verb` | 0 - 0.35 | lin | 0.0 | small dark room-tail wet share |
| `rt` | 0.04 - 0.35 | log | 0.12 | room-tail RT60 (s) |
| `hp` | 80 - 2500 Hz | log | 200 | high-pass cutoff. Above 800 Hz the high-pass is 4th order, else 2nd order |
| `lp` | 7000 - 19000 Hz | log | 17000 | top roll-off (2nd-order low-pass) |
| `deharsh` | 0 - 8 dB | lin | 3.0 | depth of a dip at `deharsh_f` |
| `deharsh_f` | 2400 - 4200 Hz | log | 3200 | dip centre |
| `sat` | 0 - 9 dB | lin | 2.0 | tanh drive |
| `attack_ms` | 0.05 - 2.5 | log | 0.3 | onset ramp (ms) |
| `length` | 0.025 - 0.14 | lin | 0.06 | end gate, s after the onset |
| `release_ms` | 2 - 30 | log | 10.0 | gate release (ms) |
| `width` | 0 - 0.25 | lin | 0.05 | stereo width of the space and room only. The direct sound stays mono |

### 1.3 Shared UI layer functions

The recipes `toggle`, `keypress`, `typing`, `popin`, `ui_pop` and the motion recipe `hover` reuse these functions. The
clean implementation must expose them from the click module (or a shared UI-layer module) with these exact
semantics. The names below are descriptive. The clean side picks its own names.

**L1 band burst** (`seconds`, rng, `f_lo`, `f_hi`, attack `rise` = 0.0003, filter order = 2). Output length is
max(6 x `seconds`, 0.004) s. Envelope e(t) = (exp(-t / max(`seconds`, 5e-5)))^2. The first max(2, N(`rise`))
samples are multiplied by sin^2 of a ramp from 0 to pi/2. The source is white Gaussian noise, filtered by a
Butterworth band-pass (P-BUTTER) from lo = max(40, `f_lo`) to max(1.05 lo, min(21500, `f_hi`)). Multiply by e.
Normalise to a peak of 1.

**L2 damped partials** (`f0`, ratios, amps, `tau`, `seconds`, rng, glide = 0, `rise` = 0.0002). Partial k has
frequency f0 x ratio_k and decay time constant tau x ratio_k^-0.6. Render them with the modal bank (P-MODAL) at
jitter 0.004 and the given glide. Apply the same sin^2 onset ramp over max(2, N(`rise`)) samples. Normalise to a
peak of 1.

**L3 designed thump** (`f`, `seconds`, attack `rise`, hold `hold_s`, release `rel_s`, drop = 0.1, harm = 0.1, rng
or none). Instantaneous frequency f_i(t) = f x (1 + drop x exp(-t / max(rise + 0.5 hold_s, 0.002))). Phase is the
running sum of 2 pi f_i / SR plus a random start phase (uniform 0..2 pi when an rng is given, else 0). Signal =
sin(phase) + harm x sin(2 phase + 0.3). Envelope: a raised-cosine rise 0 -> 1 over a = max(2, N(rise)) samples, 1
until a + N(hold_s), then exp(-(t - t_h) / max(rel_s, 0.001)). Normalise to a peak of 1.

**L4 end gate** (x, `length`, `release`, start = 0). end = min(n, N(start + length)). rl = max(8, min(end - 1,
N(release))). Gain is 1 before end - rl, then a raised-cosine fall 1 -> 0 over rl samples ending at `end`. Truncate
the signal at `end`. Works on mono and stereo.

**L5 finish chain** (x, hp = 120, deharsh = 0, deharsh_f = 3200, sat = 0, lp = none).
1. High-pass Butterworth at hp, order 4 when hp > 800, else order 2.
2. When deharsh > 0.05: peaking EQ (P-PEQ) at deharsh_f, gain -deharsh dB, Q 1.2.
3. When lp is set: low-pass Butterworth order 2 at lp.
4. When sat > 0.05: tanh saturation (P-SAT) with drive `sat` dB and peak-level matching.

**L6 tiny space** (x mono, rng, wet, width = 0, size = 0.12, damp = 0.7). Returns stereo of the same length as x.
When wet <= 0.002 return x on both channels. Else: early reflections (P-EARLY, size, 14 taps, damp, width 1.0)
cut to n samples, low-passed at 7000 Hz (order 1). Let m be the per-sample mean of the 2 reflection channels. Set
er = m + 4 x width x (er - m). Scale er to RMS(x) x wet. Output = x on both channels + er.

**L7 click layers** (p, rng, pitch_scale = 1, level = 1, seconds = 0.16, send = false). Mono, onset at sample 0.
1. pitch = p.pitch x pitch_scale x exp(0.01 g), g standard normal.
2. Transient band: hi = pitch x 2^(0.5 + 1.6 bright), lo = pitch / 2^0.6. Transient = L1(snap_ms / 1000, lo, hi,
   rise = attack_ms / 1000). Add (1 - 0.5 tone) x transient at t = 0 to the main bus and to the send bus.
3. Tink: ratios (1, inharm, inharm^1.6) and amps (1, 0.55, 0.3), both cut to `partials` entries. Tink =
   L2(pitch, ratios, amps, tink_ms / 1000, seconds, glide 0.004, rise = max(0.0001, attack_ms / 1000)). Add
   1.4 x tone x tink to the main bus only.
4. Body, when body > 0.004: L3(body_f x pitch_scale^0.5, seconds, att = max(0.0004, attack_ms / 1000), hold 0,
   rel = body_ms / 1000, drop = body_drop, harm = 0.15). Add 1.5 x body x thump to the main bus only.
5. Grit, when grit > 0.005: g = L1(grit_ms / 1000, pitch x 0.5, min(16000, 2.5 pitch), att 0.0008, order 1), then
   multiply by sqrt(exp(-t / (grit_ms / 1000))) (a softer tail than squared). Normalise to peak 1. Add
   0.35 x grit x g at t = 0.0004 s to both buses.
6. Scale both buses by level / peak(main bus). Return the main bus, or (main, send) when `send` is true.

The send bus holds only the broadband part (transient and grit). The room tail is fed from it, so the tink partials
and the body do not ring on in the room. A room fed with the full signal draws narrow tonal lines through the click.

**L8 room tail** (stereo `st`, mono send `snd`, rng, p, size = 0.12, lowcut = 300). When p.verb <= 0.003 return st.
Gain g = sqrt(mean(st^2) / (mean(snd^2) + 1e-20)), or 0 when snd is all zero. Wet = algorithmic reverb (P-VERB) of
[snd, snd] x g with rt60 = p.rt, size, predelay 0.003, damp 0.75, wet = p.verb, dry 0, early level -12 dB,
width = min(1, 4 p.width), stereo output, low cut `lowcut`. Output = st + wet, zero-padded to the longer length.

**L9 family narrowing.** A family dict maps a key to either a fixed value or a tuple (lo, hi) or (lo, hi, scale).
- Narrowed space: a fixed value becomes a one-choice list [v]. A tuple replaces the range. The scale is the tuple's
  3rd item when present. Else it is `int` when the key is an int key in the full space, else linear.
- Family centre: start from the defaults. A fixed value replaces the default. A tuple sets the key to its centre:
  the geometric mean sqrt(lo x hi) for a log tuple, else the arithmetic mean. Round to an integer when the default
  is an integer.

### 1.4 Render algorithm

1. lvl = 10^(0.04 g). This is a small round-robin level jitter.
2. (x, snd) = L7(p, level = lvl, send = true).
3. When rel > 0.004: (y, ys) = L7 with body x 0.5, tone x 0.8, pitch_scale 1.18, level rel x lvl, send = true.
   dt = rel_dt x exp(0.05 g). Add y into x and ys into snd at dt.
4. x = L5(x, hp, deharsh, deharsh_f, sat, lp). snd = L5(snd, max(hp, 300), deharsh, deharsh_f, sat 0, lp).
5. Prepend 2 ms of silence to x and to snd.
6. st = L6(x, wet = room, width = width). st = L8(st, snd, p).
7. end = max(length, rel_dt + 0.015 when rel > 0.004 else 0). st = L4(st, end, release_ms / 1000, start 0.002).
8. Limit to a -1 dBFS ceiling (P-LIMIT).
9. Fade in 0.5 ms and fade out 2 ms (P-FADE).

Output: stereo, length 2 ms + `end` (about 27-160 ms).

### 1.5 Families

A family fixes some keys and narrows others. Keys a family does not name keep the full range. Format: `key=v` is a
fixed value, `key=lo..hi` a linear range, `key=lo..hi log` a log range.

Category `pro_click` (the bright digital tick of product videos; the measured reference has 2 short bright bursts
about 12 ms apart, 3-15 kHz, gated at about 25 ms). Every family keeps a body, because the held-out film clicks
carry energy at 150 Hz - 1 kHz.

| Family | Values |
| --- | --- |
| `tink` | verb=0; pitch=5000..8000 log; tone=0.4..0.8; partials=2..3; tink_ms=1.5..4 log; rel=0.35..0.8; rel_dt=0.009..0.015; body=0.06..0.18 log; body_f=600..1000 log; body_ms=2..5 log; hp=350..800 log; length=0.025..0.04; release_ms=2..8 log |
| `snap` | verb=0; hp=250..600 log; pitch=4000..7000 log; tone=0..0.3; tink_ms=1.5..3 log; bright=0.5..1.0; snap_ms=0.3..1.2 log; rel=0.3..0.7; rel_dt=0.009..0.016; body=0.12..0.3 log; body_f=500..900 log; body_ms=2..5 log; length=0.025..0.045; release_ms=2..8 log |
| `mouse` | verb=0; pitch=2500..4500 log; tone=0.1..0.4; tink_ms=1.5..3 log; body=0.35..0.8 log; body_f=300..600 log; body_ms=3..7 log; hp=100..200 log; attack_ms=0.3..1.5 log; rel=0; deharsh=0..3; lp=11000..16000 log; length=0.04..0.06; release_ms=4..12 log |
| `glass` | verb=0; pitch=7000..9500 log; tone=0.6..0.9; partials=3; tink_ms=2..5 log; rel=0; body=0.03..0.1 log; body_f=700..1200 log; body_ms=2..4 log; hp=500..1000 log; length=0.03..0.06; release_ms=4..12 log |
| `soft` | verb=0; hp=150..350 log; pitch=3500..5500 log; tone=0.3..0.7; tink_ms=1.5..4 log; rel=0; body=0.2..0.5 log; body_f=400..900 log; body_ms=2..6 log; deharsh=2..6; length=0.035..0.06; release_ms=4..12 log |

Category `click` (real close-up clicks; the measured band balance is 1-4 kHz at -3.7 dB, 4-12 kHz at -4.7 dB and
above 12 kHz at -29 dB of the total). Pitch is 1.5-4.5 kHz, no dip in the 1-4 kHz band, top roll-off 5-17 kHz,
short tink rings (at most 3 ms) and short bodies (at most 6 ms). `thock` is the dark bodied member, `tink` the bright
thin member, and `soft`, `snap`, `double` sit between them.

| Family | Values |
| --- | --- |
| `soft` | pitch=1800..3200 log; tone=0.05..0.3; tink_ms=1.5..3 log; bright=0.1..0.5; body=0.1..0.35 log; body_f=300..700 log; body_ms=2..5 log; rel=0; deharsh=0..2; grit=0.15..0.45; verb=0.08..0.3; rt=0.08..0.25 log; lp=8000..12000 log; length=0.08..0.12 |
| `thock` | pitch=1200..2200 log; tone=0..0.2; tink_ms=1.5..3 log; bright=0..0.3; body=0.5..0.8 log; body_f=120..260 log; body_ms=3..6 log; rel=0..0.08; rel_dt=0.025..0.04; hp=50..100 log; deharsh=0..2; grit=0.1..0.35; verb=0.05..0.25; rt=0.08..0.25 log; lp=5000..8000 log; length=0.09..0.13 |
| `tink` | pitch=3800..6000 log; tone=0.25..0.5; partials=2; tink_ms=1.5..3 log; bright=0.4..0.8; body=0.01..0.05 log; body_ms=1.5..3 log; rel=0; hp=400..900 log; deharsh=0..2; grit=0.1..0.4; verb=0.05..0.25; lp=13000..17000 log; length=0.06..0.1 |
| `snap` | pitch=2200..4000 log; tone=0..0.2; bright=0.3..0.7; grit=0.2..0.5; rel=0.2..0.6; rel_dt=0.008..0.016; body=0.03..0.2 log; body_ms=1.5..5 log; deharsh=0..2; verb=0.05..0.3; lp=10000..15000 log; length=0.06..0.1 |
| `double` | pitch=2000..3800 log; tone=0.05..0.3; tink_ms=1.5..3 log; rel=0.05..0.15; rel_dt=0.03..0.07; body=0.05..0.3 log; body_ms=2..6 log; deharsh=0..2; grit=0.1..0.4; verb=0.05..0.25; lp=9000..13000 log; length=0.09..0.13 |

Note: `thock` narrows `body_f` to 120..260 and `hp` to 50..100. Both lower bounds are below the full space minimum
(150 and 80). The narrowed space replaces the range, so a family range may leave the full range.

Helpers: `family_space(cat, fam)` = L9 narrowed space of the full space. `family_params(cat, fam)` = L9 centre of
the defaults.

### 1.6 Variations

Per render, from the rng: the 2 ms-lead click jitters its level by 10^(0.04 g) (about +-0.4 dB at 1 sigma), its
pitch by exp(0.01 g) (about 17 cents), the release click delay by exp(0.05 g), the modal phases and detune, and all
noise. The same seed and parameters give the same output (index, "Determinism").

## 2. Recipe `toggle`

### 2.1 Sound

A designed UI toggle or switch flip: 2 contacts 25-160 ms apart, one gesture. The lever leaves its detent (contact A)
and lands in the other detent (contact B). A viewer expects ON to rise or sit higher and OFF to fall or sit lower. A
neutral toggle keeps 1 pitch. Measured real switch flips last 0.19-0.45 s, centroid 1.7-5.8 kHz, time to -40 dB
90-180 ms. In some real flips the loud contact is the second one (one measured case: 160 ms after a softer first).
The idiom is taken from the pro click: bright, short, band-limited, a mono object, a hard end.

Layers: contact A (the click layers L7 at level `a_lvl`, pitch `pitch`), optionally a sliding friction noise in the
gap, contact B (level 1, every pitch multiplied by `dir`), an optional faint spring rattle after B. Then the
finish chain L5, the tiny space L6, the room tail L8 and the end gate L4.

| Field | Value |
| --- | --- |
| `TARGET` | `a small switch flipped on with two quick clicks` |
| `TARGETS` | `toggle` -> same text as `TARGET` |
| `ALTS` | `a gun being cocked`, `a pen cap snapping shut`, `a mechanical keyboard key press`, `a coin dropped` |
| `REFSET` | `toggle` |

### 2.2 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `gap` | 0.025 - 0.16 | log | 0.06 | time from contact A to contact B (s) |
| `dir` | 0.7 - 1.45 | log | 1.15 | B pitch / A pitch. Above 1 = on, below 1 = off |
| `a_lvl` | 0.12 - 2.5 | log | 0.45 | contact A level relative to B. Above 1, contact A is louder than B |
| `pitch` | 1600 - 8000 Hz | log | 4500 | click pitch of contact A |
| `bright` | 0 - 1 | lin | 0.5 | as click |
| `snap_ms` | 0.15 - 1.2 | log | 0.4 | as click |
| `tone` | 0 - 1 | lin | 0.5 | as click |
| `partials` | 1 - 3 | int | 2 | as click |
| `inharm` | 1.25 - 2.8 | lin | 1.62 | as click |
| `tink_ms` | 1.5 - 8 | log | 5.0 | as click. Longer rings draw narrow 3-8 kHz lines after B |
| `body` | 0.01 - 0.8 | log | 0.15 | body level, same patch for both contacts |
| `body_f` | 150 - 1200 Hz | log | 500 | as click |
| `body_ms` | 2 - 12 | log | 6.0 | as click |
| `body_drop` | 0 - 0.5 | lin | 0.15 | as click |
| `grit` | 0 - 0.6 | lin | 0.15 | as click |
| `grit_ms` | 2 - 30 | log | 6.0 | as click |
| `slide` | 0 - 0.12 | lin | 0.05 | friction noise level between the contacts. 0.3 already reads as a zip |
| `slide_f` | 1500 - 7000 Hz | log | 3500 | friction band centre |
| `spring` | 0 - 0.08 | lin | 0.0 | faint spring rattle level after B |
| `spring_f` | 2000 - 6000 Hz | log | 3500 | spring frequency |
| `room` | 0 - 0.3 | lin | 0.08 | early-reflection wet |
| `verb` | 0 - 0.35 | lin | 0.12 | room-tail wet |
| `rt` | 0.06 - 0.4 | log | 0.15 | room-tail RT60 |
| `hp_steep` | choice {0, 1} | choice | 0 | 1 = an extra 2nd-order high-pass before the finish chain (4th order total) and the rattle spring |
| `hp` | 80 - 500 Hz | log | 150 | high-pass |
| `lp` | 7000 - 19000 Hz | log | 17000 | low-pass |
| `deharsh` | 0 - 8 dB | lin | 3.0 | as click |
| `deharsh_f` | 2400 - 4200 Hz | log | 3200 | as click |
| `sat` | 0 - 8 dB | lin | 2.0 | as click |
| `attack_ms` | 0.05 - 2.0 | log | 0.3 | as click |
| `tail` | 0.03 - 0.2 | lin | 0.06 | time from B to the end gate |
| `release_ms` | 5 - 35 | log | 12.0 | gate release |
| `width` | 0 - 0.25 | lin | 0.05 | as click |

The click keys `rel`, `rel_dt` and `length` are not in this space. L7 does not read them.

### 2.3 Render algorithm

Buffer length 0.45 s. pre = 0.002 s. All "place" operations add into the buffer at a time offset.

1. lvl = 10^(0.03 g). gap' = gap x exp(0.04 g).
2. Contact A: (a, A_send) = L7(p, pitch_scale 1, level a_lvl x lvl, send true). Place a in the main bus and A_send in the send
   bus at pre.
3. Slide, when slide > 0.004: m = N(gap') samples of white noise, band-pass order 2 from slide_f / 1.6 to
   min(16000, 1.6 slide_f), multiplied by a ramp (k / (m - 1))^2 from 0 to 1. Normalise to peak 1, scale by
   slide x lvl, place in both buses at pre. The slide swells into B.
4. Contact B: (b, B_send) = L7(p with body_f x dir^0.5, pitch_scale dir, level lvl, send true). Place in both buses at
   pre + gap'.
5. Spring, when spring > 0.004. With hp_steep = 1 (a short noisy rattle): F_spring = spring_f x dir; sp = L2(F_spring, ratios
   (1, 1.37, 2.11), amps (1, 0.6, 0.35), tau 0.005, 0.06 s, att 0.001); nz = L1(0.006, F_spring / 1.8,
   min(16000, 1.8 F_spring), att 0.001, order 1); sp = 0.4 sp + nz (nz zero-padded to the length of sp); normalise to
   peak 1. With hp_steep = 0 (a tonal ring): sp = L2(spring_f, same ratios and amps, tau 0.018, 0.12 s, att 0.001).
   Place spring x lvl x sp in the main bus only, at pre + gap' + 0.002.
6. When hp_steep = 1: high-pass the main bus at hp, Butterworth order 2.
7. Main = L5(main, hp, deharsh, deharsh_f, sat, lp). Send = L5(send, max(hp, 300), deharsh, deharsh_f, 0, lp).
8. st = L6(main, room, width). st = L8(st, send, p, size 0.15).
9. st = L4(st, gap' + tail, release_ms / 1000, start pre).
10. Normalise st to a peak of -1.5 dBFS, then limit at -1 dBFS. The normalisation comes first so that the limiter
    does not squash a loud contact A down to contact B.
11. Fade in 0.5 ms, fade out 2 ms.

### 2.4 Families (category `toggle`)

The measured reference band balance is 150 Hz-1 kHz -12.6 dB, 1-4 kHz -4.4 dB, 4-12 kHz -7.2 dB, above 12 kHz
-22 dB of the total. Bodies are small, pitch is 2-4.5 kHz, the top rolls off at 8-14 kHz. In 4 of 5 families contact
A is the loudest (`a_lvl` above 1). `rocker` keeps the softer-first idiom. The tail values aim at a time to -40 dB of
about 80-110 ms. A tink `tone` above about 0.45 reads as a ding.

| Family | Values |
| --- | --- |
| `on` | dir=1.12..1.35 log; gap=0.045..0.08 log; a_lvl=1.7..2.5 log; pitch=2400..4200 log; body=0.015..0.07 log; body_f=450..900 log; body_ms=2..4 log; tink_ms=1.5..3 log; spring=0..0.04; hp_steep=1; tone=0.2..0.45; room=0..0.12; slide=0..0.06; hp=180..400 log; verb=0.06..0.22; rt=0.08..0.2 log; deharsh=0..2; lp=9000..14000 log; tail=0.07..0.12 |
| `off` | dir=0.72..0.88 log; gap=0.045..0.08 log; a_lvl=1.7..2.5 log; pitch=2800..4500 log; body=0.015..0.07 log; body_f=450..900 log; body_ms=2..4 log; tink_ms=1.5..3 log; spring=0..0.04; hp_steep=1; tone=0.2..0.45; room=0..0.12; slide=0..0.06; hp=180..400 log; verb=0.06..0.22; rt=0.08..0.2 log; deharsh=0..2; lp=9000..14000 log; tail=0.07..0.12 |
| `latch` | gap=0.08..0.12 log; a_lvl=1.15..1.6 log; body=0.03..0.08 log; slide=0.03..0.08; tone=0.1..0.4; room=0..0.12; body_f=320..600 log; body_ms=2..4 log; pitch=2000..3500 log; tink_ms=1.5..3 log; hp_steep=1; verb=0.08..0.2; rt=0.08..0.18 log; hp=150..300 log; spring=0.02..0.06; deharsh=0..2; lp=8000..12000 log; tail=0.09..0.13 |
| `micro` | gap=0.025..0.045 log; a_lvl=1.5..2.3 log; pitch=3200..4800 log; body=0.01..0.04 log; body_f=600..1100 log; body_ms=2..4 log; hp=250..500 log; room=0..0.12; tink_ms=1.5..3 log; slide=0; spring=0; hp_steep=1; verb=0.06..0.2; rt=0.07..0.15 log; lp=10000..13000 log; tail=0.07..0.11 |
| `rocker` | gap=0.06..0.1 log; a_lvl=0.45..0.8 log; pitch=2000..3500 log; tone=0.1..0.35; body=0.03..0.09 log; body_f=400..800 log; tink_ms=1.5..3 log; spring=0..0.03; hp_steep=1; hp=150..350 log; room=0..0.12; verb=0.05..0.2; rt=0.08..0.2 log; body_ms=2..4 log; deharsh=0..2; slide=0.02..0.08; lp=8000..12000 log; tail=0.08..0.13 |

The narrowed `hp_steep` = 1 is a fixed value (one-choice list). `family_space` and `family_params` follow L9.

### 2.5 Variations

Per render: level jitter 10^(0.03 g), gap jitter exp(0.04 g), the 2 per-contact pitch jitters inside L7, modal
phases, noise.

## 3. Recipe `ui_pop`

### 3.1 Sound

A soft, rounded mid-register UI pop: the most used effect in music-led product films. It uses the tonal building
blocks (part `recipes-impacts-tonal.md`, codes T1-T15).

The measured film pop (147 pops in 13 music-led films, each at least 8 dB clear of the music; medians with the
inter-quartile range): length 0.16 s (0.10-0.24), a soft onset of 33 ms (18-62) to the peak, 20 dB down 81 ms
(57-129) after the peak, centroid 615 Hz (475-830), about 5 % of the energy under 150 Hz, almost nothing over 4 kHz,
tonal share 0.55 (0.48-0.63): a pitched body with air in it, not a pure beep. Pitch is flat or falls 0 to 8
semitones. Mono (correlation 0.97). Pops come in runs about 114 ms apart (4.7 per 10 s). The older bank pops of the
source were 3-5 times harder at the onset, decayed 2 times faster and were purer (tonal 0.82-0.93). The `pop` and
`popin` recipes model a bubble or a designed chirp. This recipe models the film pop.

Model parts:
- Body: a sine. Its pitch begins `bend` semitones above `f0` and settles onto `f0` with time constant `ptau` (T3).
  When `sweep` > 0.001 the pitch instead falls linearly in log frequency over `sweep` s, shaped by `ease`. Soft 2nd and 3rd partials
  (`h2`, `h3`) die faster (`hdec`). An optional inharmonic 2nd mode (`m2` at `m2r` x f0, decay factor `m2dec`) adds a
  hollow tube or wooden body.
- Envelope: T1 with `onset`, `tau`, `hold`, `curve`, `dcurve`.
- Air: noise through a band-pass that tracks the pitch (`breath` dB relative to the body, Q `bq`, envelope power
  `bdec`; a power below 1 lets the air last longer than the tone). With the air the tonal share stays near 0.55. At
  the release a brief wide-band breath (`puff`, `puff_hz`, `puff_ms`).
- A quiet low thump (`thump`, `thump_f`) supplies the 5 % of energy below 150 Hz. An optional lip click (`click`,
  `click_hz`).
- Optionally a second pop, smaller (`dbl` dB, `dgap` s later, at `dratio` times the pitch).
- The finish T13: high-pass `hp`, low-pass `lp`, small room (`room`, `rt`), `width`.
- `jit`: each take moves the pitch, the decay and the onset a little, so a run of pops is not a set of clones.

| Field | Value |
| --- | --- |
| `TARGET` | `a soft rounded pop, a small bubble popping in a user interface` |
| `ALTS` | `a mouse click`, `a water drop`, `a wooden knock`, `a synth beep`, `a finger snap` |
| `REFSET` | none (the module has no refset field) |
| `STUDY` | target ranges for T15, see 3.5 |

### 3.2 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `f0` | 250 - 1200 Hz | log | 370 | landing pitch of the body |
| `bend` | -3 - 72 st | lin | 0.7 | semitones the pitch starts above f0. 40-72 = a designed chirp |
| `ptau` | 0.004 - 0.08 | log | 0.0075 | settle time constant (used when sweep is off) |
| `sweep` | 0 - 0.15 s | lin | 0.0 | log-linear fall length. Above 0.001 it replaces the glide |
| `ease` | 0.4 - 2.5 | lin | 1.0 | sweep easing. Above 1 falls fast first |
| `onset` | 0.004 - 0.07 | log | 0.0236 | soft rise time |
| `curve` | 0.6 - 2.5 | lin | 2.45 | rise shape |
| `hold` | 0 - 0.03 | lin | 0.0147 | hold at the top |
| `tau` | 0.004 - 0.09 | log | 0.041 | decay time constant |
| `dcurve` | 1 - 2.5 | lin | 2.12 | decay shape |
| `h2` | 0 - 0.8 | lin | 0.57 | 2nd partial amplitude |
| `h3` | 0 - 0.5 | lin | 0.35 | 3rd partial amplitude |
| `hdec` | 1 - 3 | lin | 1.0 | partial decay power |
| `inh` | 0 - 0.08 | lin | 0.0 | partial sharpening: 2nd at 2(1 + inh), 3rd at 3(1 + 2 inh) |
| `m2` | 0 - 0.8 | lin | 0.66 | inharmonic mode amplitude |
| `m2r` | 1.4 - 4.5 | lin | 1.72 | inharmonic mode ratio |
| `m2dec` | 1 - 4 | lin | 1.43 | inharmonic mode decay power |
| `breath` | -60 - -2 dB | lin | -14.0 | pitch-following air. At or below -59: off |
| `bq` | 1 - 8 | log | 3.6 | air band Q |
| `bdec` | 0.4 - 2 | lin | 0.6 | air envelope power |
| `puff` | -60 - -6 dB | lin | -40.0 | release breath level relative to the body peak. At or below -59: off |
| `puff_hz` | 600 - 5000 Hz | log | 4270 | puff band centre |
| `puff_ms` | 0.5 - 8 | log | 2.7 | puff decay (ms) |
| `thump` | -60 - -6 dB | lin | -15.7 | low thump level. At or below -59: off |
| `thump_f` | 70 - 200 Hz | log | 129 | thump frequency |
| `click` | -60 - -10 dB | lin | -60.0 | lip click level. At or below -59: off |
| `click_hz` | 1500 - 8000 Hz | log | 3500 | click band centre |
| `dbl` | -60 - 0 dB | lin | -60.0 | 2nd pop level. At or below -59: off |
| `dgap` | 0.03 - 0.12 | lin | 0.07 | 2nd pop delay (s) |
| `dratio` | 0.6 - 1.6 | lin | 0.8 | 2nd pop pitch ratio |
| `hp` | 60 - 300 Hz | log | 112 | high-pass |
| `lp` | 2500 - 12000 Hz | log | 3530 | low-pass |
| `room` | 0 - 0.35 | lin | 0.15 | room wet |
| `rt` | 0.06 - 0.35 | log | 0.1 | room RT60 |
| `width` | 0 - 1.5 | lin | 0.1 | decorrelated side above 400 Hz. 1.43 gives an L/R correlation near 0 |
| `jit` | 0 - 1 | lin | 0.5 | take variation: at 1, pitch about +-2 %, decay +-8 %, onset +-10 % (1 sigma) |

The defaults are the typical film pop: a round 370 Hz body with soft 2nd and 3rd partials and a near-octave mode at
1.72 x (tonal share about 0.5), a 24 ms soft rise held 15 ms, a damped decay, a 3.5 kHz low-pass, a quiet thump at
130 Hz, and air that lasts a little longer than the tone. Without the air only 3 bare partials remain in the tail, and a
comb check then reports a metallic ring at the pitch period.

Fine keys (free in a member's search space; every other key is pinned): `ptau`, `ease`, `onset`, `curve`, `hold`,
`tau`, `dcurve`, `h2`, `h3`, `hdec`, `inh`, `m2dec`, `breath`, `bq`, `bdec`, `puff`, `puff_hz`, `puff_ms`, `thump`,
`thump_f`, `click`, `click_hz`, `hp`, `lp`, `room`, `rt`, `width`. The character keys are `f0`, `bend`, `sweep`,
`m2`, `m2r`, `dbl`. `family_space(params)` = T15 pinned space of the full space, the defaults merged with params,
and the fine keys.

### 3.3 One pop (before the finish)

Inputs: p, rng, n samples, pitch f0, decay tau, gain = 1.

1. e = T1(t, onset, tau, hold, curve, dcurve).
2. Pitch track: when sweep > 0.001, u = clip(t / sweep, 0, 1) and f = f0 x 2^((bend / 12) x (1 - u)^ease). Else
   f = T3(t, f0, bend, ptau).
3. Parts: (2(1 + inh), h2, hdec), (3(1 + 2 inh), h3, 1.5 hdec), and when m2 > 0 also (m2r, m2, m2dec).
4. y = T6(f, parts, e).
5. When breath > -59: y += 10^(breath / 20) x T7(n, f, bq, e^bdec).
6. Return gain x y.

### 3.4 Render algorithm

1. p = defaults merged with the given p.
2. j = jit x 3 standard normals. f0 x= exp(0.02 j1), tau x= exp(0.08 j2), onset x= exp(0.1 j3).
3. Double is on when dbl > -59. gap = dgap when on, else 0.
4. Length = min(0.8, onset + hold + 7 tau + gap + 0.01) s.
5. y = one pop (n, f0, tau).
6. When double is on: k = N(gap). Add one pop (n - k, f0 x dratio, 0.9 tau, gain 10^(dbl / 20)) from sample k.
7. pk = peak of |y|.
8. When puff > -59: y += pk x 10^(puff / 20) x T8(n, puff_hz / 2.2, 2.2 puff_hz, puff_ms / 1000,
   rise = max(0.0006, 0.3 onset)).
9. When thump > -59: y += pk x 10^(thump / 20) x T9(n, thump_f, max(0.012, 0.6 tau), rise = max(0.002, 0.5 onset)).
10. When click > -59: y += pk x 10^(click / 20) x T8(n, click_hz / 1.6, 1.6 click_hz, 0.0008).
11. Return T13(y, hp, lp, room_wet = room, rt60 = rt, width, cut = 400). Output is stereo.

### 3.5 Study targets (`STUDY`)

Ranges for T15 on a T14 measurement:

| Key | lo | hi | scale |
| --- | --- | --- | --- |
| `dur_s` | 0.10 | 0.24 | 0.08 |
| `attack_ms` | 18 | 62 | 15 |
| `t20_ms` | 57 | 129 | 30 |
| `centroid_hz` | 475 | 830 | 200 |
| `lo150` | 0.0 | 0.14 | 0.1 |
| `tonal` | 0.48 | 0.63 | 0.1 |
| `corr` | 0.93 | 1.0 | 0.1 |

### 3.6 Palette (category `ui_pop`)

6 members in this order. Names: `soft`, `step`, `boop`, `blip`, `fall`, `wide`. Each member is the defaults with
the listed overrides.

| Member | Overrides |
| --- | --- |
| `soft` | none (the typical film pop; all 7 measured features fall inside the film ranges) |
| `step` | f0=466.16, bend=3.0, ptau=0.03 (a step higher, falling in) |
| `boop` | f0=357.1, bend=9.0, ptau=0.03, onset=0.025, curve=2.0, hold=0.0062, tau=0.026, dcurve=1.4, h2=0.61, h3=0.17, hdec=1.63, m2=0.79, m2r=3.69, m2dec=2.68, breath=-13.8, bq=7.0, bdec=0.4, puff=-51.0, puff_hz=4600, puff_ms=1.9, thump=-8.0, thump_f=76, click=-54.0, hp=95, lp=12000, room=0.115, rt=0.093, width=0.2 (a rounder boop falling 9 st) |
| `blip` | f0=889.1, bend=-3.0, ptau=0.08, ease=1.576, onset=0.01359, curve=2.5, hold=0.00311, tau=0.01052, dcurve=2.007, h2=0.3, h3=0.09244, hdec=1.107, inh=0.01188, m2=0.2913, m2r=4.115, m2dec=1.032, breath=-21.12, bq=2.26, bdec=1.076, puff=-46.31, puff_hz=4264, puff_ms=0.5636, thump=-36.45, thump_f=108.6, click=-45.55, click_hz=4013, dbl=-55.33, dgap=0.05156, dratio=0.7141, hp=60, lp=12000, room=0.0, rt=0.2241, width=1.341, jit=0.6717 (a bright blip from an effects-led film, channels fully decorrelated, L/R correlation about 0.1) |
| `fall` | f0=345.76, bend=7.36, ptau=0.0058, sweep=0.0894, ease=1.123, onset=0.0177, curve=1.658, hold=0.0006, tau=0.0225, dcurve=2.138, h2=0.671, h3=0.49, hdec=1.424, m2=0.53, m2r=4.5, m2dec=1.0, breath=-8.81, bq=2.61, bdec=1.89, puff=-47.5, puff_hz=5000, puff_ms=6.13, thump=-26.1, thump_f=81, click=-33.5, click_hz=4083, dbl=-29.6, dgap=0.113, dratio=1.155, hp=79.7, lp=11076, room=0.0, rt=0.076, width=0.64, jit=0.83 (an eased 7 st fall, dry because its room rang) |
| `wide` | f0=622.5, bend=-0.9783, ptau=0.08, sweep=0.00291, ease=1.822, onset=0.04115, curve=2.5, hold=0.00244, tau=0.03267, dcurve=1.844, h2=0.5426, h3=0.1129, hdec=1.054, inh=0.02561, m2=0.2683, m2r=4.5, m2dec=1.0, breath=-9.013, bq=3.179, bdec=0.5391, puff=-13.68, puff_hz=5000, puff_ms=1.019, thump=-46.85, thump_f=107.4, click=-10.1, click_hz=6070, dbl=-47.17, dgap=0.07519, dratio=0.6551, hp=67.89, lp=11620, room=0.00401, rt=0.1363, width=0.8105, jit=0.3896 (a wide mid pop with a lip click) |

The members were found by a random search plus a local search on the film numbers, then refined by the forge.

### 3.7 Variations

Per take: the 3 jitters of step 2 scaled by `jit`, all oscillator start phases, all noise, the room.

## 4. Recipe `shutter`

### 4.1 Sound

A camera shutter: a 2-stage mechanical click. Stage A is the release, stage B the curtain or mirror landing. It uses
the tonal building blocks T1-T15.

The measured film shutter (6 shutters of 5 films at least 8 dB clear of the music; 21 events in 9 films, on snap
zooms and whip-cuts, often stacked with a click): 0.19 s (0.16-0.38), 39 ms (16-170) from the first sound to the
peak (often the 2nd stage is the louder), 20 dB down 87 ms (20-123) after the peak, centroid 1032 Hz (624-2187), 7 %
under 150 Hz, 3 % over 4 kHz, tonal share 0.4 (0.3-0.52). It is darker and longer than any toggle of the older bank.

Mechanism (a focal-plane shutter with a mirror box):
- Stage A, the release, at `a_db` relative to stage B: a small latch opens. A bright contact (noise band-passed
  near `a_hz`, decay `a_ms`) over 3 stiff steel modes from `a_hz` (`a_modes`).
- The travel (`whirr` dB, `whirr_hz`): the curtain moves during the `gap` between the stages.
- Stage B, the landing at `gap`: the box modes from `b_hz` (decay `b_decay`, darker than A), struck by a contact
  of `b_contact` s; its contact noise (`b_noise`, `b_noise_hz`); the camera body's low thump (`body`, `body_f`,
  `body_ms`); a short spring rattle 4 ms after the slap (`rattle`, `rattle_hz`).
- The finish T13: `hp`, `lp`, small room (`room`, `rt`), `width`.

| Field | Value |
| --- | --- |
| `TARGET` | `a camera shutter click` |
| `ALTS` | `a door latch`, `a stapler`, `a mouse click`, `a light switch`, `a gun being cocked` |

### 4.2 Parameters

| Key | Range | Scale | Default | Meaning |
| --- | --- | --- | --- | --- |
| `gap` | 0.015 - 0.14 | log | 0.045 | release to landing (s) |
| `a_db` | -24 - 6 dB | lin | -6.0 | stage A level relative to stage B |
| `a_hz` | 1500 - 9000 Hz | log | 3500 | latch contact centre and lowest latch mode |
| `a_ms` | 0.2 - 3 | log | 0.8 | latch contact decay (ms) |
| `a_modes` | 0 - 1 | lin | 0.5 | latch modes relative to its contact noise |
| `whirr` | -60 - -6 dB | lin | -30.0 | travel noise. At or below -59: off |
| `whirr_hz` | 600 - 5000 Hz | log | 1800 | travel band centre |
| `b_hz` | 300 - 3000 Hz | log | 900 | lowest box mode |
| `b_decay` | 0.004 - 0.08 | log | 0.025 | box mode time constant (s) |
| `b_noise` | -40 - 0 dB | lin | -10.0 | stage B contact noise relative to its modes. At or below -39: off |
| `b_noise_hz` | 600 - 6000 Hz | log | 2000 | stage B noise centre |
| `b_contact` | 0.0002 - 0.003 | log | 0.0008 | stage B contact pulse length (s) |
| `body` | -60 - 0 dB | lin | -16.0 | camera body thump. At or below -59: off |
| `body_f` | 70 - 300 Hz | log | 150 | body frequency |
| `body_ms` | 4 - 50 | log | 14.0 | body decay (ms) |
| `rattle` | -60 - -6 dB | lin | -30.0 | spring rattle. At or below -59: off |
| `rattle_hz` | 2000 - 7000 Hz | log | 3500 | rattle centre |
| `hp` | 60 - 500 Hz | log | 90 | high-pass |
| `lp` | 3000 - 16000 Hz | log | 9000 | low-pass |
| `room` | 0 - 0.3 | lin | 0.1 | room wet |
| `rt` | 0.05 - 0.4 | log | 0.14 | room RT60 |
| `width` | 0 - 0.5 | lin | 0.06 | stereo width (above 600 Hz) |

Fine keys: every key except `gap` and `b_hz` (`a_db`, `a_hz`, `a_ms`, `a_modes`, `whirr`, `whirr_hz`, `b_decay`,
`b_noise`, `b_noise_hz`, `b_contact`, `body`, `body_f`, `body_ms`, `rattle`, `rattle_hz`, `hp`, `lp`, `room`, `rt`,
`width`). The character keys are `gap`, `a_db`, `b_hz`. Note that `a_db` is also a fine key in the source list.
`family_space(params)` = T15 pinned space with these fine keys.

Constant mode tables:

| Table | Entries (ratio, amplitude, decay factor) |
| --- | --- |
| Box modes (stage B) | (1.0, 1.0, 1.0), (1.62, 0.7, 0.7), (2.31, 0.5, 0.5), (3.17, 0.35, 0.35), (4.4, 0.2, 0.25) |
| Latch modes (stage A) | (1.0, 1.0), (1.71, 0.6), (2.63, 0.35); decay of mode k = 0.004 / ratio s |

### 4.3 Render algorithm

1. p = defaults merged with p.
2. Draw in this order: gap' = gap x exp(0.03 g); b_hz x= exp(0.03 g); a_hz x= exp(0.03 g).
3. tail = 7 x max(b_decay, body_ms / 1000 when body > -59 else 0). n = N(min(1.0, gap' + tail + 0.03)).
4. Stage A, ka = N(0.03): a = T8(ka, a_hz / 1.8, min(1.8 a_hz, 18000), a_ms / 1000, rise 0.0002). When a_modes >
   0.005: am = T10(ka, a_hz x latch ratios, latch amps, 0.004 / ratio, jitter 0.01); a += a_modes x am / peak(am).
   Normalise a to peak 1.
5. Stage B, kb = n - N(gap'): bm = T10(kb, b_hz x box ratios, box amps, max(0.0008, b_decay x decay factor),
   jitter 0.01). b = T11(bm, b_contact, bright 0.6), cut to kb, normalised to peak 1.
   - When b_noise > -39: b += 10^(b_noise / 20) x T8(kb, b_noise_hz / 2, 2 b_noise_hz, 0.001, rise 0.0002).
   - When body > -59: b += 10^(body / 20) x T9(kb, body_f, body_ms / 1000, rise 0.0008).
   - When rattle > -59: rr = T8(kb, rattle_hz / 1.6, 1.6 rattle_hz, 0.006, rise 0.002), delayed by
     s = min(kb, N(0.004)) samples (and cut to kb). b += 10^(rattle / 20) x rr.
6. y = zeros(n). Add 10^(a_db / 20) x a into the first ka samples. Add b into the last kb samples.
7. Whirr, when whirr > -59: kw = N(gap'). w = band-pass order 2 of kw white samples from whirr_hz / 1.7 to
   1.7 whirr_hz, multiplied by sin(pi u)^0.7 with u a ramp 0 -> 1 over kw samples. Add 10^(whirr / 20) x w / peak(w)
   into the first kw samples.
8. Return T13(y, hp, lp, room_wet = room, rt60 = rt, width, cut = 600).

The levels of stages A and B are set before the finish, so the louder stage depends on `a_db`. Default stage A is
6 dB under stage B.

### 4.4 Study targets (`STUDY`)

| Key | lo | hi | scale |
| --- | --- | --- | --- |
| `dur_s` | 0.16 | 0.38 | 0.08 |
| `attack_ms` | 16 | 170 | 20 |
| `t20_ms` | 20 | 123 | 30 |
| `centroid_hz` | 624 | 2187 | 300 |
| `lo150` | 0.01 | 0.24 | 0.1 |
| `tonal` | 0.3 | 0.52 | 0.1 |
| `corr` | 0.96 | 0.99 | 0.1 |

### 4.5 Palette (category `shutter`)

Names in order: `dslr`, `snap`, `mirror`, `film`, `quick`. Each member is the defaults with these overrides.

| Member | Overrides |
| --- | --- |
| `dslr` | gap=0.0391, a_db=-23.5, a_hz=5537, a_ms=0.5005, a_modes=0.3966, whirr=-9.218, whirr_hz=1085, b_hz=1433, b_decay=0.006, b_noise=-1.243, b_noise_hz=1355, b_contact=0.00221, body=-20.61, body_f=238.2, body_ms=19.41, rattle=-26.17, rattle_hz=5575, hp=92.91, lp=5091, room=0.2557, rt=0.2552, width=0.1069 |
| `snap` | gap=0.05821, a_db=-22.88, a_hz=3300, a_ms=2.964, a_modes=0.1957, whirr=-15.59, whirr_hz=5000, b_hz=2037, b_decay=0.004, b_noise=-2.783, b_noise_hz=2530, b_contact=0.00031, body=-10.33, body_f=156.8, body_ms=5.495, rattle=-48.36, rattle_hz=2581, hp=173.8, lp=9437, room=0.01662, rt=0.0526, width=0.234 |
| `mirror` | gap=0.06462, a_db=-18.51, a_hz=8113, a_ms=0.5578, a_modes=0.1115, whirr=-6.0, whirr_hz=688.7, b_hz=1511, b_decay=0.00622, b_noise=0.0, b_noise_hz=1468, b_contact=0.003, body=-33.33, body_f=300, body_ms=22.0, rattle=-26.59, rattle_hz=7000, hp=128.8, lp=6009, room=0.3, rt=0.374, width=0.09233 |
| `film` | gap=0.095, a_db=-12.29, a_hz=4200, a_ms=0.9, whirr=-14.0, whirr_hz=900, b_hz=720, b_decay=0.01103, b_noise=-7.79, b_noise_hz=600, b_contact=0.001728, body=-12.0, body_f=112.7, body_ms=26.32, rattle=-24.0, rattle_hz=4200, hp=84.56, lp=6500, room=0.3, rt=0.2, width=0.08 |
| `quick` | gap=0.021, a_db=-4.0, a_hz=3800, a_ms=0.2281, a_modes=0.4098, whirr=-40.0, b_hz=1250, b_decay=0.008, b_noise=-6.0, b_noise_hz=2200, b_contact=0.001477, body=-17.09, body_f=199.6, body_ms=8.0, rattle=-50.0, hp=150, room=0.09189, rt=0.2138, width=0.1 |

Character of each member: `dslr` sits inside all 7 profile ranges (distance 0.35): a quiet latch, then 39 ms of
travel, then the curtain hits a box that resonates at 1.4 kHz, with contact noise, in a small room. `snap` is a dry 2 kHz snap-zoom
shutter. `mirror` is a slower mirror box (65 ms travel). `film` travels 95 ms onto a 720 Hz box, with a thump of the
body and a rattling spring. `quick` puts only 21 ms between release and curtain; medium brightness, dry. The first 3 came from a
random plus local search on the reference profile. The last 2 came from a search from a hand-set corner, with every
candidate's takes audited.

### 4.6 Variations

Per take: gap, b_hz and a_hz each move by exp(0.03 g) (about 3 %), mode jitter 1 %, phases, noise, room.

