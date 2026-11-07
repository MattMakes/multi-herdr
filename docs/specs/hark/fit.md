# hark fit: audio to recipe code (index)

Unit: **fit**. It turns a real recording into code. It fits a parametric model to the recording by analysis by
synthesis: it fits model parameters, renders the model, measures the render against the recording, and keeps the
best. Then it writes the model as a self-contained Python recipe module that the forge critic can search and vary.

The model keeps no sample of the recording. It keeps what the sound is built from: strikes of damped modes,
tracked partials, band-noise envelopes, a noise gesture with stereo and texture, a diffuse side layer. From that
structure it makes new takes (fresh noise per seed), variations (knobs) and morphs between 2 recordings.

This spec is the behaviour contract for a clean implementation. It does not hold source text. "The source" means the
system this spec describes. Our names: package and CLI `hark`, engine at `skills/hark/engine/hark/`, Python package
`hark.forge.fit`, thread env variable `HARK_THREADS`.

## Parts

The spec is longer than 60 KB, so it is split. This index holds the overview, the cross-part contracts, the data
assets, the unit-level acceptance tests, the dependencies and the open questions. Each part holds its own interface,
algorithms, constants and edge cases.

| part | covers | module |
|---|---|---|
| [`fit-quality.md`](fit-quality.md) | The quality measure: metrics dict, score, floor, CLAP and describe checks, the pair picture. Acceptance Q-1 .. Q-9. | `hark.forge.fit.quality` |
| [`fit-modal.md`](fit-modal.md) | The modal fitter: onsets, ESPRIT on sub-bands, least squares, contact modes, sub pulse, ERB band noise, render. | `hark.forge.fit.modal` |
| [`fit-sines.md`](fit-sines.md) | The sines fitter: peaks and partial tracking, strike atoms, third-octave residual, HF roll-off, side model, render. | `hark.forge.fit.sines` |
| [`fit-spectral.md`](fit-spectral.md) | The spectral fitter: 48-band noise gesture, denoise, gain iterations, stereo, texture, ridges, motion knobs. | `hark.forge.fit.spectral` |
| [`fit-gesture.md`](fit-gesture.md) | The gesture fitter: tones plus air with a magnitude-domain split. | `hark.forge.fit.gesture` |
| [`fit-space.md`](fit-space.md) | The space layer (a seeded diffuse side layer) and the stereo measures. | `hark.forge.fit.space` |
| [`fit-auto.md`](fit-auto.md) | The engine, fitting side: bed strip, character, plan, candidates, hybrids, post stage, gate, scoring, confirm, CLAP margin, bed check. | `hark.forge.fit.auto` |
| [`fit-render.md`](fit-render.md) | The engine, render side: knob tables, render, takes, finish, gate time map, morph, fit pair, summary. | `hark.forge.fit.auto` |
| [`fit-codegen.md`](fit-codegen.md) | The generated recipe module, `write_recipe` / `load_recipe`, the bitwise identity contract, and the `hark fit` CLI. Acceptance C-1 .. C-13. | `hark.forge.fit.codegen`, `hark.forge.fit` |

Every source file of the unit maps to a part: the package file (API and CLI) and the codegen file to
`fit-codegen.md`, the engine file to `fit-auto.md` and `fit-render.md`, and each other file to the part of the same
name. The source's 2 documents (the method write-up and the credits list) supply the measured facts, the quality
targets and the data assets in this index.

## Interface

### Package layout

```
hark/forge/fit/
  __init__.py   package API (fit, render, morph, to_recipe), run_fit, run_morph, add_cli
  auto.py       the engine (fit-auto.md, fit-render.md)
  codegen.py    model -> recipe module (fit-codegen.md)
  quality.py    the quality measure (fit-quality.md)
  modal.py      fit-modal.md
  sines.py      fit-sines.md
  spectral.py   fit-spectral.md
  gesture.py    fit-gesture.md
  space.py      fit-space.md
```

Every module uses numpy and scipy only, plus core-io for file loading and the forge-core helpers named under
Dependencies. CLAP and describe (listen unit) load only when a caller asks for them.

### Package API (`hark.forge.fit`)

| name | contract |
|---|---|
| `fit(x, kind='auto', **kw)` | A model of a recording. `x`: a path or a 48 kHz float array, `(n,)` or `(n, 2)`. `kind`: `auto`, `modal`, `spectral`, `sines`, `gesture`, `hybrid`. Keywords as the engine's `fit` (`fit-auto.md`). |
| `render(model, seed=0, **knobs)` | The render of a model: float64 at 48 kHz, `(n, 2)` for a stereo model, else `(n,)`. Keywords as the engine's `render` (`fit-render.md`). |
| `morph(a, b, amount=0.5)` | A model between 2 fits of one kind (`fit-render.md`). |
| `to_recipe(model, **kw)` | The source text of a recipe module (`fit-codegen.md`). |
| `run_fit(...)`, `run_morph(...)`, `add_cli(sub)` | The CLI pipelines as functions, and the parser hook (`fit-codegen.md`). |

### Model kinds

A model is a plain JSON dict. It has `kind`, `version` and `sr = 48000`. `save` writes compact JSON (no spaces).
`load` reads it and checks the kind. Every fitter's `check` raises `ValueError` for a wrong kind or a version above
the supported one.

| `kind` value | `kind_of` name | version | made by | part |
|---|---|---|---|---|
| `modal` | `modal` | 1 | modal fitter | `fit-modal.md` |
| `sines` | `sines` | 1 | sines fitter | `fit-sines.md` |
| `spectral_noise` | `spectral` | 1 | spectral fitter | `fit-spectral.md` |
| `gesture` | `gesture` | 1 | gesture fitter | `fit-gesture.md` |
| `hybrid` | `hybrid` | 1 | engine (layers of the kinds above) | `fit-auto.md` |
| `morph` | `morph` | 1 | engine (`morph_model`: 2 models and an amount) | `fit-render.md` |

The engine adds `post` (the post stage, with optional `bed` and `space` entries), `vary_scale`, `channels`, `source`
and `fit_engine` (diagnostics) to the chosen model (`fit-auto.md`, "Fields the engine adds to any model"). The
renderer never reads `fit_engine`.

### The generated recipe module

The deliverable of the unit. Exact contract in `fit-codegen.md`. In short, top-level names in file order:

| name | meaning |
|---|---|
| docstring | A summary of the model, the source name with a licence note, the measured quality, usage examples. |
| `TARGET`, `ALTS` | The critic's text description and alternatives. |
| `REFSET` (optional), `REFS` | The critic's reference category and reference file paths. |
| `KIND` | The model kind. |
| `SPACE` | Knob name -> search range `(lo, hi)` or `(lo, hi, 'log')`, with the knob's description as a comment. |
| `DEFAULTS` | Knob name -> identity value. At `DEFAULTS` the module re-renders the fit. |
| `MODEL` | The model without `fit_engine`, as a readable Python literal. |
| `render(p, r)` | The forge-core recipe convention: knob dict `p`, numpy Generator `r`. One take. |
| `render_seed(seed=0, **knobs)` | A render with an explicit seed. `render_seed(0)` is the fit as measured. |
| main block | `python recipe.py [seed] [out.wav]` writes a 48 kHz 32-bit float WAV. |

### CLI

`hark fit <file> [-o recipe.py] [--kind auto] [...]`, `hark fit --morph A B [--amount 0.5]` and
`hark fit --forge recipe.py [--budget 96]`. Flags, output paths, steps and result dicts: `fit-codegen.md`.

### Knobs

Each kind has a knob table: name -> (search range, identity, description). The engine's `knobs_of(model)` gives a
model's table and `identity(model)` its identity values. Tables per kind: `fit-render.md` ("Knob tables") and
`fit-gesture.md`. Every kind has a `vary` knob (seeded jitter) unless the part says it does not apply. A morph model
adds a `morph` knob (range 0.2 .. 0.8, identity = the stored amount). An unknown knob name raises `ValueError`.

### Environment

`HARK_THREADS`: the FFT worker count of the spectral fitter. Default 2. A value that is not a positive int gives 2.

## Behaviour

### Fit (kind auto)

1. Load the recording as `(n, 2)` float64 at 48 kHz. Mono is duplicated.
2. Bed: when the recording has a steady pre-roll 12 to 40 dB under the sound (for example music under a film clip), strip it. The
   fitters see the stripped sound. Candidates are scored against the original with a bed noise layer rendered back
   under them. `bed=False` (CLI `--keep-bed`) fits the unmodified recording.
3. Character: measure the sound (tonal, impulsive, noisy, gliding, length, t40, strikes, and more).
4. Plan: the character picks an ordered candidate list from the single kinds, the hybrids and the gesture kind.
5. For each candidate: fit it, fit its post stage (EQ, level, attack clamp, gate) from renders at seeds 0 and 1,
   render it fully at each scoring seed, measure quality, score. A candidate that fails records an error row.
6. Choose the lowest score. Confirm the top 3 on seeds 2 and 3. Apply the CLAP margin rule when CLAP is on.
7. Bed check: fit the winner's kind on the unstripped recording too, and keep that when it scores lower.
8. Space layer: for a stereo recording with a decorrelated tail, fit a seeded diffuse side layer.
9. Set the vary scale (0.5 for modal and hybrid winners) and store the diagnostics.

Details, numbers and reasons: `fit-auto.md`. A single `kind` skips the plan and the choice but keeps the post stage
and the space layer.

### Render

`render(model, seed, **knobs)` is a pure function of its arguments. In order: morph resolution, vary scale, the
kind's raw render, the post stage (with the gate time map), the space layer, the bed layer, in a take the 8 Hz
high-pass, and for a finished model the finish (a short fade-in from zero and a gain down to -1 dBFS when the peak is
over it). The seed changes only noise and jitter. Identity knobs at seed 0 give the render the fit was measured on.
Details: `fit-render.md`.

A take (`take=True`, or by default when `vary > 0`) rings out (the decays run to 60 dB down instead of stopping at the
recording's end) and has no DC.

### Recipe

The CLI writes the recipe, imports it, renders through it and measures that render. So every reported quality number
describes the output of the file itself. It also checks that the recipe's render equals the engine's render bit for
bit (`code_renders_fit`). Details: `fit-codegen.md`.

### Morph

2 recordings are fitted with one kind (the kind that scores best for both together). The morph pairs modes or
partials within 1 octave, mixes paired values geometrically and phases circularly, fades unpaired rows, warps time
through anchor points, and mixes the post stages. Amount 0 and 1 give the sources exactly. Details: `fit-render.md`.

### Variation search

`hark fit --forge recipe.py` runs the forge-core critic search on the recipe module. The critic scores renders
against the recipe's `REFSET` or `REFS` and against `TARGET` with CLAP. The search itself belongs to forge-core.

## Algorithms

Each algorithm is specified step by step in its part. Where to look:

| topic | part, section |
|---|---|
| Quality metrics, score formula, floor | `fit-quality.md`: mel_db, env_corr, onset_err, tone_db, excess and missing, stereo measures, Score, Floor |
| Onsets, ESPRIT, model order, least squares, cancelling clusters, contact modes, sub pulse, noise bands | `fit-modal.md` |
| Peak picking, partial tracking, strike atoms (matrix pencil), residual bands, HF roll-off, side model, window choice | `fit-sines.md` |
| Band axis on `ln(1 + f/80)`, low-rank denoise, gain iterations, stereo per band group, keyframes, texture, ridges, bend and pass pan | `fit-spectral.md` |
| Magnitude-domain residual (alpha 3), tones plus air render | `fit-gesture.md` |
| Diffuse side power, flattened impulse responses, wet layer | `fit-space.md` |
| Character, plan, bed detection, bed strip, bed noise, hybrid fit, post stage, gate fit, attack clamp, choice rules | `fit-auto.md` |
| Render order, takes, finish, hybrid render, gate time map, morph per kind, fit pair | `fit-render.md` |
| Model literal, critic words, recipe text, CLI pipelines | `fit-codegen.md` |

## Constants

Each part has a full constants table. The constants that cross parts:

| name | value | where |
|---|---|---|
| sample rate | 48000 Hz everywhere | all |
| render dtype | float64 (engine render) | `fit-render.md` |
| take seeds | `(0, 1, 2)` (`--takes`) | `fit-auto.md` |
| confirm | top 3, seeds `(2, 3)` | `fit-auto.md` |
| CLAP margin | +0.02 CLAP, score tolerance 1.5, envelope tolerance 0.05 | `fit-auto.md` |
| floor seeds | `(11, 12)` | `fit-quality.md` |
| score | `mel + 0.3 tone + 8 (1 - env) + min(onset, 2)`, plus the optional terms in the part | `fit-quality.md` |
| vary scale | 0.5 for modal and hybrid winners | `fit-auto.md`, `fit-render.md` |
| finish | fade-in 0.25 .. 2 ms (0.5 ms before the -40 dB onset), peak gain to -1 dBFS when over | `fit-render.md` |
| take high-pass | 8 Hz, 2nd-order Butterworth, causal | `fit-render.md` |
| gate time grid | 0.5 ms frames, 1 ms smoothing Hann, 2 ms hold before a strike in the time map | `fit-auto.md`, `fit-render.md` |
| bed range | pre-roll 12 to 40 dB under the loudest 50 ms | `fit-auto.md` |
| layer seeds | hybrid `(seed x 7 + 101 (i + 1)) & 0x7FFFFFFF`, gesture air `(seed x 7 + 101) & 0x7FFFFFFF` | `fit-render.md`, `fit-gesture.md` |
| white-noise calibration seed | 12345 | `fit-sines.md`, `fit-spectral.md` |
| morph pairing | 1 octave limit, 0.003 per dB level weight | `fit-render.md` |
| recipe seed draw | `r.integers(0, 2**31 - 1)` | `fit-codegen.md` |
| recipe WAV | 48 kHz, 32-bit float | `fit-codegen.md` |

## Data assets

| asset | format | origin | status | fallback |
|---|---|---|---|---|
| Bank fits: the bank's fit file (version tag `fit1`) | JSON bank file. Per entry: the fit model without `fit_engine`, seed, `vary`, a source record (id, title, author, licence, URL) and the measured note of a tonal fit. | 15 fits of CC0 recordings from Freesound, selected out of 46 CC0 fits on measured values: each has grade high (CLAP cosine against its recording of 0.92 or more, mel gap of 2.5 dB or less) and 5 takes that pass the bank's take check at the shipped `vary`. The ids and Freesound numbers are listed below. | REGENERATE (with this unit, given the 15 CC0 recordings). The file is owned by bank-kit-sfx. Fit's job is that the regeneration is possible. If the build may not download audio: NEEDS-DATA. | Fit CC0 recordings the operator supplies, or synthetic sounds made in the build (struck modes, glide pops, noise swishes). Select with the same grade and take rules. |
| Validation references: 12 recordings, 2 per category (click, typing_key, pop, impact, whoosh_fast, notify) | WAV files | Licensed sound-library and Freesound clips. The source keeps them and their renders local because the renders derive from licensed recordings. | NEEDS-DATA | A synthetic set with the same 6 categories made by a seeded script (see A-12), plus any CC0 clips. The measured targets of A-1 .. A-3 then do not apply. Record the fallback's values as the new baseline. |
| Replication benchmark set: 116 one-gesture clips in 24 categories, split by clip id into 78 dev and 38 held-out, plus 28 fresh held-out | WAV files with a fixed split list | Private licensed reference clips. | NEEDS-DATA | Any CC0 set in the same categories with a fixed split. Targets as for the validation fallback. |
| White-noise bias and noise-level calibrations | Arrays computed in memory | White noise from generator seed 12345 (sines residual bias, spectral denoise noise level), and the white-noise STFT power constant of the bed noise. | REGENERATE at runtime. Nothing is stored. | None needed. |
| Refsets registry and category descriptions | Owned by forge-core | Used by critic words only | DERIVE from forge-core | With no registry, critic words fall back to the character rules (`fit-codegen.md`). |

The 15 bank fits (sound id: Freesound id): `fit/bell/small_bell`: 452379, `fit/click/floss_lid`: 167829,
`fit/hit/bamboo_tok`: 386888, `fit/key.space/thock_space`: 789630, `fit/key/lubed`: 665075, `fit/key/medium`:
378084, `fit/key/thock`: 789628, `fit/land.light/finger_phone`: 704862, `fit/land.light/finger_wood`: 452621,
`fit/land.light/fingertip_book`: 760337, `fit/pop/lip_pop`: 202230, `fit/pop/mouth_plop`: 181907,
`fit/snap/switch_flip`: 278205, `fit/tick/domino`: 329098, `fit/whoosh.short/bamboo_swish`: 427979. All are CC0 1.0.
A recording was excluded when its title, its author or its page mentions a commercial sound library, even when its licence is free.

## Errors and edge cases

Each part lists its own. The ones that cross parts:

- A wrong model kind or a version above the supported one raises `ValueError` (every `check`, `load` and `kind_of`).
- An unknown knob raises `ValueError` in the engine render. A recipe's `render(p, r)` ignores keys outside
  `DEFAULTS`, but `render_seed` passes them through.
- Auto fit: a candidate that fails is recorded as an error row and the loop goes on. When every candidate fails, the
  fit raises `RuntimeError` that names the errors. A bed check that fails keeps the stripped fit.
- `fit_kind` with a name outside `modal`, `sines`, `spectral`, `gesture` raises `ValueError`.
- Morph: kinds that differ, a morph of a morph, or bands that do not match raise `ValueError`. Amount is clipped to
  0 .. 1. Amount 0 or 1 returns a deep copy of that source.
- A mono recording gives mono renders. A stereo recording with equal channels counts as mono.
- A recording shorter than 30 ms or silent has no bed.
- A pure noise recording fitted as a gesture has no tones (`tones = null`).
- CLAP or describe without the ML extras: the listen unit's install prompt applies (TTY asks, non-TTY exits with the
  install command). Fitting without `--clap`, `--report` or `--describe` never loads them.
- The CLI with no file and no mode prints the usage line and exits 2.
- `code_renders_fit = False` is reported, not raised. It means a bug in codegen or a non-pure render.

## Acceptance tests

The parts hold more tests: Q-1 .. Q-9 in `fit-quality.md` and C-1 .. C-13 in `fit-codegen.md`. They are part of this
list. A-1 .. A-4 need the private data (NEEDS-DATA). The others use synthetic sounds made in the test.

Synthetic sounds used below (all 48 kHz, made from a fixed generator seed in the test):
- S1 struck: 3 exponentially decaying sines (440, 1130, 2390 Hz; decay time constants 0.4, 0.2, 0.1 s) after a 2 ms
  noise click, 0.8 s, mono.
- S2 glide pop: a sine gliding from 900 to 500 Hz over 60 ms with a 3 ms attack and a 40 ms decay, 0.2 s.
- S3 whoosh: white noise through a band-pass whose centre sweeps 500 Hz to 4 kHz, with a raised-cosine level
  envelope, 0.6 s, stereo with partly decorrelated channels.
- S4 gated pulses: 4 pulses of S1's first sine, each cut hard after 30 ms, 80 ms apart.
- S5 bed: S1 placed 200 ms into a steady pink noise bed 25 dB under its peak.

| id | setup | expect |
|---|---|---|
| A-1 | NEEDS-DATA. The 12 validation references. For each: `hark fit <ref> --clap --report`, then measure the recipe's seed-0 render. | Median CLAP cosine at least 0.945. Minimum CLAP at least 0.881. (The source measured exactly these values.) |
| A-2 | NEEDS-DATA. Same runs. | Median mel gap (render `mel_db` minus floor `mel_db`) at most 0.75 dB. `mel_db` at most 2.9 dB over its floor on at least 11 of 12. Median `env_corr` at least 0.98. Median `tone_db` at most 2.0 dB. |
| A-3 | NEEDS-DATA. Same runs with `--describe`. | The top describe tag of the render equals the original's on at least 10 of 12. |
| A-4 | NEEDS-DATA. The replication benchmark, dev split (78 clips), each fitted with `--clap`. Seed-0 render, the floor, and 4 takes (seeds 1 .. 4, `vary` 0.3). | Median CLAP at least 0.921, median mel gap at most 0.85 dB, at most 1 clip graded poor, at least 42 clips whose 4 takes pass the take check. Held-out split (38): median CLAP at least 0.903. Grades: high = CLAP >= 0.92 and gap <= 2.5 dB, good >= 0.88 / <= 3.5, fair >= 0.80 / <= 5.0, poor the rest. |
| A-5 | Fit S1, S2, S3, S4 with `kind='auto'` and each single kind. Write and load each recipe. | `render_seed(s, **k)` equals `engine.render(m, seed=s, **k)` bit for bit for s in 0, 1, 5 and for identity knobs and one changed knob. The CLI reports `code_renders_fit: true`. |
| A-6 | Any model from A-5. Render twice with the same seed and knobs, in 2 separate processes. Fit S1 twice with the same options. | The renders are equal bit for bit. The 2 models are equal after JSON round trip (the `seconds` timing fields excepted). |
| A-7 | A model from A-5 that has a noise layer (S3). Render seeds 0 and 1. | The renders differ. Their `mel_db` to each other is more than 0. A model with no noise layer (a deterministic copy, `vary = 0`) renders the same for every seed. |
| A-8 | 2 sines fits of S2 and of S2 pitched up 3 semitones. `morph(a, b, 0)`, `morph(a, b, 1)`, `morph(a, b, 0.5)`. | Amount 0 and 1 render bit for bit as a and b (without `fit_engine`). At 0.5 the partials' mean frequency lies between the 2 sources' (within 1 %, of the geometric mean). A modal fit of S1 morphed with a sines fit raises `ValueError`. |
| A-9 | A modal and a hybrid fit of S1, rendered with `take=True`, `vary=0.3`, seeds 1 .. 4. | Each take's mean over the whole take is under 0.5 % of its peak (8 Hz high-pass, no DC). Without the high-pass a noise take can carry 1 to 2 %. Each take is at least as long as the recording (it rings out). |
| A-10 | Every finished model of A-5, rendered at seed 0 and as takes at seeds 1 .. 4, including a fit of S1 scaled to peak at +6 dBFS. | The peak of every render is at most -1 dBFS (+1e-9). A render whose raw peak is under -1 dBFS keeps its level (no gain up). The first sample is 0, and the first samples rise as a half-cosine fade no longer than 2 ms. |
| A-11 | Auto fit of S4. | The chosen model's `post` has a `gate`. In the seed-0 render, the level 10 ms after each pulse's cut is at least 20 dB under the pulse's peak. The level of each pulse onset is within 1 dB of the original's (the gate reopens before each pulse). |
| A-12 | Auto fit of S5 with the default `bed=True`, and with `bed=False`. | Default: `fit_engine.bed` exists with `t_on` within 3 ms of 0.2 s and `level_db` within 3 dB of -25, and `kept` is set by the bed check. When `kept` is true: the RMS of 10 .. 190 ms in `render(m, bed=False)` is at least 15 dB under the same span of `render(m)`. With `bed=False` at fit time, `fit_engine` has no bed entry and no `post.bed`. |
| A-13 | Auto fit of S1, S2, S3 without CLAP. | The chosen kinds are: S1 modal, sines or a hybrid with a modal or sines layer. S3 spectral, gesture or a hybrid whose air layer holds most energy. Every chosen render scores within 3 points of its floor's score. Proposed thresholds (see Q3). |
| A-14 | S3 (stereo, decorrelated tail) auto fit. | The model renders `(n, 2)`. In `quality(S3, render)`, `side_db[1]` (render) is within 6 dB of `side_db[0]` (original). A mono fit of S1 renders `(n,)` and has no `post.space` entry. |
| A-15 | `hark fit S1.wav -o out/s1.py --report --json`. | Exit 0. `out/s1.py`, `.wav`, `.png`, `.json`, `.report.json` exist. The JSON result has `kind`, `chosen`, `quality`, `floor`, `files`, `code_renders_fit: true`, `seconds`, `candidates`. Running `python out/s1.py 0 x.wav` writes a file equal to `out/s1.wav`. |
| A-16 | `HARK_THREADS` set to 1, 4 and `bad`. Fit S3 with the spectral kind. | The 3 models are equal. `bad` behaves as 2. |

## Dependencies on other units

| unit | what fit uses | direction |
|---|---|---|
| core-io | Audio file loading and resampling to 48 kHz (the quality loader). WAV writing (32-bit float) for renders, the recipe's main block and describe temp files. | fit uses |
| listen | The CLAP audio and text model (`clap_cos`, the CLAP judge, the CLAP margin rule) and `describe` (`--describe`). The optional ML extras and their install prompt. | fit uses, optional |
| timeline | No direct use. The modal render plans strike times itself. | none |
| forge-core | The seeded generator helper used in recipe usage (`rng(seed)`). The tone profile (28 third-octave band levels) and its band centres. The minimum-phase third-octave EQ bands. The perceptual K-weighting filter. The refsets registry and category table. The recipe loader, the recipe list and the critic search (`--forge`). The recipe `SPACE` range format and the `render(p, r)` convention. | fit uses |
| recipes-impacts, recipes-motion | Their recipes with a `REFSET` lend `TARGET` and `ALTS` to fitted recipes of the same category (critic words). Fit has no other tie to them. | fit reads |
| bank-kit-sfx | Owns the bank's fit file and its `add-fit` command. It renders fit entries through `render(model, seed, vary=..., take=True, bed=False)` and maps its pitch and length knobs to fit knobs (pitch: hybrid `pitch`, sines `transpose`, modal `size`; length: spectral `length`, sines `stretch`, gesture `length`). It uses `kind_of` and needs a `vary` knob on every shipped fit. | bank uses fit |
| fit | This unit. | |
| surfaces | The CLI parser's common flags (among them `--json`) and `emit` (JSON or text output). The `fit` sub-command is registered there. | fit uses |
| skills-and-docs | The user docs of `hark fit` (method, measured results, known limits) and the credits list of the bank fits. Fit supplies the facts. | docs use fit |

## Open questions

| id | question | proposed answer |
|---|---|---|
| Q1 | Must hark reproduce the source's random streams exactly (bit-equal renders of the source's saved models)? | No. Only self-consistency is required: same model, seed and knobs give the same samples, and a recipe renders as the engine does. The bank's fits are then regenerated by hark, not copied. |
| Q2 | The bed check (strip or keep the bed) compares model-free scores only. It has no CLAP guard, while the post stage's gate has one. Should hark add one? | Keep the source's behaviour first. Measure with CLAP on a bed set later. |
| Q3 | The reference targets of A-1 .. A-4 need private data. Which thresholds apply to the synthetic and CC0 fallback sets? | Record the first clean run's values as the baseline and fail on a fall of more than 0.01 CLAP or 0.5 dB mel gap. The numeric thresholds in A-9, A-11, A-12, A-13 and A-14 are proposals for synthetic sounds, not values measured in the source. |
| Q4 | The onset error is measured on very short frames and is noisy (the source reports 0.6 to 1.5 octaves of sharpness difference on half the references). Keep its weight in the score? | Keep it, capped at 2 as specified, so the choice matches the source's. |
| Q5 | The post stage's envelope calibration exists but is off by default. Should hark implement it? | Implement it behind the same off-by-default switch, so the option is not lost. |
| Q6 | May the bank fit regeneration download the 15 CC0 recordings from Freesound at build time? | Ask the operator. Without a download, the bank fits are NEEDS-DATA and the fallback applies. |
| Q7 | A-1 .. A-4 state the source's measured values as hard targets. What tolerance applies to a hark result just under them? | Treat a result within 0.01 CLAP and 0.25 dB as a pass, and anything worse as a regression to investigate. |
