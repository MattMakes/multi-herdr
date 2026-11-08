# bank-kit-sfx, part 3: film kits (`hark kit`), the mix helpers and the presets

Part of the spec `docs/specs/hark/bank-kit-sfx.md`. Read the index first for the
exact CLI flags, the errors and the acceptance tests. This part holds the
behaviour, the algorithms and the constants of kits.

## 1. What a kit is

A kit is one film's sounds, by role. It names one bank sound per role and how
to render it. `kit render` turns it into a folder of takes (24-bit WAVs) and a
lock file that records each take's measured onset, peak and end. A renderer
(the timeline unit's score, or any mixer) places each take on its cue by those
measured sync points, not by the file start.

## 2. Roles

The role names that every renderer knows (film cue kinds, with variants):

`tick`, `tick.crisp`, `tock`, `hit`, `land`, `land.light`, `pop`,
`whoosh.short`, `whoosh.long`, `suck`, `riser`, `drop`, `key`, `key.space`,
`key.return`, `click`, `click.confirm`, `snap`, `swell`, `bell`, `text`.

- `text` is a soft sound for a text fragment that resolves.
- Other role names are allowed. They must be lowercase words joined by dots:
  `^[a-z][a-z0-9_]*(\.[a-z0-9_]+)*$`. A renderer must know them. `kit check`
  notes a role whose first word is not the first word of a shared role.

Default sync point per role (`align`):

| role | default align |
|---|---|
| whoosh.short, whoosh.long | peak |
| suck, riser, swell | end |
| every other role | onset |

## 3. kit.json (schema 1)

```json
{"kit": 1, "name": "calm", "sr": 48000, "note": "free text",
 "roles": {
   "hit":  {"sound": "notify/m0", "takes": 2, "seed": 1, "edits": {"dur": 1.6}, "align": "onset",
            "max_s": 2.0, "level_db": -3.0, "note": "one struck bell for the lockup"},
   "bell": {"sound": "pluck/kalimba", "pitches": ["G4", "A4", "C5", "D5"]},
   "riser": null},
 "off": {"riser": "the reveal lands on one sound"},
 "mix": {"level": "bed", "duck_db": 0, "stop_down": {"before": ["hit"], "depth_db": [6, 12], "length_s": [0.3, 0.5]}}}
```

Top level:

| key | meaning |
|---|---|
| `kit` | must be 1 |
| `name` | the kit name (used in summaries and in the audition cache folder) |
| `sr` | the rate of the take files: 44100, 48000 (default), 88200 or 96000 |
| `note` | free text. Ignored by the code. |
| `roles` | a non-empty object: role -> spec or null |
| `off` | role -> the reason it is null. Information only. |
| `mix` | optional kit-level information for the mixer (6.6) |

Role spec keys. No other keys are allowed:

| key | default | meaning |
|---|---|---|
| `sound` | required | a bank id (forge, `pluck/*` or `fit/*`) |
| `takes` | 1 | takes to render, an integer 1 to 32 (a boolean is not an integer) |
| `seed` | the sound's own seed | an integer. Take k (counted from 1) uses seed `seed + k - 1`. With the default seed, take 1 equals the delivered final. |
| `edits` | `{}` | the knob values applied to all takes: `dur`, `apex`, `pitch`, `peak_db` (default -1, 0 or less), `weight`. Validated like a render (section 4). |
| `pitches` | none | a non-empty list of notes or Hz. One set of `takes` takes per pitch. It cannot be combined with `edits.pitch`. `C#5`, `Db5`, `Cs5` and 554.37 all name the same pitch; a list must name each pitch once. |
| `align` | per role (section 2) | `onset`, `peak` or `end`: which measured time the mixer places on the cue |
| `max_s` | none | a take longer than this many seconds is faded out to it (the dur fade of part 2, 4.2). At least 0.01. |
| `level_db` | 0 | the role's level in the mix, in dB, relative to a take at its own peak. The mixer reads it; the audition uses it. |
| `note` | none | free text: the reason for the choice |
| `bed_db` | none | mix key (6.1), 0.5 to 30 |
| `offset_ms` | none | mix key (6.2), -500 to 500 |
| `min_gap_ms` | none | mix key (6.3), 0 to 2000 |
| `width` | none | mix key (6.5), 0 to 1 |

A role set to null is off. A renderer gives its cues no sound.

What each align means:

- `onset`: the measured onset of the take.
- `peak`: the loudest 5 ms of the take. Use it for whoosh apexes, and for a riser or a
  swell that falls after its climax (the forge04 members).
- `end`: the measured end of the take (part 1, 6.1). Use it for
  builds that stop on their loudest sample (the forge_v2 risers).

When a kit sets no mix key, its plan, cache keys and lock are the same as for
a kit written before mix keys were added.

## 4. Validation (`problems`)

`problems(kit)` returns every problem as a list of strings, `[]` when the kit
is valid. `load(kit)` reads a path (a JSON parse error is a `KitError` "not
JSON") or takes a dict, and raises `KitError` "invalid kit: " plus all problems
joined by `; ` when there are any. The checks, in order:

1. Not a dict, or `kit` is not 1: one problem, "not a kit file", and stop.
2. `sr` (default 48000) not one of the 4 rates.
3. `roles` not a non-empty object: add that problem and stop.
4. Per role, in file order:
   1. the role name fails the name pattern;
   2. null: skip the rest for this role;
   3. not an object: a problem, skip the rest;
   4. unknown keys (listed sorted, with the known keys);
   5. `sound` not a bank id: a problem with up to 3 close ids (ratio 0.5 or more);
   6. `takes` not an integer in 1..32;
   7. `seed` given and not an integer;
   8. `edits` not an object, or with keys other than the 5 edit keys (then the
      edits count as empty);
   9. `pitches` given and not a non-empty list (then it counts as absent);
   10. both `pitches` and `edits.pitch`;
   11. `align` (or the role default) not one of the 3;
   12. `level_db` present and not a finite number (a boolean is not a number);
   13. `max_s` given and not a finite number of at least 0.01;
   14. each mix key present and not a finite number in its range;
   15. the edit values (4.1).
5. Take file names that collide (4.2).
6. The `mix` block (6.6).

### 4.1 Edit values

The same checks a render runs, without rendering:

- The level: `peak_db` (default -1) and `weight` through the bank level rule
  (part 1, 5.2): over 0 dBFS, a weight outside (0, 4], or a non-number.
- `dur` and `apex`, when given, must be finite numbers. When one is not, stop
  checking this role's edits.
- When the sound is a known id: the knob validation of the entry for `dur` and
  `apex` together (dur 0.01-60 s, at most 4.4 s for the whoosh family; apex only
  on the whoosh family, inside 0.01 s to 4.3 s and under dur - 0.02 s).
- Each pitch (every item of `pitches`, or `edits.pitch`) through the pitch
  validation of the entry (part 2, 4.4): refused sounds, the range, the note
  syntax. The problem names the pitch.

### 4.2 Take file names

Plan the file names (5.1) for every sounded role whose pitches all parse.
Compare them case-insensitively (macOS and Windows file systems ignore case).
For each collision:

- within one role, from two pitches: "pitches X and Y are one pitch (file):
  list each pitch once";
- else: "take file F clashes with F0 of role R0 when case is ignored: give
  the role another name".

Duplicate problem strings are removed, first occurrence kept.

## 5. Planning and rendering

### 5.1 The plan

`plan(kit)` lists every take a kit renders. It raises `KitError` when take
files collide (4.2). For each sounded role in file order:

- base seed = the role's `seed`, else the entry's own seed;
- n = `takes`;
- stem = the role with every `.` replaced by `-` (`tick.crisp` -> `tick-crisp`);
- edits = the role's edits without `pitch`;
- pitches = the `pitches` list, else `[edits.pitch]` (which may be `[null]`);
- for each pitch, for k = 1..n: the file name is
  - with a `pitches` list: `<stem>_<label>.wav` when n is 1, else
    `<stem>_<label>_<k>.wav`;
  - without: `<stem>_<k>.wav` (always numbered, also when n is 1);
  - `<label>` is the pitch label (part 2, 4.5);
- each planned take: `{role, file, sound, seed: base + k - 1, pitch, edits,
  max_s, k}`, plus `width` (as a float) when the role sets it.

Every pitch of a role uses the same seeds. Examples: `hit_1.wav`,
`tick-crisp_2.wav`, `bell_C5.wav`, `text_Cs5_2.wav`, `bell_445hz.wav`.

### 5.2 Cache key

`take_key(take, sr)` is the SHA-1 hex digest of the canonical JSON (keys
sorted, separators `,` and `:`, non-JSON values as strings) of:

`{sound, entry: <digest of the bank entry>, seed, pitch, edits, max_s, sr,
bank: <bank VERSION>, hark: <hark version>}` plus `width` only when the take
has one.

The entry digest is the same canonical SHA-1 over the whole entry as `get`
returns it (with its `id`). So the key changes when the entry, the seed, the
pitch, the edits, `max_s`, the rate, any bank file version, the hark version,
or the width changes. A kit without width keys keeps its old keys.

### 5.3 One take, with re-rolls

`render_take(take)` renders one planned take at 48 kHz and returns
`(y, audit, meta, seed_used)`.

One render on seed s:

1. `bank.render(sound, seed=s, dur, apex, pitch, peak_db (default -1),
   weight, checks off)`.
2. When `max_s` is set and the take is longer than `max_s` seconds, fit it to
   `max_s` with the dur fade (part 2, 4.2; padding never happens here).
3. When `width` is set, set the stereo width (6.5) and keep its info in
   `meta.width`.
4. Audit (part 2, section 3) with the entry's category, the role, and the
   sound id as the name. Add the pitch warning when the pitch missed (part 2,
   4.4).

Re-rolls: try seeds `s0 + 1000 j` for j = 0, 1, 2, 3 (`REROLL` = 3 extra seeds).
Return the first take whose audit is clean. When none is clean, return the take
on the planned seed with its warnings. This is what a sound designer does with a bad
take. Measured in the source: about 1 take in 20 on a new seed rings or
stops early; on the 3 non-launch presets 4 of 191 takes needed a re-roll (3 key
takes and 1 tuned ding; all of them forge members, none a fit).

Rendering is deterministic: the same kit gives the same takes.

### 5.4 `kit render`

`render(kit, out, force=False)` renders every take into the folder `out` and
writes `out/kit.lock.json`. Steps:

1. Load and validate the kit (`KitError` on problems). sr = the kit's `sr`.
2. Create `out`. Read the old lock when it exists and parses (an unreadable
   lock counts as absent). Index its takes by (key, file).
3. Start the lock: `{kit: 1, hark: <version>, bank: <VERSION>, sr, name,
   roles: {}}`. Each role: null when not sounded, else `{sound, align (the
   default when absent), level_db (float, 0 when absent), max_s, <each set mix
   key as a float>, takes: []}`. Copy the kit's `mix` block when it has one.
4. For each planned take:
   - Its key (5.2) and path.
   - Cache hit: not `force`, the old lock has a take with this key and this
     file name, the file exists, and the SHA-1 of the file bytes equals the old
     record's `sha1`. Then keep the old record. When the role has `bed_db` and
     the old record has no `l50_lufs` (cached before the role had it), read the
     file and add `l50_lufs` (6.1). Count it as cached.
   - Else render it (5.3), measure it at 48 kHz (part 1, 6.5), resample to sr
     when sr is not 48000, write a 24-bit WAV, hash the file. The record:
     `{file, seed (the seed used), pitch, onset_s, peak_s, end_s, dur_s,
     peak_db, lufs_m_max, sha1, key, checks: [warning codes]}`, plus
     `planned_seed` when a re-roll changed the seed, plus `l50_lufs` when the
     role has `bed_db` (measured on the written signal at sr), plus `corr`
     (the measured correlation) when the take has a width. Count it as rendered.
5. Remove stale files: each file named in the old lock that the new lock does
   not name, only when the name is a plain file name (no folder part) and the
   file exists in `out`. Nothing else in `out` is touched.
6. Write the lock: JSON with an indent of 1 and a final newline, to
   `kit.lock.json.tmp`, then rename it over `kit.lock.json`.
7. Return the lock plus `stats = {takes, rendered, cached, removed, seconds}`,
   `summary` and `file` (the lock path with `/`). The file on disk does not hold
   `stats`, `summary` or `file`.

Summary: `kit <name>: <N> takes in <out> (<r> rendered, <c> cached[, <x> stale
removed]) in <t> s; checks clean`, or `...; warnings: <role>:<code>, ...` (the
sorted set of role:code pairs over every take).

When not `--json`, the command also logs one line per rendered take: the file,
the sound, the planned seed, the length, and `WARN <codes>` when the take has
warnings.

### 5.5 kit.lock.json

```json
{"kit": 1, "hark": "0.1.0", "bank": "<bank VERSION>", "sr": 48000, "name": "calm",
 "roles": {
   "hit": {"sound": "notify/m0", "align": "onset", "level_db": -3.0, "max_s": null,
           "takes": [{"file": "hit_1.wav", "seed": 1826701615, "pitch": null, "onset_s": 0.0,
                      "peak_s": 0.0045, "end_s": 2.157, "dur_s": 2.3, "peak_db": -1.0,
                      "lufs_m_max": -11.06, "sha1": "<hex>", "key": "<hex>", "checks": []}]},
   "pop": {"sound": "ui_pop/soft", "align": "onset", "level_db": -8.0, "max_s": null,
           "bed_db": 8.5, "offset_ms": 4.0, "min_gap_ms": 114.0,
           "takes": [{"file": "pop_1.wav", "...": "...", "l50_lufs": -14.2}]},
   "riser": null},
 "mix": {"level": "bed"}}
```

`checks` holds the audit warning codes of the take, in rule order
(`pitch_off` last when present).

### 5.6 `kit check`

`check(kit, out=None)` validates the kit and audits every take. It returns
`{ok, summary, problems, roles: {role: [take rows]}, notes}`.

1. Read the kit (path or dict). A file that cannot be read or parsed gives
   `ok` false, summary `kit unreadable: <error>`, problems `[error]`, no rows.
2. Problems (section 4): when there are any, `ok` false, summary
   `<N> problem(s): <p>; <p>`, and stop.
3. Notes, per role in file order:
   - a sounded role whose first word is `riser`, `swell` or `suck`: the note
     says the role is on, while calm, crisp and minimal turn it off (a reveal
     works better on one sound than on a build plus a hit; launch uses one
     riser or swell where a section changes);
   - a sound the presets never use, with the reason (6.8);
   - an end-aligned role whose sound's stored `end_s - peak_s` is more than
     30 ms: the sound goes on ringing after its peak, so its climax comes that
     much before the cue; with align `peak` the climax is on the cue;
   - a role not in the shared vocabulary (section 2): a renderer must know it.
4. When `out` is given and its lock parses, index its takes by key.
5. For each planned take: a row `{file, sound, seed, pitch}`. When the lock has
   a take with this take's key and its file exists in `out`, audit the file
   (role as context; `source: "rendered file"`), and add the `pitch_off`
   warning again when the lock record lists it (the file cannot show it).
   Else render the take in memory with re-rolls (`source: "rendered in
   memory"`; when the seed changed, `seed` is the used seed and
   `planned_seed` the planned one). Add `ok`, `warnings`, `notes`. Any error in
   this step (for example a knob the sound cannot take) gives `ok` false and one
   warning `{code: "render", msg: "<ErrorType>: <text>"}`; the check goes on with
   the next take.
6. Summary: `kit <name>: <clean> / <N> takes clean`, plus `, <b> with warnings:
   <file>: <codes>; ...`, plus `; off: <roles>` when roles are null, plus
   `; <k> note(s)` when there are notes. `ok` is true when every take is clean.

Text output: the summary, then one `note: <text>` line per note. When not
`--json`, one log line per take: file, sound, then `ok` or the warning
messages. Exit code 0 when ok, else 1.

### 5.7 `kit audition`

`audition(kit, out_wav, takes=None, gap_s=0.3, role_gap_s=0.8)` writes every
take of a kit, role by role, into one WAV, each at its role's level.

1. Load the kit. The takes folder is `takes`, else
   `<HARK_HOME>/kits/<name or "kit">_<first 10 hex of the kit digest>` (the kit
   digest is the canonical SHA-1 of the kit document).
2. Render the kit into that folder (5.4, cached).
3. For each sounded role in lock order, each take in order:
   - gain = `gain_db(take, role, -24)` (6.1): a role with `bed_db` is set
     against a nominal music bed of -24 LUFS, any other role at its
     `level_db`;
   - read the file as float32; a mono file becomes 2 identical channels; more
     than 2 channels keep the first 2;
   - append the take times `10^(gain/20)`, then `gap_s` of silence;
   - the index entry `{t, role, file, sound, pitch}` with `t` the start time in
     seconds, 3 decimals.
   - After each role, append `role_gap_s` of silence.
4. No sounded role: `KitError` "the kit sounds no role".
5. When the mix's sample peak is over -1 dBFS (a level_db over 0 on a -1 dBFS
   take), scale the whole mix down to -1 dBFS.
6. Write the WAV (24-bit, at the lock's sr). Write the index beside it, with
   the extension `.txt`: one line per take, the time as `%8.3f`, two spaces,
   the role in 14 columns, the sound in 26 columns, the file.
7. Return `{summary, file, index, takes_dir}`. Summary: `wrote <file>: <N>
   takes of <R> roles, <len> s (index in <stem>.txt)`.

### 5.8 `kit init`

`init(preset="calm", out="audio/kit.json", force=False)`:

1. Load the preset by name from the package presets folder. Unknown name:
   `KitError` listing the presets (sorted names of the `.json` files there).
2. When `out` exists and not `force`: `KitError` (force overwrites).
3. Create the parent folder. Write the preset as JSON with an indent of 1,
   non-ASCII characters kept, and a final newline.
4. Return `{summary, file, kit}`. Summary: `wrote <out>: preset <name>, <s>
   roles sounded, <o> off (<null roles>)`.

## 6. Mix keys and the mix helpers

The mix keys are for films cut to music. The helpers are pure numpy and scipy
functions a mixer calls. hark exposes them in Python; a CLI is not needed.

### 6.1 `bed_db`: level over the music bed

Model. The source measured its references on the full mix (effect and bed
together). `bed_db` = the K-weighted LUFS level of the loudest 50 ms of that
mix around the effect, minus the bed level. The bed level = the median of the
K-weighted 50 ms levels of the music during the 1 s before the cue. In films
cut to music, effects sit 3 to 10 dB over the bed, not at one fixed level.

K-weighted power (`kpower(x)`): filter each channel with the BS.1770 K filter,
square, sum the channels. A mono signal (1-D or 1 channel) counts as the 2
identical channels a player makes of it: its power is doubled (+3.01 dB). This
makes a mono take and its stereo file measure the same.

- `loud_lufs(y, sr=48000, win=2400)`: resample to 48 kHz when needed. The
  largest mean of `kpower` over any 2400 consecutive samples (50 ms); when the
  signal is shorter, its total over 2400. Return `-0.691 + 10 log10(mean +
  1e-24)`, 2 decimals. This is a take's `l50_lufs`.
- `bed_level(music, t, sr=48000)`: `music` is samples at sr or a path (loaded
  at 48 kHz). Resample to 48 kHz when needed. Take the samples from
  `max(0, t - 1)` s to `max(0, t)` s. Split them into whole 2400-sample blocks.
  No whole block: `KitError` "no music before t". Mean `kpower` per block; the
  median of those; `-0.691 + 10 log10(median + 1e-24)`, 2 decimals. Return
  None when the result is under -60 LUFS (no bed: for example before the music
  starts, in a break in the score, or on an end card after the music ends).
- `over_bed(bed_db) = 10 log10(10^(bed_db/10) - 1)`: how far the take alone must
  sit over the bed so that take plus bed (powers add) sit `bed_db` over it.
  At 8.5 dB the take alone sits 7.8 dB over the bed; at 3 dB it is level with it.
- `gain_db(take, role, bed_lufs=None, ceiling_db=-1)`: the gain in dB for one take
  file at one cue. `take` and `role` are lock rows.
  - When `bed_lufs` is not None and at least -60, and the role has `bed_db`,
    and the take has `l50_lufs`: `g = bed_lufs + over_bed(bed_db) - l50_lufs`,
    then, when the take has `peak_db`, `g = min(g, ceiling_db - peak_db)`
    (the take's sample peak never goes over -1 dBFS; under a loud bed, a short
    tick can then end up below its bed_db). Round to 2 decimals.
  - Else the role's `level_db` (0 when absent).

### 6.2 `offset_ms`: placement against the picture

`place(take, role, cue_s)` = `cue_s + offset_ms / 1000 - take[<align>_s]`,
4 decimals. `align` is the role's align (onset when absent), `offset_ms` 0
when absent. The result is the start time of the take file. It may be
negative; the caller trims. A negative `offset_ms` puts the sound before the
picture (taps, knocks and clicks come a frame or two early). A positive one
puts it after (pop-in thumps, the peaks of whooshes and swells).

### 6.3 `min_gap_ms`: spacing in a run

`space_run(times, min_gap_ms)`: sort the cue times; keep the first; each next
one is `max(t, previous + gap)`, 4 decimals. A gap of 0 or None changes
nothing but the order.

### 6.4 Correlation

`corr(y)`: 1 for mono (1-D or 1 channel). Else
`sum(L R) / (sqrt(sum(L^2) sum(R^2)) + 1e-20)`.

### 6.5 `width`: set the L/R correlation

`stereo_width(y, target, tol=0.05)` returns `(take, info)`. Every take of a
role with `width` gets this at render, after `max_s`.

1. Make it stereo: a 1-D signal becomes 2 identical channels, 1 channel is
   repeated, more than 2 channels keep the first 2. Remember the sample peak.
2. Mid `M = (L + R) / 2`, side `S = (L - R) / 2`. Measure `c0 = corr`. Clip
   the target to 0..1. A take is rebuilt as `L = M + side`, `R = M - side`.
3. When `|c0 - target| <= tol / 4`: keep the take as it is.
4. When `c0 < target` (too wide): find the factor g in 0..1 for `side = g S`
   by 40 bisection steps (corr above the target: raise the lower bound; else
   lower the upper bound). Use the middle of the final bracket.
5. When `c0 > target` (too narrow): make a decorrelated copy D of the mid:
   - pass M through 3 Schroeder all-pass filters in series, delays 149, 263 and
     397 samples with gains 0.6, -0.55 and 0.5. Each all-pass is
     `u[n] = x[n] + g u[n-d]`, `y[n] = u[n-d] - g u[n]` (zero initial state);
   - high-pass the result at 250 Hz (2nd-order Butterworth), so the low end
     stays mono; do not re-level it;
   - fade its last `min(len, 240)` samples (5 ms) with `cos^2` from 1 to 0
     (the all-pass output continues after the input stops).
   Find h in 0..4 for `side = S + h D` by 40 bisection steps. When even h = 4
   leaves the correlation above the target (a sound mostly under 250 Hz), use
   h = 4: as wide as it goes.
6. Scale the result back to the remembered sample peak. Measure the
   correlation.
7. `info = {target, before, measured, ok}`, 3 decimals; `ok` is true when the
   measured value is within `tol` of the target.

A mono take comes back stereo. The result is deterministic. The lock records
the measured value as the take's `corr`.

### 6.6 `mix` (kit level)

Information for the mixer. hark validates it and copies it into the lock:

- it must be an object;
- `duck_db`, when present, a finite number;
- `stop_down`, when present, an object with: `before` (default `[]`) a list of
  role names of this kit; `depth_db` and `length_s`, when present, `[lo, hi]`
  pairs of finite numbers with lo <= hi;
- every other key is free (the launch preset adds `level`, `release`,
  `runs.accent_at_end` and `density_per_10s`).

The stop-down: the music dips just before a large hit and returns with it.
The music does not duck under each effect. Source facts: in its reference
clips, 53 % of strong hits followed a music dip of 6 dB or more within the
0.5 s before them (9 % for random times), and no ducking under effects was
found (the low band did not drop under bright effects). A mixer that applies
it lowers the music by 6-12 dB for 0.3-0.5 s before a hit, a drop, a large
whoosh or a swell, and restores it at the hit.

### 6.7 How a mixer uses a lock

1. Render the kit and read the lock (`render`).
2. For a cue of a role, pick a take from that role's `takes` (a new one each
   time).
3. Measure the bed at the cue time (`bed_level`; None when there is no bed).
4. Get the gain of the take for that cue (`gain_db`).
5. Get the start time of the take file (`place`).
6. For a run of cues of one role, first space the cue times (`space_run` with
   the role's `min_gap_ms`).

A mixer mirrors a whoosh take left to right for the travel direction: the
whoosh recipe picks its pan direction from the seed.

### 6.8 Sounds the presets never use

`kit check` notes these. The choice stays the kit author's.

| sound or category | reason |
|---|---|
| `pro_zip/up`, `pro_zip/up_hi`, `pro_zip/arc` | the pitch goes up, which reads as a cheap synth whip |
| `riser/shepard` | an endless (Shepard) rise |
| `downlifter/shepard_fall` | an endless (Shepard) fall |
| `downlifter/synth_fall` | a synthesizer fall |
| category `logo_sting` | a layered sting: lead-in, hit and chime at once |
| category `pro_logo_sting` | a layered launch gesture |
| categories `shimmer`, `pro_shimmer` | a shimmer wash, a granular shimmer |
| categories `glitch`, `pro_glitch` | a glitch |
| category `pro_pop` | tuned pops, which can read as 8-bit sound |

## 7. The presets

Four presets ship in the package presets folder: `calm` (the default),
`crisp`, `minimal` and `launch`. Each role in a preset has a `note`. hark
writes every note in its own words. The last column of each table lists the
facts that the note must give. Each preset also has a top-level `note` and an
`off` map (a reason for each role that is off).

### 7.1 How calm, crisp and minimal were chosen

- Audit: all takes of the 3 presets are clean in `kit check`. Any member with
  an audit warning was left out (for example keys that ring, airy ticks with
  clicks, zips whose pitch goes up).
- Measures per role: each candidate (forge members and fitted sounds) got the
  same 4 readings: a calibrated text match to the role's description (a z
  score); a forced choice between that description and 3 cheap-synth
  descriptions (a laser zap, a game blip, a chiptune sound); the realness
  reading (shown, not scored); and the model-free length, centroid, attack and
  crest.
- Fit or forge: a fitted sound took the place of a forge member only when it
  scored clearly better: higher match and forced choice, with realness equal
  or higher. On a near tie the forge member was kept. Examples: calm's
  `land.light` stays `tap/soft` (z +1.88, realness 0.96; the fitted fingertip
  had +2.07, 0.95), and calm's `key` stays `typing_key/thock` (z +5.74; the
  fitted keys had +4.65 to +5.14).
- Style: designed sounds where the style is designed. crisp uses a designed
  tink, a gated hit, a designed sub drop and dings. minimal uses a sub-led set
  and high-passed key ticks.
- Restraint: none of the 3 presets uses a zip or whip that rises in pitch, an
  endless riser or downlifter, a layered logo sting, a shimmer, a glitch, or a
  tuned pop that can read as 8-bit. The hit has one attack and no lead-in.
  `riser`, `swell` and `suck` are off, and calm also turns `drop` off, so that
  a reveal lands on one sound.
- Levels: when a role changed its sound, its `level_db` changed by the
  loudness difference of the old and new sound (momentary-max LUFS, both at
  -1 dBFS), rounded to 0.5 dB. The role keeps its loudness.
- Pitch: `hit` (calm) and `drop` (crisp, minimal) take `edits.pitch` set to
  the film's root note. `bell` covers 2 octaves of one pentatonic scale, so a
  motif that climbs with progress has a note for each step. A tuned `text`
  role has the 5 pentatonic notes of one octave, so successive fragments can
  walk the chord. crisp and minimal use an untuned soft tap for `text` (a new
  take for each fragment), which cannot clash with the music.

Measurements behind the fitted replacements (match z / forced choice /
realness):

| role, description | forge member before | fitted sound now | presets |
|---|---|---|---|
| tick, a quiet soft interface tick | tap/glass -0.08 / 0.01 / 0.45 | fit/tick/domino +1.02 / 0.67 / 0.83 | calm, crisp |
| click, a soft interface button click | tap/finger +1.37 / 0.61 / 0.89; pro_click/mouse +1.03 / 0.35 / 0.06 | fit/click/floss_lid +1.70 / 0.73 / 0.97 | calm, crisp |
| pop, a soft natural pop | pro_pop_in/airy +1.88 / 0.98 / 0.65 | fit/pop/mouth_plop +2.98 / 1.00 / 0.97 | calm |
| land.light, a soft finger tap on wood | pro_tap/knock +0.13 / 0.00 / 0.28; pro_tap/finger +0.69 / 0.01 / 0.16 | fit/land.light/fingertip_book +2.07 / 0.99 / 0.95 | crisp, minimal |
| whoosh.short, a fast short whoosh through air | swipe/velvet +2.01 / 1.00 / 0.97 (its tags say heavy thud) | fit/whoosh.short/bamboo_swish +5.32 / 1.00 / 0.99 | calm |
| key, one mechanical key press | typing_key/crisp +4.48 / 1.00 / 0.95 | fit/key/medium +4.65 / 1.00 / 0.99 (centroid 2.2 kHz, the forge member 0.6 kHz) | crisp |
| key.space, a space bar press | typing_key/deep +2.14 / 0.97 / 0.84; typing_key/thock +2.59 / 0.98 / 0.96 | fit/key.space/thock_space +3.22 / 1.00 / 0.99 | calm, crisp |
| snap, the click of a toggle switch | pro_pop_in/knock +0.61 / 0.27 / 0.08; toggle/latch +2.04 / 0.97 / 0.50 | fit/snap/switch_flip +2.24 / 1.00 / 0.97 | calm, crisp, minimal |
| text, a soft touch when a fragment resolves | pro_tap/soft, pro_tap/finger (realness 0.16-0.25) | fit/land.light/finger_phone (its tags say soft button tap; realness 0.89) | crisp, minimal |

The descriptions in this table are the role descriptions used for the text
match. Our listen unit can use these or its own; the numbers are the source's
results.

### 7.2 calm (text-led films; the default)

Top-level note facts: natural, soft touches; a single tuned instrument for
both bell and text; soft air for movement; a single struck note on the lockup. `sr` 48000. Off:
suck, riser, drop, swell. 64 takes.

| role | sound | takes | seed | edits / pitches | align | level_db | facts for the note |
|---|---|---|---|---|---|---|---|
| tick | fit/tick/domino | 3 | | | onset | -5 | CC0 fit of dominoes, 0.14 s; 3 dB up because it is quieter at equal peak |
| tick.crisp | pro_tap/glass | 3 | | | onset | -8 | glass touch, 98 ms, 225 Hz body plus a glass partial; a crisp tick with no synth tink |
| tock | tap/knuckle | 3 | | | onset | -6 | knuckle knock; the low pair of the glass tick |
| hit | notify/m0 | 2 | | | onset | -3 | lockup = one struck bell, C5 unless `edits.pitch` moves it; one attack, no lead-in, 1.9 s natural decay; tune it to the root with `edits.pitch` |
| land | pro_pop_in/deep | 3 | | | onset | -5 | low round thump, 75 Hz body, 0.18 s; felt rather than heard |
| land.light | tap/soft | 3 | | | onset | -8 | soft fingertip thump for small elements |
| pop | fit/pop/mouth_plop | 4 | | | onset | -2.5 | CC0 fit of a mouth plop, 0.28 s; no pitch glide |
| whoosh.short | fit/whoosh.short/bamboo_swish | 3 | | | peak | -9 | CC0 fit of a whoosh, 0.48 s, one smooth pass |
| whoosh.long | whoosh_big/swell | 2 | | | peak | -7 | soft swell of air (1.36 s, peak 0.44 s) when the scene moves |
| suck, riser, drop, swell | null | | | | | | |
| key | typing_key/thock | 12 | | | onset | -10 | mechanical key with a thock; a new take for each keystroke; the light and laptop members ring, so they are not used |
| key.space | fit/key.space/thock_space | 4 | | | onset | -9.5 | CC0 fit of a mechanical space bar, 0.39 s |
| key.return | typing_key/deep | 2 | 4100 | | onset | -7 | deep cap, pressed harder, own seed; 2.5 dB over the space bar |
| click | fit/click/floss_lid | 2 | | | onset | -4.5 | CC0 fit of a small plastic click, 0.13 s, centroid 4.9 kHz |
| click.confirm | pluck/kalimba_soft | 1 | 8101 | edits.pitch C6 | onset | -10 | soft tuned pluck as confirmation; the bell and text voice, quieter |
| snap | fit/snap/switch_flip | 2 | | | onset | -3.5 | CC0 fit of a switch, 0.19 s, one attack |
| bell | pluck/kalimba | 1 | | pitches G4 A4 C5 D5 E5 G5 A5 C6 D6 E6 | onset | -5 | kalimba over 2 octaves of C pentatonic (10 notes); one note for each progress step or motif note |
| text | pluck/kalimba_soft | 1 | | pitches C5 D5 E5 G5 A5 | onset | -12 | a single quiet tuned note each time a text fragment completes (never per word); under the picture |

Off reasons (facts): `suck` puts a build on top of the reveal. `riser`: the
reveal lands on one sound; `pro_riser/air_gate` is the choice when a film
needs a build. `drop`: a calm film does not need a sub drop to resolve;
for weight, use `pro_sub_drop/soft` tuned to the root. `swell`: a reversed swell into the hit adds a second gesture to it.

### 7.3 crisp (energetic dev-tool and product films)

Top-level note facts: short, tight interface ticks and clicks; quick air for
movement; a single clean hit with a gate; a brief sub drop in tune. Off: suck, riser, swell. 64
takes.

| role | sound | takes | edits / pitches | align | level_db | facts for the note |
|---|---|---|---|---|---|---|
| tick | fit/tick/domino | 3 | | onset | -5 | CC0 fit of dominoes |
| tick.crisp | pro_click/tink | 3 | | onset | -10 | bright designed tink, 37 ms, one attack; keep it low |
| tock | tap/knuckle | 3 | | onset | -6 | knuckle knock |
| hit | pro_impact/short | 3 | | onset | 0 | one clean gated hit, one attack, no lead-in (pre-swell 10 ms), 0.67 s |
| land | pro_pop_in/deep | 3 | | onset | -4 | low round thump for heavier elements |
| land.light | fit/land.light/fingertip_book | 3 | | onset | -7 | CC0 fit of fingers on a book, 0.38 s |
| pop | pro_pop_in/round | 4 | | onset | -6 | round pop-in thump without a glide, so that a run of pops does not sound 8-bit |
| whoosh.short | whoosh_fast/tight | 3 | | peak | -6 | short quick pass (0.54 s, peak 0.16 s); for whip pans and fly-ins |
| whoosh.long | whoosh_big/swell | 2 | | peak | -6 | full air swell for scene changes |
| suck, riser, swell | null | | | | | |
| drop | pro_sub_drop/quick | 1 | | onset | -4 | shortest designed sub drop, 1.0 s, grows 110 ms after the cut; tune it to the root with `edits.pitch` (G1-C2) |
| key | fit/key/medium | 12 | | onset | -11 | CC0 fit of one mechanical key, down and up, 0.43 s, centroid 2.2 kHz; a new take for each keystroke |
| key.space | fit/key.space/thock_space | 4 | | onset | -9 | CC0 fit of a mechanical space bar |
| key.return | typing_key/clicky | 2 | | onset | -7 | clicky switch for Return; brighter and harder than the run |
| click | fit/click/floss_lid | 2 | | onset | -4.5 | CC0 fit of a small plastic click |
| click.confirm | pro_notify/ding_2045 | 1 | | onset | -9 | short clean ding, 0.36 s |
| snap | fit/snap/switch_flip | 2 | | onset | -6.5 | CC0 fit of a switch |
| bell | pluck/kalimba | 1 | pitches G4 A4 C5 D5 E5 G5 A5 C6 D6 E6 | onset | -5 | tuned kalimba over 2 octaves of C pentatonic |
| text | fit/land.light/finger_phone | 3 | | onset | -5 | CC0 fit of a finger on a phone, 0.33 s; untuned, so it cannot clash with the music; 7 dB up because it is quieter at equal peak (centroid 95 Hz) |

Off reasons (facts): `suck` puts a build on top of the reveal. `riser`: off;
`pro_riser/air_gate` is the short choice when needed, never an endless riser.
`swell`: a lead-in on top of the hit.

### 7.4 minimal (designed, sub-led, effects-only films with no music bed)

Top-level note facts: breathy digital ticks; low swells of sub for movement;
a single hit led by the sub; a designed sub drop; key ticks with the lows
removed. For the light touch, the text touch and the snap, it uses CC0 fits,
because the designed taps and the
knock they replace scored as cheap synth in the forced choice (p 0.01-0.27).
Off: suck, riser, swell. 63 takes.

| role | sound | takes | edits / pitches | align | level_db | facts for the note |
|---|---|---|---|---|---|---|
| tick | pro_hover_tick/soft | 3 | | onset | -10 | airy digital tick, 32 ms, for blinking cursors and counters; the `air` and `glass` members give click warnings |
| tick.crisp | pro_hover_tick/ping | 3 | | onset | -12 | short, high ping for exact interface ticks |
| tock | pro_tap/soft | 3 | | onset | -8 | one touch: a soft, low thump |
| hit | pro_impact/air | 2 | | onset | 0 | one gated hit led by the sub: one attack, 0.85 s, weight under 150 Hz, no layer on top |
| land | pro_pop_in/deep | 3 | | onset | -5 | low round thump, 75 Hz body |
| land.light | fit/land.light/fingertip_book | 3 | | onset | -7.5 | CC0 fit of fingers on a book, 0.38 s, centroid 240 Hz; sits under the sub-led sounds |
| pop | pro_pop_in/round | 4 | | onset | -7 | round pop-in thump without a glide |
| whoosh.short | whoosh_fast/heavy | 3 | | peak | -7 | darker fast pass, centroid 381 Hz; sits under the sub-led sounds |
| whoosh.long | pro_whoosh_big/swell | 2 | | peak | -5 | designed swell of low sub that leads into a cut (peak 0.35 s), nearly all energy under 150 Hz |
| suck, riser, swell | null | | | | | |
| drop | pro_sub_drop/ref | 1 | | onset | -3 | designed drop that grows after the cut; tune it to the root with `edits.pitch` (G1-C2) |
| key | pro_typing_key/glass | 12 | | onset | -12 | designed high-passed key tick for animated type; a new take for each keystroke |
| key.space | pro_typing_key/tick | 4 | | onset | -12 | softer designed tick for spaces |
| key.return | pro_typing_key/double | 2 | | onset | -10 | double tick at the end of a line |
| click | pro_click/soft | 2 | | onset | -9 | soft designed interface click, 53 ms |
| click.confirm | pro_notify/ding_1822 | 1 | | onset | -10 | short clean ding, 0.26 s |
| snap | fit/snap/switch_flip | 2 | | onset | -3.5 | CC0 fit of a switch |
| bell | pro_notify/ding_1822 | 1 | pitches A5 C6 D6 F6 G6 A6 C7 D7 F7 G7 | onset | -8 | the clean ding over 2 octaves of F pentatonic around its own 1822 Hz (10 notes); few designed notes, no ringing bell |
| text | fit/land.light/finger_phone | 3 | | onset | -8 | CC0 fit of a finger on a phone, untuned; 5 dB up because it is quieter at equal peak |

Off reasons (facts): `suck` puts a build on top of the reveal. `riser`: off;
`pro_riser/air_gate` is the short choice when needed, never an endless riser.
`swell`: `whoosh.long` (a sub swell) already gives the movement.

Source facts: two versions of the source engine rendered calm, crisp and
minimal to the same bytes (191 takes). All takes of the 3 presets are clean.

### 7.5 launch (music-led launch films; mix keys)

Top-level note facts: soft, short effects in the mid register, set against
the music bed instead of a fixed level. Each mix number is a median the
source measured on reference clips that we do not have (index, Data assets):

- `bed_db`: the level of the effects over the music during the 1 s before each effect;
- `offset_ms`: the signed time from the nearest cut to the sound (sound minus
  picture);
- `min_gap_ms`: the spacing inside runs;
- `width`: the L/R correlation;
- `level_db`: the same balance for a mixer with no bed (the bed model applied
  to the takes at -1 dBFS, the loudest role being 0 dB).

Only one gesture happens at a time: the music dips before a hit, a drop, a large whoosh or
a swell and returns with it. Nothing ducks under the effects. Off: suck. 72
takes.

| role | sound | takes | seed | pitches | align | bed_db | offset_ms | min_gap_ms | width | level_db | facts for the note |
|---|---|---|---|---|---|---|---|---|---|---|---|
| tick | pro_hover_tick/ping | 3 | | | onset | 3 | 0 | 66 | 0.1 | 0 | airy tick at 8.8 kHz, 60 ms, made fully wide; ticks come in runs about 66 ms apart; the run is felt rather than heard, with the same small margin over the bed as the light touches |
| tick.crisp | pro_click/tink | 3 | | | onset | 3 | 0 | 66 | 0.3 | -13 | bright designed tink, slightly wide |
| tock | knock/wood | 3 | | | onset | 8 | -40 | 100 | | -1.5 | mid-band wood knock, 0.15 s, 400 Hz block; knocks come about 40 ms before the cut |
| hit | cut_hit/punch | 3 | | | onset | 7.5 | 0 | | | -9 | one short punchy hit on the cut, 0.28 s, two thirds of it under 150 Hz; after the music dips |
| land | pro_pop_in/round | 4 | | | onset | 8 | 30 | 159 | | -5.5 | low, round thump for elements that pop in (124 Hz, 0.18 s); about 30 ms after the cut |
| land.light | tap/finger | 3 | | | onset | 3 | -60 | 115 | | -10 | light finger tap (150-260 Hz with a faint click), quiet against the bed; it comes about 60 ms before the cut |
| pop | ui_pop/soft | 4 | | | onset | 8.5 | 4 | 114 | | -8 | soft round mid pop (0.14 s, 550 Hz, 22 ms to the peak, 20 dB down after 75 ms); the effect used most, in runs about 114 ms apart |
| whoosh.short | whoosh_fast/crisp | 3 | | | peak | 9 | 37 | | | -5.5 | fast air whoosh, 0.37 s, 570 Hz; peak about 37 ms after the cut; `swish/bright` is the brighter 2.4 kHz option |
| whoosh.long | whoosh_big/thump | 2 | | | peak | 8.5 | 3 | | | -6.5 | large low whoosh that lands on a thump about 80 ms in and rings for about 0.35 s |
| suck | null | | | | | | | | | | |
| riser | riser/dark | 2 | | | peak | 8 | -23 | | | -3.5 | short dark riser (0.8 s; noise, sub and a stutter that gets faster; no synth sweep); peak about 23 ms before the cut, its fall goes across the cut; aligned on the peak because it rings past it; 1 or 2 in a film at most |
| swell | reverse_swell/dark | 2 | | | peak | 9.5 | 42 | | | -5.5 | dark, almost mono reverse swell, peak about 42 ms after the cut; 1 in a film at most; `reverse_swell/cymbal` is the bright, wide option |
| drop | cut_hit/kick | 2 | | | onset | 7 | 0 | | | -8.5 | short sub hit led by a kick, 0.22 s, on the cut; tune it to the root with `edits.pitch` |
| key | typing_key/clicky | 12 | | | onset | 6.5 | 8 | 87 | | -1 | real-sounding keys in the mid band (1.3-4.4 kHz); every keystroke gets a new take; about 87 ms apart in a run |
| key.space | fit/key.space/thock_space | 4 | | | onset | 6.5 | 8 | | | -1.5 | CC0 fit of a mechanical space bar |
| key.return | typing_key/clicky | 2 | 4100 | | onset | 7 | 8 | | | -0.5 | the same switch as the run, on its own seeds, slightly louder |
| click | fit/click/floss_lid | 2 | | | onset | 6.5 | -11 | 85 | | 0 | CC0 fit of a small plastic click; clicks come about 11 ms early |
| click.confirm | notify/mid_tine | 1 | | | onset | 8.5 | 0 | | | -8.5 | round mid ding (690 Hz, 32 ms to the peak); it closes a run as its accent |
| snap | shutter/dslr | 2 | | | onset | 4 | 9 | 66 | | -7 | shutter of a camera, for snap zooms and whip cuts; use `fit/snap/switch_flip` for a toggle |
| bell | mallet/glock | 1 | | G4 A4 C5 D5 E5 G5 A5 C6 D6 E6 | onset | 7.5 | -20 | 110 | 0.6 | -8.5 | gated glockenspiel note with a voice one octave lower (0.35 s), C pentatonic, made wide; notes come about 20 ms early |
| text | mallet/pluck | 1 | | C4 D4 E4 G4 A4 | onset | 4 | 0 | 110 | | -16 | one soft felt pluck for each resolved fragment, low under the bed |

The preset writes every `offset_ms` of 0 explicitly, so the lock holds
`offset_ms: 0.0` for those roles.

`off` (facts): `suck`, a reverse whoosh into a cut, puts a lead-in on top of
the hit; the dip in the music already does that job.

`mix` (keys and values are data; the `release` text is ours):

```json
{"level": "bed", "duck_db": 0,
 "stop_down": {"before": ["hit", "drop", "whoosh.long", "swell"], "depth_db": [6, 12], "length_s": [0.3, 0.5],
               "release": "the music returns with the hit"},
 "runs": {"accent_at_end": ["click.confirm", "land", "drop"]},
 "density_per_10s": {"music_led": [3, 13], "voice_over_explainer": [18, 30], "effects_led": [30, 38]}}
```

- `runs.accent_at_end`: a run of interface sounds keeps its spacing and ends
  on one accent from these roles.
- `density_per_10s`: the measured number of effects per 10 s for 3 kinds of
  film.
- Restraint in a music-led film: `riser` and `swell` are sounded (and `kit
  check` notes this), roughly once per film, where the section changes and
  the music already builds.
- Ticks: the only reference ticks came from an effects-led mix. The tick
  `bed_db` of 3 dB is a choice: the ticks are felt in the run, not heard over
  the music.
- Source facts: all 72 takes pass the audit (3 after a re-roll), and the 10
  bell notes are within 8 cents.
- The bed model is a median. Effects are 3-10 dB over the bed, and the
  spread per category is wide (for pops, 5.6-10.6 dB between the quartiles).

## 8. Constants of kits

| name | value | meaning |
|---|---|---|
| kit schema | 1 | the kit and lock schema |
| lock file name | `kit.lock.json` | the lock file name in the takes folder |
| takes maximum | 32 | takes per role |
| re-rolls | 3 | extra seeds per take (seed + 1000 j, j = 1..3) |
| end-ring limit | 0.03 s | kit check: an end-aligned sound that rings on longer than this past its peak |
| align values | onset, peak, end | |
| edit keys | dur, apex, pitch, peak_db, weight | |
| role keys | sound, takes, seed, edits, pitches, align, max_s, level_db, note, bed_db, offset_ms, min_gap_ms, width | |
| mix key ranges | bed_db 0.5-30, offset_ms -500-500, min_gap_ms 0-2000, width 0-1 | |
| max_s minimum | 0.01 s | |
| width tolerance | 0.05 | correlation tolerance |
| width bisection | 40 steps; side factor 0..1, decorrelated amount 0..4 | |
| decorrelator | all-pass delays 149, 263, 397 samples; gains 0.6, -0.55, 0.5; high-pass 250 Hz (2nd order); 5 ms end fade | |
| loudness window | 2400 samples | 50 ms at 48 kHz |
| bed span | 1.0 s | the bed level uses the 1 s before the cue |
| bed floor | -60 | a quieter bed is no bed |
| audition bed | -24 | the nominal bed in an audition |
| gain ceiling | -1 dBFS | gain_db never lifts a take's peak over this |
| audition gaps | 0.3 s after each take, 0.8 s after each role | |
| audition ceiling | -1 dBFS | |
| restrained roles | riser, swell, suck | |
| role name pattern | `^[a-z][a-z0-9_]*(\.[a-z0-9_]+)*$` | |
| presets | calm (default), crisp, minimal, launch | |
| preset take counts | calm 64, crisp 64, minimal 63, launch 72 | |
