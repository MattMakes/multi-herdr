# skills-and-docs, part 4: the design format, the mixer, the master chain and QA

Part of the `skills-and-docs` spec. Index: `docs/specs/hark/skills-and-docs.md`.

This part specifies 3 scripts of the `sound-sourcing` skill and the JSON they share:

- `scripts/mix.py`: renders a JSON sound design to stems, a premaster and a master.
- `scripts/master.py`: the mastering chain and the loudness meter.
- `scripts/qa.py`: measured QA of a render, tied to the placement that caused each finding.

All 3 run under the Python of the hark base venv (`~/.local/state/hark/venv/bin/python`), with numpy, scipy,
soundfile, soxr, librosa and pyloudnorm. They call `ffmpeg` and `ffprobe` on the PATH. The source runs every
script with `python -I` (isolated mode) so that a downloaded file can never shadow a module. Keep that rule. A
script that imports a sibling adds its own folder to `sys.path` itself.

Sample rate everywhere: 48000 Hz (written SR below). Audio is float32 stereo `(n, 2)` unless a step says otherwise.

## 1. Interface

### 1.1 The cue file (input, written by the picture side)

A JSON object, or a JSON list whose first element is that object.

| key | type | meaning |
|---|---|---|
| `fps` | number | frames per second. Default 60 when absent. |
| `total` | number | film length in frames. |
| `bpm` | number | optional, the picture's grid tempo (read by people, not by the mixer). |
| `events` | list | the cue events, below. |
| `acts` | object | optional. `name -> {from, dur}` in frames. QA reports loudness per act. |
| `cue` | object | optional. `name -> frame`. Named moments. Not read by the scripts. |

Event fields:

| key | type | meaning |
|---|---|---|
| `f` | number | the absolute frame the sound belongs on. Fractional values are allowed. Required. |
| `kind` | string | what happens: `tick`, `tock`, `key`, `click`, `pop`, `snap`, `land`, `hit`, `drop`, `whoosh`, `riser`, `swell`, `suck`, `bell`, or a word of the user's own. Required. |
| `id` | string | a stable label with the shape `act:thing:n`. Rules match it with a regex. |
| `weight` | number | 0..2, 1 = normal. Rules can scale level by it. |
| `pan` | number | -1..1, the object's screen x. |
| `variant` | string | a sub-type: for `key` one of `word`, `space`, `return`, `back`; for `land` `light`; for `click` `confirm`. |
| `dur` | number | length in frames, for `whoosh`, `riser`, `swell`. |
| `apexFrac` | number | where in `dur` a whoosh peaks (0..1). |
| `pitch` | number | optional pitch hint. Not read by the mixer. |

The sync point of each event is where the eye reads the cause: an impact on its contact frame, a pop-in at 50 % of
its travel, a whoosh on the velocity peak of the move, a riser on the frame of the hit it leads into, a logo sound on
the frame where the wordmark becomes readable. Cue frames come from the picture code. Hand-typed sync points land
8-11 frames off in practice.

### 1.2 The design (input, written by the lead and the designers)

One top-level file (default `audio/sounddesign.json`) plus fragments it includes (`audio/design/<category>.json`).
A fragment is also a valid single-file design. Keys that start with `_` are comments. The mixer ignores unknown keys
everywhere, except inside `buses`: there every key that does not start with `_` must be `sfx`, `amb` or `music`.

Every relative path (the design, the cues, the outputs, every `file`) resolves against `--root`.

#### Top level

| key | default | meaning |
|---|---|---|
| `duration` | cue `total / fps` | film length in seconds. |
| `cues` | `out/cues.json` | default cue file. The `--cues` flag wins. |
| `include` | none | fragment paths, merged as in 2.1. |
| `samples`, `slices`, `pools`, `rules`, `place`, `ambience`, `music` | none | also allowed at the top level. |
| `buses`, `master` | none | read from the top-level file only. |

Name everything in a fragment with the fragment's prefix (`kb.`, `ui.`, `mo.`, `am.`, `mu.`, ...). Fragments are
built in parallel by different agents, and a duplicate name is an error.

#### `samples`: `name -> object`, a trimmed piece of one recording

| key | default | meaning |
|---|---|---|
| `file` | required | any file ffmpeg reads. |
| `start`, `end` | 0, end of file | the trim, in source seconds. |
| `mono` | false | fold to mono (mean of the 2 channels, put on both). |
| `eq` | none | the EQ object, 1.3. |
| `pitch` | 0 | semitones by resampling. Pitch and length change together. |
| `glide` | none | `[from, to]` semitones, a varispeed glide across the sample. |
| `env` | none | `[[t, dB], ...]`, a piecewise-linear gain curve, t in seconds from the sample start. |
| `norm_peak_db` | none | scale the processed sample so its sample peak is this dBFS. |
| `fade_in`, `fade_out` | 0.002, 0.012 | raised-cosine fades in seconds. |
| `gain_db` | 0 | static gain, applied last. |
| `reverse` | | refused with an error. The design keeps a recording in its real direction. |
| `qa_ok` | | a reason string. QA lists this sample's edge finding as acknowledged. |

#### `slices`: `name -> object`, one recording of many hits cut into single hits

The slices get the names `name#0`, `name#1`, ... in the order they are kept.

| key | default | meaning |
|---|---|---|
| `file`, `start`, `end` | | as for a sample: the window to search. |
| `delta` | 0.07 | onset detector threshold. Lower finds softer hits. |
| `min_gap` | 0.06 | minimum seconds between onsets. |
| `gate_db` | -18 | drop onsets whose first-30 ms peak is more than this many dB under the loudest onset. |
| `pre` | 0.004 | seconds kept before each onset. |
| `len` | 0.16 | maximum slice length in seconds. |
| `stop_before_next` | true | end each slice `pre` seconds before the next onset. |
| `pick` | all | list of slice indices to keep, in that order. |
| `max` | all | keep at most this many (after `pick`). |
| sample options | | `mono`, `eq`, `pitch`, `glide`, `env`, `norm_peak_db`, `gain_db`, `fade_in` (0.002), `fade_out` (default 0.02 for slices). |

#### `pools`: `name -> [sample names]`

`"group#*"` expands to every slice of the slice group `group`, sorted by index. A name that is not a sample is an
error. A pool that ends up empty is an error.

#### `rules`: a list of cue-driven placements

Every cue event that a rule matches gets one sound from the rule's pool. Several rules that match one event layer it.

| key | default | meaning |
|---|---|---|
| `name` | the rule's index | the key of all hashed variation. Renaming a rule changes its variation. |
| `match` | `{}` (all events) | `kind`: regex, full match on the event kind. `id`: regex, search in the event id. `from`, `to`: film seconds, the half-open range `[from, to)`. `variant`: exact string. |
| `pool` | required | the pool to play. |
| `mode` | `cycle` | `cycle`, `hash` or `fixed`, see 2.3. |
| `cycle_start` | 0 | first index for `cycle`. |
| `gain_db` | 0 | level. |
| `weight_db`, `weight_ref` | 0, 1.0 | dB per doubling of the event weight against `weight_ref`. |
| `jitter_db` | 0 | +- hashed random level per event. |
| `pitch` | `[0, 0]` | `[lo, hi]` hashed random semitones per event. |
| `pitch_ramp`, `gain_ramp` | none | `[a, b]` added linearly across the matched events in time order. |
| `pan` | `"cue"` | `"cue"` = event `pan` x `pan_scale`; or a number -1..1. |
| `pan_scale` | 1.0 | |
| `align` | `attack` | the point of the sound placed on the cue: `attack`, `peak`, `end`, `start`. |
| `offset` | 0 | seconds added to the cue time. |
| `bus` | `sfx` | `sfx`, `amb` or `music`. |
| `no_cue` | false | the placement does not mark its event as sounded. |
| `qa_ok` | | a reason string, see 4.11. |

#### `place`: a list of free placements (no cue event)

`{sample, at, gain_db? (0), pan? (0), align? (attack), offset? (0), pitch? (0), bus? (sfx), qa_ok?}`. `at` is film
seconds. `sample` is any sample or slice name (`group#3` is valid).

#### `ambience`: a list of beds on the `amb` bus

| key | default | meaning |
|---|---|---|
| `name` | none | label. |
| `file` | required | the recording. |
| `src_start`, `src_end` | 0, end | the clean window of the recording. |
| `at`, `until` | required | film seconds. |
| `fade_in`, `fade_out` | 1.0, 1.0 | seconds. |
| `gain_db` | 0 | |
| `eq` | none | 1.3. |
| `width` | none | side-signal scale: 0 = mono, 1 = as recorded, above 1 = wider. |
| `auto` | none | `[[film t, dB], ...]`, a gain ride inside the bed. |

#### `music`: one object

| key | default | meaning |
|---|---|---|
| `file` | required | the track. |
| `src_bpm` | required | the track's measured tempo. |
| `target_bpm` | 120 | the picture tempo. |
| `stretch` | `rubberband` | `none` plays the source at its own speed. Otherwise the whole file is time-stretched by `target_bpm / src_bpm`. |
| `gain_db` | 0 | music level. |
| `eq` | none | EQ on the whole music bus. |
| `segments` | required | list, below. |

Segment fields: `src_from`, `src_to` (ORIGINAL source seconds, before any stretch, on downbeats), `at` (film second
where `src_from` lands), `fade_in` (0.008), `fade_out` (0.06), `fade_in_at_start` (true; false = no head fade),
`tail_fade` (seconds of a long fade over the segment's end), `gain_db` (0), `label` (a string: the bars and the
reason), `qa_ok`.

#### `buses` and `master`

`buses: {sfx|amb|music: {gain_db?, eq?, auto?}}`. `auto` is `[[film t, dB], ...]`.

`master: {lufs?, ceiling?, no_glue?, no_comp?, mono_hz?}` with defaults -14, 0.77, true, false, 120.

### 1.3 The EQ object

`{hp, hp_order (2), lp, lp_order (2), peq: [[f0, q, gain_db], ...], lowshelf: [f0, gain_db], highshelf: [f0, gain_db]}`.
`hp` and `lp` are Butterworth filters of the given order, run as second-order sections. `peq`, `lowshelf` and
`highshelf` are RBJ cookbook biquads (2.2).

### 1.4 `mix.py` command line

```
mix.py [--root .] [--design audio/sounddesign.json] [--cues PATH] [--out PATH] [--stems DIR] [--no-master]
       [--only TEXT] [--dump DIR] [--report PATH] [--cues-out PATH] [--trace PATH] [--cache DIR] [--master-script PATH]
```

| flag | default | meaning |
|---|---|---|
| `--root` | `.` | project root. |
| `--design` | `audio/sounddesign.json` | |
| `--cues` | design `cues`, else `out/cues.json` | |
| `--out` | `out/audio/soundtrack.wav`; solo: `out/audio-review/solo/<tag>.wav` | the master, or the unmastered sum with `--no-master` / `--only`. |
| `--stems` | `out/audio/stems`; solo: `<out base>_stems` | |
| `--no-master` | off | write the unmastered sum to `--out`. |
| `--only` | none | solo render: keep only the includes whose path contains this text. Forces `--no-master`. |
| `--dump` | none | write every sample and slice as WAV plus `_samples.json`, then exit. |
| `--report` | `out/qa/mix_report.json`; solo: `<out base>_report.json` | |
| `--cues-out` | `out/qa/cues_sounded.json`; solo: `<out base>_cues.json` | |
| `--trace` | none | write every bus add as JSON. |
| `--cache` | `<root>/out/audio/cache` | decode cache; a relative value resolves against the root. |
| `--master-script` | `master.py` next to `mix.py` | |

`<tag>` is the `--only` text with every run of characters outside `[A-Za-z0-9._-]` replaced by `_`. `<out base>`
is `--out` without its extension. Example: `--only ui --out out/audio-review/ui/solo.wav` writes
`solo_report.json`, `solo_stems/` and `solo_cues.json` next to `solo.wav`.

### 1.5 `mix.py` outputs

| output | format | content |
|---|---|---|
| `--out` | WAV 48 kHz | mastered: 24-bit PCM from `master.py`. With `--no-master`/`--only`: the unmastered sum, 32-bit float. |
| `premaster.wav` | WAV float, next to `--out` | the unmastered sum (only when mastering). |
| `--stems` | `sfx.wav`, `amb.wav`, `music.wav`, WAV float | each bus after its bus processing. The 3 stems sum to the premaster. |
| `--report` | JSON | see below. |
| `master.json` | JSON, in the folder of `--report` | the `master.py` report (only when mastering). |
| `--cues-out` | JSON | `{fps, total, events}`: the events that got an sfx-bus sound. |
| `--trace` | JSON list | one record per bus add. |

Report fields:

- `placements`: per rule placement `{t, event, kind, rule, sample, gain_db, pitch, pitch_total, pan, align}`; per
  free placement `{t, event: null, kind: "place", sample, gain_db, bus}`. Order: rules in merged order (2.1), events
  in time order inside each rule, then free placements in design order.
- `unsounded_events`: `{t, kind, id}` for each cue event with no counting placement (2.3).
- `buses`: `name -> {peak_dbfs, rms_dbfs}`.
- `ambience`: per bed `{name, file, at, until, looped, body_rms_dbfs}`.
- `music_segments`: each segment's own fields plus `stretched_from` (seconds in the stretched file where the cut
  starts, pre-roll included) and `film_start` (film second where that cut starts).
- `files_used`: sorted list of every recording that is audible: the files of placed samples and slices, every
  ambience file, the music file. Paths as written in the design, with forward slashes. This list is the input of
  `credits.py` and of the rights audit.
- `premaster_peak_dbfs`.

Trace record: `{bus, start, n, gain, pan, ...meta}`. `start` and `n` are in samples (start may be negative). `gain`
is linear. Meta is `{sample, rule, kind}` for a rule add, `{sample, kind: "place"}` for a free placement,
`{name, kind: "ambience"}` for a bed, `{label, kind: "music"}` for a music segment.

`--cues-out` events keep only the keys `f`, `kind`, `weight`, `id`, `pan`, `dur`, `apexFrac`. An event of kind
`riser` or `swell` also gets `"ends": true` (the timeline listener aligns it by its end). `total` is the cue total,
else `int(duration x fps)`.

`--dump` writes `<safe name>.wav` (24-bit) per sample and slice. The safe name replaces every run of characters
outside `[A-Za-z0-9._#-]` with `_`, then `#` with `-`. `_samples.json` is a list. A sample row is `{name, file, src,
dur, peak_dbfs, rms_dbfs, attack_ms, peak_ms, head_db, tail_db}`: `src` is `{file, start, end, pitch}` (slices:
`{file, start, pitch}`, start = the slice's source second), `head_db` is the max of the first 2 ms re the peak,
`tail_db` the max of the last 20 ms re the peak. A pool row is `{pool, size, items}`. The command prints one line per
row and exits without rendering.

Console: one line with the premaster path, its peak, the placement count and the unsounded count; then one line per
bus. Output is ASCII only (non-ASCII is replaced) because some consoles cannot print it.

### 1.6 `master.py` command line

```
master.py INPUT [OUTPUT] [--measure] [--lufs -14] [--ceiling 0.77] [--tol 0.5] [--max-tp -1.5]
          [--sidechain KEY.wav] [--sc-depth 0.55] [--sc-tau 0.14] [--mono-hz 120] [--no-comp] [--no-glue]
          [--fade 0] [--tail-zero 480] [--json PATH|-]
```

- INPUT is read with soundfile at any rate. Mono is duplicated to 2 channels, extra channels are dropped, and other
  rates are resampled to 48 kHz with a polyphase resampler.
- OUTPUT is required unless `--measure`. It is written as 48 kHz 24-bit PCM.
- `--json -` prints the report. A path writes it.
- The console line has the shape `PASS|FAIL  <lufs> LUFS  TP <tp> dBTP  LRA <lra> LU  loudest <m> LUFS-M at <t>s`,
  then one indented line per gate failure.
- Exit 0 when the gate passes, 1 when it fails or on any error (`error: <message>` on stderr).

Report: `{lufs, true_peak_db, sample_peak_db, lra, momentary_max, momentary_max_t, tail_50ms_db, last_480_zero,
seconds}` plus, when it masters, `limiter_max_gr_db` and `output`; always `pass` and `fails` (a list of strings).

### 1.7 `qa.py` command line

```
qa.py [--root .] [--design audio/sounddesign.json] [--report PATH] [--trace out/qa/adds.json] [--stems DIR]
      [--master PATH|none] [--cues PATH] [--cues-sounded out/qa/cues_sounded.json] [--only TEXT] [--cache DIR]
      [--video PATH] [--out out/qa/qa.json] [--layers] [--listen] [--realness] [--json]
```

| flag | default | meaning |
|---|---|---|
| `--report` | `out/qa/mix_report.json`; with `--only`: `out/audio-review/solo/<tag>_report.json` | |
| `--stems` | `out/audio/stems`; with `--only`: `out/audio-review/solo/<tag>_stems` | |
| `--master` | `out/audio/soundtrack.wav`; with `--only`: none | `none` skips it. |
| `--trace` | `out/qa/adds.json` | used when the file exists. |
| `--cues` | design `cues`, else `out/cues.json` | a missing file means no events. |
| `--only` | | the same text the mix used. |
| `--layers` | off | also judge each layer of multi-layer events. |
| `--listen` | off | hark timeline, sheet and critique of the master. |
| `--realness` | off | hark realness of every recording in `files_used`. |
| `--video` | none | the film, passed to hark. |
| `--json` | off | print the full JSON instead of the digest. |

Output: `--out` JSON `{summary, status, note, sections, inputs}`. NaN and infinities become `null`, numpy numbers
become plain numbers. `sections` maps a section name (`clicks`, `loudness`, `coverage`, `edges`, `audibility`,
`harshness`, `sync`, `density`, `listen`, `mix`) to an object with at least `status` (`ok`, `warn`, `fail`,
`skipped`) and `summary`. The console gets a digest: the overall summary, one line per section with its status in
capitals, and up to 5-8 detail lines for clicks, edges, audibility, harshness and sync. Times print as both seconds
and `m:ss.ss`.

Exit codes: 2 for a missing or mismatched input (one line `qa.py: <what> not found: <path> (<flag to pass>)`), 1
when the worst section status is `fail`, else 0.

## 2. Behaviour and algorithms of `mix.py`

### 2.1 Loading the design

1. Read the top-level design. A missing file exits with an error naming the absolute path.
2. For each path in `include`, in order: with `--only`, skip it unless the path contains the `--only` text (this
   filter applies to the top level only). Load the fragment recursively (its own includes are loaded too, unfiltered).
3. Merge `samples`, `slices` and `pools` by name. A name already present, from the top level or an earlier fragment,
   is an error naming the fragment, the section and the name.
4. Merge `rules`, `place` and `ambience` by list concatenation, with the fragment's list placed BEFORE the list so
   far. Consequence: the last include's rules come first in the merged list. The order changes only report order, not
   sound, because all variation is per rule (2.3).
5. Take `music` from the first fragment that has one, when the top level has none.

### 2.2 Decoding and per-sample processing

Decoding: `ffmpeg -i FILE` to 32-bit float, 2 channels, 48 kHz, read from stdout. A mono source becomes 2 channels
by ffmpeg's own channel conversion. A failed decode raises an error with the first 300 characters of stderr.

Decode cache: the key is the first 16 hex digits of SHA-1 of `"<absolute path>|<size>|<mtime>"`. The cache stores a
`.npy` array in the cache folder. Writes go to a temporary name with the process id, then an atomic rename, so 2
mixers can share the cache. Each process also keeps decoded arrays in memory.

Processing order of a sample (after the trim `[int(start x SR), int(end x SR))`):

1. `mono`: the mean of the 2 channels on both channels.
2. `reverse` present: error.
3. `eq`, in float64, in this order: high-pass, low-pass, each `peq`, low shelf, high shelf. Output float32.
4. `pitch`: resample by the ratio r = 2^(semitones/12) with a very-high-quality resampler (soxr VHQ): treat the
   input as sampled at SR x r and convert to SR. The output is len/r samples long. Skip when |semitones| < 1e-4.
5. `glide [s0, s1]`: varispeed, 2.4.
6. `env`: points sorted by time, the dB value linearly interpolated at every sample time and held flat outside the
   points, converted to linear gain and multiplied in.
7. `norm_peak_db`: multiply by 10^(norm/20) / peak, where peak is the max absolute sample (1.0 when the sample is
   silent).
8. Fades: raised-cosine fade-in over int(fade_in x SR) samples and fade-out over int(fade_out x SR) samples, each
   clipped to the sample length. The fade curve over n samples is w[i] = 0.5 - 0.5 cos(pi x i/(n - 1)), i = 0..n-1,
   rising for the fade-in, reversed for the fade-out.
9. `gain_db`.

Because step 7 runs after step 3, an EQ cut followed by `norm_peak_db` renormalises the peak. A cut on a band that
does not hold the peak can make that band LOUDER in the mix. The fix is a lower `norm_peak_db` together with the cut.

RBJ biquads (Audio EQ Cookbook): A = 10^(gain/40), w0 = 2 pi f0/SR, alpha = sin(w0)/(2Q), c = cos(w0).

- Peaking: b = [1 + alpha A, -2c, 1 - alpha A], a = [1 + alpha/A, -2c, 1 - alpha/A].
- Low shelf and high shelf use the cookbook shelf formulas with the term 2 sqrt(A) alpha, and Q fixed at 0.707.
- Normalise b and a by a[0]. Run as a direct-form IIR filter along time.

Pan: p clipped to -1..1, theta = (p + 1) pi/4, left gain sqrt(2) cos(theta), right gain sqrt(2) sin(theta). Centre
is unity on both channels. A pan of exactly 0 applies no gain.

### 2.3 Rules: from cue events to placements

1. Cue file: `--cues`, else the design's `cues`, else `out/cues.json`. If it is missing and the design has rules,
   exit with an error that says to pass `--cues`. If it is missing and there are no rules, use `{fps: 60, events: []}`.
2. Duration: the design's `duration`, else cue `total / fps`. If neither exists, exit with an error that asks for
   one of them.
3. Check that every referenced recording exists (samples, slices, ambience, music). If any is missing, exit with a
   count and one line per item `<kind> <name>: <path>`, paths relative to the root.
4. Build the sample library (samples, then slices, then pools).
5. Sort the events by `f` (stable).
6. For each rule, in merged order, with rule key K = `name` or the rule index:
   - Matched events M = the events that pass `match`, in time order. Matching: `kind` is a full regex match on the
     event kind; `id` is a regex search in the event id (missing id = empty string); the event time t = f/fps must
     satisfy t >= from - 1e-6 and t < to - 1e-6; `variant` must equal the event variant.
   - For the i-th matched event e (i = 0..|M| - 1):
     - Choose the sample. `fixed`: the first pool item. `hash`: item number floor(h("<id>|<f>|K") x n) mod n.
       `cycle`: a counter per rule starts at `cycle_start`. Take item counter mod n. If n > 1 and that item equals
       the item this rule played last, step the counter once more. Then store counter + 1 and remember the item.
     - Semitones: with `pitch [lo, hi]` and hi != lo, lo + (hi - lo) x h("p|<id>|<f>|K"), else lo. Add the ramp
       a + (b - a) x i / max(1, |M| - 1) when `pitch_ramp [a, b]` is set.
     - Gain dB: `gain_db` + the `gain_ramp` term (same ramp formula) + `weight_db` x log2(max(weight, 0.001) /
       `weight_ref`) (weight default 1) + `jitter_db` x (2 h("g|<id>|<f>|K") - 1).
     - Pan: event pan (default 0) x `pan_scale` when the rule pan is `"cue"`, else the rule's number.
     - Sound x = the library sample at those semitones. Pitched copies are cached per (name, semitones rounded to 2
       decimals). |semitones| < 0.001 uses the unpitched sample. Rule pitch adds to the sample's own `pitch`.
     - Time t = f/fps + `offset`. Start sample = round(t x SR) - align_index(x, `align`).
     - Add x to the rule's bus at that start with the gain and pan, and record the placement and the trace record.
     - If the bus is `sfx` and `no_cue` is false, mark the event (key: f, kind, id) as sounded. The first time an
       event is marked, append it to the sounded-cue list.
7. Every event not marked becomes an `unsounded_events` entry. Events sounded only on `amb` or `music`, or only by
   `no_cue` rules, are unsounded.

h(s) = CRC-32 (the zlib polynomial, unsigned) of the UTF-8 string s, divided by 2^32 - 1. The string uses the event
id (the text `None` when absent) and the frame number as written in the cue file. Any deterministic text form is
acceptable for hark, but the same event must give the same string in a solo render and in a full mix.

Solo equals full: variation depends only on the rule key, the event and the rule's own matched list. A solo render
(`--only`) therefore plays the same sounds as the full mix, provided every rule has a `name` (an unnamed rule is
keyed by its index, and the index changes when fragments are left out).

### 2.4 Align index and varispeed

align_index(x, mode), with m[i] = max absolute value over the 2 channels:

| mode | index |
|---|---|
| `start` | 0. |
| `end` | the last i with m[i] > 0.003 x max(m) (about -50 dB); the last sample when none. |
| `attack` | split m into 1 ms blocks (48 samples), take each block's max; the first block whose max >= 0.1 x the largest block max (within 20 dB of the peak); index = that block's first sample. |
| `peak` | split m into 5 ms blocks (240 samples); index = centre of the block with the largest max. |

A sample shorter than one block gives index 0.

Varispeed `glide [s0, s1]` (r0 = 2^(s0/12), r1 = 2^(s1/12)):

1. Upsample the sample 4x with soxr VHQ (float64).
2. When max(r0, r1) > 1, low-pass the upsampled signal with an 8th-order Butterworth at 0.95 x (SR/2) / max(r0, r1,
   1), run forward and backward (zero phase). This stops aliasing when the read speed exceeds 1.
3. Output length L = max(2, int(len / ((r0 + r1)/2))). The read rate goes linearly from r0 to r1 over L outputs.
4. Read positions: 0, then the running sum of the rates, times 4 (in upsampled samples), clipped to the signal.
5. Linear interpolation between neighbouring upsampled samples.

### 2.5 Slicing

1. Take the window `[start, end)`. Work on its channel mean.
2. Detect onsets with librosa's onset detector at sr 48000, hop 128, backtracking on, threshold `delta`, and a wait of
   int(min_gap x SR / 128) frames. Positions are in samples.
3. Gate: for each onset, the peak of the next 30 ms. Keep onsets whose peak >= the loudest such peak x
   10^(gate_db/20).
4. For onset k: slice start = max(0, onset - pre), end = min(window length, onset + len). With `stop_before_next`,
   end = min(end, next onset - pre). Drop a slice shorter than 20 ms. The kept slices are numbered after this drop.
5. Process each slice with the sample chain (fade-out default 0.02).
6. Apply `pick` (indices past the end are skipped), then `max`.
7. No slice left: an error naming the file.

### 2.6 Free placements, beds, music, buses

Free placement: the sample at `pitch`, time `at + offset`, start = round(t x SR) - align_index, gain, pan, bus.

Ambience bed:

1. The window = `src_start..src_end` of the decoded file. need = int((until - at) x SR).
2. If the window is shorter than need, loop it: start with the window; while too short, crossfade the last 1.5 s of
   the result with the first 1.5 s of a new copy, using equal-power curves (the square roots of the raised-cosine
   fade-out and fade-in), then append the rest of the copy. Mark the bed `looped`.
3. Cut to need samples.
4. `width`: mid = (L + R)/2, side = (L - R)/2 x width, L = mid + side, R = mid - side.
5. EQ, then the fades (defaults 1 s and 1 s).
6. `auto`: the points shifted to bed time (t - at), interpolated in dB as for `env`, applied as gain.
7. Add to the `amb` bus at round(at x SR) with `gain_db`.
8. Report `body_rms_dbfs` = RMS in dB of the bed without its fade-in and fade-out regions, plus `gain_db`.

Music:

1. ratio = target_bpm / src_bpm, or 1 when `stretch` is `none`.
2. When |ratio - 1| >= 1e-6, stretch the whole file once with ffmpeg's Rubber Band filter (2.7) and decode the
   result. k = 1/ratio converts source seconds to stretched seconds.
3. For each segment (fade-in fi, fade-out fo): cut from round(src_from x k x SR) - int(fi x SR) to round(src_to x k
   x SR) + int(fo x SR). The fade-in is a pre-roll before `src_from`, so the downbeat's own attack survives. When the
   cut start is negative, clip it to 0 and shorten the pre-roll by the same amount.
4. Fades: fade-in fi (0 when `fade_in_at_start` is false), fade-out fo. Then, when `tail_fade` is set, multiply the
   last int(tail_fade x SR) samples by a falling raised cosine as well.
5. Place it at round(at x SR) - pre-roll on the `music` bus with the segment `gain_db`. So `src_from` lands exactly on
   `at`.
6. After all segments: the music block's `eq` on the whole music bus, then its `gain_db`.

Buses: for each `buses` entry (skip keys that start with `_`; any other key that is not a bus name exits with an
error): EQ, then the `auto` curve over film time, then `gain_db`. On the music bus this runs after step 6 above.

### 2.7 Rubber Band settings

The stretch uses ffmpeg's `rubberband` filter with tempo = the ratio (6 decimals), transients mixed, detector
compound, phase laminar, window standard, smoothing off, formant shifted, pitch quality "quality", channels together;
output 48 kHz stereo float. The cache key adds the ratio and a version tag to the decode key. When ffmpeg has no
Rubber Band, the error message starts with `rubberband failed`. The fix is `stretch: none` (when the track is within
about 0.2 % of the film tempo) or an ffmpeg build with the filter.

### 2.8 Writing and mastering

1. Write the 3 stems as float WAV and their peak and RMS.
2. Sum them. Write the sum to `--out` (no master) or to `premaster.wav` next to `--out`.
3. Write the sounded cues, the report and, with `--trace`, the trace.
4. When mastering and the sum is all zeros: print a message that says nothing is mastered (and, when `--out` exists,
   that it is from an earlier render), then exit 1.
5. When mastering: run `master.py <premaster> <out> --lufs <master.lufs> --ceiling <master.ceiling> --json
   <report folder>/master.json`, plus `--no-glue` when `master.no_glue` is true (the default), `--no-comp` when
   `master.no_comp`, `--mono-hz` when `master.mono_hz` is set. Print the tail of its output. A non-zero exit prints
   `master gate FAILED` and exits 1.

The source's mixer DSP is frozen: one shipped 60 s film reproduces bit-identically from its design. hark keeps the
same principle: the DSP of `mix.py` and `master.py` changes only with a new version and a new golden render.

## 3. Behaviour and algorithms of `master.py`

### 3.1 The chain (`finish`)

| step | what | constants |
|---|---|---|
| 1 | high-pass | 2nd-order Butterworth at 28 Hz |
| 2 | mono below `mono_hz` | mid = (L + R)/2; side = (L - R)/2 through a 2nd-order Butterworth high-pass at mono_hz; L' = mid + side, R' = mid - side. 0 disables. |
| 3 | bus compressor (unless `--no-comp`) | 3.2 |
| 4 | glue (unless `--no-glue`) | y = tanh(0.9 x drive x x) / tanh(drive), drive 1.15; then y + 0.25 x (y through a 1st-order Butterworth high-pass at 2.5 kHz) |
| 5 | optional end fade (`--fade F` > 0) | gain = clip((dur - 0.03 - t)/F, 0, 1)^2 |
| 6 | loudness passes | 3.4 |
| 7 | edge fades | raised cosine (curve of 2.2 step 8) from exact zero over 3 ms at sample 0; fade onto the last sample over max(end_fade, 10 ms), end_fade default 60 ms |
| 8 | tail zero | the last `--tail-zero` samples (480 = 10 ms) set to 0 |

The mixer passes `--no-glue` by default, so a design master skips step 4 unless it sets `no_glue: false`.

### 3.2 Bus compressor

Threshold -14 dBFS, ratio 2:1, attack 15 ms, release 120 ms, detector time 5 ms, control block 48 samples (1 ms).

1. Detector: the mean of the squares of the 2 channels, through a 1st-order Butterworth low-pass with cutoff
   1/(2 pi x 0.005) Hz.
2. Take the maximum of each 48-sample block (edge-padded).
3. over = max(0, 10 log10(d) - threshold). Target gain reduction g* = -over x (1 - 1/ratio) dB.
4. One-pole smoothing per block: when the target is below the current gain (more reduction), use coefficient
   exp(-48/(0.015 x SR)), else exp(-48/(0.12 x SR)); g = g* + (g - g*) x coefficient.
5. Interpolate the block gains linearly to every sample (block centres), convert from dB, multiply.

### 3.3 True-peak lookahead limiter

Ceiling 0.77 linear (-2.27 dBFS), lookahead 4 ms, release 80 ms, control block 16 samples.

1. Upsample 4x with a polyphase resampler. Take the absolute max over channels, then the max of each group of 4
   upsampled values that belong to one original sample.
2. Peak ahead: at each sample, the max over the window [i, i + L), L = max(16, int(0.004 x SR)) = 192.
3. need = min(1, ceiling / peak).
4. Block gain = the min of `need` in each 16-sample block (edge-padded).
5. Smooth: k = max(3, L/16) = 12. Take a Hann window of k + 2 points without its 2 end points, normalised to sum 1.
   Convolve the edge-padded block gains with it, and keep the min of the original and the smoothed value.
6. Release: run over blocks. A lower value takes effect at once. A higher value is approached with coefficient
   exp(-16/(0.08 x SR)).
7. A 3-block minimum filter.
8. Interpolate to samples (block centres). The final gain per sample = min(interpolated, need). Multiply.

The limiter returns the gain per sample. Gain reduction in dB = -20 log10(gain).

### 3.4 Loudness passes

Up to 8 passes: scale the signal by 10^((target - integrated)/20), then run the limiter. Stop after at least 3
passes once |integrated - target| <= 0.05 LU.

### 3.5 Measurement

- K-weighting: the 2 standard BS.1770 biquads at 48 kHz (the high shelf, then the RLB high-pass), applied per channel.
- Block loudness for window W and hop H: L = -0.691 + 10 log10(sum over channels of the mean square of the K-weighted
  signal in the block). Block time = the block centre. Momentary: W 0.4 s, H 0.1 s. Short-term: W 3 s. A signal
  shorter than W gives one block of -70 at its centre.
- Integrated loudness: BS.1770 gated integration (absolute gate -70 LUFS, relative gate -10 LU), as pyloudnorm
  computes it. Silence gives minus infinity.
- LRA: short-term blocks (3 s, hop 0.1 s) above -70; relative gate = 10 log10(mean(10^(L/10))) - 20; keep blocks
  above it; LRA = 95th percentile - 10th percentile. Fewer than 2 blocks gives 0.
- True peak: 20 log10 of the max absolute value after 4x polyphase upsampling.
- `tail_50ms_db`: RMS dB of the last 50 ms. `last_480_zero`: the last 480 samples are exact zeros.

### 3.6 Gate

Fail when |lufs - target| > tol (default 0.5), when the true peak > max-tp (default -1.5 dBTP), or when the last 480
samples are not digital zero. In `--measure` mode the same gate runs, so a file that was not mastered by this chain
fails on the tail check.

### 3.7 Side-chain ducking (standalone use)

`--sidechain KEY.wav` ducks the input before the chain, from a key signal (for example a kick stem):

1. The key's absolute max over channels, then the max of each 48-sample block.
2. Normalise by the 99.9th percentile of the non-zero block values.
3. Envelope: instant attack; release coefficient exp(-48/(tau x SR)), tau = `--sc-tau` (0.14 s).
4. Gain = 1 - depth x clip(envelope, 0, 1), depth = `--sc-depth` (0.55), interpolated to samples.

The key is zero-padded or cut to the input length. The module also offers a time-list ducker: each hit (t, weight)
pulls the gain to 1 - depth x weight x shape, where the shape is exp(-t/tau) over 0.6 s with a 3 ms raised-cosine
attack, and overlapping hits keep the lowest gain.

## 4. Behaviour and algorithms of `qa.py`

QA needs no models. `--listen` and `--realness` add hark's listening when the hark package is importable.

### 4.1 Inputs and scale

1. Load the design (with `--only`), the report (required), the trace (when the file exists), the 3 stems (required,
   all the same length, else an input error), the master (when given or at its default path; explicit and missing is
   an input error; a length that differs from the stems by more than 1 sample is an input error), and the cue file
   (missing = no events, fps 60).
2. target = the design's `master.lufs` (default -14).
3. pre_lufs = integrated loudness of the stem sum. master_lufs = integrated loudness of the master. A signal shorter
   than 400 ms or with no gated loudness has none.
4. G = master_lufs - pre_lufs when both exist, else 0. Every level QA reports for an event is at master scale (the
   stem level plus G) when a master exists and both loudness values exist, else at stem scale. The overall summary
   says which.
5. no_bed = the music stem and the amb stem are both all zeros (a solo render).

### 4.2 Rebuilding the layers (needs the trace)

1. Take the trace records that carry a `sample` (rule and free placements) and the report's placements, in order. If
   the counts differ, every trace-based section except edges is skipped with a note that says the trace and the
   report disagree and that the mix must run again with `--trace`.
2. For each pair: a free placement takes the next `place` item of the design (its pitch, offset, align, `at` as its
   cue time, rule name `place`, its `qa_ok`). A rule placement finds its rule by name (or index text): pitch = the
   placement's rule pitch, offset = the rule's offset, align from the placement, cue time = placement t - offset,
   `qa_ok` from the rule.
3. Rebuild the sound with the same library. If its length differs from the trace `n`, count it as not rebuilt and
   skip it (a note reports the count: the design changed after the mix).
4. Keep only `sfx`-bus layers. Clip to the film. Apply the trace gain, the pan gains and the sfx bus curve (`auto`
   times `gain_db`). Record: sample, rule, kind, event, cue, offset, align, start, n, clipped span [a, b), the signal,
   `qa_ok`, attack sample (start + align_index(x, attack)) and peak dB.
5. Group layers into events. Sort by attack. Rule layers group by (event id, cue time rounded to 4 decimals). A free
   placement joins the most recent free-placement group (looking back at most 12 groups) whose first attack is within
   140 ms, else it starts a new group.
6. Per group: span, summed signal, cue = the earliest layer cue, the loudest layer, kind = the loudest layer's kind,
   label = the first layer's event id (free placements: `place:<sample>`), `long` = any layer of kind `whoosh`,
   `riser`, `swell` or `suck`, or aligned by `peak`; `builds` = any layer of kind `riser` or `swell`; `qa_ok` = the
   first layer's that has one; peak dB (+ G); `silent` = peak < -70 dBFS.

"Live" groups are the groups that are not silent.

### 4.3 Section `clicks`

Detector, per channel, on the master and each stem:

1. h = the signal through a 4th-order Butterworth high-pass at 14 kHz. e = |h|.
2. loc = sqrt(moving mean of h^2 over 960 samples, centred) + 1e-5 (20 ms). near = the same over 192 samples (4 ms).
3. Candidates: samples with e/loc > 9, e/near > 6 and e > 0.01.
4. A candidate starts a detection only if it is more than 480 samples (10 ms) after the previous candidate. Every
   candidate updates "previous", so a burst gives one detection at its first sample.
5. conc = energy of h^2 within +-24 samples / energy within +-240 samples. conc >= 0.8 is a `click` (a cut), else an
   `attack` (a designed transient).
6. Merge the channels: sort by (time, channel); drop a detection within 1 ms of the previous one in that order.

Ownership of each click:

- On the master and the sfx stem, when layers exist: search the layers that overlap the click +-480 samples. For each,
  high-pass the layer's own signal (+-480 samples around the click, at least 64 samples, on the click's channel) and
  take its max |h| within +-24 samples. The strongest layer is the owner if its value >= 0.3 x the click level
  (linear; on the master the click level is first brought back to stem scale by 10^(-G/20)).
- Otherwise: up to 3 trace records whose span +-960 samples holds the click (on a stem: only records of that bus),
  sorted with edge positions first, then by distance to the nearest edge.
- Position: `start` when within 96 samples (2 ms) of the record start, `end` when within 96 samples of its end, else
  `body`.

Item: `{in, t, ch, level_db, conc, kind, owners: [{bus, what, rule, where, from_start_ms, from_end_ms|level_db}],
at_edge}`. `at_edge` is true when the first owner's position is `start` or `end`. Status: `fail` when any master
click is at an edge, `warn` when any click exists, else `ok`. `counts` gives clicks and attacks per signal. A click
inside a recording is either its own crisp transient or a defect in the source: the summary says so and a critic
decides.

### 4.4 Section `edges`

Used samples (samples named in the placements):

1. Decode the source, take the trim, its fades (defaults 2 ms and 12 ms) and the trim's raw peak.
2. Trim peak under -60 dBFS: an issue that says the trim is empty (start >= end or past the file end), digital
   silence, or near-silent, with the file length.
3. Else process the trim with the sample chain without fades and measure:
   - head_db and tail_db: RMS of the first and the last 48 samples (1 ms) re the processed peak;
   - pre20_db and post20_db: RMS of the raw source in the 960 samples (20 ms) before and after the trim re the raw
     trim peak;
   - attack_ms: align_index(attack);
   - late_onset: over 5 ms block maxima e[k], the first k from 60 % of the blocks on with e[k] > 0.12 x peak and
     e[k] > 4 x e[k - 1] (a jump of more than 12 dB to within about 18 dB of the peak); its time in seconds.
4. Judge (defects are audible; notes are craft advice):
   - The trim starts after 0 and pre20 > -20 dB: the trim cuts into a sound already playing. A defect when fade_in <
     5 ms and (attack_ms > 10 or pre20 > -10), because no attack follows at once to cover it. Else a note that the
     fade-in softens it.
   - Otherwise, head > -30 dB: a note that the attack starts in the first ms and the fade-in dulls it, so start the
     trim 3-5 ms earlier.
   - tail > -35 and post20 > -35 dB: the sound continues after the trim. A defect ("chopped") when fade_out < 30 ms,
     else a note ("decay faded early").
   - late_onset: a note about a second hit inside the sample (fine for a rattle, wrong for a single hit).
   - A sample with `qa_ok` turns its defects into notes marked acknowledged, with the reason.

Used slice groups (any placed sample named `group#n`): slice again without fades (an error becomes an issue "cannot
slice"). For each slice measure head, tail and late onset with no source outside (post20 := tail, trimmed start =
false), with fades 2 ms and 20 ms. Collect the defect and note messages without their numbers (deduplicated). One item
per group.

Music splices (when the design has music segments): on the same stretched audio as the mixer, with w = 50 ms and
lv(i) = RMS dB of the 50 ms from sample i (clamped at 0), segments sorted by `at`:

- end_t = at + (src_to - src_from) x k. out = lv(end - w) (the last 50 ms inside). cut = lv(end) (the source just
  after `src_to`). in = lv(next segment's src_from), or -120 when there is no next segment.
- Issue "chops the music" when the segment has no `tail_fade`, fade_out < 0.1 s, end_t is more than 50 ms before the
  film end, cut > out - 6 dB and cut > in + 6 dB. With `qa_ok` it becomes an acknowledged note.
- Note a gap > 20 ms of no music before the next segment, or an overlap > 100 ms.
- Note "begins inside a sustained sound" when the segment start > w and lv(start - w) > lv(start) - 3 dB (no fresh
  attack on the splice).

Placements cut by the film ends: trace records on a bus other than music whose start < 0 or end > the film length.

Status `warn` when any item has a defect, else `ok`. Edges never fail.

### 4.5 Section `audibility`

1/3-octave bands: 22 centres fc = 10^(n/10) for n = 20..41 (100 Hz to 12.6 kHz), band edges fc x 2^(-1/6) and fc x
2^(1/6). Band energy of a window: Tukey window (alpha 0.25) per channel, power spectrum summed over channels, the sum
of bins in [lo, hi), plus 1e-30.

Per group (effect signal E over its span [a, b), attack att):

1. bed = music + amb stems over the span. rest = sfx stem - E + bed (everything else).
2. Window search: candidate starts every 48 samples; pick the one where E has the most energy.
   - 60 ms window, starts from att - 5 ms to att + 300 ms (a `long` group: to the span end - 60 ms).
   - 20 ms window, starts from att - 3 ms to att + 80 ms. A `long` group reuses the 60 ms start.
   Windows that run past the span are zero-padded.
3. Margin of E against X: over bands where E's energy >= 1e-3 x its strongest band (within 30 dB), the margin is
   10 log10(E/X) clipped to +-60 dB. Take the best band: (margin, centre Hz).
4. vs_bed = margin of E (60 ms) against the bed. vs_all = margin of E (60 ms) against rest. first20 = margin of E
   (20 ms) against rest (20 ms).
5. read_db = max(vs_all, first20). Verdict: `clear` >= 6, `ok` 3-6, `barely` 0-3, `masked` < 0.
   `covered_by` = `music/amb` when vs_bed < 3, else `other effects`.

Rows: `{t, label, kind, n_layers, rules, peak_db, vs_bed_db, vs_bed_hz, vs_all_db, first20_db, first20_hz, read_db,
verdict, covered_by, qa_ok?}`. A silent group gets verdict `silent` and its sample names. With no bed every non-silent
group is `not judged`.

Status: `fail` when any silent group lacks `qa_ok`. With no bed, otherwise `skipped`. Else `warn` when any masked
group lacks `qa_ok`, else `ok`. `items` = silent groups, then judged groups sorted by read_db ascending (at most 40).
`events` = all rows.

`--layers`: for each live group with 2 or more layers, judge each layer alone (its "rest" includes the other layers
of the same event). A layer whose peak + G < -70 dBFS, or whose read_db < 0, is a dead layer:
`dead_layers: [{t, rule, sample, read_db, silent?}]`.

### 4.6 Section `harshness`

1. The music stem's channel mean through a 4th-order Butterworth band-pass 2-6 kHz. Its 5 ms (240 samples) moving
   mean power, centred. No music when the max is <= 1e-10. ref = 10 log10(that max) + G.
2. Per live group (mono mean of its signal): band-pass 2-6 kHz and 80-400 Hz, each with 10 ms of zeros before the
   signal (then removed) so the filter settles. Window from 10 ms before the attack to 80 ms after it (a `long` group:
   to the end). Skip a window under 5 ms.
3. pres_db = 10 log10 of the max 5 ms mean power of the 2-6 kHz band in the window, + G. The same for the low band.
4. Row: `{t, label, rules, pres_db, pres_minus_low_db, centroid_hz, music_pres_db, over_music_max_db, issues,
   bright}`. centroid = power-weighted mean frequency of the window (Hann). music_pres_db = mean 2-6 kHz music power
   within +-250 ms of the attack, + G. over = pres_db - ref.
5. Issue when over > 6 dB. `bright` when centroid > 6500 Hz. `qa_ok` clears the issues.

Status `warn` when any issue, else `ok`. `items` = up to 20 hot rows, then other rows up to 10 in total, sorted by
pres_db descending. `bright` lists the bright events. Without music the presence check is not judged.

### 4.7 Section `sync`

Per live group, x = max absolute value over channels of its signal, in 5 ms blocks:

1. The test: a group that `builds` uses `apex` when its loudest layer aligns by `peak`, else `end`. A group whose
   loudest layer aligns by `start` or `end` and does not build is not judged. A `long` group uses `apex`. All others
   use `impact`.
2. Measurements in ms re the cue: attack = the loudest layer's attack; peak = centre of the loudest 5 ms block;
   apex = the max of the 50 ms (2400 samples) moving mean of the summed channel power; end (end test only) = the last
   sample above 0.1 x max(x) (-20 dB).
3. Verdict for a shift s (ms, 0 at first):
   - `end`: an issue when end - s is outside -300..+60 ms. The norm is -250..+30; the check is looser.
   - `apex`: an issue when |apex - s| > 45 ms.
   - `impact`: an issue "sound before picture" when attack - s < -45 ms, else "lands late" when peak - s > 75 ms.
4. When there is an issue: for each layer whose |offset| >= 30 ms, try s = that offset. The first that removes the
   issue makes the row `deliberate`, with `offset_s`, `offset_rule` and an explanation. Else a group `qa_ok` makes it
   deliberate. Else it is an issue row, with `offsets_tried_s` when any offset was tried.

Row: `{t_cue, label, kind, test, align, attack_ms, peak_ms, apex_ms, end_ms?, long, issue}`. Status `warn` when any
issue row, else `ok`. The summary says that the deliberate rows must still be checked on the frames.

### 4.8 Section `density`

- `events_per_s`: count of live groups per whole film second (by first attack).
- `busiest_s`: the 5 busiest seconds. `most_used_samples`: the 8 most placed samples.
- Per rule with 4 or more rule placements: `{rule, n, distinct_samples, ioi_s (mean inter-onset), ioi_cv (sd/mean),
  gain_sd_db, pitch_sd_st}`.
- Machine gun: distinct_samples = 1, pitch_sd_st < 0.1 and gain_sd_db < 0.5. Status `warn` when any, else `ok`.
- The summary gives the event count, the events per 10 s and the busiest seconds.

### 4.9 Section `loudness` (needs the master)

No master: `skipped`. Shorter than 400 ms: `skipped`. All zeros: `fail`.

Fields: `lufs`, `target`, `true_peak_dbtp`, `lra_lu`, `premaster_lufs`, `loudest_momentary`, `loudest_at`,
`loudest_frac` (time / duration), `short_term_per_s` (3 s blocks, hop 1 s), `head_5ms_peak_dbfs` (first 240
samples), `tail_50ms_rms_dbfs` (last 2400), `tail_10ms_peak_dbfs` (last 480), and per act of the cue file
`{from, momentary_median, momentary_max}` over the momentary blocks whose centre is inside the act.

| condition | result |
|---|---|
| \|lufs - target\| > 0.5 | fail |
| true peak > -1.0 dBTP | fail (AAC would clip) |
| tail 50 ms RMS > -45 dBFS or tail 10 ms peak > -60 dBFS | fail (the end is not silent) |
| head 5 ms peak > -40 dBFS | warn (starts mid-sound or clicks on sample 0) |
| LRA < 4 LU | warn (flat) |
| LRA > 10 LU | warn (quiet passages vanish on phones) |
| loudest_frac > 0.85 | warn (the peak belongs on the drop) |

### 4.10 Section `coverage`

1. No sound = the report's `unsounded_events` plus each silent rule group without `qa_ok` (placed but silent).
2. Class of an event = (kind, id without its trailing `:<digits>` groups). `demo:type:key:3` belongs to class
   `demo:type:key`.
3. `partial`: classes where some but not all events have no sound. `unsounded_classes`: classes where all have none.
4. Status `warn` when any partial class or any placed-but-silent event, else `ok`. A class never sounded is listed but
   does not warn (it can be a choice). Partial coverage reads as a mistake to a viewer. With `--only` the summary says
   that events of other fragments are expected to have no sound.

### 4.11 `qa_ok`

A sample, a rule, a free placement or a music segment may carry `"qa_ok": "<reason>"`. The mixer ignores it. QA then
reports that item's edge, sync, audibility or harshness finding as acknowledged instead of a warning. It is for
decisions a critic has already confirmed (a riser choked 25 ms before a pop on purpose, a chime that rings past its
cue, a hard music cut into a quiet breakdown).

### 4.12 Section `listen` (`--listen`, `--realness`)

hark not importable: `skipped`, with the install hint. Otherwise, with the output folder = the folder of `--out`:

- `--listen` with a master: hark timeline of the master (video and the sounded-cue file when they exist) with its
  text saved as `qa_timeline.txt`; hark sheet to `qa_sheet.png`, reusing the timeline result; hark critique of the
  master with the AudioSet tagger and realness off (fast). The section keeps each summary, the critique's verdict,
  score, actions and the metrics whose status is `low` or `high`.
- `--realness`: hark realness of each file in `files_used`: `{file, p_real, verdict}`, sorted by p_real ascending,
  unknown last. `not_real` = p_real < 0.5, or, without p_real, a verdict that contains "synth".
- A failing hark call becomes a note and does not stop QA. A timeline summary over 400 characters is shortened to 360
  in the digest with a pointer to the text file.
- Status `warn` when any recording is not real or any call failed, else `ok`.

### 4.13 Section `mix` and the overall result

`mix` appears only when every stem is all zeros: `fail`, with advice to check trims (`--dump`), gains and cue matches.

Without the trace, edges, audibility, harshness, sync and density are `skipped` with a message to run the mix with
`--trace`. Clicks, loudness and coverage always run.

Overall status = the worst section status in the order fail, warn, ok, skipped. The summary names the failing and
warning sections, the sections not run, and the scale (master gain over the stems in dB, or stem scale).

## 5. Constants

| constant | value | where |
|---|---|---|
| sample rate | 48000 Hz | all |
| default fades, sample | 2 ms in, 12 ms out | 2.2 |
| default fades, slice | 2 ms in, 20 ms out | 2.5 |
| slice detector | hop 128, delta 0.07, min_gap 0.06 s, gate -18 dB over first 30 ms, pre 4 ms, len 0.16 s, min slice 20 ms | 2.5 |
| attack point | first 1 ms block within 20 dB (x 0.1) of the peak block | 2.4 |
| peak point | centre of the loudest 5 ms block | 2.4 |
| end point | last sample above x 0.003 of the peak | 2.4 |
| ambience loop crossfade | 1.5 s equal power | 2.6 |
| ambience fades | 1 s, 1 s | 2.6 |
| music fades | 8 ms pre-roll in, 60 ms out | 2.6 |
| target BPM default | 120 | 2.6 |
| master defaults | -14 LUFS, ceiling 0.77, no_glue true, comp on, mono below 120 Hz | 1.2 |
| master high-pass | 28 Hz, 2nd order | 3.1 |
| compressor | -14 dBFS, 2:1, 5 ms detector, 15 ms attack, 120 ms release, 1 ms blocks | 3.2 |
| glue | drive 1.15, air 0.25 above 2.5 kHz | 3.1 |
| limiter | 4x oversampled, 4 ms lookahead, 80 ms release, 16-sample blocks, ceiling 0.77 | 3.3 |
| loudness passes | min 3, max 8, tolerance 0.05 LU | 3.4 |
| edge fades | 3 ms head, tail max(60 ms, 10 ms), 480 zero samples | 3.1 |
| gate | +-0.5 LU, true peak <= -1.5 dBTP, last 480 zero | 3.6 |
| side-chain | depth 0.55, tau 0.14 s, 3 ms attack, 0.6 s shape | 3.7 |
| click | > 14 kHz, 9x the 20 ms RMS, 6x the 4 ms RMS, > 0.01, conc >= 0.8 in +-0.5 ms of +-5 ms, 10 ms debounce | 4.3 |
| click at an edge | within 2 ms of a placement start or end | 4.3 |
| abrupt entry | pre20 > -20 dB, fade-in < 5 ms, attack > 10 ms or pre20 > -10 dB | 4.4 |
| chopped decay | tail > -35 dB, post20 > -35 dB, fade-out < 30 ms | 4.4 |
| head note | head > -30 dB | 4.4 |
| second hit | 5 ms block x4 jump to > 0.12 x peak in the last 40 % | 4.4 |
| chopped music | cut within 6 dB of out, 6 dB over what enters, fade-out < 100 ms, no tail_fade | 4.4 |
| splice notes | gap > 20 ms, overlap > 100 ms, sustained start within 3 dB | 4.4 |
| silent effect | peak < -70 dBFS | 4.2 |
| margin cap | +-60 dB | 4.5 |
| audibility | 60 ms and 20 ms windows; bands within 30 dB; clear >= 6, ok >= 3, barely >= 0 | 4.5 |
| harshness | 2-6 kHz, 5 ms RMS, > 6 dB over the music's loudest; bright > 6.5 kHz | 4.6 |
| sync | impact attack >= -45 ms, peak <= +75 ms; apex +-45 ms; end -300..+60 checked (norm -250..+30); offsets >= 30 ms are tried | 4.7 |
| machine gun | >= 4 events, 1 sample, pitch sd < 0.1 st, gain sd < 0.5 dB | 4.8 |
| loudness | +-0.5 LU fail, true peak > -1.0 fail, tail -45 RMS / -60 peak fail, head -40 warn, LRA 4-10 warn, loudest after 85 % warn | 4.9 |
| free-placement grouping | 140 ms, last 12 groups | 4.2 |

## 6. Errors and edge cases

| case | behaviour |
|---|---|
| design file missing | `mix.py` exits with the absolute path; `qa.py` exits 2. |
| a name defined twice across fragments | error naming the fragment, section and name. |
| pool names an unknown sample | error `pool <P>: unknown sample <S>`. A slice group needs `#*`. |
| pool empty, or slices with no onset | error. Fix: widen the window, lower `delta` (0.05) or `gate_db` (-24). |
| `reverse` on a sample | error. Use a recording that already has the shape. |
| comment key inside `buses` | skipped when it starts with `_`; any other unknown bus key exits. |
| missing recordings | exit, listing every missing file. |
| no cue file with rules | exit, telling to pass `--cues`. |
| no duration and no cue total | exit, asking for one. |
| ffmpeg without Rubber Band | `rubberband failed`. |
| mix all silent while mastering | message, exit 1, the old `--out` is not touched. |
| master gate fails | `master gate FAILED`, exit 1. Read `master.json`. Lower `master.lufs` by 1 for a dynamic track, or ride the loudest peak down. |
| unnamed rule | solo and full mix differ in variation. |
| solo has no premaster | `--only` writes the unmastered sum to `--out` itself. |
| `stretch: none` with a tempo off by up to 0.2 % | accepted. The drift inside one segment stays under 0.2 %. |
| ambience window shorter than the loop crossfade (1.5 s) and too short for the bed | not handled by the source (the crossfade cannot fit). hark must reject it with an error that names the bed. |
| placement before 0 or after the end | the outside part is dropped. QA lists it under `cut_by_film_ends`. |
| first run is slow | every recording decodes once; later runs read the cache. |
| QA: trace from a different run or `--only` | trace-based sections skipped with the disagreement note. |
| QA: design changed after the mix | placements that cannot be rebuilt are counted in a note. |
| QA: solo render (no bed) | audibility `skipped` unless a silent effect exists; harshness has no music reference. |
| QA: stems of different lengths, master of another length | exit 2. |
| `master.py --measure` on a file the chain did not master | fails the gate on `last_480_zero`. Use hark measure for a delivered MP4. |

## 7. Acceptance tests

Fixtures are generated in the test (a tone burst, a noise burst or a click made in code is allowed in TESTS; the
"recordings only" rule applies to films, not to test fixtures).

| # | input | expected properties |
|---|---|---|
| M1 | a design with 1 sample (a 100 ms burst), 1 rule `align: attack` on 1 event at f 60, fps 60, duration 2 s, `--no-master` | the sfx stem has its first sample above -20 dB re peak within 1 ms of sample 48000; amb and music stems are zeros; the stems sum to the output exactly; report has 1 placement with t 1.0 and 0 unsounded. |
| M2 | the same design rendered twice | the outputs are bit-identical. |
| M3 | 2 fragments, rules named, `--only` one of them | the solo placements of that fragment equal the full mix placements of that fragment (sample, gain_db, pitch, pan, t). |
| M4 | 2 fragments that both define `x.a` | error naming the second fragment and `x.a`; no output written. |
| M5 | rule `mode: cycle` over a pool of 3 on 7 events | samples played are pool[0], [1], [2], [0], [1], [2], [0]; never the same twice in a row. |
| M6 | `pitch: [1, 2]`, `pitch_ramp: [0, 1]` on 5 events | every placement pitch is in [1, 3]; the ramp part grows from 0 to 1 in time order; a second render gives the same pitches. |
| M7 | sample with `pitch: 12` | the processed sample is half as long (+-1 sample) and its dominant frequency doubles. |
| M8 | ambience bed of 10 s from a 4 s window | `looped: true`; length 10 s; no sample-to-sample step larger than the source's own largest step x 1.5 at the crossfades. |
| M9 | music `stretch: none`, 1 segment src_from 2.0 at 5.0 on a track with a click at 2.0 s | the music stem's click sits at 5.0 s +-1 sample; the 8 ms before it carry the pre-roll fade. |
| M10 | `master.py` on a 10 s pink-noise-like test signal at -30 LUFS | output integrated within -14 +-0.5 LUFS; true peak <= -1.5 dBTP; last 480 samples zero; first sample 0; exit 0. |
| M11 | `master.py --measure` on a file with a non-zero tail | exit 1 with a fail line about the last 480 samples. |
| Q1 | a render whose sfx sample starts at a non-zero sample value (no fade-in) on a quiet bed, with master | QA clicks: at least 1 master click with `at_edge` true and owner `where: start`; status fail; exit 1. |
| Q2 | the same with `fade_in: 0.004` | no click at an edge; clicks status not fail. |
| Q3 | an effect 10 dB under a broadband music bed in all bands | audibility verdict `masked`, `covered_by` `music/amb`, status warn. |
| Q4 | a rule with offset 0.2 s on an impact | sync lists it as deliberate with `offset_rule` = that rule; no issue row. |
| Q5 | a rule that plays 1 sample on 6 events with no jitter or pitch | density lists it under `machine_gun`; status warn. |
| Q6 | cue events `a:k:0..3`, a rule sounding only `a:k:0` and `a:k:1` | coverage `partial` has class `a:k` with 2 of 4 unsounded; status warn. |
| Q7 | qa.py without a trace file | edges, audibility, harshness, sync, density are `skipped`; clicks, loudness, coverage ran. |
| Q8 | qa.py with stems missing | exit 2, one line naming `--stems`. |
| Q9 | a sample trimmed past the end of its file | edges issue says the trim is empty; audibility marks the event `silent`; status fail. |
| Q10 | a music segment that ends inside a held chord and splices into silence, fade_out 0.06, no tail_fade | edges splice issue "chops the music"; with `qa_ok` it becomes an acknowledged note. |
