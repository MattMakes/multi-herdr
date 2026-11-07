# hark listen: behaviour spec (index)

Unit: **listen**. This unit is the part of hark that listens to audio and
answers questions about it in calibrated numbers and plain words:

- `describe`: what a sound sounds like (vocabulary tags, AudioSet labels,
  measurements, one summary paragraph).
- `match`: how well a sound fits one or more text descriptions.
- `rank`: which of many files best fits a description.
- `find`: the best moments inside one recording for a description, as
  sample-exact trims.
- `compare`: how a sound sits against a set of reference recordings.
- `ab`: which of two sounds (or two trims) is closer to a description or a
  reference set, and how B differs from A in measurable terms.
- `realness`: is a sound a real recording or professionally produced audio,
  or cheap audio synthesized in code.
- `profile`: a numeric sound profile of a set of reference films or
  soundtracks.
- The listening vocabulary and its calibration statistics, which every
  text-scoring command above shares.

This spec was written from the source by a contaminated spec writer. It holds
behaviour, the public interface, formulas, constants and tests. It holds no
source text. "The source" means the upstream project.

## Parts

The spec is split because it is longer than 60 KB. Read the parts in this
order:

| Part | File | Covers |
|---|---|---|
| index | `docs/specs/hark/listen.md` | this file: scope, shared conventions, the exact interface, dependencies, data assets summary, open questions |
| calibration | `docs/specs/hark/listen-calibration.md` | vocabulary file, text and audio embedding, the calibration math (cosine, term bias, per-sound z, percentile, rank), reference statistics, the verdict bands and their evidence |
| describe-match-rank | `docs/specs/hark/listen-describe.md` | `describe`, `match`, `rank` |
| find | `docs/specs/hark/listen-find.md` | `find`: query kind, candidate segments, edge snapping, scoring and selection |
| compare | `docs/specs/hark/listen-compare.md` | `compare`, `ab`, trim-proof measurements, typicality, differences in words |
| realness | `docs/specs/hark/listen-realness.md` | `realness`, signal measurements, the model file, its training and its held-out evaluation |
| profile | `docs/specs/hark/listen-profile.md` | `profile` and per-film metrics |
| eval | `docs/specs/hark/listen-eval.md` | the held-out evaluation of the semantic commands (calibration, verdicts, query kind, find) with target numbers, and how to rebuild the data assets |

Each part has its own `Behaviour`, `Algorithms`, `Constants`,
`Errors and edge cases` and `Acceptance tests` sections. This index holds the
`Interface`, `Data assets`, `Dependencies on other units` and
`Open questions` sections for the whole unit, plus a summary of the other
sections.

## Shared conventions

These hold for every command of this unit.

1. **Sample rate.** All audio is decoded by core-io at 48 000 Hz, float32.
   "Stereo decode" gives an (n, 2) array (a mono file is duplicated at unity
   gain). "Mono decode" gives an (n,) array (core-io defines the downmix).
2. **Time ranges.** `start` / `end` are seconds from the start of the file.
   A range is half-open `[start, end)`. Output times are always file times,
   never times relative to the range.
3. **Embeddings.** The semantic commands use one CLAP model (core-io loads
   it). Its audio and text embeddings are unit vectors of 512 dimensions in
   one shared space. Cosine = dot product of two unit vectors.
4. **Near-silence line.** A sample peak below -70 dBFS is "near-silent" for
   `describe`, `match`, `rank` and `find`. `realness` uses its own line: RMS at
   or below -60 dBFS is "silent".
5. **z text.** Every z printed in text is floored (not rounded) to 0.1 and
   carries its sign: `floor(z * 10 + 1e-9) / 10`, formatted `+2.9`. So 2.96
   prints as `+2.9` and never contradicts a verdict threshold of 3.0. JSON
   carries z rounded to 2 decimals.
6. **Caching.** Every embedding, every model probability vector and every
   realness result is cached on disk under the hark cache folder (core-io
   owns its location). A cache key includes the file identity from core-io
   (which changes when the file content changes), the range, the padding mode
   and the model identity. A cache entry that cannot be read is recomputed.
   Cache writes go to a temporary file and are then moved into place, so a
   concurrent reader never sees half a file. A failed cache write never fails
   the call.
7. **Determinism.** No command uses randomness at inference. The CLAP model's
   own random crop is never used: long audio is cut into fixed windows.
8. **Summaries.** Every result is a JSON object with a `summary` string: one
   paragraph that an agent can read without the rest. The text output of a
   command is the summary, sometimes followed by a table. The exact wording
   is free. Each part lists the facts the summary must state.
9. **Unvalidated categories.** Vocabulary categories `music_genre`,
   `music_mood`, `instruments` and `voice` were never validated against
   labelled data. Their scores are "hints": they never get a verdict in
   `describe`, and `match` marks a query that reads as one of them with
   `validated: false`.

## Interface

This section is exact. Command names, flag names, positional arguments,
defaults, JSON field names and Python function signatures are the contract.
The CLI program is `hark`.

### Flags shared by every command

core-io adds these to every command:

| Flag | Meaning |
|---|---|
| `--json` | print the full result object as JSON (ASCII-escaped, indent 1) instead of the text output |
| `--out PATH` | also write the JSON result to PATH (parent folders are created) |
| `--threads N` | CPU threads for the model runtime (also accepted before the command name) |

### Error contract

For `describe`, `match`, `rank`, `find`, `compare` and `ab`:

- An expected problem is one of: a missing file, a file that cannot be
  decoded, a bad range, an empty query, a path that yields no media file.
- On an expected problem the command writes one line to stderr:
  `hark <command>: error: <message>`. Whitespace in the message is collapsed
  to single spaces.
- With `--json`, the command also writes one JSON line to stdout:
  `{"summary": "error: <message>", "error": "<message>"}`.
- The exit code is 1.
- Any other exception goes to the generic handler of core-io.
- On success the exit code is 0.

`realness` and `profile` have their own error behaviour (see their parts).

### `hark describe PATH [--start S] [--end E] [--top N]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `PATH` | path | required | audio or video file |
| `--start` | float s | none | describe only from this time |
| `--end` | float s | none | describe only up to this time |
| `--top` | int | 8 | how many validated tags and how many hints to list |

Python: `describe(path, start=None, end=None, top=8) -> dict`

Result fields:

| Field | Type | Meaning |
|---|---|---|
| `path` | str | as given |
| `start`, `end` | float or null | as given |
| `duration` | float | seconds of audio analysed, 3 decimals |
| `measurements` | object | see listen-describe.md, "Measurements shown" |
| `shape` | str | a few words on envelope and texture |
| `top_tags` | list | up to `top` items `{term, category, z, cosine, verdict}`: best validated vocabulary terms |
| `music_hints` | list | up to `top` items `{term, category, z, cosine}`: best terms of unvalidated categories |
| `tags` | object | per vocabulary category, the best 3 terms `{term, z, cosine}` (all categories) |
| `audioset` | list | `{label, p}` AudioSet labels |
| `character` | list of str | up to 2 quality words |
| `distinct_windows` | list | long files only: `{start, end, top_tag, audioset}` |
| `summary` | str | the paragraph |

### `hark match PATH QUERY [QUERY ...] [--alts ALT ...] [--start S] [--end E]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `PATH` | path | required | audio or video file |
| `QUERY` | str, 1 or more | required | descriptions to judge |
| `--alts` | str, 1 or more | none | alternative descriptions for a forced choice |
| `--start`, `--end` | float s | none | judge only this part |

Python: `match(path, query, alts=None, start=None, end=None) -> dict`.
`query` is one string or a list of strings.

Result fields (the top-level score fields belong to the best query by z):

| Field | Type | Meaning |
|---|---|---|
| `summary` | str | the paragraph |
| `path`, `start`, `end` | | as given |
| `peak_dbfs` | float | stereo-decode sample peak of the judged range, 1 decimal |
| `query` | str | the best query |
| `cosine` | float | raw cosine, 3 decimals |
| `z` | float | calibrated z, 2 decimals |
| `percentile` | float | share of vocabulary terms the query beats, in %, 1 decimal |
| `rank_in_vocab` | int | where the query would rank among the vocabulary terms (1 = best) |
| `p_vs_alts` | float or null | forced-choice probability, 3 decimals |
| `verdict` | str | `strong`, `plausible`, `weak` or `no match` |
| `validated` | bool | false when the query reads as an unvalidated category |
| `alts` | list | only with `--alts`: `{query, p, z, cosine}` per alternative |
| `results` | list | every query, sorted by z descending: `{query, cosine, z, percentile, rank_in_vocab, vocab_size, verdict, validated, p_vs_alts}` plus `alts` when alternatives were given |
| `closest_vocab` | list | the 4 best vocabulary terms (categories `quality` and `production` excluded): `{term, category, z, cosine}` |
| `verdict_thresholds_z` | object | `{"strong": 3.0, "plausible": 2.5, "weak": 2.0}` |
| `verdict_evidence` | object | the evidence table of listen-calibration.md, keyed by verdict name |

### `hark rank QUERY PATHS ... [--top N]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `QUERY` | str | required | the description |
| `PATHS` | 1 or more | required | files, folders or globs |
| `--top` | int | all | list only the best N files |

Python: `rank(paths, query, top=None) -> dict`. Note the argument order:
the CLI takes the query first, the function takes the paths first.

Result fields: `summary`, `query`, `n_files` (readable files ranked),
`results` (list), `skipped` (list of `{path, error}`). Each result:
`{path, name, z, cosine, vocab_percentile, verdict, near_silent, sounds_most_like}`.

### `hark find PATH QUERY [--top N] [--win S] [--hop S] [--kind K] [--start S] [--end E]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `PATH` | path | required | the recording |
| `QUERY` | str | required | the description |
| `--top` | int | 5 | number of non-overlapping segments |
| `--win` | float s | derived | window length |
| `--hop` | float s | derived | hop between windows |
| `--kind` | `auto`, `event`, `texture` | `auto` | candidate kinds (see listen-find.md) |
| `--start`, `--end` | float s | none | search only this part |

Python: `find(path, query, win=None, hop=None, top=5, start=None, end=None, kind='auto') -> dict`

Result fields: `summary`, `path`, `query`, `query_kind` (`event`, `texture`
or `either`), `query_kind_score` (3 decimals), `duration` (searched span,
3 decimals), `n_candidates`, `n_windows`, `n_events`, `segments` (list).
Each segment:
`{start, end, duration, score, z, cosine, verdict, kind, peak_dbfs, head_db, tail_db, cut}`.
`kind` is `stroke`, `event` or `window`. `cut` is `""`, `"head cut"`,
`"tail cut"` or `"head cut, tail cut"`.

### `hark compare PATH --refs REF ... [-k K] [--start S] [--end E]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `PATH` | path | required | the sound to judge |
| `--refs` | 1 or more | required | reference files or folders |
| `-k` | int | 5 | how many nearest references to list |
| `--start`, `--end` | float s | none | judge only this part of PATH |

Python: `compare(path, refs, k=5, start=None, end=None) -> dict`

Result fields: `summary`, `path`, `range` (`[start, end]`), `n_refs`,
`nearest` (list of `{path, name, cosine}`), `typicality`
(`{mean_cos_knn, z, percentile, k, verdict}`), `features` (per measurement
key `{value, refs_median, robust_z, refs_p10_p90}`), `feature_distance`
(float or null), `skipped`. With fewer than 2 readable references only
`summary`, `path`, `n_refs`, `nearest` (empty) and `skipped` are present.

### `hark ab A B [--query Q] [--refs REF ...] [--a-start S] [--a-end E] [--b-start S] [--b-end E]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `A`, `B` | path | required | the 2 sounds; B may be the same file as A with other trims |
| `--query` | str | none | a description both should match |
| `--refs` | 1 or more | none | references both should resemble |
| `--a-start`, `--a-end`, `--b-start`, `--b-end` | float s | none | trims of A and B |

Python:
`ab(a, b, query=None, refs=None, k=5, a_start=None, a_end=None, b_start=None, b_end=None) -> dict`.
The CLI has no `-k` flag for `ab`; it uses k = 5.

Result fields: `summary`, `a`, `b`, `a_range`, `b_range`, `cosine_ab`,
`query` (only with a query: `{query, a, b, closer, dz}`), `refs` (only with
at least 2 readable references: `{n_refs, a: {mean_cos_knn, z}, b: {mean_cos_knn, z}, closer, skipped}`),
`features` (`{a: {...}, b: {...}}`), `differences` (list of measurement
keys). `closer` is `"a"`, `"b"` or null.

### `hark realness PATHS ... [--start S] [--end E] [--ast]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `PATHS` | 1 or more | required | files or folders (audio or video) |
| `--start`, `--end` | float s | none | judge only this part (applies to each file) |
| `--ast` | flag | off | also run the AudioSet tagger for evidence (slower) |

Python: `realness(path, start=None, end=None, ast=False, cache=True) -> dict`
and `realness_many(paths, ast=False, start=None, end=None) -> dict`.

Single-file result fields: `path`, `duration`, `range` (only with a range),
`p_real` (float, 3 decimals, or null), `verdict`, `why`, `evidence` (list of
str), `summary`, `note`, `gain_db` (only when a quiet-source gain was applied).
Long files add `segments` (list of `{start, end, p_real, verdict, why?}`),
`synth_windows`, `lean_synth_windows`, `p_real_min`.
`verdict` is one of `real or pro-produced`, `uncertain`,
`likely code-synthesized`, `silent`, `error`.

Many-file result fields: `files` (list of single-file results or error
rows), `counts` (count per verdict name, all 5 names), `summary`.

### `hark profile REFS ... [-o PATH | --out PATH] [--styles FILE] [--no-ast]`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `REFS` | 1 or more | required | reference videos, soundtracks or folders |
| `-o` | path | none | write the profile JSON here (same as `--out`) |
| `--styles` | path | none | JSON: a list of style descriptions in file order, or an object `{basename: description}` |
| `--no-ast` | flag | off | skip the AudioSet pass (no music and speech presence; faster) |

Python: `profile(paths, styles=None, use_ast=True, out=None) -> dict`,
`film_metrics(path, video=None, use_ast=True) -> dict`,
`load_profile(path=None) -> dict`.

The profile JSON format is in listen-profile.md.

### Library functions other units call

Other units call these. Their names are free, but each must exist with this
behaviour (see the named part):

| Function (suggested name) | Part | Used by |
|---|---|---|
| `vocab.get()`: the process-wide vocabulary object | calibration | forge-core critic, surfaces (warm-up) |
| `vocab.embed_files(paths, pad, max_win, ranges)` | calibration | forge-core critic |
| `Vocab.calibrate`, `Vocab.scores`, `Vocab.top_terms`, `Vocab.embed_text` | calibration | forge-core critic, timeline |
| `verdict(z)` and the verdict table | calibration | timeline, surfaces |
| `compare.feats(measurements, audio)`: trim-proof measurements | compare | forge-core critic |
| `compare.typicality(a, refs, k)` | compare | forge-core critic |
| `realness.load_model()`, `thresholds_of(model, file)`, `head_logit`, `fuse`, `pure_tone`, `signal_features`, `audible` | realness | forge-core critic, timeline (critique) |
| `profile.load_profile`, `film_metrics`, metric table | profile | timeline (critique) |
| the prompt templates of the vocabulary file | calibration | timeline |

## Behaviour (summary)

- `describe` embeds the sound once and scores all 322 vocabulary terms. It
  reports the best validated terms with verdicts, unvalidated hints, the best
  3 terms of every category, AudioSet labels, measurements and a summary. A
  file longer than 20 s is read as a whole plus up to 3 windows that read
  differently. See listen-describe.md.
- `match` scores each query against the sound with the same calibration, so
  the query competes with the 322 vocabulary terms for that sound. It gives
  a verdict band from the z and quotes the held-out evidence of that band.
- `rank` embeds every readable file and sorts them by the z of one query.
- `find` reads the query as one-shot, continuous or either, builds candidate
  segments (onset strokes, grouped events, sliding windows), scores each one
  with CLAP and returns the best non-overlapping ones as trims.
- `compare` places a sound among references by CLAP similarity (nearest
  references, typicality z) and by robust z of measurements.
- `ab` reports which of two sounds is closer to a query or a reference set
  and how B differs from A in words.
- `realness` gives `p_real` from a logistic model on the CLAP embedding, with
  signal measurements as evidence in words and one rule for pure tones.
- `profile` measures reference films the same way each time and stores the
  median, p10, p90, min and max of each metric.

## Algorithms, Constants, Errors and edge cases, Acceptance tests

These are in the parts. Acceptance tests are numbered per part with a prefix:

| Prefix | Part | Count |
|---|---|---|
| `CAL-` | listen-calibration.md | 12 |
| `DES-`, `MAT-`, `RNK-` | listen-describe.md | 10, 9, 6 |
| `FND-` | listen-find.md | 14 |
| `CMP-`, `AB-` | listen-compare.md | 8, 7 |
| `REA-` | listen-realness.md | 16 |
| `PRO-` | listen-profile.md | 11 |
| `EVS-` | listen-eval.md | 12 |

Tests that need the CLAP or AST model are marked **[model]**. A clean
implementer can run the others with a stub model that returns fixed unit
vectors.

## Data assets

Each asset is described in full in the named part.

| Asset | Part | Format | Origin in the source | Status | Fallback |
|---|---|---|---|---|---|
| vocabulary file | calibration | JSON: templates + categories of phrases | hand-written phrases (322 terms in 16 categories, 5 templates) | **DERIVE** | write our own phrases to the stated category list and counts |
| reference statistics file | calibration | JSON: model id, templates, logit scale, mean embedding (512 floats), per-term mean / std cosine | computed from 241 unlabelled reference recordings | **REGENERATE** | without the file, the term bias is 0 (a small loss, measured) |
| realness model file | realness | JSON: 2 logistic heads, fusion layer, thresholds, prompts, facts, silence base rates | trained on 1484 labelled clips + 714 augmented copies | **NEEDS-DATA** | zero-shot CLAP prompt score, or train on a public corpus with our own synth as negatives |
| launch profile file | profile | JSON: profile of 7 reference films | measured from 7 professional launch films | **NEEDS-DATA** (owned by the timeline unit) | the user builds one with `hark profile` from their own references |
| semantic evaluation sets | eval | labelled recordings + label JSON files | private recordings of one film project | **NEEDS-DATA** | a public labelled corpus mapped to the same class structure |
| query-kind label set | eval | JSON: event and texture query texts, tune and test parts | hand-written | **DERIVE** | write our own query texts by the stated rule |

## Dependencies on other units

| Unit | What this unit needs from it |
|---|---|
| core-io | decode (stereo and mono, with a range), duration probe, media probe (codec, has_video, lossy, duration), media listing of files / folders / globs with sorted order, path check that raises for paths with no media, file identity for cache keys, resampling, windowing helper, dB helper, the measurement function (`measure`: peak, true peak, RMS, LUFS, LRA, short-term max LUFS, attack, decay, brightness, bandwidth, noise floor, flatness, harmonic ratio, f0, onset count and rate, side-to-mid dB, correlation, band energies, digital silence fraction, error for input under 5 ms), frame RMS, loudness curve, integrated LUFS, onset detector, the CLAP wrapper (audio and text embedding, logit scale), the AST wrapper (527 class probabilities, top labels, probability of a label), cache folder, the shared CLI flags, the JSON / text emitter, the error line, the time formatter `m:ss.ss`, atomic file replace |
| timeline | picture-change detector for `profile` (frame differences and visual events); timeline also consumes this unit (critique uses `profile` and `realness`) |
| forge-core | consumes this unit (critic uses vocabulary, embeddings, realness heads, compare measurements and typicality); this unit needs nothing from it, except that the realness training fallback may use forge renders as negatives |
| recipes-impacts, recipes-motion | no direct dependency; their renders can serve as synthesized negatives for realness training and as test sounds |
| bank-kit-sfx | no direct dependency |
| fit | consumes `describe` (quality checks of fitted sounds) |
| surfaces | consumes every command of this unit (MCP tools and gallery) |
| skills-and-docs | documents the commands; the verdict bands and their evidence text come from this spec |

## Open questions

- **Q1. Vocabulary phrases.** The calibration is relative to the vocabulary:
  every z depends on all 322 phrases. Our own vocabulary (DERIVE) changes
  every z a little, so the verdict thresholds 3 / 2.5 / 2 and their evidence
  numbers are only valid after a re-run of the held-out evaluation. Do we
  accept new phrases with the source's thresholds as starting values until a
  labelled evaluation set exists?
- **Q2. Short anchor and prompt phrases.** `find` reads a query as one-shot
  or continuous by its text similarity to 2 sets of 6 short anchor phrases.
  `realness` names the closest of 12 short prompts. The kind thresholds
  (-0.08 and -0.27) are only valid for the exact anchor phrases. The spec
  gives the anchors as constants (they are generic noun phrases such as "a
  click"). If the orchestrator rules that these must also be rewritten, the
  thresholds must be recalibrated by the rule in listen-find.md.
- **Q3. Realness model.** The shipped model needs 1484 labelled clips from a
  private library. Which fallback do we ship first: the zero-shot CLAP
  prompt score (AUC 0.858 in the source's evaluation, no data needed) or a
  model trained on a public corpus with our forge recipes as negatives?
- **Q4. Launch profile.** The bundled launch profile needs 7 professional
  launch films. Do we ship no bundled profile and require the user to build
  one, or build one from films the operator owns?
- **Q5. `--top` meaning in describe.** The source's help text says "tags per
  category", but the flag limits the total number of validated tags and the
  total number of hints. The per-category table is always 3 per category.
  The spec keeps the behaviour. Do we also keep the help text?
- **Q6. AudioSet in describe.** `describe` always runs the AST tagger. Is AST
  optional in hark (an "ML extra" that prompts to install), and if so, does
  `describe` degrade to CLAP only when AST is absent? The spec assumes the
  install prompt of core-io, not a silent degradation.
- **Q7. Evaluation corpus.** Which public labelled corpus do we use for the
  held-out evaluation (EVS tests)? The class structure of listen-eval.md fits
  a corpus with folder-level categories and file-level classes.
