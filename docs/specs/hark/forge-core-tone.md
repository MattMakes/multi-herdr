# forge-core part 3: model-free tone and envelope checks

Index: [forge-core.md](forge-core.md). This part covers module `hark.forge.tone`: the 1/3-octave tonal balance of a
sound against real references (`ToneRef`) and its band-energy shape over time against them (`EnvRef`).

Purpose. The machine-learning checks (text-audio match, typicality, realness) saturate once a search has tuned
against them: made sounds then land inside or above real recordings on every model check, also on held-out text
phrasings and an independent tagger. A plain 1/3-octave measurement still separates them. In the source project, a
made key press sat 8-13 dB from the real keys' balance while the real keys sat 3-6 dB from each other. A room and a
microphone noise floor cannot fool these measurements. A blind test also caught a forged impact by its tail, which
an average spectrum cannot hear, so a second check compares band energy over time.

All measurements use the mono sum (channel mean) of the whole file at 48 kHz. A file path argument is loaded with
the core-io loader at 48 kHz.

## Interface

Module constants:

| name | value |
|---|---|
| `SR` | 48000 |
| `CEN` | 28 band centres 1000 x 2^(k/3), k = -15 .. 12 (31.5 Hz to 15.9 kHz) |
| `EDGE` | 2^(1/6) (a band spans [c / EDGE, c x EDGE)) |
| `REGIONS` | `{'<150': (0, 150), '150-1k': (150, 1000), '1-4k': (1000, 4000), '4-12k': (4000, 12000), '>12k': (12000, 24000)}` (Hz, [lo, hi)) |
| `FLOOR` | -70.0 dB (bandgram floor) |
| `HOP_MS` | 10.0 |
| `STRETCH` | (0.85, 0.92, 1.0, 1.08, 1.18) |
| `SHIFT` | (-3, -2, -1, 0, 1, 2, 3) frames |
| `SEGMENTS` | pre (-inf, -2), attack [-2, 3), body [3, 20), tail [20, 100), late [100, inf) frames from the anchor |
| `GROUPS` | 8 band groups by centre frequency (Hz, [lo, hi)): `31-80 Hz` [0, 90), `100-200 Hz` [90, 224), `250-500 Hz` [224, 560), `630-1250 Hz` [560, 1400), `1.6-3.2 kHz` [1400, 3550), `4-6.3 kHz` [3550, 7100), `8-10 kHz` [7100, 11200), `12.5-16 kHz` [11200, inf) |
| `OUT_MARGIN` | 6.0 dB |

Functions:

| function | returns |
|---|---|
| `label(c)` | band label: `'<c/1000 with 3 significant digits> kHz'` when c >= 1000, else `'<c rounded> Hz'` |
| `profile(x)` | 28 floats, dB |
| `shares(x)` | dict region -> dB, 1 decimal |
| `t40_ms(x, db=40.0, ms=5.0)` | float ms |
| `bandgram(x, hop_ms=10.0)` | array (frames, 28), dB |
| `env_dist(A, B, stretch=STRETCH, shift=SHIFT, detail=False)` | float, or (float, info dict) with detail |

Class `ToneRef(refs, margin=3.0)`: attributes `n`, `margin`, `P` (n x 28 profiles), `med`, `lo`, `hi` (per-band
median, min, max; None with no refs), `t40` (array), `S` (list of shares dicts), `len_s` (array of lengths in s),
`ref_dist` (leave-one-out distances, only with 3 or more refs, else empty). Property `spread`. Methods
`check(x, exclude=None)`, static `words(m)`, static `region_penalty(m, regions=('<150', '4-12k', '>12k'),
margin=6.0, scale=6.0)`, `penalty(m)`.

Class `EnvRef(refs, names=None, margin=OUT_MARGIN)`: attributes `names`, `margin`, `G` (list of bandgrams), `n`, `D`
(n x n distances, inf on the diagonal), `cell_spread` (dict), `out_ref` (array). Methods `spread(exclude=None)`,
`check(x, exclude=None, G=None)`, static `words(m)`, static `penalty(m, cap=2.0)`.

`refs` items are file paths or 48 kHz arrays.

## Algorithms

### `profile(x)`

Power spectrum |rfft(m)|^2 of the whole mono file. For each centre c, sum the bins with frequency in [c / EDGE,
c x EDGE). Add 1e-20 to every band. Return max(10 log10(band / sum of bands), -60). Level never decides: the profile
is relative to the file's total power in these bands.

### `shares(x)`

Power spectrum as above. Total = sum of all bins + 1e-20. For each region [a, b): 10 log10(sum of bins in the region
/ total + 1e-12), 1 decimal.

### `t40_ms(x, db=40, ms=5)`

Frames of n = SR ms / 1000 samples (240 at 5 ms), k = floor(len / n) frames; return 0.0 when k < 2. Frame level
e = 20 log10(RMS + 1e-12). Peak frame i. The first frame after i (counting i as 0) whose level is more than `db`
under the peak gives index j; when none, j = the number of frames from i to the end. Return j x ms.

### `ToneRef`

Construction: load refs; profiles, median, per-band min and max, t40 per ref, shares per ref, lengths. With 3 or more
refs, `ref_dist[i]` = RMS over bands of (P[i] - median of the other refs' profiles).

`spread` = (median of ref_dist, max of ref_dist), or None with fewer than 3 refs.

`check(x, exclude=None)`:
1. Return `{}` when there are no refs, or when `exclude` removes the only ref.
2. keep = all ref indices except `exclude`. med, lo, hi over the kept profiles.
3. p = profile(x); dev = p - med; tone_db = sqrt(mean(dev^2)).
4. sp = ref_dist of the kept refs (empty when ref_dist is empty). tone_ratio = tone_db / max(median(sp), 0.5) when sp
   has at least 2 values, else None. (The kept refs' leave-one-out distances were computed against all refs; this is
   a known small leak.)
5. Per band: over = max(p - hi - margin, 0) - max(lo - p - margin, 0). `outside` = list of (label, dev rounded to
   1 decimal, 'above' or 'below') for every band with over != 0.
6. Return:
   `tone_db` (2 decimals), `tone_ratio` (2 decimals or None), `tone_ref` (sp, 2 decimals), `band_dev` (28 values,
   1 decimal), `outside`, `t40_ms`, `t40_ref` (min, max of kept refs), `length_s` (3 decimals), `shares`,
   `shares_ref_med` (per region, median over kept refs, 1 decimal), `length_ref` (min, max of kept lengths,
   3 decimals), `words`.

`words(m)` builds one line:
1. `tone <tone_db:.1f> dB from the refs' median`.
2. When tone_ref is not empty: ` (the refs sit <min:.1f>-<max:.1f> apart)`.
3. The 3 outside bands with the largest |dev|: `: <dev:+.0f> dB at <band> (<above|below> every ref)`, comma-joined.
4. With t40_ref = (a, b): when t40 > 1.5 b + 10 or t40 < 0.6 a - 5: `; tail t40 <t40:.0f> ms (refs <a:.0f>-<b:.0f>)`.
5. Regions whose share differs from the refs' median by more than 9 dB: `; energy share vs the refs: <region>
   <diff:+.0f> dB`, comma-joined.
6. When length > 1.3 x the longest ref: `; <length:.2f> s long (refs <lo:.2f>-<hi:.2f>)`.

`region_penalty(m, regions, margin, scale)` = sum over the regions of max(0, share - ref median share - margin) /
scale. It counts a low puff or a bright hiss on a mid-band sound: 1 per 6 dB beyond median + 6 dB.

`penalty(m)`: 0 when m is empty or tone_ratio is None. Else max(0, tone_ratio - 1) (0 while the sound is no farther
from the median than a typical real recording). Plus, when the longest ref t40 b > 0 and t40 > b:
0.5 x min(2, (t40 - b) / b).

### `bandgram(x, hop_ms=10)`

A filter bank of 28 band-passes, 3rd-order Butterworth, band [c / EDGE, min(c x EDGE, 0.49 SR)], built once and
reused. The low bands smear in time, like an ear. Frames of n = 480 samples (10 ms); k = ceil(len / n) frames
(at least 1); zero-pad the mono signal to k n. For each band, filter (causal) and take the mean square per frame.
Return max(10 log10(G / (max over frames of the frame's sum over bands + 1e-30) + 1e-30), FLOOR): dB relative to the
loudest frame's total power, floored at -70 dB.

### `env_dist(A, B, ...)` (A = the sound judged, B = a reference)

1. Total level per frame: tot = 10 log10(sum over bands of 10^(G / 10) + 1e-30).
2. Anchors of each bandgram: peak = argmax tot; onset = the first frame with tot >= tot[peak] - 20; the active span
   is from the first to the last frame with tot >= tot[peak] - 50.
3. Fractional row sampling: rows(G, t) linearly interpolates in dB between floor(t) and floor(t) + 1; a row outside
   the bandgram is FLOOR.
4. For anchor name in (onset, peak): a0 = A's anchor, b0 = B's anchor. Relative frame range: lo = max(-300,
   min(a_lo - a0, b_lo - b0)) - 3; hi = min(600, max(a_hi - a0, b_hi - b0)) + 4; tt = integers lo .. hi - 1.
   Bv = rows(B, b0 + tt).
5. Cost for (anchor, stretch s, shift sh): Av = rows(A, a0 + tt / s + sh). Weight w = clip((max(Av, Bv) - FLOOR) / 40,
   0, 1) per cell (how loud the louder of the two is). d = sum(w min(|Av - Bv|, 30)) / (sum w + 1e-9). Cost =
   d (1 + 0.5 |ln s|) (stretching costs a little).
6. Coarse search: the minimum cost over both anchors and every stretch, with shift 0. Fine search: with the best
   anchor and stretch, try every non-zero shift; keep the minimum (the coarse best included).
7. Return the cost of the best. The tolerance is small on purpose: a tail half as long as the ref's cannot be
   stretched away.
8. With detail: cells. W = sum w + 1e-9. For each segment (frame range in tt) and each band group (centre frequency
   range): ww = w over those rows and bands. Skip when sum ww < 0.01 W. diff = sum(ww (Av - Bv)) / sum ww (signed:
   made minus ref). Cell key `'<segment> <group>'`, value (diff rounded to 1 decimal, sum ww / W rounded to 3
   decimals). Info dict: `{'anchor', 'stretch', 'shift_ms' (shift x 10), 'cells'}`.

### `EnvRef`

Construction:
1. Names: given names, else the ref's path when it is a string, else `ref<i>`.
2. Bandgrams of all refs. D[i, j] = env_dist(G_i, G_j) for i != j (ref i judged against ref j), with detail.
3. `cell_spread[key]` = median of |diff| over all ordered ref pairs that have the cell, kept when at least 2 pairs
   have it. It says how far real refs sit from each other in each cell.
4. `out_ref[i]` = the excess (below) of ref i against all other refs. These cell spreads use all refs, a small leak
   in leave-one-out scoring.

Outside cells for a list of detailed results (one per compared ref), only with 2 or more results:
- For every cell key of the first result that all results have: diffs = the signed diffs; share = mean of the shares.
  lo = min(diffs) when all diffs > 0; -max(diffs) when all diffs < 0; else 0. (lo is how far the sound is beyond
  every ref in one direction.) tol = max(margin, cell_spread of the key, else margin).
- When lo > tol and share >= 0.02: list (key, sign(diffs[0]) x lo rounded 1 decimal, share rounded 3 decimals), and
  add (lo - tol) x min(1, share / 0.05) to the excess.
- Sort the list by -|value| x share.
- Return (list, excess).

`spread(exclude=None)`: keep = all but exclude. With fewer than 3 kept, empty. Else, for each kept i, the minimum
D[i, j] over the other kept j.

`check(x, exclude=None, G=None)`:
1. `{}` when no ref is kept.
2. Bandgram of x (or the given G). Detailed env_dist against each kept ref. The nearest ref k has the smallest
   distance d.
3. sp = spread(exclude). env_ratio = d / max(median(sp), 1.0) when sp is not empty, else None.
4. Outside cells and excess against all kept refs.
5. oref = out_ref of the kept refs when n >= 3, else empty.
6. Return: `env_db` (d, 2 decimals), `env_ratio`, `env_ref` (sp, 2 decimals), `nearest` (the nearest ref's name,
   basename after replacing backslashes), `nearest_i`, `anchor`, `stretch`, `shift_ms`, `cells` (of the nearest),
   `outside`, `env_out` (excess, 2 decimals), `env_out_ref` (median of oref, 2 decimals, or None), `words`.

The reason for the outside test: nearest-ref distances flatter averages. With few, diverse refs, a sound in their
middle sits closer to each ref than the refs sit to each other, so env_ratio alone cannot find a gap. A cell where
the sound is beyond every ref in the same direction, where the refs agree, is a consistent gap no real recording
shares.

`words(m)`: `envelope <env_db:.1f> dB from the nearest ref <nearest>`; when env_ref is not empty, ` (the refs sit
<min:.1f>-<max:.1f> apart)`; when outside is not empty, `; beyond every ref: ` and the first 3 as `<key> <v:+.0f> dB`;
else the 2 cells of the nearest with |diff| >= 8 ranked by |diff| x sqrt(share): `: vs it <key> <v:+.0f> dB`.

`penalty(m, cap=2.0)`: 0 for an empty m. near = max(0, env_ratio - 1) (0 when env_ratio is None). tol = max(3,
env_out_ref or 0). Return min(cap, near + max(0, env_out - tol) / 6). So 6 dB beyond every ref in a cell holding at
least 5 % of the weight adds 1.

## Constants

All constants are in the Interface table. Additional numbers: profile floor -60 dB; ToneRef margin 3 dB; tone_ratio
denominator floor 0.5 dB; env_ratio denominator floor 1.0 dB; words thresholds: t40 above 1.5 b + 10 ms or under
0.6 a - 5 ms, region share differences over 9 dB, length over 1.3 x the longest ref; env_dist weight scale 40 dB, cell
error cap 30 dB, stretch cost 0.5 |ln s|, frame range clamp [-300, 600] frames plus margins 3 and 4; cell minimum
weight 1 %; outside minimum share 2 %, full-weight share 5 %; region penalty margin 6 dB and scale 6 dB; tail penalty
weight 0.5, capped at 2 x b excess; env penalty: tolerance floor 3 dB, scale 6 dB, cap 2.

## Errors and edge cases

- `ToneRef([])`: `check` returns `{}` and `penalty({})` is 0.
- 1 ref scored with exclude = 0: `{}`.
- With 1 or 2 refs, `ref_dist` is empty, `spread` is None, `tone_ratio` is None and the tone penalty is 0 (only the
  tail term can count when tone_ratio exists; with None the whole penalty is 0).
- `t40_ms` of a file shorter than 2 frames (10 ms) is 0.0.
- `EnvRef` with fewer than 3 refs: env_ratio is None, env_out_ref is None; the outside test still runs with 2 kept
  refs.
- A bandgram of silence: all frames at FLOOR; anchors fall on frame 0.

## Acceptance tests

T1. `profile(white noise 1 s)` rises about 1 dB per band (3 dB per octave) from 31.5 Hz to 16 kHz, within +-1.5 dB
    of a straight line in the bands above 100 Hz; the values are all >= -60.
T2. `profile(x)` equals `profile(0.01 x)` within 1e-9 (level-independent).
T3. `t40_ms` of an exponential decay with amplitude tau 100 ms is about 460 ms (40 dB = 4.6 tau), within one 5 ms
    frame plus 2 %.
T4. `ToneRef([a, b, c, d])` where all 4 are white noise with different seeds: `check(white noise)` gives tone_db under
    1 dB, tone_ratio near 1 (0.3 to 3), and no `outside` bands. `check(pink noise)` gives tone_ratio over 5 and
    `outside` bands above at low frequency and below at high frequency.
T5. `ToneRef.penalty` is 0 for tone_ratio 0.9 and 0.5 for tone_ratio 1.5 when the tail is inside the refs.
T6. `bandgram` of a 1 kHz tone burst has its maximum in the 1 kHz band, value 0 dB in the loudest frame.
T7. `env_dist(G, G)` is 0. `env_dist` of a sound against a copy shifted by 20 ms is under 1 dB (the shift and anchor
    tolerance remove it).
T8. `env_dist` of a decay with half the tail length of the ref is at least 3 dB (stretch cannot remove it).
T9. `EnvRef` of 4 similar decays: a sound with an extra loud tail (+15 dB in 200 ms-1 s across all bands) gets
    `outside` cells that start with `tail` and env_out > 0, and penalty > 0.
T10. `region_penalty` with shares `<150` = -3 dB and ref median -15 dB gives (12 - 6) / 6 = 1.0.
