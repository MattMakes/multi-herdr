# bank-kit-sfx, part 2: `hark sfx` (list, render, check) and the knob contract

Part of the spec `docs/specs/hark/bank-kit-sfx.md`. Read the index first for the
exact CLI flags, the errors and the acceptance tests. This part holds the
behaviour, the algorithms and the constants of `sfx list`, `sfx render`,
`sfx check`, and the observable contract of the knobs (`dur`, `apex`, `pitch`)
that `sfx render` and kits use. The forge-core spec owns the knob engine; this
part says what a caller sees.

## 1. Purpose

`hark sfx` renders one bank sound by id and aims it at the picture: a length,
a time for its loudest moment, a note. It also audits any sound effect file
with measurements only. No ML model loads. The audit is the same check that
every bank render and every kit take runs.

## 2. `sfx list`

### 2.1 Rows

`list_sounds(category=None, kind=None)` returns one row per bank entry, sorted
by id. A row:

| key | from |
|---|---|
| `id` | the id |
| `category` | the entry |
| `member` | the entry |
| `type` | `forge` (default) or `fit` |
| `dur_s`, `peak_s`, `onset_s`, `end_s`, `centroid_hz`, `attack_ms`, `lufs_m_max` | the entry's stored `meta` (null when absent) |
| `kinds` | the entry's own `kinds`, else the category's kinds, else `[]` |
| `knobs` | the knob description of the entry (4.1) |
| `notes` | the entry's `notes`, else the category's `use`, else `""` |
| `source` | fit entries only: the source record |

Filters:

- `category`: keep rows whose category equals it exactly.
- `kind`: keep rows where some kind equals it, or where the part of a kind
  before its first `.` equals it. So `whoosh` keeps `whoosh.short` and
  `whoosh.long`, and `whoosh.long` keeps only `whoosh.long`.

### 2.2 Result

`sfx_list(category, kind)` returns
`{summary, version, sounds}`. `summary` is `<N> sounds in the bank <VERSION>`
plus ` (category C kind K)` naming the filters that were given. `version` is
the bank VERSION. `sounds` is the row list.

### 2.3 Text output

Without `--json` the command prints:

1. The summary line.
2. A header: `id` left-aligned in 30 columns, `dur s` and `peak s` right-aligned
   in 6 columns each, then `kinds / knobs`.
3. One line per row: the id in 30 columns, `dur_s` and `peak_s` as `%6.3f` (0
   when null), two spaces, the kinds joined by `,` (or `-` when empty), ` | `,
   then the knob words joined by `, `: always `dur <method>` (shape, param or
   trim), then `apex` when the apex knob applies, then `pitch <note>` when the
   pitch knob applies (the native note name; for a pitch-class sound the
   pitch-class name).

Exit code 0.

## 3. The audit: `sfx check`

### 3.1 Contract

`check(x, sr=48000, category=None, role=None, sound=None)` audits one sound.
`x` is a sample array (`(n,)` or `(n, ch)`, float) at `sr`, or a file path. A
path loads through the core-io audio loader at 48 kHz. An array at another
rate is resampled to 48 kHz first. `category` (a bank category), `role` (a kit
role) and `sound` (a display name) are context.

It returns:

| key | meaning |
|---|---|
| `ok` | true when there is no warning |
| `warnings` | list of `{code, msg}`: faults |
| `notes` | list of `{code, msg}`: information, never faults |
| `summary` | one line (3.5) |
| `measures` | the raw numbers (3.4) |
| `glide` | the rising-glide result (3.6) |
| `layers` | the attack-layer result (3.7) |
| `context` | the given non-empty values of category, role and sound |

It measures. It never listens. It loads no model.

### 3.2 Context sets

| set | members | effect |
|---|---|---|
| designed | `glitch`, `pro_glitch` | clicks and inner digital silence are notes, not warnings |
| gated | `glitch`, `pro_glitch`, `riser`, `pro_riser`, `pro_zip`, `pro_impact` | a truncated tail is a note: the stop is the design |
| hit roles | `hit`, `land` | stacked attacks and a pre-swell are warnings |
| hit categories | `impact`, `pro_impact`, `logo_sting`, `pro_logo_sting` | without a hit role, stacked attacks and a pre-swell are notes |

A sound also counts as gated when it ends on its loudest moment: the index of
its largest absolute mono sample is at 97 % of its length or later (a riser, a
reverse swell).

### 3.3 The rules, in order

Each rule adds at most one item. "Warning" makes `ok` false. "Note" does not.

1. `silent`: the sample peak is under -60 dBFS, or the sound stays within
   40 dB of its loudest moment for less than 5 ms in total (3.8). Always a
   warning. The message says "digital silence" when the peak is exactly 0,
   else it gives the peak in dBFS and the live time in ms. (Of all bank
   sounds, `pro_hover_tick/air` has the least live time: 9 ms.)
2. `clicks`: the forge-core click detector found isolated discontinuities
   inside the sound. The message gives the count and up to 4 click times
   (3 decimals). A note for the designed set, else a warning.
3. `edge`: the first or last sample is more than -40 dB re the peak (it
   clicks when played). Always a warning. The message gives the level.
4. `clipping`: there are flat tops at the peak (runs of 3 or more equal samples
   at the peak, per channel) or samples beyond full scale. Always a warning.
   The message gives both counts.
5. `dc`: the absolute mean is more than -40 dB re the peak. Always a warning.
6. `ring`: the forge-core ring detector's z-score of a metallic comb in the
   tail is over 15. Always a warning. The message gives the comb delay in ms
   (1 decimal) and the z.
7. `truncated`: the forge-core truncation detector reports a cut level above
   -45 dB re the peak. A note when the sound is gated (3.2), with "(the stop is
   the design)", else a warning that says to fade it out or render it longer.
8. `silence`: more than 4 ms of digital silence inside the sound (runs of
   |x| under 1e-6 of at least 2 ms, between the first and last sample above
   -60 dB re peak). A note for the designed set, else a warning.
9. `tail`: after the peak, the level climbs back up by over 6 dB (the forge-core tail
   re-rise measure). Always a note: a second event, a roll, or a double member.
10. `rising_glide`: the glide detector (3.6) reports a climb of more than
    4 semitones that lasts at least 50 ms (`to_s - from_s >= 0.05`). A
    warning, with the message "a rising synth whip reads as cheap", the
    climb in semitones, its length in ms and its span in seconds. Exception:
    when the category is `drip` and the climb is one a bubble makes (3.9), it
    is a note with the message "a bubble rising (drip allowance)".
11. `stacked` and `pre_swell`: only when the role is a hit role or the category
    is a hit category.
    - `stacked`: the layer detector (3.7) counts 2 or more separate attacks. The
      message lists the attack times (2 decimals).
    - `pre_swell`: the build before the hit is more than 100 ms. The message
      gives the ms.
    - Both are warnings for a hit role and notes otherwise.

A twelfth code, `pitch_off`, is added after the audit by a render or a kit
take when the pitch knob missed its note (4.4).

### 3.4 `measures`

`measures` holds these forge-core polish numbers, as measured (null when the
detector does not judge the sound): `clicks`, `edge_db`, `clip_runs`,
`overs`, `dc_db`, `ring_z`, `trunc_db`, `silence_ms`, `bump_db`,
`attack_ms`, `t40_ms`, `crest_db`, `res_db`, `res_hz`. Plus:

- `peak_dbfs`: the sample peak in dBFS, 2 decimals, null for digital silence.
- `live_ms`: the live time (3.8), 2 decimals.

### 3.5 Summary line

- With warnings: `<name>: <N> warning(s): <msg>; <msg>` ("warning" plural when
  N is not 1).
- Without: `<name>: clean`, plus ` (<note msg>; <note msg>)` when there are
  notes.
- `<name>` is `sound` when given, else the base name of the path, else `sound`.

### 3.6 Rising-glide detector

Purpose: find the cheap "synth whip": a tonal sound whose pitch climbs. A
noise band is not tonal and is not tracked. Returns
`{tonal_share, rise_st, from_s, to_s}`.

Steps:

1. Mono mix. When the signal is shorter than 1024 samples, zero-pad it to 1024.
2. Frames: 1024 samples (21.3 ms) every 240 samples (5 ms). Hann window. FFT
   zero-padded to 8192 points. Power spectrum.
3. Keep the bins from 80 Hz to 10 kHz. Per frame, total power `T` in that band.
4. A frame is active when its `T` is within 30 dB of the largest `T`.
5. Per frame, `fp` = the frequency of the strongest bin in the band.
6. Half-width `h = max(fp * (2^(3/12) - 1), 2 * 48000 / 1024)`: plus or minus
   3 semitones, but at least the window's main lobe (93.75 Hz).
7. A frame is tonal when it is active and the power within `[fp - h, fp + h]`
   is at least half of `T`. (A fast smeared sweep still counts as tonal.)
8. Pitch per frame in semitones: `st = 12 log2(fp / 440)`. Frame time:
   `i * 240 / 48000 + 512 / 48000` (the frame centre).
9. Walk the tonal frames in time order and cut them into tracks. Start a new
   track when the frame index jumps by more than 2 since the last tonal frame,
   or when the pitch jumps by more than 3 semitones from the last frame (a
   partial swap).
10. For each track of at least 6 frames: smooth the pitches with a running
    median over 5 frames (the window is clipped at the track ends). Compute
    the climb at each frame as the smoothed pitch minus the running minimum so
    far. The track's climb is the largest such value; its start is the time of
    the minimum before that frame, its end is the time of that frame.
11. Keep the largest climb over all tracks.
12. `tonal_share` = tonal frames / max(1, active frames), 2 decimals.
    `rise_st` = the largest climb, 1 decimal (0 when none). `from_s`, `to_s` =
    its start and end, 3 decimals, or null when no track qualified.

Measured on the 183 forge_v2 finals in the source: it flags pro_zip up,
up_hi and arc (+20 to +64 semitones), the zip lead of logo_sting/zip_ping
(+25) and the sweep lead of pro_logo_sting/sweep_hit (+29), and two bubble pops
(pop/2 and pro_pop/bloop, +6 semitones over 55-75 ms: a real bubble). Downward
sweeps, bells and plucks give 0.

### 3.7 Attack-layer detector

Purpose: tell one clean hit from a stacked sting (two hits, or a lead-in
built onto the hit). Returns `{attacks: [s ...], count, pre_swell_ms}`.

1. Mono mix. Window 25 ms (1200 samples), hop 5 ms (240). When the signal is
   shorter than one window plus 2 hops, return `attacks [0.0], count 1,
   pre_swell_ms 0.0`.
2. Envelope: the mean square of each window, in dB (+1e-24), minus its
   maximum, so the loudest frame is 0 dB. (A shorter window reads a gated grain
   texture as many attacks; 25 ms does not.)
3. `i30` = the first frame at or above -30 dB. `i6` = the first frame at or
   above -6 dB.
4. Candidate peaks: frames at or above -6 dB that are at least as loud as both
   neighbours.
5. Walk the candidates in order:
   - Stop when a candidate lies more than 0.5 s after the first kept attack.
   - When the envelope between the last kept attack and this candidate never
     dips 10 dB under the quieter of the two, they are one attack: keep the
     louder of the two as the attack and go on.
   - Else keep the candidate as a new attack.
6. Attack times: `(i * 240 + 600) / 48000` (the window centre), 3 decimals.
   `count` = the number of attacks. `pre_swell_ms` = `(i6 - i30) * 5`, 1
   decimal: the build from 30 dB down to 6 dB down.

Source results: 2 noise hits at 60 ms or 120 ms spacing give a count of 2. A
gated grain texture gives 1 (pro_impact short, air and swell: 1; deep: 2;
rumble: 3). The pre-swell is 220 ms for impact/swell and 140 ms for
logo_sting/zip_ping; impact/classic and pro_impact/short have 0.

### 3.8 Live time

`live_ms(x, db=-40)`: mono mix, padded to at least 1 ms. Mean square in 1 ms
windows (48 samples) every 0.25 ms (12 samples). Count the windows whose mean
square is at least the largest one times `10^(-40/10)`. The result is that
count times 0.25 ms. It is 0 when the signal is all zero.

### 3.9 The drip allowance

A water drop's bubble rises in pitch as its cavity closes. That climb is the
physics, not a synth whip. For the `drip` category only, a glide is allowed
(note, not warning) when all hold:

- `rise_st` is at most 14 semitones;
- the climb starts within the first 60 ms (`from_s <= 0.06`);
- the climb is done within 250 ms of its start (`to_s - from_s <= 0.25`).

A glide with no start (`from_s` null) is allowed. Any other rising tone in a
drip, and any climb in another category, stays a warning. `drip/bloop` (about
5 semitones over 0.1-0.15 s) is the member that needs the allowance; the other
drips rise inside 50 ms and never reach the glide rule.

### 3.10 `sfx_check` on files

- One path: `check(path, category, role, sound=basename)` plus `file: path`.
- A list of paths: `{summary: "<clean> / <N> clean", ok, files: [one result per
  path]}`. `ok` is true when every file is clean.
- Text output: for several files, the overall summary line, then each file's
  summary line. For one file, its summary line.
- Exit code 0 when `ok`, else 1.

### 3.11 Audit of the shipped bank (source facts)

With each entry's own seed, 236 of the 252 takes are clean; all 51 forge04
members are among them. The other 16: the 3 rising zips (pro_zip up, up_hi, arc), the 2 logo-sting
leads (logo_sting/zip_ping, pro_logo_sting/sweep_hit), the 2 rising bubbles
(pop/2, pro_pop/bloop), typing_key light and laptop (comb ring),
pro_hover_tick air and glass (clicks), hover_tick/detent and
pro_typing_key/soft (truncated), glitch/dropout and logo_sting/reverse_bloom,
and pro_shimmer/m3 (ring). No preset uses any of them. With their own seeds,
the 15 fit entries are all clean. With some other seeds, `fit/bell/small_bell`
and `fit/key/lubed` get the comb-ring warning, which their recordings also get.

## 4. The knob contract (as seen through `sfx render` and kits)

The knobs fit one bank sound to the numbers of a film by changing the
recipe's own parameters. Nothing is resampled or time-stretched. `bank.render` calls the
forge-core knob engine with the entry, its renderer, `dur`, `apex` and
`pitch`. The engine returns the raw render and `edits`, a record, per knob, of the
change it made and the error that remains.

The knob engine also has 4 "tonal" knobs (decay, onset, bright, width). They
are not part of `sfx render`, `bank.render` or kit `edits` in this unit. The
kit role key `width` is a different thing (part 3, 6.5).

### 4.1 Knob description

`describe(entry)` returns:

```json
{"dur": "shape" | "param" | "trim", "apex": true | false,
 "pitch": {...} | null, "why": "<reason when pitch is null>",
 "tonal": {"decay": "<param>" | null, "onset": ..., "bright": ..., "width": ...}}
```

- `pitch` for a pitched sound: `{param, native_hz, note, range_hz: [lo, hi]}`.
- For a pitch-class sound (shimmer, pro_shimmer): `{param, mode: "pc", key,
  note}`.
- For a click or toggle: `{param, mode: "partial", param_hz, native_hz (the
  heard partial), note, range_hz}`. The range is the parameter's range moved
  by heard / parameter.
- For a fit: `{param (the fit's pitch knob), native_hz, note, range_hz}` with
  a range of one octave either side.

### 4.2 `dur` (seconds: the length of the file)

Range 0.01 to 60 s. The method depends on the category:

| method | categories | what happens |
|---|---|---|
| shape | the whoosh family: whoosh_fast, whoosh_big, pro_whoosh_big, swish, swipe | the rise, hold and fall of the pass all get one scale factor, so the shape of the pass stays the same. With apex too: the rise follows from the apex, hold and fall from the rest. At most 4.4 s. |
| param | riser, pro_riser, downlifter, sub_drop, pro_sub_drop, shimmer, pro_shimmer, pro_zip, glitch, pro_glitch, pro_typing_run, pro_logo_sting, the end gate of the interface sounds (click, pro_click, hover_tick, pro_hover_tick, tap, pro_tap, pro_pop_in, pop, pro_pop, notify, pro_notify, pro_typing_key), reverse_swell; a fit of kind spectral or gesture (its length knob) or sines (its stretch knob) | scale the recipe's length key, render, measure, then correct by the secant method over the renders made so far (2 corrections at most; keep the closest render; when the key does not change the length, use trim) |
| trim | everything else: impact, pro_impact, toggle, typing_key, logo_sting, pluck (always 3 s of ring and room), cut_hit (its length is a ceiling), knock, mallet, ui_pop, drip, shutter, modal and hybrid fits, and a category whose entry lacks its length key | the render keeps its own length; only the final cut or pad applies |

After that, the file has exactly `round(dur * 48000)` samples:

- When the render is too long, cut it and apply a raised-cosine fade-out
  over one quarter of the target length, at least 10 ms and at most 150 ms (never more
  than the whole length). With `apex` too, the fade starts no earlier than
  5 ms after the apex, so the fade can be as short as 2 ms.
- When the render is too short, add silence at the end.
- For a riser, the length parameter is already exact: `length` 1.0 makes a
  1.000 s file whose last 5 ms are its loudest.

`edits.dur` holds `{target, method, renders, fit}` plus the method's own keys
(`scale` for shape alone, `post_scale` with apex, `param` and `value` for a
parameter, `knob` and `value` for a fit). `fit` is `exact`, `trim N ms` or
`pad N ms` (with apex: `trim N ms (fade M ms)`).

### 4.3 `apex` (seconds from the start to the loudest 5 ms)

- The whoosh family only. Every other category refuses it, and says why: the
  loudest sample of a riser is its last one, so use `dur` and align on `end`.
- Range: 0.01 s to 4.3 s, and below `dur - 0.02` s when `dur` is given.
- The whoosh pass has an analytic peak time
  `tp = rise * (|start_db| / 30)^(1 / rise_k)` (start_db and rise_k are
  whoosh recipe parameters, recipes-motion spec). The engine solves the rise for
  the target, renders, measures, and corrects the rise by target / measured,
  up to 2 corrections (the rise is kept within 0.25x to 4x of the closed-form
  value), keeping the best render. When `dur` is also set, the score of a render
  is its apex error plus the length error of the part after the apex, and
  hold and fall get scaled to fit that part.
- Then the best render's peak is moved onto the target exactly: when the peak
  is late, the quiet approach is cut off (only a head that stays at least
  20 dB under the peak; a 3 ms raised-cosine fade-in follows the cut); when
  the peak is early, silence is padded before the start. The shift is a whole
  number of 1 ms steps, the hop of the peak measure. (With a shift off this grid,
  the measure can jump between two maxima of almost equal level, for example
  0.2 dB and 15 ms apart on whoosh_big/swell.) When the head is too loud to cut, the
  render is kept and the record says why.
- When `dur` is set too, measure the apex again on the final file, after the
  length fit.
- `edits.apex` = `{target, measured, err_ms, search_err_ms, rise, renders,
  fit, ok}`. `ok` is true when the delivered file's apex is within 5 ms.
  `fit` is `exact`, `head trim N ms`, `head pad N ms` or `head kept: ...`.

Source results: with the closed form only, the error is about 12 ms or less on
the fast family. On a broad pass that flutters, the 5 ms peak does not move
steadily with the rise: a 10 % change can move it by up to 50 ms (whoosh_big
and pro_whoosh_big swell). With the placement every tested target (59 whoosh
targets, including peaks 0.03-0.1 s before the end) landed within 2.5 ms.

Example: `--dur 0.8 --apex 0.32` makes a file of 0.8 s with its peak at
0.32 s.

### 4.4 `pitch` (`'C5'`, `'Eb6'`, `'F#4'`, `'Fs4'`, `440`, `'440hz'`)

Note names: a letter A-G (either case), then 0 to 2 accidentals (`#` or `s`
for sharp, `b` for flat), then an octave number (may be negative). MIDI number
= 12 * (octave + 1) + pitch class + sharps - flats. A4 = 440 Hz. A number, or a
number followed by `hz` (any case, optional space), is Hz. The result must lie
in 8 Hz to 20 kHz. A boolean or anything else is refused.

The target sets the recipe's own pitch parameter; the other parameters of the
same resonating body get the same ratio. The target must lie inside the range:

| category | what moves | range |
|---|---|---|
| pluck | the fundamental | 130-2100 Hz |
| notify | the head fundamental | 220-3200 Hz |
| pro_notify | the head fundamental | 600-3200 Hz |
| click, pro_click | the tink centre, moved until the heard partial is on the note | parameter 1.2-9.5 kHz, moved by heard / parameter |
| toggle | the tink centre, moved until the heard partial is on the note | parameter 1.6-8 kHz, moved likewise |
| hover_tick, pro_hover_tick | the ping (and the tick with it); only when the ping layer is on (level at least 0.1) | 3-15 kHz |
| tap, pro_tap, pro_pop_in | the body resonance (knock and glass partials with it) | 70-220 Hz |
| pop | the fundamental (its high, low and blip partners with it) | 400-3500 Hz |
| pro_pop | the fundamental (partners and noise bands with it) | 280-1400 Hz |
| sub_drop, pro_sub_drop | the landing pitch; the start keeps the glide ratio | 28-62 Hz |
| pro_zip | the end pitch (start and peak with it) | 80 Hz-12 kHz |
| riser, pro_riser | the tonal layer; only when it is on (level at least -40 dB) | 60-4000 Hz |
| downlifter | the tonal layer; only when it is on | 150-2500 Hz |
| logo_sting | the root | 300-2200 Hz |
| pro_logo_sting | the tone root (the lead root with it) | 50-1800 Hz |
| impact, pro_impact | the body landing pitch (body start and sub with it) | 34-75 Hz |
| pro_whoosh_big | the sub; only sub-led members (sub at least -30 dB) | 25-120 Hz |
| shimmer, pro_shimmer | the key: pitch class only, the register stays | any note |
| ui_pop | the landing pitch | 250-1200 Hz |
| mallet | the fundamental (runs too) | 150-3200 Hz |
| knock | the lowest mode | 250-2600 Hz |
| drip | the pitch at pinch-off | 250-1600 Hz |
| cut_hit | the sub's landing pitch (its start keeps the glide ratio) | 36-130 Hz |
| a fit with a measured note | sines: transpose (semitones); hybrid: pitch (semitones); modal: size (ratio 2^(-st/12)) | one octave either side of its note |

Refused, each with its reason:

- noise sounds: whoosh_fast, whoosh_big (passing air has no pitch), swish (the
  whistle tracks the speed), swipe (rubbing noise), glitch, pro_glitch (a
  stutter has no note), typing_key, pro_typing_key, pro_typing_run (keys have
  no tuning; variation comes from new takes);
- a member whose tonal layer is off (see the table);
- a click or toggle with no clear partial (`click/thock`, `toggle/micro`);
- a fit with no measured note, or of a kind without a pitch knob;
- a category with no pitch parameter (for example shutter, reverse_swell);
- a target outside the range. The message names the target note and Hz, the
  range in Hz and as notes, and the sound's own note.

Measurement after the render (`edits.pitch`): `target_hz`, `note` (the nearest
note name of the target, with cents when 5 or more off: `A4 +12c`),
`native_hz`, `semitones`, `param`, `params` (the new values) or `knobs` (for a
fit), `measured_hz` (8.1 of part 1, near the target; on the second half of the
render for sub drops, pro_zip and pro_whoosh_big, which glide to their note),
`cents_off` (`round(1200 log2(measured / target))`) and `ok` (true within 50
cents). `ok` is null for sounds whose pitch is not measured: riser, pro_riser,
downlifter, impact, pro_impact, drip (its heard pitch is where it rises to),
cut_hit (the kick and the short glide hide the landing). A pitch-class edit
has `{param, key, note, native_key, mode}` and no measurement.

Clicks and toggles use a measured search: scale the parameter by target /
heard, render, measure, then scale by target / measured; 2 corrections at
most, and stop as soon as the error is under 25 cents. Keep the closest
render. Stop the search when a render shows no clear partial. pro_click
members reach their notes within a few cents. Some click and toggle members
cannot reach all notes; for those, `ok` is false.

The pitch warning: when `edits.pitch.ok` is false, the render (and a kit take)
adds the warning `pitch_off` to its audit: "pitch <note> missed: measured <note>
(<Hz>, <cents>), more than 50 cents off", or "pitch <note> missed: the
render has no clear partial near the target" when no pitch was measured.

Source results: pluck E5, notify G5 and pro_sub_drop G1 are 0 cents off. The
body of a tap is a damped thump; it gets within about 60 cents.

### 4.5 Labels

`label(pitch)` gives one file-name label per pitch: when the pitch is no more
than 5 cents from a note, the note name with sharps spelled `s` (`C#5`, `Db5` and
523.25 Hz all give `Cs5` or `C5`), else the Hz rounded to 2 decimals in the
shortest form plus `hz` (`445hz`). Sharps use the names C, C#, D, Eb, E, F, F#,
G, Ab, A, Bb, B (so a flat name such as `Eb` stays `Eb`). It refuses a value
that is not a pitch.

`note(f)`: the nearest note name with the same name table, plus ` +Nc` or
` -Nc` when the cents are 5 or more off.

## 5. `sfx render`

### 5.1 Behaviour

`sfx_render(sound, out=None, seed=None, dur=None, apex=None, pitch=None,
peak_db=-1.0, weight=None, sr=48000)`:

1. `bank.render` (part 1, section 5) with all the knobs and checks on.
2. When `out` is given, write the take as a 24-bit PCM WAV at `sr`, creating
   the parent folder. The returned path uses `/` as separator.
3. Return `{summary, file, meta}`. `file` is null without `out`.

The CLI requires `-o`.

### 5.2 Summary line

The parts, joined by `; `:

1. `<id> seed <seed>: <dur> s, onset <onset> s, peak at <peak> s, end <end>
   s, <peak_db> dBFS` (times 3 decimals, peak 1 decimal), plus `, <L> LUFS
   momentary max` (1 decimal) when it is known.
2. When apex was used: `apex <target> s (measured <m>, <+err> ms)`.
3. When dur was used: `dur <target> s (<method>, <fit>)` (`exact` when there
   is no fit text).
4. When pitch was used: `pitch <note>` plus ` (measured <Hz> Hz)` when measured.
5. When there are checks: `checks: clean`, or `checks: ` and the warning
   messages joined by `; `.

The line starts with `wrote <file>: ` when a file was written. Exit code 0
(a warning in the checks does not change the exit code).

### 5.3 Default take

`hark sfx render whoosh_fast/crisp -o w.wav` writes the delivered final at
-1 dBFS: same length as `meta.dur_s` of the entry, peak time within 5 ms of
the stored `peak_s`.

## 6. Constants of `sfx`

| name | value | meaning |
|---|---|---|
| audit rate | 48000 | audit rate |
| silent peak | -60 | a sample peak under this is silent |
| silent live time | 5 ms | live time under this is silent |
| live threshold | -40 dB | re the loudest 1 ms |
| live frames | 1 ms window, 0.25 ms hop | |
| edge, dc limits | -40 dB re peak | |
| ring limit | z 15 | |
| truncated limit | -45 dB re peak | |
| inner silence limit | 4 ms | |
| tail re-rise limit | 6 dB | note only |
| glide limit | 4 semitones | the rising-glide limit |
| glide minimum length | 0.05 s | the shortest climb that counts |
| glide frames | 1024 samples, hop 240, FFT 8192, Hann | |
| glide band | 80 Hz-10 kHz | |
| glide active floor | -30 dB re the loudest frame | |
| glide tonal test | half the band power within +-3 st (at least 93.75 Hz) | |
| glide track split | index gap over 2 frames or pitch jump over 3 st | |
| glide track minimum | 6 frames | |
| glide smoothing | 5-frame running median | |
| layer envelope | 25 ms window, 5 ms hop | |
| layer peak level | -6 dB re loudest | |
| layer dip | 10 dB | |
| layer window | 0.5 s after the first attack | |
| pre-swell limit | 100 ms | |
| pre-swell span | from -30 dB to -6 dB | |
| ends-on-peak | largest sample at 97 % of the length or later | |
| drip allowance | at most 14 st, start by 60 ms, done within 250 ms | |
| apex tolerance | 5 ms | |
| apex fade gap | 5 ms after the apex | |
| shortest dur fade | 2 ms | |
| dur fade | a quarter of the length, 10-150 ms, raised cosine | |
| dur range | 0.01-60 s | whoosh family at most 4.4 s |
| apex range | 0.01 s to 4.3 s, and under dur - 0.02 s | |
| apex head limit | -20 dB re peak | |
| apex head fade-in | 3 ms | |
| knob corrections | at most 2 after the first targeted render | |
| length search stop | within 1 % of dur or 5 ms | |
| length key scale | 0.15x to 6x of the member's own value | |
| pitch tolerance | 50 cents | |
| partial early stop | 25 cents | |
| pitch range | 8 Hz-20 kHz | |
| fit pitch reach | 12 semitones | |
| label snap | 5 cents | |
| WAV format | 24-bit PCM | |
