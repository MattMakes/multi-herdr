# hark listen: describe, match, rank

Part of the listen spec. Index: `docs/specs/hark/listen.md`. The calibration
(z, verdicts, embeddings) is in `docs/specs/hark/listen-calibration.md`.

## Interface

The CLI flags, Python signatures and JSON fields are in the index, section
`Interface`. This part adds the inner field formats.

### Measurements shown (`describe.measurements`)

The keys below, in this order, each only when its value is not null. Values
come from the core-io measurement function.

`duration`, `peak_dbfs`, `true_peak_dbtp`, `rms_dbfs`, `lufs`, `lra_lu`,
`attack_ms`, `decay_ms`, `brightness_hz`, `bandwidth_hz`, `noise_floor_dbfs`,
`flatness`, `harmonic_ratio`, `f0_hz`, `onset_count`, `onsets_per_s`, `width`,
`digital_silence_frac`.

- `width` is a string: `"mono"` when the side-to-mid ratio is unknown or
  below -40 dB, else `"side/mid N dB"` (N rounded to an integer).
- `noise_floor_dbfs` is dropped for sounds shorter than 1 s (it would measure
  padding, not a floor).

### describe inner records

- `top_tags[i]`: `{term, category, z, cosine, verdict}`.
- `music_hints[i]`: `{term, category, z, cosine}`.
- `tags[category]`: list of up to 3 `{term, z, cosine}`.
- `audioset[i]`: `{label, p}`, p rounded to 3 decimals by the AST wrapper.
- `distinct_windows[i]`: `{start, end, top_tag, audioset}`; `top_tag` is a
  `top_tags` record; `audioset` uses the window rule (top 3, p >= 0.1).

## Behaviour

### describe

1. Check the range (listen-calibration.md, Errors). Decode the stereo audio
   of the range. duration = samples / 48000.
2. Measure it with the core-io measurement function. Use the light mode
   when the duration is over 60 s.
3. If the measurement reports an error (input under 5 ms): return an
   untagged result. `measurements` = `{duration}`. The summary says how many
   ms of audio there are and that at least 5 ms are needed.
4. If the sample peak is below -70 dBFS: return an untagged result with the
   measurements. The summary says the duration, that it is near-silence
   with its peak, and that model tags of silence mean nothing.
   An untagged result has `shape` = the summary text, `top_tags` = [],
   `music_hints` = [], `tags` = {}, `audioset` = [], `character` = [].
5. Embed and tag:
   - Duration up to 20 s: audio embedding = file embedding of the range
     (listen-calibration.md A4). AudioSet probabilities = the AST wrapper's
     file probabilities of the range (per-class maximum over its windows).
   - Duration over 20 s: spans = the CLAP windows of the range (A2, at most
     6). W = the span embeddings (one unit row per span). Audio embedding =
     the renormalised mean of W. AudioSet probabilities per span (each span
     cut from the mono decode); the file's probabilities = the per-class
     maximum over spans.
6. Tags (A8 with all terms, categories `quality` and `production`
   excluded): `top_tags` = the first `top` terms of validated categories,
   each with its verdict. `music_hints` = the first `top` terms of the 4
   unvalidated categories. `tags` = for every category (all 16), its best 3
   terms.
7. AudioSet list: the top 5 labels by probability; keep a label when its
   probability is at least 0.05, and always keep the first.
8. Music probability = the AudioSet probability of the class `Music`.
9. Other readings: walk `top_tags` from position 4 on. Take a tag when its
   category is not among the categories of the first 3 tags and not taken
   yet. Stop at 2.
10. Character: from `tags["quality"]`, the terms with z >= 2.5, at most 2.
    Each becomes its character word (drop the first word, drop a trailing
    " sound": "a deep sound" -> "deep").
11. Shape words (Algorithm D1).
12. Long files only: the per-span top tag = the first validated tag of that
    span's embedding. Distinct windows (Algorithm D2). For each picked span:
    `{start, end, top_tag, audioset}` with start and end rounded to 3
    decimals; the AudioSet list of the span = top 3 labels with p >= 0.1,
    always the first.
13. Write the summary (D3).

### match

1. Normalise the query to a list. If the list is empty or any query is not a
   non-empty string after trimming: value error "no query given".
2. Check the range. Embed the range (A4).
3. Score every query (A7) for this sound. Each row also gets:
   - `verdict` from its z;
   - `validated` = its nearest category (A9) is a validated one;
   - with alternatives: `p_vs_alts` = the softmax (A12) of
     `[query cosine] + [alternative cosines]`, first entry, 3 decimals;
     `alts` = per alternative `{query, p, z, cosine}` (p = its softmax entry,
     3 decimals). The alternatives are scored with the same calibration.
   - without alternatives and with 2 or more queries: `p_vs_alts` = the
     query's entry of the softmax over all queries' cosines (the queries
     compete with each other).
   - one query, no alternatives: `p_vs_alts` = null.
4. Sort the rows by z descending. The best row gives the top-level fields.
5. `closest_vocab` = the best 4 vocabulary terms of the sound, `quality` and
   `production` excluded.
6. Peak = the stereo-decode sample peak of the range in dB.
7. Summary facts, in order:
   - the file name, with the range when given (`[a-b s]` or `[from a s]`);
   - per query: the query, its verdict, the unvalidated mark when not
     validated, z text, its rank among the vocabulary size, its cosine;
   - with alternatives: the forced-choice percentages of the best query and
     each alternative; without alternatives and with several queries: the
     forced-choice percentages among the queries. A percentage prints as
     `>99%` above 0.995, `<1%` below 0.005, else rounded to an integer;
   - "it sounds most like" the closest vocabulary terms that are not one of
     the queries, with z text;
   - when the best verdict is not `no match` and the best query is
     validated: the held-out evidence of that band: the right kind of sound
     `precision_library` of the time, the exact variant
     `precision_library_strict` (as integer percentages), with the note that a
     close variant counts as wrong in the strict number;
   - if the peak is below -70 dBFS, the summary starts with a warning that
     the input is near-silent and the scores mean nothing.

### rank

1. Check the paths: every given path must yield at least one media file;
   else the core-io path error naming each bad path.
2. List the media files (sorted, folders recursive). Split them into readable
   files and skipped `{path, error}` rows.
3. No readable file: return `summary` (no readable media files found, plus
   the skipped note), `query`, `n_files` 0, `results` [], `skipped`.
4. Embed every readable file (A4, up to 6 windows each). Calibrate against
   the query (A6). z rounded to 2 decimals.
5. Near-silent flag per file: mono-decode sample peak below -70 dBFS.
6. Order: non-silent files first, then by z descending. Apply `top`.
7. Per listed file: `path`, `name` (base name), `z`, `cosine` (3 decimals),
   `vocab_percentile` (1 decimal), `verdict` (`no match` when near-silent),
   `near_silent`, `sounds_most_like` = the file's best vocabulary term
   (`quality` and `production` excluded) as `"<term> (z <z text>)"`.
8. Counts of strong, plausible and weak among all non-silent files (not only
   the listed ones).
9. Summary facts, in order:
   - the number of files ranked and the query;
   - the best 3 non-silent listed files with z text and verdict (or "none,
     all near-silent");
   - the counts per band;
   - when the first listed file is not near-silent and its verdict is
     `weak` or `no match`: even the best is only a weak match / not a match,
     and what it sounds more like;
   - when more than 1 file is strong: the held-out precision of `strong`
     (lenient and strict), with an example of a close variant;
   - the number of near-silent files ranked last and that their scores mean
     nothing;
   - the skipped files: count and the first 3 names.

### Text output

- describe: the summary. When there are top tags: a blank line, a header that
  restates the bands (>= 3 clear, 2.5-3 plausible, 2-2.5 weak, < 2
  tentative), one line per top tag (z text right-aligned in 5 characters,
  term, category in brackets, cosine 3 decimals), a line of up to 5
  unvalidated hints, a line with the best term of each category, a line with
  the AudioSet labels. Always last: a line with every shown measurement as
  `key value`.
- match: the summary only.
- rank: the summary, a blank line, then one numbered line per result: rank,
  z text, verdict, cosine, the name, what it sounds most like, and
  `NEAR-SILENT` when flagged. When 2 results have the same base name, both
  show `<parent folder>/<name>`.

## Algorithms

### D1. Shape words

Inputs from the measurements: duration d, attack ms, decay ms, onset count,
onsets per second r, f0, spectral flatness, loudness range.

1. Long sound (d > 20 s): the first phrase is
   - "long recording" when the onset count is unknown;
   - "busy, R onsets/s" (1 decimal) when r >= 1.5;
   - "steady, few onsets" when the onset count <= d / 10;
   - else "N separate events".
   Add "loudness range N LU" (integer) when known. Join with commas. Stop.
2. Short sound (d < 2.5 s and the onset count unknown or <= 3): "short K",
   where K is
   - "transient" when attack <= 15 ms,
   - "swell" when attack >= 150 ms,
   - else "one-shot" (also when the attack is unknown).
3. Else "busy, R onsets/s" when r >= 1.5; else "N separate events" when the
   onset count >= 2; else "sustained".
4. Add a texture phrase: when f0 is known, "tonal, pitch ~F Hz (NOTE)";
   else "noise-like" when flatness >= 0.3.
   NOTE: MIDI number m = round(69 + 12 log2(f0 / 440)); name = the pitch
   class of m % 12 from C, C#, D, Eb, E, F, F#, G, Ab, A, Bb, B; octave =
   floor(m / 12) - 1. Example: 440 Hz -> A4; 261.6 Hz -> C4.
5. Add the envelope in parentheses when known: "attack N ms", "decays in
   T" where T is "N ms" below 2000 ms, else "N.N s".

### D2. Distinct windows of a long file

Inputs: spans, their unit embeddings W, the per-span top tag term.

1. With 1 span or fewer: none.
2. Mean direction u = mean(W) / (norm + 1e-12). Distance of span i:
   dᵢ = 1 - Wᵢ · u.
3. Walk the spans by distance descending. Pick span i when
   - it does not overlap any picked span (touching ends are allowed, with a
     tolerance of 1e-6 s), and
   - its top-tag term differs from the top-tag terms of all picked spans.
   Stop at 3 picks.
4. If fewer than 2 spans are picked, return none. Else return the picked
   spans in time order.

Summary sentence: with windows, "windows that read differently" with each
window's time span (`m:ss.ss-m:ss.ss`), its top term with z text and its top
AudioSet label with p. Without windows: "uniform: all N analysed 10 s
windows read as <top term of the first span>".

### D3. describe summary

The facts, in order:

1. "<duration with 2 decimals> s <shape words>."
2. When the music probability >= 0.5: that it is music, with the AudioSet
   Music probability, then unvalidated hints: up to 2 genre hints and up to 2
   mood hints with z text, and a warning that mood was wrong for about 1 in 5
   tracks in a test.
3. When the best validated tag has z >= 2.0: "sounds most like" the first 3
   top tags with z text, and the reading of the best tag in brackets:
   - z >= 3: "clear reading";
   - 2.5 <= z < 3: "plausible reading";
   - 2 <= z < 2.5: "weak reading".
   Then "other readings" (step 9 of describe) with category and z text.
4. Else: no sound-effect description stands out, the closest 2 tags with z
   text, tentative.
5. When the music probability < 0.5 and the best hint has z >= 3.0: one
   unvalidated hint with z text.
6. "Character:" with the character words, when any.
7. AudioSet text: the first 3 labels with p (2 decimals); prefixed
   "AudioSet:" when the top p >= 0.2, else "AudioSet tagger unsure (...)".
8. Measurement text: "peak N.N dBFS", "N.N LUFS" (when known), "brightness
   F", "bandwidth F" (F as "N Hz" below 1000 Hz, else "N.N kHz"), the noise
   floor only for sounds up to 20 s ("digital silence between sounds" when it
   is below -120 dBFS, else "noise floor N dBFS"), and the width string.
9. Long files: the distinct-window sentence of D2.

## Constants

| Name | Value | Used by |
|---|---|---|
| long file | > 20 s | describe: whole + windows; shape words |
| near-silence | peak < -70 dBFS | describe, match, rank |
| minimum input | 5 ms | describe (from the measurement function) |
| light measurement | duration > 60 s | describe |
| mono width | side/mid < -40 dB | describe |
| noise floor hidden | duration < 1 s | describe |
| music line | AudioSet Music >= 0.5 | describe |
| AudioSet keep | p >= 0.05, top 5, first always kept | describe |
| AudioSet sure | top p >= 0.2 | describe text |
| window AudioSet keep | p >= 0.1, top 3 | describe windows |
| distinct windows | at most 3, at least 2 | describe |
| other readings | at most 2 | describe |
| character | z >= 2.5, at most 2 | describe |
| "sounds most like" line | best z >= 2.0 | describe |
| unvalidated hint line | best hint z >= 3.0, no music | describe |
| transient attack | <= 15 ms | shape |
| swell attack | >= 150 ms | shape |
| short sound | < 2.5 s, <= 3 onsets | shape |
| busy | >= 1.5 onsets/s | shape |
| steady | onsets <= d / 10 | shape (long) |
| noise-like | flatness >= 0.3 | shape |
| closest vocabulary | 4 terms | match |
| default top | 8 | describe |
| forced-choice text | > 0.995 -> ">99%", < 0.005 -> "<1%" | match |

## Errors and edge cases

- describe of a range shorter than 5 ms: untagged result, no model is
  loaded.
- describe of near-silence: untagged result, no model is loaded.
- describe of a long file whose spans all read the same top term: no
  distinct windows; the "uniform" sentence is used.
- match with an empty query list or a blank query: value error (exit 1 on
  the CLI).
- match of a near-silent range: the scores are still computed and returned;
  only the warning is added.
- match where the query equals a vocabulary term: it ties with that term (rank
  and percentile count neither direction); `closest_vocab` text leaves that
  term out of "sounds most like".
- rank with a given path that yields no media file: an error that names
  the path, also when the other paths are valid (so a mistyped path is
  reported, not skipped).
- rank with undecodable files: they are skipped and listed; the rest are
  ranked.
- rank with every file near-silent: all are listed last with verdict
  `no match`, and the summary says "none (all near-silent)".
- Video files: every command accepts a video and uses its audio track
  (core-io decodes it).

## Acceptance tests

Fixtures: `click.wav` = a 1 s file with one short click (attack under 2 ms,
decay about 50 ms) at 0.2 s over silence; `sine.wav` = 2 s of a 440 Hz sine
at -12 dBFS; `silence.wav` = 1 s of zeros; `long.wav` = 45 s made of 15 s of
rain-like noise, 15 s of a sine melody, 15 s of clicks every 0.5 s.

- **DES-1** Too short. A 3 ms file. Properties: `top_tags` == [],
  `measurements` == `{duration: 0.003}`, the summary says at least 5 ms are
  needed, no model loads.
- **DES-2** Near-silence. `silence.wav` and a 1 s file at -80 dBFS peak.
  Properties: untagged; the summary names near-silence and the peak; no
  model loads.
- **DES-3** Shape of a click. `click.wav`. Property: `shape` starts with
  "short transient" and contains "attack" and "decays in".
- **DES-4** Shape of a tone. `sine.wav`. Property: `shape` contains "tonal,
  pitch ~440 Hz (A4)".
- **DES-5** Measurements hide what is not measurable. `click.wav` (1 s) and a
  0.5 s click. Properties: the 0.5 s result has no `noise_floor_dbfs`; a
  mono file gives `width` == "mono"; null values are absent.
- **DES-6** **[model]** Field structure. `click.wav`. Properties: every
  `top_tags` item has a verdict that equals verdict(z); no `top_tags` item
  has a category in the 4 unvalidated ones, `quality` or `production`;
  `tags` has 16 keys with at most 3 items each; `top_tags` is sorted by z
  descending; at most `top` items.
- **DES-7** **[model]** Long file windows. `long.wav`. Properties:
  `distinct_windows` has 0 or 2 to 3 items; they do not overlap; their
  `top_tag.term` values are all different; they are in time order; the
  summary mentions either "read differently" or "uniform".
- **DES-8** **[model]** Range. `describe(long.wav, start=30, end=40)`.
  Properties: `duration` == 10.0; no `distinct_windows` key (10 s is not
  long).
- **DES-9** Note names. f0 261.63 -> C4; 27.5 -> A0; 4186 -> C8; 466.16 -> Bb4.
- **DES-10** **[model]** Character words. A result whose quality terms "a
  deep sound" (z 3.1) and "a warm sound" (z 2.6) lead -> `character` ==
  ["deep", "warm"]; a third quality term at z 2.7 is not added (at most 2).
- **MAT-1** Empty query. `match(click.wav, [])` and `match(click.wav, "  ")`
  -> value error; CLI exit 1 with one stderr line `hark match: error: ...`.
- **MAT-2** **[model]** Single query. Properties: `p_vs_alts` is null;
  `results` has 1 item; the top-level fields equal that item's fields;
  `verdict_thresholds_z` == {strong: 3.0, plausible: 2.5, weak: 2.0}.
- **MAT-3** **[model]** Several queries. Three queries, no alts. Properties:
  `results` sorted by z descending; the 3 `p_vs_alts` values sum to 1 (±0.002).
- **MAT-4** **[model]** Alternatives. One query with 2 alts. Properties:
  `alts` has 2 items; `p_vs_alts` + the 2 `alts[i].p` sum to 1 (±0.002).
- **MAT-5** **[model]** Rank and percentile agree. Recompute the bias-free
  term scores u_t of the sound and the query's u_q. Properties:
  rank_in_vocab == 1 + count(u_t > u_q); percentile == 100 × count(u_t < u_q)
  / vocab_size (1 decimal); for a query that is not a vocabulary term,
  rank_in_vocab - 1 + percentile × vocab_size / 100 == vocab_size (±1).
- **MAT-6** **[model]** Near-silent warning. A 1 s file at -75 dBFS peak.
  Properties: the summary starts with the near-silent warning; scores are
  present; `peak_dbfs` ≈ -75.
- **MAT-7** **[model]** Unvalidated. Query "uplifting and hopeful music"
  -> `validated` false; no evidence sentence in the summary.
- **MAT-8** **[model]** Evidence sentence. A strong, validated best query ->
  the summary quotes 98 % and 61 %.
- **MAT-9** **[model]** Real match. A real recording of a single mechanical
  keyboard key press, query "a single mechanical keyboard key press" -> z >=
  2.5; query "heavy rain on a roof" on the same file -> z < 2.
- **RNK-1** Bad path. `rank(["missing_dir"], "x")` -> path error naming
  `missing_dir`.
- **RNK-2** Unreadable file. A folder with `click.wav` and `bad.wav` (random
  bytes). Properties: `skipped` has 1 item with path `bad.wav` and a
  non-empty error; `n_files` == 1.
- **RNK-3** Silence last. A folder with `click.wav`, `silence.wav`.
  Properties: the silent file is last, `near_silent` true, verdict
  `no match`; the summary mentions 1 near-silent file.
- **RNK-4** **[model]** Order. Results are sorted: all non-silent before
  silent, z descending within each group; `top=1` returns 1 result but the
  band counts still cover all files.
- **RNK-5** **[model]** Duplicate names in text. Two files `a/hit.wav` and
  `b/hit.wav`: the text output shows `a/hit.wav` and `b/hit.wav`.
- **RNK-6** **[model]** Cached re-rank. A second rank of the same folder with a
  new query embeds no audio (cache hit for every span) and is at least 5 times
  faster than the first.
