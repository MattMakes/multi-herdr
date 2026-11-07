# forge-core part 10: the held-out benchmark, the blind test kit and the refset build

Index: [forge-core.md](forge-core.md). This part covers the evaluation tools of the forge. They are scripts, not
library modules. They live in `eval/forge/` of our repo:
- `bench.py`: scores made sounds against the held-out `bench` refs (part 9) with checks that no recipe optimises.
- `frozen_tone.py`: a frozen copy of the tone checks, so the benchmark does not move when part 3 changes.
- `build_refsets.py`: cuts the reference recordings and writes the registry of part 9.
- `blind_v2.py`: builds a blind listening test (made against real) and scores the answers.

Rule of the benchmark: it is held out only while it decides nothing. Never select a candidate, steer a search or tune a
recipe from benchmark numbers. Use it only to report.

## 10.1 `bench.py`

### Interface

CLI: `python eval/forge/bench.py --out FILE.json [options]`.

| option | meaning |
|---|---|
| `--made DIR` | folder of `<category>/*.wav`. When `<category>/final/` exists, read that folder instead (the forge v2 layout). |
| `--files CAT=GLOB ...` | extra made sounds per category; quote the glob |
| `--out FILE.json` | required. The results JSON. A markdown table is written beside it with the extension `.md`. |
| `--tag NAME` | name in the report. Default: the out file's stem without a `results_` prefix. |
| `--cats CAT ...` | only these categories |
| `--real-only` | no made sounds; score the real ceilings of every registry category |
| `--no-real` | skip the real ceilings (faster) |
| `--registry PATH` | the refsets registry. Default `refsets.registry_path()` (part 9). |
| `--retable` | read an existing `--out` JSON and only rewrite its `.md` table |

Functions a test can call: `env_feats(x)`, `EnvRef(refs)` with `.check(x, exclude=None)`, `artifacts(x, cat=None)`,
`diversity(xs, E)`, `texture(x)`, `inside_real(made, real, metrics=METRICS, q=0.8)`, `gather(made=None, files=None)`,
`recipe_phrases()`, `Yardstick(paths, xs, E)` with `.check(x, e, exclude=None)`, `bench_category(cat, made_paths,
reg, models, real=True, log=print)`, `table(res)`, `main(argv=None)`.

`SR = 48000`. Mono of a 2-D array = the mean of its channels.

### Shared helper: frame levels

`frames_db(m, ms)`: n = max(1, int(SR ms / 1000)) samples per frame; k = max(1, len(m) // n) frames. Zero-pad m to
k n samples and cut it to k n. Each frame's level is 20 log10(RMS + 1e-12).

### Model-free checks

`env_feats(x)` on the mono signal:
1. e1 = 1 ms frame levels. p1 = index of the loudest frame. onset = the first frame with e1 >= e1[p1] - 20.
2. e5 = 5 ms frame levels. last = the last 5 ms frame with e5 >= max(e5) - 40.
3. Return: `onset_s` = onset / 1000 (4 decimals); `attack_ms` = max(1, p1 - onset) as float; `t40_ms` = the frozen
   tone copy's `t40_ms(m)` (10.2); `active_s` = max(0.005, (last + 1) x 0.005 - onset / 1000) (3 decimals);
   `peak_s` = p1 / 1000 (4 decimals).

`curve(x, pre, post)`: 10 ms frame levels of the mono; p = the loudest frame. Make it relative to the peak and floor it
at -60 dB. Place it into an array of length pre + post filled with -60 so that the peak frame lands at index pre.
Frames outside the array are dropped.

`EnvRef(refs)` (the benchmark's own envelope reference, separate from part 3):
- Load each ref (a path is read through core-io audio load; an array is used as it is). Store `env_feats` of each.
- PRE = 10 frames (100 ms before the peak). tail = the max over refs of (onset_s + active_s - peak_s), default 0.1
  with no refs. post = int(min(800, max(10, ceil(1.25 x tail / 0.01)))).
- C = the stacked curves of the refs (or None with no refs).
- `check(x, exclude=None)`: keep = every ref index except `exclude`. With none kept return `{}`. f = env_feats(x).
  For each of attack_ms, t40_ms, active_s: med = the median over kept refs; floors 1.0, 5.0, 0.01;
  log = log2(max(f, floor) / max(med, floor)). Return:
  - `attack_ms`, `t40_ms`, `active_s`, `onset_s` of x.
  - `env_oct` = mean of |log| over the 3 measures (3 decimals). A value of 1.0 means the sound is typically 2x off.
  - `curve_db` = RMS of (curve(x) - median over kept refs of C), 2 decimals.
  - `t40_ratio`, `attack_ratio`, `active_ratio` = 2^log (3 decimals).
  - `ref_median` = the 3 medians (3 decimals).

`ENDS_ON_PEAK = ('riser', 'pro_riser')`: these grow out of silence and stop on their peak.

`artifacts(x, cat=None)`: pk = max |mono| + 1e-12. Numbers:
- `clipped_samples` = the count of samples (all channels) with |v| >= 0.999.
- `dc_db` = 20 log10(|mean of mono| / pk + 1e-12), 1 decimal.
- `first_sample` = |mono[0]| / pk, 3 decimals.
- `tail_db` = the 10 ms level of the last 10 ms minus max(e5), 1 decimal.
- `last_ms_rel` = RMS of the last 48 samples / pk, 3 decimals.
- `lead_s` = (the first 1 ms frame within 50 dB of the max 1 ms level) / 1000, 3 decimals.
- `peak_dbfs` = 20 log10(max |x| + 1e-12), 2 decimals.

Flags, in this order: `clipped` (clipped_samples >= 3), `dc offset` (dc_db > -30), `start click` (first_sample > 0.05),
`end click` (last_ms_rel > 0.02), `tail cut` (tail_db > -30), `late start` (lead_s > 0.1). The last 3 are not raised
for a category in ENDS_ON_PEAK. Calibration in the source: 5 of 150 real trims raised one flag, all of them ticks cut
from a flurry or plain 5 s windows.

`diversity(xs, E)` among one category's variants (E = their CLAP embeddings):
- n < 2: `{'n': n}`.
- Normalise each embedding to unit length (+1e-12). P = frozen tone profiles. For every pair i < j: cos = E_i . E_j,
  td = RMS(P_i - P_j).
- Return n, `cos_mean` (3 decimals), `cos_max` (3), `tone_min_db` (2), `tone_mean_db` (2), and `near_duplicates` =
  the pairs with cos > 0.985 and td < 1.5.

`texture(x)`:
- `floor_db` = max(the 10th percentile of e5 - max(e5), -120), 1 decimal. -120 means digital silence.
- `stereo_corr`: for a 2-channel file, the Pearson correlation of the mean-removed channels (1.0 when the denominator
  is 0); for mono 1.0.
- `darken`: t40 = the frozen `t40_ms(mono)`; n = 20 ms; p = the index of max |mono|; late = p + int(t40 / 2000 x
  SR) (half the tail). Only when t40 >= 40 and late + n <= len: centroid = sum(F f) / (sum F + 1e-12) of the
  Hann-windowed magnitude spectrum of an n-sample segment; darken = log2(max(centroid(late), 20) / max(centroid(p),
  20)), 2 decimals. Negative means the decay darkens (usual for real decays).

### Model checks

`Yardstick(paths, xs, E)`: one ref set. It holds the CLAP embeddings E, a frozen `ToneRef(xs)` and an `EnvRef(xs)`
(None with no refs). `check(x, e, exclude=None)`: keep = indices except exclude.
- With 3 or more kept: `typ_z` = the typicality z of e against E[keep] with k = 3 (unit listen, compare typicality),
  2 decimals.
- With 1 or more kept: `nn_cos` = max(E[keep] . e) (3 decimals); from the tone check: `tone_db`, `tone_ratio`,
  `n_outside` (the number of outside bands), `outside` (the 3 with the largest |dB|); and every key of the EnvRef
  check.

`bench_category(cat, made_paths, reg, models, real=True, log=print)`, with models = (the CLAP vocab model, the AudioSet
tagger) from unit listen:
1. B = the bench split, F = the forge split of the category (empty lists when the category is absent). allp = made +
   B + F. None at all: return `{n_bench 0, n_forge 0, note, made [], summary {}}`.
2. Load every file. E = CLAP file embeddings of allp.
3. phr = PHRASES[cat], conf = CONFUSIONS[cat]. When phr is not empty, Z = the calibrated z of every file against the
   text embeddings of phr + conf (unit listen `calibrate`, key `z_q`).
4. labs = the category's AS_LABELS that exist in the tagger's index.
5. A yardstick for the bench refs and one for the forge refs.
6. `row(i, ys, exclude)`:
   - `file` (with `/` separators), `length_s` (3 decimals), and ys.check when ys has paths.
   - With Z: zp = the phrase columns, zc = the confusion columns, best_c = max(zc) (or -9 with no confusions).
     `phrase_z` = mean zp (2 decimals), `phrase_min` = min zp, `margin` = mean(zp - best_c), `wins` = the share of
     phrasings with zp >= 2.5 and zp > best_c, `best_confusion` = the confusion text with the highest z.
   - AudioSet: p = the file's class probabilities; `as_top` = the top 3 (label, prob); `as_names` = any top-3 label is
     in labs; `as_p` = the max probability over labs (0.0 with none), 3 decimals.
   - Every key of `texture(x)`; `artifacts` = the flag list; `artifact_detail` = clipped_samples, dc_db,
     first_sample, tail_db, lead_s, peak_dbfs.
7. `made` = rows of the made sounds against the bench yardstick. `diversity` of the made sounds (`{'n': 0}` with none).
8. With real:
   - `real_cross` = the forge refs scored against the bench yardstick (needs at least 1 bench ref). This is the main
     yardstick for made sounds: real recordings that the bench never saw.
   - `real_loo` = each bench ref against the other bench refs (needs at least 2 bench refs).
   - `real_rev` = the bench refs against the forge yardstick (JSON only).
   - `diversity_bench`, `diversity_forge`.
   - With made sounds and real rows (cross + loo): `inside` = inside_real(made, real rows), plus `real_per_sound` =
     inside_real(real rows, real rows) per sound.
9. `summary` for each of made, real_cross, real_loo = `summ(rows)`. Add `var_cos` = the cos_mean of the matching
   diversity (made: diversity; real_cross: diversity_forge; real_loo: diversity_bench) when it has n >= 2.
10. Log one line: `<cat>: <nm> made, <nb> bench, <nf> forge refs`, plus, with inside, `; inside the real 80% limits on
    <median> of <n_checks> checks (a real recording: <median>)`.

`METRICS` = typ_z high, tone_db low, env_oct low, curve_db low, phrase_z high, margin high, as_p high.

`summ(rows)`: {} for no rows. Else n, and the median (2 decimals) of every METRICS key plus tone_ratio, t40_ratio,
wins, floor_db, stereo_corr, darken (over rows where the key is present); `as_names` = the mean of the booleans (2
decimals); `artifacts` = the total number of flags.

`inside_real(made, real, metrics=METRICS, q=0.8)`: for each metric with real values: the limit is the q quantile
(linear interpolation) of the real values for a low metric, the 1 - q quantile for a high metric. Per made row, count
the metrics whose value is present and inside the limit (<= limit for low, >= for high). Return `per_sound`,
`n_checks` (the number of limits), `limits` (3 decimals). q = 1 means the worst real value.

### Run

`gather(made, files)`: per sub-folder of made (sorted), use `<cat>/final/` when present; take the sorted `*.wav`
files that are not skipped. Each `CAT=GLOB` spec extends that category with sorted matches. Skip a file whose
lower-case basename starts with `audition` (the forge's interleaved made + ref file) or `_` (scratch).

`recipe_phrases()`: the set of every recipe's TARGET and ALTS text, stripped and lower-case. A recipe that fails to
import is skipped (it can be under edit).

`main`:
1. `--retable`: read the JSON, set a missing `summary` to `{}` per category, write the table, print `wrote <md>`.
2. Load the registry (part 9; a missing registry raises its clear error).
3. Independence check: when any PHRASES text (stripped, lower-case) is also a recipe phrase, exit with an error that
   lists the clashes.
4. made = gather (empty with `--real-only`). cats = `--cats`, else all registry categories with `--real-only`, else the
   sorted made categories. A made category not in PHRASES: exit with an error that lists the known ones.
5. Load the CLAP and AudioSet models once. For each category run bench_category. Record `tag`, `made_from`,
   `registry`, `created` (`YYYY-MM-DD HH:MM`), `phrases`, `confusions`, `as_labels` (of the run's categories),
   `categories`, `seconds`.
6. Scrub machine paths from the result (10.5) after a JSON round trip (arrays as lists, other objects as text). Write
   the JSON (indent 1) and the markdown table. Print `wrote <json> and <md> (<s> s)`.

`table(res)`: a heading `# forge benchmark: <tag>`, a line with made_from, registry and created, a legend paragraph, and
one row per category and set (made, real x, real loo) with n and the columns typ z, tone dB, env oct, curve dB, t40 x,
phrase z, margin, AS names, artifacts, var cos and inside. The inside cell (made rows only) reads `<median> / <n>
(real <median>)`. A category with no rows gets a `no rows` line; a category note gets a `note:` line. Then a texture
table (floor dB, stereo corr, darken) and a diversity list per category: `cos <mean> / <max>, tone min <d> dB, <n>
near duplicate pair(s)` with the bench refs' values in brackets.

### Constants: phrasings, confusions and AudioSet labels

The phrasings must stay independent of every recipe's TARGET and ALTS and of the build script's find queries. The
benchmark numbers in 10.6 were measured with these exact texts, so keep them unchanged. Add new categories only as
additions.

PHRASES (2 per category):

| category | phrasing 1 | phrasing 2 |
|---|---|---|
| whoosh_fast | a quick swoosh as something flies past | a short sharp swoosh of air |
| whoosh_big | a heavy swooping rush of air | a big slow swoosh in a movie trailer |
| swish | a thin airy swish | a light quick swoosh of a thrown object |
| swipe | fabric rustling in one quick swipe | a sleeve brushing past quickly |
| riser | a tension build-up getting louder and louder | a whooshing crescendo that swells to a climax |
| downlifter | a descending whoosh that fades out | a falling pitch sweep winding down |
| impact | a huge low thud for a movie trailer | a massive hit with a rumbling tail |
| sub_drop | a low frequency boom that drops in pitch | a deep rumbling bass drop |
| click | a crisp tiny plastic click | a short clean click of a button being pressed |
| tap | a fingertip tapping on glass | a light tap of a finger on a phone screen |
| toggle | a light switch clicking on | a small switch being flipped |
| hover_tick | a very short faint tick | a tiny dry click |
| pop | a popping sound like a bubble bursting | a quick plop |
| notify | a pleasant chime alert sound | a single bell ding on a phone |
| shimmer | sparkling twinkling chimes | a glittering magical sound |
| typing_key | one key being pressed on a computer keyboard | a single keystroke on a mechanical keyboard |
| glitch | a digital glitch with stuttering noise | a broken electronic signal stutter |
| logo_sting | a short musical logo with a ringing finish | a bright struck bell ringing out |
| pro_click | a crisp bright digital click | a tiny sharp interface click |
| pro_tap | a soft muted tap on a screen | a short dull interface tap |
| pro_pop_in | a soft round thump as something appears on screen | a short muffled pop of an element appearing |
| pro_hover_tick | a tiny bright tick of a cursor | a faint high interface tick |
| pro_pop | a quick electronic blip | a tiny synthetic pop |
| pro_notify | a soft digital notification ding | a short pure alert tone |
| pro_shimmer | a glittering digital sparkle | a shimmering burst of tiny tones |
| pro_typing_key | a light clean keystroke | a soft tick of a keyboard key |
| pro_typing_run | someone typing a short line quickly | a rapid run of keystrokes |
| pro_glitch | a short burst of digital stutter | a crackling data glitch that cuts off |
| pro_whoosh_big | a deep heavy swoosh with a low rumble | a slow powerful whoosh of air |
| pro_riser | a rising stutter effect that speeds up | a pulsing electronic build-up |
| pro_zip | a fast rising electronic tone | a quick swooping synth glide |
| pro_impact | a deep designed boom that cuts off | a heavy low rumble hit in a launch video |
| pro_sub_drop | a deep sub bass boom that sinks | a low electronic bass drop |
| pro_logo_sting | a short electronic logo sound | a falling synth tone landing on a hum |

CONFUSIONS (4 per category; the margin is taken over the best of them):

| category | confusions |
|---|---|
| whoosh_fast | a steam hiss; a car driving past; tearing paper; a bird flapping its wings |
| whoosh_big | a strong wind storm; an airplane taking off; ocean waves crashing; a car driving past |
| swish | a short hiss; a sword being drawn; a page being turned; a bird flying past |
| swipe | paper crumpling; a broom sweeping; a gust of wind; a zipper |
| riser | a jet engine starting; a siren; steady rain; an orchestra tuning |
| downlifter | a siren; a car passing by; a falling bomb whistle; wind |
| impact | a gunshot; a door being slammed; a bass drum; distant fireworks |
| sub_drop | an earthquake rumble; a kick drum; a truck engine idling; thunder in the distance |
| click | a wooden stick hit; a finger snap; a drop of water; a stapler |
| tap | a knock on a door; a raindrop; a keyboard key; a ballpoint pen click |
| toggle | a door latch; a stapler; a seatbelt buckle; a ball bouncing |
| hover_tick | a clock ticking; a drop of water; a typewriter; a cricket chirp |
| pop | a hand clap; a gunshot; a kiss; a tongue click |
| notify | a doorbell; a microwave beep; a church bell; a cash register |
| shimmer | a harp glissando; breaking glass; a music box; a sleigh bell jingle |
| typing_key | a ballpoint pen click; a stapler; a light switch; a dripping tap |
| glitch | radio static; a modem dialing; a scratched record; an electric buzz |
| logo_sting | a church bell; a doorbell; a gong; a cymbal crash |
| pro_pop_in | a kick drum; a knock on a door; a ball bouncing; a hand clap |
| pro_pop | a microwave beep; a bird chirp; a tongue click; a kiss |
| pro_typing_run | rain on a roof; a clock ticking; popcorn popping; a horse galloping |
| pro_riser | a helicopter; a machine gun; a dial-up modem; a ticking clock |
| pro_zip | a whistle; a bird singing; a mosquito buzzing; a slide whistle |
| pro_logo_sting | a doorbell; a car horn; an error buzzer; a church bell |

The other pro_* categories reuse the list of their base category: pro_click (click), pro_tap (tap), pro_hover_tick
(hover_tick), pro_notify (notify), pro_shimmer (shimmer), pro_typing_key (typing_key), pro_glitch (glitch),
pro_whoosh_big (whoosh_big), pro_impact (impact), pro_sub_drop (sub_drop).

AS_LABELS (AudioSet class names that name the category; riser, downlifter and glitch have no exact class, so the
nearest are used):

| category | AudioSet classes |
|---|---|
| whoosh_fast, swish | Whoosh, swoosh, swish; Whip |
| whoosh_big, downlifter | Whoosh, swoosh, swish |
| swipe | Whoosh, swoosh, swish; Rustle |
| riser | Whoosh, swoosh, swish; Cymbal; Drum roll |
| impact | Boom; Thump, thud; Explosion; Bang; Bass drum |
| sub_drop | Boom; Rumble; Thump, thud |
| click | Clicking; Tick; Tap |
| tap | Tap; Knock; Clicking; Tick |
| toggle | Clicking; Mechanisms; Tick |
| hover_tick | Tick; Clicking; Tick-tock |
| pop | Burst, pop; Plop |
| notify | Ding; Chime; Bell; Glockenspiel; Jingle, tinkle; Ding-dong; Ringtone; Marimba, xylophone; Vibraphone |
| shimmer | Chime; Wind chime; Jingle, tinkle; Glockenspiel; Bell; Jingle bell |
| typing_key | Computer keyboard; Typing; Typewriter; Clicking |
| glitch | Static; Noise; Beep, bleep; White noise; Crackle |
| logo_sting | Chime; Bell; Ding; Jingle, tinkle; Gong; Cymbal; Glockenspiel |
| pro_pop_in | Thump, thud; Burst, pop; Plop |
| pro_pop | Burst, pop; Plop; Beep, bleep; Chirp tone |
| pro_notify | the notify list plus Beep, bleep |
| pro_typing_run | the typing_key list |
| pro_riser | Whoosh, swoosh, swish; Drum roll; Beep, bleep |
| pro_zip | Chirp tone; Sine wave; Beep, bleep; Whoosh, swoosh, swish |
| pro_impact | the impact list plus Rumble |
| pro_logo_sting | the logo_sting list plus Beep, bleep; Synthesizer |

The other pro_* categories reuse the list of their base category (pro_click, pro_tap, pro_hover_tick, pro_shimmer,
pro_typing_key, pro_glitch, pro_whoosh_big, pro_sub_drop).

### Errors and edge cases

- A category with fewer than 3 kept refs has no typ_z. A category with no refs at all still gets phrase, AudioSet,
  artifact, texture and diversity numbers.
- real_loo needs at least 2 bench refs; real_cross needs at least 1 bench ref; inside needs made rows and real rows.
- A phrasing clash with a recipe's TARGET or ALTS stops the run before any model loads. This keeps the benchmark
  independent of the recipes.
- A made category not in PHRASES stops the run.
- `audition*.wav` and `_*.wav` are never made sounds. (In the source, the audition file once entered four areas'
  first runs and polluted them.)
- Run it with HARK_THREADS=2 so that it does not starve a shared machine.

## 10.2 `frozen_tone.py`

A fixed copy of the tone measures of part 3 as they stood when the benchmark started (round 0). The benchmark imports
it from its own folder. Never edit it: a change moves every benchmark number. Add a new measure in `bench.py` and
record it in the benchmark change log (10.6). When our part 3 code later changes, this file stays as specified here.

Constants: SR 48000; band centres CEN = 1000 x 2^(k/3) for k = -15 .. 12 (28 bands, 31.5 Hz to 15.9 kHz); band edges
centre / 2^(1/6) (inclusive) to centre x 2^(1/6) (exclusive); REGIONS `<150` 0-150 Hz, `150-1k` 150-1000, `1-4k`
1000-4000, `4-12k` 4000-12000, `>12k` 12000-24000.

Functions (all on the mono mean of the channels; a path is loaded through core-io audio load):
- `label(c)`: `<c/1000 to 3 significant digits> kHz` when c >= 1000, else `<c, no decimals> Hz`.
- `profile(x)`: power spectrum |rfft(m)|^2 of the whole file; band power = the sum over the band's bins + 1e-20;
  result = max(10 log10(band / sum of bands), -60) per band.
- `shares(x)`: per region, 10 log10(region power / (total + 1e-20) + 1e-12), 1 decimal.
- `t40_ms(x, db=40, ms=5)`: n = 5 ms of samples; k = len // n. k < 2 gives 0.0. e = the RMS dB of the k whole frames
  (no padding). i = the loudest frame. Return (the first frame after i, counted from i, that is more than db under
  e[i]; else the number of frames from i to the end) x ms.
- `ToneRef(refs, margin=3.0)`:
  - P = stacked profiles (shape (0, 28) with no refs); med, lo, hi = the band median, min, max; t40 per ref; shares
    per ref; length in seconds per ref.
  - ref_dist (only with n >= 3): for each ref i, the RMS of (P_i - the median of the other refs).
  - `spread`: (median, max) of ref_dist, or None with fewer than 3 refs.
  - `check(x, exclude=None)`: `{}` with no refs. keep = all but exclude. med, lo, hi over kept refs. dev = profile(x)
    - med. dist = RMS(dev). sp = ref_dist[keep] (empty when ref_dist is empty). ratio = dist / max(median(sp), 0.5)
    when len(sp) >= 2, else None. over = max(p - hi - margin, 0) - max(lo - p - margin, 0). outside = for each band
    with over != 0: (label, dev to 1 decimal, `above` when over > 0 else `below`). Return tone_db (2 decimals),
    tone_ratio (2), tone_ref (the sp list, 2), band_dev (1), outside, t40_ms, t40_ref (min, max of kept), length_s
    (3), shares, shares_ref_med (median per region of kept, 1 decimal), length_ref (min, max of kept, 3), words.
  - `words(m)`: `tone <d:.1f> dB from the refs' median`, then ` (the refs sit <min>-<max> apart)` when tone_ref is not
    empty, then `: <dev:+.0f> dB at <band> (<above|below> every ref)` for up to 3 outside bands with the largest |dev|
    (comma separated), then `; tail t40 <t> ms (refs <a>-<b>)` when t40 > 1.5 b + 10 or t40 < 0.6 a - 5, then `;
    energy share vs the refs: <region> <diff:+.0f> dB, ...` for regions more than 9 dB from the refs' median share,
    then `; <len:.2f> s long (refs <lo>-<hi>)` when the length is more than 1.3 x the longest ref.
  - `region_penalty(m, regions=('<150', '4-12k', '>12k'), margin=6, scale=6)`: the sum over regions of max(0, share
    - ref median share - margin) / scale.
  - `penalty(m)`: 0 when m is empty or tone_ratio is None. Else max(0, tone_ratio - 1), plus 0.5 x min(2, (t40 - b) /
    b) when the longest ref t40 b > 0 and t40 > b.

The benchmark uses `profile`, `t40_ms` and `ToneRef.check` only. The rest is kept so the copy stays whole.

## 10.5 `scrub_paths.py` (results hygiene, used by `bench.py`)

The results JSON names every registry file and ref path. Absolute machine paths do not belong in a committed results
file. Before the write, `scrub(obj)` rewrites every string (dict keys too) of the result:
- Roots, longest first: the pro-study trims folder (env `HARK_REFS_PRO`, default `<repo>/out/refs_pro`) as
  `<refs_pro>`; `<repo>/out/refs` as `<refs>`; the cache home (`HARK_HOME`, else the config home, else `<repo>/.cache`)
  as `<HARK_HOME>`; the repo itself as a repo-relative path (`.` for the root itself). When the registry's ref paths
  share a common folder 3 levels above the files, that is outside the repo and longer than 3 characters, it is also
  `<refs>`. Drop duplicate roots (case-insensitive).
- Match a root on a word boundary, with `/` or `\` separators (case-insensitive), followed by the rest of the path
  up to a space, quote, back quote, `;`, `,`, `)`, `]`, `<`, `>`, `|`, `*` or `?`. Write the rest with `/`.
- Ref file names stay. They are the evidence of which recording a row measured.
- `leftovers(text)`: any remaining drive path (`X:/...` or `X:\...`) or home path (`/Users/...`, `/home/...`).

CLI `python eval/forge/scrub_paths.py [FILES] [--check]`: default files are `eval/forge/results_*.json` and
`results_*.md`. JSON files are rewritten with indent 1 (a trailing newline kept); other files as text. Print `<file>:
scrubbed|clean|would change` plus `; left: <up to 3>`. Exit 1 when any file has leftovers or, with `--check`, would
change.
