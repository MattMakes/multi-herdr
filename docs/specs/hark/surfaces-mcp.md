# hark spec: surfaces, part M (MCP server and match)

Part of `docs/specs/hark/surfaces.md`. The interfaces are in the index: `hark mcp` in 1.2, `hark match` in
1.4. Section numbers here start with M.

## M1. Behaviour: MCP server

### M1.1 Lifecycle

1. The server process holds the BLAS thread count before numpy loads, the same way the CLI does (unit core-io:
   set `OPENBLAS_NUM_THREADS` from `--threads`, else `HARK_THREADS`, else min(4, cores/2), unless it is
   already set). `python -m <mcp module>` does this too.
2. The MCP SDK is imported only when the server starts. If it is missing, print a 1-line message to stderr
   that names the install extra for MCP, and exit 1.
3. Build the server: name `hark`, the instructions of M1.9, log level WARNING, the package version. Older SDKs
   do not take a version argument: then set the version on the low-level server object when it has one.
4. Register the 16 tools of index 1.2, in order. Each tool: name, description (the docstring with whitespace
   collapsed), annotations `{title: "hark <name>", readOnlyHint, destructiveHint: false, openWorldHint}`.
   Register with unstructured output (plain text content). An SDK that does not know that option gets the
   tool without it.
5. With `--warm`, start the warm-up thread (M1.7).
6. Print 1 line to stderr: the version, the number of tools, the transport (stdio) and the thread count.
7. Serve on stdio until the client closes the pipe. Exit 0.

The handshake normally takes a few seconds; a cold disk or a loaded machine makes it slower. Claude Code gives
up after `MCP_TIMEOUT` ms and then has no hark tools for the rest of the session. hark docs advise
`MCP_TIMEOUT=60000` on slow machines.

### M1.2 One call at a time

1 re-entrant lock guards every tool call and the warm-up. Calls run one after another. This avoids import races
(2 threads importing torch or scipy at the same time can see half-built modules) and keeps the CPU load low.
A model loads at the first call that uses it (20 to 60 s) and then stays in memory until the server exits.

### M1.3 Protecting the stdio transport

While a backing function runs, everything it prints to stdout goes to stderr. Stdout carries only JSON-RPC.
The warm-up thread must not redirect stdout: a redirect that races with the transport start sent the
initialize answer to stderr in the source, and the client hung.

### M1.4 Result text

Every backing function returns a dict (a non-dict result `r` becomes `{summary: str(r)}`). The tool text is
built as follows:

1. head = `summary`, trimmed. body = `text`, trimmed. When body starts with head, head is dropped.
2. When `full` is false and body is longer than 24000 characters, cut it to 24000 and add a line that says
   the text was cut at 24000 characters and that `full=true` gives all of it.
3. JSON: with `full`, the cleaned result (M2.1 step 1) as JSON with default separators and non-ASCII kept.
   Else the compact JSON of M2.1.
4. Ignored arguments note, when there are any: `(this hark version ignored: a, b)`.
5. Join with 1 blank line, skipping empty parts: head, body, note, then `JSON: <json>` (full) or
   `JSON (compact): <json>`.

The JSON part is always valid JSON.

### M1.5 Argument handling

- File argument: must be a non-empty string, else an error `<what>: a file path is required`. Trim spaces and
  surrounding double quotes, expand `~`, make it absolute. A missing path is an error that names the absolute
  path, says to pass an absolute path, and names the server's working folder. A folder is an error (a file is
  expected).
- Optional file (video, profile): checked the same way only when given.
- Many paths (`rank.paths`, `compare.refs`, `ab.refs`, `realness` on a folder or glob): a string becomes a
  1-item list. An empty list is an error. Each item is trimmed and made absolute. Each item must yield at least
  1 media file on its own (media listing of unit core-io: files by extension, folders searched recursively,
  globs that match files only). The items that yield nothing are named in the error (at most 5), with the
  working folder. The tool gets the media list of all items.
- Output paths (`sfx_render.out`, `kit_render.out`): a non-empty string, trimmed, `~` expanded, absolute. It
  need not exist.
- Cues: a string whose first non-space character is not one of `[ { 0-9 . -` is a file path and is checked as
  a file. Any other value (list, number text, JSON text) passes through. Empty means none.
- `detail` must be `low`, `normal` or `high`. `find.kind` must be `auto`, `event` or `texture`. Else an error.
- Video default for `timeline`, `sheet`, `critique`: the `video` argument when given; else the path itself
  when its extension is a video extension (`.mp4 .mov .m4v .webm .mkv .avi`); else none. `llm_critique` does
  not apply this default (its own context step does, see part C).
- Keyword filter: arguments whose value is null are dropped. When the backing function accepts any keyword,
  all are passed. Else only the names it accepts are passed, and the other names are reported in the ignored
  note. This keeps an older engine working with a newer server.

### M1.6 Missing modules and failures

- A backing function is looked up by its public API name (unit core-io keeps the name -> module table). When
  the import fails and the module does not exist in this install, the error says the function is not part of
  this install and lists the tools whose modules are present. When the module exists but fails to load, the
  error names the error type and message, says to retry if hark is being updated, and points to `hark doctor`.
- Any exception in a backing function becomes a tool error `<tool> failed: <type>: <message>`. The server
  keeps running.
- The tool error reaches the client as an MCP error result with the message text.

### M1.7 Warm-up (`--warm`)

A daemon thread takes the tool lock, then: loads the CLAP model and the AudioSet model (unit core-io), runs 1
measurement on 1 s of seeded noise (seed 0, 48000 samples, gain 0.1) to import and compile the measurement
code, and loads the vocabulary when that module exists (unit listen). It prints `models warm (<s> s)` or
`warm-up failed: <type>: <message>` to stderr. It never stops the server.

### M1.8 Per-tool behaviour

The implementer writes its own descriptions. Each description must carry the facts listed here.

| tool | behaviour and facts for the description |
|---|---|
| `measure` | Calls the core-io measurement with `music`, `start`, `end`, `light`, and adds the 1-sentence `summary` of M2.2. Description facts: model-free and quick; reports length, integrated LUFS, dBTP true peak, LRA, noise floor, clipped-sample count, exact-zero share, attack and decay times, brightness, bandwidth, band energies and onsets; `music` also estimates tempo, beat times and key; the values are meter readings and can be trusted; typical uses are loudness, peak, clipping and silence checks and level comparison before and after a mix edit |
| `describe` | Passes `path`, `start`, `end`, `top`. Description facts: returns calibrated tags grouped by category (for example source, material, action, character), the top AudioSet classes, main measurements and a short paragraph; the first tool for an unknown file or to confirm that a downloaded asset matches its title; needs CLAP and AST, so the first call is slow (20 to 60 s) |
| `match` | Passes `path`, `query`, `alts`, `start`, `end` to match (M3). Description facts: returns z with its verdict bands (M4.2), the percentile, `p_vs_alts` (a forced choice among the caller's alternatives, not calibrated), raw cosine (not a probability) and the nearest vocabulary terms; judge by z and verdict; pass believable alternatives |
| `rank` | Expands `paths`; passes `query`, `top`. Description facts: orders candidate files by calibrated fit to the text; next steps are `find` on the best file and `describe` plus `realness` to confirm; embeddings are cached, so a second query over the same files is quick |
| `find` | Validates `kind`; passes `query`, `top`, `kind`, `win`, `hop`, `start`, `end`. Description facts: returns ranked segments with `start`, `end`, `z`, `verdict`, `kind`, `head_db`, `tail_db`, `cut` (seconds); event segments begin 5 ms ahead of the attack and close after the decay; texture windows snap their edges to the sound when the query names a one-shot; `cut` is `head cut` or `tail cut` when a segment edge falls inside a sound, and such segments rank lower |
| `compare` | Expands `refs`; passes `k`, `start`, `end`. Description facts: lists the closest reference files, a typicality z (>= -1 belongs to the set, < -2 is an outlier) and the measured differences in plain words |
| `ab` | Checks `a` and `b` as files (the same file twice is allowed); expands `refs` when given; passes the 4 trim bounds. Description facts: says which of the 2 sounds fits a text or a reference set better and how they differ in measured terms (level, brightness, decay length); for files with several takes, pass trims, because a whole-file comparison includes every take and the gaps |
| `realness` | See M1.8.1 |
| `timeline` | Validates `detail`; resolves the video default and the cues. Description facts: a transcript of the whole soundtrack (events with time, level, tags and AudioSet classes), the loudness curve, picture cuts when there is video, and sync statistics against cuts and cues; a video file supplies its own picture unless `video` names another file |
| `sheet` | See M1.8.2 |
| `critique` | Resolves the video default; checks `profile` as an optional file; `fast` true passes `use_ast=false` and `use_realness=false`. Description facts: scores every soundtrack metric (for example loudness, true peak, dynamics, spectral balance, density of events, sync, realness) against the range of the professional launch films, each with a status (ok, low, high), a weight, the reason it matters and timed actions, plus 1 overall verdict out of launch-grade, close, needs work, not launch-grade; run before delivery and after each fix round |
| `doctor` | Calls the core-io doctor check with `load`. Description facts: reports packages, ffmpeg, both models (pinned revision, cache state, offline flag), CPU threads and whether the LLM provider is ready; run it after a tool failure or once before first use |
| `llm_critique` | Checks `path` and the optional `video`; passes `consent`, `prompt`, `video`, `with_timeline`, `model`, `start`, `end`, `synth_intended` to the critique of part C. Description facts: an optional verbal review by an audio language model covering sync, realism, balance, loudness, cheap or synthetic moments and timed fixes; the audio leaves the machine, so set `consent` to true only after the user said yes; without a ready provider or without consent nothing is sent and the result explains how to enable it; it is opinion, so confirm each fix with `timeline` or `ab` |
| `sfx_list` | Calls the bank list with `category`, `kind`. Builds `text`: the bank summary, then 1 line per sound: `<id>: <dur 3 decimals> s, peak <peak_s 3 decimals> s; <kinds comma-joined or -> ; dur <dur knob>[, apex][, pitch <native note>]`. No models; instant |
| `sfx_render` | Resolves `out`; passes `sound`, `out`, `seed`, `dur`, `apex`, `pitch`, `peak_db`. Description facts: writes 1 bank sound as a 24-bit WAV in about 1 s; the length equals `dur`; `apex` sets the time of the loudest point of a whoosh; `pitch` changes the recipe's own pitch parameter and never resamples; the result has the measured onset, peak and end and the model-free audit |
| `kit_render` | Checks `kit` as a file; resolves `out`; passes `force`. Description facts: renders the takes of every role and a lock with measured onset, peak, end and loudness per take; the cache key is the sound, seed, edits and bank version; a take that fails the audit gets a new seed and renders again |

#### M1.8.1 `realness`

1. Look up the backing function. When the realness model file of unit listen is missing, fail. The message
   makes clear that the real-versus-synthetic check did not run and that the agent must say so to the user, and
   it offers `describe` (synth-type AudioSet classes) and `measure` as weaker evidence.
2. Normalize the path (trim, strip quotes, expand `~`, absolute). When it is a file: call the single-file
   function with `path`, `start`, `end`, `ast` (false is dropped).
3. When it is not a folder and has none of `* ? [`: run the file check, which gives the usual not-found error.
4. Else expand it as many paths. When the many-file function is missing, fail: this version judges 1 file per
   call. Else call it with `paths`, `ast`, `start`, `end`.
5. Build `text`: the summary; a header line `p_real  verdict                  file: main reason`; then 1 row
   per file: p_real right-aligned in 6 characters with 2 decimals (or `-`), 2 spaces, the verdict
   left-aligned in 24 characters, 1 space, the base name, `: `, then the file's `why`, else its `summary`.
6. Format as M1.4.

Facts for the description: returns `p_real` (a probability), a verdict and the evidence in words; a file
longer than 6 s also gets verdicts per window; a folder or glob gives 1 row per file; the target is simple
procedural audio, so electronic music and professional sound design count as real; synthetic sound can be
the goal, so the agent reports the verdict and treats it as a fault only when the user wants real sound; the
result is strong evidence and not proof.

#### M1.8.2 `sheet`

1. Check `path`, validate `detail`, resolve the video default and the cues.
2. Output path: `out_png` (expanded, absolute) when given. Else
   `<HARK_HOME>/sheets/<file stem>_<first 8 characters of the file id>[<span>].png`, where span is empty when
   start and end are both null, else `_<start or 0, shortest form>-<end shortest form, or "end">`. The file id
   comes from unit core-io. Create the folder.
3. Call the sheet function with `path`, `out_png`, `video`, `cues`, `start`, `end`, `detail`.
4. PNG = the result's `png`, else the planned path. When the file does not exist, return the formatted text
   plus a line `(no PNG found at <path>)`.
5. Text = `PNG: <path>`, a blank line, the formatted text.
6. With `include_image`: also return the picture as JPEG: open the PNG, shrink so the long edge is at most
   1600 px (keep the aspect), convert to RGB, quality 88, optimized. The JPEG is about 1/3 of the PNG bytes.
   The SDK sends it as image content (format jpeg) after the text.

Facts for the description: draws on 1 shared time axis the video frames, the sections, labelled events, a
spectrogram, the waveform and loudness, beats, and cue markers whose colour shows sync (green inside 1 frame,
amber inside 100 ms, red no hit); cuts are dashed. `start` and `end` zoom in; `timeline` confirms details.

### M1.9 Server instructions

The server sends instructions text with the handshake. hark writes its own text with this content:

1. Purpose: the agent has no hearing, so it uses these tools whenever it must judge, choose or check audio
   (effects, music, ambience, mixes, the soundtrack of a video). Paths are absolute.
2. Typical chains:
   - asset choice: `rank` across candidates, `find` inside the winner, `describe` to confirm, and `realness`
     too when real or professionally produced sound is wanted;
   - mix or film review: `timeline`, `sheet`, `critique`;
   - checking an edit: `ab` between the old and new render, then `timeline` on the new render;
   - plain meters without models: `measure`;
   - `llm_critique`: optional, uploads audio, and needs the user's explicit yes.
3. Reading numbers: the z bands of M4.2, computed against a few hundred vocabulary phrases on the same sound;
   `p_real` and AudioSet scores are probabilities; `p_vs_alts` is a forced choice, uncalibrated and only as
   useful as the alternatives; cosine is raw CLAP similarity and only comparable for 1 sound and 1 query.
   Trust measurements most, then verdicts and z, and cosine least.
4. When `realness` cannot run, the agent reports that real versus synthetic is unchecked; the synth classes of
   `describe` are weaker evidence.
5. The first model call takes 20 to 60 s; later calls are quick.
6. Model-free film sounds: `sfx_list` for the bank, then `sfx_render` for 1 take aimed at a cue (dur, apex,
   pitch) or `kit_render` for a whole kit (takes plus a lock with measured onset, peak and end).

Rule from the source history: tool descriptions, instructions, skills and docs must not state 1 user's
preferences as instructions (for example "the user rejects synthetic sound"). They say what a tool measures
and leave the decision to the current user. A test enforces this.

## M2. Algorithms: MCP

### M2.1 Compact JSON (limit 7000 characters)

1. Clean: dict keys become strings; tuples become lists; numpy scalars and arrays become native values; NaN
   and infinity become null.
2. For n in 24, 12, 6, 3, 1: shrink the cleaned value with list cap n and serialize with no spaces
   (separators `,` and `:`, non-ASCII kept). When the length is <= 7000, return it.
   Shrink rules: a float is rounded to 3 decimals; a string longer than 400 characters is cut to 400 plus
   ` ...`; at the top level only, the keys `summary` and `text` are removed; a list keeps its first n items
   (shrunk) and, when it was longer, 1 extra string item `... +<k> more`; other values stay.
3. When still too long and the value is not a dict: return `{"cut": <message>}`, where the message says the
   result does not fit the compact view and that `full=true` returns it.
4. Else (using the n = 1 version): order the top-level keys by the length of their serialized values,
   longest first. Remove keys 1 at a time. After each removal, serialize the remaining keys plus
   `cut_fields` (the removed keys, in removal order) and `note` (fields were left out; call again with
   full=true). Stop at the first result <= 7000 characters. When none fits, return the last attempt.

### M2.2 `measure` summary sentence

Input: the measurement dict m (unit core-io keys).
1. When m has `error`: `<duration 3 decimals> s: <error>.`
2. When `peak_dbfs` < -70: `<d> s, <digitally silent | near-silent> (peak <p 0 decimals> dBFS)` plus a
   statement that loudness, spectrum and envelope values of silence mean nothing. Digitally silent when
   `digital_silence_frac` >= 0.999.
3. Else join with `, `: `<d> s`; `<lufs> LUFS integrated` plus ` (short-term max <v>)` when present, else
   `RMS <rms_dbfs> dBFS (too short for LUFS)`; `true peak <v> dBTP` plus `, <n> clipped samples` when n > 0;
   `LRA <v> LU` when present; `noise floor <v> dBFS`; `brightness <kHz 1 decimal> kHz` when non-zero;
   `bandwidth <kHz 1 decimal> kHz` when non-zero; `attack <ms> ms` plus `, decay <ms> ms` when attack is known;
   `<n> onsets` when present; `tempo ~<bpm> BPM, key <first key or ?>` when tempo is present. End with `.`.
4. When `digital_silence_frac` > 0.01, add a sentence: `<pct 1 decimal>% exact digital zeros`, usually edit,
   fade or gate padding, weak evidence alone; use `realness` for real versus synthesized.

## M3. Behaviour: match

### M3.1 Steps

1. Queries: a string becomes a 1-item list. Every query must be a non-empty string, else a value error
   (no query given).
2. Validate the file and range with the listen unit's check: start must not be negative, start must be before
   the end of the file, end must be after start (or after 0).
3. Get the vocabulary (unit listen) and the audio embedding of the file or segment (512 values).
4. Score every query and every alternative (M4.1). Sort the query rows by z, highest first. The best row is
   the first.
5. Closest vocabulary: the 4 vocabulary terms with the highest calibrated z for this sound, skipping the
   categories `quality` and `production`.
6. Peak: sample peak of the stereo decode (a mono file duplicated at unity gain), in dB.
7. Build the summary (M3.3) and the result (index 1.4).

### M3.2 Validated flag

For each query, find the vocabulary term whose text embedding is closest to the query's text embedding (dot
product). The query is unvalidated when that term's category is 1 of `music_genre`, `music_mood`,
`instruments`, `voice`. Those kinds of descriptions were never validated: their verdicts are hints.

### M3.3 Summary sentence

1. `<file base name>[<segment>]: ` then, per query row, joined by `; `:
   `'<query>' -> <verdict>[ (unvalidated: music / instrument / voice description)] (z <zf>; would rank
   #<rank> among <vocab size> vocabulary descriptions; cosine <2 decimals>)`, then `.`.
   Segment: ` [<start or 0, 3 decimals>-<end 3 decimals> s]` when end is given; ` [from <start> s]` when only
   a non-zero start is given; else empty.
2. With alternatives: a forced-choice sentence for the best query and each alternative with its percentage
   (M4.3). With several queries and no alternatives: a forced-choice sentence among the queries.
3. When any closest vocabulary term is not 1 of the queries: `It sounds most like: '<term>' (z <zf>), ...`.
4. When the best verdict is not `no match` and the best query is validated: an evidence sentence for its band:
   in a held-out library search with about 3% true pairs, the right kind of sound <precision_library>% of the
   time and the exact variant <precision_library_strict>% (for the exact variant, a near class counts as a miss,
   for example a return key heard for a space bar). The text quotes precision, not the likelihood ratio.
5. When the peak is below -70 dBFS: prefix a warning that the input is near-silent (peak in dBFS, 0
   decimals) and that the scores mean nothing.

### M3.4 CLI wrapper

Expected problems (missing or undecodable file, bad range, value errors, OS errors) print 1 line to stderr
(`hark match: error: <message>`, whitespace collapsed) and exit 1. With `--json`, a JSON object
`{summary: "error: <message>", error: <message>}` also goes to stdout. On success print the summary (or the
JSON with `--json`) and exit 0.

## M4. Algorithms: match

The calibration itself belongs to unit listen. This unit uses these outputs per sound a and query q:

### M4.1 Scores

- cosine c = a . e(q), where e(q) is the text embedding (each phrase through every prompt template, averaged,
  renormalized).
- bias-removed u = c - b(q), where b(q) = e(q) . m and m is the mean audio embedding of the reference set.
- For all T vocabulary terms t on the same sound: u_t = a . e(t) - b(t). mu = mean(u_t), sd = std(u_t) + 1e-9.
- z = (u - mu) / sd, rounded to 2 decimals in the row.
- percentile = 100 x (share of vocabulary terms with u_t < u), 1 decimal.
- rank_in_vocab = 1 + the number of vocabulary terms with u_t > u. vocab_size = T.

### M4.2 Verdict

Bands of z, not cumulative: z >= 3.0 `strong`; z >= 2.5 `plausible`; z >= 2.0 `weak`; else `no match`.

### M4.3 Forced choice

softmax(x, s)_i = exp(s (x_i - max x)) / sum_j exp(s (x_j - max x)), with s = CLAP's logit scale (stored with
the reference statistics, so no model load is needed; else read from the model).
- With alternatives: for each query, softmax over [its cosine] + [every alternative cosine]. `p_vs_alts` = the
  first value (3 decimals). `alts` = per alternative `{query, p (3 decimals), z, cosine}`.
- Without alternatives and with more than 1 query: softmax over the queries' cosines; each query gets its own
  value.
- Else `p_vs_alts` is null.
- Percent text: `>99%` when p > 0.995; `<1%` when p < 0.005; else the whole percent.

### M4.4 z text (`zf`)

Floor z to 0.1 (add 1e-9 before the floor), then write with a sign and 1 decimal. 2.96 becomes `+2.9`, so the
text never contradicts a verdict threshold.

## M5. Constants

| name | value |
|---|---|
| compact JSON limit | 7000 characters |
| text body limit | 24000 characters |
| list caps tried | 24, 12, 6, 3, 1 |
| float rounding in compact | 3 decimals |
| string cap in compact | 400 characters |
| inline image long edge | 1600 px, JPEG quality 88 |
| near-silence line (measure summary, match) | -70 dBFS |
| digital silence fraction for "digitally silent" | 0.999 |
| digital zeros sentence threshold | 0.01 |
| verdict bands | strong >= 3.0, plausible >= 2.5, weak >= 2.0 |
| match: closest vocabulary terms | 4, skipping `quality`, `production` |
| unvalidated categories | `music_genre`, `music_mood`, `instruments`, `voice` |
| first model call | 20 to 60 s |
| tool count | 16 |
| video extensions | `.mp4 .mov .m4v .webm .mkv .avi` |
| audio extensions (core-io) | `.wav .flac .ogg .oga .opus .mp3 .m4a .aac .aif .aiff .wma` |

Verdict evidence. Data: 125 real recordings that tuning never saw, with labels taken from the file names
before any scoring. Pair counts: 248 true; wrong 7614 (lenient) or 8222 (strict). Base rate 3.2%. Columns:
`lr` is the likelihood ratio, true-pair rate in the band over wrong-pair rate in the band;
`precision_library` is the true share in the band at a base rate near 3% (a library search);
`precision_even_odds` is the true share at a base rate of 50%; the `_strict` columns score a related class
(a return key for a space bar) as a miss.

| verdict | z | lr | precision_library | precision_even_odds | lr_strict | precision_library_strict | recall_at_or_above |
|---|---|---|---|---|---|---|---|
| strong | >= 3 | 1289 | 0.98 | 1.0 | 51 | 0.61 | 0.51 |
| plausible | 2.5 - 3 | 21 | 0.41 | 0.95 | 9.1 | 0.22 | 0.63 |
| weak | 2 - 2.5 | 12 | 0.28 | 0.92 | 6.6 | 0.17 | 0.76 |
| no match | < 2 | 0.25 | 0.008 | 0.2 | 0.25 | 0.008 | (none) |

The `z` column holds strings (`">= 3"`, `"2.5 - 3"`, `"2 - 2.5"`, `"< 2"`) in `verdict_evidence`. Only 3
wrong pairs fall in the strong band, so its ratio is uncertain: about 440 to 6200 at 95%, and 170 on the
tuning library.

## M6. Errors and edge cases

| case | result |
|---|---|
| MCP SDK missing | stderr message naming the MCP install extra; exit 1 |
| path empty or not a string | tool error: a file path is required |
| path missing | tool error naming the absolute path and the server's working folder |
| path is a folder where a file is expected | tool error |
| a many-paths item yields no media | tool error naming up to 5 such items |
| empty many-paths list | tool error |
| bad `detail` or `kind` | tool error naming the allowed values |
| backing module absent | tool error listing the tools that are present |
| backing module fails to import | tool error with the type, a retry hint and `hark doctor` |
| backing function raises | tool error `<tool> failed: <type>: <message>`; the server keeps running |
| backing function does not accept a newer argument | the call runs; the text names the ignored argument |
| `realness` model file missing | tool error: real vs synthesized NOT checked; use describe and measure |
| `realness` on a folder and no many-file function | tool error: pass a file |
| `sheet` PNG not written | text plus `(no PNG found at <path>)`, no image |
| result too large for compact | lists cut, then the largest fields left out and named in `cut_fields` |
| NaN or infinity in a result | null in JSON |
| something prints to stdout during a call | it goes to stderr |
| warm-up fails | 1 stderr line; the server keeps serving |
| match: empty query list or empty string | value error, exit 1 |
| match: negative start, start past the end, end not after start | value error naming the times |
| match: near-silent file | the summary starts with a near-silence warning |
| a NaN or infinity sample in the file | the decode refuses it with 1 line naming the file, the count and the first bad time (unit core-io); the tool returns that error |

## M7. Acceptance tests (MCP and match)

Tests run the server over a real stdio connection where marked "stdio". Backing functions may be faked for the
others.

| id | input | properties of the output |
|---|---|---|
| M-T1 | `hark mcp --tools --json` | 16 tools in the order of index 1.2; each row has `name`, `read_only`, `network`, `description`; only `llm_critique` has `network` true; `sheet`, `llm_critique`, `sfx_render`, `kit_render` have `read_only` false |
| M-T2 | stdio: initialize, then list tools | server name `hark`; 16 tools; each annotation title is `hark <name>`; destructiveHint false; instructions present |
| M-T3 | stdio: `measure` on a 1 s sine WAV | 1 text item; the first line is the measure sentence (starts with `1.000 s`); the text contains `JSON (compact): ` followed by valid JSON without `summary` or `text` keys |
| M-T4 | `measure` on a file of zeros | the sentence says digitally silent and that the values mean nothing |
| M-T5 | `measure` with `path` = a relative name that does not exist | tool error that holds the absolute path and the working folder; the server answers the next call |
| M-T6 | `rank` with `paths` = [a real folder, an empty folder] | tool error naming the empty folder |
| M-T7 | `find` with `kind` = `blob` | tool error naming `auto`, `event`, `texture` |
| M-T8 | a fake backing function that returns a dict with a 50-item list of long strings and a 30000-character `text` | compact JSON <= 7000 characters and valid; lists end with a `... +<k> more` item; body cut at 24000 with the cut note; `full=true` gives the whole text and the whole JSON |
| M-T9 | a fake backing function whose result has 1 huge top-level field | compact JSON has `cut_fields` naming that field and a `note` |
| M-T10 | a fake backing function without a `start` parameter, called with start = 1.0 | the call succeeds; the text holds `(this hark version ignored: start)` |
| M-T11 | `sheet` on a WAV with `include_image` true (fake sheet function that writes a 3000 x 1000 PNG) | 2 content items; the second is a JPEG image with long edge 1600; the text starts with `PNG: <path>`; the default path is under `<HARK_HOME>/sheets/` and ends with `_<8 chars>.png` |
| M-T12 | `sheet` with start 2, end null | the default file name ends with `_2-end.png` |
| M-T13 | `realness` on a folder of 2 WAVs (fake many-file function) | the text has the header line and 2 rows; a missing p_real shows `-` |
| M-T14 | `timeline` with `path` = `film.mp4` and no `video` | the backing function gets `video` = the same path |
| M-T15 | `cues` = `"1.2,3.4"` and `cues` = `"/abs/cues.json"` (missing) | the first passes through as text; the second gives a file-not-found tool error |
| M-T16 | backing function prints to stdout during a stdio call | the client still gets a valid JSON-RPC answer |
| M-T17 | match with queries ["a click", "a whoosh"] and no alts (fake vocabulary) | `results` sorted by z; both rows have a `p_vs_alts`, and the 2 values sum to 1 within 0.001 |
| M-T18 | match with 1 query and 2 alts | `p_vs_alts` + the 2 `alts[].p` sum to 1 within 0.001; the summary has a forced-choice sentence with percentages |
| M-T19 | `zf(2.96)`, `zf(-0.04)`, `zf(3.0)` | `+2.9`, `-0.1`, `+3.0` |
| M-T20 | verdict of z = 2.99, 2.5, 1.99 | `plausible`, `plausible`, `no match` |
| M-T21 | match on a file whose peak is below -70 dBFS | the summary starts with the near-silence warning |
| M-T22 | match CLI with `--start 5` on a 2 s file | exit 1; 1 stderr line with `hark match: error:`; with `--json`, stdout has `{summary, error}` |
| M-T23 | match with a query whose nearest vocabulary term is in `music_mood` | `validated` false; the summary marks it unvalidated; no evidence sentence |
