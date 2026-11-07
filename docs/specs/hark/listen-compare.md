# hark listen: compare and ab

Part of the listen spec. Index: `docs/specs/hark/listen.md`. Embeddings and
calibration are in `docs/specs/hark/listen-calibration.md`.

## Purpose

- `compare` places one sound among a set of reference recordings: which
  references are nearest, whether the sound is typical of the set, and which
  measurements stand out against the set.
- `ab` judges two sounds or two trims (even of the same file): which is
  closer to a description and / or to a reference set, how similar they are,
  and how B differs from A in measurable terms an ear can hear.

## Interface

The CLI flags, signatures and top-level fields are in the index. Inner
formats:

### Trim-proof measurements (`feats`)

A dict with these keys (a key is absent when its value is unknown):

| Key | Unit | Definition |
|---|---|---|
| `loudness_lufs` | LUFS | gated integrated loudness (core-io) of the sound, zero-padded to at least 0.4 s; 1 decimal |
| `peak_dbfs` | dBFS | sample peak (core-io measurement) |
| `brightness_hz` | Hz | core-io brightness (median magnitude-weighted spectral centroid) |
| `bandwidth_hz` | Hz | core-io bandwidth |
| `attack_ms` | ms | core-io attack |
| `decay_ms` | ms | core-io decay |
| `length_s` | s | sounding length (C1), 3 decimals |
| `duration` | s | decoded length |
| `noise_floor_dbfs` | dBFS | core-io noise floor, floored at -120; only for sounds of 1 s or more |
| `width_db` | dB | side-to-mid ratio, floored at -40 (= mono); -40 when unknown |
| `low_end_db` | dB | energy sum of the `sub` (20-60 Hz) and `low` (60-250 Hz) bands: 10 log10(10^(sub/10) + 10^(low/10)); 1 decimal; only when both bands are known |

Silence before or after a sound does not change these values. So two trims of
the same sample, with different amounts of silence around it, read the same.

### compare `features[key]`

`{value, refs_median, robust_z, refs_p10_p90}`:

- `value`: the sound's measurement.
- `refs_median`: the references' median (back in natural units for
  logarithmic keys), 3 decimals.
- `robust_z`: C2, 2 decimals.
- `refs_p10_p90`: the 10th and 90th percentiles of the references' natural
  values, 3 decimals each.

### compare `typicality`

`{mean_cos_knn, z, percentile, k, verdict}`: C3; `mean_cos_knn` 3 decimals,
`z` 2 decimals, `percentile` 1 decimal, `k` as given, `verdict` the words of
C3.

### ab `query`

`{query, a, b, closer, dz}`: `a` and `b` are the score records of the
calibration part (A7) for each sound; `closer` from C5; `dz` = |z_a - z_b|,
2 decimals.

### ab `refs`

`{n_refs, a: {mean_cos_knn, z}, b: {mean_cos_knn, z}, closer, skipped}`.

### Text output

- compare: the summary, a blank line, `nearest:` with one line per nearest
  reference (cosine, name), then `features (value / refs median / robust
  z):` with one line per measurement.
- ab: the summary only.

## Behaviour

### compare

1. Check the range of PATH.
2. References: check that every given reference path yields media (else the
   path error, labelled `refs`); list their media files; remove PATH itself
   (compared as absolute, case-normalised paths); split into readable and
   skipped.
3. Fewer than 2 readable references: return the short result. The summary
   says at least 2 readable reference files are needed and how many were
   found, plus the skipped note.
4. Embed PATH with its range and every reference whole (A4, one call).
5. Cosine of PATH to every reference. `nearest` = the best k, each
   `{path, name, cosine (3 decimals)}`.
6. Typicality (C3).
7. Measurements: PATH with its range; every reference whole (C4).
8. Robust z of every key PATH has, except `duration`, for which at least 2
   references have a value (C2).
9. Outliers: keys with |robust z| >= 2 that are in the difference table
   (Constants), ordered by that table, at most 4. Each in words (C6 with the
   reference median as the "from" value).
10. `feature_distance` = median of |robust z| over all keys in `features`,
    2 decimals; null when there are none.
11. Summary facts: the name (with the trim when given) and the number of
    references; the 3 nearest with cosines (2 decimals); typicality z text,
    its percentile as an ordinal ("37th percentile of how close the
    references are to each other") and its words; the outliers in words,
    or that every measurement is within the references' spread; the skipped
    note (count, first 3 names, "..." when more).

### ab

1. Check both ranges. Embed A and B with their ranges (one call).
2. `cosine_ab` = A · B, 3 decimals.
3. With a query: score both (A7). `closer` and its words from C5 on z_a and
   z_b. Summary: "<A or B> is clearly / slightly closer" (or "neither is
   closer") to the query, with both z texts.
4. With references: references minus A and B, readable only. With at least
   2: typicality (C3) of A and of B against them; `closer` from C5 on the
   typicality z values. Summary: which is closer to the N references, both
   typicality z texts, and that z >= -1 fits the set; skipped note. With
   fewer than 2: the summary says so; no `refs` key.
5. Similarity words from `cosine_ab` (Constants). Summary: CLAP hears them
   as <words> (cosine, 2 decimals).
6. Measurements of A and B with their trims (C4). Differences (C6, at most
   4). `differences` = their keys. Summary: "B is <phrases>." or "Measured:
   no measurable difference big enough to hear (level, brightness,
   envelope, low end, noise)."
7. The summary starts with "A = <name and trim>, B = <name and trim>."
   A name with a trim prints as `name [m:ss.ss-m:ss.ss]`, or `[m:ss.ss-end]`
   when only the start is given.

## Algorithms

### C1. Sounding length

1. Mono downmix. 10 ms frames (480 samples), frame RMS (core-io).
2. Active frames: level (dB) >= the loudest frame's level - 40 dB.
3. length = (last active frame - first active frame + 1) × 0.01 s, at most
   the decoded duration. 3 decimals.

### C2. Robust z against references

For key k with sound value x and reference values r₁..rₙ (n >= 2):

1. Transform: g(v) = ln(max(v, 0.001)) for the logarithmic keys
   (`brightness_hz`, `bandwidth_hz`, `attack_ms`, `decay_ms`, `length_s`);
   g(v) = v for the others.
2. med = median of g(rᵢ). MAD = median of |g(rᵢ) - med|.
3. scale = max(1.4826 × MAD, minimum scale of k) (Constants; in transformed
   units, so 0.05 on a log key is about 5 %; default 0.1).
4. robust_z = (g(x) - med) / scale.
5. refs_median = exp(med) for logarithmic keys, else med.

### C3. Typicality

Inputs: the sound's embedding x, reference embeddings R (n >= 2), k.

1. S = R Rᵀ with the diagonal set to minus infinity (a reference is not its
   own neighbour).
2. kk = max(1, min(k, n - 1)).
3. For each reference i: sᵢ = mean of its kk highest similarities in S.
4. sx = mean of the kk highest cosines R x.
5. z = (sx - mean(s)) / (population std(s) + 1e-6).
6. percentile = 100 × share of references with sᵢ <= sx.
7. Words: z >= -1 "fits the set"; -2 <= z < -1 "somewhat unusual for the
   set"; z < -2 "an outlier: unlike the references".

So the sound is compared with the references the same way each reference
compares with the rest of the set.

### C4. Measurements of a path

Decode stereo with the trim. Run the core-io measurement in light mode. Build
the trim-proof dict (Interface).

### C5. Closer

Inputs: 2 scores sa, sb (z of a query, or typicality z).

- d = |sa - sb|. d < 0.3: `closer` = null, "neither is closer".
- Else the larger one wins (`"a"` or `"b"`). Words: "clearly closer" when
  d >= 1, else "slightly closer".

### C6. Differences in words

Inputs: 2 measurement dicts fa (A, or the reference median) and fb (B, or
the sound). Walk the difference table in order (the order is how much each
matters to the ear). For each key present in both:

- dB kind: keep when |fb - fa| >= the threshold.
- ratio kind: keep when both values are positive and max / min >= the
  threshold. For `attack_ms`, also require |fb - fa| >= 3 ms.

Keep the first 4 kept keys. For each, a phrase:

- `loudness_lufs`: "<|difference| as integer> dB louder" or "... quieter".
- Others: the direction word (up word when fb > fa, else down word), then in
  parentheses the label (when the key has one), fb's value, "vs", fa's value.
  Keys marked "with" get the prefix "with " ("with a sharper attack (...)").
- Values print as: frequencies "N Hz" below 1000, else "N.N kHz"; times "N
  ms" below 2000 ms, else "N.N s"; lengths "N.NN s"; width "mono" at -40 or
  below; other dB values as integers with LUFS / dBFS / dB.

Join: phrases without "with " first, then those with "with ", in table order
within each group; the last 2 joined by " and ", the rest by ", ".

In compare, the outlier phrases compare the sound's value against "median
<reference median>", and loudness reads "<N> dB louder / quieter than their
median".

## Constants

Difference table (order = importance to the ear):

| Key | Threshold | Kind | Up word | Down word | Label | "with" |
|---|---|---|---|---|---|---|
| `loudness_lufs` | 1.5 dB | dB | louder | quieter | loudness | no |
| `brightness_hz` | 1.15 × | ratio | brighter | darker | brightness | no |
| `attack_ms` | 1.5 × (and >= 3 ms) | ratio | a softer attack | a sharper attack | (none) | yes |
| `decay_ms` | 1.5 × | ratio | a longer tail | a shorter tail | (none) | yes |
| `low_end_db` | 3 dB | dB | more low end | less low end | energy below 250 Hz | yes |
| `length_s` | 1.25 × | ratio | longer | shorter | sounding length | no |
| `noise_floor_dbfs` | 6 dB | dB | noisier | cleaner | noise floor | no |
| `width_db` | 6 dB | dB | wider | narrower | side/mid | no |
| `bandwidth_hz` | 1.4 × | ratio | more top end | less top end | bandwidth | yes |

`peak_dbfs` has the words "hotter peaks" / "lower peaks" but is not in the
difference table (never reported as a difference).

Minimum robust scales:

| Key | Minimum scale |
|---|---|
| `loudness_lufs` | 1.0 dB |
| `peak_dbfs` | 1.0 dB |
| `noise_floor_dbfs` | 2.0 dB |
| `low_end_db` | 1.0 dB |
| `width_db` | 2.0 dB |
| `brightness_hz` | 0.05 (log) |
| `bandwidth_hz` | 0.05 (log) |
| `attack_ms` | 0.1 (log) |
| `decay_ms` | 0.1 (log) |
| `length_s` | 0.05 (log) |
| any other | 0.1 |

Other constants:

| Name | Value |
|---|---|
| default k | 5 |
| outlier line | |robust z| >= 2 |
| max outliers / differences in words | 4 |
| closer dead band | 0.3 z |
| "clearly" | difference >= 1 z |
| typicality: fits | z >= -1 |
| typicality: outlier | z < -2 |
| similarity: nearly the same sound | cosine >= 0.9 |
| similarity: similar | 0.75 <= cosine < 0.9 |
| similarity: related | 0.5 <= cosine < 0.75 |
| similarity: different sounds | cosine < 0.5 |
| loudness padding | 0.4 s |
| sounding-length range | 40 dB under the loudest 10 ms frame |
| noise floor shown | duration >= 1 s, floored at -120 dBFS |
| mono width | -40 dB |
| MAD factor | 1.4826 |

Ordinals: 1st, 2nd, 3rd, then "th"; 11th, 12th, 13th (any n with n mod 100
in 10..20 gets "th").

## Errors and edge cases

- A reference path that yields no media: the path error (labelled `refs`).
- PATH listed among its own references: it is removed before comparing.
- In `ab`, A and B are removed from the references.
- Fewer than 2 readable references: compare returns the short result (exit
  0); ab adds a sentence and no `refs` key.
- With 2 references, kk = 1.
- A key with fewer than 2 reference values gets no robust z.
- Zero or negative values on a ratio key: no difference is reported for it.
- A and B the same file without trims: cosine 1.0, "nearly the same sound",
  no differences.
- A whole-file A/B of multi-take files compares every take plus the silence
  between them. Use trims to judge one take.

## Acceptance tests

Fixtures: `click.wav` (as in listen-describe.md); `sine.wav` (2 s, 440 Hz,
-12 dBFS); `click_pad.wav` = the same click with 0.5 s of silence before and
1 s after.

- **CMP-1** Trim-proof. feats(click.wav) and feats(click_pad.wav): equal
  `loudness_lufs` (±0.2), `length_s` (±0.01), `attack_ms` and `decay_ms`
  (±10 %).
- **CMP-2** Sounding length. A 1 s file with a 200 ms tone burst at -6 dBFS
  -> `length_s` in [0.19, 0.22].
- **CMP-3** Noise floor rule. A 0.5 s file -> no `noise_floor_dbfs`; a 2 s
  file of digital silence plus a click -> `noise_floor_dbfs` == -120.
- **CMP-4** Robust z. Reference loudness values -20, -21, -19, -20, -22 and a
  sound at -10 -> robust_z ≈ (−10 − (−20)) / max(1.4826 × 1, 1.0) ≈ 6.74;
  the summary words say "10 dB louder than their median".
- **CMP-5** Log keys. Reference brightness 1000, 1100, 900, 1000 Hz and a
  sound at 2000 Hz -> robust z computed on ln values; `refs_median` == 1000.
- **CMP-6** Typicality flags an outlier. 10 reference embeddings clustered
  (pairwise cosine about 0.9) and one far vector (cosine about 0.1 to all) ->
  the cluster member has z >= -1, the far vector z < -2, verdict "an outlier".
- **CMP-7** Too few refs. A refs folder with one readable file and one
  corrupt file -> `n_refs` 1, `nearest` [], `skipped` has 1 item, exit 0.
- **CMP-8** **[model]** Self excluded. `compare(x.wav, refs=[folder])` where
  the folder holds x.wav and 3 others -> `n_refs` 3; x.wav is not in
  `nearest`.
- **AB-1** Differences in words. A = `click.wav`, B = the same click 6 dB
  louder -> `differences` starts with `loudness_lufs`; the summary says "6 dB
  louder".
- **AB-2** Brightness and attack. A = a click low-passed at 2 kHz, B = the
  same click unfiltered -> "brighter" is reported; a 2 ms attack difference
  is never reported as an attack difference.
- **AB-3** No difference. A and B = the same file with trims around the same
  click (different silence around it) -> `differences` == [] and the summary
  says no measurable difference.
- **AB-4** Closer dead band. z_a 2.1, z_b 2.3 -> `closer` null; z_a 1.0,
  z_b 2.2 -> `closer` "b", "clearly"; z_a 2.0, z_b 2.5 -> "b", "slightly".
- **AB-5** Similarity words. cosine 0.95, 0.8, 0.6, 0.2 -> nearly the same
  sound, similar, related, different sounds.
- **AB-6** **[model]** Same file, two trims. `ab(pack.wav, pack.wav,
  a_start=1.2, a_end=1.6, b_start=7.9, b_end=8.3)` -> `a_range` == [1.2, 1.6],
  `b_range` == [7.9, 8.3]; the measurements of A come from the A trim only.
- **AB-7** Ordering of phrases. Differences in brightness (no "with") and
  decay ("with") -> the joined text puts "brighter (...)" before "with a
  longer tail (...)", joined by " and ".
