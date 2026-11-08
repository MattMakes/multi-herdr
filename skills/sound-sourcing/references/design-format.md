# The design format

`mix.py` renders a JSON design. The top level is `audio/sounddesign.json`. Each category is a fragment in
`audio/design/<category>.json`. A fragment is also a valid single-file design. Keys that start with `_` are comments.
Unknown keys are ignored, except inside `buses`. Every relative path resolves against `--root`.

## The cue file (`out/cues.json`)

A JSON object, or a list whose first item is that object.

| key | meaning |
|---|---|
| `fps` | frames per second (default 60) |
| `total` | film length in frames |
| `bpm` | the picture grid tempo (for people only) |
| `events` | the cue events |
| `acts` | `name -> {from, dur}` in frames; QA reports loudness per act |
| `cue` | `name -> frame`, named moments (for people only) |

Event fields: `f` (required, frame, fractions allowed), `kind` (required: `tick`, `tock`, `key`, `click`, `pop`,
`snap`, `land`, `hit`, `drop`, `whoosh`, `riser`, `swell`, `suck`, `bell` or your own word), `id` (`act:thing:n`,
rules match it with a regex), `weight` (0..2, 1 normal), `pan` (-1..1, screen x), `variant` (`key`: `word`, `space`,
`return`, `back`; `land`: `light`; `click`: `confirm`), `dur` (frames, for whoosh, riser, swell), `apexFrac` (0..1,
where a whoosh peaks), `pitch` (a hint, not read).

The sync point is the frame where the viewer sees the cause: the contact; a pop-in at 50 % of its travel; the fastest
frame of a move; the hit a riser builds to; the first frame the wordmark is legible. Take frames from the picture
code. Hand-typed sync points land 8-11 frames off.

## Top level

| key | default | meaning |
|---|---|---|
| `duration` | cue `total / fps` | film length in seconds |
| `cues` | `out/cues.json` | default cue file; `--cues` wins |
| `include` | none | fragment paths |
| `samples`, `slices`, `pools`, `rules`, `place`, `ambience`, `music` | none | allowed here too |
| `buses`, `master` | none | read from the top-level file only |

Name every item with the fragment prefix (`ui.`, `kb.`, `mo.`, `im.`, `am.`, `mu.`). A name defined twice is an error.

How includes merge: `samples`, `slices` and `pools` merge by name. `rules`, `place` and `ambience` concatenate, each
fragment's list going in front, so the last include's rules come first; that changes report order, not sound.
`music` comes from the top level, else from the first fragment that has one. `--only TEXT` keeps only the top-level
includes whose path contains TEXT.

## `samples`: name -> a trimmed piece of 1 recording

| key | default | meaning |
|---|---|---|
| `file` | required | any file ffmpeg reads |
| `start`, `end` | 0, file end | the trim, source seconds |
| `mono` | false | the mean of both channels, on both |
| `eq` | none | the EQ object below |
| `pitch` | 0 | semitones by resampling; pitch and length change together |
| `glide` | none | `[from, to]` semitones, a varispeed glide across the sample |
| `env` | none | `[[t, dB], ...]`, a linear-in-dB gain curve from the sample start |
| `norm_peak_db` | none | scale the processed sample so its peak is this dBFS |
| `fade_in`, `fade_out` | 0.002, 0.012 | raised-cosine fades, seconds |
| `gain_db` | 0 | static gain, applied last |
| `reverse` | | refused: keep a recording in its real direction |
| `qa_ok` | | a reason: QA lists this sample's edge finding as acknowledged |

Processing order: trim, mono, EQ (high-pass, low-pass, each peak, low shelf, high shelf), pitch, glide, env,
`norm_peak_db`, fades, gain. Because `norm_peak_db` runs after the EQ, a cut on a band that does not hold the peak can
make that band louder. Lower `norm_peak_db` together with the cut.

## `slices`: name -> 1 recording of many hits cut into single hits

The slices are named `name#0`, `name#1`, ... in the order kept.

| key | default | meaning |
|---|---|---|
| `file`, `start`, `end` | | the window to search |
| `delta` | 0.07 | onset threshold; lower finds softer hits |
| `min_gap` | 0.06 | minimum seconds between onsets |
| `gate_db` | -18 | drop onsets whose first-30 ms peak is this far under the loudest |
| `pre` | 0.004 | seconds kept before each onset |
| `len` | 0.16 | maximum slice length |
| `stop_before_next` | true | end each slice `pre` before the next onset |
| `pick` | all | slice indices to keep, in that order |
| `max` | all | keep at most this many, after `pick` |
| sample keys | | `mono`, `eq`, `pitch`, `glide`, `env`, `norm_peak_db`, `gain_db`, `fade_in` (0.002), `fade_out` (0.02) |

Slices shorter than 20 ms are dropped. No slice left is an error: widen the window, lower `delta` (0.05) or `gate_db`
(-24).

## `pools`: name -> [sample names]

`"group#*"` expands to every slice of `group`, by index. An unknown name or an empty pool is an error.

## `rules`: cue-driven placements

Each event a rule matches gets 1 sound from the rule's pool. Several rules on 1 event layer it.

| key | default | meaning |
|---|---|---|
| `name` | the rule index | the key of all variation; renaming a rule changes its variation |
| `match` | all events | `kind` (regex, full match), `id` (regex, search), `from`/`to` (film seconds, `[from, to)`), `variant` (exact) |
| `pool` | required | the pool to play |
| `mode` | `cycle` | `cycle` (in order, never the same item twice in a row), `hash` (per event), `fixed` (the first item) |
| `cycle_start` | 0 | first index for `cycle` |
| `gain_db` | 0 | level |
| `weight_db`, `weight_ref` | 0, 1.0 | dB per doubling of the event weight |
| `jitter_db` | 0 | +- a hashed level per event |
| `pitch` | `[0, 0]` | `[lo, hi]` hashed semitones per event; adds to the sample's own pitch |
| `pitch_ramp`, `gain_ramp` | none | `[a, b]` added linearly across the matched events in time order |
| `pan` | `"cue"` | `"cue"` = event pan x `pan_scale`, or a number -1..1 |
| `pan_scale` | 1.0 | |
| `align` | `attack` | the point placed on the cue: `attack`, `peak`, `end`, `start` |
| `offset` | 0 | seconds added to the cue time |
| `bus` | `sfx` | `sfx`, `amb` or `music` |
| `no_cue` | false | this placement does not mark its event as sounded |
| `qa_ok` | | a reason, see below |

Variation is a hash of the rule name, the event id and its frame. A solo render therefore plays the same sounds as the
full mix, as long as every rule has a `name`.

Align points: `attack` = the first 1 ms block within 20 dB of the loudest block; `peak` = the centre of the loudest
5 ms block; `end` = the last sample above 0.003 x the peak (about -50 dB); `start` = sample 0.

## `place`: free placements

`{sample, at, gain_db? (0), pan? (0), align? (attack), offset? (0), pitch? (0), bus? (sfx), qa_ok?}`. `at` is film
seconds. `sample` is any sample or slice (`group#3`).

## `ambience`: beds on the `amb` bus

| key | default | meaning |
|---|---|---|
| `name` | | label |
| `file` | required | the recording |
| `src_start`, `src_end` | 0, end | the clean window |
| `at`, `until` | required | film seconds |
| `fade_in`, `fade_out` | 1.0, 1.0 | seconds |
| `gain_db` | 0 | |
| `eq` | none | |
| `width` | none | side scale: 0 mono, 1 as recorded, > 1 wider |
| `auto` | none | `[[film t, dB], ...]`, a ride inside the bed |

A window shorter than the bed loops with a 1.5 s equal-power crossfade. A window too short for that crossfade is an
error that names the bed.

## `music`: 1 object

| key | default | meaning |
|---|---|---|
| `file` | required | the track |
| `src_bpm` | required | the track's measured tempo |
| `target_bpm` | 120 | the picture tempo |
| `stretch` | `rubberband` | `none` plays the source at its own speed; otherwise the whole file is stretched by `target_bpm / src_bpm` |
| `gain_db` | 0 | music level |
| `eq` | none | EQ on the whole music bus |
| `segments` | required | the edit |

Segment: `src_from`, `src_to` (source seconds before any stretch, on downbeats), `at` (film second where `src_from`
lands), `fade_in` (0.008, a pre-roll before `src_from` so the downbeat attack survives), `fade_out` (0.06),
`fade_in_at_start` (true), `tail_fade` (seconds of a long fade over the end), `gain_db` (0), `label` (bars and
reason), `qa_ok`. `stretch: none` is fine when the tempo is within about 0.2 %.

## `buses` and `master`

`buses: {sfx|amb|music: {gain_db?, eq?, auto?}}`, `auto` = `[[film t, dB], ...]`. Any other key that does not start
with `_` is an error. Each bus runs EQ, then `auto`, then `gain_db`.

`master: {lufs (-14), ceiling (0.77), no_glue (true), no_comp (false), mono_hz (120)}`.

## The EQ object

`{hp, hp_order (2), lp, lp_order (2), peq: [[f0, q, gain_db], ...], lowshelf: [f0, gain_db], highshelf: [f0, gain_db]}`.
`hp` and `lp` are Butterworth. `peq` and the shelves are cookbook biquads (shelf Q 0.707).

## `qa_ok`

A sample, rule, free placement or music segment may carry `"qa_ok": "<reason>"`. The mixer ignores it. QA reports that
item's edge, sync, audibility or harshness finding as acknowledged. Use it only for a decision a critic confirmed: a
chime left ringing past its cue, a deliberate hard music cut into a soft breakdown, a riser stopped 25 ms before a pop.

## `mix.py` outputs

| output | content |
|---|---|
| `--out` (`out/audio/soundtrack.wav`) | the master (24-bit), or the unmastered float sum with `--no-master` / `--only` |
| `premaster.wav` | the unmastered sum, next to `--out`, when mastering |
| `--stems` (`out/audio/stems/`) | `sfx.wav`, `amb.wav`, `music.wav`; they sum to the premaster |
| `--report` (`out/qa/mix_report.json`) | `placements`, `unsounded_events`, `buses`, `ambience`, `music_segments`, `files_used`, `premaster_peak_dbfs` |
| `master.json` | the master report, next to the report |
| `--cues-out` (`out/qa/cues_sounded.json`) | the events that got an sfx sound; risers and swells get `"ends": true` |
| `--trace` | 1 record per bus add: `{bus, start, n, gain, pan, ...}` |
| `--dump DIR` | every sample and slice as WAV plus `_samples.json` (attack_ms, peak_ms, head_db, tail_db), then exit |

Solo renders (`--only ui --out out/audio-review/ui/solo.wav`) write `solo_report.json`, `solo_stems/` and
`solo_cues.json` beside the WAV. `files_used` is the input of the rights audit and the credits.

The same design renders bit-identically. The mixer DSP changes only with a new version.
