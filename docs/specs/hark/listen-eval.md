# hark listen: held-out evaluation of the semantic commands

Part of the listen spec. Index: `docs/specs/hark/listen.md`.

## Purpose

This part specifies how we measure the text-scoring commands (`describe`,
`match`, `rank`, `find`) and how we set or confirm their constants. It
covers:

- the labelled data sets and the rule that keeps tuning data and test data
  apart,
- the metrics and their exact formulas,
- the rule that picks the verdict thresholds (strong / plausible / weak),
- the query-kind check and the `find` scene checks,
- the real-effects check, the precision check and the timings,
- the target numbers that the source reached. A clean implementation that
  uses the same model, templates and calibration must come near them.

The realness evaluation is in `listen-realness.md` (R13). The verdict band
table of the held-out library is in `listen-calibration.md` (verdict
evidence). This part does not repeat those tables.

The evaluation is a developer tool. It is not a user command and it is not
shipped in the package. It reads labelled recordings that only the
developer has.

## Interface

One script with sub-commands. Name and location are our choice:
`eval/semantics/run_eval.py` in the hark repository. Every sub-command
writes one JSON result file next to the script and prints one line per
result row.

| Command | Flags | Writes | Does |
|---|---|---|---|
| `embed` | `--set tune\|heldout\|tuneref`, `--pad repeatpad\|zero\|spaced` (default repeatpad), `--part i/n` (default 1/1) | embedding cache only | embeds part i of n of a set, so a slow machine can embed in pieces |
| `stats` | none | the reference statistics file | rebuilds the shipped reference statistics (calibration A10) from the shipped reference set |
| `analyze` | `--templates t1\|t3\|t5` (default t5) | `results.json` | every variant on the tuning library, all files and per split; the threshold grid and the picked thresholds; the band table; the reference-set sensitivity; the 10 highest and 5 lowest term biases |
| `heldout` | none | `heldout.json` | the shipped configuration on the held-out library, plus the pre-registered vocabulary and raw cosine for comparison, plus the mood check |
| `kind` | none | `kind.json` | the query-kind reader on the labelled query texts |
| `find` | `--pad` (default repeatpad) | `find_<pad>.json` | the key-press, typing and scene checks of `find` |
| `timing` | none | `timing.json` | wall-clock time per call, models warm, nothing cached |

Two more scripts:

| Script | Writes | Does |
|---|---|---|
| `eval/semantics/real_effects.py [library.json]` | `real_effects.json` | the real-effects check, for the 3 pad modes |
| `eval/semantics/precision.py` | `precision.json` | bf16 against fp32 embedding agreement and speed |

Data locations come from environment variables, then from an untracked
`eval/local_paths.json`:

| Variable | Key in local_paths.json | Points to |
|---|---|---|
| `HARK_EVAL_ASSETS` | `assets` | the labelled library root (category folders, a `manifests/` folder, a held-out sub-folder) |
| `HARK_EVAL_REFS` | `refs` | the extra reference recordings (films, mixes, stems) for the term bias |
| `HARK_EVAL_SFX_LIBRARY` | `sfx_library` | the real-effects library JSON |

A missing location stops the command with one error line that names the
variable. The scripts never copy recordings into the repository. Result
files hold only aggregates, file ids and file base names.

Label files that live in the repository next to the script:

| File | Content |
|---|---|
| `queries.json` | the 8 category queries, the 35 classes (each: the manifest needs that map to it, and 2 phrasings), the 24 specific queries (each: a text and its relevant files, given as class ids, file ids or file-id prefixes) |
| `heldout_labels.json` | class ids per held-out file id, written before any score on those files; the class of the music folder |
| `kind_queries.json` | query texts in 2 parts (`tune`, `test`), each with an `event` list and a `texture` list |
| `vocab_preregistered.json` | the vocabulary as it was before any evaluation (categories of phrases, same format as the vocabulary file) |

## Behaviour

### Data sets

| Set | Files | Labels | Used for |
|---|---|---|---|
| tuning library | 219 recordings in 8 category folders | from the asset manifests: each file has needs, the needs map to classes | choosing pad, templates and calibration; setting the verdict thresholds; tuning the `find` rules |
| held-out library | 125 recordings in the same 8 folders under a separate sub-folder | class ids written from folder and file name only, before any score | nothing is tuned on it: the honest test of the shipped configuration |
| tuning references | the held-out library plus extra recordings (films, mixes, stems): 148 files | none (only their mean embedding is used) | the term bias in `analyze` |
| shipped references | the tuning library plus the extra recordings: 241 files | none | the shipped reference statistics file |

Rules:

1. The references of a run are always disjoint from the files that the run
   scores. `analyze` scores the tuning library, so it uses the held-out
   library as references. The shipped statistics use the tuning library, so
   they stay disjoint from the held-out library.
2. The tuning library has 2 splits, `dev` and `test`, by file id: take
   SHA-1 of the UTF-8 file id, read the hex digest as an integer; even is
   `dev`, odd is `test`. Every metric is also reported per split.
3. The held-out labels are written before the first held-out score. After
   that, nothing is tuned on the held-out library. A change of vocabulary,
   templates, padding, model or statistics requires a new held-out run, and
   the held-out numbers of the old run are no longer valid.
4. A file in 2 held-out folders is counted once (the first folder in the
   order `toy`, then the other categories). In the source, one file was
   in both `foley` and `toy`: 126 files became 125.
5. Every file in the `music` folder has the class `music`. A file without a
   class takes part only in the category metrics.

Category folders: ambience, foley, garden, keyboard, magic, motion, music,
toy.

Class ids (35): room_tone, rain, meadow, wind_grass, tiny_steps,
soft_landing, glass_squeak, stretch_pop, snap_lock, small_pop, grass_steps,
soil, tree, fruit_drop, tiny_crowd, key_thock, spacebar, typing, enter,
cursor_click, ting, chime_sweep, note, small_whoosh, big_whoosh, plunge,
underwater, swell, impact, music, toy_clicks, lego, dominoes, latch, switch.

Held-out library by folder: ambience 21, foley 6, garden 5, keyboard 20,
magic 6, motion 15, music 45, toy 7. 121 of the 125 files have a class.

### Confusable class groups

Some class pairs describe the same file in a fair way (an Enter key press
can also fit "a spacebar press"). The metrics use 2 label modes:

- **lenient**: a pair of (file, class) is left out when the class is in a
  confusable group with one of the file's own classes,
- **strict**: such a pair counts as wrong.

| Group | Classes |
|---|---|
| G1 | snap_lock, toy_clicks, latch, lego, switch, soft_landing, dominoes |
| G2 | tiny_steps, soft_landing, cursor_click, key_thock, spacebar, enter, typing |
| G3 | wind_grass, tree, grass_steps, meadow, tiny_crowd, fruit_drop |
| G4 | small_whoosh, big_whoosh |
| G5 | fruit_drop, soft_landing |
| G6 | note, ting, chime_sweep |
| G7 | swell, chime_sweep, impact |
| G8 | small_pop, stretch_pop |
| G9 | plunge, underwater |
| G10 | room_tone, rain |

A file's left-out set is the union of every group that holds one of its
classes. Its own classes are always positive.

### Queries

- 8 category queries, one per folder. Each is one short noun phrase about
  the folder's sound in general.
- 35 classes, each with 2 phrasings. Both phrasings count as separate
  queries in every class metric. So a class metric averages up to 70
  queries.
- 24 specific queries: concrete sounds a user would ask for (a glass ting,
  a whoosh, a deep key thock). Each names its relevant files.

The query texts are ours to write (DERIVE). Rule: a phrasing describes the
sound, not the job it plays in a film. Write both phrasings of a class
before any score.

### Variants of `analyze`

`analyze` runs the full grid on the tuning library:

- pad: repeatpad, zero, spaced (calibration A3),
- templates: t1 = the phrase alone; t3 = the phrase plus 2 templates; t5 =
  the 5 shipped templates (calibration A1),
- calibration method: `raw`, `sound`, `center`, `both` (calibration A6),
  plus 2 rejected methods that divide by a per-term standard deviation:
  `term_sd` (no per-sound z) and `both_sd` (with per-sound z).

The rejected methods: U = (C − μ_t) / σ_t, where μ_t and σ_t are the
folder-weighted mean and standard deviation of the term's cosine over the
reference recordings (σ_t gets +1e-9). For queries the same is computed
with the query's own μ and σ. `both_sd` then applies the per-sound z of
calibration A6.

The reference statistics of each (pad, templates) cell are built from the
tuning references of that pad. The tuning references use at most 3 windows
per file.

For the shipped cell (repeatpad, t5), `analyze` also runs a reference-set
sensitivity check, per split:

| Reference variant | How |
|---|---|
| folder-balanced (shipped) | the shipped weighting of calibration A10 |
| uniform | plain mean over files |
| without music-like refs | leave out video files, music folders, mixes and synth stems |
| transductive | the tuning library itself as references (an upper bound, not usable) |
| no term bias | bias 0 |

### Mood check

`heldout` also checks the music mood tags, which are not validated. All
music tracks in both libraries were chosen for one bright, playful film. So
a dark top mood is very likely wrong. For each music track: compute z over
the vocabulary; take the mood term with the highest z among the
`music_mood` category. Report the share of tracks whose top mood is one of
the 5 negative moods (dark and tense, sad and melancholic, mysterious,
aggressive, suspenseful build-up), and the chance share (5 / number of mood
terms).

## Algorithms

### E1. Calibrated query scores

For a file set with embeddings A and query texts Q (8 category + 70 class
+ 24 specific), compute z_q and u_q by calibration A6 with the method under
test. Columns are in this order: categories, class phrasing 1 for all
classes, class phrasing 2 for all classes, specific queries.

### E2. Average precision

For one query with scores s over N files and a relevance vector r:

1. Sort files by s, highest first.
2. For each relevant file at 1-based position k, precision@k = (relevant
   files in positions 1..k) / k.
3. AP = the mean of these values over the relevant files.
4. A query with no relevant file has no AP and is left out of the mean.

MAP = the mean AP over the queries that have one.

### E3. Precision at k

P@k = the share of relevant files among the top k by score. Reported for
category queries at k = 5 and k = 10, averaged over categories.

### E4. Category metrics

For each of the 8 category queries, relevant = the files in that folder.
Report MAP, mean P@5, mean P@10 and the AP per category.

### E5. Class metrics

For each class phrasing (2 per class), relevant = the files that have the
class. Leave out a phrasing whose class has no file in the set. Report:

- class MAP: mean AP over the phrasings,
- class top-3: the share of phrasings with at least one relevant file in
  the top 3,
- the number of phrasings used.

### E6. Specific queries

For each specific query, relevant = files whose classes overlap the query's
classes, or whose id is in its id list, or whose id starts with one of its
prefixes. Report top-1 (the best file is relevant), top-3, MAP, and the
base name of the best file. Only the tuning library has specific queries.

### E7. Tagging

Tagging asks: for a file, which of the 35 class descriptions fits best? It
uses u_q, not z_q. (The per-sound z is a monotone map per file, so it
cannot change the order inside one file.)

For each phrasing set k in {1, 2} and each file with at least one class:

1. Order the 35 phrasings of set k by u, highest first.
2. rank = 1 + the best (lowest) 0-based position of any of the file's
   classes.
3. Count accuracy (rank = 1) and reciprocal rank (1 / rank).

Report accuracy and MRR as means over all (file, set) pairs.

### E8. Pooled AUC

The pooled AUC asks: does one z threshold separate true (file, class)
pairs from wrong ones across all files and classes? This is the property
the verdict bands need.

1. Build a label matrix L (files × 35 classes): +1 when the class is one
   of the file's classes; 0 when the file has no class, or (lenient only)
   when the class is in the file's left-out set; −1 otherwise.
2. Use both phrasings: take the 70 class columns of z_q and label each
   with the L column of its class.
3. Keep the pairs with label ±1.
4. AUC by rank sum: rank all kept z values together (1-based, ties broken
   by sort order), R₊ = sum of ranks of the positives, n₊ and n₋ the
   counts. AUC = (R₊ − n₊ (n₊ + 1) / 2) / (n₊ n₋). No positives or no
   negatives gives no AUC.

Report AUC lenient and AUC strict.

### E9. Verdict threshold rule

Input: the pooled (z, label) pairs of the tuning library, lenient labels,
shipped configuration.

Band likelihood ratio for a band [lo, hi):

    LR = (tp / n₊) / (fp / n₋)

where tp, fp count the true and wrong pairs with lo ≤ z < hi. With fp = 0,
LR = +∞ when tp > 0, else 0. (n₊ and n₋ are at least 1.)

Rule, from the top down:

1. hi = +∞. For each verdict, in order (strong, 100), (plausible, 20),
   (weak, 5):
2. Walk the candidate t from 6.0 down to 0.0 in steps of 0.5.
3. While the band [t, hi) has LR ≥ the verdict's minimum, keep t as the
   current pick. At the first t that fails after a pick, stop. A t that
   fails before any pick does not stop the walk.
4. The verdict's threshold = the last pick (none when no t passes). The
   next verdict uses hi = this threshold (hi is unchanged when none).

Result in the source: strong 3.0, plausible 2.5, weak 2.0. These are the
shipped constants of calibration A11.

Note: for strong, the band [t, +∞) is cumulative. For the lower verdicts
the band is only the slice between the threshold and the one above.

### E10. Band table

For given cuts (3, 2.5, 2), report per band (strong ≥ 3, plausible
2.5–3, weak 2–2.5, no match < 2), not cumulative:

| Field | Value |
|---|---|
| tp, fp | true and wrong pairs in the band |
| lr | band LR as in E9, 2 decimals; none when fp = 0 |
| precision | tp / max(1, tp + fp), at the data's own base rate |
| precision_even_odds | LR / (LR + 1): the precision at prior odds 1:1 |
| recall_at_or_above | true pairs with z ≥ the band's low edge, divided by n₊ |

Also report n₊, n₋ and the base rate n₊ / (n₊ + n₋). Compute it for
lenient and strict labels.

The cumulative grid (for inspection, not for the rule): for t from 0 to 6
in steps of 0.25, the tp, fp, recall, false-positive rate, cumulative LR
and precision of z ≥ t.

To state precision at a library base rate b (the source used b ≈ 3 %):
precision = LR b / (LR b + (1 − b)).

### E11. Query-kind check

For each text in `kind_queries.json`, run the query-kind reader of `find`
(listen-find.md F2). Count, per part and per true kind, how many were read
as `event`, `either` and `texture`. List the events read as `texture` (the
costly error: no tight trims) and the textures read as `event`.

Label set rule (DERIVE):

- `tune` part: the event and texture phrasings of the tuning classes. The
  thresholds of F2 are set on this part only, by the rule in
  listen-find.md.
- `test` part: generic queries written for this check, not seen when the
  thresholds were set.

### E12. find checks

A hit for key-press recordings (a labelled slice list per recording):

1. Attack of a labelled slice = the time of the first sample in the slice
   whose magnitude reaches 10 % (−20 dB) of the slice's peak sample. This
   is measured apart from the trim rule of `find`.
2. For each returned segment, take the press whose attack is nearest to
   the segment start. lead = attack − start.
3. The segment is a hit when 0 ≤ lead ≤ 60 ms and the segment ends no
   later than the next press's attack + 5 ms.
4. A recording can have a range with no labels (a fast part). Segments
   that start there count as unlabelled, not as hits.

Report hits, distinct presses hit, the median and range of lead over hits,
unlabelled segments, segments inside a known skip range, the cold and the
cached run time, the duration and the candidate count.

Checks:

| Check | Input | Query | top |
|---|---|---|---|
| K1 | 18 s recording of 17 single key presses, one stray double hit at about 9.5 s marked as a skip range 9.3–10.45 s | a single mechanical keyboard key press | 17 |
| K2 | a typing recording: labelled slow presses 0–17.3 s, then fast typing with no labels | same | 5 |
| K2b | same recording | fast continuous typing on a keyboard | 5; count the segments that start at or after 17.3 s |
| K3 | a recording of 32 key presses on another board | same as K1 | 10 |

Scene checks use built test files (written as 32-bit float WAV at the
analysis rate):

- **Room tone bed**: a real room-tone recording, mono, tiled to the length,
  scaled to −55 dBFS RMS.
- **One-shot**: a real recording, mono. Find the first sample at 5 % of the
  peak magnitude; start 10 ms before it; keep at most 2.5 s; apply a 50 ms
  linear fade-out; scale to a peak of −6 dBFS.
- **S1, 44 s scene**: the bed plus 8 one-shots added at 2 + 5k s
  (k = 0..7): a glass ting, a fast whoosh, a cork pop, a lego brick click,
  an object dropping into water, a light switch click, a spacebar press, a
  small bell. Query each with its own text, top 3.
- **S2, 300 s scene**: the bed; a key press (from K1's slices, each scaled
  to −12 dBFS peak) starting at 1.0 s and then every 1.3, 1.4 or 1.5 s in
  turn up to 298 s, skipped where it would start within 0.7 s before or
  inside 2.6 s after a planted event; glass tings at 20, 150 and 275 s, a
  cork pop at 205 s and a fast whoosh at 240 s (one-shots as above). Query
  "a glass ting" (top 3), "a cork pop" (top 3) and "a fast whoosh" (top 3).

A scene event counts as found when a returned segment has
start − 50 ms ≤ event start < end. top-1 hit: the first segment finds one
of the query's events. Tight trim: a finding segment that is not a
`window`. Its lead = event start + 10 ms − segment start.

### E13. Real-effects check

Input: a library JSON of real licensed one-shot effects. Each sound has a
file, a duration, its published title, and the film jobs it was filed
under, each with the description used to judge that job.

1. Jobs = the jobs that have 2 sound-only phrasings. The phrasings say what
   the job's sound is, never the job, and are written before any score.
   Source: 26 jobs.
2. Ways of asking, per job: `job` = the filed description; `sound_only` =
   the 2 phrasings; `union` = the description and the 2 phrasings. A way's
   score for (sound, job) is the best z over its texts.
3. `title`: each sound's own title as the query. Report the verdict counts
   of the own-title z, the median, the AUC of own-title pairs against
   pairs with a different title (equal titles after trim and lower case
   count as own), top-1 and top-5 (fewer than 1 or 5 titles beat its own).
4. For each way: the own job of a sound = its filed job with the highest z.
   Report verdict counts and the median of own-job z; the AUC of own-job
   pairs against other-job pairs (pairwise, ties count half); the mean
   share of other jobs a sound passes (z ≥ 2); and how often the forced
   choice (calibration A12) between the own-job text's cosine and 4
   cheap-synth descriptions goes to a cheap synth.
5. Run all of it for repeatpad, spaced and zero padding.

This check measures whether the model hears real effects as what they are.
It is not a realness test.

### E14. Precision check

1. Take the first 10 s of 12 recordings (2 from each of 6 category
   folders: ambience, foley, keyboard, motion, music, toy), mono.
2. Embed them with CLAP in fp32 and in bf16 (repeatpad, batch 4, after one
   warm-up batch).
3. Report the mean and minimum cosine between the 2 embeddings of each
   clip, the seconds per 10 s window for each, and the speed-up.

Use bf16 only where the CPU supports it natively (core-io decides).

### E15. Timings

Models warm, every input a fresh copy in a new temporary folder (so no
cache hit), 2 threads:

| Row | Call |
|---|---|
| model_load_s | load the vocabulary, AST and CLAP |
| describe_short_cold | `describe` on a short key recording |
| match_cached_new_query | `match` on the same file with a new text |
| describe_31s_cold | `describe` on a 31 s rain recording |
| find_18s_cold | `find` on K1's recording |
| find_18s_cached_new_query | `find` on it again with a new text |
| find_300s_cold | `find` on S2 |
| rank_20_foley_files_cold | `rank` over a folder of 20 foley files; also seconds per file and total audio seconds |
| rank_20_cached_new_query | `rank` again with a new text |

## Constants

| Name | Value |
|---|---|
| split hash | SHA-1 of the UTF-8 file id; even → dev, odd → test |
| templates t1 / t3 / t5 | 1 / 3 / 5 templates; t5 is the shipped set |
| reference windows | at most 3 windows per extra reference file; tuning files use the normal maximum (6) |
| threshold grid | 6.0 down to 0.0, step 0.5 |
| LR minimums | strong 100 (cumulative), plausible 20 (band), weak 5 (band) |
| inspection grid | 0 to 6, step 0.25 |
| key-press hit | 0 ≤ lead ≤ 60 ms; end ≤ next attack + 5 ms |
| attack level | 10 % of the slice peak (−20 dB) |
| scene bed level | −55 dBFS RMS |
| one-shot | start 10 ms before 5 % of peak, ≤ 2.5 s, 50 ms fade, −6 dBFS peak |
| S2 key level | −12 dBFS peak; spacing 1.3, 1.4, 1.5 s in turn |
| scene match slack | 50 ms before the event start |
| mood negatives | 5 terms |
| library base rate for stated precision | ≈ 3 % |
| timing threads | 2 |

## Target numbers

The source's results with laion/larger_clap_general, bf16 on CPU, 2
threads. A clean implementation with our own vocabulary and query texts
will differ. Use the targets to judge a re-run, not as exact values.

### Tuning library, 219 files (optimistic: 67 terms were added after seeing these classes)

| Pad | Templates | Calibration | Cat MAP | Class MAP | Class top-3 | Tag acc | AUC | AUC strict |
|---|---|---|---|---|---|---|---|---|
| repeatpad | t1 | raw | 0.698 | 0.597 | 0.843 | 0.553 | 0.944 | 0.927 |
| repeatpad | t5 | raw | 0.728 | 0.627 | 0.886 | 0.594 | 0.951 | 0.936 |
| repeatpad | t5 | sound | 0.770 | 0.650 | 0.900 | 0.594 | 0.963 | 0.948 |
| repeatpad | t5 | center | 0.728 | 0.627 | 0.886 | 0.580 | 0.954 | 0.939 |
| **repeatpad** | **t5** | **both (shipped)** | **0.778** | 0.642 | 0.886 | 0.580 | **0.966** | **0.951** |
| repeatpad | t5 | both_sd | 0.768 | 0.629 | 0.900 | 0.578 | 0.964 | 0.949 |
| zero | t5 | both | 0.758 | 0.631 | 0.843 | 0.591 | 0.964 | 0.949 |
| spaced | t5 | both | 0.784 | 0.644 | 0.871 | 0.589 | 0.965 | 0.950 |

Shipped cell, more: cat P@5 0.85, cat P@10 0.85, tag MRR 0.712; dev / test
AUC 0.973 / 0.956, class MAP 0.678 / 0.639. Specific queries: top-3 23 of
24, top-1 19 of 24 (the miss: "a cork pop", where mouth pops ranked first).

Reading: the per-sound z gives most of the gain. The term bias adds a
little pooled AUC (the property the verdicts use) and costs a little class
MAP.

Reference sensitivity (dev / test AUC): folder-balanced 0.973 / 0.956,
uniform 0.972 / 0.957, without music-like refs 0.974 / 0.959, transductive
0.974 / 0.961, no term bias 0.971 / 0.953.

Tuning band LR (lenient / strict): strong 170 / 40, plausible 29 / 12,
weak 7.1 / 4.1, no match 0.18. An earlier weak band of z 1–2 had LR 0.98
(no evidence), which is why weak starts at 2.

### Held-out library, 125 files (nothing tuned on it)

| Vocabulary | Cat MAP | Class MAP | Class top-3 | Tag acc | AUC | AUC strict |
|---|---|---|---|---|---|---|
| shipped (322 terms) | 0.691 | 0.646 | 0.827 | 0.707 | 0.980 | 0.965 |
| pre-registered (217 terms) | 0.685 | 0.669 | 0.827 | 0.707 | 0.976 | 0.960 |
| raw cosine | 0.646 | 0.700 | 0.788 | 0.727 | 0.978 | 0.964 |

Shipped, more: cat P@5 0.70, P@10 0.625, tag MRR 0.807; 248 true pairs,
7614 wrong lenient pairs. Category AP: ambience 0.92, keyboard 0.98, magic
0.98, music 1.00, motion 0.82, garden 0.45, toy 0.20, foley 0.18.

Readings:

- The added terms help verdicts on new files (strong-band precision 0.926
  → 0.977 lenient, 0.543 → 0.606 strict) but not file ranking (class MAP
  0.669 → 0.646).
- Raw cosine ranks files better on the held-out library (class MAP 0.700)
  but worse on the tuning library (0.627). Calibration stays because the
  verdicts need absolute numbers.

Mood check: on 45 held-out music tracks, the top mood was negative for
18 % (chance 31 %; tuning library 0 of 8). A music-only term bias did not
help (20 %). Mood tags stay labelled as unvalidated hints.

### Query kind

| Part | True kind | Read event | Read either | Read texture |
|---|---|---|---|---|
| tune | event (31) | 31 | 0 | 0 |
| tune | texture (12) | 0 | 6 | 6 |
| test | event (30) | 25 | 4 | 1 |
| test | texture (20) | 0 | 6 | 14 |

The one test event read as texture was "a bird chirp".

### find

| Check | Result |
|---|---|
| K1 | 16 usable trims on 16 distinct presses; the 17th segment is the stray double hit in the skip range. Lead 5–54 ms, median 16 ms, never negative |
| K3 | 9 of 10 usable, lead 6–8 ms |
| K2 | 2 segments on labelled slow presses, 3 strokes from the fast part |
| K2b | 5 of 5 windows in the fast part |
| S1 | top-1 on the right event 8 of 8; 7 tight trims (lead 5–66 ms); the small bell came back as a 5 s window |
| S2 | 5 of 5 events found as tight trims |

### Real effects (195 sounds, 148 shorter than 1 s, 26 jobs)

| Asked with | Strong | Plausible | Weak | No match | AUC | Other jobs passed | Cheap synth won |
|---|---|---|---|---|---|---|---|
| own title | 124 | 27 | 17 | 27 | 0.919 | – | – |
| job description | 95 | 33 | 24 | 43 | 0.912 | 10.4 % | 14 |
| sound-only phrasings | 108 | 34 | 23 | 30 | 0.914 | 12.5 % | 22 |
| union | 119 | 36 | 21 | 19 | 0.922 | 14.9 % | 10 |
| own title, spaced pad | 135 | 24 | 16 | 20 | 0.921 | – | – |
| union, spaced pad | 125 | 31 | 20 | 19 | 0.920 | 15.4 % | 15 |

Readings:

- Asked by their own title, 168 of 195 sounds pass (z ≥ 2). Failures as
  filed come from job words that the model cannot hear (a pop that was
  described by what it does on screen).
- 2 or 3 sound-only phrasings, best kept, pass more sounds and separate
  jobs slightly better. The cost: a sound passes more of the other jobs,
  because jobs overlap.
- Spaced padding lifts title matches at the same AUC. Zero padding is worse
  on every row (title no match 38). Spaced padding is not shipped: the
  verdict bands were set with repeatpad, so a switch needs a new held-out
  run.

### Precision and timings

bf16 against fp32 on 12 clips: cosine mean 0.99975, minimum 0.99925;
1.26 s against 0.904 s per window, speed-up 1.39.

Timings (2 threads, bf16, a shared 8-core laptop CPU): model load 27 s;
describe short key 17.0 s; describe 31 s rain 52.5 s; match new query on a
cached file 2.0 s; find 18 s file 29.9 s cold, 1.2 s new query; find
300 s file 61.8 s cold; rank 20 foley files (804 s of audio) 55.8 s cold
(2.8 s per file), 3.4 s new query. A CLI `match` on a cached file and a
cached query loads no model: 1.5 s wall.

## Data assets

### Semantic evaluation sets: NEEDS-DATA

The source's tuning and held-out libraries are private recordings of one
film project, with manifests and hand labels. We cannot use them.

Fallback: a public labelled corpus mapped to the same structure (index Q7):

1. Pick 8 folder-level categories with file-level classes. Map the
   corpus's labels to about 35 classes. Keep classes that are hard to tell
   apart (keyboard keys, toy clicks, whooshes) in confusable groups as
   above.
2. Write 2 phrasings per class and 1 query per category before any score.
3. Split the files in two by a fixed rule before any score: one part for
   tuning, one held-out part. Write the held-out labels first.
4. Re-run `analyze` and E9 on the tuning part. Keep 3 / 2.5 / 2 when the
   rule gives them again. Otherwise, the new values replace them (index
   Q1).
5. Run `heldout` once. Report the band table (E10) and the metrics of E4,
   E5, E7, E8.

Until then, the shipped verdict bands and their evidence text are the
source's values.

### Query-kind label set: DERIVE

Write our own texts: a `tune` part (the event and texture phrasings of our
tuning classes) and a `test` part (about 30 generic one-shot texts and 20
generic continuous texts, written without looking at their scores). Set
the F2 thresholds on `tune` only.

### Key-press recordings and scene files

The `find` checks need 3 key recordings with labelled slices, 8 one-shots,
and a room-tone recording. Any recordings with the stated properties work.
Free recordings with labelled single presses are enough. The scenes are
built by E12, so they need no stored file.

### Real-effects library: NEEDS-DATA

A JSON of real one-shot effects with titles and filed jobs. Fallback: a
set of public CC0 one-shot effects with their published titles. Write the
2 sound-only phrasings per job before any score.

### Reference statistics

Rebuild with `stats` after any change of vocabulary, templates, padding or
model (calibration A10). The reference set must stay disjoint from the
held-out set.

## Errors and edge cases

- A query with no relevant file in the set has no AP. It is left out of
  MAP, top-3 and specific means, and the count of used queries says so.
- An AUC with no positive or no negative pair is reported as none.
- A band with fp = 0 has no reported LR. In the threshold rule it counts as
  +∞ when tp > 0, else 0.
- A label file that names a file id with no recording: the id is ignored.
  A held-out recording with no entry in the label file stops `heldout` with
  an error that names the id.
- A file with no class takes part in the category metrics only.
- `embed --part i/n` with i outside 1..n stops with an error.
- The scene builders overwrite their output file. They write only under the
  evaluation output folder, never into the repository.
- A missing recording in a find check stops that check with an error line.
  The other checks still run.

## Acceptance tests

Tests marked **[model]** need CLAP. The others run with fixed numbers.

- **EVS-1. Split.** The split of id "abc" is `dev` when int(sha1("abc"),
  16) is even, else `test`. The split of every id is the same on every run
  and platform.
- **EVS-2. Average precision.** Scores [0.9, 0.8, 0.7, 0.6], relevance
  [1, 0, 1, 0]: AP = (1/1 + 2/3) / 2 = 0.8333. No relevant file: no AP.
- **EVS-3. AUC by rank sum.** Positives [3, 2], negatives [1, 0]: AUC = 1.0.
  Positives [0], negatives [1]: AUC = 0.0. Positives [2, 0], negatives
  [1, 1.5]: AUC = 0.5.
- **EVS-4. Lenient and strict labels.** A file with classes {enter},
  classes [enter, spacebar, rain]. Lenient labels: +1, 0, −1. Strict
  labels: +1, −1, −1. A file with no class: 0, 0, 0 in both modes.
- **EVS-5. Tagging uses u, not z.** For one file, any per-sound z of its u
  row gives the same tagging rank as u. A file whose class is second by u
  has rank 2, accuracy 0, reciprocal rank 0.5.
- **EVS-6. Threshold rule.** n₊ = 100, n₋ = 10000. Pairs (true / wrong):
  z = 3.2: 50 / 33; z = 2.7: 20 / 80; z = 2.2: 15 / 250; z = 1.7: 10 /
  1000; z = 0: the rest. The rule gives strong 3.0 (LR 151.5; at 2.5 the
  cumulative LR is 61.9), plausible 2.5 (band LR 25; [2, 3) gives 10.6),
  weak 2.0 (band LR 6; [1.5, 2.5) gives 2.0). Variant: z = 2.7 holds 10 /
  100 instead (band LR 10). Then plausible has no threshold, hi stays 3.0,
  and weak = 2.0 (band [2, 3) LR 7.1; [1.5, 3) gives 2.6).
- **EVS-7. Band table.** With n₊ = 10, n₋ = 1000 and a band holding 5
  true and 2 wrong pairs: LR = (5/10) / (2/1000) = 250, precision 5/7 =
  0.714, precision at even odds 250/251 = 0.996. At base rate 3 %:
  0.03 × 250 / (0.03 × 250 + 0.97) = 0.885.
- **EVS-8. Key-press hit.** Attacks at [1.000, 2.000] s. A segment
  1.990–2.500 s is a hit (lead 10 ms, no next press). A segment
  0.930–1.500 s is not (lead 70 ms). A segment 0.995–2.004 s is a hit (it
  ends within 5 ms after the next attack). A segment 0.995–2.010 s is not.
- **EVS-9. Scene builder.** S1 is 44.0 s long; its bed RMS outside the
  one-shots is −55 ± 0.5 dBFS; each one-shot's peak is −6 dBFS ± 0.1 dB
  before it is added; one-shot k starts at 2 + 5k s. S2 holds no key press
  that starts within 0.7 s before a planted event or inside 2.6 s after
  one.
- **EVS-10. [model] Held-out targets.** On a held-out set built by the
  NEEDS-DATA fallback, the shipped configuration gives pooled AUC lenient
  ≥ 0.95, and the strong band (z ≥ 3) has lenient LR ≥ 100. If not, the
  verdict text must not claim the source's evidence numbers.
- **EVS-11. [model] Query kind.** On our `tune` label set, after F2's
  thresholds are set by its rule, every tune event reads as `event`. On
  our `test` label set, at most 10 % of events read as `texture` and no
  texture reads as `event`.
- **EVS-12. [model] find scenes.** On S1 built from our own one-shots,
  top-1 is the right event for at least 7 of 8 queries. On S2, all 5
  planted events are found in the top 3 of their queries, at least 4 as
  tight trims.
