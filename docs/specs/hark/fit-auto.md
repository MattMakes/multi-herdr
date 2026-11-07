# hark fit: the fit engine, choosing a model (part of fit.md)

This part specifies how the engine turns a recording into one model: the bed strip, the sound's character, the
candidate plan, hybrids, the post stage (EQ, level, attack clamp, gate), scoring, the confirm and CLAP margin rules,
the bed check and the finish. Rendering, takes, knobs and morphs are in `fit-render.md`. The index is `fit.md`.

Module: `hark.forge.fit.auto`. Pure numpy and scipy. CLAP only with `clap=True`.

## Interface (fitting side)

| name | contract |
|---|---|
| `fit(x, kind='auto', layers=None, candidates=None, clap=False, fast=False, seeds=(0,), name=None, verbose=False, collect=None, post=True, bed=True, space=True, confirm=None)` | A model of x (path or 48 kHz array). |
| `fit_kind(x, kind, fast=False, name=None, **opts)` | One fitter: `modal`, `sines`, `spectral`, `gesture`. Other names raise `ValueError`. |
| `fit_hybrid(x, layers=('modal', 'spectral'), fast=False, base=None, name=None)` | A hybrid model. |
| `deterministic(model)` | A copy of a modal or sines model without its noise. Other kinds raise `ValueError`. |
| `character(x)` | The measurement dict. |
| `plan(c, max_candidates=None)` | The ordered candidate list. |
| `bed_of(x)`, `strip_bed(x, bed)`, `bed_noise(bed, n, seed=0)` | The bed. |
| `fit_post(X, renders, iters=2, max_db=12, envelope=False, gate=True, judge=None, sharp=True, noise=False)` | A post stage. |
| `apply_post(y, post, scale=1, tail=1, tmap=None, anchor=0)` | A render through a post stage. |
| `kind_of(model)` | `modal`, `sines`, `spectral` (for kind `spectral_noise`), `gesture`, `hybrid`, `morph`. Else `ValueError`. |
| `channels_of(x_or_model)` | 1 or 2. |
| constants | `KINDS = ('modal', 'spectral', 'sines', 'hybrid', 'gesture')`, `ROLE = {'sines': 'tone', 'modal': 'body', 'spectral': 'air'}`, `HYBRIDS`, `PLANS`, `LONG_PLANS`, `ALL`, `TAKE_SEEDS = (0, 1, 2)`, `TAKE_VARY`, `CONFIRM = 3`, `CONFIRM_SEEDS = (2, 3)`, `CLAP_MARGIN = 0.02`, `CLAP_TOL = 1.5`, `CLAP_ENV_TOL = 0.05`, `BED_CHECK = True`, `NOISE_KINDS = ('spectral', 'gesture')`, `KNOBS` (see `fit-render.md`). |

### Hybrid model (plain JSON)

`{'kind': 'hybrid', 'version': 1, 'sr': 48000, 'length_s', 'channels', 'layers': [{'role': 'tone' | 'body' | 'air',
'gain_db': 0.0, 'model': <a sines, modal or spectral model>}, ...], 'source'}`. Every layer but the last is a
deterministic model (`layer: "deterministic"`). The last is a spectral model.

Hybrid names: `modal+spectral`, `sines+spectral`, `sines+modal+spectral`. A candidate name is `hybrid:<name>`.

### Fields the engine adds to any model

| field | meaning |
|---|---|
| `post` | The post stage: `eq_db` (28 gains), `gain_db`, optional `pre`, optional `env`, optional `gate`, optional `bed`, optional `space`. |
| `vary_scale` | 0.5 for a modal or hybrid winner (see `fit-render.md`). |
| `channels` | Set on sines and spectral fits from the input. |
| `source` | The name. |
| `fit_engine` | Diagnostics. Single kind: `{'kind', 'seconds'}`. Auto: `{'kind': 'auto', 'chosen', 'character', 'candidates', 'seeds', 'seconds', 'space'?, 'bed'?}`. |

A candidate row: `candidate`, `score` (3 decimals), `fit_s`, `quality` (the seed-0 quality dict), and optionally
`score2`, `clap2`, `chosen_by: 'clap'`, or `error` (the exception type and message). The bed check adds a row named
`plain:<kind>`. The bed entry: `level_db`, `t_on`, and after the check `score_stripped`, `score_plain`, `kept`.

## Behaviour of `fit` (kind auto)

0. Load X0. When `bed` is true and `bed_of(X0)` finds a bed, X = `strip_bed(X0, bed)`. Fitters and the post stage see
   X. Candidates are scored against X0 with the bed rendered back under them.
1. `character(X)` and `plan` (or the given candidates).
2. With `clap`, embed X0 once.
3. For each candidate in order:
   - Fit it (see Candidate fits). A failure records an error row and the loop goes on.
   - With `post`: the post stage from renders at seeds 0 and 1 (not finished), with the judge (CLAP) and the noise
     flag for spectral and gesture.
   - Attach the bed (not the vary scale yet).
   - Render at each seed of `seeds` (the full render: post, bed, finish), measure quality against X0 (with CLAP when
     on), score. The candidate score is the mean.
   - Keep the lowest.
4. All failed: `RuntimeError` naming the errors.
5. Confirm (default 3; 0 or 1 is off; only with one seed, more than one kept candidate and confirm seeds not in
   `seeds`): the 3 best by seed-0 score are scored on seeds 2 and 3 too. `score2` is the mean of the 3 scores,
   `clap2` the mean of the 3 CLAP values. The choice is the lowest `score2`. Reason: on short noise-led sounds one
   seed's CLAP spread 0.84 to 0.92 and its score 7 to 13, and the seed-0 choice took the lucky draws. Seeds 0 and 1
   are not used because the post stage is fitted on them.
6. CLAP margin rule (only after a confirm, with CLAP):
   - Alternatives: candidates with seed-0 CLAP at least 0.02 over the choice's and seed-0 score at most 1.5 over it.
     Score the alternatives on the confirm seeds when not done.
   - Pick: alternatives whose `clap2` is at least 0.02 over the choice's, `score2` at most 1.5 over it and `env_corr`
     at most 0.05 under it. The one with the highest `clap2` wins and gets `chosen_by: 'clap'`.
   - Reason: the model-free terms decided near-ties against a clearly closer sound. On a held-out check 5 of 6
     regressions were the gesture model chosen with 0.015 to 0.06 less CLAP. On dev clips 5 choices changed, all to a
     higher CLAP. A score tolerance of 4 also took a candidate with an envelope r of 0.58, hence the envelope guard.
7. Bed check (with a bed): fit the winner's kind on X0 as it is (a gesture winner first fits sines on X0 for its
   tones), its post stage on X0 (judge without bed), score on the same seeds (plus the confirm seeds when confirmed).
   Add a `plain:<kind>` row. When it scores lower, use it and drop the bed (`kept: false`). An error keeps the
   stripped fit and records the row. Reason: a whoosh's own steady approach 13.6 dB under its peak read as a bed,
   and every stripped candidate fell (CLAP 0.88 to 0.64). On 12 dev clips with a bed the plain fit won on 8.
8. Finish: with `post`, the space layer (below) on the chosen model. Set the vary scale and the bed. Store
   `fit_engine` (with the space fit diagnostics and the tail correlation, and the bed entry).

### Single kind

`kind` not auto: fit that kind on X (hybrid with `layers`, default modal + spectral). With `post`: post stage from
seeds 0 and 1 without a judge, noise flag by kind, then the space layer. Set the vary scale and the bed.
`fit_engine = {'kind', 'seconds'}`.

### Candidate fits

- `hybrid:<name>`: `fit_hybrid` with that name's layers, reusing the single-kind fits already made for the first
  layer.
- `gesture`: the gesture fitter with the sines candidate's model as its tones (null when sines was not fitted, so the
  gesture fitter fits its own). So gesture always comes after sines.
- Others: `fit_kind`, and the model is kept for reuse.

`fit_kind` fast settings: modal 1 pass; sines window 1024 only; spectral no texture, 1 iteration, seed 1; gesture
fast. Sines and spectral models get `channels` from the input.

## Algorithms

### Character

On the mono sum:
1. 1 ms envelope `e` (dB). Peak frame and level. Onset: the first frame within 30 dB of the peak. `t40_ms`: frames
   from the peak to the first frame 40 dB under it (or to the end). Active: within 40 dB.
2. `rise_db`: on a 0.5 ms envelope, the largest rise over 4 frames (2 ms) among frames within 40 dB of the maximum.
3. `crest_db`: `20 log10(max |m|)` minus the dB of the mean power over active 1 ms frames.
4. Spectrum: centred Hann (symmetric `n_fft + 2` without the ends) STFT, 2048 / hop 256, bins 60 Hz to 16 kHz. Active
   frames: frame energy above `max x 10^-3.5`.
5. Peaks: bins that equal the 3-bin maximum, stand more than 12 dB over an 21-bin median, and are within 50 dB of the
   frame maximum. Peak mask widened to 5 bins.
6. `tonal_share`: per frame the share of power under the mask, weighted by frame energy over active frames.
7. `flatness`: per frame the geometric over arithmetic mean of power, then the energy-weighted geometric mean.
8. `persistence`: over consecutive active frames, the share of peak power whose bin is still under the next frame's
   mask.
9. Glide (below): `glide_oct`, `track_ms`.
10. `strikes`: the count of modal onsets.
11. `channels`.
12. Flags:
    - `impulsive`: attack at most 40 ms and (t40 at most 400 ms or crest at least 18 dB).
    - `tonal`: tonal share at least 0.45 and persistence at least 0.5.
    - `noisy`: tonal share under 0.25.
    - `gliding`: `|glide| >= 2/12` octave, track at least 15 ms, tonal share at least 0.25.
13. Class: `glide` when gliding. Else `ringing` when tonal share at least 0.8, persistence at least 0.9 and t40 over
    300 ms. Else `struck` when impulsive. Else `gesture` when attack over 40 ms and crest under 18 dB. Else `mixed`.

Output fields: `dur_s`, `attack_ms`, `rise_db`, `t40_ms`, `crest_db`, `tonal_share`, `persistence`, `glide_oct`,
`track_ms`, `flatness`, `strikes`, `channels`, `impulsive`, `tonal`, `noisy`, `gliding`, `class`.

### Glide

1. STFT 1024 / hop 128 (21 ms frames every 2.7 ms), bins 60 Hz to 16 kHz.
2. Peaks: 3-bin maximum, 15 dB over an 11-bin median, within 50 dB of the global maximum.
3. Per active frame (energy above `max x 10^-3.5`) with a peak: the loudest peak, parabolic refinement (only for a
   negative curvature), log2 of `max(f, 20)`, and its level.
4. Need 4 points. Keep points within 15 dB of the loudest. Need 4 again.
5. Break the track where the step is at least 1/6 octave or the time gap is more than 2.5 hops. Take the longest piece.
6. `track_ms` = its span. With under 4 points, glide 0. Else glide = median of the last fifth minus median of the first
   fifth (octaves).

The 2 semitones over 15 ms rule fixed a gate that before never fired on 2 to 6 semitone pops and dings.

### Plan

| class | under 1 s (then every other in ALL) | 1 s or longer (4 only) |
|---|---|---|
| glide | sines, hybrid:sines+modal+spectral, hybrid:sines+spectral, modal | sines, gesture, hybrid:sines+modal+spectral, hybrid:sines+spectral |
| ringing | sines, modal, hybrid:sines+spectral, spectral | sines, gesture, modal, hybrid:sines+spectral |
| struck | modal, hybrid:sines+modal+spectral, hybrid:modal+spectral, sines | modal, sines, hybrid:modal+spectral, spectral |
| gesture | spectral, hybrid:sines+spectral, modal, sines | sines, gesture, modal, hybrid:sines+spectral |
| mixed | modal, spectral, hybrid:modal+spectral, sines | modal, sines, gesture, hybrid:modal+spectral |

ALL = modal, sines, gesture, spectral, hybrid:modal+spectral, hybrid:sines+spectral, hybrid:sines+modal+spectral.

The order is the class list, then the rest of ALL. The maximum is 4 for long sounds and 7 otherwise. When gesture
comes before sines, move it to just after sines. Reasons: under 1 s each fit takes 0.5 to 4 s (gesture 3 to 60 s),
so all are tried. The 3-layer hybrid alone took 147 s on a 4.5 s impact.

### Bed detection

Purpose: a recording with a steady pre-roll 12 to 40 dB under the sound (a film's music) is stripped, and the sound
alone is fitted.

1. Mono. Return null when shorter than 30 ms or silent.
2. 1 ms frame envelope. Onset frame k: the first k from frame 35 (30 ms + 5 ms) up to frame 119 (under 120 ms) with
   a level within 30 dB of the top and at least 10 dB over the median of frames 3 to k-3.
3. A modal onset within 3 ms (144 samples) of `k x 48` refines it (the nearest).
4. Pre-roll: from 3 ms to 2 ms before the onset. At least 30 ms.
5. Level = pre-roll power dB minus the loudest 50 ms RMS. Quarters' levels. Null unless -40 <= level <= -12, the
   quarters span at most 6 dB, and the last quarter is at most 3 dB over the first (not rising: a swell's own start).
6. Spectrum: the per-bin median of STFT power over whole frames only (Hann 1024, hop 256, scipy scaling, no padding).
7. Return `{'t_on' (s, 6 decimals), 'level_db', 'n' (samples), 'nfft': 1024, 'psd_db' (1 decimal per bin)}`.

### Bed strip

1. STFT of the mono mix (Hann 1024, hop 256, padded). Gain per cell
   `sqrt(max(1 - 2 x bed power / cell power, 10^(-25/10)))`, smoothed over 3 frames (causal moving mean on an
   edge-padded grid, aligned).
2. Apply the same gain to each channel's STFT. Inverse. Cut or pad to n.
3. Silence the pre-roll: 0 up to 2 ms before the onset, a 1.5 ms raised-cosine rise ending 0.5 ms before the onset,
   1 after.

### Bed noise (the bed layer)

1. Length `nb = min(n, bed n)`. Zero when 0.
2. White noise's median STFT power per bin under scipy scaling: `c = sum w^2 / (sum w)^2 x ln 2`, w = periodic Hann.
3. Lines: bins at least 10 dB over a 15-bin median that are local maxima. Each becomes a steady sine:
   parabolic frequency refinement (negative curvature only), amplitude `sqrt(4 x (sum of the 3 bins' power) / 1.5)`
   (a Hann peak bin holds `A^2/4`, its neighbours `A^2/16`), a random phase. The 3 bins then take the median level.
   Reason: a music bed's steady partials as noise would flicker 5 to 10 dB.
4. Noise: STFT of `nb + 1024` white samples times `sqrt(power / c)`, inverse, cut to nb, plus the lines.
5. 2 ms linear fade-in, 10 ms linear fade-out. Zero after nb.
6. Generator `[seed & 0xFFFFFFFF, 77]` (one stream: phases, then the noise).

### Hybrid fit

Layers: 2 or 3 of modal, sines, spectral, the last not sines. Else `ValueError`.

1. X as `(n, channels)`. Residual = X.
2. For each layer: the first layer reuses a base fit of its kind when given. Else fit the kind on the residual.
3. Every layer but the last becomes deterministic, and the residual loses what the layer takes:
   - The layer's seed-0 render (not finished) as `(n, channels of X)` (mono repeated, other channel counts mean-mixed).
   - A sines layer with a fitted side on a stereo X: try 8 stereo renders (seeds 0 to 7) and take the one that leaves
     the least side `sum((r_L - y_L - r_R + y_R)^2)` in the residual, or the mono render when none leaves less.
     Reason: the hybrid renders that side, so the air must not model it again (a panned tone's side, counted twice,
     came out about 3 dB too wide). The side's quadrature part has a random sign per seed, hence the search.
4. Layer entry: role by kind, gain 0, the model.

Deterministic copy: modal noise envelopes all at -240 dB, side null, no `side_t`, `channels` 1. Sines residual one
silent band, width -120, `channels` 1. Both drop `post` and `fit_engine` and set `layer: "deterministic"`. (A sines
layer inside a hybrid keeps its `side` model for the stereo render.)

### Post stage

Inputs: X (the fitted signal), renders (2 seeds, not finished), a judge or null, the noise flag.

Score function `sc(p)`: without a judge, the mean score of `quality(X mono, apply_post(y, p))` over the renders, and
CLAP 0. With a judge, the judge's `(mean score, mean CLAP)` of the post-processed renders.

1. EQ, 2 iterations:
   - Current profile: the dB mean over renders of the power of each render's tone profile (after the current EQ).
   - `d = target - current`. Remove the power-weighted mean (level neutral, centred on loud bands). Where the
     current band is under -50 dB, `d` is at most 6 (EQ cannot create what is not there). Smooth with 1/4, 1/2, 1/4
     (edge padded).
   - Total `clip(total + d, -12, 12)`.
   - The EQ is the forge-core minimum-phase third-octave EQ with 28 gains at the tone band centres.
2. Gain: the original's loudest 50 ms minus the mean of the renders' after EQ.
3. Attack (when the 0.25 ms envelope has more than 4 frames):
   - Onset frame: the first within 20 dB of the maximum. Main strike: the loudest frame from the onset over 25 ms.
     Attack length: `max(6 ms, peak - onset + 4 ms)` in frames.
   - Struck: the original's sharpness (quality attack) at least 15 dB/ms.
   - When the onset frame is at least 2, or struck with more than 4 frames after it: `pre = {'t_on', 'db' (the
     envelope before the onset), 'attack_db' (the attack frames, struck only, else empty), 'v': 2}`. Noise kinds add
     `hp_hz: 200`.
   - Re-level: add the gain difference after the clamp.
   - Noise kinds: drop the clamp (back to EQ and the first gain) unless it lowers `sc`. Measured: a whoosh's gesture
     fit 10.46 to 8.67 without the clamp, a swish 12.68 to 11.37 high-passed, a glitch run keeps the plain clamp
     (9.29 against 10.09).
4. Envelope calibration (off by default): the mean 5 ms envelope error after the attack as a gain curve
   (`{'hop': 0.005, 'db'}`, clip -12 to +6, 0 where both are 70 dB under the top, zero before the attack end + 2 ms,
   smoothed twice with 1-2-1, the first 2 frames faded in by 0.33 and 0.67), then one more EQ and level pass. Reason
   it is off: it lowered mel and raised envelope r, but CLAP fell from 0.925 to 0.918.
5. Gate (on by default): fit the gate on the post-processed renders. When one is found, re-level and keep it when
   `score < score without` and `CLAP >= CLAP without - 0.005`.

### Gate fit

Purpose: modes and partials decay exponentially and cannot stop. A gated pulse falls 30 to 60 dB in 5 ms. The gate
is a cut-only gain curve where the renders stand over the original after the onset: the gaps of a run of pulses, a
hard stop, a tail the model lets ring on.

1. Envelopes on a 0.5 ms grid, frame k covering 2 hops (1 ms windows, half overlapping). Need 8 frames and a render.
2. Render envelope: the power mean over renders (padded with -200 dB). 3-frame median on both.
3. Fell: the original is at least 12 dB under its maximum over the previous 20 ms (a maximum filter of 41 frames
   ending at the frame), or at least 40 dB under its top.
4. Start frame from `pre.t_on`, or the first frame within 20 dB of the maximum.
5. Dips between pulses: a 10-frame mean of the original. Peaks with prominence 6 dB, distance 16 frames, within 25 dB
   of the maximum. Between 2 neighbouring peaks, frames more than 6 dB under the lower peak are dips.
6. Hit: `(d < -4 and fell) or (d < -2 and dip)` with `d = original - renders`.
7. Cut = `max(d + 1, -80)` on hits where the renders are above `top - 90`, else 0. At most 0. Zero before the start.
8. The last 12 ms is the clip's own edit fade: from there the cut holds the value just before (0 when that is before
   the start).
9. For each run of negative cut: drop it when it does not reach -6 dB, or when it is shorter than 3 ms and does not
   reach the end. Runs of 5 or more frames: a 5-frame median then 1-2-1 smoothing. At most 0. When the run does not
   reach the end, its last frame is 0 (the gate reopens a frame before the original rises, so the next pulse's onset
   is never cut).
10. None when no frame is under -0.5 dB.
11. Breakpoints at frame centres `(k + 1) x 0.5 ms`, Ramer-Douglas-Peucker at 0.5 dB:
    `{'pts': [[t s (5 decimals), dB (1 decimal)], ...]}`. 0 dB before the first point. The last value holds past the
    end, so a hard stop stays shut in a take that rings out.

Measured: on 67 gated, multi-pulse and short-click dev clips the gate error fell from 21 to 1.7 dB. A gate that
helped every model-free measure still cost CLAP on 4 of 67, hence the CLAP guard.

### Apply post

In order, on a mono or stereo render:
1. EQ (when any gain is non-zero).
2. Gain.
3. Attack clamp (when `pre`).
4. Envelope gain (when `env`), time-scaled by `scale x tail` about the anchor.
5. Gate (when `gate`): per-sample linear gain from the breakpoints (interpolated in dB, 0 before, last value after),
   with the time map (default: times multiplied by `scale` when it is not 1), then smoothed by a normalised Hann of
   `round(1 ms x sr) | 1` samples (49) on an edge-padded signal. Reason: breakpoints sit 0.5 ms apart, and a 30 to 80
   dB step between 2 of them put an audible click into a loud ring (a pop's gesture takes: 4 of 4 clicked).

### Attack clamp

Purpose: band noise and window smear leak energy before a strike and blunt it. The clamp keeps the attack as clean
as the recording's.

1. Frames of 0.25 ms (12 samples). Pre-onset frame count: `t_on x scale / 0.25 ms`, or with an anchor and a scale not
   1, `(anchor + (t_on - anchor) x scale) / 0.25 ms`. Attack frames: the stored count, times scale when not 1.
2. `nb = min(len / 12, pre + attack)`. Return the input when under 2.
3. Envelope of the render's mono over those frames.
4. Pre-onset: the original's pre-roll envelope resampled onto the render's pre-onset frames (through the anchor map
   when stretched). Gain `min(0, want + 1.5 - e)`: only cut, 1.5 dB headroom.
5. Attack: the original's attack contour resampled onto the attack frames. Gain `clip(want - e, -9, 9)`, faded out
   linearly over the last 2 ms (8 frames).
6. Smooth: with `v = 2`, smooth the pre-onset part and the attack part apart (else together). Smoothing widens each
   cut by one frame on each side (a minimum filter on the negative part) and applies 1-2-1. Reason for apart: the
   joint smoothing carried half of a -20 dB pre-onset cut into the strike's first frame.
7. Gain per sample: interpolate the frame gains in dB at frame centres, 0 dB at the end of the clamped span, linear.
8. Without `hp_hz`: multiply the clamped span. With `hp_hz`: the correction `y (g - 1)` over the span plus 20 ms is
   high-passed (4th-order Butterworth at 200 Hz, zero phase) and added. The correction cannot put energy into the
   bands under 200 Hz (a whoosh got +8 dB at 31 to 99 Hz without it).

### Space layer

With `space` on, after the choice: when the recording X is stereo and needs space (tail correlation at most 0.7),
fit the space model on X with the dry = the chosen model's seed-0 render through its post stage (without bed and
space). When found, it becomes `post['space']`. Cost 0.6 to 1.6 s. Fitted per candidate it changed no choice, so it is
fitted on the winner only.

### Judge

With CLAP: renders get the bed layer under them (bed seeds 0, 1, ... by position). The judge returns the mean score
with each render's CLAP cosine and the mean CLAP.

## Constants

| name | value |
|---|---|
| take high-pass | 8 Hz, 2nd order |
| take seeds | 0, 1, 2 |
| confirm | 3 best, seeds 2 and 3 |
| CLAP margin / score tolerance / envelope tolerance | 0.02 / 1.5 / 0.05 |
| vary scale | modal 0.5, hybrid 0.5 |
| glide | 2 semitones, 15 ms track, STFT 1024 / 128, peaks 15 dB over an 11-bin median |
| character STFT | 2048 / 256, 60 Hz to 16 kHz, peaks 12 dB over a 21-bin median, mask 5 bins |
| class thresholds | attack 40 ms, t40 400 ms (ringing 300 ms), crest 18 dB, tonal 0.45 / 0.25 / 0.8, persistence 0.5 / 0.9 |
| long sound | 1 s, 4 candidates |
| bed | level -40 to -12 dB, pre-roll at least 30 ms, onset within 120 ms, steady 6 dB (rising at most 3 dB), rise 10 dB, refine 3 ms, STFT 1024 |
| bed strip | over-subtraction 2, floor -25 dB, 3-frame smoothing, fade 1.5 ms ending 0.5 ms before the onset |
| bed lines | 10 dB over a 15-bin median |
| bed fades | 2 ms in, 10 ms out |
| post EQ | 2 iterations, 12 dB, 28 bands, 6 dB boost cap under -50 dB |
| attack grid | 0.25 ms |
| attack window | at least 6 ms, or to 4 ms past the loudest frame in 25 ms |
| sharp onset | 15 dB/ms |
| clamp | +1.5 dB before the onset, +-9 dB in the attack, 2 ms fade |
| noise clamp high-pass | 200 Hz, 4th order, zero phase |
| gate grid | 0.5 ms hop, 1 ms window |
| gate thresholds | tolerance 4 dB, keep 1 dB, minimum cut 6 dB, fall 12 dB over 20 ms, quiet 40 dB, minimum 3 ms, dip 6 dB with tolerance 2 dB, depth -80 dB, edit 12 ms, breakpoint tolerance 0.5 dB, CLAP tolerance 0.005 |
| gate map | 2 ms (see `fit-render.md`) |
| gate smoothing | 1 ms Hann |
| hybrid side search | 8 seeds |

## Errors and edge cases

- Every candidate fails: `RuntimeError`.
- A failing candidate does not stop the others. Its row holds the error.
- A failing bed check keeps the stripped fit.
- A recording with no bed skips the bed check.
- `fit_hybrid` with bad layers raises `ValueError`.
- Without `post` the model has no post stage and no space layer.
