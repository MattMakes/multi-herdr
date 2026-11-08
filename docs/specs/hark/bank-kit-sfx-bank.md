# bank-kit-sfx, part 1: the sound bank

Part of the spec `docs/specs/hark/bank-kit-sfx.md`. Read the index first. It holds
the exact interface, the errors, the acceptance tests and the data-asset plan.
This part holds the behaviour, the algorithms and the constants of the bank: the
file format, how the bank loads, how one entry renders into a take, how a take
is measured, and how bank files are built and checked.

In this spec, "the source" is the upstream project. All names are hark names.

## 1. What the bank is

The bank is a set of JSON files. Each file holds named sound entries. An entry
is not audio. It is a complete, exact recipe call: which recipe renders it,
which render function of that recipe, the full parameter set at full float
precision, the noise seed and the level. Rendering an entry runs the recipe
again and gives the same samples every time on one machine.

Rendering needs only numpy and scipy (plus the hark forge recipes and the hark
fit renderer). It loads no ML model, opens no network and reads no file outside
the hark package and the files named by `HARK_BANK`.

The shipped bank holds 252 entries in 4 files (see part 4, the catalogue):

| file | name | version tag | entries | contents |
|---|---|---|---|---|
| `forge_v2.json` | forge_v2 | `forge_v2r6` | 183 | the delivered forge finals: 34 categories, 4 to 6 members each |
| `forge04.json` | forge04 | `forge04r1` | 51 | short, soft sounds for launch films cut to music: 7 categories that are new in this file, plus extra members for 6 forge-file categories |
| `fit.json` | fit | `fit1` | 15 | recipes fitted from CC0 recordings |
| `pluck.json` | pluck | `pluck1` | 3 | the tuned kalimba voices |

The hark implementer may keep these 4 file names and version tags. The version
tags are opaque strings. A regenerated file must carry a new tag (for example
`forge_v2h1`), because the cache keys of kits include the bank version.

## 2. Sound ids

- A forge entry id is `<category>/<member>`, for example `pro_impact/short`.
- A pluck entry id is `pluck/<name>`, for example `pluck/kalimba`.
- A fit entry id is `fit/<role>/<name>`, for example `fit/land.light/finger_phone`.
  It must match the regular expression `^fit/[a-z0-9_.]+/[a-z0-9_]+$`.
- Users see only the category part of an id. Which recipe renders a
  category is hidden from them. Several categories share one recipe (for
  example `click` and `pro_click`), and some categories use a second render
  function of a recipe (a "v2" pop render, a "typed run" render).
- Ids sort as plain strings. `ids()` returns them sorted.

## 3. Bank file format (format 1)

A bank file is one JSON object:

```json
{"bank": 1, "name": "forge_v2", "version": "forge_v2r6", "hark": "0.1.0", "note": "free text",
 "categories": {"click": {"recipe": "<recipe module>", "target": "<one-line target>", "kinds": ["click"], "use": "<one line>"}},
 "entries": {"click/soft": {"type": "forge", "...": "..."}}}
```

| key | type | meaning |
|---|---|---|
| `bank` | int | the format number. It must be 1. |
| `name` | str | the file's own name. |
| `version` | str | the version tag. When it is absent, the file stem is the tag. |
| `hark` | str | the hark version that wrote the file. Information only. (The source writes its own package name here. Hark writes `hark`.) |
| `note` | str | one line about the file. Information only. |
| `categories` | object | category name -> category info. Optional. |
| `entries` | object | sound id -> entry. Required. It must be an object. |

Category info:

| key | meaning |
|---|---|
| `recipe` | the recipe module name that renders the category (null for `fit`). |
| `target` | the one-line text description the forge searched toward. |
| `kinds` | the cue kinds and kit roles that the category serves (list of str). |
| `use` | one line: what a film uses the category for. |

### 3.1 Write layout

The writer puts one entry on each line, so a diff of a rebuilt file stays
small. The document stays one valid JSON object:

1. Line 1: `{` then the header keys (every key except `categories` and
   `entries`) as `"key": value` pairs joined by `, `, then a trailing `,`.
2. The line ` "categories": {`.
3. One line per category: 2 spaces, `"name": {json}`, a comma on all but the last.
4. The line ` },` then the line ` "entries": {`.
5. One line per entry: 2 spaces, `"id": {json}`, a comma on all but the last.
6. The lines ` }` and `}`, then a final newline.

The writer writes to a temporary file beside the target (`<path>.tmp<pid>`),
parses that file back as JSON, and only then renames it over the target. A
bank file is never replaced by a broken one. After a write, the loaded bank is
forgotten (see 4.3), so the next lookup reads the new file. The writer creates
the parent folder when it is absent. Files are UTF-8 with `\n` line ends.

### 3.2 Entry type `forge`

A forge entry is the render block of one forge final (the forge-core spec
defines the render block, "schema 2").

| key | type | meaning |
|---|---|---|
| `type` | str | `"forge"`. An entry without `type` is a forge entry. |
| `category` | str | the public category name. |
| `member` | str | the member name (the part after the `/`). |
| `recipe` | str | the recipe module that renders it. |
| `fn` | str | the render function of that module: the main render, a second-version render, or the typed-run render. |
| `params` | object | the complete parameter set at full float precision. |
| `seed` | int | the seed of the delivered final. |
| `peak_db` | float | the sample peak the raw render is normalised to before the gain: -1.0. |
| `gain_db` | float | the palette gain in dB that the forge applied after the peak normalisation (0.0 for most). |
| `gain` | float | optional: the same palette gain as a linear factor. When present it wins over `gain_db`. 12 shipped entries carry it. |
| `mode` | str or null | the critic mode the forge judged the pick in: `designed`, `real` or null. Information only. |
| `kinds` | list | optional: the kinds of this entry. When present they replace the category's kinds for this entry. |
| `notes` | str | optional: one line about the entry. |
| `meta` | object | the measurements of the entry (3.5). |
| `profile` | object | forge04 entries only (3.6). |
| `audit` | object | forge04 entries only (3.6). |
| `original` | object | forge04 entries only (3.6). |

The delivered final of a forge entry is:

    final = normalise(fn(params, rng(seed)), peak_db) * gain

where `normalise(x, p)` scales `x` so its largest absolute sample is
`10^(p/20)`, and `gain` is the linear gain (`gain`, else `10^(gain_db/20)`).
`rng(seed)` is the forge-core seeded generator. A bank take (section 5) does
not apply the palette gain. Only `render_final` does.

### 3.3 Entry type `fit`

A fit entry stores a model made by the fit unit from one recording.

| key | type | meaning |
|---|---|---|
| `type` | str | `"fit"`. |
| `category` | str | `"fit"`. |
| `member` | str | the id without the leading `fit/` (for example `land.light/finger_phone`). |
| `model` | object | the fit model, without its fit-engine report. The fit spec defines it. |
| `seed` | int | the seed of the reference take (0 for all 15 shipped). |
| `vary` | float | the take-to-take jitter amount passed to the fit renderer (0.3 for all 15 shipped). |
| `source` | object | `{id, title, author, license, url}` of the recording. All 5 are non-empty strings. |
| `fit` | object | `{kind, chosen, class}`: the model kind (`sines`, `modal`, `hybrid`, `spectral`, `gesture`), the variant the fit engine chose, and the character class (`struck`, `ringing`, `gesture` ...). |
| `kinds` | list | the roles it serves. Default: the role part of the id. |
| `notes` | str | one line about the entry. |
| `pitch_hz` | float or null | the measured note of a tonal fit (8.3), else null. |
| `meta` | object | the measurements (3.5). |

A fit entry renders through the fit renderer in "take" mode: the take rings
out until it is 60 dB down, past the point where the trimmed recording
ended, it has no DC offset, and a stripped background bed is never added
back (bed off). The fit spec defines take mode. `vary` is passed only when it
is not 0.

### 3.4 Pluck entries

A pluck entry is a forge entry (`type` forge, category `pluck`, recipe = the
pluck recipe, `fn` = its main render, `peak_db` -1, `gain_db` 0, plus
`notes`). See 7.2 for how it is made.

### 3.5 `meta`: the stored measurements

Each entry holds `meta`: measurements of its own-seed render,
normalised to -1 dBFS, at 48 kHz:

| key | unit | meaning |
|---|---|---|
| `dur_s` | s | the file length, 4 decimals. |
| `onset_s` | s | the onset (6.1). |
| `peak_s` | s | the peak time (6.1). |
| `end_s` | s | the end (6.1). |
| `attack_ms` | ms | the 10 % to 90 % rise of the 0.5 ms RMS envelope to its first near-peak (forge-core polish measure). |
| `centroid_hz` | Hz | the power-weighted mean frequency of one FFT of the whole mono mix, 1 decimal. |
| `lufs_m_max` | LUFS | the momentary-max loudness (6.3). |
| `channels` | int | 1 or 2. |
| `fp` | object | `{hop_s: 0.05, env_db: [...]}`: the fingerprint (6.4). |
| `pitch_hz` | Hz or null | only on forge entries of the categories `click`, `pro_click` and `toggle`: the partial a listener hears (8.2), or null when there is no clear one. |

The fingerprint lets a later re-render on another machine (other numpy or
scipy versions) be checked with tolerances instead of a hash (section 9).

### 3.6 forge04 extras

Each forge04 entry also carries:

| key | meaning |
|---|---|
| `kinds` | its own kinds. The audit of its takes uses the first kind as the kit role. |
| `notes` | one line about the member. |
| `profile` | the model-free tonal profile of its render (the forge-core tonal measure): `dur_s`, `attack_ms`, `t20_ms`, `t40_ms`, `centroid_hz`, `lo150` (energy share under 150 Hz), `hi4k` (energy share over 4 kHz), `tonal` (energy share within +-1/4 octave of the strongest bin), `glide_st`, `corr` (L/R correlation). Each value is rounded to 3 decimals or null. |
| `audit` | `{role, seeds: [first, last], clean, warnings}`: the audit of its takes (7.3). |
| `original` | `{made: "recipe", max_ref_clap}`: the originality record (7.3). |

## 4. Loading

### 4.1 Which files

The bank is the union of:

1. Every `*.json` file in the package bank folder, in this order: `forge_v2.json`
   first, then the other files whose names start with `forge`, then the rest,
   each group sorted by file name. The shipped order is forge_v2, forge04,
   fit, pluck.
2. Then each path in the environment variable `HARK_BANK`. The variable holds
   paths joined by the OS path separator (`:` on macOS and Linux, `;` on
   Windows). Empty items are skipped. Each path is made absolute.

### 4.2 How files merge

- Files are read in the order above.
- An entry id seen in a later file replaces the earlier entry. (So a user file
  in `HARK_BANK` can override a shipped sound.)
- A category seen in a later file does not replace the earlier category info.
  The first file that names a category wins.
- `VERSION` is the version tags of all loaded files joined by `+`, in load
  order. Shipped: `forge_v2r6+forge04r1+fit1+pluck1`.

### 4.3 Caching

The loaded bank is cached in the process. The cache key is the ordered tuple
of file paths. When `HARK_BANK` changes, the key changes and the bank loads
again. `reload()` forgets the cache. Every bank write calls it.

### 4.4 Reading one file

- A missing file is a `BankError` that names the path and says which
  separator joins the paths in `HARK_BANK`.
- A file that cannot be read or parsed is a `BankError` with the path, the
  error type and the error text.
- A file that parses but is not an object with `bank` equal to 1 and an
  `entries` object is a `BankError` that names the path.

### 4.5 Lookups

- `ids()`: every id, sorted.
- `get(id)`: a deep copy of the entry, plus the key `id`. For an unknown id it
  raises `BankError`. The message holds the unknown id, then a hint, then the
  count of all ids and the command that lists them (`hark sfx list`). The hint
  is:
  - up to 4 close ids (string similarity ratio 0.5 or more, best first), as
    "Did you mean a, b?"; else
  - up to 8 ids of the same category (the text before the first `/`), as
    "<category> has: a, b."; else nothing.
- `category_info(cat)`: the category info from the files, with `kinds` and
  `use` filled from the built-in category table (part 4, section 1) when the
  files do not give them. An unknown category gives empty kinds and use.
- `list(category=None, kind=None)`: rows for `hark sfx list` (part 2, 2.1).

## 5. Rendering a take

`render(sound, seed=None, dur=None, apex=None, pitch=None, peak_db=-1.0,
sr=48000, weight=None, checks=True)` returns `(y, meta)`. `y` is float32,
shape `(n,)` for a mono entry or `(n, 2)` for a stereo one. `sound` is an id or
an entry dict (a dict that already carries `id`).

Steps, in this order:

1. Compute the level (5.2). A bad `peak_db` or `weight` raises `BankError`
   before anything renders.
2. Validate `sr` (5.3).
3. Resolve the entry: `get(sound)` for an id, or the dict itself.
4. Make the renderer for the entry and the seed (5.1). Seed `None` means the
   entry's own seed. Any other integer is a new take.
5. When any of `dur`, `apex`, `pitch` is not None, hand the entry and the
   renderer to the knob engine (forge-core `knobs.apply`, contract in part 2,
   section 4). It returns the raw render and the `edits` record. Else render
   once with no edits and set `edits` to `{}`.
6. When `weight` is given, add `edits.weight = {weight, peak_db}` where
   `peak_db` is the final level, 2 decimals.
7. Normalise the raw render so its sample peak is the level, then convert to
   float32.
8. Build `meta` = `{id, seed, sr}` plus the take measures (6.5) plus
   `edits` plus `checks: null`. `seed` is the seed used (the entry's own when
   the argument is None).
9. When `checks` is true: run the audit (part 2, section 3) on `y` with the
   entry's category and the id as context, and put the result in
   `meta.checks`. Then add the pitch warning of the edits (part 2, 4.4) to
   that result when there is one (5.4).
10. When `sr` is not 48000: resample the finished float32 take from 48000 Hz to
    `sr` with a high-quality resampler, and set `meta.dur_s` to
    `len(y)/sr` rounded to 4 decimals. The other times stay as measured at
    48 kHz (they are seconds, so they stay valid). `lufs_m_max` was measured at
    48 kHz.

With seed None the take is the delivered final at -1 dBFS. Any other seed
gives a new take of the same member: the noise, the jitter and the micro-timing change,
the character stays.

### 5.1 The renderer

`renderer(entry, seed=None)` returns a function `run(edit=None, fit_knobs=None)`
that renders the raw float64 signal. The knob engine calls it many times.

- Forge entry: copy `params`, update it with `edit` (a dict of parameter
  overrides), call the entry's recipe function with the parameters and a fresh
  seeded generator `rng(seed)`. `fit_knobs` is ignored.
- Fit entry: call the fit renderer with the model, the seed, take mode on,
  bed off, `vary` (only when not 0), and the `fit_knobs` dict merged over those
  (the knob engine passes the fit's own pitch and length knobs here). `edit`
  is ignored.
- Any other `type` raises `BankError` naming the id and the type.
- Every raw render is validated: an empty result, any non-finite sample, or a
  largest absolute sample under 1e-7 raises `BankError` "the render failed or
  is silent" with the id.

The forge recipe function resolves from `recipe` and `fn` through the
forge-core block resolver.

### 5.2 Level

`level(peak_db=-1.0, weight=None)` returns the sample peak in dBFS that a take
is written at.

1. `peak_db` must be a real finite number (a boolean is not a number). Else
   `BankError` "peak_db must be a number".
2. `peak_db` over 0 raises `BankError`: a peak over 0 dBFS clips; give 0 or less.
3. When `weight` is given: it must be a finite number, else `BankError`. It
   must lie in the half-open range (0, 4], else `BankError`. The level moves
   by `20 log10(weight)` dB (6 dB per doubling: 0.5 is 6.02 dB down, 2 is
   6.02 dB up). A result over 0 dBFS raises `BankError` naming the weight, the
   peak and the resulting level, and says to lower `peak_db`.

### 5.3 Sample rate

`check_sr(sr)` accepts 44100, 48000, 88200 and 96000, as an int or an integral
float (48000.0). It returns the int. A boolean, a string or any other value
raises `BankError` that lists the 4 rates. All rendering, measuring and
auditing happens at 48000 Hz. Only the finished take is resampled.

### 5.4 Adding one warning to an audit result

`add_warning(result, warning)` adds one `{code, msg}` warning (or does nothing
for None): it appends it to `warnings`, sets `ok` to false, and rebuilds the
summary as `<name>: <N> warning(s): <msg>; <msg>` where `<name>` is the text
of the old summary before its first `:`. "warning" is plural when N is not 1.

### 5.5 The delivered final

`render_final(sound)` renders a forge entry exactly as the forge wrote its WAV:
the block resolver re-renders the block (recipe, fn, member, params, seed,
peak_db, gain_db, gain, mode) at the block's level including the palette gain.
A non-forge entry raises `BankError`. This is for checks and gallery use, not
for films.

## 6. Measuring a take

All measures work on the mono mix: the mean of the channels for a 2-D input.

### 6.1 Times: onset, peak, end

Define `env(m, win, hop)`: for window starts `s = 0, hop, 2 hop, ...` while
`s <= max(0, n - win)` (at least one start), the RMS of `m[s : s+win]` (the
window is cut at the end of the signal, and the sum is still divided by `win`).

- Onset: `env` with a 1 ms window and a 0.25 ms hop. The onset is the start of
  the first window whose RMS is at least the largest RMS times
  `10^(-30/20)` (within 30 dB of the maximum). `onset_s` = that start / sr.
- Peak: `env` with a 5 ms window and a 1 ms hop. `peak_s` = (start of the
  loudest window + 2.5 ms) / sr, so the peak is the centre of the loudest
  5 ms.
- End: in the same 5 ms envelope, the last window whose RMS is at least the
  peak RMS times `10^(-50/20)`. `end_s` = min(n, that start + 5 ms) / sr.
- `dur_s` = n / sr.
- All four are rounded to 4 decimals.

At 48 kHz: 1 ms = 48 samples, 0.25 ms = 12, 5 ms = 240.

### 6.2 Peak level

`peak_db` = `20 log10(max |y| + 1e-12)` over every sample and channel, 2 decimals.

### 6.3 Momentary-max loudness

`lufs_m_max` uses the BS.1770 K-weighting filter (the forge-core polish
module owns the filter coefficients). Per channel, filter, square, and sum the
channels. Take the largest mean over any 400 ms window (19200 samples). When
the sound is shorter than 400 ms, use the total sum divided by 19200 (the
sound counts as zero-padded, as a meter shows it). The loudness is
`-0.691 + 10 log10(that mean + 1e-24)`, 2 decimals. It is computed only at
48000 Hz. At another rate it is null.

### 6.4 Fingerprint

1. Mono mix. Frame length n = 50 ms (2400 samples at 48 kHz).
2. Pad the signal with zeros to a whole number of frames (at least 1 frame).
3. Per frame, the mean square in dB: `10 log10(mean(x^2) + 1e-24)`.
4. Subtract the largest frame value, so the loudest frame is 0 dB.
5. Keep the frames up to and including the last frame above -60 dB. Drop the
   rest (where the sound stays 60 dB down).
6. Round each value to 1 decimal.

### 6.5 Take measures and entry meta

- Take measures (`measure(y, sr)`): the 4 times (6.1), `peak_db` (6.2) and
  `lufs_m_max` (6.3).
- Entry meta (`meta_of(y)`): the take measures without `peak_db`, plus
  `attack_ms`, `centroid_hz`, `channels` and `fp` (3.5).

## 7. Building bank files

The build tools are maintenance commands for the hark developers, not film
commands. They run under one maintenance entry point with the subcommands
`build`, `add-fit` and `check` (the index, Interface section, gives the exact
flags). Every builder is all-or-nothing: when one item fails, it writes
nothing and raises one `BankError` that lists every failure ("N pick(s)
refused, nothing written:" and one line per failure).

### 7.1 Building the forge file from forge finals

Input: one or more final folders (glob patterns allowed). A folder counts when
it holds the forge report file (`report.json`, forge-core format). The
category of a folder is the name of its parent folder when the folder itself
is named `final`, else the folder's own name. Folders are deduplicated and
sorted.

For each pick in each report:

1. The pick must carry its own render block. Else refuse it ("no render block
   of its own").
2. Build the full block from the report and the pick (forge-core).
3. The id is `<category>/<member>` (member from the block, else the pick
   name).
4. Re-render the raw signal from the block. A failed or silent render is
   refused.
5. Read the pick's delivered WAV. Put the raw render at the block's written
   level (peak normalisation, then the palette gain). Compare sample by sample
   over every channel. The largest absolute difference must be at most the
   tolerance: one 24-bit step, 1e-6 (forge-core `TOL`). A shape mismatch is an
   infinite difference. Refuse a pick over the tolerance and say by how much.
6. Two picks with one id are refused.
7. The entry is `type forge`, the category, the member, and the block keys
   recipe, fn, params, seed, peak_db, gain_db, gain and mode (those present).
8. `meta` = entry meta (6.5) of the raw render normalised to -1 dBFS, as
   float32.
9. For click, pro_click and toggle entries, add `meta.pitch_hz` (8.2).
10. Log one line per entry: id, length, error.

Each category gets info: `recipe` from the first pick's block, `target` from
the report, `kinds` and `use` from the built-in category table.

The document header: `bank` 1, `name` forge_v2, `version` (default the
current forge tag), the hark version, and a note that the build matched each
entry's render to its WAV to within one 24-bit step.

Measured fact from the source: all 183 picks re-rendered within 1.2e-7 of
their WAVs.

Why the bank stores full blocks: the forge also keeps "seed banks" of good
parameter sets for its searches. Those are rounded and carry no render
function. Re-rendered from them, about a third of the delivered finals came
out as different sounds (34 of 94 checked). So the bank must store the exact
block.

### 7.2 Building the pluck file

The pluck file holds 3 tuned kalimba voices. Each is the pluck recipe's
default parameter set (that default is the pick of the source's launch film),
with `f0` set to C5 (523.2511306011972 Hz), then edited:

| id | edit | seed | what it is |
|---|---|---|---|
| `pluck/kalimba` | none | 8001 | a bright steel tine on a small wood resonator box |
| `pluck/kalimba_soft` | thumb contact time = 1.4 ms (a value, not a factor); skin level x0.4; knock level x0.5; zing level x0.5; 2nd-partial level x0.75 | 8009 | a pluck with the soft flesh of the thumb: a darker, gentler attack |
| `pluck/kalimba_ring` | first-partial decay constant x1.5 | 8006 | a longer ring: end cards and chord tones |

Every edit value is a factor on the default, except the thumb contact time,
which is set directly. The recipes-impacts spec names the pluck parameters.
Each entry: `type` forge, category pluck, member, recipe pluck, fn main,
the full params, the seed, `peak_db` -1, `gain_db` 0, `notes`, and `meta` of its
render normalised to -1 dBFS. The file's single category is `pluck` with the
the target text of part 4, 1.3, kinds and use
from the table. Version tag default `pluck1`. Every pluck file is 3 s long
because the recipe always renders 3 s of ring and room.

### 7.3 Building the forge04 file

The forge04 members are named palette members of their recipes (the recipes
specs define the palettes). Part 4, section 3, lists the 51 ids, the recipe
each one comes from, its seed and its kinds. For each member:

1. Build the bare entry: look up the named palette member in its recipe and
   complete its parameters with the recipe's defaults (a recipe whose palette
   is grouped by category completes it with its own per-category base). A
   member of a "run" palette uses the typed-run style render function and the
   run defaults. A missing member is a `BankError` naming the recipe and the
   member. Entry: `type` forge, category, member, recipe, fn, params, seed,
   `peak_db` -1, `gain_db` 0, `kinds`, `notes`.
2. Render it on its seed. Normalise to -1 dBFS as float32. Store `meta` (6.5)
   and `profile` (3.6).
3. Audit 6 takes (6 takes): its own take (seed n) and the takes on
   seeds n+1 to n+5 (rendered by section 5 with checks off). Audit each with
   the audit of part 2, section 3, with the category and the entry's first
   kind as the role. Store `audit = {role, seeds: [n, n+5], clean: count of
   clean takes, warnings: sorted union of warning codes}`.
4. Refuse the member when its own take is not clean, or when fewer than 5 of
   the 6 takes are clean (the minimum is 5).
5. Originality: each member needs a measured value `max_ref_clap` = the
   highest CLAP cosine between its render and each clip of a reference set. It must be
   at most 0.96 (the originality limit). The value comes from a fresh measurement
   passed in (`ref_clap` = `{id: cosine}`, from `--ref-clap file.json`), else
   from a recorded value. A recorded value holds only for the exact render it
   was measured on: the builder keeps, per member, a 10-hex digest of the
   render block (the first 10 hex digits of the SHA-1 of the canonical JSON of
   `{recipe, fn, params, seed}`, keys sorted, no spaces). The recorded value is
   stale when the digest differs, or when the shipped file's entry for the
   member has the same digest but the new render fails the fingerprint check
   against it (length differs, peak more than 5 ms off, or envelope more than
   0.5 dB off; section 9). A stale, missing or over-limit value refuses the
   member. A refused value is never written as if it was measured.
6. Store `original = {made: "recipe", max_ref_clap: <value>}`.
7. When fresh values were passed, log the digests to record next to them.

Categories: the file names only the 7 new categories (cut_hit, reverse_swell,
ui_pop, knock, mallet, drip, shutter), each with its recipe, the recipe's own
target line, kinds and use. Version tag default `forge04r1`.

The data plan for the originality value is in the index (Data assets: it
needs a reference set that hark does not have).

Measured facts from the source: 50 of 51 members had 6 of 6 clean takes,
`glitch/mid` had 5 of 6 (one take rings). The 51 originality values were 0.53
to 0.84. Between two members of one new category the largest CLAP cosine was
0.93 (cut_hit sub and kick; the 2 mid dings 0.95), under the about 0.97 that 2
renders of one sound reach.

### 7.4 Adding a fit entry

`add_fit(model, sound_id, source, bank_path=None, seed=0, vary=0.0,
notes=None, kinds=None, allow_license=False, force=False)` adds one fitted
model to a bank file and returns the entry plus its id.

1. `sound_id` must match `^fit/[a-z0-9_.]+/[a-z0-9_]+$`. Else `BankError`
   with the expected shape.
2. `model` is a dict or a path to a fit model JSON (loaded by the fit unit).
   `source` is a dict or a path to a JSON file.
3. `source` must have all of `id`, `title`, `author`, `license`, `url` as
   non-empty strings. Else `BankError` listing the missing keys.
4. Licence rules on the lower-cased licence text:
   - It is refused when it holds the word `nc` or `nd`, or the text
     `noncommercial`, `non-commercial` or `noderiv`: a fit is a derivative work
     and ships inside the hark package.
   - It must contain one of `cc0`, `public domain`, `publicdomain`, `cc-pdm`,
     `pdm`. Else it is refused, unless `allow_license` is set (then the user
     must credit the source).
5. When `vary` is not 0 and the fit model has no vary knob (an air-only
   gesture model has no tones to jitter), refuse it: takes still differ by seed.
6. Remove the fit-engine report from the model, keep its `chosen` variant and
   its character class.
7. Build the entry (3.3): member = the id without `fit/`, the model, the seed,
   `vary`, the 5 source keys, `fit = {kind, chosen, class}`, `kinds` (default
   `[role]`, the middle part of the id), `notes` (default "fitted from <title>
   (<license>)").
8. Render it (5.1), normalise to -1 dBFS, store `meta` (6.5) and `pitch_hz`
   (8.3).
9. The target file is `bank_path`, default the package `fit.json`. When it does
   not exist, start a new document: `bank` 1, `name` fit, `version` fit1, the
   hark version, a note, and the single category `fit` (recipe null, target
   null, kinds empty, use = the text of part 4, 1.3).
10. An id already in the file is refused unless `force`.
11. Put the entry in, set the file's hark version, write (3.1). Log one line:
    the id, the file, the length, the model kind, the pitch when there is one,
    and the licence.

## 8. Pitch measures used by the bank

### 8.1 Strongest peak near a frequency

`estimate_pitch(y, near_hz, span_st=4.0, sr=48000)` returns a frequency in Hz
or None.

1. Mono mix. Take the segment from the onset (6.1) to 0.6 s after it.
2. A segment shorter than 512 samples gives None.
3. FFT size: `2^(ceil(log2(max(len, 16384))) + 2)`. Window the segment with a
   Hann window of its own length, zero-pad to the FFT size, take the power
   spectrum.
4. The search band is `near_hz * 2^(-span_st/12)` to `near_hz * 2^(span_st/12)`.
   A band of fewer than 3 bins gives None.
5. j = the strongest bin in the band.
6. Reject (None) when the power at j is under 10 times the median power over
   the octave `[near_hz/2, 2 near_hz)`: a peak needs 10 dB over the median
   of its octave; else the sound has no pitch there (a noise sound).
7. Refine j with a parabola through the log powers at j-1, j, j+1:
   `d = 0.5 (a - c) / (a - 2b + c)` (d = 0 when the denominator is 0).
8. Return `(j + d) * sr / FFT size`.

### 8.2 The heard partial of a click or toggle

For forge entries of `click`, `pro_click` and `toggle`, the recipe's pitch
parameter sets the centre of both the tink partials and the noise band, not the
partial that a listener hears (for `click/soft` the parameter is 2454 Hz and
the heard partial is 2903 Hz, +291 cents). The heard partial is
`estimate_pitch(render, the pitch parameter, span_st=6)`. The build stores it
as `meta.pitch_hz` (2 decimals) or null. `click/thock` and `toggle/micro`
have null: no clear partial, so they refuse the pitch knob.

### 8.3 The note of a tonal fit

1. When the fit report's character says the sound is not tonal, null.
2. Mono mix. Take the segment that starts at the onset and lasts 0.6 s. Fewer than 1024 samples:
   null.
3. FFT size 2^17, Hann window, power spectrum. Find the strongest bin between
   60 Hz and 5000 Hz.
4. Refine with `estimate_pitch(y, that bin's frequency, span_st=1)`.
5. The result, 2 decimals, or null.

## 9. Checking a bank against its fingerprints

`check(ids=None)` re-renders entries (all by default) and compares each with
its stored `meta`. Per entry: render the take at its own seed, -1 dBFS, checks
off, then compare (`compare_fp`):

| field | meaning |
|---|---|
| `len_diff` | (new `dur_s` - stored `dur_s`) * sr, rounded to an int: samples |
| `peak_diff_ms` | 1000 * (new `peak_s` - stored `peak_s`), 2 decimals |
| `env_db` | the largest absolute difference between the stored and the new fingerprint, over the frames both have, counting only frames whose stored value is above -40 dB; 0 when none count; 2 decimals |
| `ok` | true when `len_diff` is 0, `abs(peak_diff_ms)` is at most 5 and `env_db` is at most 0.5 |

The maintenance command prints one line per entry (id, length difference,
peak difference, envelope difference, `ok` or `DIFFERS`), then a summary
`K / N entries re-render as stored` plus the ids that differ. It exits 1 when
any entry differs, else 0.

Measured fact from the source: on the build machine every entry re-rendered
bit-exactly, and the shipped check passed 252 of 252.

## 10. Constants of the bank

| name | value | meaning |
|---|---|---|
| internal rate | 48000 | the internal rate of every render, measure and audit |
| format number | 1 | the bank file format number |
| default peak | -1.0 | the default take level, dBFS |
| write rates | 44100, 48000, 88200, 96000 | the rates a take can be written at |
| onset threshold | -30 | onset threshold re the 1 ms envelope's maximum, dB |
| end threshold | -50 | end threshold re the loudest 5 ms, dB |
| onset envelope | 1 ms window, 0.25 ms hop | |
| peak and end envelope | 5 ms window, 1 ms hop | |
| fingerprint frame | 0.05 s | fingerprint frame |
| fingerprint floor | -60 dB | frames after the sound falls and stays under this level are dropped |
| fingerprint compare floor | -40 dB | only stored frames above this count |
| check tolerances | length exact, peak 5 ms, envelope 0.5 dB | |
| build tolerance | 1e-6 | one 24-bit step, the largest sample difference to the delivered WAV |
| weight range | (0, 4] | linear, 20 log10 dB |
| LUFS window | 400 ms | momentary loudness |
| LUFS offset | -0.691 dB | BS.1770 |
| silent render floor | 1e-7 | a raw render with a smaller peak is a failed render |
| pluck pitch | 523.2511306011972 Hz | C5, the native pitch of the pluck voices |
| pluck seeds | 8001, 8009, 8006 | kalimba, kalimba_soft, kalimba_ring |
| forge04 audit takes | 6 | takes audited per forge04 member |
| forge04 clean minimum | 5 | clean takes needed of those 6 (the own take always) |
| forge04 originality limit | 0.96 | the largest allowed CLAP cosine to any reference clip |
| block digest | SHA-1 of the canonical JSON of {recipe, fn, params, seed}, first 10 hex digits | |
| estimate_pitch window | onset to onset + 0.6 s, at least 512 samples | |
| estimate_pitch rejection | power under 10 x the octave median | 10 dB |
| heard-partial span | 6 semitones | click, pro_click, toggle |
| fit note band | 60-5000 Hz, FFT 2^17, refined over 1 semitone | |
| fit id pattern | `^fit/[a-z0-9_.]+/[a-z0-9_]+$` | |
| free licences | cc0, public domain, publicdomain, cc-pdm, pdm | substring match, lower case |
| refused licence words | nc, nd (as words), noncommercial, non-commercial, noderiv | |

## 11. Cost targets

These are measured times of the source on a development laptop (8-core
Ryzen class, 2 threads, a busy CPU). Hark should land in the same range. They
are not pass criteria.

| step | time |
|---|---|
| load the bank (import and file read) | 1.3 s (3 s when the disk cache is cold) |
| first render in a process (recipe import, JIT kernels from cache) | 10 s (25-45 s on the first run after an install, while kernels compile) |
| the same render again | 54 ms |
| short interface sounds (click, tap, pop-in, key, ding): render / render + audit | 40-65 ms / 80-160 ms |
| whooshes, swipes, bells, sub drops | 0.1-0.45 s / 0.25-0.8 s |
| pluck (3 s), impact (4.8 s) | 1.0-1.1 s / 1.4-2.2 s |
| a whoosh with dur and apex set (at most 3 renders, then the audit) | 1.9 s |
| median render + audit over the 186 forge and pluck sounds | 0.5 s |
| kit render, a whole preset, fresh process | calm 34 s (64 takes), crisp 22 s (64), minimal 9 s (63), launch 41 s (72) |
| kit render with every take cached | 3-5 s, mostly process start (0.1-0.6 s inside the process) |

A film pipeline renders its sounds in one process and keeps the takes: the
kit lock and its cache exist for that.
