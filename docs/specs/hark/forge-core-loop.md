# forge-core part 1: the forge loop, the recipe contract and the tonal helpers

Index: [forge-core.md](forge-core.md). This part covers the package entry module of the forge (the run loop,
families and palettes, re-rendering, the audition reel, the Markdown report, the `forge` CLI command), the recipe
contract that every recipe module follows, and the shared tonal building blocks that the tonal and UI recipes use.

The forge makes sounds in code. A recipe renders a sound from a parameter dict and a random generator. A search
proposes parameter dicts. A model-free pre-screen ranks many renders by measurement. A critic scores the best of
them against a text target and real reference recordings. The run keeps a small set of diverse strong sounds,
writes them as WAV files, and writes a report that lets every WAV re-render exactly.

All audio in the forge is float, 48 000 Hz. Mono is shape `(n,)`, stereo is shape `(n, 2)`.

## Interface

### Module `hark.forge` (package entry)

| function | signature | returns |
|---|---|---|
| `recipes()` | `()` | sorted list of recipe module names in `hark.forge.recipes`. A name that starts with `_` is a helper module and is not listed. |
| `load_recipe(name)` | `(name: str)` | the imported recipe module `hark.forge.recipes.<name>`. |
| `render_block(rec, params, seed, member=None, mode=None, gain=None)` | | the render block of one pick (part 6, schema), or `None` when the render function has no importable name. |
| `variant_recipe(render_fn, members, names=None, name=None, **attrs)` | | a stand-in recipe object that searches one key `variant` over a fixed list of complete member param dicts. |
| `pick_refs(refs=None, refset=None, recipe=None, log=None)` | | tuple `(reference file list or None, refset category used or None)`. |
| `resolve_refs(refs=None, refset=None, recipe=None, log=None)` | | the first element of `pick_refs`. |
| `audition(path, made, refs, pairs, gap_s=0.6, pair_gap_s=1.2, peak_db=-3.0, keep_made_levels=False)` | | `{'file', 'txt', 'cues'}`. |
| `report_md(r)` | `(report dict)` | Markdown text of a forge run report. |
| `forge(recipe, target=None, alts=None, refs=None, budget=160, pop=16, keep=4, seed=0, out=None, log=print, init=None, holdout=True, robust=0, space=None, sigma0=0.22, sigma1=0.04, critic=None, tone=0.0, tone_target=None, reps=1, strategy='es', prescreen=0, workers=None, bank=False, shortlist=1, refset=None, outputs=True, bank_dir=None, weights=None, mode='auto', families=None, box=0.15)` | | the run report dict. |
| `family_box(recipe, params, box=0.15, keys=None)` | | a narrow search space dict around one preset. |
| `palette(recipe, members, critic, out=None, name=None, peak_db=-1.0, loudness=True, robust=0, workers=None, holdout=True, log=print, outputs=True)` | | the palette report dict. |
| `palette_md(r)` | `(palette report)` | Markdown text of a palette report. |
| `render_params(recipe, params, seeds=None, out='.', refs=None)` | | `{'recipe', 'files'}`. |
| `add_cli(sub)` | | registers the `forge` subcommand. |

Constant `SCORE_KEYS`, the ordered tuple of critic score keys that the report copies into `history` and
`ref_scores`:
`total, passes, match_z, alt_margin, p_real, ref_typ_z, feat_pen, tone_db, tone_ratio, sounds_like, total_v1, mode,
tone_pen, env_db, env_ratio, env_out, env_pen, art_pen, polish_pen, hard, polish_words`.

### The recipe contract

A recipe is a Python module in `hark/forge/recipes/` (one module per sound type), or any object with the same
attributes. The forge reads these attributes:

| attribute | required | meaning |
|---|---|---|
| `SPACE` | yes | dict `name -> spec`. The search space. See "Parameter specs" below. |
| `DEFAULTS` | yes | dict `name -> value`. A complete, good starting parameter set. Every key that the render reads has a value here. |
| `TARGET` | yes | a text description that the critic aims for, for example "a clean bright user interface click". |
| `ALTS` | yes | list of text confusions. The sound must match these less than the target. |
| `render(p, r)` | yes | a function. `p` is a complete param dict. `r` is a `numpy.random.Generator`. It returns float audio at 48 kHz, mono `(n,)` or stereo `(n, 2)`. It is deterministic for a given `p` and a generator built from a given seed. No recorded audio is read: every sample is computed. |
| `REFSET` | no | name of a reference-set category (part 9). The CLI uses its forge split as the default references. |
| `REFS` | no | list of reference file paths under the reference root. The last fallback for references. |
| `TARGETS` | no | dict category -> target text, for a recipe that serves more than 1 category. |
| family presets | no | dicts of named characters, for example `FAMILIES`. The forge does not read them itself. A driver passes them to `forge(families=...)`. |
| `MEMBERS`, `MEMBER_NAMES`, `RENDER_FN` | no | set only on a `variant_recipe` stand-in. |

The recipe's name is the last dotted component of its `__name__`.

The reference root is the environment variable `HARK_REFS` (default `refs`). Backslashes become `/`, and a trailing
`/` is removed. Recipes build their `REFS` paths under it.

#### Parameter specs

A `SPACE` value is one of:
- a list: a set of choices. The search picks one element. A one-element list pins the parameter to that value.
- a tuple `(lo, hi)`: a linear float range.
- a tuple `(lo, hi, 'lin')`: the same.
- a tuple `(lo, hi, 'log')`: a float range searched in log units. `lo` is greater than 0.
- a tuple `(lo, hi, 'int')`: an integer range, inclusive.

The search encodes each parameter to a unit coordinate in [0, 1] and decodes back (part 7 gives the mapping).

### CLI: `hark forge`

`hark forge [recipe] [options]` with the common `--json` and `--out` flags of the core-io unit.

| flag | default | meaning |
|---|---|---|
| `recipe` (positional, optional) | | a recipe name. Omit it to list recipes. |
| `--list` | off | list every recipe with its TARGET and its refset, then the refset categories. |
| `--render [RECIPE]` | off | render `--params` without a search. The recipe follows the flag or is the positional. |
| `--params FILE` | | JSON: a params dict, a list of dicts, a report.json, or a seed bank file. |
| `--check DIR...` | | re-render every pick of each output folder from its report.json and compare with the WAV files (part 6). |
| `--seeds N...` | each pick's own seed, else 0 | seeds for `--render`. |
| `--target TEXT` | the recipe's TARGET | |
| `--alts TEXT...` | the recipe's ALTS | |
| `--refs PATH...` | | reference recordings, files or folders. |
| `--refset NAME` | | a refset category (its forge split). |
| `--budget N` | 160 | critic-scored candidates. |
| `--pop N` | 16 | |
| `--variants N`, alias `--keep N` | 4 | number of diverse best sounds to write. |
| `--seed N` | 0 | |
| `--dir DIR` | | output folder. |
| `--strategy es|cma` | es | |
| `--prescreen N` | 3 x pop | renders per generation ranked by measurement before the critic. 0 turns it off. |
| `--robust N` | 3 | re-render each kept sound on N new seeds. 0 turns it off. |
| `--shortlist N` | 2 | with robust: robust-score N x variants candidates and keep the best robust means. |
| `--no-bank` | off | do not start from the seed bank and do not add to it. |
| `--workers N` | `HARK_THREADS`, else 2 | render threads. |
| `--tone W` | the critic's own (1.0 in v2) | weight of the tonal-balance penalty. 0 makes it report-only. |
| `--mode auto|real|designed` | auto | critic mode. |
| `--reps N` | 1 | renders per candidate inside the search, ranked by their mean. |
| `--tone-target DB` | the refs' own spread | the tone distance to the refs' median that the critic treats as normal. |

CLI dispatch order:
1. `--check` given: run the schema checker on the folders and return its exit code.
2. `--render` given: the recipe name is the flag value, else the positional. If the name or `--params` is missing,
   print the usage line `usage: hark forge --render <recipe> --params file.json [--seeds 1 2 3] [--dir out]` and
   return 2. Else load the JSON, resolve refs, call `render_params`, print one line per file (path, or
   "failed or silent"; plus tone dB, t40 ms and nearest ref when measured), and return 0.
3. `--list` given or no recipe: print `recipes (name: TARGET):`, then one line per recipe
   `  <name>: <TARGET>  [refset <REFSET>]`. A recipe that fails to import prints `(does not load: <ErrorType>)`.
   Then print `refsets: <comma list>`, or a line that says no refsets exist on this machine. Return 0.
4. Else run a search. Pick refs with `pick_refs(--refs, --refset, recipe)`. With no refs, print a note that the
   critic cannot compare with real sounds. When the refs came from the recipe's REFSET (no `--refset` given), print
   which refset and how many files. Call `forge` with: prescreen = `--prescreen` or 3 x pop, tone = `--tone` or 0.0,
   weights = `{'tone': 0.0}` only when `--tone 0` was given exactly, bank = not `--no-bank`, refset = the category
   that `pick_refs` returned, the other flags as given. Print the summary, one line per pick
   (`  <name>: <critic summary>`), and when `--dir` is given a line that names the written files. Emit the report
   without its `history` key. Return 0.

The CLI defaults differ from the Python defaults on purpose. The Python `forge()` defaults reproduce the first
version of the loop (ES strategy, no pre-screen, no seed bank, no robust). The CLI uses the second-version setup
(pre-screen 3 x pop, seed bank, robust 3, shortlist 2, critic v2 weights, the recipe's REFSET).

### Output folder of `forge(out=DIR)`

| file | content |
|---|---|
| `<recipe>_NN.wav` | each pick, NN from 01. PCM 24-bit, 48 kHz. |
| `report.json` | the run report (below). Written with indent 1. Arrays become lists. Other non-JSON objects become strings. |
| `report.md` | `report_md(report)`. Only when `outputs` is true. |
| `compare.png` | each pick beside its nearest reference (part 9, viz). Only when `outputs` is true. |
| `audition.wav`, `audition.txt` | the listening reel and its cue list. Only when `outputs` is true. |

### Run report (`forge()` return value)

| key | meaning |
|---|---|
| `schema` | the report schema version (2). Removed when any pick lacks a `render` block. |
| `recipe` | recipe name. |
| `target`, `alts` | texts used. |
| `refs` | the critic's reference file list. |
| `refset` | the refset category or null. |
| `budget`, `seed`, `pop`, `reps`, `strategy` | run settings. |
| `tone_weight` | for a critic of version 2 or more: its weight dict entry `tone`; else the critic's `tone` value. |
| `tone_target` | the critic's tone target or null. |
| `critic` | `{'version', 'mode', 'weights'}`. |
| `prescreen` | renders per generation of the pre-screen (0 when off). |
| `restarts` | CMA restarts (0 for ES). |
| `n_bank` | starts taken from the seed bank. |
| `generations`, `gens`, `timing` | from the optimiser (part 7). `timing.forge_s` is added at the end: wall seconds of the whole call, 1 decimal. |
| `pick_rule` | one sentence that states how the picks were chosen. |
| `space` | present only when a `space` was passed. |
| `fill_warnings` | present only when the optimiser filled a key at mid-range (part 7). |
| `picks` | list of pick items (below). |
| `ref_scores` | the critic's leave-one-out scores of the references (below). |
| `ref_scores_loo` | `true`. |
| `summary` | one sentence (below). |
| `gap` | present when both picks and refs have tone numbers (below). |
| `history` | every scored candidate: `{'gen', 'params', 'seed', 'score': {SCORE_KEYS}, 'pre'?}`. |
| `files` | present when `out` is given: `compare`, or `compare_error` (`<ErrorType>: <message>`), `audition`, `report_md`. |

Pick item keys: `name` (`<recipe>_NN`), `params`, `seed`, `score` (the critic row without its index key `i`),
`render` (render block), `holdout` (held-out AudioSet tags, when computed), `robust` (when robust > 0), `measured`
(when the pre-screen has refs), `file` (when `out` is given).

`measured` holds: `tone_db`, `tone_ratio`, `ref_spread` (the pre-screen's ref spread, 2 decimals), `env_z`,
`t40_ms`, `length_s` (3 decimals), `t40_ref` ([min, max] over the refs, rounded to ms), `length_ref` ([min, max]
ref length in s, 3 decimals), `nearest_ref` (basename of the nearest ref by measurement), `nearest_dist` (the
sound distance to it, 2 decimals), `art` (artifact flag list).

Ref score row keys: `file` (basename), `alt_zs`, every SCORE_KEYS key, and when the pre-screen has that ref:
`tone_loo_db` (RMS dB difference between this ref's 1/3-octave profile and the median profile of the other refs,
2 decimals), `t40_ms`, `length_s`; plus `holdout` when computed.

## Behaviour

### `pick_refs`

Resolution order:
1. `refset` given: return the forge split of that refset category and the category name.
2. `refs` given: list the audio files in those files and folders. Drop held-out files (refsets `held_out` with
   `kit=False`). Return the list, or `None` when it is empty, and category `None`.
3. The recipe has `REFSET`: try its forge split. Return it with the category when it is not empty. A missing
   registry or an unknown category falls through silently.
4. Else take the recipe's `REFS` that exist on disk, drop held-out ones (`kit=True`, which also drops kit files cut
   from a bench source). Return the list or `None`, with category `None`.

When files are dropped and `log` is given, log one line that gives the count and the sorted basenames of the
dropped files.

### `forge()`

1. Load the recipe when `recipe` is a name. The run name is the recipe's short name.
2. When no `critic` is given, build one: target = `target` or the recipe's TARGET; alts = `alts` or the recipe's
   ALTS (an empty list when it has none); refs are resolved with `resolve_refs` only when `refs` or `refset` was
   given (otherwise `refs` passes through as is, which can be `None`); the critic gets `tone`, `tone_target`,
   `weights`, `mode` and `category` = `refset` or the recipe's REFSET. When a critic is given, take target and alts
   from it.
3. When `families` is given, run the families flow (below) and return its result.
4. Start points: `init` (a list of param dicts). When `bank` is true and the recipe is a real recipe module (its
   name is in `recipes()`), load the seed bank entries for this recipe, target and refs, and add them. Count them
   as `n_bank`.
5. Build the pre-screen for the critic's refs. Run the optimiser (part 7) with budget, pop, seed, starts, space,
   sigma0, sigma1, reps, strategy, prescreen, workers and the pre-screen.
6. The pick pool is the optimiser's best list followed by the 64 highest-total history entries.
7. With `robust` > 0, the shortlist factor S is max(1, shortlist); else S = 1. Take `keep x S` diverse picks from
   the pool (part 7, `diverse`).
8. With robust > 0, compute robust stats for each pick on `robust` new seeds. When S > 1, sort the picks: first those
   with a robust pass rate of at least 0.5, then by robust mean, descending (a missing mean counts as -1e9). Keep the
   first `keep`. The pick rule sentence says that `keep x S` diverse candidates were re-rendered on `robust` seeds
   and the `keep` best robust means kept, passing at least half first. Otherwise the rule says the strongest
   candidates with different parameters and sound were kept, passing first, and adds the robust re-render note
   when robust > 0.
9. Score the refs leave-one-out with the critic when it has refs.
10. Render each pick once more with its own seed. Compute held-out AudioSet tags for the rendered picks and for the
    loaded refs when `holdout` is true.
11. Build each pick item. When the pre-screen has refs, measure the pick and find its nearest ref by sound distance
    (part 7). The nearest-ref index is also the audition pair.
12. Write each WAV when `out` is given.
13. Drop the `schema` key when any pick lacks a render block.
14. Build `ref_scores`.
15. Summary: `forge <name>: '<target>' - <n_pass> of <n_picks> kept sounds pass (best total <t>, match z <z>,
    p_real <p>)`, then, when a finite ref total exists, `; the real references score up to <best:.2f> with the same
    critic`, then ` (references scored leave-one-out).`. The numbers come from the first pick.
16. Gap: when picks have `measured.tone_db` and refs have `tone_loo_db`, state the pick range (min-max, 1 decimal)
    and the ref range (min-max, 1 decimal) and explain that a pick inside the ref range is as typical as a real
    recording. Append the gap sentence to the summary.
17. Write `history`, `timing.forge_s`, and the output files. A plotting error must not lose the run: catch any
    exception of the compare picture and record it under `files.compare_error`.
18. When `bank` is true, the recipe is real, and there are picks, save the picks into the seed bank with a source
    text `forge <strategy> budget <budget> seed <seed>` plus ` refset <refset>` when one was used. An OS error
    while saving logs `seed bank not updated: <error>` and does not fail the run.

### Families flow (`forge(families=...)`)

`families` is a dict, or a list of `(name, spec)` pairs. A spec is either a plain params dict (a "plain" spec) or a
dict with any of the keys `init`, `space`, `fixed`, `budget`.

For member k (0-based):
1. `init` = the plain spec, or the spec's `init` (default empty). `fixed` = the spec's `fixed` (default empty).
2. `base` = the recipe's DEFAULTS, overlaid by `init`, overlaid by `fixed`.
3. Member space = the spec's `space`, else `family_box(rec, base, box)`. Remove every `fixed` key from it.
4. Build a stand-in recipe that copies TARGET, ALTS, REFSET and REFS when present, keeps the recipe's name, SPACE and
   render, and uses `base` as its DEFAULTS. This is the reason a key outside the member's box keeps the member's
   value and not the recipe's DEFAULTS.
5. Run `forge` on the stand-in with the shared critic, the member space, `init=[base]`, keep 1, budget = the spec's
   `budget` or the run budget, seed = `seed + 101 * k`, out = `<out>/runs/<member name>` (when `out` is given),
   holdout off, outputs off, bank off, and the other run settings as given.
6. When the run has a pick, append a member `{'params', 'seed', 'family': name, 'robust'?}`. Append a run row
   `{'family', 'out', 'budget', 'timing', 'total'}` in every case.

Then call `palette(rec, members, critic, out, name=refset, holdout, log, outputs)`. Add `runs` and set
`timing = {'forge_s': ...}`. Write the palette report again to `report.json` when `out` is given. Return it.

### `family_box`

For each SPACE entry: skip choice lists, keys not in `params`, and (when `keys` is given) keys not in `keys`.
For a log range with lo > 0: in log units, width w = box x (log hi - log lo). The centre is log of the value clamped
into [lo, hi]. The box is [exp(max(log lo, c - w)), exp(min(log hi, c + w))], kind `log`. For a linear or int range:
w = box x (hi - lo), the box is [max(lo, v - w), min(hi, v + w)], kind `int` is kept. Drop an entry whose low end is
not less than its high end.

### `palette()`

Scores an exact member set together and writes it as one set of interchangeable sounds.

1. `members` items are dicts with `params` (a pick of a report works), or bare params dicts (seed 0). Each member's
   params are the recipe's DEFAULTS overlaid by the member's params. Seed = the member's seed or 0.
2. Render every member (part 7 `render`). A member that fails or is silent is removed, and when `log` is given one
   line names the failed indices.
3. Loudness match: measure the loudness spread of the renders (part 4, `loudness_spread`). When `loudness` is true,
   each member gain is 10^((min LUFS - member LUFS) / 20): every member goes to the quietest member's
   momentary-maximum loudness. Then one common gain c = 10^(peak_db / 20) / max over members of (peak x gain). The
   loudest peak lands on `peak_db` (default -1 dBFS). Output samples are float32.
4. Score all members with the critic in one call. Embed members and refs (when the critic can embed and there are at
   least 2).
5. Variety: `members` and `refs` each `{'cos_mean', 'cos_max'}` (below); `tone_min_db` and `tone_mean_db`: min and
   mean of the RMS difference of the 1/3-octave profiles over all member pairs, 2 decimals.
6. Each pick item: `name` (the member's name, else `<category>_NN`), `family`, `params`, `seed`, `gain_db` =
   20 log10(gain x c) to 2 decimals, `score`, `render` (block with member = family, or member, or preset name, and
   gain = gain x c), `robust` (the member's own, else computed when robust > 0), `holdout`, `measured`
   (`tone_db`, `t40_ms`, `length_s`, `nearest_ref`, `nearest_dist`), `file`.
7. The category name is `name`, else the recipe's REFSET, else the recipe name.
8. Report keys: `schema`, `recipe`, `category`, `target`, `refs`, `critic` (`version`, `mode`, `category`),
   `pick_rule` (states the exact set, no diversity drop, matched to the quietest member, then one gain to the peak),
   `variety`, `loudness` (`before`, `after`: two loudness_spread results), `picks`, `summary`, `files`.
9. Summary: `palette <name>: <n> members, <n_pass> pass; variety cos <mean> (refs <mean>), closest pair <d> dB
   apart (1/3 oct); loudness spread <before> -> <after> LU`. A missing value prints `-`.
10. Files: `<name>_NN.wav` per member, `_audition.wav` with `keep_made_levels=True` (the leading underscore keeps
    the benchmark from counting it as a made sound), `compare.png` and `report.md` when `outputs`, and
    `report.json`. Log the summary when `log` is given.

Variety of embeddings: normalise each row to unit length (add 1e-12 to the norm), form the cosine matrix, take the
off-diagonal entries, return mean and max to 3 decimals. Return `None` with fewer than 2 rows.

### `variant_recipe`

Returns a stand-in object with: `__name__` = `name`, else the module of the render function (unwrap a
`functools.partial` through its `func`), else `hark.forge.recipes.variant`; `SPACE = {'variant': [0 .. m-1]}`;
`DEFAULTS = {'variant': 0}`; `render(p, r)` that renders `render_fn(copy of members[p['variant']], r)`; `MEMBERS`
(copies), `MEMBER_NAMES`, `RENDER_FN`; plus every extra keyword attribute (TARGET, ALTS, REFSET ...). Use it as
`forge(variant_recipe(...), budget=m, pop=m, keep=m, init=[{'variant': k} for k in range(m)])` to score a hand-made
member set.

`render_block` on a pick whose params are exactly `{'variant'}` on such an object: index v; the block names
`RENDER_FN`, the member's own params `MEMBERS[v]`, the seed, and member = `MEMBER_NAMES[v]` unless a member is given.
Otherwise the block names `rec.render` and the params.

### `render_params`

Renders param sets without a search.
- A report dict (has `picks`): for each pick, build its render block (part 6 `block`). When the pick has its own
  render block, or the inferred block names this recipe, resolve the function and render the block's complete
  params with it. On a not-reproducible, key, type or value error, fall back to the pick's `params` on the recipe.
  The seed is the pick's own.
- A seed bank dict (has `entries`): each entry's params, no seed.
- A plain dict: one set. A list: one set per element.
- Seeds: `seeds` when given, else the set's own seed, else 0. Every set renders once per seed.
- A fallback render uses the recipe's DEFAULTS overlaid by the set's params.
- Each file is `<out>/<recipe>_r<NN>_s<seed>.wav` (NN from 01), PCM 24-bit. A failed render keeps `file: null`.
- With refs, each written file gets `measured`: `tone_db`, `tone_ratio`, `t40_ms`, `art` flags, `nearest_ref`.

### `audition`

Writes a listening reel: each made sound, then its paired ref when the pair index is not null. Each clip becomes
stereo (a mono clip is duplicated) and is peak-normalised to `peak_db` (default -3 dBFS). With
`keep_made_levels=True` the made clips keep their own levels and only the refs are normalised. After a made clip
followed by its ref, the gap is `gap_s` (0.6 s); after the last clip of a pair, the gap is `pair_gap_s` (1.2 s).
The reel is 48 kHz stereo PCM 24-bit. An empty reel is 4800 samples of silence. A cue is `{'t' (s, 2 decimals),
'kind' ('made' | 'ref'), 'name'}`. The `.txt` file next to it has a header line that says each made sound is
followed by its nearest ref by tonal balance and envelope, and states the level rule (peak-matched, or made at
matched levels and refs at the peak), then one line per cue: time (8.2f) ` s  `, kind padded to 4, `  `, name.

### `report_md`

Sections, in this order:
1. Title `# forge <recipe>: '<target>'`.
2. **Asked:** recipe, target, alternatives, refset, the ref count and basenames, critic version and mode.
3. **Budget:** strategy, budget, pop, generations, pre-screen size (or "no pre-screen"), reps, tone weight, seed,
   bank starts, restarts. When timing exists: minutes, renders, scored, per-minute rates, and the render,
   measurement and critic seconds.
4. `## Search`: a table with columns gen, best total, this gen best, this gen median, passes (`passes/scored`),
   best tone dB (1 decimal), sigma (3 decimals), note (`restart from <x>` when a CMA restart happened).
5. `## Picks`: the pick rule, then per pick a `###` heading with its name and WAV basename, the critic summary, the
   robust line (n, mean, min, sd, pass rate, tone mean), the measured line (tone dB from the median and the typical
   ref spread, t40 and the ref range, length and the ref range, nearest ref by measurement and by CLAP, artifacts),
   the held-out AudioSet top tags (marked as deciding nothing), and the params as compact JSON (floats rounded to 5
   decimals) with the seed.
6. `## Gap to the real references` when ref scores exist: a note that refs are scored leave-one-out and that model
   checks saturate, then a table with columns sound, total, passes, match z, p_real, tone dB, env pen, art pen,
   polish pen, hard, t40 ms, length s. Made rows use `measured.tone_db`; real rows use `tone_loo_db`. Then the gap
   sentence.
7. `## Files`: the WAV list and one line each for compare.png, audition.wav, report.json.

Number format: `None` prints `-`; floats print with 2 decimals unless the column states otherwise.

`palette_md`: title `# palette <category> (<recipe>)`, the summary, the pick rule, a table (member, family, total,
passes, tone dB, env pen, art pen, polish pen, hard, gain dB, nearest ref), then `## Critic words` with one line per
member.

## Algorithms: tonal building blocks (`hark.forge.recipes._tonal`)

A helper module for the tonal and UI recipes (pops, knocks, mallets, drips, shutters). It is not a recipe
(leading underscore). Its parts: a soft onset followed by a natural decay; a pitch glide with gentle partials; noise
shaped around a pitch; short contact bursts; struck modes; a finishing chain. Why: in the study films, real UI sounds
start over 20-60 ms, ring down naturally within 0.2-0.4 s, and mallet notes have a stereo correlation of 0.3-0.7. An
older bank started in 5 ms, faded out at the end of a 2 s ring, and was near mono (correlation about 1.0).

`SR = 48000`. Time vectors come from part 2 (`t_of`, `n_of`).

`env(t, onset, tau, hold=0, curve=1, dcurve=1)`:
u = clip(t / max(onset, 1e-4), 0, 1)^curve; rise = 0.5 - 0.5 cos(pi u); x = max(t - onset - hold, 0) / max(tau, 1e-4);
result = rise x exp(-x) when dcurve = 1, else rise x exp(-x^dcurve). With dcurve > 1 the decay starts slower and ends
faster: t40 / t20 = 2^(1/dcurve). dcurve = 1 falls 20 dB in about 2.3 tau.

`two_rate(t, onset, fast, tf, ts, curve=1)`: the same raised-cosine rise, then with tm = max(t - onset, 0):
rise x (fast exp(-tm / tf) + (1 - fast) exp(-tm / ts)), each tau floored at 1e-4. A strike dies mostly in tens of ms
and a quiet ring carries on.

`glide(t, f0, bend_st=0, ptau=0.02)`: f(t) = f0 x 2^((bend_st / 12) exp(-t / ptau)). It starts bend_st semitones off
f0 (above when positive, so the pitch falls) and settles on f0.

`rise_track(t, f0, rise_st, rtau)`: f(t) = f0 x 2^((rise_st / 12)(1 - exp(-t / rtau))). The pitch rises (a bubble).

Partial anti-alias gain for ratio q at pitch f: clip((0.45 SR - q f) / 2400, 0, 1). A partial fades out over the
2.4 kHz below 0.45 SR.

`tone(f, r, parts=(), e=None)`: phase = 2 pi cumsum(f) / SR + a uniform random phase in [0, 2 pi). y = sin(phase) x e.
For each partial (ratio q, amp a, decay d) with a > 0: add a x antialias(f, q) x e^d x sin(q phase + new random phase).
d > 1 makes a partial die faster than the body. Ratios can be inharmonic. e defaults to ones. Random draws happen in
order: the base phase first, then 1 phase per partial with a > 0.

`follow_noise(n, r, fc, q, e)`: fc (number or per-sample track) is capped at 16 kHz. Band-pass white noise with the
state-variable filter (part 2, `svf` band mode) at fc and q. The "live" part is where e > 0.1 x (max e + 1e-12).
Scale the noise so its RMS over the live part is sqrt(0.5) (the RMS of a unit sine, so 0 dB = as strong as the
tone). Multiply by e.

`burst(n, r, lo, hi, tau, rise=0.0003, order=2)`: k = min(n, samples of max(6 tau + rise, 2 ms)). White noise of
length k, Butterworth band-pass between max(lo, 20) and min(hi, 0.45 SR) of the given order, times
(1 - exp(-t / max(rise, 1e-5))) exp(-t / max(tau, 1e-5)). Normalise to peak 1 (add 1e-12). Zero-pad to n.

`thump(n, f, tau, rise=0.002)`: sin(2 pi f t)(1 - exp(-t / rise)) exp(-t / tau), floors 1e-4, normalised to peak 1.

`modes(n, r, freqs, amps, taus, jitter=0, bend=0, bend_tau=0.004)`: a sum of damped sines. Skip a mode with a <= 0
or f >= 0.45 SR. Each mode frequency is f (1 + jitter x a standard normal draw). The pitch factor b(t) =
1 + bend exp(-t / bend_tau) (a hard strike starts sharp). Phase = 2 pi cumsum(f b) / SR + a random phase. Add
a exp(-t / tau) sin(phase).

`strike(y, r, contact_s, bright=0.5)`: excite resonator output y with a contact pulse of contact_s seconds
(part 2, `contact` and `excite`). A longer contact excites high modes less.

`room(y, r, wet, rt60, size=0.15, width=0.3)`: when wet <= 0.005 return y unchanged with no random draw. Else apply
the stereo reverb (part 2) with rt60, size, predelay 4 ms, damp 0.6, wet, stereo, width.

`finish(y, r, hp=80, lp=12000, room_wet=0, rt60=0.15, width=0, cut=500, end_db=-60, fade=0.006, pan=0, size=0.15)`:
mono in, stereo out.
1. Fade the dry end first: the last kd = min(n, 20 ms, 15 % of n) samples are multiplied by cos^2 of a ramp from 0
   to pi/2. The reason: the room rings past the dry end, and a low body cut inside one cycle leaves a step in the
   quiet tail that the artifact audit hears as a click (a 95 Hz thump has a 10.5 ms cycle).
2. High-pass, 4th-order Butterworth at hp. Low-pass, 2nd-order Butterworth at lp, only when lp < 17 500 Hz.
3. Width: when width > 0.005, decorrelate above `cut` Hz (part 2, `decorrelate`), else duplicate to 2 channels.
4. Pan: when |pan| > 0.005, theta = (clip(pan, -1, 1) + 1) pi / 4; L x cos(theta) sqrt(2), R x sin(theta) sqrt(2).
5. Room with width 0.3 + width.
6. Trim the tail where it stays end_db under the peak (part 2, `trim_tail`, with a 10 ms tail fade).
7. End fade: the last `fade` seconds times cos^2 from 0 to pi/2.
8. Start: the first 0.3 ms times a raised cosine from 0 to 1.

`measure(x)`: measures one render on silence without any model. The definitions match the film catalog of the
recipes study (stage A). Steps:
1. Make stereo. Mono mix m = mean of channels.
2. Energy track: RMS-power in a 5 ms window every 1 ms (from a cumulative sum of m^2; window [c - w/2, c + w/2]
   clipped to the signal), smoothed by a 10-tap moving average ("same" mode). The peak index ip and value pk.
3. `attack_ms` = ip minus the first index at or before ip where the track is at least pk x 1e-2 (within 20 dB).
4. Onset = the first index where the track is at least pk x 1e-3. End = the last index at least pk x 10^-4.5.
   `dur_s` = end x 0.001 - onset x 0.001, 3 decimals.
5. After the peak: `t20_ms` = the first index after ip under pk x 1e-2; `t40_ms` = the first under pk x 1e-4. `None`
   when never reached.
6. Spectrum: frames of 2048 samples, hop 240, Hann window, power. Zero-pad m to 2048 when shorter. Frame energy is
   the sum over bins. Active frames have at least 1 % of the strongest frame's energy. The mean spectrum over active
   frames gives `centroid_hz` (integer), `lo150` (share of energy under 150 Hz, 3 decimals), `hi4k` (share at or
   above 4 kHz, 3 decimals).
7. For each active frame, in the band 80-12 000 Hz: the strongest bin's frequency (the ridge), and the share of the
   band's energy within a quarter octave of it. `tonal` = the mean share (2 decimals) when at least 2 frames count.
8. `glide_st` = 12 log2(median of the last k3 ridge frequencies / median of the first k3), k3 = max(1, min(3,
   count / 3)), 1 decimal, when at least 2 ridge points exist.
9. `corr` = sum(L R) / sqrt(sum L^2 sum R^2), 2 decimals.

`outside(m, targets)`: targets are `{key: (lo, hi, scale)}`. For each key: a missing value adds 1.0; a value under lo
adds min(2, (lo - v) / scale); a value over hi adds min(2, (v - hi) / scale). The sum is 0 when every target is met.

`family_space(space, params, fine)`: for each SPACE key that `params` has: keep the SPACE spec when the key is in
`fine`, else pin it to the one-element list `[params[key]]`.

## Constants

| constant | value |
|---|---|
| sample rate | 48 000 Hz |
| CLI budget / pop / keep / robust / shortlist | 160 / 16 / 4 / 3 / 2 |
| CLI pre-screen default | 3 x pop |
| Python defaults | budget 160, pop 16, keep 4, sigma0 0.22, sigma1 0.04, box 0.15, strategy es, prescreen 0, robust 0, shortlist 1, bank off |
| history candidates in the pick pool | 64 |
| family member seed | seed + 101 x k |
| family default budget in `_forge_families` | 96 (only when called directly; `forge` passes its own budget) |
| palette peak | -1 dBFS |
| audition peak / gaps | -3 dBFS / 0.6 s / 1.2 s |
| empty audition | 4800 samples of silence |
| robust shortlist pass rule | pass rate >= 0.5 first |
| WAV format | 48 kHz, PCM 24-bit; palette samples float32 before write |

## Errors and edge cases

- A recipe whose render returns nothing or silence gives a `None` render (part 7). The forge skips it in
  `measured`, holdout and WAV writing, and palette drops it with a log line.
- With no refs: no `measured`, no ref scores, no gap. The CLI prints a note.
- A missing refset registry or unknown category when the recipe has REFSET: fall through to REFS.
- A compare picture error is caught and recorded. The run still writes report.json.
- A seed bank write error is logged and ignored.
- The report drops `schema` when any pick cannot name its render function (a driver's lambda). The schema module
  then infers a block for such reports (part 6).
- `--render` without a recipe or without `--params`: usage line and exit code 2.
- A recipe that fails to import in `--list` shows `(does not load: <ErrorType>)` and the listing continues.

## Acceptance tests (loop)

L1. `recipes()` on a package with modules `a`, `_b`, `c` returns `['a', 'c']`.
L2. `family_box` on SPACE `{'f': (100, 10000, 'log'), 'g': (0, 1), 'n': (1, 5, 'int'), 'k': ['x', 'y']}` with params
    `{'f': 1000, 'g': 0.5, 'n': 3, 'k': 'x'}` and box 0.15 returns f in about [500, 2000] (log width 0.15 x ln 100 =
    0.691, so x/2 to x2), g in [0.35, 0.65], n = (2.4, 3.6, 'int'), and no `k`.
L3. `family_box` with a value at the range edge never leaves the range, and an entry with zero width is dropped.
L4. `palette` with 3 members of different loudness returns `loudness.after.spread_lu` near 0 (under 0.5 LU), and the
    loudest member peak is -1.0 dBFS within 0.01 dB.
L5. `forge(recipe, budget=32, pop=8, keep=2, out=D)` writes `<name>_01.wav`, `<name>_02.wav`, `report.json` with
    `schema` 2, and every pick has a `render` block whose re-render matches the WAV within 1e-6 (part 6 check).
L6. `forge(..., families={'a': {...}, 'b': {...}})` returns a palette report with 2 picks, `runs` of length 2, and
    members named by family.
L7. `audition` with made `[('m', x)]`, refs `[('r', y)]`, pairs `[0]` writes a WAV of length len(x) + 0.6 s + len(y) +
    1.2 s (in samples at 48 kHz), and cues at t = 0 (made) and t = len(x) / 48000 + 0.6 (ref).
L8. `hark forge --render click` with no `--params` prints the usage line and exits 2.
L9. `render_params(rec, report)` on a report written by `forge()` writes one file per pick, and each file equals the
    pick's WAV within 1e-6 after the palette gain is taken into account.
L10. `_tonal.env` with onset 0.03 and tau 0.035: the value at t = 0 is 0, at t = 0.03 is 1, and at t = 0.03 + 2.3 x
     0.035 it is about 0.1 (20 dB down).
L11. `_tonal.glide(t, 560, 3, 0.02)` at t = 0 is 560 x 2^(3/12) (about 665.9 Hz) and tends to 560 Hz.
L12. `_tonal.outside({'a': 5}, {'a': (0, 1, 2), 'b': (0, 1, 1)})` returns min(2, 4/2) + 1 = 3.0.
L13. `_tonal.finish` of a 0.2 s mono sine returns shape (n, 2), starts at 0, and its last sample is 0.
L14. `_tonal.measure` of a mono 1 kHz sine of 1 s with amplitude exp(-t / 0.05): the track is power, which decays
     as exp(-2t / tau), so the 1e-2 threshold (20 dB) is reached at tau ln(100) / 2 = 115 ms after the peak. Assert
     `t20_ms` in [105, 125], `t40_ms` in [220, 245], `attack_ms` under 10, `centroid_hz` in [900, 1100], `tonal`
     over 0.8, `corr` 1.0.
