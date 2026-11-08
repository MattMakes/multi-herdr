# Spec: bank-kit-sfx (the sound bank, `hark sfx` and film kits)

Unit: bank-kit-sfx. Ticket T-7. In this spec, "the source" is the upstream
project. Every name here is a hark name.

## 1. Overview

hark ships a bank of 252 named sound effects. A bank entry is not audio. It is
an exact render block: the recipe, its render function, every parameter, the
seed and the level. hark renders the entry again on demand and gets the same
sound. Three knobs aim a sound at the picture: `dur` (file length), `apex`
(time of the loudest moment, whoosh family only) and `pitch` (a note or Hz).

`hark sfx` lists the bank, renders one take of a sound, and audits any audio
file without a model. `hark kit` turns a film's kit file (one bank sound per
cue role) into cached takes with measured onset, peak and end times, a lock
file for the score renderer, an audit and an audition WAV. Four presets give a
starting kit: `calm`, `crisp`, `minimal` and `launch`.

## 2. Parts

The spec is larger than 60 KB, so it is split. This index holds the exact
interface, the data-asset plan, the errors, the acceptance tests, the
dependencies and the open questions. The parts hold the behaviour, the
algorithms and the constants.

| part | file | contents |
|---|---|---|
| 1 | `docs/specs/hark/bank-kit-sfx-bank.md` | bank file format, ids, entry types, meta, loading and merge, rendering a take, level and sample rate, measures (times, loudness, fingerprint), building bank files, pitch measures, the fingerprint check, bank constants, cost targets |
| 2 | `docs/specs/hark/bank-kit-sfx-sfx.md` | `sfx list`, the audit (`sfx check`) and its detectors, the drip allowance, the knob contract (`dur`, `apex`, `pitch`), `sfx render`, sfx constants |
| 3 | `docs/specs/hark/bank-kit-sfx-kit.md` | kit roles, `kit.json`, validation, plan and cache key, re-rolls, `kit render` and the lock, `kit check`, `kit audition`, `kit init`, the mix keys and mix helpers, the 4 presets as full tables, kit constants |
| 4 | `docs/specs/hark/bank-kit-sfx-catalog.md` | the 43 categories (kinds, use, target text, recipe), and every one of the 252 sounds with its measured targets, the forge04 member descriptions and profiles, the fit sources and shipping rules |
| 5 | `docs/specs/hark/bank-kit-sfx-design.md` | the sound-design rules for film effects: design chain, variation, rules per effect, placement, levels, the cheap-effect checklist, recipe requirements, the target table per category |

## 3. Interface (exact)

### 3.1 CLI: `hark sfx`

```
hark sfx list   [--category C] [--kind K] [--json] [--out F] [--threads N]
hark sfx render SOUND -o|--output WAV [--seed N] [--dur S] [--apex S]
                [--pitch NOTE|HZ] [--peak DB] [--weight W]
                [--sr {44100,48000,88200,96000}] [--json] [--out F] [--threads N]
hark sfx check  FILE [FILE ...] [--category C] [--role R] [--json] [--out F] [--threads N]
```

- `--json`, `--out F` and `--threads N` are the shared flags of the core-io
  unit: `--json` prints the full result as JSON, `--out F` also writes the JSON
  result to F, `--threads` sets CPU threads (default from `HARK_THREADS`).
- `list`: `--category` is one category (part 4, section 1). `--kind` is a cue
  kind or kit role. Text output: part 2, 2.3. Exit 0.
- `render`: `SOUND` is a bank id. `-o` is required and is the 24-bit WAV to
  write. `--seed` default = the entry's own seed (the delivered take).
  `--dur` seconds, `--apex` seconds, `--pitch` a note (`E5`, `Eb6`, `F#4`,
  `Fs4`) or Hz (`440`, `440hz`). `--peak` default -1.0 (dBFS sample peak, 0 or
  less). `--weight` a level factor relative to `--peak` (6.02 dB per doubling;
  0.5 = 6.02 dB down). `--sr` default 48000. Text output: the summary line of
  part 2, 5.2. Exit 0, also when the checks warn.
- `check`: one or more files. `--category` and `--role` give context to the
  audit (part 2, 3.2). Text output: with more than one file, the total summary
  first, then one summary line per file. Exit 0 when every file is clean,
  else 1.

### 3.2 CLI: `hark kit`

```
hark kit init     [--preset {calm,crisp,minimal,launch}] [-o|--output PATH]
                  [--force] [--json] [--out F] [--threads N]
hark kit render   KIT --out DIR [--force] [--json] [--threads N]
hark kit check    KIT [--out DIR] [--json] [--threads N]
hark kit audition KIT -o|--output WAV [--out DIR] [--json] [--threads N]
```

- `init`: `--preset` default `calm`. `-o` default `audio/kit.json`. `--force`
  overwrites an existing file. Exit 0.
- `render`, `check`, `audition`: here `--out DIR` is the takes folder, not a
  copy of the JSON result. These three commands do not take the shared
  `--out F` flag.
- `render`: `--out` is required. `--force` renders every take again. Prints
  one log line per rendered take when not `--json`, then the summary. Exit 0.
- `check`: `--out` is the folder of rendered takes; when it holds a current
  lock, the files are audited, else the takes render in memory. Text output:
  the summary, then one `note: <text>` line per note. Exit 0 when ok, else 1.
- `audition`: `-o` is required. `--out` default is a cache folder under
  `HARK_HOME` (part 3, 5.7). Writes the WAV and an index `.txt` beside it.
  Exit 0.

### 3.3 CLI: shared behaviour

- `hark sfx` or `hark kit` with no subcommand prints its help and exits 2.
- An error prints one line on stderr, `hark <cmd>: error: <ErrorType>:
  <message>`, and exits 1. With `HARK_DEBUG` set, the traceback is printed too.
  (core-io owns this rule.)
- Environment: `HARK_BANK` adds bank files (paths joined by the OS path
  separator, `:` or `;`). `HARK_HOME` holds the audition cache.
  `HARK_THREADS` sets the default thread count.

### 3.4 Maintenance CLI (bank files)

The module path is the architect's choice (Q2). Example:
`python -m hark.forge.bank`.

```
<bank-cli> build [DIRS ...] -o OUT [--pluck | --forge04] [--version TAG] [--ref-clap JSON]
<bank-cli> add-fit MODEL --id ID --source JSON [--bank FILE] [--seed N] [--vary V]
                   [--kinds K [K ...]] [--notes TEXT] [--allow-license] [--force]
<bank-cli> check [IDS ...]
```

- `build DIRS -o OUT`: build the forge file from forge final folders (part 1,
  7.1). `--pluck`: build the pluck file (7.2), DIRS not used. `--forge04`:
  build the forge04 file from the recipe palettes (7.3). `--version` sets the
  file's version tag (defaults: part 1, section 10). `--ref-clap JSON` gives
  fresh originality values `{id: cosine}` (forge04 only). On success prints
  `wrote OUT: <N> entries (<TAG>), <K> KB`.
- `add-fit`: add one fit entry (part 1, 7.4). `--bank` default = the shipped
  fit file. `--seed` default 0. `--vary` default 0.0. `--source` is a JSON
  file `{id, title, author, license, url}`.
- `check`: re-render the given ids (default all) against their stored
  fingerprints (part 1, section 9). Exit 0 when all pass, else 1.
- An error prints `bank: <message>` on stderr and exits 1.

### 3.5 Python API

Module `hark.forge.bank` (part 1):

| name | signature, short meaning |
|---|---|
| `VERSION` | str: the file versions joined by `+` (forge file first, other forge files, then by name) |
| `BankError` | subclass of `ValueError` |
| `ids()` | sorted list of every id |
| `get(id)` | a deep copy of the entry plus `id`; `BankError` with hints for an unknown id |
| `list(category=None, kind=None)` | the `sfx list` rows (alias `list_sounds`) |
| `category_info(cat)` | `{recipe, target, kinds, use}` |
| `renderer(entry)` | the raw render callable of an entry |
| `render(id_or_entry, seed=None, dur=None, apex=None, pitch=None, peak_db=-1.0, weight=None, sr=48000, checks=True)` | `(audio, meta)`: one take at its level (part 1, section 5) |
| `render_final(id_or_entry)` | the forge entry exactly as its final was written (palette gain applied) |
| `level(peak_db=-1.0, weight=None)` | the target sample peak in dBFS, with the checks of part 1, 5.2 |
| `check_sr(sr)` | `sr` or `BankError` |
| `normalize(y, peak_db)` | scale to a sample peak |
| `add_warning(checks, code, msg)` | add one warning to an audit result |
| `times(y, sr)` | `{onset_s, peak_s, end_s}` |
| `measure(y, sr)` | the take measures (part 1, 6.5) |
| `meta_of(y, sr)` | the entry meta, with the fingerprint |
| `fingerprint(y, sr)`, `compare_fp(a, b)` | the fingerprint and its comparison (part 1, 6.4 and 9) |
| `estimate_pitch(y, sr, near_hz=None)` | the pitch measures of part 1, section 8 |
| `build(dirs, out, version=None)`, `build_pluck(out, version=None)`, `build_forge04(out, version=None, ref_clap=None)` | write bank files (part 1, section 7) |
| `add_fit(model, sound_id, source, bank=None, seed=0, vary=0.0, kinds=None, notes=None, allow_license=False, force=False)` | add a fit entry |
| `check(ids=None)` | the fingerprint check report |
| `reload()` | drop the cache and read the files again |

Module `hark.forge.sfx` (part 2):

| name | signature, short meaning |
|---|---|
| `check(y_or_path, sr=None, category=None, role=None, sound=None)` | the audit result `{ok, summary, warnings, measures}` |
| `rising_glide(y, sr)`, `layers(y, sr)`, `live_ms(y, sr)` | the detectors of part 2, 3.6-3.8 |
| `sfx_list(category=None, kind=None)` | `{summary, sounds}` |
| `sfx_render(sound, out=None, seed=None, dur=None, apex=None, pitch=None, peak_db=-1.0, weight=None, sr=48000)` | `{summary, file, meta}` |
| `sfx_check(path_or_paths, category=None, role=None)` | one result, or `{summary, ok, files}` for a list |
| `write(path, y, sr)` | a 24-bit WAV, parent folder created |

Module `hark.forge.kit` (part 3):

| name | signature, short meaning |
|---|---|
| `KitError` | subclass of `ValueError` |
| `init(preset="calm", out="audio/kit.json", force=False)` | write a preset kit |
| `load(kit)`, `problems(kit)` | read and validate a kit |
| `plan(kit)`, `take_key(...)` | the planned takes and their cache keys |
| `render_take(...)` | one take with re-rolls |
| `render(kit, out, force=False, log=None)` | takes and `kit.lock.json` |
| `check(kit, out=None, log=None)` | `{ok, summary, problems, roles, notes}` |
| `audition(kit, out_wav, takes=None, gap_s=0.3, role_gap_s=0.8)` | the audition WAV and its index |
| `presets()`, `preset(name)` | preset names and one preset document |
| `bed_level`, `loud_lufs`, `over_bed`, `gain_db`, `place`, `space_run`, `corr`, `stereo_width` | the mix helpers (part 3, section 6) |

Wrappers in `hark.api`: `sfx_list`, `sfx_render`, `sfx_check`, `kit_init`,
`kit_render`, `kit_check`, `kit_audition`, with the same arguments and
results. MCP tools (owned by the surfaces unit): `sfx_list`, `sfx_render`,
`kit_render`.

### 3.6 Files

- Bank files, shipped as package data: `forge_v2.json`, `forge04.json`,
  `fit.json`, `pluck.json` (part 1, section 3). Each file records the hark
  version under the key `hark`.
- Presets, shipped as package data: `calm.json`, `crisp.json`,
  `minimal.json`, `launch.json` (part 3, section 7).
- Kit file `kit.json` (schema 1) and lock `kit.lock.json` (part 3, 3 and 5.5).
  The lock records the hark version under the key `hark`.

## 4. Behaviour

- The bank: part 1. Loading and merge (4), rendering a take (5), measures (6),
  building files (7), the fingerprint check (9).
- `sfx list`, `sfx render`, `sfx check` and the knobs: part 2.
- Kits, the mix helpers and the presets: part 3.
- What each sound is and must sound like: part 4.
- The design brief for every recipe and preset choice: part 5.

## 5. Algorithms

- Take times, loudness and fingerprint: part 1, section 6.
- Pitch measures: part 1, section 8.
- Audit rules and the glide, layer and live-time detectors: part 2, 3.3-3.9.
- Knob validation and the `dur`, `apex` and `pitch` contract: part 2, section 4.
  The knob engine itself belongs to forge-core.
- Kit plan, cache key and re-rolls: part 3, 5.1-5.3.
- Mix helpers (`bed_level`, `gain_db`, `place`, `space_run`, `stereo_width`):
  part 3, section 6.

## 6. Constants

Part 1, section 10 (bank), part 2, section 6 (sfx), part 3, section 8 (kits).
Main values: sample rate 48000; allowed rates 44100, 48000, 88200, 96000;
default peak -1 dBFS; onset at -30 dB of the 1 ms envelope maximum; end at the
last 5 ms frame within -50 dB; fingerprint 50 ms frames to -60 dB; check
tolerance: length exact; peak time +-5 ms; envelope +-0.5 dB; pitch
tolerance 50 cents; up to 3 re-rolls on seed + 1000 j; takes 1..32.

## 7. Data assets

| asset | status | how we make it | fallback |
|---|---|---|---|
| `forge_v2.json` (183 entries) | REGENERATE | Run our forge (forge-core and the recipe units) for every id of part 4, section 2, and build the file with `build`. Each regenerated entry must keep its id, its kinds and its channel count, pass `sfx check` clean in its category, and land near its row in part 4: `dur s` within 10 % (exact when the category's `dur` mode is `param` or `shape`), `peak s` within 20 ms or 10 % of `dur s` (the larger), centroid within a factor of 1.26 (a third of an octave), `LUFS-M max` within 3 dB, stored `note Hz` within 50 cents where the row has one. | When a recipe cannot reach a row, ship the entry with our closest render and list it in the build log. A missing id is not allowed: kit presets name these ids. |
| `forge04.json` (51 entries) | REGENERATE (render blocks), NEEDS-DATA (originality values) | Build with `build --forge04` from our recipe palettes, with the members, seeds and kinds of part 4, section 3. Each member must fit its description, reach its profile in part 4, 3.2 within the same tolerances as above (plus `glide st` within 1 st and `corr` within 0.1), and pass the take audit (5 of 6 seeds clean at least). The originality value needs a private set of licensed reference clips that we do not have. | Write `original = {made: "recipe", max_ref_clap: null, ref: "none"}` and let `build --forge04` accept a null value only with this marker. Or, when Q1 allows it, measure CLAP against a public CC0 reference set and record its name in `ref`. Never copy the source's numbers. |
| forge04 recorded originality table and the digests of the renders they hold for | NEEDS-DATA | They describe the source's renders, so they do not apply to ours. | Do not ship them. Our build records a value only when it measures one (see above). |
| `pluck.json` (3 entries) | REGENERATE | `build --pluck` from our pluck recipe default, with the edits and seeds of part 1, 7.2. | none needed |
| `fit.json` (15 entries) | REGENERATE | Download the 15 CC0 recordings named in part 4, 5.1 (Q5), fit each one with the fit unit, and add it with `add-fit` at `vary` 0.3, only when it meets the shipping rules of part 4, 5.2. | When a recording cannot be fetched or its fit fails the rules, leave that id out and use the forge replacement of part 3, 7.1 in the presets that name it. The preset must still validate. |
| presets `calm`, `crisp`, `minimal`, `launch` | DERIVE | Write them from the tables of part 3, section 7. | none needed |
| `launch` mix numbers (bed levels, offsets, gaps, widths) | NEEDS-DATA (to re-measure) | They are medians the source measured on private reference films. | Use the numbers of part 3, 7.5 as constants. They are facts about level and timing, not copied text. |
| category text (`use`, `target`) | DERIVE | Our own words in part 4, section 1. | none needed |
| design rules | DERIVE | Part 5, in our own words. | none needed |

## 8. Errors and edge cases

All errors are `ValueError` subclasses: `BankError` (bank), `KitError` (kit),
`KnobError` (knob engine, forge-core). The CLI prints them as in 3.3.

Bank and render:

1. Unknown id: `BankError` with up to 4 close ids, else up to 8 ids of the
   same category, then the id count and the command `hark sfx list`.
2. A file in `HARK_BANK` that is missing or unreadable: `BankError` that names
   the path. A file without an `entries` object: `BankError` with the path.
3. An entry of an unknown `type`: `BankError` with the id and the type.
4. A render that is silent (largest sample under 1e-7): `BankError`.
5. `peak_db` not a number, or over 0: `BankError`. `weight` not finite, not in
   (0, 4], or a result over 0 dBFS: `BankError`.
6. `sr` not one of the 4 rates: `BankError` that lists them.
7. `render_final` on a non-forge entry: `BankError`.

Knobs (part 2, section 4):

8. `apex` on a sound outside the whoosh family: refused with the reason.
9. `dur` outside 0.01-60 s, or over 4.4 s on the whoosh family; `apex` outside
   0.01-4.3 s or not under `dur - 0.02`: refused.
10. `pitch` on a noise sound, a member whose tonal layer is off, a click or
    toggle with no clear partial, a fit with no note, a category with no pitch
    parameter, or a target outside the range: refused with the reason. A
    boolean, or a value that is neither a note nor Hz in 8 Hz-20 kHz: refused.

Builds:

11. `build`: any refused pick refuses the whole file, and nothing is written.
    Two picks with one id are refused. A pick whose re-render differs from its WAV by
    more than one 24-bit step is refused.
12. `build --forge04`: a member whose default take has a warning, or that has
    under 5 clean takes of 6, or with a missing, stale or over-limit originality
    value (except the null marker of section 7) is refused.
13. `add-fit`: a bad id pattern, missing source keys, a non-CC0 licence
    without `--allow-license`, or an id already in the file without `--force`:
    `BankError`.

Kits:

14. A kit file that is not JSON: `KitError` "not JSON". An invalid kit:
    `KitError` "invalid kit: " plus every problem joined by `; ` (the checks of
    part 3, section 4).
15. Two take files that collide, also by case only: a problem.
16. `kit init` with an unknown preset (lists the presets), or onto an existing
    file without `--force`: `KitError`.
17. `kit audition` with no sounded role: `KitError` "the kit sounds no role".
18. `bed_level` with no whole 50 ms block before `t`: `KitError`. Under -60
    LUFS it returns None (no bed).
19. `kit check` never raises for a bad take: it reports a `render` warning on
    that take and goes on.

Edge cases:

20. A cached take whose file is missing or whose SHA-1 differs is rendered
    again.
21. `kit render` removes only plain file names that the old lock named and the
    new lock does not. It touches nothing else in the folder.
22. A mono take gets 2 identical channels for loudness and for `width`.
23. A take name without pitches is always numbered (`hit_1.wav`, also with 1
    take). With pitches it is numbered only when takes > 1.
24. `edits.pitch` is not part of the cache key edits: the pitch is its own key
    field.
25. On another machine, a render may differ by tiny amounts. The fingerprint
    tolerances, not hashes, decide.

## 9. Acceptance tests

Each test runs on the shipped bank unless it says otherwise. "Measure" means
the measures of part 1, section 6.

1. `ids()` returns 252 sorted ids. The counts per file are 183, 51, 15 and 3.
   `VERSION` joins the 4 file tags in the order of part 1.
2. `bank check` (all ids) passes on the build machine: every entry within the
   fingerprint tolerances.
3. `hark sfx render whoosh_fast/crisp -o w.wav` writes a 48 kHz 24-bit WAV
   whose length equals the entry's `meta.dur_s` and whose peak time is within
   5 ms of `meta.peak_s`. Sample peak -1.0 dBFS (+-0.05).
4. `--dur 0.5` on a `param` or `shape` sound gives exactly 24000 samples at
   48 kHz.
5. `--apex 0.3 --dur 0.8` on a whoosh-family sound gives a take whose measured
   peak is within 5 ms of 0.3 s.
6. `--apex 0.3` on `pro_click/glass` exits 1 with a `KnobError` that names the
   whoosh family.
7. `--pitch E5` on `pluck/kalimba` gives a take whose measured pitch is within
   50 cents of 659.26 Hz.
8. `--pitch C5` on `whoosh_big/deep` is refused with the noise-sound reason.
9. `--peak 0.5` is refused (`BankError`, the take would clip).
10. `--weight 0.5` gives a sample peak of -7.02 dBFS (+-0.05).
11. `--sr 44100` gives a file of `round(dur_s * 44100)` samples (+-1) at
    44100 Hz. `--sr 22050` is refused by the argument parser (exit 2).
12. `hark sfx list --kind whoosh.long` lists exactly the 15 members of
    `whoosh_big` (including the 3 thump whooshes) and `pro_whoosh_big`, and the
    summary gives that count.
13. `bank.get("click/sotf")` (a typo) raises `BankError` whose message
    suggests `click/soft`.
14. A synthetic sine that glides from 200 Hz to 800 Hz over 0.3 s gives the
    `rising_glide` warning, with no context and also with `--category drip`
    (24 st is over the drip allowance). A sine that glides from 400 Hz to
    800 Hz in its first 0.1 s and then holds for 0.1 s, checked with
    `--category drip`, gives a `rising_glide` note (drip allowance) and `ok`
    true. Without the category it gives the warning.
15. Two noise bursts 60 ms apart, checked with `--role hit`, give the `stacked`
    warning. Without the role, they do not.
16. 0.5 s of digital silence gives `silent` with "digital silence". A 1 kHz
    sine at -6 dBFS with a DC offset of 0.1 gives `dc`. A 0.3 s 1 kHz sine at
    -6 dBFS whose last sample is at its peak gives `edge`.
17. `hark sfx check` on 2 files, one clean and one silent, prints a total line
    and 2 file lines and exits 1.
18. `hark kit init` writes `audio/kit.json` from `calm`. A second run without
    `--force` exits 1 with `KitError`. With `--force` it overwrites.
19. `kit init --preset nosuch` exits 1 and lists the 4 presets.
20. `hark kit render audio/kit.json --out takes` renders 64 takes for `calm`.
    A second run reports 64 cached, 0 rendered.
21. Change one role's sound and render again: only that role's takes render;
    the old files of that role that the new lock does not name are removed; the
    summary counts them as stale removed.
22. Delete one take file and render again: only that take renders.
23. A kit with one role `pitches: ["C#5", "Db5"]` fails validation with the
    "one pitch" problem.
24. A kit with roles `Hit` and `hit` (both sounded) fails validation with the
    case-insensitive collision problem.
25. A kit that sounds a `riser` role gets a `kit check` note that the calm,
    crisp and minimal presets keep it off.
26. `gain_db` with bed -24 LUFS, `bed_db` 8.5, take `l50_lufs` -15 and
    `peak_db` -1 gives -1.16. With `l50_lufs` -30 it gives 0.0 (capped so the
    take peak stays at -1 dBFS).
27. `place(take{onset_s: 0.012}, role{align: onset, offset_ms: -40}, 2.0)` gives
    1.948.
28. `space_run([1.0, 1.05, 1.3], 120)` gives `[1.0, 1.12, 1.3]`.
29. `stereo_width` with target 0.6 on a decorrelated stereo noise and on a
    mono noise gives a measured correlation within 0.05 of 0.6 in both cases.
    `info.ok` is true.
30. `bed_level` of 2 s of silence at `t = 1.5` returns None.
31. `hark kit audition audio/kit.json -o a.wav` writes `a.wav` and `a.txt`;
    `a.txt` has one line per take, in role order.
32. `HARK_BANK=/no/such.json hark sfx list` exits 1 with a `BankError` that
    names `/no/such.json`.
33. Every preset validates with `problems()` = `[]`, and the take counts are
    calm 64, crisp 64, minimal 63, launch 72.
34. Every one of the 252 default takes passes `sfx check` in its category,
    except the known warnings of part 2, 3.11.

## 10. Dependencies on other units

| unit | what this unit needs from it |
|---|---|
| core-io | audio load and resample, the result emitter (`--json`, `--out F`), the error line and exit codes, `HARK_HOME`, `HARK_THREADS`, `HARK_DEBUG`; `hark doctor` reports the bank `VERSION` and the counts |
| listen | the text-audio match and the realness measure used to choose preset members (part 3, 7.1); CLAP for the fit shipping rules and for originality |
| timeline | the score renderer reads `kit.lock.json` and places takes with `place`, `gain_db` and `space_run` |
| forge-core | the knob engine (`dur`, `apex`, `pitch`, `describe`, `KnobError`); the polish detectors (clicks, ring, truncated, tail, attack, aliasing); the K filter; the render-block schema and its tolerance; the seeded random source and Butterworth filters; the all-pass used by `stereo_width`; the tonal profile measure |
| recipes-impacts | the recipes click, cut hit, drip, impact, key press, knock, mallet (with its run render), pluck (and its parameters), pop (with its second-version render), pop-in, shutter, sub drop, thump whoosh, toggle, typing (with its typed-run render), UI pop, and their palettes |
| recipes-motion | the recipes bright swish, burst glitch, dark riser, downlifter, glitch, hover tick, notify chime, reverse cymbal, riser, shimmer, sting, swipe, swish, whoosh, zip, and their palettes |
| bank-kit-sfx | this unit. The bank serves the kits, and the kits read the bank; no other spec owns either. |
| fit | the fit renderer in take mode, model load and save, the model kind, the pitch knob of a fit model |
| surfaces | the MCP tools `sfx_list`, `sfx_render`, `kit_render`; the gallery that plays bank sounds |
| skills-and-docs | the user docs and skill text about the bank, kits and presets |

## 11. Open questions

| code | question | current choice in this spec |
|---|---|---|
| Q1 | The forge04 originality value needs a reference set we do not have. Do we ship a null value with a "no reference set" marker, or measure CLAP against a public CC0 set? | Null with the marker (section 7). |
| Q2 | Where does the maintenance CLI live (module path, or a hidden `hark` subcommand)? | A module entry point, for example `python -m hark.forge.bank`. |
| Q3 | Do we keep the source's sound ids, including the `pro_` prefixes, as hark's public ids? | Kept. They are public interface, and the presets and users name them. |
| Q4 | Which version tags do the regenerated bank files carry? | Our own tags (for example `forge_v2h1`, `forge04h1`, `fit1h`, `pluck1h`), with the same order rule for `VERSION`. |
| Q5 | Regenerating the fit file needs a network download of 15 CC0 recordings from Freesound. Is that allowed, and who runs it? | Allowed only for the 15 URLs of part 4, 5.1, run once by a maintainer, the audio not committed. |
| Q6 | Do we expose the 4 tonal knobs (decay, onset, bright, width) in `sfx render` and kit `edits`? | No, as in the source. |
| Q7 | Four stereo `sines` fit entries keep a side channel that is too quiet. Do we fix it in our regeneration (preset takes change) or keep it? | Fix it, since our takes are new anyway. |
