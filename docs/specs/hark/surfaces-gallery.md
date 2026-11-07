# hark spec: surfaces, part G (gallery)

Part of `docs/specs/hark/surfaces.md`. The interface of `hark gallery` is in the index, section 1.1.
Section numbers here start with G.

## G1. Behaviour

### G1.1 Purpose

`hark gallery` writes 1 static HTML page that lets a person listen to forge results. The page needs no
server, no network and no outside script. It holds inline CSS and 1 short inline script. Every number on the
page is measured on the original sound files, also when the players play compressed previews.

### G1.2 Inputs: 1 version

`FOLDER` is resolved to `{category: sound folder}` by `discover(folder, sub)`:

1. If the folder does not exist, fail (file not found).
2. If the folder itself holds sound files, it is 1 category. The category is its name, or the name of its
   parent when its own name starts with `final` (case-insensitive).
3. Else if `<folder>/<sub>` holds sound files, it is 1 category named after `<folder>`.
4. Else every child `c` (sorted by name) whose name does not start with `_` or `.` and whose `<folder>/c/<sub>`
   holds sound files is 1 category `c`.
5. If nothing was found, fail with a message that names the folder and the pattern `<category>/<sub>/*.wav`.

Sound files of a folder: the files whose lower-case names end in `.wav` or `.flac`, except files whose name,
with leading `_` removed, starts with `audition`. They are sorted by full path. Paths use forward slashes.

The single version gets the label `<sub>` (default `final`). `--alias` renames the found categories.

### G1.3 Inputs: `--compare`

Each `--compare` item is `LABEL:GLOB`. The label is the text before the first `:` when it is at least 2
characters from `[A-Za-z0-9_.+-]`. A 1-letter prefix is a drive letter, not a label (`D:/x` has no label). An
item without a label fails with a message that shows the expected form.

For each item:
1. Expand the glob (sorted). If the glob matches nothing and the text is a folder, use that folder.
2. Skip hits that are not folders.
3. If a hit holds no sound files but `<hit>/final` does, use `<hit>/final`.
4. If the folder holds sound files and its category (rule of G1.2 step 2) does not start with `_` or `.`, add
   it under the aliased category name. The first folder for a category wins.

1 label can repeat. Its folders merge into 1 version. Versions keep the order of first appearance. If no
version has any category, fail with a message that lists the items.

`--alias OLD=NEW`: every item must contain `=` with non-empty text on both sides (spaces trimmed), else fail.
The alias applies to the category name of a folder before it is stored.

### G1.4 Families and order

Cards are grouped into 6 families, then `other`. The family key, a title and a 1-line description are shown.
The member lists set the display order. hark writes its own titles and lines.

| key | member categories, in display order | subject |
|---|---|---|
| `whoosh` | whoosh_fast, whoosh_big, pro_whoosh_big, swish, swipe | moving air: transitions, pass-bys, slides |
| `sweep` | riser, pro_riser, downlifter, pro_zip | builds into a cut and falls after it |
| `impact` | impact, pro_impact, sub_drop, pro_sub_drop, logo_sting, pro_logo_sting | hits on the cut, low weight, end stings |
| `ui` | click, pro_click, tap, pro_tap, pro_pop_in, toggle, hover_tick, pro_hover_tick | interface clicks, taps, toggles, hover ticks |
| `tonal` | pop, pro_pop, notify, pro_notify, shimmer, pro_shimmer | short pitched accents |
| `digital` | glitch, pro_glitch, typing_key, pro_typing_key, pro_typing_run | digital stutters and key presses |
| `other` | everything else | categories outside the 6 families |

Family of a category: (1) the family whose list holds it; (2) else the family whose list holds the name with
a leading `pro_` removed; (3) else the first family in this keyword order whose words occur in the name:

| family | keywords |
|---|---|
| whoosh | whoosh, swish, swipe, swoosh |
| sweep | riser, downlifter, zip, sweep |
| impact | impact, sub_drop, boom, sting, hit |
| ui | click, tap, toggle, hover, pop_in, button |
| tonal | pop, notify, ding, chime, shimmer, bell |
| digital | glitch, typing, key, stutter |

(4) else `other`. Card order: categories in a family list come by (family index, list index). All others come
after them, sorted by name. Families are shown in table order; a family without cards is not shown.

Category description (shown under the card title): (1) a fixed description for `pro_` categories (table G4.3);
(2) else the forge's category description (unit forge-core, the reference-set registry module); (3) else, for
`pro_X`, the text `designed version: ` plus the description of `X`; (4) else none. If the registry module is
not importable, steps 2 and 3 find nothing.

Display name: the category with a leading `pro_` removed, `_` replaced by spaces, first letter upper case.

### G1.5 Per version and category: report data

For each version and category, `<folder>/report.json` is read (a forge report). Missing or unreadable means no
data. From the report:

- picks: each entry of `picks` keyed by the base name of its `file`, else `<name>.wav`;
- `target` (critic target text), `mode` (from `mode`, else `critic.mode`), `tag`, `refs` (list of paths);
- recipe text: `recipe`, else `recipe_module`; when `recipe_module` is set and differs from the recipe, the
  module wins; when `kind` is set and differs from the recipe, the text becomes `"<recipe> (<kind>)"`;
- method text: `method`, else `summary`.

Variant name of a pick: the first non-empty string among the pick's `member`, `family`, `preset`, `style`,
`variant`.

Critic numbers of a pick (a value counts only when it is a finite number that is not a boolean):

| field | from |
|---|---|
| `total` | `score.total` |
| `passes` | `score.passes`, only when boolean |
| `match_z`, `env_pen`, `polish_pen`, `art_pen` | `score.<same>` |
| `tone_db` | `score.tone_db`, else `measured.tone_db` |
| `t40_ms` | `measured.t40_ms` |
| `t40_ref` | `measured.t40_ref` |
| `nearest_ref` | `score.nearest_ref`, else `measured.nearest_ref` |
| `sounds_like` | the first 3 strings of `score.sounds_like` |
| `words` | `score.polish_words`, else `measured.polish.words`, else empty |
| `robust_mean`, `robust_pass` | `robust.mean`, `robust.pass_rate` |
| `audioset` | the first element of the first entry of `holdout.audioset_top` |

Each sound also gets `params` (the pick's `params` when it is a dict, else empty) and `seed`.

### G1.6 Measurements per sound

Each sound file (made sound or reference) is decoded at its own rate as (frames, channels) float32. The
decode is cached per path for the build. The SHA-1 of the file bytes is kept. A file that cannot be read adds
the warning `<file>: unreadable (<error type>: <message>)` and is left out.

Measured fields: `dur_s` (frames / rate, 3 decimals), `peak_dbfs` (20 log10 of the absolute sample maximum, 1
decimal; null when the maximum is 0), `lufs` (momentary maximum, G2.1; null for an empty file), `channels`.

### G1.7 Benchmark results (`--results`)

A results JSON comes from the forge benchmark (unit forge-core). It must be a JSON object with `categories`,
else fail with a message that names the file. Top-level `tag` and `created` (`YYYY-MM-DD HH:MM`) are read.
Per category the index keeps: `made` entries keyed by the base name of `file` (each entry also gets
`inside_n` = the entry at the same index in `inside.per_sound`, or null), `summary`, `inside`, `n_bench`,
`n_forge`.

Labels: `LABEL:PATH` applies to version LABEL. A label that matches no version fails with a message that lists
the labels. With more than 1 version, an unlabelled results file fails (the same file names exist in every
version). With 1 version, an unlabelled file applies to it.

Per sound, the benchmark entry is the made entry with the same base name. Then the staleness test (G2.5)
runs. When the reason is not "file newer", hark tries a rename match: among all made entries of the category,
keep those that pass the staleness test with a length tolerance of 0.0015 s. If exactly 1 remains, use it
and record `"<file> = <entry file base name>"`. A still-stale sound gets no benchmark numbers and is recorded
as `"<file> (<reason>)"`.

Warnings per version and category:
- renames: `<cat> <label>: <n> file(s) renamed after the benchmark run, matched by length and peak: a; b`;
- stale: `<cat> <label>: benchmark numbers not shown for <n> sound(s) that changed after the run: ` plus
  the first 3 records joined by `; `, and ` ...` when there are more than 3.

(The implementer writes its own wording. The content above is required.)

Borrowing. When a version has no results of its own for a category, hark looks at every other results file
in order. For each sound: the other file's made entry with the same base name counts when its `file` field
names this exact path (G2.6) and it passes the staleness test with tolerance 0.0015 s, using the other file's
`created`. Such a sound gets those numbers and `same_as = <other label>`. When every sound of the version
matched, the category summary of that other file is used too, and `bench_from = <other label>`. Stop at the
first such file.

Category summary: shown only when every sound of the version has benchmark numbers. When the version has
results but not every sound matched, the version is marked `bench_stale`.

Per-sound benchmark numbers (finite numbers only): `tone_db`, `t40_ratio`, `typ_z`, `phrase_z`, `inside`
(= `inside_n`), `n_checks` (= `inside.n_checks` of the category), `stereo_corr`, `artifacts` (list of
strings), `audioset` (the first element of the first `as_top` entry, cut at the first comma).

Summary rows (each row: label, made, forge refs, bench leave-one-out, hint). The 3 value columns come from
`summary.made`, `summary.real_cross`, `summary.real_loo`. A row is shown when at least 1 of its 3 values is a
number. A missing value shows `-`.

| row label | key | format | hint |
|---|---|---|---|
| variety | `var_cos` | 3 decimals | mean CLAP similarity of the variants to each other; a lower value means more variety |
| tail t40 | `t40_ratio` | 2 decimals + ` x` | ratio of the tail time to the bench median; 1 means equal |
| tone distance | `tone_db` | 1 decimal + ` dB` | spectral distance in 1/3-octave bands from the bench median; a lower value is closer |
| inside | `inside.per_sound`, `inside.real_per_sound`, `inside.n_checks` | `"<median> / <n>"` (median in shortest form) | median per sound of how many checks fall within the spread of the real recordings |

The `inside` row needs `per_sound` and `n_checks`. Its forge-refs column is the median of
`real_per_sound` or `-`. Its leave-one-out column is always `-`. The summary also keeps `n_bench`,
`n_forge`, `n_made` (= `summary.made.n`) and `artifacts` (= `summary.made.artifacts`).

### G1.8 Identical files across versions

After all versions of a card are read: for each version in order, a sound whose SHA-1 was seen in an earlier
version gets `same_as = <label of the first version that had it>`. This mark is set in addition to any
borrowing mark (the later write wins).

### G1.9 Variant alignment (`--compare`)

Each version gets a display order (a list of sounds, `None` = an empty slot). See algorithm G2.7.

### G1.10 References (`--refs`)

`--refs bench` fails at once: a page never carries bench references. Any value other than `forge` or an existing
folder fails.

The newest version of a card is the last version that is not missing. For it:

1. Reference paths. `forge`: the category's forge split from the reference registry (unit forge-core). If
   that raises, use the report's `refs` that exist on disk. A folder: the sound files of
   `DIR/<category>/forge`, else of `DIR/<category>`.
2. Held-out guard. Ask the forge registry which paths are held out (in any category's bench split, or a
   file whose stem is in the kit-bench stem list, table G4.4). Drop them and warn
   `<cat>: <n> held-out reference(s) dropped`. If the guard itself cannot run, use no references for the card
   and warn that the card has no references because the held-out check could not run (with the error). A page must never risk a
   held-out file.
3. Pairing. See algorithm G2.8. The list is cut to `--max-refs`. When every non-empty pair is the same file,
   the row shows that 1 reference once.
4. Each reference is measured like a sound (G1.6). It has no critic or benchmark numbers.
5. Audition reel: the first of `_audition.wav`, `audition.wav` in the newest version's folder. It goes on the
   page only when its index file (the same stem with `.txt`) can be read, every name it lists is checked,
   and none is held out. Listed names are the tokens that follow `ref` on lines of the form
   `<number> s ref <name>` (spaces allowed around the parts). A name is held out when its base name is in any
   category's bench split, or its stem is in the kit-bench list. A missing registry counts as an empty bench
   set. Each refusal adds a warning: no index file; the guard is unavailable; or it plays held-out refs (up to
   3 names listed).

The page counts references (`n_refs`). When the page is embedded and has references, the first warning says
that the file carries licensed reference recordings and must stay private, and to build without `--refs` to
share it. The page header also says to keep the page private when it has references.

### G1.11 Spectrogram thumbnails

1 time span per card (G2.4), shared by every tile of the card (made, all versions, references). Each tile gets a
PNG of 520 x 132 pixels (algorithm G2.3). Thumbnail file: `<stem>_files/<label>_<category>_<file stem>_<h8>.png`.
`<label>` is the version label, or `ref` for a reference. Characters outside `[A-Za-z0-9_.-]` become `-`, and
leading and trailing `-` are removed. `h8` = the first 8 hex digits of the SHA-1 of
`"<normalized path>|<size>|<mtime ns>|<span, 4 decimals>|<thumbnail version>"`. The thumbnail version is 6;
bump it when the drawing changes. An existing file is reused. A failure adds
`<file>: no spectrogram (...)` and the tile shows an empty box.

### G1.12 Assets, embedding and caching

Not embedded: audio and images use a relative URL from the page folder, forward slashes, percent-encoded
except `/._-~`. When no relative path exists (another drive), use an absolute `file://` URI.

`--embed`: every audio and image becomes a `data:<mime>;base64,...` URI.
- PNG: `image/png`. With a lossy codec, a JPEG copy (quality 90, optimized, progressive, no chroma
  subsampling) is used when it is smaller. Without the imaging library, keep the PNG.
- Audio, codec `flac`: decode, write FLAC 24-bit at the file's own rate, `audio/flac`.
- Audio, codec `wav`: the original bytes, mime from the extension (`audio/wav` or `audio/flac`).
- Audio, lossy codec: the encoded preview (below).

Identical audio bytes are embedded once. The first player gets an id `a<n>` (n from 1) and the source. Every
later player with the same SHA-1 gets only `data-src-of="a<n>"`. The inline script copies the source at load.
The count of such players is reported.

Lossy previews (`--embed-compressed`, or `--embed` with a lossy `--embed-codec`): ffmpeg encodes each sound
(table G4.2) at 48 kHz, the file's own channels, no metadata, no video; MP3 also writes the encoder-delay
header so players trim the delay. An encode that fails or gives no bytes is an error that names the file and
the last 300 characters of ffmpeg's error text. Missing ffmpeg is an error that suggests `--embed`. Timeout per
encode: 120 s. Cache: `<stem>_files/enc/<stem>_<sha1 first 10>_<kbps>k<ext>`, keyed by content. A non-empty
cached file is reused. A new file is written to a temporary `.part` name and then moved into place. Before the
page is written, every needed preview (made sounds, references, audition reels) is encoded with 3 worker
threads.

An MP3 preview can play some tens of ms longer than its WAV, because MP3 pads to whole frames. The source measured 183 forge
sounds at 192 kbps as a page of about 9 MB.

Links to `report.md`, `compare.png` (shown as `compare sheet`) and `../LOG.md` (the parent folder of the
sound folder) appear only when not embedded and only when the file exists. A sent single file cannot reach
them.

Prune after the page is written: delete every `.png` directly in `<stem>_files/` that this build did not use.
With a lossy codec, also delete files in `enc/` with a preview extension or `.part` that this build did not
use. Errors on delete are ignored. Report the number of thumbnails removed.

`--max-mb`: after writing, when embedded and the page is larger than the limit, warn with the size and the
limit, and suggest a lower `--kbps`, `--embed-codec opus`, or fewer categories.

### G1.13 The page, element by element

Document: `<!doctype html>`, `lang="en"`, UTF-8, a viewport meta tag for mobile, `<title>` = the page title,
one `<style>`, the body, one `<script>` at the end. Every text value is HTML-escaped (quotes too).

Title: `--title`, else hark's own default for 1 version ("hark sound gallery") or for several versions
("hark: version comparison").

1. Header.
   - `h1` with the title.
   - Lede paragraph. Facts: each sound comes from a code recipe and contains no recording; the forge tuned it
     by listening to its renders and comparing them with reference recordings. With several versions, add
     that each version is 1 row and equal variants share a column, so a reader compares old and new
     vertically. With 1 version, add that each category shows its variants in 1 row.
   - Meta line, items in order: `<n> categories`; `<n> sounds`; `critic pass <a> / <b>` (with several
     versions: `<label> <a> / <b>` per version, comma-separated); per results file `benchmark [<label>]
     <tag> (<created>, <file base name>)`; when references exist `<n> forge references` with the private
     warning; with lossy embedding `audio <CODEC> <kbps> kbps previews (numbers from the WAVs)` with a tooltip
     that every number and picture is measured on the originals; `built <YYYY-MM-DD HH:MM>` (local time).
   - Overview table (G1.13 item 6) and the legend (item 7) side by side; they wrap on narrow screens.
2. Family navigation bar. It sticks to the top of the window. 1 link per shown family to `#fam-<key>` with the
   title and the card count. At the right end, the loudness switch (item 8).
3. Family sections: `<section id="fam-<key>">` with `h2` title and the 1-line description, then the cards.
4. Card (`<article class="card" id="<category>">`):
   - head: `h3` display name; a `pro` badge for `pro_` categories (tooltip: modelled on the designed effects
     of reference launch films); the category slug in monospace;
   - description paragraph (when there is one);
   - critic target line, from the newest version that has sounds: `critic target "<target>"` and, when a
     mode is set, ` · <mode> mode`;
   - card meta line: per version either `<label>: -` (missing), or `[<label> ]critic pass <passed> / <count>`
     when at least 1 sound has a known pass value (passed = number of true values, count = all sounds), else
     `[<label> ]<count> sounds`; then the links of the newest version that has sounds; then
     `span <s> s` (2 decimals) with a tooltip naming log frequency 30 Hz to 20 kHz and the dB range;
   - benchmark block (item 5);
   - rows: per version 1 row. The rows carry a label column when there are several versions or a reference
     row. The label is the version label (or `made` with 1 version) and a sub-line: `unchanged` when every
     sound has `same_as`, else `<n> sounds`; a missing version has no sub-line and shows
     `no <label> sounds for this category`. The tiles of a row sit in a grid with `n` columns, where
     `n` = the largest of every version's display order length, the reference count and 1. Each slot of the
     display order is a tile; an empty slot is an empty dashed tile;
   - reference row (when there are references): label `ref` and the sub-line `nearest forge ref`, with
     reference tiles;
   - audition line (when allowed): a short label (made, nearest ref, alternating) and a player;
   - parameters table (item 9).
5. Benchmark block, per version:
   - when `bench_stale`: a line `[<label>: ]benchmark numbers withheld: these files changed after the run
     (see the notes at the end)`;
   - else when there is a summary with rows: a table. Header: `held-out bench` plus ` (<n> bench refs)` when
     `n_bench` is known, then `made`, `forge refs`, `bench LOO`, and an empty hint column. 1 row per summary
     row; the made value is bold. With several versions, a label above the table, plus `(files unchanged
     since the <src> benchmark run)` when the numbers were borrowed from another label.
   Blocks of several versions sit side by side and wrap.
6. Overview table. Columns: `category`; per version `[<label> ]sounds` and `[<label> ]critic pass`; and, for a
   version that has any category summary on the page, also `variety (refs)`, `tail`, `tone dB`, `inside`.
   Rows: 1 family row per family (a link to the family section, spanning all columns), then 1 row per card:
   the category as a link to its card; per version the sound count (or `-`), preceded by `= <label>` when every
   sound has `same_as`; the pass count `<a> / <n>` (or `-` when no pass value is known); then, when the
   version has benchmark columns, the made value of rows variety (plus the forge-refs value in brackets when
   it is not `-`), tail t40, tone distance and inside, or `-`.
7. Legend ("How to read it"): a definition list. Entries, in order:
   - critic: the score and pass result that the forge critic gave, measured against the forge references
     only and never the bench; scores rank variants of 1 category and do not compare across categories,
     because each category has its own references and mode;
   - tone / tail t40: critic numbers; the 1/3-octave spectral distance from the median of the forge
     references, and the time the sound takes to drop 40 dB;
   - LUFS: the highest momentary loudness, K-weighted, 400 ms window;
   - only when results are loaded: bench (recordings kept away from the forge; columns made, forge refs and
     bench leave-one-out); inside (how many benchmark checks fall within the spread of real recordings);
     `-` (no value, because the category has no bench split or the file changed after the benchmark; the
     notes list such files);
   - only with several versions: `= <first label>` (this row or sound has the same files as that version,
     either the same bytes or untouched at the path the benchmark measured);
   - spectrogram: frequency on a log axis from 30 Hz at the bottom to 20 kHz at the top, faint guide lines
     at 100 Hz, 1 kHz and 10 kHz, a 72 dB range, starting at the onset, with 1 time span per card so lengths
     can be compared.
8. Loudness switch ("match loudness"), a checkbox, on by default. It is present only when at least 1 sound
   has a LUFS value. Target T = the 10th percentile of all made sounds' LUFS values on the page, clamped to
   [-30, -14], written with 1 decimal in the element's data. Tooltip: louder sounds are turned down to T
   (whole number) LUFS momentary max; quieter ones play as they are. Effect in G2.9.
9. Parameters table: a collapsible block, closed by default, titled `Recipe and parameters (<n> sounds)`. It
   is present only when at least 1 sound has parameters. Versions with sounds only. Per version:
   - a method line: `[<label>: ]recipe <recipe or ?>` in bold, then `. <method>` when there is one;
   - with several versions, a label row spanning the table;
   - a header row: `parameter`, then per sound the last `_`-separated part of its name, plus the variant;
   - a `seed` row (`-` when unknown);
   - 1 row per parameter key, keys in first-seen order across the version's sounds; a sound without the key
     shows `-`. Value format in G2.11.
10. Warnings: when there are warnings, a collapsible block `<n> note(s) from the build` with a list of all
   warnings.
11. Footer: made with hark gallery; the audio is computed by forge recipes; plus, when references are shown,
   that licensed forge-side reference recordings are included for A/B; plus, with lossy embedding, that the
   players use `<CODEC> <kbps> kbps` previews of the WAV files; and that bench references are never shown.

Tile (made sound):
- spectrogram image (lazy loading, alt text `spectrogram of <name>`), or an empty box;
- name line: the name; when the sound has a variant and its name ends in `_<digits>`, show only the digits,
  then the variant in a muted style. Tooltip: the full name and `(<variant>)`. A pill at the right:
  `pass` (green) when passes is true, `no pass` (grey) when false. When `same_as` is set, a `= <label>` pill
  comes first (tooltip: same file as in that version, byte-identical, or untouched since that benchmark ran);
- player: `<audio controls preload="none">` with `data-lufs="<lufs 1 decimal>"` when LUFS is known, and
  the source attributes of G1.12;
- key-value grid of 3 cells: `length` (`<d> s`, 2 decimals below 10 s, else 1 decimal), `peak` (1 decimal or
  `-`), `LUFS` (1 decimal or `-`). Each cell has a tooltip with its meaning;
- when critic data exist: `critic` (total, 2 decimals), `tone` (1 decimal + ` dB`), `tail t40` (`<ms> ms`
  with 0 decimals below 1000 ms, else seconds with 2 decimals; `-` when unknown);
- when benchmark numbers exist: `bench tone` (1 decimal dB), `bench tail` (2 decimals + ` x`), `inside`
  (`<inside> / <n_checks>` or `-`). When both bench tone and bench tail are missing, show instead
  `phrase z` (2 decimals), `AudioSet` (label or `-`) and `stereo r` (2 decimals). Benchmark cells use an accent
  colour and a top rule;
- a red flag line `artifact: <list>` when the benchmark lists artifacts;
- a words line when critic data exist: the critic words (at most 220 characters) when present and not
  starting with `clean`; else `sounds like: ` and the first 2 `sounds_like` entries.

Tile (reference): a dashed tile with the spectrogram, the file name, the player and the 3 base cells only.

Layout facts (the implementer writes its own CSS): page width at most 1440 px; cards are white with a light
border and rounded corners on a light grey page; tiles in a grid with columns of at least 176 px that scroll
sideways when needed; on screens up to 760 px wide the label column stacks above the tiles, the hint column
of the benchmark table hides, tile columns are at least 200 px and snap while scrolling. The spectrogram is
shown 84 px high at full tile width. Number cells use tabular figures.

### G1.14 Inline script

3 behaviours, no outside code:
1. At load, every player with `data-src-of` gets the `src` of the element with that id.
2. When any player starts, every other player pauses (1 player at a time). Use a capturing `play` listener on
   the document.
3. The loudness switch sets each player's volume. Formula in G2.9. The switch updates the volumes when it
   changes and once at load.

### G1.15 Text summary (CLI)

Lines:
1. `gallery: <n> category|categories, <m> sound|sounds[, <r> refs] -> <html> (<size> MB[, embedded <codec>], <t> s)`
2. `  versions: <label> (<n>), ...`
3. `  thumbnails: <made> new, <reused> cached, <pruned> pruned in <assets dir>`
4. when embedded: `  embedded (base64): audio <a> MB, images <i> MB` plus `; <k> identical file(s) embedded once`
   when k > 0, plus `; previews <e> encoded, the rest from the cache` with a lossy codec
5. `  open: <file URI>`
6. `  note: <warning>` for the first 12 warnings, then `  ... <n> more notes` when there are more.

Dependencies of the gallery: numpy, scipy, a sound-file reader and writer, an image writer for PNG (no plotting
window), ffmpeg on PATH only for lossy embedding, and an imaging library for JPEG thumbnails (optional).
The gallery must not import the forge package at module load. It reads the forge's category descriptions and
the held-out guard lazily, so a page can be built while someone edits forge code. The gallery keeps its own
copy of the K-weighting coefficients for that reason.

## G2. Algorithms

### G2.1 Momentary-max loudness (`lufs_max`)

Input x: frames x channels at rate r.
1. Resample to 48 kHz when r is not 48000: polyphase, up = 48000/g, down = r/g, g = gcd(r, 48000).
2. Convert to float64. If there are 0 frames, return null.
3. K-weight each channel with the 2 biquad stages of G4.1 (direct form, initial state 0).
4. p[n] = the sum over channels of the squared K-weighted samples.
5. N = 0.4 x 48000 = 19200. If the length is below N: ms = sum(p) / N (zero padding, as a meter shows it).
   Else ms = max over every window start of (sum of N consecutive p) / N, by a cumulative sum.
6. LUFS = -0.691 + 10 log10(ms + 1e-24), rounded to 1 decimal.

### G2.2 Onset for drawing

On the mono signal m (channel mean): when m is empty or silent, start at 0. Else find the first index where
|m| > max|m| x 10^(-60/20). Start 5 ms (at the sound's own rate) before it, never below 0. Only leading silence
is skipped.

### G2.3 Spectrogram thumbnail (520 x 132, RGB)

1. m = channel mean (float64), from the onset of G2.2.
2. n_span = max(floor(span x r), 64). Cut m to n_span samples; pad with zeros to n_span.
3. nfft: 256 when span < 0.15 s; 512 when span < 0.6 s; 1024 when span < 2.5 s; else 2048. Then
   nfft = min(nfft, max(64, 2^floor(log2(len(m))))).
4. hop = clip(floor(n_span / (2 x 520)), 8, nfft/2). STFT with a Hann window of nfft, overlap nfft - hop,
   zero-padded boundaries, padded to whole frames. P = |Z|^2.
5. Frequency rows: 133 geometric edges from 30 Hz to min(20000, r/2). Row centre = the geometric mean of its
   edges. Each row = the mean of the STFT bins with frequency in [low edge, high edge) (a cumulative sum over
   bins, searchsorted on edges). A row with no bin takes the linear interpolation of the 2 nearest bins at its
   centre frequency.
6. Time columns: 521 linear edges from 0 to n_span / r. Each column = the mean of the frames whose centre
   time is in its slot, the same rule (empty slot: interpolate at the slot centre).
7. Streak smoothing. For each row k: nb = max(row width in Hz / (r / nfft), 1). When nb < 4:
   L = floor(min(520 / 16, round(4 x (nfft / r) / nb / dt_col))), where dt_col = n_span / r / 520.
   When L >= 3: convolve the row over time with a triangular window of L points (a Bartlett window of L+2
   points without its 2 zero ends), normalized to sum 1, same length.
8. dB = 10 log10(P + 1e-20). dB = max(dB - max(dB), -72). v = (dB + 72) / 72, rows flipped so 30 Hz is at the
   bottom.
9. Colour: the 6-stop linear colour map of G4.1 (paper white to blue-black ink) applied to clip(v, 0, 1).
10. Guides at 100 Hz, 1 kHz and 10 kHz, when inside the row-centre range: the row whose centre is nearest.
    Only on cells with v < 0.35: colour = colour x 0.9 + (0.80, 0.83, 0.87) x 0.1 - 0.035.
11. Output = round(clip(colour, 0, 1) x 255) as uint8.

### G2.4 Card span

d = the durations of every tile of the card (zeros and nulls removed). With no durations: 1.0 s. Else
span = max(0.05, min(max(d), 2.5 x median(d))). 1 long outlier cannot squash the rest.

### G2.5 Benchmark staleness

Inputs: entry, sound, `created`, tolerance (default 0.011 s). Return a reason, or empty when the entry
describes the file.
1. No entry: empty.
2. When `created` parses as local time `YYYY-MM-DD HH:MM` and the file mtime is later than that time + 60 s:
   `file newer than the benchmark run`. A parse or stat error skips this test.
3. When the entry `length_s` is a number and |length_s - dur_s| > tolerance: `length <dur> s, benchmark <len> s`
   (3 decimals).
4. When `artifact_detail.peak_dbfs` is a number, the sound peak is known, and they differ by more than
   0.3 dB: `peak <p> dBFS, benchmark <q>` (1 decimal).

### G2.6 Same-file test

The entry's `file` is the same path as the sound when: (absolute entry path) its normalized absolute form
equals the sound's; or (relative path) joined to the current folder, or to the folder 2 levels above the
results file, it equals the sound's normalized absolute path. Comparison is case-normalized for the platform.

### G2.7 Variant alignment

Versions in order. Each version starts with its file order. The base is the variant-name list of the first
version that has sounds. For each later version: realign only when every sound has a variant name, the names
are unique, and at least 2 names are in the base. Then:
1. slots = max(len(base), len(sounds)) empty slots;
2. each sound whose variant is in the base goes to the base index of that variant when that slot is empty;
   the others go to a rest list in order;
3. each rest sound fills the first empty slot, else is appended;
4. trailing empty slots are removed.
Result: before and after of the same variant stand in the same column.

### G2.8 Reference pairing

Over the newest version's shown sounds in display order (empty slots stay empty): when the sound's critic
`nearest_ref` is the base name of a reference path, use that path. Else use the references in order, cycling
(a counter that advances only on this branch). With no references, none.

### G2.9 Loudness switch volume

For each player with a finite `data-lufs` value l: volume = min(1, 10^((T - l) / 20)) when the switch is on,
else 1. Players without a value keep volume 1. Only the player volume changes, never the files.

### G2.10 Codec choice

1. codec = `--embed-codec`, else `flac`. It must be 1 of `flac`, `wav`, `mp3`, `opus`, `vorbis`.
2. `--embed-compressed` sets embed on; a non-lossy codec becomes `mp3`.
3. `--kbps` is allowed only with embedding and a lossy codec, else fail. It must be 16 to 320.
4. The default kbps per lossy codec is in G4.2.

### G2.11 Parameter value format

bool: `yes` or `no`. int: as is. Non-finite float: as is. Float with |v| >= 1000: 0 decimals. Other float:
4 significant digits (shortest form). List: the first 8 values formatted the same way, joined by `, `, plus
` ...` when longer. Dict: `{k: v, ...}` for the first 6 items. Else: the string form.

## G3. Errors and edge cases

| case | result |
|---|---|
| no folder and no `--compare` | error: give a folder or `--compare` |
| folder missing | file-not-found error naming the folder |
| folder without sounds in any layout | file-not-found error naming the folder and the expected layout |
| `--compare` item without a label (or a 1-letter drive prefix only) | error showing the `LABEL:GLOB` form |
| `--compare` finds no folder with sounds | file-not-found error listing the items |
| `--alias` item without `=` or with an empty side | error showing the `OLD=NEW` form |
| unknown codec | error listing the codecs |
| `--kbps` without a lossy embedded codec | error |
| `--kbps` outside 16..320 | error |
| `--refs bench` | error: a page never carries bench references (checked before and inside the build) |
| `--refs` neither `forge` nor a folder | error |
| `--results` file not a results JSON | error naming the file |
| `--results` label unknown | error listing the labels |
| unlabelled `--results` with several versions | error asking for a label per results file |
| unreadable sound file | warning, the sound is left out |
| thumbnail failure | warning, empty image box |
| lossy embed without ffmpeg | error suggesting `--embed` |
| ffmpeg encode failure | error naming the file and ffmpeg's last 300 characters |
| held-out guard unavailable | warning, no references for that card |
| audition reel without an index, guard unavailable, or held-out names | warning, reel left out |
| category missing in 1 version | that row says there are no sounds for the version; overview shows `-` |
| report.json missing | the sounds still show, without variant, critic data or parameters |
| a sound with a 0 peak | peak shows `-` |
| page over `--max-mb` when embedded | warning only; the page is still written |
| output folder missing | it is created |

## G4. Constants

### G4.1 Measurement and drawing

| name | value |
|---|---|
| analysis rate | 48000 Hz |
| thumbnail size | 520 x 132 px |
| thumbnail version (cache key) | 6 |
| dB range of thumbnails | 72 dB |
| frequency range | 30 Hz to 20000 Hz (capped at r/2) |
| onset threshold, pre-roll | -60 dB below peak, 5 ms |
| momentary window | 0.4 s |
| loudness offset | -0.691 dB |
| loudness target clamp | -30 to -14 LUFS, 10th percentile |
| span clamp | at least 0.05 s, at most 2.5 x the median |
| default span with no durations | 1.0 s |
| staleness: length tolerance, rename tolerance, peak tolerance, mtime grace | 0.011 s, 0.0015 s, 0.3 dB, 60 s |
| words line limit | 220 characters |
| CLI note lines | 12 |

K-weighting at 48 kHz (BS.1770), 2 biquads `[b0, b1, b2, a0, a1, a2]`:

| stage | b0 | b1 | b2 | a0 | a1 | a2 |
|---|---|---|---|---|---|---|
| 1 (shelf) | 1.53512485958697 | -2.69169618940638 | 1.19839281085285 | 1.0 | -1.69065929318241 | 0.73248077421585 |
| 2 (high-pass) | 1.0 | -2.0 | 1.0 | 1.0 | -1.99004745483398 | 0.99007225036621 |

Colour map stops (position, hex): 0.00 `#fbfbfa`, 0.18 `#e6eaef`, 0.42 `#a9b8c9`, 0.62 `#5f7793`,
0.80 `#283b55`, 1.00 `#070c14`.

Palette facts (optional, for a similar look): ink `#14181f`, page `#f5f5f3`, card `#fff`, accent `#27466b`,
pass `#1d7048` on `#e7f2ec`, flag `#9b3b2a` on `#f8ebe7`.

### G4.2 Lossy codecs

| codec | ffmpeg encoder | container | mime | extension | default kbps |
|---|---|---|---|---|---|
| mp3 | libmp3lame | mp3 | audio/mpeg | .mp3 | 192 |
| opus | libopus | ogg | audio/ogg | .opus | 128 |
| vorbis | libvorbis | ogg | audio/ogg | .ogg | 160 |

Every lossy preview is at 48 kHz. MP3 plays in every current browser. Ogg is smaller for the same quality but
does not play in older Safari. JPEG thumbnails are 2 to 3 times smaller than the PNGs.

### G4.3 Descriptions of designed (`pro_`) categories

hark writes its own sentences. Facts per category:

| category | fact to state |
|---|---|
| pro_whoosh_big | swell-type whooshes that build into a cut, modelled on launch-film effects |
| pro_riser | risers modelled on launch-film effects |
| pro_zip | a zip: pitch sweeps upward quickly, about 0.2 s long |
| pro_impact | hits modelled on launch-film effects |
| pro_sub_drop | a falling sub-bass tone that sits under a hit |
| pro_logo_sting | the closing sting of a launch film that has effects and no music |
| pro_click | UI clicks modelled on launch-film effects |
| pro_tap | a tap: a low thump near 150 Hz plus a quiet click |
| pro_pop_in | the effect those films use most: an element appears, with a soft low thump and a quiet tick |
| pro_hover_tick | hover ticks modelled on launch-film effects |
| pro_pop | pops modelled on launch-film effects |
| pro_notify | notification dings modelled on launch-film effects |
| pro_shimmer | shimmers modelled on launch-film effects |
| pro_glitch | glitch bursts modelled on launch-film effects |
| pro_typing_key | key presses modelled on launch-film effects |
| pro_typing_run | several key presses in a row, for on-screen typing |

### G4.4 Kit-bench stems (held out by stem)

`ui_notify_02`, `whoosh_med_01`, `whoosh_med_02`, `whoosh_big_02`, `swipe_01`, `riser_02`, `impact_03`,
`shimmer_01`. This list is owned by unit forge-core. The gallery only asks the guard.

## G5. Acceptance tests (gallery)

Fixtures are synthetic WAVs written by the test (no licensed audio).

| id | input | properties of the output |
|---|---|---|
| G-T1 | a 1 s 997 Hz sine at -20 dBFS peak, mono, 48 kHz | `lufs_max` is within 0.2 dB of the value from the G2.1 formula computed by hand; `measure` gives `dur_s` 1.0, `peak_dbfs` -20.0, `channels` 1 |
| G-T2 | a 0.1 s sound (shorter than 400 ms) | `lufs_max` uses sum / 19200 (zero-padded window): it is lower than the loudness of a 0.4 s copy of the same level |
| G-T3 | a 44.1 kHz file | it is resampled to 48 kHz before K-weighting; the result equals the result for the same sine made at 48 kHz within 0.1 dB |
| G-T4 | tree `out/click/final/{click_01,click_02}.wav`, `out/pop/final/pop_01.wav`, `out/_judge/final/x.wav` | `discover("out")` = {click, pop}; `_judge` is skipped; card order is click (ui) before pop (tonal); `ui` family shown before `tonal` |
| G-T5 | `discover("out/click/final_r2")` with sounds inside | category `click` (parent name, because the folder name starts with `final`) |
| G-T6 | `--compare "D:/x"` | error: no label (1-letter prefix is a drive) |
| G-T7 | `--compare v1:old/*/final v2:new/*/final --alias keypress=typing_key`, old has `keypress` | the v1 sounds appear on the `typing_key` card; the overview has 1 row for typing_key with both versions |
| G-T8 | 2 versions, v2 holds 1 byte-identical copy of a v1 file | the v2 tile has a `= v1` pill; when embedded, that audio is inside the page once and the second player has `data-src-of` |
| G-T9 | v1 variants [crisp, soft, deep], v2 variants [deep, crisp, new] | v2 display order is [crisp, new, deep]: crisp under crisp, deep under deep, new in the free slot |
| G-T10 | results JSON with an entry whose `length_s` differs by 0.05 s from the file, and another entry that matches length within 1 ms and peak within 0.3 dB | the sound gets the second entry's numbers; a rename warning names both files |
| G-T11 | results JSON `created` 1 hour before the file mtime | no benchmark numbers for the file; the version is `bench_stale`; the bench block says the numbers are withheld |
| G-T12 | `--refs bench` | error before any file is written |
| G-T13 | `--refs DIR` where DIR/click/forge has 2 WAVs, 1 with stem `impact_03` | only 1 reference tile; a warning says 1 held-out reference was dropped |
| G-T14 | audition.wav without audition.txt | no audition player; a warning says the index is missing |
| G-T15 | 3 sounds with LUFS -12, -18 and -24 | the switch target T is the 10th percentile with linear interpolation, -22.8, clamped to [-30, -14], so -22.8; the data value is `-22.8`; with the switch on the -12 sound plays at volume 10^(-10.8/20) and the -24 sound at volume 1 |
| G-T16 | `--embed-compressed` with ffmpeg on PATH | audio sources are `data:audio/mpeg;base64,...`; no links to report.md; `codec` is `mp3 192 kbps`; previews exist in `<stem>_files/enc/` and a second build encodes 0 |
| G-T17 | a second build after deleting 1 category | the thumbnails of the removed category are pruned; `images.pruned` > 0 |
| G-T18 | `--kbps 12 --embed-compressed` | error (outside 16..320) |
| G-T19 | the page HTML | contains no `<script src`, no `<link` to another host, and no `http` URL in any audio or image source |
| G-T20 | a 2 s sound and five 0.1 s sounds on 1 card | span = min(2.0, 2.5 x 0.1) = 0.25 s |
