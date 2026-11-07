# hark listen: vocabulary, embedding and calibration

Part of the listen spec. Index: `docs/specs/hark/listen.md`.

## Purpose

A raw CLAP cosine between a sound and a text is not comparable across
sounds or across texts:

- Dense, busy sounds score high against almost every text.
- Some phrases score high against almost every sound.

The calibration turns a cosine into a z that means the same thing for every
sound and every query: "how far this description stands out, for this very
sound, among a fixed set of 322 vocabulary descriptions, after removing the
description's own bias". `describe`, `match`, `rank`, `find` and `ab` all use
it.

## Interface

### Vocabulary file

A JSON object, UTF-8:

| Key | Type | Meaning |
|---|---|---|
| `_doc` | str | free text about the file (not read by code) |
| `templates` | list of str | prompt templates; each has exactly one `{}` where the phrase goes |
| `categories` | object | category name -> list of phrases |

Rules:

- A phrase appears in exactly one category.
- The term list is the concatenation of the category lists in file order.
  This order is the column order of every term array.
- Category names are part of the format. Code refers to these names:
  `quality` and `production` (excluded from "what it sounds like" lists),
  `music_genre`, `music_mood`, `instruments`, `voice` (unvalidated).

The source's file has 5 templates and 16 categories (counts in Constants).

### Reference statistics file

A JSON object, UTF-8:

| Key | Type | Meaning |
|---|---|---|
| `model` | str | CLAP model id the statistics belong to |
| `precision` | str | numeric precision used when they were built (`bf16` or `fp32`) |
| `pad` | str | padding mode used (`repeatpad`) |
| `templates` | list of str | templates used for the term embeddings |
| `logit_scale` | float | the CLAP model's softmax temperature (4 decimals) |
| `n_ref` | int | number of reference recordings |
| `n_folders` | int | number of distinct parent folders among them |
| `mean` | list of 512 float | folder-balanced mean audio embedding (5 decimals; not renormalised) |
| `term_cos` | object | term -> `[mean, std]` of its cosine over the references (4 decimals; for inspection only) |
| `max_win`, `reference_set` | | free metadata about how it was built |

Only `model`, `mean` and `logit_scale` are read at run time. If `model` is not
the CLAP model in use, the file is ignored (term bias 0). If the file is
missing, the term bias is 0 and the logit scale comes from the model.

The file is written with one key per line and one term per line inside
`term_cos` (so a diff shows which terms moved).

### Rebuild command

`python -m hark.vocab --build-stats REF ...` (the exact module path is the
architect's choice): reference files or folders. It embeds every reference
(at most 3 windows each), builds the statistics and writes the file. It prints
the number of recordings and folders. Without arguments it prints the term
count, the category count and the number of reference recordings in the
statistics (or "none").

## Behaviour

1. The vocabulary object is built once per process (thread-safe singleton).
   It reads the vocabulary file, embeds every term through every template,
   reads the statistics and computes the term bias of every term.
2. A text embedding of a phrase = the mean of the CLAP text embeddings of the
   phrase put into each template, renormalised to unit length. Queries and
   vocabulary terms use the same templates. So a query is comparable with
   the vocabulary terms.
3. An audio embedding of a file or a range = the mean of its CLAP window
   embeddings, renormalised.
4. Calibration gives, per sound and per text: raw cosine, bias-free score u,
   per-sound z, percentile and rank among the vocabulary terms.
5. A verdict band comes from z alone.

## Algorithms

### A1. Text embedding with templates

Input: a list of phrases P, the templates T (5).

1. For each phrase p and each template t, form the string t with p in place
   of `{}`.
2. Get the CLAP text embedding of each string (unit vectors, from core-io).
3. For each phrase, average its |T| embeddings and renormalise to unit
   length (divide by norm + 1e-12).
4. Return an (|P|, 512) float32 array.

Caching: a request for more than 16 phrases is cached as one file keyed by
(templates, whole list). A request for 16 or fewer phrases is cached per
phrase, keyed by (templates, phrase). So a repeated query needs no model
load. A cached array whose shape is wrong is recomputed.

### A2. Audio windows of a range

Input: `start`, `end` in seconds, `max_win` (default 6).
Window length W = 10 s, hop H = 5 s, both in samples at 48 kHz.

1. n = round((end - start) * 48000).
2. If n <= W samples: one span `[start, end)`.
3. Else: window starts at 0, H, 2H, ... while start + W <= n. If the last
   window does not end flush with n, add one more start at n - W.
4. If there are more than `max_win` starts, keep `max_win` of them, evenly
   spaced: the starts at index round(i) for i in
   linspace(0, count - 1, max_win).
5. Return spans `(start + s / 48000, start + (s + W) / 48000)`.

Examples: 25 s -> starts 0, 5, 10, 15 s (4 spans, the last ends at 25 s).
23 s -> starts 0, 5, 10, 13 s. 100 s -> 19 starts reduced to 6: indices 0,
4, 7, 11, 14, 18 -> 0, 20, 35, 55, 70, 90 s.

### A3. Span embedding and padding

Each span is one CLAP window of at most 10 s. A span shorter than 10 s is
padded to 10 s. Padding modes:

| Mode | Behaviour |
|---|---|
| `repeatpad` (default, shipped) | the CLAP feature extractor tiles the sound back to back to fill 10 s, then fills any remainder with silence |
| `zero` | silence after the sound up to 10 s |
| `spaced` | a span shorter than 1 s is first padded with silence to 1 s, then tiled (one hit per second); a span of 1 s or more is tiled as in `repeatpad` |

A span shorter than 10 ms (480 samples) is zero-padded to 480 samples first
so the CLAP front end has something to read.

Cache key per span: (file identity, start rounded to 4 decimals, end rounded
to 4 decimals, the padding actually applied). The padding actually applied is
`-` for a span of 10 s or more (within 1 ms), `repeatpad` for a `spaced`
span of 1 s or more, else the mode. Spans not in the cache are embedded in
batches of 8. The file is decoded mono once per batch run.

### A4. File embedding

`embed_files(paths, pad, max_win, ranges)`:

1. For each file j: s = max(0, start or 0); e = file duration if end is
   None, else min(duration, end).
2. Spans = A2 applied to `[s, max(e, s + 0.01))`.
3. Embed all spans (A3).
4. Embedding of file j = mean of its span embeddings, renormalised.

`embed_file(path, start, end)` = `embed_files` for one file.

### A5. Term bias

The term bias of a text q is its mean cosine over the reference recordings:

    b(q) = q · m

where m is the folder-balanced mean of the reference audio embeddings
(the statistics file's `mean`, not renormalised). Because cosine is linear in
the audio embedding, q · m equals the weighted mean of the cosines of q over
the references. So the bias works for any query, not only vocabulary terms.
Without statistics, b(q) = 0.

### A6. Calibration

Inputs: audio embeddings A (n, 512), vocabulary term embeddings E (T, 512),
optional query embeddings Q (k, 512), method (default `both`).

1. Term cosines: C = A Eᵀ (n, T).
2. Bias-free term scores: U = C - b(E) (bias subtracted per column) when the
   method is `center` or `both`; else U = C.
3. Per-sound statistics over the T terms of row i: μᵢ = mean(Uᵢ),
   σᵢ = population std(Uᵢ) + 1e-9, when the method is `sound` or `both`;
   else μ = 0, σ = 1.
4. Term z: Z = (U - μ) / σ (per row).
5. Query cosines: Cq = A Qᵀ (n, k).
6. Bias-free query scores: Uq = Cq - b(Q) (when centering), else Cq.
7. Query z: Zq = (Uq - μ) / σ, with the μ and σ of the row's **vocabulary
   terms** (the query does not enter μ or σ).
8. Percentile of query j for sound i: the share of the T terms t with
   U[i, t] < Uq[i, j].
9. Rank in vocabulary: 1 + the number of terms t with U[i, t] > Uq[i, j].

Methods (for evaluation): `raw` (cosine only, no centering, no z), `sound`
(per-sound z only), `center` (bias only), `both` (shipped).

Note: a query equal to a vocabulary term ties with that term. Ties count
neither in "beats" nor in "beaten by".

### A7. Scores of named queries

`scores(A, queries)` returns, per sound and per query, a record:

| Field | Value |
|---|---|
| `query` | the text |
| `cosine` | Cq, 3 decimals |
| `z` | Zq, 2 decimals |
| `percentile` | 100 × percentile, 1 decimal |
| `rank_in_vocab` | the rank of A6 step 9 |
| `vocab_size` | T |

### A8. Best terms of one sound

`top_terms(a, k, cats=None, exclude=())`: compute term z for the one sound,
sort the terms by z descending, keep terms whose category is in `cats` (when
given) and not in `exclude`, return the first k as
`(term, category, z rounded to 2, cosine rounded to 3)`.

### A9. Nearest category of a query

The category of the vocabulary term whose text embedding has the highest
cosine with the query's text embedding. `validated` = that category is not
one of `music_genre`, `music_mood`, `instruments`, `voice`.

### A10. Reference statistics

Inputs: reference audio embeddings R (n, 512) of paths p₁..pₙ.

1. Folder weight of file i: 1 / (number of reference files in the same
   parent folder). Folders are compared after making paths absolute and
   case-normalised. Normalise the weights to sum 1. So every source folder
   counts equally, whatever its size.
2. m = Σ wᵢ Rᵢ (float64).
3. Per term t: mean cosine μₜ = Σ wᵢ (Rᵢ · Eₜ), std σₜ = sqrt(Σ wᵢ (Rᵢ · Eₜ - μₜ)²).
4. Write the statistics file (format above), with `logit_scale` from the
   CLAP model.

The reference embeddings are file embeddings (A4) with `max_win` 3 for the
rebuild command. (The source's shipped file used `max_win` 6 for its tuning
files and 3 for the other references.)

### A11. Verdict

    verdict(z) = strong      if z >= 3.0
                 plausible   if 2.5 <= z < 3.0
                 weak        if 2.0 <= z < 2.5
                 no match    if z < 2.0

Bands are not cumulative. The evidence for each band is in Constants.

### A12. Forced choice

    softmax(x, s)ᵢ = exp(s (xᵢ - max x)) / Σⱼ exp(s (xⱼ - max x))

with s = the logit scale (from the statistics file when present, else from
the model; the source's value is 38.75). The inputs are raw cosines. This is
the CLAP model's own zero-shot classifier over the given texts: it is only as
meaningful as the set of alternatives.

### A13. z text

    zf(z) = format(floor(z * 10 + 1e-9) / 10, "+.1f")

## Constants

| Name | Value | Meaning |
|---|---|---|
| CLAP window | 10 s | one CLAP input |
| CLAP hop for long audio | 5 s | hop between windows |
| max windows per file | 6 | evenly spaced when more |
| max windows per reference (rebuild) | 3 | |
| default padding | `repeatpad` | |
| spaced padding unit | 1 s | |
| minimum CLAP input | 480 samples (10 ms) | zero-padded below |
| embedding size | 512 | |
| span batch | 8 | |
| text cache split | 16 phrases | above: one cache file per list |
| logit scale (source model) | 38.75 | stored with the statistics |
| per-sound std floor | 1e-9 | added to σ |
| normalisation floor | 1e-12 | added to norms |
| templates | 5 | see below |
| vocabulary terms | 322 | in 16 categories |

Templates (the source's prompt ensemble; generic English carrier phrases):
the bare phrase; "the sound of {}"; "a recording of {}"; "{} sound effect";
"an audio clip of {}".

Category list with term counts in the source's vocabulary (the order is the
file order):

| Category | Terms | Validated | Kind of phrase |
|---|---|---|---|
| `transitions` | 17 | yes | whooshes, swishes, risers, reverse swells |
| `impacts` | 18 | yes | booms, sub drops, hits, thuds |
| `ui` | 16 | yes | clicks, taps, notification sounds, interface blips |
| `keyboard` | 10 | yes | typing, single key presses of mechanical keyboards, switch types |
| `foley` | 39 | yes | small objects: plastic, wood, paper, glass, metal, cloth, pops |
| `water` | 11 | yes | splashes, drops, bubbles, pouring |
| `nature` | 13 | yes | footsteps on surfaces, leaves, grass, branches |
| `chimes` | 15 | yes | bells, glass tings, chimes, celesta and music-box notes |
| `ambience` | 26 | yes | rain, wind, room tone, city, crowd, forest beds |
| `everyday` | 35 | yes | vehicles, household devices, tools, animals, people sounds |
| `music_genre` | 28 | **no** | genre descriptions of music |
| `music_mood` | 16 | **no** | mood descriptions of music |
| `instruments` | 25 | **no** | instruments and drum parts |
| `voice` | 12 | **no** | speech, narration, laughter, whisper |
| `quality` | 22 | yes (excluded from "sounds like") | adjectives of timbre: "a deep sound", "a bright sound" form |
| `production` | 19 | yes (excluded from "sounds like") | recording and production character: real recording, close-miked, field recording, synthesized |

Rule for `quality` phrases: each has the form "a <word> sound" (article,
one or more descriptive words, then "sound"). `describe` turns a quality term
into its "character" word by dropping the first word and a trailing " sound".

Verdict evidence (held-out library of 125 real recordings, shipped
configuration; 248 true pairs, 7614 wrong pairs lenient, 8222 strict; base
rate 3.2 %). This table is part of the `match` JSON (`verdict_evidence`):

| Verdict | z | LR | precision_library | precision_even_odds | LR strict | precision_library strict | recall at or above |
|---|---|---|---|---|---|---|---|
| strong | >= 3 | 1289 | 0.98 | 1.00 | 51 | 0.61 | 0.51 |
| plausible | 2.5 - 3 | 21 | 0.41 | 0.95 | 9.1 | 0.22 | 0.63 |
| weak | 2 - 2.5 | 12 | 0.28 | 0.92 | 6.6 | 0.17 | 0.76 |
| no match | < 2 | 0.25 | 0.008 | 0.20 | 0.25 | 0.008 | (none) |

JSON keys per band: `z` (the text in column 2), `lr`, `precision_library`,
`precision_even_odds`, `lr_strict`, `precision_library_strict`,
`recall_at_or_above` (absent for `no match`).

Meaning of the columns:

- LR = P(z in band | true pair) / P(z in band | wrong pair).
- precision_library = share of pairs in the band that are true when about 3 %
  of candidate pairs are true (searching a library).
- precision_even_odds = LR / (LR + 1): when the caller already half expects
  a fit.
- lenient leaves out pairs of related classes (an Enter key judged as "a
  spacebar press"). strict counts them as wrong: it is the number to use
  when the exact variant matters.
- The strong LR rests on only 3 wrong pairs (95 % range about 440 to 6200; 170
  on the tuning library). So text output quotes precision, not the LR.

## Data assets

### Vocabulary file: DERIVE

- Origin: hand-written short caption-like phrases. History in the source:
  217 terms written before any evaluation; 67 added after seeing the tuning
  library's classes (so tuning numbers are optimistic); 38 general everyday
  terms (vehicles, household, tools, animals, people) added from general
  knowledge; then frozen before the held-out library was scored.
- Re-creation: write our own phrases, about the same count per category as
  the table above, in the style "a/an + concrete sound source + action",
  for example "a door slamming shut", 2 to 8 words, no proper names, no
  brand names. Keep the 16 category keys and the 5 templates.
- After any change of the vocabulary: no statistics rebuild is needed (the
  statistics hold the mean audio embedding, which does not depend on the
  vocabulary). But every z changes a little: re-run the held-out evaluation
  (listen-eval.md) to confirm the verdict bands.

### Reference statistics file: REGENERATE

- Origin: 241 unlabelled reference recordings from 13 folders: the 219 tuning
  recordings, 7 professional launch films, one film soundtrack and its stems,
  and some code-synthesized stems. They were disjoint from the held-out
  library.
- Re-creation: run the rebuild command on any diverse set of about 200 or
  more recordings in about 10 or more folders (one folder per kind of
  material: foley, ambience, music, UI, films ...). No labels are needed.
  Keep it disjoint from any evaluation set.
- Fallback without recordings: no file. The term bias is then 0. The
  measured cost on the tuning library: pooled AUC 0.966 -> 0.963.
- Rebuild after any change of the CLAP model, its precision or the reference
  set.

## Errors and edge cases

- Range checks (used by `describe`, `match`, `find`, `compare`, `ab`), in this
  order, after the duration probe (a missing or undecodable file raises the
  core-io audio error first):
  1. `start < 0` -> value error "start S s is negative".
  2. `start >= duration` -> value error naming the file and its duration.
  3. `end <= (start or 0)` -> value error "end must be after start".
  4. `end` past the duration is allowed: it is clamped to the duration.
- A batch helper splits a list of paths into readable paths and
  `{path, error}` rows. The error text is the first line of the decode error,
  at most 200 characters. One bad file never fails a batch.
- Empty phrase list -> a (0, 512) array, no model call.
- Missing statistics or statistics of another model -> bias 0, logit scale
  from the model.
- A range shorter than 10 ms still gets one span (A4 makes it at least
  10 ms long; A3 pads it to 480 samples).
- The near-silence helper gives the sample peak of the mono decode in dB;
  an empty decode counts as peak 0 (minus infinity dB, floored by core-io's
  dB helper).

## Acceptance tests

- **CAL-1** Vocabulary format. Input: the vocabulary file. Properties: every
  template has exactly one `{}`; no phrase appears twice; the category keys
  include the 16 names above; the 4 unvalidated names and `quality`,
  `production` exist.
- **CAL-2** Statistics match. Input: the statistics file. Properties: `model`
  equals the configured CLAP model; `mean` has 512 entries; `templates`
  equals the vocabulary templates; `term_cos` has an entry for every term or
  a documented subset after a vocabulary edit.
- **CAL-3** Windows. `spans(0, 8)` -> `[(0, 8)]`. `spans(0, 25)` -> 4 spans,
  starts 0, 5, 10, 15, the last ends at 25. `spans(0, 23)` -> starts 0, 5,
  10, 13. `spans(0, 100)` -> 6 spans with starts 0, 20, 35, 55, 70, 90.
  `spans(30, 55)` -> starts 30, 35, 40, 45.
- **CAL-4** Folder weights. Input: 3 files in folder X, 1 in folder Y.
  Properties: weights 1/3, 1/3, 1/3, 1; after normalisation the X files sum
  to the same as the Y file.
- **CAL-5** Calibration math. Input: a stub with T = 4 term embeddings, a
  mean m, one audio vector. Properties: z_t has mean 0 and population std 1
  over the 4 terms (to 1e-6); a query equal to term t gets the same z as term
  t; percentile and rank agree with a hand count (a query above all terms ->
  percentile 100, rank 1).
- **CAL-6** Method `raw` returns z equal to the cosine (μ = 0, σ = 1, no
  bias).
- **CAL-7** Category filter. `top_terms(a, k=3, cats=('keyboard',))` returns
  only keyboard terms; `exclude=('quality', 'production')` returns none of
  those.
- **CAL-8** Verdict bands. verdict(3.0) = strong, verdict(2.999) = plausible,
  verdict(2.5) = plausible, verdict(2.0) = weak, verdict(1.99) = no match,
  verdict(-5) = no match. The thresholds are strictly decreasing.
- **CAL-9** z text never contradicts a verdict. zf(2.96) = "+2.9";
  zf(3.0) = "+3.0"; zf(2.49) = "+2.4"; zf(-0.04) = "-0.1"; for 1000 random z
  in [-5, 8], verdict(float(zf(z))) equals verdict(z) (the thresholds lie on
  the 0.1 grid, so flooring never crosses one).
- **CAL-10** Softmax. Output sums to 1, all entries in (0, 1), the largest
  input gets the largest output, adding a constant to all inputs does not
  change the output.
- **CAL-11** Range errors. A 1 s file: start -1 -> value error; start 1.0 ->
  value error; start 0.5, end 0.5 -> value error; end 5 -> allowed (clamped).
  A missing path -> the core-io audio error.
- **CAL-12** **[model]** Text cache. Embedding the same query twice in two
  processes loads the CLAP model only in the first (the second reads the
  cache). Embedding 20 phrases writes 1 cache file; embedding 3 phrases
  writes 3.
