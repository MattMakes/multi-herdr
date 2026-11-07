# hark fit: gesture fitter, tones plus air (part of fit.md)

This part specifies the gesture fitter. Its targets are designed gestures (whooshes, swishes, risers, reverse
swells, camera moves, glitch runs), and short sounds that a few tones with some noise describe better than many
partials with a residual. The model has 2 layers. "Tones" is the noise-free part of a sines or modal fit. "Air" is a
fine-grained spectral noise model of the remainder after the tones are removed. The 2 layers are separated on STFT
magnitudes. The index is `fit.md`.

Module: `hark.forge.fit.gesture`. Pure numpy and scipy.

Why it exists (measured facts): a designed gesture mixes tonal lines (steady or gliding), short tonal bursts, clicks,
gated stutters and a noise body, often panned and moving. The sines fitter by itself made 60 to 180 partials of such
sounds, and their takes sounded metallic. The spectral fitter by itself blurred them into an even noise (CLAP 0.44 to
0.73). A hybrid removes the partial render from the waveform by subtraction, so any tone that does not cancel exactly
in phase stays in the residual. A split on magnitudes cannot put tone energy into the air that the recording lacks.

## Interface

| name | contract |
|---|---|
| `KIND = 'gesture'`, `VERSION = 1` | |
| `KNOBS` | name -> (search range, identity, description); the engine's knob table format (see `fit-auto.md`). |
| `fit(x, name=None, tones=None, alpha=3.0, tone_min=0.05, min_partial_ms=0, fast=False, tones_kind='sines', verbose=False, **air_kw)` | A gesture model. `tones`: an existing sines or modal fit of x that supplies the tones; null makes the fitter fit one (`tones_kind` `'sines'` or `'modal'`); `False` gives air only. |
| `render(model, seed=0, length=1, sweep=1, doppler=0, proximity=0, pan=1, motion=0, width=1, roughness=1, tone_db=0, air_db=0, vary=0, gain_db=0, stereo=None, ring=False)` | float64 at 48 kHz: `(n, 2)` for a stereo source or `stereo=True`, else `(n,)`. |
| `air(x, y_det=None, alpha=3.0, fast=False, name=None, mask_fft=1024, **kw)` | A spectral model of x, or of the remainder of x after the noise-free render `y_det` is removed. The hybrid engine uses it for its fixed air layer. |
| `residual(X, Y, alpha=3.0, n_fft=1024)` | X minus Y in the magnitude domain. |
| `gain_tones(det, gain_db)` | A copy of a tones model, `gain_db` louder. |
| `check(model)`, `summary(model)`, `save(model, path)`, `load(path)` | As for the other kinds. `tones_kind` other than sines or modal raises `ValueError`. |

### Knob table

| knob | range | identity | effect |
|---|---|---|---|
| `length` | 0.8 .. 1.25 log | 1 | Time-stretch. The tones are stretched around their first strike, and the air follows. |
| `sweep` | 0.7 .. 1.43 log | 1 | Factor on the air's centroid excursion. |
| `doppler` | -2 .. 2 | 0 | Doppler bend of the full sound in semitones, with the turn at the air's peak. Positive: higher while it comes closer. |
| `proximity` | -0.5 .. 1 | 0 | Makes the air's softer keyframes darker and softer. |
| `pan` | 0.5 .. 1.5 | 1 | Factor on the air's L/R level differences. |
| `motion` | -0.6 .. 0.6 | 0 | Moves the full sound across the stereo field as it passes. |
| `width` | 0.5 .. 1.5 | 1 | Factor on the tones' side and the air's width. |
| `roughness` | 0.5 .. 1.5 | 1 | The air's texture depth (flutter, crackle). |
| `tone_db` | -6 .. 6 | 0 | Level of the tones. |
| `air_db` | -6 .. 6 | 0 | Level of the air. |
| `vary` | 0 .. 0.4 | 0 | Seeded jitter of the tones. The air already differs per seed. |

Without tones, `tone_db` and `vary` do nothing. The engine leaves them out of the knob set of such a model.

### Model (plain JSON)

| field | meaning |
|---|---|
| `kind`, `version`, `sr` | `"gesture"`, 1, 48000 |
| `dur` | s, 6 decimals |
| `channels` | 1 or 2 |
| `source` | name or null |
| `tones` | Null, or a deterministic sines or modal model (`layer: "deterministic"`, noise removed). It has `onset` (the first strike, s). |
| `air` | A spectral model with `channels`. |
| `traj` | The air's trajectory (spectral part). |
| `fit` | `alpha`, `tones_kind` (null without tones), `tone_share` (4 decimals), `partials` (count of partials or modes), `air_keys`, `ridges`, `seconds` (`tones`, `air`). |

## Algorithms

### Loading

Any input becomes `(n, 2)` float64 through the quality loader (a path loads at 48 kHz). Mono is duplicated. The source
is stereo when its channels are not equal (allclose).

### Magnitude residual

For each channel c, STFT of X and of Y (Y's channel c, or its last channel, cut or padded to n): Hann, size 1024, hop
256. Gain per bin `sqrt(clip(1 - alpha |Y|^2 / (|X|^2 + 1e-30), 0, 1))`. Inverse STFT of `X x gain` (X's phase).
A bin that Y accounts for is removed. A bin where Y is weak keeps X. `alpha = 3` removes more than Y holds, so the
skirts of partials and their fit error do not reach the air. Otherwise the air's ridges would rebuild them as
unsteady whistles.

### Deterministic tones

From a sines or modal model (without its `post` and `fit_engine` entries), deep copied:
- Set `layer: "deterministic"` and `channels` (2 for a stereo source).
- Modal: every noise band envelope becomes `[[0, -240], [length, -240]]`. `onset` = the first strike time (0 when no
  strike). Mono source: drop `side` and `side_t`.
- Sines: drop partials whose life `t1 + rel - t0` is under `min_partial_ms` (0 by default, so none). The residual
  becomes one silent band (`bands [1000]`, `times [0, duration]`, `db [[-150, -150]]`). Mono source: drop `side`.

### Tones render

- Modal: `modal.render(det, seed, spacing=s, damping=1/s, vary, width, gain=tone_db, ring, stereo=two)`. A stretch s
  stretches strike spacing and decays.
- Sines: `sines.render(det, seed, stretch=s, vary, width, gain_db=tone_db, ring, stereo=two)`.

### Air

1. Residual R: X itself, or the magnitude residual of X against the tones (as `(n, 2)`).
2. Options: the spectral `DETAIL` options, then the air options `gesture_ms=5, quiet_db=70, grain_norm_ms=5,
   grain_halves=True`, then the fast options (`iters=1, texture=False, seeds=(1,)`) when fast, then the caller's.
3. Spectral fit of R. Set `channels`: 1 when X's channels are equal, else 2.

Reasons (facts): with a 5 ms gesture grid, and keyframes that track cells as far as 70 dB below the loudest one,
silent gaps and stutters stay silent. Compared with the first settings (alpha 1, 10 ms, 50 dB) on 7 dev gesture
clips, the mean score improved from 10.21 to 9.47 and CLAP from 0.857 to 0.875. The 5 ms grain levelling and the
stationarity test exist because short fits made mostly of air (keys, ticks) gave takes whose CLAP ranged 0.84 to
0.92 across seeds. Over 20 dev clips the median CLAP shortfall of takes against seed 0 dropped from 0.046 to 0.031.

### Fit

1. Load. `two` = the source is stereo. `e_x` = the total energy.
2. Unless tones is `False`: take the given model, or fit one (sines with all windows, or with window 1024 only when
   fast; modal with 1 pass when fast). The sines fit sees the stereo source when two, else the left channel.
3. Make the deterministic tones and render them at seed 0 as `(n, 2)`. Share = tone energy / `e_x`. When the share
   is under 0.05, drop the tones (a pure noise gesture).
4. Air of X against the tones render (or X alone).
5. Store the model with the air's trajectory and the fit diagnostics.

### Render

1. `two` = model channels is 2, unless `stereo` is given. `s = max(length, 0.001)`. `n = max(64, round(dur s sr))`.
2. Tones (when present): the tones render with stretch s, vary, width, `tone_db` and ring.
3. Air: when ring, extend the air model by `max(60 ms, tones length / s - dur)`: one keyframe at the new end with every
   band 60 dB under the last keyframe, stereo values held. The air then decays 60 dB linear in dB under the ringing
   tones.
4. Air render: `spectral.render(air, seed (seed x 7 + 101) & 0x7FFFFFFF, length=s, sweep, roughness, width, pan,
   proximity, gain_db=air_db, limit=False)`.
5. Alignment: the tones are stretched around their first strike, the air around time 0. Shift the air by
   `round(onset x (1 - s) x sr)` samples (pad zeros in front when positive, drop samples when negative).
6. Sum as `(m, 2)` with `m = n`, or the longest of n, tones and air in ring mode.
7. Doppler or motion: the stored trajectory's peak times s and tau (as the spectral part), then bend and pass pan of
   the whole sum.
8. Output gain.
9. Ring: a raised-cosine end fade over `min(len/3, 10 ms)` samples (when more than 1).
10. Return `(m, 2)` when two, else the channel mean.

`render(model, seed, knobs)` is a pure function. Identity knob values at seed 0 give the fit as measured.

### Gain of tones

Sines: add `gain_db` to every amplitude breakpoint and multiply every atom amplitude by `10^(gain_db/20)`. Modal:
multiply the amplitude column of every mode and contact row and every sub sample. The morph uses it to fade tones
that only one side has.

### Summary text

Source and length. Tones: modal (strike count, mode count and range) or sines (partial count and the range of first
frequencies), with the tone share in percent, or "no tones (air only)". The air's summary. The trajectory: peak time,
rise and fall in ms, centroid at start, peak and end, pan range, median width. The model size in kB.

## Measured facts

On 7 dev gesture clips (camera air, glitch, 2 reverse swells, riser, swish, fast whoosh), scored as the engine scores
a candidate (bed stripped, post stage, CLAP judge, seed 0 with the bed back), against the engine's choice without
this kind: the gesture model scored lower (better) on 4 of 7, each with higher CLAP (camera move 0.855 to 0.880,
reverse swells 0.858 to 0.935 and 0.808 to 0.842, riser 0.871 to 0.921), and its takes passed the take check. It lost
on short or bursty sounds: a 0.24 s swish (0.845 against modal's 0.884), a grainy whoosh (0.795 against 0.796) and a
glitch run (0.905 against 0.885 CLAP, but a blunter first onset cost it the score). Modal tones scored worse on 6 of
7, so sines tones stay the default and modal tones are opt-in. Inside the engine's plans the gesture model was
chosen on 38 of 78 dev clips, including short sounds such as keys, pops and ticks.

## Constants

| name | value |
|---|---|
| alpha | 3.0 |
| mask STFT | Hann 1024, hop 256 |
| tone minimum share | 0.05 |
| minimum partial life | 0 ms |
| air options | spectral DETAIL + gesture 5 ms, quiet 70 dB, grain norm 5 ms, grain halves on |
| fast air options | 1 iteration, no texture, seed 1 |
| ring air tail | at least 60 ms, 60 dB fall |
| ring end fade | 10 ms raised cosine, at most a third of the length |
| air seed | `(seed x 7 + 101) & 0x7FFFFFFF` |
| minimum length | 64 samples |

## Errors and edge cases

- Kind or version mismatch raises `ValueError`.
- A pure noise recording (tone share under 5 %) gives `tones = null`. The model is air only.
- A modal tones model with no strikes has onset 0.
- A negative alignment shift larger than the air drops the whole air render (the sum then holds the tones only).
