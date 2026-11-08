# forge-core part 9: reference sets and the comparison picture

Index: [forge-core.md](forge-core.md). This part covers module `hark.forge.refsets` (per-category real reference
recordings split into a `forge` side and a held-out `bench` side) and module `hark.forge.viz` (one PNG that shows
each made sound beside its nearest reference).

## `hark.forge.refsets`

The reference recordings are licensed third-party sounds. They are never shipped. They live in a local registry, a
JSON file. Users bring their own refs.

Split rule: a source recording and its sister takes go to one split only, in every category. The forge tunes on
`forge` refs, and the benchmark then scores it on recordings the forge never heard.

### Interface

| name | meaning |
|---|---|
| `SPLITS` | `('forge', 'bench')` |
| `CATEGORIES` | dict of the 18 benchmark categories -> a one-line meaning (Constants) |
| `KIT_BENCH` | file stems of kit files cut from a bench source; never forge refs |
| `RefsetsMissing` | exception, subclass of `FileNotFoundError` |
| `registry_path()` | `HARK_REFSETS` (absolute) when set, else `<HARK_HOME>/refsets.json` |
| `load(path=None)` | the registry dict with absolute paths |
| `categories(path=None)` | sorted category names in the registry |
| `refset(category, split='forge', path=None)` | list of paths |
| `note(category, path=None)` | the category's note text, `''` when none |
| `held_out(paths, kit=True, path=None)` | the paths that must not be forge refs |
| `scan(root, exts=('.wav', '.flac', '.ogg', '.mp3', '.aif', '.aiff'))` | a registry dict from a folder layout |
| `check_split(reg, sources=None)` | list of problem strings (empty = fine) |
| `write(reg, path=None)` | writes the registry; returns the path |

Registry format: `{"version": 1, "categories": {"<cat>": {"forge": [paths], "bench": [paths], "note": "<text>"}}}`.
Relative paths are relative to the registry file.

### Behaviour

- `load`: a missing file raises RefsetsMissing with a message that says the refs are licensed and not shipped, and
  gives the 3 ways to get them: a folder layout `<root>/<category>/{forge,bench}/*` with `write(scan(root))`, the
  environment variable pointing at a registry JSON, or the build script (part 10). Make each split path absolute
  (normalised, `/` separators).
- `refset`: a split not in SPLITS raises ValueError. An unknown category raises KeyError that lists the known ones.
  Any listed file that does not exist raises RefsetsMissing (`<n> <cat>/<split> ref(s) missing, e.g. <path>: rebuild
  the refs`). A split may be empty.
- `held_out`: compare by normalised absolute path (case-normalised on case-insensitive systems). A path is held out
  when it is in any category's bench split (an absent registry gives an empty bench set), or, with kit, its file stem
  is in KIT_BENCH.
- `scan`: each sub-folder of root that does not start with `_` or `.` is a category; its `forge` and `bench`
  sub-folders give sorted file lists (extension match is case-insensitive); keep categories with at least one file;
  note `''`.
- `check_split`: a file in both splits of a category gives `<cat>: <basename> in both splits`. With `sources`
  (path -> source id): a source id in forge anywhere and bench anywhere gives `source <id> is forge in [cats] and
  bench in [cats]`.
- `write`: refuse (ValueError, first 3 problems) when check_split finds problems. Create the folder. Indent 1.

### Constants

`CATEGORIES` (meaning in our words, typical length):

| category | meaning |
|---|---|
| whoosh_fast | short crisp pass-by of a rod through air, 0.2-0.8 s |
| whoosh_big | big deep slower swoop, 0.8-3 s |
| swish | light thin quick swish of a thrown object, 0.15-0.5 s |
| swipe | soft cloth or paper slide, 0.25-1 s |
| riser | swell that builds to a peak, 1.5-6 s |
| downlifter | downward sweep that fades out after a hit, 1-4 s |
| impact | deep cinematic boom with a long tail, 1-8 s |
| sub_drop | sub-bass pitch drop under a hit, 1-4 s |
| click | small plastic UI button click, 30-350 ms |
| tap | soft finger tap on glass or a hard surface, 30-300 ms |
| toggle | a switch flipped, crisp, often two-part, 50-450 ms |
| hover_tick | very small soft tick for hover and scroll steps, 15-150 ms |
| pop | short round pop (bubble, mouth, cork), 50-500 ms |
| notify | short bright notification ding or chime, 0.4-2 s |
| shimmer | sparkling chime sweep or bell-tree, 1-6 s |
| typing_key | one mechanical keyboard key press, 60-450 ms |
| glitch | digital glitch or stutter burst, designed, 0.1-1 s |
| logo_sting | brief logo sound: a struck hit that rings out, 1-6 s |

`KIT_BENCH` in the source named 8 kit files of its own library. A clean build has its own kit (unit bank-kit-sfx):
keep the mechanism (a stem list of kit files cut from bench sources) and fill the list from our own kit manifest when
a kit file shares a source with a bench ref.

### Data assets

The registry `refsets.json`: NEEDS-DATA (licensed recordings we do not have). Fallback: (1) designed mode with no
refs (the critic's study targets, part 8, carry thin categories); (2) users lay out their own recordings and call
`write(scan(root))`; (3) the build script of part 10 makes the registry from any local library with per-file
manifests.

### Acceptance tests

RS1. `scan` of a temp tree `root/click/forge/a.wav`, `root/click/bench/b.wav`, `root/_x/forge/c.wav` gives one
     category `click` with 1 forge and 1 bench path.
RS2. `write` of a registry with the same path in both splits raises ValueError.
RS3. `refset('click', 'train')` raises ValueError; an unknown category raises KeyError; a deleted file raises
     RefsetsMissing.
RS4. `held_out([bench_path, forge_path])` returns `[bench_path]`.
RS5. With no registry, `load()` raises RefsetsMissing whose message names the folder layout.

## `hark.forge.viz`

### Interface

| function | returns |
|---|---|
| `onset_align(x, db=-30.0, pre_ms=5.0)` | the mono sound from 5 ms before its first sample within 30 dB of the peak |
| `spec_db(x, span_s)` | `(t_ms, f_hz, S_db)`: log-magnitude STFT, 0 dB = loudest bin, floor -80 |
| `envelope_db(x, span_s, hop_ms=2.0)` | `(t_ms, e_db)`: RMS envelope in dB re its peak, floor -80 |
| `compare_png(path, made, refs=(), pairs=None, title=None, max_s=8.0)` | `{'file', 'rows', 'pairs'}` (pairs as ref names or None) |

Needs matplotlib, imported lazily, non-interactive backend.

### Behaviour

- `spec_db`: pad or cut to span (at least 256 samples). FFT size 512 when span < 0.6 s, 1024 when < 2.5 s, else 2048;
  overlap nfft - nfft / 8 (hop nfft / 8); no boundary padding.
- `envelope_db`: pad or cut to span; frames of hop; at least 1 frame.
- `compare_png`: `pairs=None` pairs each made sound with its nearest ref by sound distance (part 7). One row per made
  sound (at least 1 row), 4 columns with width ratios 1.25, 1.25, 1, 1; figure 15 wide by 2.25 per row (+0.5 with a
  title, else +0.2); font 7.5. Made colour blue, ref colour red.
  1. The made spectrogram (onset-aligned), log frequency 40 Hz-24 kHz, -80..0 dB, time axis 0..span ms. Title
     `made: <name>`.
  2. The paired ref's spectrogram, same span and range. Title `nearest ref: <name>`. No ref: the panel is off.
  3. Both envelopes on one axis; hop = max(2 ms, span / 300) (about 300 points, so a long tail reads as a line); y
     -70..3 dB; title `envelope (dB re peak)`; legend.
  4. 1/3-octave balance: the refs' min-max band (shaded) and median (dashed), the made profile; log x 25 Hz-20 kHz, y
     -60..0; title `1/3-octave balance: <d:.1f> dB from the refs` (RMS distance to the median) or without refs
     `1/3-octave balance`; legend on the first row.
  span = min(max_s, the longer of the two aligned sounds). The x label `ms from onset` is on the last row. Save at
  90 dpi, close the figure.

### Acceptance tests

V1. `compare_png(p, [('m', x)], [('r', y)])` writes a PNG and returns rows 1 and pairs ['r'].
V2. With no refs it still writes a PNG; pairs [None].
V3. `onset_align` of 100 ms of silence followed by a click returns a signal that starts 5 ms (240 samples) before the
    click.
