# hark fit: rendering, takes, knobs and morphs (part of fit.md)

This part specifies how the engine renders any model: the knob tables, the hybrid render, take mode, the bed and
space layers, the finish, the gate time map, and morphs between 2 fits. The fitting side is in `fit-auto.md`. The
index is `fit.md`.

Module: `hark.forge.fit.auto` (same module as the fitting side).

## Interface (render side)

| name | contract |
|---|---|
| `render(model, seed=0, take=None, bed=None, **knobs)` | float64 at 48 kHz, `(n, 2)` when the source was stereo, else `(n,)`. A pure function of its arguments. |
| `KNOBS` | Per kind: name -> (search range, identity, description). |
| `knobs_of(model)` | The knob table of a model. |
| `identity(model)` | `{name: identity value}`: the knob setting at which the render reproduces the fit (a recipe's `DEFAULTS`). |
| `finished(model)` | True when renders of this model get the finish step. |
| `morph(a, b, amount=0.5)` | A new model between 2 fits of one kind. |
| `morph_model(a, b, amount=0.5)` | `{'kind': 'morph', 'version': 1, 'amount', 'a', 'b'}` (both without `fit_engine`). |
| `fit_pair(xa, xb, clap=False, fast=False, verbose=False)` | 2 recordings fitted with one kind: `(model a, model b, {kind: (score a, score b)})`. |
| `summary(model)` | Readable lines. |
| `save(model, path)`, `load(path)` | Compact JSON. `load` checks the kind. |
| constants | `FADE_IN_S = 0.002`, `FADE_MIN_S = 0.00025`, `PEAK_DB = -1`, `TAKE_DC_HZ = 8`, `GATE_MAP_S = 0.002`, `GATE_SMOOTH_S = 0.001`. |

### Search range format

A range is `(lo, hi)` (linear) or `(lo, hi, 'log')` (searched on a log scale). This is the forge-core recipe
`SPACE` format.

### Knob tables

modal:

| knob | range | identity | effect |
|---|---|---|---|
| `size` | 0.8 .. 1.25 log | 1 | Object size: mode frequencies / size, decays x size. |
| `damping` | 0.7 .. 1.43 log | 1 | Decays / damping. |
| `brightness` | -3 .. 3 | 0 | dB per octave about 1 kHz on modes and noise. |
| `hardness` | 0.7 .. 1.43 log | 1 | Contact: shorter and brighter attack above 1. |
| `noise` | -6 .. 4 | 0 | dB on the noise bands. |
| `width` | 0.5 .. 1.5 | 1 | Factor on the side. |
| `room` | 0 .. 0.25 | 0 | Wet of a small stochastic room. |
| `vary` | 0 .. 0.6 | 0 | Seeded jitter of modes and noise. |
| `spacing` | 0.8 .. 1.25 log | 1 | Factor on the time between strikes. |

sines:

| knob | range | identity | effect |
|---|---|---|---|
| `transpose` | -3 .. 3 | 0 | Semitones, partials and residual together. |
| `stretch` | 0.85 .. 1.18 log | 1 | Time factor about the first strike. |
| `decay` | 0.7 .. 1.43 log | 1 | Factor on every decay after its peak. |
| `brightness` | -3 .. 3 | 0 | dB per octave about the partial centroid. |
| `vibrato` | 0 .. 20 | 0 | Cents. |
| `drift` | 0 .. 10 | 0 | Cents of slow wander. |
| `residual` | 0.5 .. 2 log | 1 | Gain on the noise. |
| `transient` | 0.5 .. 2 log | 1 | Gain on the strike noise (first 15 ms). |
| `vary` | 0 .. 0.6 | 0 | Seeded jitter. |
| `inharm` | -0.03 .. 0.03 | 0 | Partial ratio stretch. |
| `width` | 0.5 .. 1.5 | 1 | Factor on the fitted side. |

spectral:

| knob | range | identity | effect |
|---|---|---|---|
| `length` | 0.8 .. 1.25 log | 1 | Time-stretch. |
| `speed` | 0.8 .. 1.25 log | 1 | Shorter and higher above 1. |
| `sweep` | 0.7 .. 1.43 log | 1 | Factor on the centroid excursion. |
| `shift` | -0.4 .. 0.4 | 0 | Octaves. |
| `tilt` | -2.5 .. 2.5 | 0 | dB per octave about 1 kHz. |
| `roughness` | 0.5 .. 1.5 | 1 | Texture depth. |
| `width` | 0.5 .. 1.5 | 1 | Side and pan amount. |
| `ridges` | 0.5 .. 1.5 | 1 | Tonal ridge gain. |

hybrid:

| knob | range | identity | effect |
|---|---|---|---|
| `pitch` | -3 .. 3 | 0 | Semitones on every layer. |
| `decay` | 0.7 .. 1.43 log | 1 | Decay of the tone and body layers. |
| `bright` | -3 .. 3 | 0 | dB per octave on every layer. |
| `vary` | 0 .. 0.6 | 0 | Jitter of the tone and body layers. |
| `width` | 0.5 .. 1.5 | 1 | Factor on every layer's side. |
| `tone_db` | -6 .. 6 | 0 | Level of the sines layer. |
| `body_db` | -6 .. 6 | 0 | Level of the modal layer. |
| `air_db` | -6 .. 6 | 0 | Level of the spectral layer. |

gesture: the gesture fitter's own table (`fit-gesture.md`).

`knobs_of`:
- hybrid: the hybrid table without the `<role>_db` knobs of roles the model does not have.
- gesture: the gesture table, without `tone_db` and `vary` when the model has no tones.
- morph: the table of side a plus `morph` with range 0.2 .. 0.8 and identity = the stored amount. The range keeps a
  search between the 2 recordings, not on copies of either.

Extra names every render accepts: `stereo`, `gain`, `gain_db`, `ring`. Any other unknown name raises `ValueError`
listing the bad and the known names.

### Channels

`channels_of(model)`: morph = the larger of its sides. spectral = 2 when it has stereo, else its `channels` (default
1). gesture = its `channels`. sines = its `channels`, or 2 when its `width` is above -50 dB. Others: `channels`
(default 1). For an array: 1 when 1-D, 1 column, or both channels equal.

## Algorithms

### Render

1. Take: `take` null means take when `vary > 0`.
2. Morph model: pop the `morph` knob (default the stored amount), morph the 2 sources, render that.
3. Vary scale: when the model has `vary_scale` and vary is set, vary is multiplied by it (0.5 for modal and hybrid
   fits). Reason: modal and hybrid takes at the bank's vary 0.3 drifted from their fit (CLAP to the target -0.047 and
   -0.075, 10 of 38 and 0 of 9 passing the take check). Now 0.3 jitters them as 0.15 did.
4. `ring` defaults to the take flag.
5. Raw render by kind (below).
6. Time scale for the post stage: spectral `length / speed`, gesture `length`, else 1. Tail factor: modal
   `size / damping`, sines `stretch x decay`, hybrid `decay`, else 1. Anchor: a gesture's `tones.onset`, else 0.
7. Post stage with the scale, tail, the gate time map (when the stage has a gate) and the anchor.
8. Space layer (when `post.space`): `render_space(space, y, seed, width=knob width, ring=take, limit=False)`.
9. Bed layer (when `post.bed` and `bed` is not false): add `bed_noise(bed, len, seed)` (to both channels).
10. Take: a causal 2nd-order Butterworth high-pass at 8 Hz per channel. Reason: amplitude modulation of a noise band by
    its envelope puts some energy at very low frequencies, so a take with a new seed could have a DC offset of 1 to
    2 % of its peak.
11. Finish (only when the model is finished and the render is final): fade-in length
    `clip(onset_index(mono, -40 dB) / sr - 0.5 ms, 0.25 ms, 2 ms)` and the -1 dBFS peak gain (the sines finish).
    Reason: 18 of 464 reference takes went above 0 dBFS. When a recording starts inside its own attack, the short
    fade keeps that attack.

Internal switches (not public knobs): `final=False` leaves out the finish (the engine measures levels and post
stages on such renders). `space=False` leaves out the space layer (its dry).

`take=False` keeps the recording's length and all of its low end. The identity render at seed 0 is what the fit
quality is measured on.

### Finished

modal and sines: the `coherent` flag. hybrid: any layer model with `coherent`. spectral: the `finish` flag. gesture
and morph: always. A model fitted before these flags renders without the finish, sample for sample as before.

### Raw render by kind

- modal: `gain_db` is renamed to `gain`. `modal.render`.
- sines: `gain` renamed to `gain_db`. `stereo` defaults to the model's channel count. `sines.render`.
- gesture: `gain` renamed to `gain_db`. `gesture.render`.
- spectral: `gain` renamed to `gain_db`. `ring` is dropped (noise has no ringing modes).
  `spectral.render(limit=False)`. A mono model returns the channel mean.
- hybrid: `gain_db` (or `gain`) is taken out and applied after the hybrid render.

### Hybrid render

1. `two`: the model's channels, unless `stereo` is given. `ring` null means `vary > 0`.
2. Per layer i, seed `(seed x 7 + 101 (i + 1)) & 0x7FFFFFFF`:
   - sines: `transpose=pitch, decay, brightness=bright, vary, ring, stereo = two and the layer has a side, width`.
   - modal: `size = 2^(-pitch/12)`, `damping = size / max(decay, 0.001)`, `brightness=bright, vary, width, ring`,
     `stereo` = two unless its side is null (then false).
   - spectral: `shift = pitch / 12`, `tilt = bright`, `width`, with `limit=False` and `steady=False`.
   - Shape each to the output channel count (mono repeated, stereo mean-mixed).
   - Gain `10^((layer gain + the role's knob)/20)`.
3. Sum the layers on the longest length (shorter ones zero-padded). Mono output takes the single column.

### Gate time map

The knobs that move time move the gate's breakpoints (the decay knobs do not: the gate stands for an edit by the
sound designer, so a longer ring still ends at the same time).

- sines with stretch not 1: `t -> onset + (t - onset) x stretch`.
- spectral with `length / speed` not 1: `t -> t x scale`.
- gesture with length not 1: `t -> onset + (t - onset) x length`, onset = the tones' first strike.
- modal with spacing not 1 and 2 or more strikes:
  - New strike times as the modal plan makes them: `tn_i = tn_{i-1} + max(3 ms, (t_i - t_{i-1}) x spacing)`.
  - Each breakpoint belongs to the latest old strike at or before it.
  - For strike i (not the last), with `s = max(t_{i+1} - 2 ms, t_i)` and `ns = max(tn_{i+1} - 2 ms, tn_i)`:
    breakpoints before s whose moved time is before ns move by `tn_i - t_i`. Then 2 points:
    `(min(s + d, ns) - 1e-6, G(e - d))` and `(ns, G(s))` with G the original curve (0 before, last value after).
    The cut holds at its value across a longer gap, or ends at its value where a shorter gap ends. Breakpoints at or
    after s (the reopening in the last 2 ms before strike i + 1) move with strike i + 1.
  - The last strike's breakpoints move by its shift.
  - Keep only strictly increasing times.
- Otherwise none.

### Morph

`morph(a, b, amount)`:
1. `w = clip(amount, 0, 1)`. When w is 0 or 1 and both kinds match (not morph): a deep copy of that source without
   `fit_engine`.
2. Kinds must match, else `ValueError` naming both and suggesting a refit with one kind. A morph of a morph model
   raises `ValueError`.
3. The kind's morph (below).
4. Post stages mixed (below), through the time maps when either source has a gate.
5. `vary_scale`: when either has one, `(1 - w) va + w vb` with 1 for a missing one, 4 decimals.

Shared tools:
- Pairing: an optimal assignment (Hungarian) on cost `|log2 fa - log2 fb| + 0.003 x |level difference in dB|` with
  the levels relative to each side's maximum energy. Pairs more than 1 octave apart are not allowed and stay unpaired.
- Geometric mix: `exp((1 - w) ln a + w ln b)`. Circular phase mix: the angle of `(1 - w) e^(j pa) + w e^(j pb)`.
- Rows `[f, tau, amp, phase]`: a negative amplitude becomes positive with phase + pi. Energy for pairing
  `amp^2 x tau`. Paired rows mix frequency, tau and amplitude geometrically and phase circularly. Unpaired rows of a
  keep their values with amplitude times `1 - w` (when w < 1). Unpaired rows of b with amplitude times w (when w > 0).
- Anchors: paired anchor times of each side, morph anchors `(1 - w) ta + w tb`, all made strictly increasing (steps
  at least 1e-6). Time maps a -> morph and b -> morph are piecewise linear through the anchors and extend with slope 1
  outside.
  - modal anchors: 0, the first `min(count)` strike times, the length.
  - sines anchors: 0, the first `min(count)` onsets (or the onset), the duration.
  - spectral anchors: 0, the keyframe with the most total power, the duration.
- Curve mix: both curves on the morph axis, the union of their times (6 decimals), each interpolated, weighted sum.

modal morph:
- The first k = `min(strike counts)` strikes pair by index: time = the morph anchor, ramp mixed linearly, modes and
  contact rows mixed. Extra strikes of one side keep their mapped time and ramp with amplitudes times that side's
  weight (dropped at weight 0). Sort by time.
- Noise edges must match, else `ValueError`. Each band envelope: both mapped to the morph axis, mixed, simplified
  (Ramer-Douglas-Peucker 0.25 dB), times 6 decimals, dB 3 decimals.
- Sub: both waveforms weighted and summed (padded), kept when non-zero.
- Side: the linear mix of both per-band ratios (a missing side counts as zeros), 4 decimals. `side_t`: the sines
  side morph through the time maps.
- Length: the last morph anchor. Channels: the maximum. Fade-out: linear mix. `coherent` when either has it.
  Version: the maximum.

sines morph:
- Partials pair by energy-weighted mean frequency, energy `10^(peak/10) x (t1 - t0 + 0.01)`.
- A paired partial: t0 and t1 mapped and mixed (`t1` at least `t0 + 0.1 ms`). Frequency (in log2) and amplitude (in
  dB) curves are each normalised to 0..1 over their own mapped life, mixed on the union grid, then put back on the
  new life. Rise and release mixed linearly. Phase circular.
- Unpaired partials are mapped onto the morph axis with a gain of `20 log10(1 - w)` (a) or `20 log10(w)` (b).
- Sort strongest first.
- Strike atoms: the first k = `min(counts)` strikes pair by index (time mixed, atoms mixed). Extras keep their mapped
  time with amplitudes times their weight.
- Residual bands must match, else `ValueError`. Levels mixed on the union of mapped times (2 decimals).
- Onsets: mixed for the first k, mapped extras for the side with non-zero weight. Sorted. The onset is the first.
- Width mixed linearly. Side by the sides morph (below). Channels the maximum. `coherent` when either. Analysis empty.
  Summary recomputed.

sines side morph: each side's keyframe times through its map, gains interpolated on the union grid, mixed linearly.
A missing side counts as all zeros. Null when neither has one. Band lists must match, else `ValueError`.

spectral morph:
- Bands must match, else `ValueError`.
- Envelope: both keyframe sets mapped, union grid (5 decimals), per band linear mix (2 decimals).
- Stereo: when either has it, each side's ild and icc on the grid (a side without stereo counts as ild 0 and icc 1),
  mixed.
- Ridges: each side's ridges with mapped times and dB plus `20 log10(weight)` (dropped at weight 0).
- Texture: the nearer side's (w under 0.5: a; else b; or the one that exists) copy. Grains: both present gives the
  geometric mix of density. One present is kept only when its side's weight is at least 0.5. AM: both with equal group
  counts gives the nearer side's AM with linearly mixed depths. One present gives its AM with depths times its weight.
- Fine: both present gives a linear mix (or the nearer when the shapes differ).
- Level mixed linearly. Duration the last morph anchor. Channels the maximum. `finish` when either. Source null.
  Fit empty.

gesture morph:
- Tones of one kind on both sides: the kind's morph, `layer: "deterministic"`, and for modal the onset = the first
  strike time.
- Tones of 2 kinds (cannot be paired): the nearer side's only. Tones only on one side: that side's tones faded by its
  weight (`gain_tones` with `20 log10(weight)`), dropped at weight 0.
- Air: the spectral morph. Trajectory recomputed on the morphed air. Channels the maximum (also set on the tones).

hybrid morph:
- Roles in the order tone, body, air. A role on both sides: gain mixed linearly, model morphed (recursively through
  `morph`, so the layer post stages also mix). A role on one side: that layer with gain plus `20 log10(weight)`,
  dropped at weight 0.
- Length mixed linearly, channels the maximum.

Post stage mix:
- None when neither has one. A missing one counts as EQ all 0 (28 bands) and gain 0.
- EQ and gain: linear mix.
- Attack: only when both have `pre`. The pre-onset envelopes are resampled onto a common normalised axis of
  `round(t_on / 0.25 ms)` frames (at least 1) with `t_on` mixed, and mixed. `v: 2` when both have it. `hp_hz`: the
  larger when both have one. Attack contours: when both exist, resampled to `max(2, round(mixed length))` points and
  mixed.
- Gate: when either has one. A missing gate counts as `[[0, 0]]`. With time maps, each side's breakpoints move onto
  the morph axis first. Then union of times, each gate interpolated in dB (0 before), linear mix.
- Space and bed: the nearer source's copy.

Time maps for the gate: modal, sines and spectral use their anchors. gesture uses the anchors of its tones
when both sides have tones of the same kind, else those of its air. hybrid uses the first role (tone, body, air) both have with one kind.

Measured (facts): 3 morphs at 0.5 (2 clicks as hybrids, 2 key presses as modal, 2 dings as sines) had CLAP 0.48 to
0.76 against each source, so they are new sounds and not copies of either. Known limits: strikes are paired by their
index, which pairs a key with 2 strikes and a key with 4 strikes poorly. Partials more than 1 octave apart stay
unpaired and cross-fade rather than glide. Pole copies in a later strike that cancel an earlier ring no longer
cancel it after a morph.

### Fit pair

1. Fit both with kind auto.
2. Same chosen kind: return both, and per kind both candidate tables.
3. Else: for each of the 2 winning kinds, reuse the fit whose winner it is (its candidate score), and fit the other
   recording with that single candidate (its score, 1e9 when it failed). Keep the kind with the lowest summed score.
   Return both models and `{kind: (score a, score b)}`.

### Summary

- Auto fits start with a line: the chosen candidate and every candidate with its score (or "failed").
- modal: the modal summary. sines: the sines one-line description. spectral and gesture: their summaries.
- morph: the amount and kind, then both sides' summaries, indented.
- hybrid: the layer count and length in ms, then per layer its role, kind, whether it is a deterministic part, its gain
  and its summary, indented.

## Constants

| name | value |
|---|---|
| fade-in | 2 ms, at least 0.25 ms, 0.5 ms before the 40 dB onset |
| peak gain | -1 dBFS |
| take high-pass | 8 Hz, 2nd-order Butterworth, causal |
| hybrid layer seed | `(seed x 7 + 101 (i + 1)) & 0x7FFFFFFF` |
| morph knob range | 0.2 .. 0.8 |
| pairing | 1 octave limit, level weight 0.003 per dB |
| modal envelope morph tolerance | 0.25 dB |
| gate map hold | 2 ms before a strike |

## Errors and edge cases

- Unknown knob: `ValueError`.
- Kinds that differ: `ValueError`. Morph of a morph: `ValueError`.
- Band mismatches in a morph: `ValueError`.
- `morph` at amount 0 or 1 returns the exact source (deep copy), so its renders match the source's.
- A morph model with a `morph` knob outside 0..1 is clipped to 0..1.
