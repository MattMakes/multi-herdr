# forge-core part 5: knobs (aiming a bank sound at target numbers)

Index: [forge-core.md](forge-core.md). This part covers module `hark.forge.knobs`. A knob aims one sound of the
sound bank (unit bank-kit-sfx) at a requested length, apex time, pitch, decay, onset, brightness or stereo width. A
knob always works through the recipe's own parameters and a re-render. It never resamples or time-stretches the
audio. The bank's `render` calls `apply`, then applies its own level (peak and weight) after the knobs.

## Interface

| name | kind | meaning |
|---|---|---|
| `KnobError` | exception, subclass of `ValueError` | a knob the sound cannot take, or a target out of range. The message names the sound id and the reason. |
| `midi(pitch)` | function | MIDI number of a note name, or None when the text is not a note name. |
| `hz(pitch)` | function | Hz of a note name, a number, or text like `440hz`. Raises KnobError. |
| `note(f)` | function | nearest note name, with cents when 5 or more off: `E5`, `A4 +12c`. |
| `label(pitch)` | function | a canonical file-name label of a pitch. |
| `describe(entry)` | function | what the knobs can do on one bank entry. |
| `partial_hz(entry, x)` | function | the heard partial of a click or toggle render, or None. |
| `pitch_warning(edits)` | function | `{'code': 'pitch_off', 'msg'}` or None. |
| `validate(entry, dur=None, apex=None, pitch=None, decay=None, onset=None, bright=None, width=None)` | function | raises KnobError for what `apply` refuses, without a render. |
| `apply(entry, run, dur=None, apex=None, pitch=None, decay=None, onset=None, bright=None, width=None)` | function | `(raw render, edits)`. |
| `place_apex(x, pk, apex)` | function | `(x, note text)`. |
| `fit_length(x, dur, keep_s=None)` | function | `(x, note text)`. |

A bank entry is a dict with at least: `id` (text), `category`, `recipe` (the recipe module name), `params`
(complete parameters), optional `type` (`'fit'` for a fitted recording, unit fit), optional `model` (for a fit),
optional `pitch_hz` (a fit's measured note), optional `meta` (`dur_s`, `peak_s`, `pitch_hz` measured when the bank
was built).

`run(params_edit, fit_knobs)` renders the entry with the given parameter overrides and fit-knob values (the bank
unit provides it). The whoosh path calls `run(params_edit)` with one argument; the bank's renderer must accept both.

Helpers from other units: `bank.estimate_pitch(x, target_hz, span_st=...)` (the strongest clear spectral peak near a
target, or None); `bank.times(x)['peak_s']` (time of the loudest 5 ms, measured in 1 ms hops); `fit.auto.kind_of(model)`
(the kind of a fit model: `hybrid`, `sines`, `modal`, `spectral`, `gesture` ...).

### Pitch text

- Note names: a letter A-G (either case), 0-2 accidentals from `#`, `s` (a sharp, as in file names: `Cs5` = `C#5`)
  and `b`, then an octave that may be negative. MIDI = 12 (octave + 1) + pitch class + sharps - flats, with pitch
  classes C 0, D 2, E 4, F 5, G 7, A 9, B 11.
- `hz`: a bool raises. A number is used as Hz. A note name is 440 x 2^((midi - 69) / 12). Else text matching a
  decimal number with an optional `hz` suffix (any case, spaces allowed). Else raise with a message that lists the
  accepted forms. The result must be within 8 Hz to 20 kHz, else raise.
- `note(f)`: n = 69 + 12 log2(f / 440); k = round(n); cents = round(100 (n - k)). Names C, C#, D, Eb, E, F, F#, G,
  Ab, A, Bb, B; octave k // 12 - 1. Add ` <+cents>c` when |cents| >= 5.
- `label(pitch)`: f = hz(pitch); when within 5 cents of a note (|n - k| < 0.05), the note name with `#` replaced by
  `s` (so `C#5` and `Db5` both give `Cs5`); else `<f rounded to 2 decimals, compact>hz` (`445hz`).

## Behaviour

### `describe(entry)`

Returns `{'dur': 'shape' | 'param' | 'trim', 'apex': bool, 'pitch': spec or None, 'why': reason (only when pitch is
None), 'tonal': {decay, onset, bright, width: the parameter name or None}}`.

Length method:
- A fit: `param` with the fit length knob from FIT_LENGTH for its kind, else `trim`.
- A category in WHOOSH: `shape`.
- A category in LENGTH whose parameter is in the entry's params: `param`.
- Else `trim`.

`apex` is true only for the WHOOSH categories.

### Pitch spec of an entry

- A fit: requires `pitch_hz` (else "this fit has no measured note") and a kind in FIT_PITCH (else "a <kind> fit has no
  pitch knob"). Spec `{'param': knob, 'native_hz', 'note', 'range_hz': [f / 2^(12/12), f x 2^(12/12)]}` (one octave
  either way, 1 decimal).
- A category in UNTUNED: refuse with its reason text.
- No PITCH spec, or its parameter is not in the params: refuse ("its recipe has no pitch parameter").
- mode `pc` (pitch class): `{'param', 'mode': 'pc', 'key': int value, 'note': name of key mod 12}`.
- `partial` (clicks and toggles): the parameter is the centre of the tink partials and noise band, not the partial
  heard (for one click preset the parameter is 2454 Hz and the heard partial 2903 Hz). The heard partial is the note.
  Spec `{'param', 'mode': 'partial', 'param_hz'}` plus: when the entry's meta has no `pitch_hz` (an old bank file),
  `native_hz` None, `note` None, the parameter range, and `heard: 'measured when rendered'`; when meta `pitch_hz` is
  null, refuse ("no clear tonal partial near its tink"); else `native_hz` = heard, its note, and the range scaled by
  heard / param.
- `needs` (param, minimum): when the entry's value is under the minimum (a missing value counts as -1e9), refuse ("its
  tonal layer is off ..., it has no note to tune").
- Else `{'param', 'native_hz', 'note', 'range_hz'}`.

### `validate`

1. Tonal knobs given: each must be a finite number (a bool is refused) within TONAL_LIMITS, else raise with the unit
   name from TONAL_UNITS. The entry must have the knob (tonal spec below).
2. dur: a number in [0.01, 60] s. For a WHOOSH category, at most WHOOSH_MAX_S (4.4 s).
3. apex: a number; only WHOOSH categories (else raise with the advice to aim a riser with dur and align its end);
   0.01 <= apex <= WHOOSH_MAX_S - 0.1, and apex < dur - 0.02 when dur is given.
4. pitch: run the pitch planning (below), which raises on a refusal.

### Tonal spec

Refuse a fit entry (message: the knob does not exist for a fitted recording; only pitch and length apply), a category with no
TONAL entry for the knob (list the categories that have it), a parameter that is neither in the entry's params nor in
the recipe module's DEFAULTS, and a `styles` restriction that the entry's style (params over DEFAULTS) does not meet.
The recipe DEFAULTS cover a parameter added after the entry was built: it renders the entry unchanged.

### `apply`

1. validate.
2. pitch: plan the pitch (below). Merge its parameter and fit-knob updates. Record `edits['pitch']`. When the pitch
   mode is `partial`, land the partial by measurement (below) and merge its update.
3. Tonal knobs in the order decay, onset, bright, width: for each given knob, aim it (below), merge its update, record
   `edits[knob]`.
4. Render:
   - apex given, or dur given on a WHOOSH category: the whoosh path (below).
   - dur given on another category: the parameter path (below).
   - else one render `run(pe, fk)`.
5. When dur is given: fit the render to the exact length (`fit_length`, with keep_s = apex + APEX_KEEP when apex is
   given). Record `edits['dur']['fit']`. With apex: measure the peak of the fitted file and update `edits['apex']`
   with measured (4 decimals), err_ms (1 decimal), ok (|error| <= APEX_TOL).
6. When the pitch plan has a target: measure where PITCH says (`start` default, `end` = the second half of the
   render, None = not measured; a fit uses `start`). `measured_hz` (2 decimals or None), `cents_off` =
   round(1200 log2(measured / target)), `ok` = measured and |cents| <= PITCH_TOL_CENTS, or None when not measured.
7. When any tonal knob was given, measure the shipped file with the recipes' tonal `measure` (part 1) and set
   `measured` and `ok` for each.
8. Return (x, edits).

### Pitch planning

- mode `pc`: n = midi(pitch), else round(69 + 12 log2(hz / 440)). Update {param: n mod 12}. Info: param, key, note,
  native_key, mode text that says only the pitch class changes and the register stays.
- f = hz(pitch); the spec's range must hold f. For a partial with unknown heard pitch, check the parameter range,
  then return no update and info {target_hz, note, native_hz None, param, mode 'partial'}; apply lands it.
- Out of range: raise with the note, Hz, range in Hz and notes, and the sound's own note.
- st = 12 log2(f / native). Info {target_hz (3 decimals), note, native_hz, semitones (2 decimals), param}.
- A fit: knob value = 2^(-st / 12) for `size` (a modal fit gets smaller to go up), else st semitones. Return it as a
  fit-knob update; info `knobs`.
- partial with a known heard pitch: update param = value x f / native; info mode 'partial', params.
- Else update param = f, and each co-parameter that is a number (not a bool) scaled by f / native. Info params
  (3 decimals).

### Landing a partial (clicks and toggles)

k = the parameter, f = target, range from PITCH, p0 = the entry's value. Start value v = the planned value; when there
is none (heard pitch unknown), render once, measure the heard partial (strongest clear peak within 6 semitones of
the parameter); none found: raise "no clear tonal partial"; else record native_hz and v = p0 x f / heard.
Loop at most MAX_FIX + 1 times: clip v to the range, render, estimate the pitch near f. No pitch: keep the best so far
(or this value with no error) and stop. Cents c = 1200 log2(got / f). Keep the value with the smallest |c|. Stop when
|c| <= PITCH_TOL_CENTS / 2. Else v x= f / got. Record `params` {k: best} (3 decimals) and `renders`.

### Aiming a tonal knob

Spec = the tonal spec; k = its parameter, (lo, hi) its range. Base = recipe DEFAULTS, then entry params, then the
current updates. v0 = base[k]. Co-parameters scale by v / v0 (only when v0 > 0).

Measures (part 1 `_tonal.measure` on the render): decay = t20_ms, onset = attack_ms, bright = centroid_hz,
width = corr.

Coordinates (X, inverse of X, Y), chosen so the measure is near linear in the parameter:
- width: X = v^2, Y = (1 - c) / (1 + c) with c clamped into [-0.99, 0.999]. (Decorrelation grows as width^2.)
- a `lin` spec: X = v, Y = m.
- else log-log: X = ln max(v, 1e-9), Y = ln max(m, 1e-9).

Search:
1. Render at v0 and measure. best = that point.
2. Up to TONAL_FIX (4) times: stop when best is within tolerance. Propose v (below), clip to [lo, hi]. Stop when v
   equals a tried value (relative 1e-9). Render, measure, keep it as best when it is closer to the target.
3. Proposal:
   - When tried points lie both under and over Y(target): take the pair (one under, one over) with the smallest X
     distance; interpolate X linearly at Y(target); clip it into the bracket's inner 70 % (15 % off each end).
   - Else, when the knob is not width and the last two points have different values and different measures: the
     secant slope in (X, Y). Only when the slope has the knob's sense (positive, or negative for an `inverse` spec):
     extrapolate to Y(target), at most 0.5 (hi - lo) away for lin specs or ln 4 for log specs.
   - Else a first step from the last point (v1, m1):
     - width: 0.5 when v1 is about 0 or m1 is None; else sqrt(Y(target) / max(Y(m1) / v1^2, 1e-9)).
     - lin onset with a measure: v1 + 0.9 (target - m1) / 1000 (a soft rise of T s adds about 0.9 T to the attack).
     - other lin: v1 + 0.25 (hi - lo) x (1 when m1 is None, else sign(target - m1) x (-1 for inverse, else 1)).
     - log: v1 x (target / m1)^(+1, or -1 for inverse); ratio 2 when m1 is None or 0.
4. Info `{target, param, value (6 decimals), renders, search_measured (3 decimals or None)}`, plus `co` when
   co-parameters exist.

Tolerance: ok when |measured - target| <= max(abs, rel x |target|) with TONAL_TOL (rel, abs). A None measure is not
ok.

The bracket stops the search from drifting. When the peak of an envelope slides along a flat top, the measured value
jumps in steps, and a secant without a bracket can then move away from the answer.

### Length by a parameter

`trim` method: one render; info `{target, method 'trim', renders 1}`.
Else L0 = meta `dur_s`, or the length of one render. A fit searches its length knob value s from 1.0. A recipe
scales its length parameter v0 by s. The secant search (below) finds s. Info `{target, method 'param', 'knob' (fit) or
'param' (recipe), value (fit 4 decimals: s; recipe 5 decimals: v0 s), renders}`.

Secant search for `measure(make(s)) = target` from the known point (s0, L0):
1. tol = max(DUR_ABS, DUR_REL x target) unless given.
2. s = clip(s0 x target / max(L0, 1e-6), SCALE). Render and measure. best = it. The previous point is (s0, L0).
3. Up to MAX_FIX times: stop when within tol, or the measure did not move (|L - Lp| < 1e-4), or s equals the
   previous s. New s = clip(s + (target - L)(s - sp) / (L - Lp), SCALE). Render, measure, keep the closest.
4. Return the closest render, not the last: a noise render's length is not monotonic in the parameter.

### Whoosh path (dur and apex)

The whoosh recipe has params `rise`, `hold` (default 0), `fall`, `start_db`, `rise_k`. Its envelope peaks
analytically at tp = rise x (max(-start_db, 1) / 30)^(1 / rise_k) (the recipes-motion unit defines the envelope).

1. dur > WHOOSH_MAX_S: raise.
2. L0, pk0 = meta `dur_s` and `peak_s`; when either is missing, render once and measure them.
3. dur alone: secant search for a common scale s of rise, hold and fall (the pass keeps its shape). Info dur `{target,
   method 'shape', scale (4 decimals), renders}`.
4. With apex: k = (max(-start_db, 1) / 30)^(1 / rise_k); r1 = apex / k (the closed form); rise = r1. post_t = dur -
   apex when dur is given. q (scale of hold and fall) = 1, or clip(post_t / max(L0 - pk0, 1e-6), SCALE) with dur.
5. Loop: render with rise, hold x q, fall x q; measure the peak pk and length L. Error = |pk - apex| + (|(L - pk) -
   post_t| with dur). Keep the lowest error. Stop when |pk - apex| <= APEX_TOL and (no dur, or |L - dur| <=
   max(DUR_ABS, DUR_REL dur)), or after MAX_FIX + 1 renders. Else rise = clip(rise x apex / max(pk, 1e-3), 0.25 r1,
   4 r1); with dur, q = clip(q x post_t / max(L - pk, 1e-3), SCALE).
6. Take the best render and place its apex exactly (`place_apex`). Info apex `{target, measured (4 decimals), err_ms,
   search_err_ms, rise (5 decimals), renders, fit (the place note), ok}`; with dur also dur `{target, method 'shape',
   post_scale (4 decimals), renders}`.

Measured: the closed form lands within about 12 ms on the fast family. On the big families, a 10 % change of rise can
move the 5 ms peak of a wide, fluttering pass by up to 50 ms in either direction. So only the placement guarantees
APEX_TOL.

### `place_apex(x, pk, apex)`

The shift n = round((pk - apex) x SR / 48) x 48 samples (whole 1 ms hops of the peak measure; a shift between hops can
make a fluttering pass report a different one of its almost equal maxima, so the measured peak moves more than the
shift).
- n = 0: `exact`.
- n < 0 (peak early): pad -n zeros in front: `head pad <ms:.1f> ms`.
- n > 0: the head level = 20 log10(max |mono| over the first n samples / max |mono|). When it is above HEAD_MAX_DB
  (-20 dB): keep x, note `head kept: a <ms:.0f> ms trim would remove part of the pass (<lvl:.0f> dB re peak)`. Else drop
  the first n samples and fade in the first min(len, 3 ms) with a half raised cosine: `head trim <ms:.1f> ms`.

### `fit_length(x, dur, keep_s=None)`

n = round(dur x SR).
- Same length: `exact`.
- Shorter: pad zeros at the end: `pad <ms:.0f> ms`.
- Longer: cut to n. Fade length k = min(n, max(10 ms, min(150 ms, n / 4))). With keep_s: k = min(n, max(2 ms
  (FADE_MIN_S), min(k, n - round(keep_s SR)))). Fade with a half raised cosine falling from 1 to 0 over the last k
  samples. Note `trim <cut ms:.0f> ms`, plus ` (fade <k ms:.0f> ms)` when keep_s is given.

### `pitch_warning(edits)`

None unless `edits['pitch']['ok']` is exactly false. Message: `pitch <note>: measured <note of measured> (<Hz:.1f>
Hz, <cents:+d> cents); the knob did not land the note (tolerance 50 cents)`, or `no clear partial near it in the
render` when nothing was measured.

## Constants

| name | value | meaning |
|---|---|---|
| `SR` | 48000 | |
| `APEX_TOL` | 0.005 s | apex met within this |
| `APEX_KEEP` | 0.005 s | the dur fade starts at least this long after the apex |
| `PITCH_TOL_CENTS` | 50 | |
| `PARTIAL_SPAN_ST` | 6.0 | the heard partial search span around a click's pitch parameter |
| `DUR_REL`, `DUR_ABS` | 0.01, 0.005 s | length search tolerance |
| `MAX_FIX` | 2 | corrections after the first targeted render |
| `SCALE` | (0.15, 6.0) | a length key moves at most this far from the member's own value |
| `WHOOSH` | whoosh_fast, whoosh_big, pro_whoosh_big, swish, swipe | the pass-engine family |
| `WHOOSH_MAX_S` | 4.4 s | the pass engine renders at most 4.5 s |
| `HEAD_MAX_DB` | -20 dB | place_apex trims only a head this far under the peak |
| `FADE_MIN_S` | 0.002 s | |
| `FIT_PITCH` | hybrid: pitch, sines: transpose, modal: size | fit kind -> pitch knob |
| `FIT_LENGTH` | spectral: length, sines: stretch, gesture: length | fit kind -> length knob |
| `FIT_PITCH_ST` | 12 | a fit moves at most an octave |
| `TONAL_LIMITS` | decay (2, 3000) ms, onset (0.5, 300) ms, bright (50, 16000) Hz, width (-1, 1) | accepted targets |
| `TONAL_TOL` | decay (0.1, 3.0), onset (0.15, 3.0), bright (0.08, 30.0), width (0.0, 0.05) | (relative, absolute) |
| `TONAL_ORDER` | decay, onset, bright, width | |
| `TONAL_FIX` | 4 | |
| `TONAL_UNITS` | decay 't20 ms', onset 'attack ms', bright 'centroid Hz', width 'L/R correlation' | |

`LENGTH` (category -> length parameter):

| parameter | categories |
|---|---|
| `length` | riser, pro_riser, downlifter, sub_drop, pro_sub_drop, shimmer, pro_shimmer, pro_zip, click, pro_click, hover_tick, pro_hover_tick, tap, pro_tap, pro_pop_in, pop, pro_pop, notify, pro_notify, pro_typing_key, reverse_swell |
| `dur` | glitch, pro_glitch, pro_typing_run |
| `g_length` | pro_logo_sting |

Every other category uses trim (impact, toggle, typing_key, logo_sting, pluck, cut_hit and the struck fits). The
cut_hit `length` is a ceiling, so it stays trim.

`PITCH` (category -> param, co-parameters, range Hz, needs, measure, mode):

| category | param | co | range | needs | measure | mode |
|---|---|---|---|---|---|---|
| pluck | f0 | | 130-2100 | | start | |
| notify | f0 | | 220-3200 | | start | |
| pro_notify | f0 | | 600-3200 | | start | |
| click, pro_click | pitch | | 1200-9500 | | start | partial |
| toggle | pitch | | 1600-8000 | | start | partial |
| hover_tick, pro_hover_tick | ping_f | tick_f | 3000-15000 | ping >= 0.1 | start | |
| tap, pro_tap, pro_pop_in | body_f | knock_f, glass_f | 70-220 | | start | |
| pop | f0 | f_hi, f_lo, f_blip | 400-3500 | | start | |
| pro_pop | f0 | f_hi, f_lo, f_blip, n_f0, n_f1 | 280-1400 | | start | |
| sub_drop, pro_sub_drop | f1 | f0 | 28-62 | | end | |
| pro_zip | f1 | f0, fpk | 80-12000 | | end | |
| riser, pro_riser | tf0 | | 60-4000 | tone_db >= -40 | none | |
| downlifter | tf0 | | 150-2500 | tone_db >= -40 | none | |
| logo_sting | root | | 300-2200 | | start | |
| pro_logo_sting | g_root | root | 50-1800 | | start | |
| impact, pro_impact | body_f1 | body_f0, sub_f | 34-75 | | none | |
| pro_whoosh_big | sub_f | | 25-120 | sub_db >= -30 | end | |
| shimmer, pro_shimmer | key | | | | | pc (pitch class) |
| ui_pop | f0 | | 250-1200 | | start | |
| mallet | f0 | | 150-3200 | | start | |
| knock | f1 | | 250-2600 | | start | |
| drip | f0 | | 250-1600 | | none | |
| cut_hit | sub_f1 | sub_f0 | 36-130 | | none | |

`UNTUNED` (category -> reason, own words): whoosh_fast and whoosh_big (a noise pass-by whose band moves with the
motion), swish (a noise flick whose whistle follows the speed), swipe (friction noise), glitch and pro_glitch (a
digital stutter), typing_key and pro_typing_key (keys are not tuned; takes and seeds vary each press), pro_typing_run
(a typed run).

`TONAL` (knob -> category -> param, range, co, lin, inverse, styles):

| knob | category | param | range | other |
|---|---|---|---|---|
| decay | notify, pro_notify | ring | 0.05-4.0 | |
| decay | pluck | tau1 | 0.03-2.4 | |
| decay | ui_pop | tau | 0.006-0.2 | |
| decay | drip | tau | 0.008-0.25 | |
| decay | knock | decay | 0.002-0.2 | |
| decay | mallet | decay | 0.02-2.0 | co gate |
| decay | shutter | b_decay | 0.002-0.15 | co body_ms |
| onset | notify, pro_notify | soft | 0.0-0.12 | lin |
| onset | ui_pop | onset | 0.002-0.12 | |
| onset | drip | onset | 0.0005-0.12 | |
| onset | mallet | onset | 0.0003-0.12 | |
| bright | notify | hard | 0.0-1.0 | lin, styles: bell |
| bright | mallet | hard | 0.0-1.0 | lin, styles: note |
| bright | ui_pop | h2 | 0.0-1.2 | lin |
| bright | drip | h2 | 0.0-1.0 | lin |
| bright | shutter | b_hz | 300-3000 | |
| width | notify, pro_notify | width | 0.0-3.0 | lin, inverse |
| width | ui_pop, drip, knock, mallet, shutter, pluck | width | 0.0-1.5 | lin, inverse |

Knock has no bright knob: no single parameter controls its centroid, which depends on the nulls of the contact
spectrum and on the noise. The
decorrelator reaches a correlation of about 0 at width 1.43; notify halves its width internally, so its range is 2x.

## Errors and edge cases

- Every refusal is a KnobError with the sound id and a reason that a user can act on.
- A non-numeric or non-finite knob value: "<knob> must be a number".
- A pitch outside 8 Hz to 20 kHz, or outside the sound's range: refused before any render.
- A tonal knob on a fit, or on a category without it: refused.
- The partial landing stops when a render has no clear partial.
- `place_apex` refuses to trim into the pass (head above -20 dB) and reports it in the note; the apex then may miss,
  and `ok` says so.

## Acceptance tests

K1. `hz('A4')` = 440.0; `hz('Cs5')` = `hz('C#5')` = 554.37 (within 0.01); `hz('440hz')` = 440.0; `hz(True)` raises;
    `hz('H2')` raises; `hz(5)` raises (under 8 Hz).
K2. `note(622.25)` = `Eb5`; `note(452)` = `A4 +47c`; `label('Db5')` = `Cs5`; `label(445)` = `445hz`.
K3. `fit_length(x of 1.0 s, 0.5)` returns 24 000 samples, the last sample 0, the note `trim 500 ms`; with a 0.3 s
    signal the note is `pad 200 ms`.
K4. `fit_length(x, 0.5, keep_s=0.49)` uses a fade of at most 10 ms (and at least 2 ms).
K5. `place_apex(x, 0.30, 0.25)` on a whoosh whose first 50 ms are 30 dB under the peak trims 2400 samples and starts
    with a 3 ms fade-in; on a sound whose head is -10 dB it keeps x with a `head kept` note.
K6. `validate(entry of category 'riser', apex=0.5)` raises; `validate(whoosh entry, dur=5.0)` raises (over 4.4 s).
K7. `describe` on a typing_key entry gives `dur 'trim'`, `apex False`, `pitch None`, and `why` that says keys are not
    tuned.
K8. `apply(sub_drop entry, run, pitch='G1')` sets f1 to 49.0 Hz (within 0.01) and scales f0 by the same ratio.
K9. `apply(riser entry, run, dur=2.0)` returns a render of exactly 96 000 samples, with `edits['dur']['method']`
    `param`.
K10. `apply(notify entry, run, decay=150)` ends with `edits['decay']['ok']` true when the target is reachable inside
     the `ring` range, and uses at most 5 renders for the search.
K11. `pitch_warning({'pitch': {'ok': False, 'note': 'C5', 'measured_hz': 530.0, 'cents_off': 23}})` returns code
     `pitch_off`; with ok True or None it returns None.
